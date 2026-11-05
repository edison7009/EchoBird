mod codec;
mod stream;
mod text_calls;

use super::{protocol, stream_response, ProxyError};
use crate::{models::model::AccountProvider, services::qoder_accounts::Edition};
use axum::{
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::Response,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Mutex, OnceLock},
    time::Duration,
};

const MODELS_PATH: &str = "/algo/api/v2/model/list?Encode=1";
const CHAT_PATH:&str="/algo/api/v2/service/pro/sse/agent_chat_generation?FetchKeys=llm_model_result&AgentId=agent_common&Encode=1";

pub(super) fn speech_headers(c: &Value, path: &str) -> Result<HeaderMap, ProxyError> {
    codec::speech_headers(c, path)
}

type Catalog = BTreeMap<String, Value>;
#[derive(Default, Serialize, Deserialize)]
struct Cache(BTreeMap<String, Catalog>);
fn cache() -> &'static Mutex<Cache> {
    static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
    CACHE.get_or_init(|| {
        #[cfg(not(test))]
        let value = crate::services::cursor_auth::read(&cache_path()).unwrap_or_default();
        #[cfg(test)]
        let value = Cache::default();
        Mutex::new(value)
    })
}
#[cfg(not(test))]
fn cache_path() -> std::path::PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join(".echobird/cache/qoder-models.json")
}
fn edition(provider: AccountProvider, c: &Value) -> Result<Edition, ProxyError> {
    let edition = match provider {
        AccountProvider::Qoder => Edition::Qoder,
        AccountProvider::QoderCn => Edition::QoderCn,
        _ => return Err(ProxyError::upstream("Invalid Qoder provider")),
    };
    if c["edition"] != serde_json::to_value(edition).expect("edition serialization") {
        return Err(ProxyError::new(
            StatusCode::UNAUTHORIZED,
            "Qoder account region does not match the route",
        ));
    }
    Ok(edition)
}
fn base(edition: Edition, c: &Value) -> Result<String, ProxyError> {
    #[cfg(test)]
    if let Some(base) = c["baseUrl"].as_str() {
        let url = url::Url::parse(base).map_err(ProxyError::upstream)?;
        if url.scheme() == "http"
            && matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "[::1]"))
        {
            return Ok(base.trim_end_matches('/').into());
        }
        return Err(ProxyError::upstream("Fixture endpoint must be loopback"));
    }
    let _ = c;
    Ok(edition.gateway().into())
}
fn key(edition: Edition, c: &Value, base: &str) -> Result<String, ProxyError> {
    let user = c["userId"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            ProxyError::new(
                StatusCode::UNAUTHORIZED,
                "Qoder account has no user identifier",
            )
        })?;
    Ok(format!("{}\0{user}\0{base}", edition.tool()))
}
fn http_client() -> Result<&'static reqwest::Client, ProxyError> {
    static CLIENT: OnceLock<Result<reqwest::Client, String>> = OnceLock::new();
    CLIENT
        .get_or_init(|| {
            reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .connect_timeout(Duration::from_secs(15))
                .tcp_keepalive(Duration::from_secs(60))
                .build()
                .map_err(|_| "Could not create Qoder client".into())
        })
        .as_ref()
        .map_err(ProxyError::upstream)
}
fn directory(value: &Value) -> Catalog {
    let mut catalog = Catalog::new();
    for row in value["app"]
        .as_array()
        .into_iter()
        .flatten()
        .chain(value["chat"].as_array().into_iter().flatten())
    {
        let Some(id) = row["key"]
            .as_str()
            .filter(|id| !id.is_empty() && !matches!(*id, "auto" | "default"))
        else {
            continue;
        };
        if row["enable"] != true || row["hidden"] == true || row["disabled"] == true {
            continue;
        }
        if contains_secret(row) {
            continue;
        }
        catalog.entry(id.into()).or_insert_with(|| row.clone());
    }
    catalog
}
fn model_aliases(models: &Catalog) -> BTreeMap<String, String> {
    models
        .iter()
        .map(|(key, row)| {
            let name = row["display_name"]
                .as_str()
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .unwrap_or(key);
            let unique = models
                .values()
                .filter(|row| row["display_name"].as_str().map(str::trim) == Some(name))
                .count()
                == 1;
            let alias = if unique && (name == key || !models.contains_key(name)) {
                name
            } else {
                key
            };
            (alias.to_owned(), key.clone())
        })
        .collect()
}
fn resolve_model<'a>(models: &'a Catalog, id: &str) -> Option<&'a Value> {
    models.get(id).or_else(|| {
        model_aliases(models)
            .get(id)
            .and_then(|key| models.get(key))
    })
}
pub(super) fn cached_model_labels(
    provider: AccountProvider,
    account_id: &str,
) -> BTreeMap<String, String> {
    model_aliases(&cached_catalog(provider, account_id))
        .into_iter()
        .filter(|(alias, key)| alias != key)
        .map(|(alias, key)| (key, alias))
        .collect()
}
pub(super) fn cached_free_model_ids(provider: AccountProvider, account_id: &str) -> Vec<String> {
    let models = cached_catalog(provider, account_id);
    let mut free: BTreeSet<String> = models
        .iter()
        .filter(|(_, row)| is_free_model(row))
        .map(|(key, _)| key.clone())
        .collect();
    for (alias, key) in model_aliases(&models) {
        if free.contains(&key) {
            free.insert(alias);
        }
    }
    free.into_iter().collect()
}
fn is_free_model(row: &Value) -> bool {
    if let Some(price) = row.get("price_factor").or_else(|| row.get("priceFactor")) {
        let Some(price) = price.as_f64().filter(|price| price.is_finite()) else {
            return false;
        };
        if price != 0.0 {
            return false;
        }
        let promotion = row.get("promotion").or_else(|| row.get("prommotion"));
        let discounted = promotion.is_some_and(|promotion| {
            promotion["active"] == true
                && promotion
                    .get("before_promotion_price_factor")
                    .or_else(|| promotion.get("beforePromotionPriceFactor"))
                    .and_then(Value::as_f64)
                    .is_some_and(|before| before.is_finite() && before > 0.0)
        });
        return !discounted;
    }
    row.get("is_free")
        .or_else(|| row.get("isFree"))
        .and_then(Value::as_bool)
        == Some(true)
}
fn cached_catalog(provider: AccountProvider, account_id: &str) -> Catalog {
    let edition = match provider {
        AccountProvider::Qoder => Edition::Qoder,
        AccountProvider::QoderCn => Edition::QoderCn,
        _ => return BTreeMap::new(),
    };
    let Ok(cache) = cache().lock() else {
        return BTreeMap::new();
    };
    for (cache_key, models) in &cache.0 {
        let mut parts = cache_key.split('\0');
        let (Some(tool), Some(user), Some(base), None) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            continue;
        };
        if tool != edition.tool() || user.is_empty() || base != edition.gateway() {
            continue;
        }
        let cached_account_id = format!("{:x}", Sha256::digest(format!("{tool}\0{user}")));
        if cached_account_id == account_id {
            return models.clone();
        }
    }
    BTreeMap::new()
}
fn contains_secret(value: &Value) -> bool {
    match value {
        Value::Object(fields) => fields.iter().any(|(k, v)| {
            matches!(
                k.to_ascii_lowercase().as_str(),
                "api_key"
                    | "apikey"
                    | "access_token"
                    | "accesstoken"
                    | "refresh_token"
                    | "refreshtoken"
                    | "authorization"
                    | "password"
            ) || contains_secret(v)
        }),
        Value::Array(values) => values.iter().any(contains_secret),
        _ => false,
    }
}
async fn catalog(edition: Edition, c: &Value, base: &str) -> Result<Catalog, ProxyError> {
    let response = http_client()?
        .get(format!("{base}{MODELS_PATH}"))
        .headers(codec::headers(c, MODELS_PATH, "")?)
        .timeout(Duration::from_secs(15))
        .send()
        .await
        .map_err(ProxyError::upstream)?;
    let status = response.status();
    let headers = response.headers().clone();
    if status.is_redirection() {
        return Err(ProxyError::new(
            StatusCode::BAD_GATEWAY,
            "Qoder model directory returned an unexpected redirect",
        ));
    }
    let value = response
        .json::<Value>()
        .await
        .map_err(ProxyError::upstream)?;
    if !status.is_success() {
        return Err(failure(status, &headers, &value));
    }
    if value
        .get("code")
        .is_some_and(|code| code != 0 && code != "0")
    {
        return Err(failure(status, &headers, &value));
    }
    let models = directory(&value);
    if models.is_empty() {
        return Err(ProxyError::upstream(
            "Qoder account model directory is empty",
        ));
    }
    let mut cache = cache()
        .lock()
        .map_err(|_| ProxyError::upstream("Qoder model cache is unavailable"))?;
    cache.0.insert(key(edition, c, base)?, models.clone());
    #[cfg(not(test))]
    if crate::services::cursor_auth::write(&cache_path(), &*cache).is_err() {
        log::warn!("[QoderProxy] Could not persist the model directory");
    }
    Ok(models)
}
pub(super) async fn list_models(
    provider: AccountProvider,
    c: Value,
    _client: &reqwest::Client,
) -> Result<Vec<String>, ProxyError> {
    let edition = edition(provider, &c)?;
    let base = base(edition, &c)?;
    Ok(model_aliases(&catalog(edition, &c, &base).await?)
        .into_keys()
        .collect())
}
pub(super) async fn route(
    provider: AccountProvider,
    c: Value,
    id: &str,
    mut body: Value,
    _client: &reqwest::Client,
) -> Result<Response, ProxyError> {
    super::strip_response_metadata(&mut body);
    let edition = edition(provider, &c)?;
    let base = base(edition, &c)?;
    let cache_key = key(edition, &c, &base)?;
    let cached = cache()
        .lock()
        .map_err(|_| ProxyError::upstream("Qoder model cache is unavailable"))?
        .0
        .get(&cache_key)
        .and_then(|models| resolve_model(models, id))
        .cloned();
    let model = match cached {
        Some(model) => model,
        None => {
            let models = catalog(edition, &c, &base).await?;
            resolve_model(&models, id).cloned().ok_or_else(|| {
                let mut e = ProxyError::new(
                    StatusCode::NOT_FOUND,
                    "Qoder model is unavailable for this account",
                );
                e.model_scope = true;
                e.error_type = Some("model_not_supported".into());
                e
            })?
        }
    };
    let request = prepare(&body, &model)?;
    let wire = codec::encode(&request.to_string());
    let mut headers = codec::headers(&c, CHAT_PATH, &wire)?;
    headers.insert(
        header::ACCEPT,
        HeaderValue::from_static("text/event-stream"),
    );
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    for (name, value) in [
        ("x-model-key", model["key"].as_str().unwrap_or(id)),
        ("x-model-source", model["source"].as_str().unwrap_or("")),
    ] {
        headers.insert(
            name,
            HeaderValue::from_str(value).map_err(ProxyError::upstream)?,
        );
    }
    let response = http_client()?
        .post(format!("{base}{CHAT_PATH}"))
        .headers(headers)
        .body(wire)
        .send()
        .await
        .map_err(ProxyError::upstream)?;
    let status = response.status();
    let headers = response.headers().clone();
    if status.is_redirection() {
        return Err(ProxyError::new(
            StatusCode::BAD_GATEWAY,
            "Qoder chat returned an unexpected redirect",
        ));
    }
    if !status.is_success() {
        let value = response.json::<Value>().await.unwrap_or(Value::Null);
        return Err(failure(status, &headers, &value));
    }
    if !headers
        .get(header::CONTENT_TYPE)
        .and_then(|h| h.to_str().ok())
        .is_some_and(|h| {
            h.split(';')
                .next()
                .is_some_and(|mime| mime.trim() == "text/event-stream")
        })
    {
        let value = response.json::<Value>().await.unwrap_or(Value::Null);
        return Err(failure(StatusCode::BAD_GATEWAY, &headers, &value));
    }
    stream::convert(response, id, &body).await
}
fn unsupported(parameter: &str) -> ProxyError {
    let mut e = ProxyError::new(
        StatusCode::BAD_REQUEST,
        format!("Qoder cannot enforce parameter {parameter}"),
    );
    e.error_code = Some("unsupported_parameter".into());
    e.model_scope = true;
    e
}
fn prepare(body: &Value, model: &Value) -> Result<Value, ProxyError> {
    protocol::reject_structured_output(body)?;
    for parameter in [
        "temperature",
        "top_p",
        "frequency_penalty",
        "presence_penalty",
        "stop",
        "seed",
        "logprobs",
        "top_logprobs",
        "functions",
        "function_call",
        "prediction",
        "audio",
        "modalities",
    ] {
        if body.get(parameter).is_some_and(|v| !v.is_null()) {
            return Err(unsupported(parameter));
        }
    }
    if body.get("n").is_some_and(|n| !n.is_null() && n != 1)
        || body
            .get("parallel_tool_calls")
            .is_some_and(|v| v != true && !v.is_null())
    {
        return Err(unsupported("parallel_tool_calls/n"));
    }
    let choice = body["tool_choice"].as_str();
    if body
        .get("tool_choice")
        .is_some_and(|v| !v.is_null() && !matches!(v.as_str(), Some("auto" | "none")))
    {
        return Err(unsupported("tool_choice"));
    }
    let mut tools = match body.get("tools").filter(|v| !v.is_null()) {
        Some(value) => value
            .as_array()
            .cloned()
            .ok_or_else(|| unsupported("tools"))?,
        None => vec![],
    };
    for tool in &tools {
        if tool["type"] != "function"
            || !tool["function"]["name"].is_string()
            || tool["function"]["strict"] == true
        {
            return Err(unsupported("tools"));
        }
    }
    for tool in &mut tools {
        if let Some(parameters) = tool
            .get_mut("function")
            .and_then(|function| function.get_mut("parameters"))
        {
            super::tools::type_object_unions(parameters);
        }
    }
    let mut system=vec!["You are a Qoder agent. Use the instructions below and the tools available to you to assist the user.".to_string()];
    let mut messages = vec![];
    for message in body["messages"]
        .as_array()
        .ok_or_else(|| unsupported("messages"))?
    {
        let role = message["role"]
            .as_str()
            .ok_or_else(|| unsupported("messages.role"))?;
        if !matches!(role, "system" | "developer" | "user" | "assistant" | "tool") {
            return Err(unsupported("messages.role"));
        }
        let blocks = blocks(&message["content"], model["is_vl"] == true, role)?;
        if matches!(role, "system" | "developer") {
            system.push(protocol::text(&Value::Array(blocks)));
            continue;
        }
        let mut item = json!({"role":role,"content":blocks});
        if role == "tool" {
            let id = message["tool_call_id"]
                .as_str()
                .filter(|s| !s.is_empty())
                .ok_or_else(|| unsupported("tool_call_id"))?;
            item = json!({"role":"tool","tool_call_id":id,"content":protocol::text(&message["content"])});
        }
        if let Some(calls) = message.get("tool_calls").filter(|v| !v.is_null()) {
            if role != "assistant" {
                return Err(unsupported("messages.tool_calls role"));
            }
            let calls = calls
                .as_array()
                .ok_or_else(|| unsupported("messages.tool_calls"))?;
            for call in calls {
                if !call["id"].is_string()
                    || call.get("type").is_some_and(|kind| kind != "function")
                    || !call["function"]["name"].is_string()
                    || !call["function"]["arguments"].is_string()
                {
                    return Err(unsupported("messages.tool_calls"));
                }
            }
            item["content"] = json!(protocol::text(&message["content"]));
            item["tool_calls"] = json!(calls
                .iter()
                .map(|call| json!({"id":call["id"],"type":"function","function":call["function"]}))
                .collect::<Vec<_>>());
        }
        messages.push(item);
    }
    let sys = json!({"type":"text","text":system.join("\n\n")});
    messages.insert(0, json!({"role":"system","content":[sys.clone()]}));
    let cap = body
        .get("max_completion_tokens")
        .filter(|v| !v.is_null())
        .or_else(|| body.get("max_tokens").filter(|v| !v.is_null()))
        .map(|v| {
            v.as_u64()
                .filter(|n| *n > 0)
                .ok_or_else(|| unsupported("max_tokens"))
        })
        .transpose()?
        .unwrap_or(32000);
    if let (Some(a), Some(b)) = (body.get("max_tokens"), body.get("max_completion_tokens")) {
        if !a.is_null() && !b.is_null() && a != b {
            return Err(unsupported("conflicting max_tokens"));
        }
    }
    let tc = &model["thinking_config"];
    let thinks = tc.get("enabled").is_some_and(|v| !v.is_null()) || model["is_reasoning"] == true;
    let mut thinking = thinks;
    let mut effort = None;
    if let Some(asked) = body
        .get("reasoning_effort")
        .or_else(|| body["reasoning"].get("effort"))
        .filter(|v| !v.is_null())
    {
        let asked = asked
            .as_str()
            .ok_or_else(|| unsupported("reasoning_effort"))?;
        if asked == "none" {
            if thinks && tc["disabled"].is_null() {
                return Err(unsupported("reasoning_effort=none"));
            }
            thinking = false;
        } else if !thinks
            || !(tc["enabled"]["efforts"].get(asked).is_some()
                || model["reasoning_efforts"]
                    .as_array()
                    .is_some_and(|values| values.iter().any(|v| v == asked)))
        {
            return Err(unsupported("reasoning_effort"));
        } else {
            effort = Some(asked);
        }
    }
    if let Some(reasoning) = body.get("reasoning").filter(|v| !v.is_null()) {
        if reasoning
            .as_object()
            .is_none_or(|object| object.keys().any(|key| key != "effort"))
        {
            return Err(unsupported("reasoning"));
        }
    }
    let mut parameters = json!({"enable_thinking":thinking,"max_tokens":cap});
    if let Some(effort) = effort {
        parameters["reasoning_effort"] = json!(effort);
    }
    let context = model["context_config"]
        .as_object()
        .and_then(|o| o.values().find(|v| v["is_default"] == true))
        .and_then(|v| v["token_count"].as_u64())
        .or_else(|| model["default_context_window"].as_u64())
        .or_else(|| model["max_input_tokens"].as_u64());
    if let Some(context) = context.filter(|n| *n > 0) {
        parameters["context_length"] = json!(context);
    }
    let mut request = json!({"parameters":parameters,"business":{"product":"app","version":codec::VERSION,"type":"agent","id":uuid::Uuid::new_v4().simple().to_string(),"name":"EchoBird session","begin_at":chrono::Utc::now().timestamp_millis(),"stage":"start"},"agent_id":"agent_common","task_id":"common","session_type":"app","model_config":model,"system":[sys],"messages":messages});
    if choice != Some("none") && !tools.is_empty() {
        request["tools"] = json!(tools);
    }
    Ok(request)
}
fn blocks(content: &Value, images: bool, role: &str) -> Result<Vec<Value>, ProxyError> {
    if content.is_null() {
        return Ok(vec![]);
    }
    if let Some(text) = content.as_str() {
        return Ok(vec![json!({"type":"text","text":text})]);
    }
    let blocks = content
        .as_array()
        .ok_or_else(|| unsupported("messages.content"))?;
    for block in blocks {
        match block["type"].as_str() {
            Some("text") if block["text"].is_string() => {}
            Some("image_url")
                if images && role == "user" && block["image_url"]["url"].is_string() => {}
            _ => return Err(unsupported("messages.content attachment")),
        }
    }
    Ok(blocks.clone())
}
fn failure(status: StatusCode, headers: &HeaderMap, value: &Value) -> ProxyError {
    let code = value
        .get("code")
        .and_then(|v| {
            v.as_str()
                .map(str::to_string)
                .or_else(|| v.as_u64().map(|n| n.to_string()))
        })
        .unwrap_or_default();
    let queue = if value["queue"].is_object() {
        value["queue"].clone()
    } else {
        value["message"]
            .as_str()
            .and_then(|s| serde_json::from_str::<Value>(s).ok())
            .unwrap_or(Value::Null)
    };
    let queued = code == "10605" || queue["isQueued"] == true || queue["serviceAvailable"] == false;
    let quota = matches!(
        code.as_str(),
        "110" | "112" | "113" | "114" | "115" | "116" | "117" | "118" | "119" | "122"
    );
    let incompatible = matches!(
        code.as_str(),
        "406" | "416" | "430" | "80411" | "80412" | "100400" | "100401" | "100403"
    );
    let status = if queued {
        StatusCode::SERVICE_UNAVAILABLE
    } else if quota {
        StatusCode::TOO_MANY_REQUESTS
    } else if incompatible {
        StatusCode::BAD_REQUEST
    } else if code == "105" || matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN) {
        StatusCode::UNAUTHORIZED
    } else if !status.is_client_error() && !status.is_server_error() {
        StatusCode::BAD_GATEWAY
    } else {
        status
    };
    let mut e = ProxyError::new(
        status,
        if queued {
            "Qoder model is queued; retry later".to_string()
        } else {
            format!(
                "Qoder upstream request failed{}",
                if code.is_empty() {
                    String::new()
                } else {
                    format!(" (code {code})")
                }
            )
        },
    );
    e.error_code = Some(if incompatible {
        "unsupported_parameter".into()
    } else {
        format!("qoder_{code}")
    });
    e.model_scope = queued || incompatible;
    e.retry_after = if queued {
        HeaderValue::from_str(
            &queue["retryAfterSeconds"]
                .as_f64()
                .filter(|n| n.is_finite() && *n > 0.0)
                .map(|n| n.ceil() as u64)
                .unwrap_or(30)
                .to_string(),
        )
        .ok()
    } else {
        headers.get(header::RETRY_AFTER).cloned()
    };
    e
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn protocol_regression_object_unions_keep_their_contract() {
        let schema = json!({"type":"object","properties":{"choice":{"$ref":"#/$defs/Choice"}},"$defs":{"Choice":{"oneOf":[{"type":"object","properties":{"a":{"type":"string"}}},{"type":"object","properties":{"b":{"type":"number"}}}]}},"examples":[{"oneOf":[{"type":"object"}]}]});
        let original = json!({"messages":[{"role":"user","content":"hello"}],"tools":[{"type":"function","function":{"name":"read","parameters":schema}}]});
        let prepared = prepare(&original, &json!({})).unwrap();
        let actual = &prepared["tools"][0]["function"]["parameters"];
        assert_eq!(actual["$defs"]["Choice"]["type"], "object");
        assert_eq!(actual["examples"], schema["examples"]);
        assert_eq!(original["tools"][0]["function"]["parameters"], schema);
    }
    use axum::body::Body;
    use axum::{
        extract::{Request, State},
        routing::{get, post},
        Router,
    };
    use bytes::Bytes;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    type Requests = Arc<tokio::sync::Mutex<Vec<(HeaderMap, String)>>>;
    fn credentials(edition: Edition, base: &str, user: &str) -> Value {
        json!({"edition":edition,"accessToken":"fixture-access","userId":user,"machineId":"fixture-machine","name":"name","email":"email","baseUrl":base})
    }
    fn model() -> Value {
        json!({"key":"model","display_name":"Visible Model","enable":true,"source":"vendor","is_vl":true,"thinking_config":{"enabled":{"efforts":{"low":{"is_default":true},"high":{}}},"disabled":{}},"context_config":{"small":{"token_count":200000,"is_default":true},"large":{"token_count":1000000}}})
    }
    fn frame(value: Value) -> String {
        format!(
            "data: {}\n\n",
            json!({"statusCodeValue":200,"body":value.to_string()})
        )
    }
    async fn directory_handler() -> axum::Json<Value> {
        axum::Json(
            json!({"app":[model(),{"key":"fast","display_name":"Fast","source":"vendor","enable":true}],"chat":[{"key":"model","display_name":"Wrong Scene","source":"other","enable":true}]}),
        )
    }
    async fn chat(State(requests): State<Requests>, request: Request) -> Response {
        let h = request.headers().clone();
        let bytes = axum::body::to_bytes(request.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let wire = String::from_utf8(bytes.to_vec()).unwrap();
        let text = codec::decode(&wire).unwrap();
        requests.lock().await.push((h, text));
        let payload = frame(json!({"choices":[{"delta":{"content":"模型"}}]}))
            + &frame(json!({"choices":[{"delta":{},"finish_reason":"stop"}]}))
            + &frame(
                json!({"choices":[],"usage":{"prompt_tokens":2,"completion_tokens":1,"total_tokens":3}}),
            )
            + &format!(
                "data: {}\n\n",
                json!({"statusCodeValue":200,"body":"[DONE]"})
            );
        stream_response(Body::from_stream(futures_util::stream::iter(
            payload
                .into_bytes()
                .into_iter()
                .map(|byte| Ok::<_, std::io::Error>(Bytes::from(vec![byte]))),
        )))
    }
    #[test]
    fn directory_prefers_app_configs_and_excludes_disabled_and_secret_configs() {
        let models = directory(
            &json!({"app":[{"key":"same","source":"app","enable":true},{"key":"disabled","enable":false},{"key":"custom","enable":true,"api_key":"private"}],"chat":[{"key":"same","source":"chat","enable":true},{"key":"legacy","enable":true},{"key":"auto","enable":true}]}),
        );
        assert_eq!(models.len(), 2);
        assert_eq!(models["same"]["source"], "app");
        assert!(models.contains_key("legacy"));
        assert!(!serde_json::to_string(&models).unwrap().contains("private"));
    }
    #[test]
    fn readable_model_aliases_resolve_to_unchanged_native_configs_and_old_cached_keys() {
        let rows = [
            ("cmodel", "Cantus"),
            ("qmodel_38max", "Qwen3.8-Max"),
            ("kmodel_latest", "Kimi-K3"),
            ("gmodel", "GLM-5.3"),
            ("dmodel", "DeepSeek-V4-Pro"),
            ("dfmodel", "DeepSeek-Flash"),
        ];
        let models = directory(&json!({"app":rows.iter().map(|(key,name)| json!({
            "key":key,"display_name":name,"enable":true,"source":"system",
            "context_config":{"200K":{"token_count":200000,"is_default":true}},
        })).collect::<Vec<_>>()}));
        let aliases = model_aliases(&models);
        assert_eq!(aliases.len(), rows.len());
        for (key, name) in rows {
            assert_eq!(aliases[name], key);
            assert_eq!(resolve_model(&models, name), models.get(key));
            assert_eq!(resolve_model(&models, key), models.get(key));
        }
        let old_cache = Cache(BTreeMap::from([("fixture-account".into(), models)]));
        let restored: Cache =
            serde_json::from_value(serde_json::to_value(old_cache).unwrap()).unwrap();
        let models = &restored.0["fixture-account"];
        assert_eq!(resolve_model(models, "Cantus"), models.get("cmodel"));
        assert_eq!(resolve_model(models, "cmodel"), models.get("cmodel"));
        assert!(resolve_model(models, "Unavailable Model").is_none());
    }
    #[test]
    fn ambiguous_names_and_names_matching_other_native_keys_do_not_change_routes() {
        let models = directory(&json!({"app":[
            {"key":"first","display_name":"Shared Name","enable":true},
            {"key":"second","display_name":"Shared Name","enable":true},
            {"key":"reserved","display_name":"Readable Name","enable":true},
            {"key":"third","display_name":"reserved","enable":true},
            {"key":"unnamed","display_name":" ","enable":true},
        ]}));
        assert_eq!(
            model_aliases(&models).into_keys().collect::<Vec<_>>(),
            vec!["Readable Name", "first", "second", "third", "unnamed"]
        );
        assert_eq!(resolve_model(&models, "reserved"), models.get("reserved"));
        assert_eq!(
            resolve_model(&models, "Readable Name"),
            models.get("reserved")
        );
        assert!(resolve_model(&models, "Shared Name").is_none());
    }
    #[test]
    fn free_models_require_explicit_valid_zero_prices_or_an_unpriced_free_flag() {
        for row in [
            json!({"price_factor":0}),
            json!({"priceFactor":0.0}),
            json!({"price_factor":0,"is_free":false}),
            json!({"price_factor":0,"original_price_factor":0.1}),
            json!({"is_free":true}),
            json!({"isFree":true}),
        ] {
            assert!(is_free_model(&row), "{row}");
        }
        for row in [
            json!({}),
            json!({"is_free":false}),
            json!({"is_free":"true"}),
            json!({"price_factor":0.5,"is_free":true}),
            json!({"priceFactor":1,"isFree":true}),
            json!({"price_factor":-1,"is_free":true}),
            json!({"price_factor":null,"is_free":true}),
            json!({"price_factor":"0","is_free":true}),
            json!({"price_factor":false,"is_free":true}),
            json!({"price_factor":[],"is_free":true}),
            json!({"price_factor":{},"is_free":true}),
            json!({"price_factor":null,"priceFactor":0,"is_free":true}),
        ] {
            assert!(!is_free_model(&row), "{row}");
        }
    }
    #[test]
    fn active_discounts_are_not_free_and_expired_discounts_preserve_zero_prices() {
        for row in [
            json!({"price_factor":0,"is_free":true,"promotion":{"active":true,"before_promotion_price_factor":0.5}}),
            json!({"priceFactor":0,"isFree":true,"prommotion":{"active":true,"beforePromotionPriceFactor":1}}),
        ] {
            assert!(!is_free_model(&row), "{row}");
        }
        for row in [
            json!({"price_factor":0,"promotion":{"active":false,"before_promotion_price_factor":0.5}}),
            json!({"price_factor":0,"promotion":{"active":true,"before_promotion_price_factor":0}}),
            json!({"price_factor":0,"promotion":{"active":true}}),
        ] {
            assert!(is_free_model(&row), "{row}");
        }
    }
    #[test]
    fn passive_cached_labels_are_bound_to_the_saved_account_and_edition() {
        let user = uuid::Uuid::new_v4().to_string();
        let other_user = uuid::Uuid::new_v4().to_string();
        let mut entries = Vec::new();
        for (edition, user, name) in [
            (Edition::Qoder, user.as_str(), "Global Model"),
            (Edition::QoderCn, user.as_str(), "CN Model"),
            (Edition::Qoder, other_user.as_str(), "Other User Model"),
        ] {
            let models = directory(&json!({"app":[
                {"key":"cmodel","display_name":name,"enable":true,"price_factor":0},
                {"key":"first","display_name":"Shared Name","enable":true,"price_factor":0},
                {"key":"second","display_name":"Shared Name","enable":true},
                {"key":"paid","display_name":"cmodel","enable":true,"price_factor":1,"is_free":true},
                {"key":"self","display_name":"self","enable":true,"price_factor":0},
                {"key":"unnamed","enable":true},
            ]}));
            let cache_key = format!("{}\0{user}\0{}", edition.tool(), edition.gateway());
            cache()
                .lock()
                .unwrap()
                .0
                .insert(cache_key.clone(), models.clone());
            entries.push((cache_key, models));
        }
        let opaque_id = |edition: Edition, user: &str| {
            format!(
                "{:x}",
                Sha256::digest(format!("{}\0{user}", edition.tool()))
            )
        };
        for (provider, edition, user, name) in [
            (
                AccountProvider::Qoder,
                Edition::Qoder,
                user.as_str(),
                "Global Model",
            ),
            (
                AccountProvider::QoderCn,
                Edition::QoderCn,
                user.as_str(),
                "CN Model",
            ),
            (
                AccountProvider::Qoder,
                Edition::Qoder,
                other_user.as_str(),
                "Other User Model",
            ),
        ] {
            assert_eq!(
                cached_model_labels(provider, &opaque_id(edition, user)),
                BTreeMap::from([("cmodel".into(), name.into())])
            );
            assert_eq!(
                cached_free_model_ids(provider, &opaque_id(edition, user)),
                vec![name, "cmodel", "first", "self"]
            );
        }
        assert!(
            cached_model_labels(AccountProvider::Qoder, &opaque_id(Edition::QoderCn, &user))
                .is_empty()
        );
        assert!(
            cached_free_model_ids(AccountProvider::Qoder, &opaque_id(Edition::QoderCn, &user))
                .is_empty()
        );
        assert!(cached_model_labels(AccountProvider::Qoder, &user).is_empty());
        assert!(cached_model_labels(AccountProvider::Qoder, "invalid-opaque-id").is_empty());
        assert!(cached_free_model_ids(AccountProvider::Qoder, &user).is_empty());
        assert!(cached_free_model_ids(AccountProvider::Qoder, "invalid-opaque-id").is_empty());
        assert!(
            cached_model_labels(AccountProvider::Kilo, &opaque_id(Edition::Qoder, &user))
                .is_empty()
        );
        assert!(
            cached_free_model_ids(AccountProvider::Kilo, &opaque_id(Edition::Qoder, &user))
                .is_empty()
        );
        let mut cached = cache().lock().unwrap();
        for (key, original) in entries {
            assert_eq!(cached.0.remove(&key).unwrap(), original);
        }
    }
    #[test]
    fn passive_labels_never_use_nonofficial_or_malformed_cache_endpoints() {
        let user = uuid::Uuid::new_v4().to_string();
        let account_id = format!("{:x}", Sha256::digest(format!("qoder\0{user}")));
        let models = directory(&json!({"app":[
            {"key":"cmodel","display_name":"Private Endpoint Model","enable":true,"price_factor":0},
        ]}));
        let keys = [
            format!("qoder\0{user}\0http://127.0.0.1:54321"),
            format!("qoder\0{user}\0{}\0extra", Edition::Qoder.gateway()),
            format!("qoder\0{user}\0{}", Edition::QoderCn.gateway()),
        ];
        {
            let mut cached = cache().lock().unwrap();
            for key in &keys {
                cached.0.insert(key.clone(), models.clone());
            }
        }
        assert!(cached_model_labels(AccountProvider::Qoder, &account_id).is_empty());
        assert!(cached_free_model_ids(AccountProvider::Qoder, &account_id).is_empty());
        let mut cached = cache().lock().unwrap();
        for key in &keys {
            assert_eq!(cached.0.remove(key).unwrap(), models);
        }
    }
    #[test]
    fn preparation_preserves_parallel_tool_history_and_exact_native_parameters() {
        let body = json!({"max_tokens":4096,"reasoning_effort":"high","messages":[{"role":"system","content":"instructions"},{"role":"user","content":[{"type":"text","text":"question"},{"type":"image_url","image_url":{"url":"data:image/png;base64,eA=="}}]},{"role":"assistant","content":null,"tool_calls":[{"id":"a","type":"function","function":{"name":"read","arguments":"{}"}},{"id":"b","type":"function","function":{"name":"read","arguments":"{}"}}]},{"role":"tool","tool_call_id":"a","content":"first"},{"role":"tool","tool_call_id":"b","content":"second"}],"tools":[{"type":"function","function":{"name":"read","parameters":{"type":"object","properties":{"p":{"type":"string"}},"required":["p"]}}}]});
        let request = prepare(&body, &model()).unwrap();
        assert_eq!(request["parameters"]["max_tokens"], 4096);
        assert_eq!(request["parameters"]["reasoning_effort"], "high");
        assert_eq!(request["parameters"]["context_length"], 200000);
        assert_eq!(request["model_config"], model());
        assert_eq!(
            request["messages"][2]["tool_calls"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        assert_eq!(request["messages"][3]["tool_call_id"], "a");
        assert_eq!(request["messages"][4]["tool_call_id"], "b");
        assert_eq!(request["tools"], body["tools"]);
        assert!(request["system"][0]["text"]
            .as_str()
            .unwrap()
            .contains("instructions"));
    }
    #[test]
    fn strict_parameters_and_unrepresentable_attachments_fail_explicitly() {
        for (key, value) in [
            ("tool_choice", json!("required")),
            (
                "tool_choice",
                json!({"type":"function","function":{"name":"read"}}),
            ),
            ("parallel_tool_calls", json!(false)),
            (
                "response_format",
                json!({"type":"json_schema","json_schema":{"name":"x","schema":{"type":"object"}}}),
            ),
            ("temperature", json!(0.5)),
            ("max_tokens", json!(0)),
            ("reasoning_effort", json!("ultra")),
        ] {
            let mut body = json!({"messages":[{"role":"user","content":"text"}]});
            body[key] = value;
            let error = prepare(&body, &model()).unwrap_err();
            assert_eq!(error.error_code.as_deref(), Some("unsupported_parameter"));
            assert!(error.model_scope);
        }
        let body = json!({"messages":[{"role":"user","content":[{"type":"file","file":{"file_data":"private"}}]}]});
        assert!(prepare(&body, &model()).is_err());
        let mut body = json!({"tool_choice":"none","tools":[{"type":"function","function":{"name":"read"}}],"messages":[{"role":"user","content":"text"}]});
        assert!(prepare(&body, &model()).unwrap().get("tools").is_none());
        body["tools"][0]["function"]["strict"] = json!(true);
        assert!(prepare(&body, &model()).is_err());
    }
    #[tokio::test]
    async fn dynamic_models_and_split_streams_keep_region_identity_and_wire_body() {
        let requests: Requests = Arc::default();
        let app = Router::new()
            .route("/algo/api/v2/model/list", get(directory_handler))
            .route(
                "/algo/api/v2/service/pro/sse/agent_chat_generation",
                post(chat),
            )
            .with_state(requests.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let client = reqwest::Client::new();
        for (provider, edition) in [
            (AccountProvider::Qoder, Edition::Qoder),
            (AccountProvider::QoderCn, Edition::QoderCn),
        ] {
            let c = credentials(edition, &base, &uuid::Uuid::new_v4().to_string());
            assert_eq!(
                list_models(provider, c.clone(), &client).await.unwrap(),
                vec!["Fast", "Visible Model"]
            );
            for streaming in [false, true] {
                let id = if streaming { "Visible Model" } else { "model" };
                let response=route(provider,c.clone(),id,json!({"stream":streaming,"max_tokens":4096,"messages":[{"role":"user","content":"hello"}]}),&client).await.unwrap();
                let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
                    .await
                    .unwrap();
                let text = String::from_utf8(body.to_vec()).unwrap();
                if streaming {
                    assert!(text.contains("data: [DONE]"));
                    assert!(text.contains("模型"));
                } else {
                    let value: Value = serde_json::from_str(&text).unwrap();
                    assert_eq!(value["model"], id);
                    assert_eq!(value["choices"][0]["message"]["content"], "模型");
                    assert_eq!(value["usage"]["total_tokens"], 3);
                }
            }
        }
        let requests = requests.lock().await;
        assert_eq!(requests.len(), 4);
        for (h, body) in requests.iter() {
            assert_eq!(h["cosy-machineid"], "fixture-machine");
            assert_eq!(h["x-model-key"], "model");
            assert_eq!(h["x-model-source"], "vendor");
            let value: Value = serde_json::from_str(body).unwrap();
            assert_eq!(value["parameters"]["max_tokens"], 4096);
            assert_eq!(value["model_config"]["source"], "vendor");
            assert_eq!(value["model_config"], model());
            assert!(!body.contains("fixture-access"));
        }
        server.abort();
    }
    #[tokio::test]
    async fn redirects_never_forward_signed_credentials_and_stream_eof_is_failure() {
        let received = Arc::new(AtomicUsize::new(0));
        let count = received.clone();
        let destination = Router::new().fallback(move || {
            let count = count.clone();
            async move {
                count.fetch_add(1, Ordering::SeqCst);
                "redirected"
            }
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let target = format!("http://{}", listener.local_addr().unwrap());
        let target_server =
            tokio::spawn(async move { axum::serve(listener, destination).await.unwrap() });
        let app = Router::new()
            .route(
                "/algo/api/v2/model/list",
                get(move || {
                    let target = target.clone();
                    async move {
                        Response::builder()
                            .status(StatusCode::TEMPORARY_REDIRECT)
                            .header(header::LOCATION, target)
                            .body(Body::empty())
                            .unwrap()
                    }
                }),
            )
            .route(
                "/algo/api/v2/service/pro/sse/agent_chat_generation",
                post(|| async {
                    stream_response(Body::from(frame(
                        json!({"choices":[{"delta":{"content":"partial"}}]}),
                    )))
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let client = reqwest::Client::new();
        let c = credentials(Edition::Qoder, &base, &uuid::Uuid::new_v4().to_string());
        let error = list_models(AccountProvider::Qoder, c.clone(), &client)
            .await
            .unwrap_err();
        assert_eq!(error.status, StatusCode::BAD_GATEWAY);
        assert_eq!(received.load(Ordering::SeqCst), 0);
        cache().lock().unwrap().0.insert(
            key(Edition::Qoder, &c, &base).unwrap(),
            BTreeMap::from([("model".into(), model())]),
        );
        for streaming in [false, true] {
            let response = route(
                AccountProvider::Qoder,
                c.clone(),
                "model",
                json!({"stream":streaming,"messages":[{"role":"user","content":"text"}]}),
                &client,
            )
            .await
            .unwrap();
            assert!(axum::body::to_bytes(response.into_body(), 1024 * 1024)
                .await
                .is_err());
        }
        server.abort();
        target_server.abort();
    }
}
