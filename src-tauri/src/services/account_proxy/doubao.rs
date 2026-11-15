//! Doubao Work and Dola use the desktop web chat protocol, not an OpenAI API.
//! Model identifiers come from the account's action-bar directory. Every API
//! request creates its own conversation; client tool rounds travel as explicit
//! history through a text tool protocol, never through the desktop agent's tools.

use super::{json_response, protocol, providers, stream_response, ProxyError, SseParser};
use crate::models::model::AccountProvider;
use axum::{
    body::Body,
    http::{header, HeaderMap, StatusCode},
    response::Response,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use bytes::Bytes;
use futures_util::{Stream, StreamExt};
use hmac::{Hmac, Mac};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    pin::Pin,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

const CACHE_TTL: Duration = Duration::from_secs(300);
const MAX_IMAGE: usize = 20 * 1024 * 1024;
const MAX_OUTPUT: usize = 16 * 1024 * 1024;
// Native AgentMode enum: Unknown=0, MOA=1, Chat=2.
const MODE_MOA: u64 = 1;
const MODE_CHAT: u64 = 2;

#[derive(Clone)]
struct Model {
    id: String,
    name: String,
    extra: Value,
    agent_mode: u64,
    mode_id: Option<String>,
    mode_name: Option<String>,
    deep_think: i64,
}

#[derive(Clone)]
struct Catalog {
    at: Instant,
    credential: String,
    bot: String,
    models: BTreeMap<String, Model>,
}

fn cache() -> &'static Mutex<BTreeMap<String, Catalog>> {
    static CACHE: OnceLock<Mutex<BTreeMap<String, Catalog>>> = OnceLock::new();
    CACHE.get_or_init(Mutex::default)
}

fn unsupported(message: impl Into<String>) -> ProxyError {
    let mut error = ProxyError::new(StatusCode::BAD_REQUEST, message);
    error.error_code = Some("unsupported_parameter".into());
    error.model_scope = true;
    error
}

fn provider_id(provider: AccountProvider) -> Result<&'static str, ProxyError> {
    match provider {
        AccountProvider::DoubaoWork => Ok("doubaowork"),
        AccountProvider::Dola => Ok("dola"),
        _ => Err(ProxyError::upstream("Invalid Doubao account provider")),
    }
}

pub(super) fn base(provider: AccountProvider, credentials: &Value) -> Result<String, ProxyError> {
    let expected = match provider_id(provider)? {
        "doubaowork" => "https://www.doubao.com",
        _ => "https://www.dola.com",
    };
    let supplied = credentials["baseUrl"]
        .as_str()
        .unwrap_or(expected)
        .trim_end_matches('/');
    #[cfg(test)]
    if url::Url::parse(supplied)
        .is_ok_and(|url| url.scheme() == "http" && url.host_str() == Some("127.0.0.1"))
    {
        return Ok(supplied.into());
    }
    if supplied != expected {
        return Err(ProxyError::new(
            StatusCode::UNAUTHORIZED,
            "Doubao account edition does not match its endpoint",
        ));
    }
    Ok(expected.into())
}

fn client() -> Result<&'static reqwest::Client, ProxyError> {
    static CLIENT: OnceLock<Result<reqwest::Client, String>> = OnceLock::new();
    CLIENT
        .get_or_init(|| {
            reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .connect_timeout(Duration::from_secs(15))
                .build()
                .map_err(|_| "Could not create Doubao client".into())
        })
        .as_ref()
        .map_err(ProxyError::upstream)
}

fn cookie(credentials: &Value) -> Result<&str, ProxyError> {
    credentials["cookieHeader"]
        .as_str()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            ProxyError::new(
                StatusCode::UNAUTHORIZED,
                "Doubao account has no session cookies",
            )
        })
}

fn cache_key(provider: AccountProvider, credentials: &Value) -> Result<String, ProxyError> {
    let id = credentials["accountId"]
        .as_str()
        .filter(|id| !id.is_empty())
        .ok_or_else(|| {
            ProxyError::new(StatusCode::UNAUTHORIZED, "Doubao account has no identifier")
        })?;
    Ok(format!(
        "{}\0{id}\0{}",
        provider_id(provider)?,
        base(provider, credentials)?
    ))
}

fn number(value: &Value) -> Option<u64> {
    value.as_u64().or_else(|| value.as_str()?.parse().ok())
}

fn identifier(value: &Value) -> Option<String> {
    value
        .as_str()
        .map(str::to_string)
        .or_else(|| value.as_u64().map(|id| id.to_string()))
        .filter(|id| !id.trim().is_empty())
}

fn request(
    provider: AccountProvider,
    credentials: &Value,
    path: &str,
) -> Result<reqwest::RequestBuilder, ProxyError> {
    let base = base(provider, credentials)?;
    let aid = if provider == AccountProvider::Dola {
        "582465"
    } else {
        "1044603"
    };
    let mut query: Vec<(&str, String)> = vec![
        ("aid", aid.into()),
        ("real_aid", aid.into()),
        ("device_platform", "web".into()),
        ("version_code", "20800".into()),
        ("samantha_web", "1".into()),
        ("web_platform", "desktop".into()),
        ("doubao_device_platform", "desktop".into()),
        ("client_platform", "pc_client".into()),
        ("pkg_type", "release_version".into()),
        ("runtime", "web".into()),
        (
            "runtime_version",
            if provider == AccountProvider::Dola {
                "3.42.1"
            } else {
                "3.42.2"
            }
            .into(),
        ),
        ("use-olympus-account", "1".into()),
        (
            "language",
            credentials["language"]
                .as_str()
                .filter(|value| !value.is_empty())
                .unwrap_or(if provider == AccountProvider::Dola {
                    "en"
                } else {
                    "zh"
                })
                .into(),
        ),
    ];
    for (wire, field) in [
        ("device_id", "deviceId"),
        ("web_id", "webId"),
        ("pc_version", "clientVersion"),
        ("doubao_pc_version", "clientVersion"),
        ("chromium_version", "chromiumVersion"),
        ("channel", "channel"),
    ] {
        if let Some(value) = credentials[field].as_str().filter(|s| !s.is_empty()) {
            query.push((wire, value.into()));
        }
    }
    if let Some(device) = credentials["deviceId"]
        .as_str()
        .filter(|value| !value.is_empty())
    {
        query.push(("fp", format!("verify_{device}")));
    } else if let Some(fingerprint) = credentials["fingerprint"]
        .as_str()
        .filter(|value| !value.is_empty())
    {
        query.push(("fp", fingerprint.into()));
    }
    for (wire, field) in [("region", "region"), ("sys_region", "sysRegion")] {
        if let Some(value) = credentials[field]
            .as_str()
            .filter(|value| !value.is_empty())
            .or_else(|| (provider == AccountProvider::DoubaoWork).then_some("CN"))
        {
            query.push((wire, value.into()));
        }
    }
    Ok(client()?.post(format!("{base}{path}")).query(&query).header(header::COOKIE, cookie(credentials)?).header(header::ORIGIN, &base).header(header::REFERER, format!("{base}/chat/")).header("Agw-Js-Conv", "str").header(header::USER_AGENT, credentials["userAgent"].as_str().unwrap_or("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/147.0.0.0 Safari/537.36")))
}

fn api_error(
    status: StatusCode,
    headers: &HeaderMap,
    value: &Value,
    model_scope: bool,
) -> ProxyError {
    let status = if status.is_success() {
        match number(&value["status_code"]).or_else(|| number(&value["code"])) {
            Some(401) => StatusCode::UNAUTHORIZED,
            Some(403) => StatusCode::FORBIDDEN,
            Some(429) => StatusCode::TOO_MANY_REQUESTS,
            _ => StatusCode::BAD_GATEWAY,
        }
    } else {
        status
    };
    let detail = value
        .get("error_detail")
        .filter(|value| value.is_object())
        .unwrap_or(value);
    let mut error = providers::upstream_error(status, headers, detail);
    if error.error_code.is_none() {
        error.error_code = identifier(&value["code"]).or_else(|| identifier(&value["error_code"]));
    }
    error.model_scope |= model_scope && status != StatusCode::UNAUTHORIZED;
    error
}

async fn api_json(request: reqwest::RequestBuilder) -> Result<Value, ProxyError> {
    api_json_codes(request, false).await
}

async fn api_json_codes(request: reqwest::RequestBuilder, ug: bool) -> Result<Value, ProxyError> {
    let response = request
        .timeout(Duration::from_secs(30))
        .send()
        .await
        .map_err(ProxyError::upstream)?;
    let status = response.status();
    let headers = response.headers().clone();
    let value = response.json::<Value>().await;
    if !status.is_success() {
        return Err(api_error(
            status,
            &headers,
            &value.unwrap_or(Value::Null),
            false,
        ));
    }
    let value = value.map_err(ProxyError::upstream)?;
    if number(&value["code"]).is_some_and(|code| code != 0 && !(ug && code == 200)) {
        return Err(api_error(status, &headers, &value, false));
    }
    Ok(value)
}

pub(super) async fn account_json(
    provider: AccountProvider,
    credentials: &Value,
    path: &str,
    body: &Value,
) -> Result<Value, ProxyError> {
    api_json_codes(
        request(provider, credentials, path)?.json(body),
        path.starts_with("/alice/ug/"),
    )
    .await
}

fn model_row(row: &Value, mode: Option<&Value>) -> Option<Model> {
    let id = identifier(&row["model_item_key"]).or_else(|| identifier(&row["item_id"]))?;
    let name = row["name"]
        .as_str()
        .filter(|name| !name.trim().is_empty())
        .unwrap_or(&id)
        .to_string();
    let extra = match &row["model_extra_params"] {
        Value::String(text) => {
            serde_json::from_str(text).unwrap_or_else(|_| Value::String(text.clone()))
        }
        Value::Object(_) => row["model_extra_params"].clone(),
        _ => json!({}),
    };
    let agent_mode = match mode {
        Some(mode) => number(&mode["agent_mode"])?,
        None => number(&row["agent_mode"]).unwrap_or(0),
    };
    let mode_id = mode
        .and_then(|mode| identifier(&mode["mode_id"]))
        .or_else(|| identifier(&row["default_mode"]));
    let mode_name = mode
        .and_then(|mode| mode["name"].as_str())
        .filter(|name| !name.trim().is_empty())
        .map(str::to_string);
    let mut deep_think = number(&row["menu_type"])
        .or_else(|| number(&row["item_id"]))
        .or_else(|| id.parse().ok())
        .unwrap_or(0) as i64;
    if mode_id.as_deref() == Some("2") && deep_think == 4 {
        deep_think = 3;
    }
    Some(Model {
        id,
        name,
        extra,
        agent_mode,
        mode_id,
        mode_name,
        deep_think,
    })
}

fn mode_permits_model(row: &Value, mode: &Value) -> bool {
    let Some(mode_id) = identifier(&mode["mode_id"]) else {
        return false;
    };
    let id = identifier(&row["model_item_key"]).or_else(|| identifier(&row["item_id"]));
    row["conversation_mode_list"]
        .as_array()
        .is_none_or(|modes| {
            modes
                .iter()
                .any(|value| identifier(value).as_ref() == Some(&mode_id))
        })
        && mode["support_models"].as_array().is_none_or(|models| {
            models.iter().any(|value| {
                identifier(&value["model_item_key"])
                    .or_else(|| identifier(&value["item_id"]))
                    .or_else(|| identifier(value))
                    == id
            })
        })
}

fn directory(value: &Value) -> BTreeMap<String, Model> {
    let mut contexts: BTreeMap<String, Vec<Model>> = BTreeMap::new();
    let mut insert = |model: Model| {
        let variants = contexts.entry(model.id.clone()).or_default();
        if let Some(previous) = variants.iter_mut().find(|previous| {
            previous.mode_id == model.mode_id && previous.agent_mode == model.agent_mode
        }) {
            *previous = model;
        } else {
            variants.push(model);
        }
    };
    for entry in value["data"]["entry_list"].as_array().into_iter().flatten() {
        let conf = &entry["active_switch_conf"];
        if conf["use_menu_conf_v2"] != true {
            for row in conf["menu_conf"]["item_list"]
                .as_array()
                .into_iter()
                .flatten()
            {
                if let Some(model) = model_row(row, None) {
                    insert(model);
                }
            }
            continue;
        }
        let v2 = &conf["menu_conf_v2"];
        let modes = v2["mode_list"]["item_list"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_default();
        for row in v2["model_list"]["item_list"]
            .as_array()
            .into_iter()
            .flatten()
        {
            let find = |id: Option<String>| {
                id.and_then(|id| {
                    modes.iter().find(|mode| {
                        identifier(&mode["mode_id"]).as_ref() == Some(&id)
                            && mode_permits_model(row, mode)
                    })
                })
            };
            // The desktop first uses the model/server scene default, then the
            // chat scene. Its menu order is only the final compatible fallback.
            let mode = find(identifier(&row["default_mode"]))
                .or_else(|| {
                    find(identifier(
                        &v2["default_select_mode_map"]["new_chat"]["mode_id"],
                    ))
                })
                .or_else(|| {
                    modes.iter().find(|mode| {
                        number(&mode["agent_mode"]) == Some(MODE_CHAT)
                            && mode_permits_model(row, mode)
                    })
                })
                .or_else(|| modes.iter().find(|mode| mode_permits_model(row, mode)));
            if !modes.is_empty() && mode.is_none() {
                continue;
            }
            if let Some(model) = model_row(row, mode) {
                insert(model);
            }
        }
        // Mode-local metadata takes precedence over a global row for that same
        // context, matching the desktop's local-list-before-global-list lookup.
        for mode in modes {
            for row in mode["model_list"]["item_list"]
                .as_array()
                .into_iter()
                .flatten()
            {
                if let Some(model) = model_row(row, Some(mode)) {
                    insert(model);
                }
            }
        }
    }
    let mut result = BTreeMap::new();
    for (id, variants) in contexts {
        let scoped = variants.len() > 1;
        for mut model in variants {
            let key = if scoped {
                let mode_id = model
                    .mode_id
                    .clone()
                    .unwrap_or_else(|| format!("agent-{}", model.agent_mode));
                let mode_name = model.mode_name.as_deref().unwrap_or(&mode_id);
                model.name = format!("{} · {mode_name}", model.name);
                format!("{}::{}", encode(&mode_id), encode(&id))
            } else {
                id.clone()
            };
            result.insert(key, model);
        }
    }
    result
}

fn aliases(models: &BTreeMap<String, Model>) -> BTreeMap<String, String> {
    models
        .iter()
        .map(|(id, model)| {
            let unique = models.values().filter(|row| row.name == model.name).count() == 1
                && (!models.contains_key(&model.name) || model.name == *id);
            (
                if unique {
                    model.name.clone()
                } else {
                    id.clone()
                },
                id.clone(),
            )
        })
        .collect()
}

pub(super) fn cached_model_labels(
    provider: AccountProvider,
    account_id: &str,
) -> BTreeMap<String, String> {
    let Ok(provider) = provider_id(provider) else {
        return BTreeMap::new();
    };
    let prefix = format!("{provider}\0{account_id}\0");
    cache()
        .lock()
        .ok()
        .and_then(|cache| {
            cache
                .iter()
                .find(|(key, _)| key.starts_with(&prefix))
                .map(|(_, catalog)| {
                    aliases(&catalog.models)
                        .into_iter()
                        .filter(|(alias, id)| alias != id)
                        .map(|(alias, id)| (id, alias))
                        .collect()
                })
        })
        .unwrap_or_default()
}

pub(super) async fn list_models(
    provider: AccountProvider,
    credentials: Value,
    _client: &reqwest::Client,
) -> Result<Vec<String>, ProxyError> {
    let launch =
        api_json(request(provider, &credentials, "/alice/user/launch")?.json(&json!({}))).await?;
    let bot = identifier(&launch["data"]["assistant_bot_id"])
        .or_else(|| identifier(&launch["assistant_bot_id"]))
        .ok_or_else(|| {
            ProxyError::upstream("Doubao launch response has no assistant identifier")
        })?;
    let value = api_json(request(provider, &credentials, "/alice/slot/action_bar_v3/brief_list")?.json(&json!({"language_code":if provider == AccountProvider::Dola { "en" } else { "zh" }, "bot_id":bot}))).await?;
    let models = directory(&value);
    if models.is_empty() {
        return Err(ProxyError::upstream(
            "Doubao account returned no model directory",
        ));
    }
    let names = aliases(&models).into_keys().collect();
    let key = cache_key(provider, &credentials)?;
    let catalog = Catalog {
        at: Instant::now(),
        credential: hex::encode(Sha256::digest(cookie(&credentials)?)),
        bot,
        models,
    };
    let mut cache = cache().lock().map_err(ProxyError::upstream)?;
    cache.retain(|_, catalog| catalog.at.elapsed() < CACHE_TTL);
    cache.insert(key, catalog);
    Ok(names)
}

async fn resolve(
    provider: AccountProvider,
    credentials: &Value,
    model: &str,
    client: &reqwest::Client,
) -> Result<(String, Model), ProxyError> {
    let key = cache_key(provider, credentials)?;
    let credential = hex::encode(Sha256::digest(cookie(credentials)?));
    let get = || {
        cache().lock().ok().and_then(|cache| {
            cache
                .get(&key)
                .filter(|catalog| {
                    catalog.at.elapsed() < CACHE_TTL && catalog.credential == credential
                })
                .cloned()
        })
    };
    let catalog = if let Some(catalog) = get() {
        catalog
    } else {
        list_models(provider, credentials.clone(), client).await?;
        get().ok_or_else(|| ProxyError::upstream("Doubao model directory was replaced"))?
    };
    let selected = catalog
        .models
        .get(model)
        .or_else(|| {
            aliases(&catalog.models)
                .get(model)
                .and_then(|id| catalog.models.get(id))
        })
        .cloned()
        .ok_or_else(|| {
            let mut error = unsupported("This model is absent from the Doubao account directory");
            error.error_code = Some("model_not_found".into());
            error
        })?;
    Ok((catalog.bot, selected))
}

struct ToolBridge {
    marker: String,
    names: BTreeSet<String>,
    forced: bool,
    parallel: bool,
    declarations: Value,
}

impl ToolBridge {
    fn new(body: &Value) -> Result<Self, ProxyError> {
        let rows = body
            .get("tools")
            .filter(|v| !v.is_null())
            .map(|v| {
                v.as_array()
                    .ok_or_else(|| unsupported("Invalid Doubao tools"))
            })
            .transpose()?
            .cloned()
            .unwrap_or_default();
        let mut names = BTreeSet::new();
        for row in &rows {
            let name = row["function"]["name"]
                .as_str()
                .filter(|s| !s.is_empty())
                .ok_or_else(|| unsupported("Doubao function name is required"))?;
            if row["type"] != "function" || !names.insert(name.to_string()) {
                return Err(unsupported(
                    "Doubao requires uniquely named client function tools",
                ));
            }
        }
        let choice = body.get("tool_choice").filter(|v| !v.is_null());
        let forced = match choice.and_then(Value::as_str) {
            None if choice.is_none() => false,
            Some("auto") => false,
            Some("none") => {
                names.clear();
                false
            }
            Some("required") if !names.is_empty() => true,
            None if choice.is_some_and(Value::is_object) => {
                let name = choice.expect("choice")["function"]["name"]
                    .as_str()
                    .unwrap_or_default();
                if choice.expect("choice")["type"] != "function" || !names.contains(name) {
                    return Err(unsupported(
                        "Doubao named tool choice must select a declared function",
                    ));
                }
                names.retain(|item| item == name);
                true
            }
            _ => return Err(unsupported("Invalid Doubao tool choice")),
        };
        Ok(Self {
            marker: format!("echobird_tools_{}", uuid::Uuid::new_v4().simple()),
            names,
            forced,
            parallel: body["parallel_tool_calls"] != false,
            declarations: json!(rows),
        })
    }

    fn instruction(&self) -> String {
        if self.names.is_empty() {
            return String::new();
        }
        format!("The following JSON conversation contains the complete client history, including tool call IDs and tool results. Follow its system/developer instructions. Client-side tools are available; do not run substitute desktop tools. To request tools, return ONLY a JSON object with key {} and an array of objects {{\"name\":\"declared name\",\"arguments\":{{...}}}}. Allowed names: {}. Tool definitions: {}. A tool result is provided in the next request. Otherwise return your answer as text. Tool use is {}. Parallel tool use is {}.\n", serde_json::to_string(&self.marker).unwrap_or_default(), json!(self.names), self.declarations, if self.forced { "required" } else { "optional" }, self.parallel)
    }

    fn message(&self, text: &str) -> Result<Value, ProxyError> {
        let parsed = serde_json::from_str::<Value>(text.trim()).ok();
        let calls = parsed.as_ref().and_then(|value| value.get(&self.marker));
        if calls.is_none() && text.contains(&self.marker) {
            return Err(ProxyError::upstream(
                "Doubao returned an incomplete client tool response",
            ));
        }
        if let Some(calls) = calls {
            let calls = calls
                .as_array()
                .filter(|calls| !calls.is_empty())
                .ok_or_else(|| ProxyError::upstream("Doubao returned invalid tool calls"))?;
            if !self.parallel && calls.len() > 1 {
                return Err(ProxyError::upstream(
                    "Doubao did not honor parallel_tool_calls=false",
                ));
            }
            let mut normalized = Vec::new();
            for call in calls {
                let name = call["name"].as_str().unwrap_or_default();
                if !self.names.contains(name) || !call["arguments"].is_object() {
                    return Err(ProxyError::upstream(
                        "Doubao returned an undeclared tool or invalid arguments",
                    ));
                }
                normalized.push(json!({"id":format!("call_{}",uuid::Uuid::new_v4().simple()),"type":"function","function":{"name":name,"arguments":call["arguments"].to_string()}}));
            }
            return Ok(json!({"role":"assistant","content":null,"tool_calls":normalized}));
        }
        if self.forced {
            return Err(ProxyError::upstream(
                "Doubao did not honor the required client tool choice",
            ));
        }
        Ok(json!({"role":"assistant","content":text}))
    }
}

fn content(body: &Value, bridge: &ToolBridge) -> Result<(String, Vec<String>), ProxyError> {
    protocol::reject_structured_output(body)?;
    let messages = body["messages"]
        .as_array()
        .filter(|messages| !messages.is_empty())
        .ok_or_else(|| unsupported("Doubao messages are required"))?;
    let mut history = messages.clone();
    let mut images = Vec::new();
    for message in &mut history {
        if !matches!(
            message["role"].as_str(),
            Some("system" | "developer" | "user" | "assistant" | "tool")
        ) {
            return Err(unsupported("Unsupported Doubao message role"));
        }
        if let Some(parts) = message["content"].as_array_mut() {
            for part in parts {
                match part["type"].as_str() {
                    Some("text") if part["text"].is_string() => {}
                    Some("image_url") => {
                        let url = part["image_url"]["url"]
                            .as_str()
                            .ok_or_else(|| unsupported("Doubao image URL is required"))?;
                        if !url.starts_with("data:image/") {
                            return Err(unsupported(
                                "Doubao image input currently requires a base64 image data URL",
                            ));
                        }
                        if images.len() >= 10 {
                            return Err(unsupported("Too many Doubao input images"));
                        }
                        images.push(url.into());
                        *part = json!({"type":"text","text":format!("[Attached image {}]",images.len())});
                    }
                    _ => return Err(unsupported("Unsupported Doubao message content block")),
                }
            }
        } else if !message["content"].is_string() && !message["content"].is_null() {
            return Err(unsupported("Unsupported Doubao message content"));
        }
    }
    let mut prompt = bridge.instruction();
    prompt.push_str(&json!(history).to_string());
    Ok((prompt, images))
}

fn text_block(text: &str) -> Value {
    json!({"block_type":10000,"content":{"text_block":{"text":text,"icon_url":"","icon_url_dark":"","summary":""},"pc_event_block":""},"block_id":uuid::Uuid::new_v4().to_string(),"parent_id":"","meta_info":[],"append_fields":[]})
}

fn payload(
    bot: &str,
    model: &Model,
    prompt: &str,
    attachments: Vec<Value>,
    body: &Value,
) -> Result<Value, ProxyError> {
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp_millis();
    let mut blocks = Vec::new();
    if !attachments.is_empty() {
        blocks.push(json!({"block_type":10052,"content":{"attachment_block":{"attachments":attachments},"pc_event_block":""},"block_id":uuid::Uuid::new_v4().to_string(),"parent_id":"","meta_info":[],"append_fields":[]}));
    }
    blocks.push(text_block(prompt));
    let mut result = json!({"client_meta":{"local_conversation_id":format!("local_{}",uuid::Uuid::new_v4().as_u128()%10u128.pow(16)),"conversation_id":"","bot_id":bot,"last_section_id":"","last_message_index":null},"messages":[{"local_message_id":id,"content_block":blocks,"message_status":0}],"option":{"create_time_ms":now,"need_create_conversation":true,"conversation_init_option":{"need_ack_conversation":true},"need_deep_think":model.deep_think,"is_audio":false,"answer_with_suggest":false,"tts_switch":false,"is_regen":false,"is_replace":false,"unique_key":uuid::Uuid::new_v4().to_string(),"start_seq":0,"sse_recv_event_options":{"support_chunk_delta":true},"recovery_option":{"is_recovery":false,"req_create_time_sec":now/1000,"append_sse_event_scene":0},"message_storage_type":0},"user_context":[],"ext":{"use_deep_think":model.deep_think.to_string(),"sub_conv_firstmet_type":"1","commerce_credit_config_enable":"0"}});
    if model.agent_mode != 0 || model.mode_id.is_some() {
        let effort = match body["reasoning_effort"].as_str() {
            None => None,
            Some("low") => Some(3),
            Some("medium") => Some(4),
            Some("high") => Some(5),
            Some("xhigh") => Some(6),
            Some("max") => Some(7),
            Some(_) => return Err(unsupported("Unsupported Doubao reasoning effort")),
        };
        let task = json!({"action":0,"thread_local_message_id":[id],"client_option":{"enable_sandbox":false,"os":std::env::consts::OS,"shared_folder_path":[]},"runtime_type":2,"agent_task_param":{"runtime_type":2},"agent_task_param_change":{"runtime_changed":true,"device_changed":false,"sandbox_auth_type_changed":false},"need_modify_conversation":false,"task_input_json":"{\"project_context\":{}}"});
        result["option"]["agent_mode"] = json!(model.agent_mode);
        result["option"]["model_config"] =
            json!({"model_item_key":model.id,"model_extra_params":model.extra});
        result["option"]["aggregate_params"] = json!({"mode_id":model.mode_id.as_deref().unwrap_or("3"),"model_item_key":model.id,"agent_mode":model.agent_mode.to_string()});
        if let Some(provider) = model.extra["provider_id"].as_str() {
            result["option"]["aggregate_params"]["provider_id"] = json!(provider);
        }
        if let Some(effort) = effort {
            result["option"]["model_config"]["reasoning_effort"] = json!(effort);
            result["option"]["aggregate_params"]["reasoning_effort"] = json!(effort.to_string());
        }
        result["ext"]["agent_mode"] = json!(model.agent_mode.to_string());
        if matches!(model.agent_mode, MODE_MOA | MODE_CHAT) {
            result["option"]["general_task_param"] = task.clone();
            result["ext"]["general_task_param"] = json!(task.to_string());
            result["ext"]["is_finish"] = json!("1");
        }
    } else if !body["reasoning_effort"].is_null() {
        return Err(unsupported(
            "This Doubao mode has no reasoning-effort selector",
        ));
    }
    Ok(result)
}

fn crc32(bytes: &[u8]) -> String {
    let mut crc = flate2::Crc::new();
    crc.update(bytes);
    format!("{:08x}", crc.sum())
}

fn hmac(key: &[u8], value: &str) -> Vec<u8> {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("HMAC accepts any key length");
    mac.update(value.as_bytes());
    mac.finalize().into_bytes().to_vec()
}

fn encode(value: &str) -> String {
    value
        .bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
                char::from(byte).to_string()
            } else {
                format!("%{byte:02X}")
            }
        })
        .collect()
}

fn image_sign(
    auth: &Value,
    method: &str,
    host: &str,
    query: &BTreeMap<&str, String>,
    body: &str,
    region: &str,
    time: &str,
) -> Result<(String, String), ProxyError> {
    let required = |field| {
        auth[field]
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| ProxyError::upstream("Incomplete Doubao image upload credentials"))
    };
    let access = required("access_key")?;
    let secret = required("secret_key")?;
    let token = required("session_token")?;
    let date = &time[..8];
    let query = query
        .iter()
        .map(|(key, value)| format!("{}={}", encode(key), encode(value)))
        .collect::<Vec<_>>()
        .join("&");
    let headers = format!("host:{host}\nx-amz-date:{time}\nx-amz-security-token:{token}\n");
    let names = "host;x-amz-date;x-amz-security-token";
    let canonical = format!(
        "{method}\n/\n{query}\n{headers}\n{names}\n{}",
        hex::encode(Sha256::digest(body))
    );
    let scope = format!("{date}/{region}/imagex/aws4_request");
    let string = format!(
        "AWS4-HMAC-SHA256\n{time}\n{scope}\n{}",
        hex::encode(Sha256::digest(canonical))
    );
    let key = hmac(
        &hmac(
            &hmac(&hmac(format!("AWS4{secret}").as_bytes(), date), region),
            "imagex",
        ),
        "aws4_request",
    );
    Ok((
        query,
        format!(
            "AWS4-HMAC-SHA256 Credential={access}/{scope}, SignedHeaders={names}, Signature={}",
            hex::encode(hmac(&key, &string))
        ),
    ))
}

fn upload_base(host: &str) -> Result<String, ProxyError> {
    #[cfg(test)]
    if url::Url::parse(host)
        .is_ok_and(|url| url.scheme() == "http" && url.host_str() == Some("127.0.0.1"))
    {
        return Ok(host.trim_end_matches('/').into());
    }
    let url = url::Url::parse(&format!("https://{host}/")).map_err(ProxyError::upstream)?;
    if url.host_str() != Some(host)
        || url.port().is_some()
        || ![
            ".bytedanceapi.com",
            ".bytevcloudapi.com",
            ".volcengineapi.com",
            ".byteimg.com",
            ".ibyteimg.com",
            ".ibytedtos.com",
            ".imagex.cn",
            ".douyinpic.com",
        ]
        .iter()
        .any(|suffix| host.ends_with(suffix))
    {
        return Err(ProxyError::upstream("Invalid Doubao image upload host"));
    }
    Ok(format!("https://{host}"))
}

async fn upload_image(
    provider: AccountProvider,
    credentials: &Value,
    data: &str,
) -> Result<Value, ProxyError> {
    let (prefix, encoded) = data
        .split_once(',')
        .ok_or_else(|| unsupported("Invalid Doubao image data URL"))?;
    if !prefix.starts_with("data:image/")
        || !prefix.ends_with(";base64")
        || encoded.len() > MAX_IMAGE * 4 / 3 + 4
    {
        return Err(unsupported("Invalid or oversized Doubao image data URL"));
    }
    let bytes = STANDARD
        .decode(encoded)
        .map_err(|_| unsupported("Invalid Doubao base64 image"))?;
    if bytes.len() > MAX_IMAGE {
        return Err(unsupported("Doubao input image is too large"));
    }
    let format = image::guess_format(&bytes).map_err(|_| unsupported("Invalid Doubao image"))?;
    let extension = match format {
        image::ImageFormat::Png => ".png",
        image::ImageFormat::Jpeg => ".jpg",
        image::ImageFormat::WebP => ".webp",
        image::ImageFormat::Gif => ".gif",
        _ => return Err(unsupported("Unsupported Doubao image format")),
    };
    let (width, height) = image::ImageReader::new(std::io::Cursor::new(&bytes))
        .with_guessed_format()
        .map_err(|_| unsupported("Invalid Doubao image"))?
        .into_dimensions()
        .map_err(|_| unsupported("Invalid Doubao image dimensions"))?;
    let prepared = api_json(
        request(provider, credentials, "/alice/resource/prepare_upload")?
            .json(&json!({"tenant_id":"5","scene_id":"4","resource_type":2})),
    )
    .await?;
    let prep = &prepared["data"];
    let host = prep["upload_host"]
        .as_str()
        .ok_or_else(|| ProxyError::upstream("Doubao image upload host is missing"))?;
    let endpoint = upload_base(host)?;
    let auth = &prep["upload_auth_token"];
    let token = auth["session_token"]
        .as_str()
        .ok_or_else(|| ProxyError::upstream("Doubao image upload token is missing"))?;
    let service = prep["service_id"]
        .as_str()
        .ok_or_else(|| ProxyError::upstream("Doubao image upload service is missing"))?;
    let region = prep["region"]
        .as_str()
        .unwrap_or(if provider == AccountProvider::Dola {
            "us-east-1"
        } else {
            "cn-north-1"
        });
    let time = chrono::Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
    let query = BTreeMap::from([
        ("Action", "ApplyImageUpload".into()),
        ("Version", "2018-08-01".into()),
        ("ServiceId", service.into()),
        ("FileSize", bytes.len().to_string()),
        ("FileExtension", extension.into()),
    ]);
    let (query, signature) = image_sign(auth, "GET", host, &query, "", region, &time)?;
    let applied = api_json(
        client()?
            .get(format!("{endpoint}/?{query}"))
            .header("x-amz-date", &time)
            .header("x-amz-security-token", token)
            .header(header::AUTHORIZATION, signature),
    )
    .await?;
    let address = &applied["Result"]["UploadAddress"];
    let store = &address["StoreInfos"][0];
    let upload_host = address["UploadHosts"][0]
        .as_str()
        .ok_or_else(|| ProxyError::upstream("Doubao image upload address is missing"))?;
    let uri = store["StoreUri"]
        .as_str()
        .ok_or_else(|| ProxyError::upstream("Doubao image upload URI is missing"))?;
    if uri.starts_with('/')
        || uri.split('/').any(|segment| segment == "..")
        || uri.contains(['?', '#'])
    {
        return Err(ProxyError::upstream("Invalid Doubao image upload URI"));
    }
    let authorization = store["Auth"]
        .as_str()
        .ok_or_else(|| ProxyError::upstream("Doubao image upload authorization is missing"))?;
    let response = client()?
        .post(format!("{}/upload/v1/{uri}", upload_base(upload_host)?))
        .header(header::AUTHORIZATION, authorization)
        .header("Content-CRC32", crc32(&bytes))
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .body(bytes)
        .timeout(Duration::from_secs(60))
        .send()
        .await
        .map_err(ProxyError::upstream)?;
    let status = response.status();
    let headers = response.headers().clone();
    let uploaded = response
        .json::<Value>()
        .await
        .map_err(ProxyError::upstream)?;
    if !status.is_success() || number(&uploaded["code"]) != Some(2000) {
        return Err(api_error(status, &headers, &uploaded, true));
    }
    let session = address["SessionKey"]
        .as_str()
        .ok_or_else(|| ProxyError::upstream("Doubao image upload session is missing"))?;
    let body = json!({"SessionKey":session}).to_string();
    let query = BTreeMap::from([
        ("Action", "CommitImageUpload".into()),
        ("Version", "2018-08-01".into()),
        ("ServiceId", service.into()),
    ]);
    let (query, signature) = image_sign(auth, "POST", host, &query, &body, region, &time)?;
    let committed = api_json(
        client()?
            .post(format!("{endpoint}/?{query}"))
            .header("x-amz-date", &time)
            .header("x-amz-security-token", token)
            .header(header::AUTHORIZATION, signature)
            .header(header::CONTENT_TYPE, "application/json")
            .body(body),
    )
    .await?;
    let result = &committed["Result"]["Results"][0];
    if number(&result["UriStatus"]) != Some(2000) {
        return Err(ProxyError::upstream(
            "Doubao image upload was not committed",
        ));
    }
    let uri = result["Uri"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| ProxyError::upstream("Doubao uploaded image has no URI"))?;
    Ok(
        json!({"type":1,"identifier":uuid::Uuid::new_v4().to_string(),"image":{"name":format!("image{extension}"),"uri":uri,"image_ori":{"url":"","width":width,"height":height,"format":"","url_formats":{}}},"parse_state":0,"review_state":1,"upload_status":1,"progress":100,"src":""}),
    )
}

#[derive(Default)]
struct Decoder {
    text: String,
    reasoning: String,
    reasoning_delta: String,
    reasoning_blocks: BTreeSet<String>,
    blocks: BTreeMap<String, String>,
    done: bool,
    received: usize,
}

impl Decoder {
    fn blocks(&mut self, value: &Value, append: bool) -> Result<String, ProxyError> {
        let mut delta = String::new();
        for block in value.as_array().into_iter().flatten() {
            if number(&block["block_type"]) == Some(10040) {
                continue;
            }
            if number(&block["block_type"]) != Some(10000) {
                continue;
            }
            let Some(text) = block["content"]["text_block"]["text"].as_str() else {
                continue;
            };
            let id = identifier(&block["block_id"]);
            let icons = ["icon_url", "icon_url_dark"];
            let tagged = icons.iter().any(|key| {
                block["content"]["text_block"][key]
                    .as_str()
                    .is_some_and(|icon| icon.to_ascii_lowercase().contains("deep_think"))
            });
            if let Some(id) = id.as_ref() {
                if tagged {
                    self.reasoning_blocks.insert(id.clone());
                } else if icons
                    .iter()
                    .any(|key| block["content"]["text_block"].get(key).is_some())
                {
                    self.reasoning_blocks.remove(id);
                }
            }
            if tagged
                || id
                    .as_ref()
                    .is_some_and(|id| self.reasoning_blocks.contains(id))
            {
                self.reasoning.push_str(text);
                self.reasoning_delta.push_str(text);
                if self.reasoning.len() > MAX_OUTPUT {
                    return Err(ProxyError::upstream("Doubao reasoning output is too large"));
                }
                continue;
            }
            let id = id.unwrap_or_else(|| "default".into());
            let prior = self.blocks.entry(id).or_default();
            if append {
                prior.push_str(text);
                delta.push_str(text);
            } else if let Some(suffix) = text.strip_prefix(prior.as_str()) {
                delta.push_str(suffix);
                *prior = text.into();
            } else if prior.is_empty() {
                delta.push_str(text);
                *prior = text.into();
            } else {
                return Err(ProxyError::upstream(
                    "Doubao replaced an already emitted answer",
                ));
            }
        }
        self.text.push_str(&delta);
        if self.text.len() > MAX_OUTPUT {
            return Err(ProxyError::upstream("Doubao answer is too large"));
        }
        Ok(delta)
    }

    fn frame(&mut self, event: &str, raw: &str, headers: &HeaderMap) -> Result<String, ProxyError> {
        self.received = self.received.saturating_add(raw.len());
        if self.received > MAX_OUTPUT * 4 {
            return Err(ProxyError::upstream("Doubao stream is too large"));
        }
        if let Some(mut error) = providers::stream_error(event, raw, headers) {
            error.model_scope |= error.status != StatusCode::UNAUTHORIZED;
            return Err(error);
        }
        let value = serde_json::from_str::<Value>(raw)
            .map_err(|_| ProxyError::upstream("Invalid Doubao stream event"))?;
        if event.contains("ERR")
            || event == "gateway-error"
            || number(&value["code"]).is_some_and(|code| code != 0)
        {
            return Err(api_error(StatusCode::OK, headers, &value, true));
        }
        if self.done {
            return Ok(String::new());
        }
        match event {
            "SSE_REPLY_END" => {
                self.done = true;
                Ok(String::new())
            }
            "STREAM_MSG_NOTIFY" => self.blocks(&value["content"]["content_block"], true),
            "CHUNK_DELTA" if value["text"].is_string() => self.blocks(&json!([{"block_id":"delta","block_type":10000,"content":{"text_block":{"text":value["text"]}}}]), true),
            "FULL_MSG_NOTIFY"
                if self.text.is_empty() && number(&value["message"]["user_type"]) == Some(2) =>
            {
                self.blocks(&value["message"]["content_block"], false)
            }
            "STREAM_CHUNK" => {
                let mut text = String::new();
                for patch in value["patch_op"].as_array().into_iter().flatten() {
                    if number(&patch["patch_object"]) == Some(1) {
                        text.push_str(&self.blocks(
                            &patch["patch_value"]["content_block"],
                            number(&patch["patch_type"]) == Some(1),
                        )?);
                    }
                }
                Ok(text)
            }
            _ => Ok(String::new()),
        }
    }
}

struct Turn {
    source: Pin<Box<dyn Stream<Item = Result<Bytes, reqwest::Error>> + Send>>,
    parser: SseParser,
    pending: VecDeque<(String, String)>,
    headers: HeaderMap,
    decoder: Decoder,
    received: usize,
    deltas: VecDeque<Delta>,
}

enum Delta {
    Text(String),
    Reasoning(String),
}

impl Turn {
    async fn next(&mut self) -> Result<Option<Delta>, ProxyError> {
        loop {
            if let Some(delta) = self.deltas.pop_front() {
                return Ok(Some(delta));
            }
            if let Some((event, data)) = self.pending.pop_front() {
                let text = self.decoder.frame(&event, &data, &self.headers)?;
                if !self.decoder.reasoning_delta.is_empty() {
                    self.deltas.push_back(Delta::Reasoning(std::mem::take(
                        &mut self.decoder.reasoning_delta,
                    )));
                }
                if !text.is_empty() {
                    self.deltas.push_back(Delta::Text(text));
                }
                continue;
            }
            if self.decoder.done {
                return Ok(None);
            }
            let chunk = tokio::time::timeout(Duration::from_secs(120), self.source.next())
                .await
                .map_err(|_| {
                    ProxyError::new(StatusCode::GATEWAY_TIMEOUT, "Doubao stream timed out")
                })?;
            match chunk {
                Some(Ok(bytes)) => {
                    self.received = self.received.saturating_add(bytes.len());
                    if self.received > MAX_OUTPUT * 4 {
                        return Err(ProxyError::upstream("Doubao stream frame is too large"));
                    }
                    self.pending.extend(self.parser.push(&bytes));
                }
                Some(Err(error)) => return Err(ProxyError::upstream(error)),
                None => {
                    self.pending.extend(self.parser.finish());
                    if self.pending.is_empty() {
                        return Err(ProxyError::upstream(
                            "Doubao stream ended before SSE_REPLY_END",
                        ));
                    }
                }
            }
        }
    }
}

fn chat_chunk(id: &str, model: &str, delta: Value, reason: Value) -> Bytes {
    protocol::chat_sse(
        json!({"id":id,"object":"chat.completion.chunk","created":chrono::Utc::now().timestamp(),"model":model,"choices":[{"index":0,"delta":delta,"finish_reason":reason}]}),
    )
}

pub(super) async fn route(
    provider: AccountProvider,
    credentials: Value,
    model: &str,
    body: Value,
    client: &reqwest::Client,
) -> Result<Response, ProxyError> {
    let bridge = ToolBridge::new(&body)?;
    let (prompt, images) = content(&body, &bridge)?;
    let (bot, selected) = resolve(provider, &credentials, model, client).await?;
    let mut attachments = Vec::new();
    for image in images {
        attachments.push(upload_image(provider, &credentials, &image).await?);
    }
    let payload = payload(&bot, &selected, &prompt, attachments, &body)?;
    let upstream = request(provider, &credentials, "/chat/completion")?
        .json(&payload)
        .send()
        .await
        .map_err(ProxyError::upstream)?;
    let status = upstream.status();
    let headers = upstream.headers().clone();
    if !status.is_success() {
        let value = upstream.json::<Value>().await.unwrap_or(Value::Null);
        return Err(api_error(status, &headers, &value, true));
    }
    if !headers
        .get(header::CONTENT_TYPE)
        .and_then(|h| h.to_str().ok())
        .is_some_and(|value| value.starts_with("text/event-stream"))
    {
        let value = upstream.json::<Value>().await.unwrap_or(Value::Null);
        return Err(api_error(status, &headers, &value, true));
    }
    let mut turn = Turn {
        source: Box::pin(upstream.bytes_stream()),
        parser: SseParser::default(),
        pending: VecDeque::new(),
        headers,
        decoder: Decoder::default(),
        received: 0,
        deltas: VecDeque::new(),
    };
    let id = format!("chatcmpl_{}", uuid::Uuid::new_v4().simple());
    if body["stream"] != true || !bridge.names.is_empty() {
        while turn.next().await?.is_some() {}
        if turn.decoder.text.is_empty() {
            return Err(ProxyError::upstream("Doubao completed without an answer"));
        }
        let mut message = bridge.message(&turn.decoder.text)?;
        if !turn.decoder.reasoning.is_empty() {
            message["reasoning_content"] = json!(turn.decoder.reasoning);
        }
        let reason = if message["tool_calls"].is_array() {
            "tool_calls"
        } else {
            "stop"
        };
        if body["stream"] == true {
            if let Some(calls) = message["tool_calls"].as_array_mut() {
                for (index, call) in calls.iter_mut().enumerate() {
                    call["index"] = json!(index);
                }
            }
            let frames = [
                chat_chunk(&id, model, message, Value::Null),
                chat_chunk(&id, model, json!({}), json!(reason)),
                Bytes::from_static(b"data: [DONE]\n\n"),
            ];
            return Ok(stream_response(Body::from_stream(
                futures_util::stream::iter(frames.into_iter().map(Ok::<_, std::io::Error>)),
            )));
        }
        return Ok(json_response(
            &json!({"id":id,"object":"chat.completion","created":chrono::Utc::now().timestamp(),"model":model,"choices":[{"index":0,"message":message,"finish_reason":reason}]}),
        ));
    }
    let first = tokio::time::timeout(Duration::from_secs(120), turn.next())
        .await
        .map_err(|_| {
            ProxyError::new(
                StatusCode::GATEWAY_TIMEOUT,
                "Doubao did not start an answer",
            )
        })??;
    if let Some(first) = first {
        turn.deltas.push_front(first);
    } else {
        return Err(ProxyError::upstream("Doubao completed without an answer"));
    }
    struct Output {
        turn: Turn,
        id: String,
        model: String,
        queued: VecDeque<Bytes>,
        finished: bool,
    }
    let output = Output {
        turn,
        id,
        model: model.into(),
        queued: VecDeque::new(),
        finished: false,
    };
    let stream = futures_util::stream::try_unfold(output, |mut output| async move {
        loop {
            if let Some(bytes) = output.queued.pop_front() {
                return Ok::<_, std::io::Error>(Some((bytes, output)));
            }
            if output.finished {
                return Ok(None);
            }
            match output.turn.next().await.map_err(std::io::Error::other)? {
                Some(Delta::Text(text)) => {
                    output.queued.push_back(chat_chunk(
                        &output.id,
                        &output.model,
                        json!({"role":"assistant","content":text}),
                        Value::Null,
                    ));
                }
                Some(Delta::Reasoning(reasoning)) => {
                    output.queued.push_back(chat_chunk(
                        &output.id,
                        &output.model,
                        json!({"reasoning_content":reasoning}),
                        Value::Null,
                    ));
                }
                None => {
                    if output.turn.decoder.text.is_empty() {
                        return Err(std::io::Error::other(ProxyError::upstream(
                            "Doubao completed without an answer",
                        )));
                    }
                    output.queued.push_back(chat_chunk(
                        &output.id,
                        &output.model,
                        json!({}),
                        json!("stop"),
                    ));
                    output
                        .queued
                        .push_back(Bytes::from_static(b"data: [DONE]\n\n"));
                    output.finished = true;
                }
            }
        }
    });
    Ok(stream_response(Body::from_stream(stream)))
}

#[cfg(test)]
mod tests;
