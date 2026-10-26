use std::{
    collections::BTreeMap,
    sync::{Mutex, OnceLock},
    time::Duration,
};

use axum::{
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::Response,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::{protocol::WireProtocol, providers, ProxyError};
use crate::models::model::AccountProvider;

const MODEL_PATH: &str = "/api/ide/v1/get_detail_param";
const CHAT_PATH: &str = "/api/agent/v3/llm_utils_chat";
const API_VERSION_CODE: &str = "20260212";

#[cfg(test)]
pub(super) fn stream_diagnostics_enabled() -> bool {
    std::env::var("ECHOBIRD_LIVE_TRAE_STREAM_DIAGNOSTICS").as_deref() == Ok("1")
}

#[cfg(test)]
pub(super) fn stream_event_shape(event: &str, data: &str) -> Value {
    let parsed = serde_json::from_str::<Value>(data);
    let payload_type = match &parsed {
        Ok(Value::Object(_)) => "object",
        Ok(Value::Array(_)) => "array",
        Ok(Value::String(_)) => "string",
        Ok(Value::Number(_)) => "number",
        Ok(Value::Bool(_)) => "boolean",
        Ok(Value::Null) => "null",
        Err(_) => "non_json",
    };
    let notice = if event == "progress_notice" {
        match &parsed {
            Ok(Value::String(text)) => text.as_str(),
            Ok(Value::Object(object)) => object
                .get("notice")
                .or_else(|| object.get("message"))
                .or_else(|| object.get("content"))
                .and_then(Value::as_str)
                .unwrap_or_default(),
            Err(_) => data,
            _ => "",
        }
    } else {
        ""
    };
    let notice = notice
        .chars()
        .take(512)
        .flat_map(char::to_lowercase)
        .collect::<String>();
    let value = parsed.unwrap_or(Value::Null);
    let event = match event {
        "" => "unnamed",
        "metadata"
        | "output"
        | "error"
        | "done"
        | "timing"
        | "extra"
        | "token_usage"
        | "progress_notice"
        | "request_wait_in_queue"
        | "request_wait_in_limit_queue" => event,
        _ => "other",
    };
    let fields: Vec<_> = value
        .as_object()
        .into_iter()
        .flat_map(|object| object.keys())
        .take(32)
        .collect();
    let code = value
        .get("code")
        .or_else(|| value["error"].get("code"))
        .and_then(|code| {
            code.as_u64()
                .or_else(|| code.as_str().and_then(|code| code.parse::<u64>().ok()))
        });
    json!({"event":event,"payload_type":payload_type,"fields":fields,"code":code,"done_event":event == "done" || data == "[DONE]",
        "queue_hint":(["queue","排队","队列"].iter().any(|word| notice.contains(word))),
        "busy_hint":(["busy","繁忙","资源不足","资源紧张"].iter().any(|word| notice.contains(word))),
        "wait_hint":(["wait","等待","稍候","稍等"].iter().any(|word| notice.contains(word))),
        "finish_reason_present":value["finish_reason"].is_string(),"output_present":value.get("response").is_some() || value.get("reasoning_content").is_some() || value.get("tool_calls").is_some()})
}

#[cfg(test)]
pub(super) fn stream_error_shape(event: &str, data: &str, error: &str) -> Value {
    let value = serde_json::from_str::<Value>(data).unwrap_or(Value::Null);
    let detail = value
        .get("error")
        .filter(|value| value.is_object())
        .unwrap_or(&value);
    let message = detail["message"]
        .as_str()
        .or_else(|| detail["msg"].as_str())
        .or_else(|| value["error"].as_str())
        .or_else(|| value.as_str())
        .unwrap_or(error);
    let message = message
        .chars()
        .take(800)
        .flat_map(char::to_lowercase)
        .collect::<String>();
    let mut shape = stream_event_shape(event, data);
    shape["error_hints"] = json!({
        "timeout":(["timeout","timed out","超时"].iter().any(|word| message.contains(word))),
        "model":(["model","模型"].iter().any(|word| message.contains(word))),
        "unsupported":(["unsupported","not supported","不支持"].iter().any(|word| message.contains(word))),
        "invalid":(["invalid","无效"].iter().any(|word| message.contains(word))),
        "function":(["function","函数","通道"].iter().any(|word| message.contains(word))),
        "queue":(["queue","排队","队列"].iter().any(|word| message.contains(word))),
        "capacity":(["capacity","资源","繁忙"].iter().any(|word| message.contains(word)))
    });
    let mut pending = vec![(
        detail
            .get("extra")
            .or_else(|| value.get("extra"))
            .cloned()
            .unwrap_or(Value::Null),
        0,
    )];
    let mut status = None;
    let mut code = None;
    let mut hints = [false; 5];
    for _ in 0..16 {
        let Some((extra, depth)) = pending.pop() else {
            break;
        };
        if depth > 4 {
            continue;
        }
        match extra {
            Value::Object(object) => {
                for key in ["status", "status_code", "statusCode", "http_status"] {
                    if let Some(number) = object.get(key).and_then(|value| {
                        value
                            .as_u64()
                            .or_else(|| value.as_str().and_then(|value| value.parse().ok()))
                    }) {
                        if (100..=599).contains(&number) {
                            status.get_or_insert(number);
                        }
                    }
                }
                match object.get("code").and_then(Value::as_str) {
                    Some("ModelLoading") => {
                        code = Some("ModelLoading");
                    }
                    Some("InvalidParameter") => {
                        code = Some("InvalidParameter");
                    }
                    _ => {}
                }
                for key in [
                    "error", "body", "response", "data", "extra", "message", "msg",
                ] {
                    if let Some(value) = object.get(key) {
                        pending.push((value.clone(), depth + 1));
                    }
                }
            }
            Value::String(text) => {
                if text.len() <= 16_384 {
                    if let Ok(parsed) = serde_json::from_str::<Value>(&text) {
                        if parsed.is_object() {
                            pending.push((parsed, depth + 1));
                        }
                    }
                }
                let text = text
                    .chars()
                    .take(800)
                    .flat_map(char::to_lowercase)
                    .collect::<String>();
                for (hint, words) in hints.iter_mut().zip([
                    &["reasoning", "思考"] as &[&str],
                    &["thinking", "思考"],
                    &["disabled", "禁用"],
                    &["invalid", "无效"],
                    &["capacity", "资源", "繁忙"],
                ]) {
                    *hint |= words.iter().any(|word| text.contains(word));
                }
            }
            _ => {}
        }
    }
    shape["inner_error"] = json!({"status":status,"code":code,"hints":{
        "reasoning":hints[0],"thinking":hints[1],"disabled":hints[2],"invalid":hints[3],"capacity":hints[4]}});
    shape
}

#[cfg(test)]
fn catalog_row_shape(row: &Value) -> Value {
    let usage = row.get("usage");
    let usage_type = match usage {
        None => "missing",
        Some(Value::Null) => "null",
        Some(Value::String(_)) => "string",
        Some(Value::Number(_)) => "number",
        Some(Value::Bool(_)) => "boolean",
        Some(Value::Object(_)) => "object",
        Some(Value::Array(_)) => "array",
    };
    let usage_text = usage.and_then(Value::as_str).unwrap_or_default().trim();
    let usage_empty = usage.is_none_or(|value| {
        value.is_null() || value.as_str().is_some_and(|text| text.trim().is_empty())
    });
    let usage_chat = usage_text == "chat_completion";
    let usage_enum = (!usage_text.is_empty()
        && usage_text.len() <= 32
        && usage_text
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte == b'_'))
    .then_some(usage_text);
    let fields: Vec<_> = row
        .as_object()
        .into_iter()
        .flat_map(|object| object.keys())
        .take(64)
        .collect();
    let display_fields: Vec<_> = row["display_config"]
        .as_object()
        .into_iter()
        .flat_map(|object| object.keys())
        .take(32)
        .collect();
    let details: Vec<_> = row["model_detail_list"]
        .as_array()
        .into_iter()
        .flatten()
        .take(16)
        .map(|detail| {
            let fields: Vec<_> = detail
                .as_object()
                .into_iter()
                .flat_map(|object| object.keys())
                .take(32)
                .collect();
            let identifiers: BTreeMap<_, _> = ["model", "model_name", "name", "config_name"]
                .into_iter()
                .filter_map(|key| {
                    let text = detail[key].as_str()?;
                    (!text.is_empty()
                        && text.len() <= 128
                        && text.bytes().all(|byte| {
                            byte.is_ascii_alphanumeric()
                                || matches!(byte, b'-' | b'_' | b'.' | b'/')
                        }))
                    .then_some((key, text))
                })
                .collect();
            json!({"fields":fields,"identifiers":identifiers})
        })
        .collect();
    json!({"config_name":row["config_name"],"fields":fields,"usage_type":usage_type,"usage_empty":usage_empty,
        "usage_chat_completion":usage_chat,"usage_other":!usage_empty && !usage_chat,"usage_enum":usage_enum,
        "hidden":row["hidden"] == true,"disabled":row["disabled"] == true,
        "custom_models_count":row["custom_models"].as_array().map(Vec::len),"detail_count":row["model_detail_list"].as_array().map(Vec::len),"details":details,"display_fields":display_fields})
}

fn http_client() -> Result<&'static reqwest::Client, ProxyError> {
    static CLIENT: OnceLock<Result<reqwest::Client, String>> = OnceLock::new();
    CLIENT
        .get_or_init(|| {
            reqwest::Client::builder()
                .connect_timeout(Duration::from_secs(15))
                .tcp_keepalive(Duration::from_secs(60))
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .map_err(|error| error.to_string())
        })
        .as_ref()
        .map_err(ProxyError::upstream)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Model {
    function: String,
    max_tokens: Option<u64>,
    multimodal: bool,
    reasoning: Vec<String>,
}

type Catalog = BTreeMap<String, Model>;
type Catalogs = BTreeMap<String, Catalog>;

fn visible_chat_model(id: &str) -> bool {
    !id.starts_with("custom_model_")
        && !id.starts_with("search_agent_")
        && !id.starts_with("fast_apply")
        && !matches!(
            id,
            "search_agent"
                | "file_search_agent"
                | "summary"
                | "title_generation"
                | "input_optimization"
                | "laziness_detection"
        )
}

fn native_output_limit(row: &Value) -> Option<u64> {
    row["model_detail_list"]
        .as_array()?
        .iter()
        .map(|detail| detail["max_tokens"].as_u64().filter(|limit| *limit > 0))
        .collect::<Option<Vec<_>>>()?
        .into_iter()
        .max()
}

/// Trae 把展示与能力信息放在 `display_config` 里，且该字段可能是对象或 JSON 字符串。
fn display_config(row: &Value) -> Value {
    match &row["display_config"] {
        Value::Object(_) => row["display_config"].clone(),
        Value::String(text) => serde_json::from_str(text).unwrap_or(Value::Null),
        _ => Value::Null,
    }
}

/// 读取推理档位。
///
/// 新版本用 `reasoning_effort_config { support_thinking, options, default_level }`，
/// 旧版本用 `reasoning_effort_options`。只读旧字段会让新模型的能力被判成空，
/// 客户端一旦携带 `reasoning_effort` 就会被本地拦成 400。
fn reasoning_options(node: &Value) -> Option<Vec<String>> {
    let config = &node["reasoning_effort_config"];
    if config["support_thinking"] == true {
        if let Some(options) = config["options"].as_array() {
            let levels: Vec<String> = options
                .iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|level| !level.is_empty())
                .map(str::to_string)
                .collect();
            if !levels.is_empty() {
                return Some(levels);
            }
        }
    }
    let legacy: Vec<String> = node["reasoning_effort_options"]
        .as_array()?
        .iter()
        .filter_map(Value::as_str)
        .map(str::trim)
        .filter(|level| !level.is_empty())
        .map(str::to_string)
        .collect();
    (!legacy.is_empty()).then_some(legacy)
}

fn cache() -> &'static Mutex<Catalogs> {
    static CACHE: OnceLock<Mutex<Catalogs>> = OnceLock::new();
    CACHE.get_or_init(|| {
        #[cfg(not(test))]
        let saved = crate::services::cursor_auth::read(&cache_path()).unwrap_or_default();
        #[cfg(test)]
        let saved = Catalogs::new();
        Mutex::new(saved)
    })
}

#[cfg(not(test))]
fn cache_path() -> std::path::PathBuf {
    crate::utils::platform::echobird_dir().join("cache/trae-models.json")
}

fn field<'a>(credentials: &'a Value, name: &str) -> Result<&'a str, ProxyError> {
    credentials[name]
        .as_str()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            ProxyError::new(
                StatusCode::UNAUTHORIZED,
                format!("Trae account has no {name}"),
            )
        })
}

fn region(provider: AccountProvider, credentials: &Value) -> Result<&'static str, ProxyError> {
    let expected = match provider {
        AccountProvider::Trae => "ai",
        AccountProvider::Traecn => "cn",
        _ => {
            return Err(ProxyError::new(
                StatusCode::BAD_REQUEST,
                "Invalid Trae provider",
            ))
        }
    };
    let declared = credentials["region"]
        .as_str()
        .unwrap_or(expected)
        .to_ascii_lowercase();
    match (expected, declared.as_str()) {
        ("cn", "cn") => Ok("cn"),
        ("ai", "ai") => Ok("ai"),
        ("ai", "sg") => Ok("sg"),
        ("ai", "us") => Ok("us"),
        ("ai", "usttp") => Ok("usttp"),
        _ => Err(ProxyError::new(
            StatusCode::UNAUTHORIZED,
            "Trae account region does not match its provider",
        )),
    }
}

pub(super) fn base(provider: AccountProvider, credentials: &Value) -> Result<String, ProxyError> {
    let region = region(provider, credentials)?;
    #[cfg(test)]
    if let Some(raw) = credentials["baseUrl"].as_str() {
        let url = url::Url::parse(raw).map_err(ProxyError::upstream)?;
        if url.scheme() != "http"
            || !matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "[::1]"))
        {
            return Err(ProxyError::new(
                StatusCode::BAD_REQUEST,
                "Trae fixture must use loopback",
            ));
        }
        return Ok(raw.trim_end_matches('/').into());
    }
    Ok(match region {
        "cn" => "https://trae-api-cn.mchost.guru",
        "usttp" => "https://core-normal.traeapi.us",
        "sg" | "us" => "https://coresg-normal.trae.ai",
        _ => "https://core-normal.trae.ai",
    }
    .into())
}

fn headers(
    credentials: &Value,
    json_response: bool,
) -> Result<reqwest::header::HeaderMap, ProxyError> {
    let token = field(credentials, "accessToken")?;
    let request_id = uuid::Uuid::new_v4().to_string();
    let trace = request_id.replace('-', "");
    let version = field(credentials, "appVersion")?;
    let mut result = reqwest::header::HeaderMap::new();
    let entries = [
        ("authorization", format!("Cloud-IDE-JWT {token}")),
        ("x-ide-token", token.into()),
        ("x-cloudide-token", token.into()),
        ("x-uid", field(credentials, "userId")?.into()),
        ("x-machine-id", field(credentials, "machineId")?.into()),
        ("x-device-id", field(credentials, "deviceId")?.into()),
        (
            "x-device-type",
            match std::env::consts::OS {
                "macos" => "mac",
                other => other,
            }
            .into(),
        ),
        ("x-app-id", "6eefa01c-1036-4c7e-9ca5-d891f63bfcd8".into()),
        ("x-app-version", version.into()),
        ("x-ide-version", version.into()),
        ("x-ide-version-type", "stable".into()),
        ("x-plugin-channel", "icube-ai".into()),
        ("x-request-id", request_id.clone()),
        ("x-trae-request-id", request_id),
        ("x-custom-trace-id", trace.clone()),
        (
            "x-flow-traceparent",
            format!("04-{trace}-{}-01", &trace[..16]),
        ),
        ("request-traffic-type", "prod".into()),
        ("user-agent", format!("Trae/{version}")),
        ("content-type", "application/json".into()),
        (
            "accept",
            if json_response {
                "application/json"
            } else {
                "text/event-stream"
            }
            .into(),
        ),
    ];
    for (name, value) in entries {
        result.insert(
            name,
            HeaderValue::from_str(&value).map_err(|_| {
                ProxyError::new(
                    StatusCode::UNAUTHORIZED,
                    "Trae account contains an invalid request header",
                )
            })?,
        );
    }
    #[cfg(test)]
    if std::env::var("ECHOBIRD_LIVE_TRAE_CATALOG_DIAGNOSTICS").as_deref() == Ok("1")
        && std::env::var("ECHOBIRD_LIVE_TRAE_CATALOG_VERSION_CODE").as_deref() == Ok("none")
    {
        return Ok(result);
    }
    result.insert(
        "x-ide-version-code",
        HeaderValue::from_static(API_VERSION_CODE),
    );
    Ok(result)
}

fn key(provider: AccountProvider, credentials: &Value, base: &str) -> Result<String, ProxyError> {
    Ok(json!([
        provider,
        region(provider, credentials)?,
        field(credentials, "userId")?,
        base
    ])
    .to_string())
}

fn unsupported(parameter: &str) -> ProxyError {
    let mut error = ProxyError::new(
        StatusCode::BAD_REQUEST,
        format!("Trae does not advertise support for {parameter} on this model"),
    );
    error.error_code = Some("unsupported_parameter".into());
    error.model_scope = true;
    error
}

pub(super) fn stream_error(event: &str, data: &str, headers: &HeaderMap) -> Option<ProxyError> {
    let value: Value = serde_json::from_str(data).ok()?;
    failure(event, &value, headers)
}

fn failure(event: &str, value: &Value, headers: &HeaderMap) -> Option<ProxyError> {
    let detail = value
        .get("error")
        .filter(|value| value.is_object())
        .unwrap_or(value);
    let code = detail["code"].as_u64();
    if event != "error"
        && !code.is_some_and(|code| code >= 4000 || code == 1005)
        && !value.get("error").is_some_and(|value| !value.is_null())
    {
        return None;
    }
    let (status, model_scope, hint) = match code {
        Some(1005) => (
            StatusCode::PAYMENT_REQUIRED,
            true,
            "This model requires a different Trae subscription",
        ),
        Some(4008) => (
            StatusCode::PAYMENT_REQUIRED,
            false,
            "Trae account quota is exhausted",
        ),
        Some(4011) => (
            StatusCode::TOO_MANY_REQUESTS,
            false,
            "Trae account is rate limited",
        ),
        Some(4001 | 4023) => (
            StatusCode::NOT_FOUND,
            true,
            "Trae does not serve this model on the selected route",
        ),
        _ => (StatusCode::BAD_GATEWAY, false, "Trae refused the request"),
    };
    let mut message = hint.to_string();
    if let Some(code) = code {
        message.push_str(&format!(" (Trae code {code})"));
    }
    let extra = if let Some(text) = detail["extra"].as_str() {
        serde_json::from_str::<Value>(text).unwrap_or(Value::Null)
    } else {
        detail["extra"].clone()
    };
    if let Some(plan) = extra["plan"].as_u64() {
        message.push_str(&format!("; required plan {plan}"));
    }
    if let Some(text) = detail["message"].as_str().filter(|text| !text.is_empty()) {
        message.push_str(": ");
        message.extend(text.chars().take(800));
    }
    let mut error = ProxyError::new(status, message);
    error.error_code = code.map(|code| format!("trae_{code}"));
    error.model_scope = model_scope;
    error.retry_after = headers.get(header::RETRY_AFTER).cloned();
    Some(error)
}

async fn catalog(
    provider: AccountProvider,
    credentials: &Value,
    _client: &reqwest::Client,
) -> Result<Catalog, ProxyError> {
    let client = http_client()?;
    let base = base(provider, credentials)?;
    // 与 Trae 客户端 model_list_by_function 使用的 function 集合保持一致。
    // solo_agent / solo_agent_lite / builder 等分支承载了新发布的模型，
    // 只查旧的那几个会让整代模型在目录里缺失。
    let mut functions = vec![
        "solo_work_remote",
        "solo_work_lite",
        "solo_agent",
        "solo_agent_lite",
        "solo_agent_remote",
        "solo_design_lite",
        "solo_design_remote",
        "builder",
        "chat_v3",
    ];
    if provider == AccountProvider::Traecn {
        functions.push("solo_coder");
    }
    let requests = functions.iter().map(|function| {
        let request = client.post(format!("{base}{MODEL_PATH}")).json(&json!({"function":function,"config_names":null,"need_prompt":false,"current_config_info":null,"poly_prompt":true,"mode_type":null,"agent_type":null})).timeout(Duration::from_secs(30));
        async move {
            let request_headers = headers(credentials, true)?;
            #[cfg(test)]
            let revision = request_headers.get("x-ide-version-code").and_then(|value| value.to_str().ok()).unwrap_or("absent").to_string();
            let response = request.headers(request_headers).send().await.map_err(ProxyError::upstream)?;
            let status = response.status();
            let response_headers = response.headers().clone();
            if status.is_redirection() { return Err(ProxyError::new(StatusCode::BAD_GATEWAY, "Trae model directory returned an unexpected redirect")); }
            if !status.is_success() { return Err(ProxyError::new(status, "Trae model directory request failed")); }
            let value = response.json::<Value>().await.map_err(ProxyError::upstream)?;
            #[cfg(test)]
            if std::env::var("ECHOBIRD_LIVE_TRAE_CATALOG_DIAGNOSTICS").as_deref() == Ok("1") {
                for row in value["config_info_list"].as_array().into_iter().flatten().filter(|row| matches!(row["config_name"].as_str(), Some("doubao-for-auto" | "DeepSeek-V4-Flash" | "glm-5"))) {
                    eprintln!("Trae catalog function={function} revision={revision} shape={}", catalog_row_shape(row));
                }
            }
            if let Some(error) = failure("", &value, &response_headers) { return Err(error); }
            Ok(value)
        }
    });
    let results = futures_util::future::join_all(requests).await;
    let mut models = Catalog::new();
    let mut last_error = None;
    for (function, result) in functions.into_iter().zip(results) {
        let value = match result {
            Ok(value) => value,
            Err(error)
                if matches!(
                    error.status,
                    StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN
                ) =>
            {
                return Err(error)
            }
            Err(error) => {
                last_error = Some(error);
                continue;
            }
        };
        for row in value["config_info_list"].as_array().into_iter().flatten() {
            let Some(id) = row["config_name"].as_str().filter(|id| !id.is_empty()) else {
                continue;
            };
            if !visible_chat_model(id)
                || row["hidden"] == true
                || row["disabled"] == true
                || row["is_invisible_to_user"] == true
                || row["config_switch"] == false
                || row["usage"].as_str() == Some("custom_model")
            {
                continue;
            }
            let display = display_config(row);
            let detail = &row["model_detail_list"][0];
            let reasoning = reasoning_options(row)
                .or_else(|| reasoning_options(detail))
                .unwrap_or_default();
            models.entry(id.into()).or_insert_with(|| Model {
                function: function.into(),
                max_tokens: native_output_limit(row),
                multimodal: row["multimodal"] == true
                    || detail["multimodal"] == true
                    || display["multimodal"] == true
                    || row["metadata"]["capabilities"]["vision"] == true,
                reasoning,
            });
        }
    }
    if models.is_empty() {
        return Err(last_error.unwrap_or_else(|| {
            ProxyError::new(
                StatusCode::BAD_GATEWAY,
                "Trae model directory returned no models",
            )
        }));
    }
    let mut saved = cache()
        .lock()
        .map_err(|_| ProxyError::upstream("Trae model cache is unavailable"))?;
    saved.insert(key(provider, credentials, &base)?, models.clone());
    #[cfg(not(test))]
    if crate::services::cursor_auth::write(&cache_path(), &*saved).is_err() {
        log::warn!("[TraeProxy] Could not persist the model directory");
    }
    Ok(models)
}

pub(super) async fn list_models(
    provider: AccountProvider,
    credentials: Value,
    client: &reqwest::Client,
) -> Result<Vec<String>, ProxyError> {
    Ok(catalog(provider, &credentials, client)
        .await?
        .into_keys()
        .collect())
}

fn prepare(body: &Value, id: &str, model: &Model) -> Result<Value, ProxyError> {
    for parameter in [
        "temperature",
        "top_p",
        "frequency_penalty",
        "presence_penalty",
        "stop",
        "seed",
        "logprobs",
        "top_logprobs",
        "response_format",
        "functions",
        "function_call",
    ] {
        if body.get(parameter).is_some_and(|value| !value.is_null()) {
            return Err(unsupported(parameter));
        }
    }
    for parameter in ["max_tokens", "max_completion_tokens"] {
        let Some(limit) = body.get(parameter).filter(|value| !value.is_null()) else {
            continue;
        };
        let limit = limit
            .as_u64()
            .filter(|value| *value > 0)
            .ok_or_else(|| unsupported(parameter))?;
        if !model.max_tokens.is_some_and(|native| native <= limit) {
            return Err(unsupported(
                "max_tokens below the model's native output limit",
            ));
        }
    }
    if body
        .get("n")
        .is_some_and(|value| !value.is_null() && value != 1)
    {
        return Err(unsupported("multiple completions"));
    }
    if body["parallel_tool_calls"] == false {
        return Err(unsupported("parallel_tool_calls=false"));
    }
    if body
        .get("text")
        .is_some_and(|value| !value.is_null() && value["format"]["type"] != "text")
    {
        return Err(unsupported("structured text format"));
    }
    let tools_allowed = match body.get("tool_choice").filter(|value| !value.is_null()) {
        None => true,
        Some(value) if value == "auto" => true,
        Some(value) if value == "none" => false,
        Some(_) => return Err(unsupported("forced tool_choice")),
    };
    let mut messages = body["messages"].as_array().cloned().ok_or_else(|| {
        ProxyError::new(StatusCode::BAD_REQUEST, "Trae messages must be an array")
    })?;
    for message in &mut messages {
        if message["role"] == "developer" {
            message["role"] = json!("system");
        }
        if !matches!(
            message["role"].as_str(),
            Some("system" | "assistant" | "user" | "tool" | "function")
        ) {
            return Err(unsupported("message role"));
        }
        if let Some(content) = message["content"].as_str() {
            message["content"] = json!([{"type":"text","text":content}]);
        }
        if message["content"].is_null() {
            message["content"] = json!([]);
        }
        if !message["content"].is_array() {
            return Err(ProxyError::new(
                StatusCode::BAD_REQUEST,
                "Trae message content must be text or an array",
            ));
        }
        for part in message["content"].as_array().into_iter().flatten() {
            match part["type"].as_str() {
                Some("text") => {}
                Some("image_url") if model.multimodal => {}
                Some("image_url") => return Err(unsupported("image input")),
                _ => return Err(unsupported("message content type")),
            }
        }
        if message["role"] == "tool"
            && !message["tool_call_id"]
                .as_str()
                .is_some_and(|id| !id.is_empty())
        {
            return Err(ProxyError::new(
                StatusCode::BAD_REQUEST,
                "Trae tool results require tool_call_id",
            ));
        }
        for call in message
            .get_mut("tool_calls")
            .and_then(Value::as_array_mut)
            .into_iter()
            .flatten()
        {
            let function = call
                .as_object_mut()
                .and_then(|object| object.remove("function"))
                .ok_or_else(|| {
                    ProxyError::new(
                        StatusCode::BAD_REQUEST,
                        "Trae tool history requires function",
                    )
                })?;
            call["function_call"] = function;
        }
    }
    let mut output = json!({"messages":messages,"model":id,"config_name":id,"function":model.function,"stream":true});
    if tools_allowed {
        let mut tools = body["tools"].as_array().cloned().unwrap_or_default();
        for tool in &mut tools {
            if tool["type"] != "function" || tool["function"]["strict"] == true {
                return Err(unsupported("strict or non-function tools"));
            }
            if !tool["function"]["name"]
                .as_str()
                .is_some_and(|name| !name.is_empty())
            {
                return Err(ProxyError::new(
                    StatusCode::BAD_REQUEST,
                    "Trae tool name is required",
                ));
            }
            let parameters = tool["function"]
                .get("parameters")
                .cloned()
                .unwrap_or_else(|| json!({"type":"object","properties":{}}));
            if !parameters.is_object() {
                return Err(ProxyError::new(
                    StatusCode::BAD_REQUEST,
                    "Trae tool parameters must be a JSON schema object",
                ));
            }
            tool["function"]["parameters"] = json!(parameters.to_string());
        }
        if !tools.is_empty() {
            output["tools"] = json!(tools);
        }
    }
    if let Some(effort) = body["reasoning_effort"]
        .as_str()
        .or_else(|| body["reasoning"]["effort"].as_str())
    {
        match model.reasoning.iter().find(|value| *value == effort) {
            Some(_) => output["reasoning_effort"] = json!(effort),
            // 模型未声明该档位时静默丢弃，而不是让整个请求失败。
            // 目录数据可能滞后于服务端能力，客户端也会发来通用档位名
            // （low/medium/max 等），硬拒绝会让本可成功的请求全部 400。
            None => {
                if model.reasoning.is_empty() {
                    log::debug!(
                        "[TraeProxy] dropping reasoning_effort={effort}: model declares no levels"
                    );
                } else {
                    log::debug!(
                        "[TraeProxy] dropping unknown reasoning_effort={effort}, supported: {:?}",
                        model.reasoning
                    );
                }
            }
        }
    }
    Ok(output)
}

pub(super) async fn route(
    provider: AccountProvider,
    credentials: Value,
    model: &str,
    mut body: Value,
    client: &reqwest::Client,
) -> Result<Response, ProxyError> {
    if !visible_chat_model(model) {
        let mut error = unsupported("an internal model configuration");
        error.status = StatusCode::NOT_FOUND;
        error.error_code = Some("model_not_supported".into());
        return Err(error);
    }
    let base = base(provider, &credentials)?;
    let key = key(provider, &credentials, &base)?;
    let cached = cache()
        .lock()
        .map_err(|_| ProxyError::upstream("Trae model cache is unavailable"))?
        .get(&key)
        .and_then(|catalog| catalog.get(model))
        .cloned();
    let metadata = match cached {
        Some(metadata) => metadata,
        None => catalog(provider, &credentials, client)
            .await?
            .get(model)
            .cloned()
            .ok_or_else(|| {
                let mut error = unsupported("the requested model");
                error.status = StatusCode::NOT_FOUND;
                error.error_code = Some("model_not_supported".into());
                error
            })?,
    };
    let streaming = body["stream"].as_bool().unwrap_or(false);
    let tools_allowed = body["tool_choice"] != "none";
    super::strip_response_metadata(&mut body);
    let request = prepare(&body, model, &metadata)?;
    #[cfg(test)]
    let request = {
        let mut request = request;
        if std::env::var("ECHOBIRD_LIVE_TRAE_PHYSICAL_MODEL").as_deref() == Ok("1")
            && matches!(model, "doubao-for-auto" | "glm-5" | "DeepSeek-V4-Flash")
        {
            request["model"] = json!(format!("{model}__dev"));
        }
        if std::env::var("ECHOBIRD_LIVE_TRAE_REQUEST_CONTEXT").as_deref() == Ok("1") {
            let context = uuid::Uuid::new_v4().to_string();
            request["request_id"] = json!(context);
            request["session_id"] = json!(context);
        }
        if let Ok(function) = std::env::var("ECHOBIRD_LIVE_TRAE_FUNCTION") {
            if matches!(
                function.as_str(),
                "solo_work_lite" | "solo_coder" | "chat_v3" | "inline_chat"
            ) {
                request["function"] = json!(function);
            }
        }
        if let Some(limit) = std::env::var("ECHOBIRD_LIVE_TRAE_MAX_TOKENS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .filter(|value| (1..=32).contains(value))
        {
            request["max_tokens"] = json!(limit);
        }
        request
    };
    #[cfg(test)]
    if stream_diagnostics_enabled() {
        eprintln!(
            "Trae stream request model={model} function={} native_output_limit={:?} request_id_present={} session_id_present={}",
            request["function"].as_str().unwrap_or_default(), metadata.max_tokens,
            request.get("request_id").is_some(), request.get("session_id").is_some()
        );
    }
    #[cfg(test)]
    let request_started = std::time::Instant::now();
    let response = http_client()?
        .post(format!("{base}{CHAT_PATH}"))
        .headers(headers(&credentials, false)?)
        .json(&request)
        .send()
        .await
        .map_err(ProxyError::upstream)?;
    let status = response.status();
    #[cfg(test)]
    if stream_diagnostics_enabled() {
        let content_type = match response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.split(';').next())
        {
            Some("text/event-stream") => "text/event-stream",
            Some("application/json") => "application/json",
            None => "missing",
            _ => "other",
        };
        eprintln!(
            "Trae stream headers status={} content_type={content_type} elapsed_ms={}",
            status.as_u16(),
            request_started.elapsed().as_millis()
        );
    }
    if status.is_redirection() {
        return Err(ProxyError::new(
            StatusCode::BAD_GATEWAY,
            "Trae account request returned an unexpected redirect",
        ));
    }
    if !status.is_success() {
        let response_headers = response.headers().clone();
        let value = response.json::<Value>().await.unwrap_or(Value::Null);
        let mut error = if value["code"].is_u64()
            && !matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN)
        {
            failure("error", &value, &response_headers)
                .unwrap_or_else(|| ProxyError::new(status, "Trae account request failed"))
        } else {
            ProxyError::new(status, "Trae account request failed")
        };
        error.retry_after = response_headers.get(header::RETRY_AFTER).cloned();
        return Err(error);
    }
    providers::converted(
        response,
        WireProtocol::Trae,
        model,
        streaming,
        tools_allowed,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::super::{
        protocol::{ChatDecoder, SseParser},
        stream_response,
    };
    use super::*;
    use axum::{
        body::{to_bytes, Body},
        extract::{Request, State},
        routing::post,
        Router,
    };
    use bytes::Bytes;
    use futures_util::{stream, StreamExt};
    use std::sync::Arc;

    type Captures = Arc<tokio::sync::Mutex<Vec<(String, HeaderMap, Value)>>>;
    const INTERNAL_FIXTURE_IDS: &[&str] = &[
        "custom_model_native",
        "search_agent",
        "search_agent_web",
        "fast_apply",
        "fast_apply_new",
        "file_search_agent",
        "summary",
        "title_generation",
        "input_optimization",
        "laziness_detection",
    ];

    fn credentials(provider: AccountProvider, base: &str) -> Value {
        json!({"accessToken":"fixture-access","userId":uuid::Uuid::new_v4().to_string(),"deviceId":"saved-device","machineId":"saved-machine","appVersion":"3.3.83","region":if provider == AccountProvider::Traecn {"cn"} else {"sg"},"baseUrl":base})
    }

    fn metadata() -> Model {
        Model {
            function: "solo_work_remote".into(),
            max_tokens: Some(32000),
            multimodal: false,
            reasoning: vec!["high".into()],
        }
    }

    async fn fixture() -> (String, Captures) {
        let captures = Captures::default();
        async fn handler(State(captures): State<Captures>, request: Request) -> Response {
            let path = request.uri().path().to_string();
            let headers = request.headers().clone();
            let bytes = to_bytes(request.into_body(), 1024 * 1024).await.unwrap();
            let body: Value = serde_json::from_slice(&bytes).unwrap();
            captures
                .lock()
                .await
                .push((path.clone(), headers.clone(), body.clone()));
            if path == MODEL_PATH {
                if headers["x-uid"] == "auth-failure" {
                    return Response::builder().status(401).body(Body::empty()).unwrap();
                }
                let function = if headers["x-uid"] == "other-account" {
                    "chat_v3"
                } else {
                    "solo_work_remote"
                };
                let rows = if body["function"] == function {
                    let mut rows = json!([
                        {"config_name":"test-model","model_detail_list":[{"max_tokens":32000,"reasoning_effort_options":["high"]}]},
                        {"config_name":"vision-model","multimodal":true,"model_detail_list":[{"max_tokens":32000}]},
                        {"config_name":"early-error","model_detail_list":[{"max_tokens":32000}]},
                        {"config_name":"truncated","model_detail_list":[{"max_tokens":32000}]},
                        {"config_name":"tool-model","model_detail_list":[{"max_tokens":32000}]},
                        {"config_name":"gpt-5.2","model_detail_list":[{"max_tokens":32000}]},
                        {"config_name":"glm-5","model_detail_list":[{"max_tokens":32000}]},
                        {"config_name":"paygo"},
                        {"config_name":"auto"},
                        {"config_name":"advisor"},
                        {"config_name":"hidden-model","hidden":true},
                        {"config_name":"disabled-model","disabled":true}
                    ]);
                    rows.as_array_mut()
                        .unwrap()
                        .extend(INTERNAL_FIXTURE_IDS.iter().map(
                        |id| json!({"config_name":id,"model_detail_list":[{"max_tokens":32000}]}),
                    ));
                    rows
                } else {
                    json!([])
                };
                return super::super::json_response(&json!({"config_info_list":rows}));
            }
            if body["model"] == "early-error" {
                return stream_response(Body::from("event: metadata\ndata: {}\n\nevent: request_wait_in_queue\ndata: {\"position\":1}\n\nevent: error\ndata: {\"code\":4011,\"message\":\"fixture rate limit\"}\n\n"));
            }
            if body["model"] == "truncated" {
                return stream_response(Body::from(
                    "event: output\ndata: {\"response\":\"partial\"}\n\n",
                ));
            }
            let output = if body["model"] == "tool-model"
                && (body["tools"].is_array()
                    || body["messages"][0]["content"][0]["text"] == "force-tool")
            {
                json!({"response":"", "tool_calls":[{"index":0,"id":"call_fixture","type":"function","function_call":{"name":"echo_probe","arguments":"{\"text\":\"fixture\"}"}}]})
            } else if body["messages"]
                .as_array()
                .unwrap()
                .iter()
                .any(|message| message["role"] == "tool")
            {
                json!({"response":"tool-result-consumed"})
            } else {
                json!({"response":"你好🌍"})
            };
            let data = format!("event: metadata\ndata: {{}}\n\nevent: output\ndata: {output}\n\nevent: token_usage\ndata: {{\"prompt_tokens\":10,\"completion_tokens\":3,\"total_tokens\":13,\"cache_read_input_tokens\":8}}\n\nevent: done\ndata: {{\"finish_reason\":\"stop\"}}\n\n");
            let bytes = data
                .into_bytes()
                .into_iter()
                .map(|byte| Ok::<_, std::io::Error>(Bytes::from(vec![byte])));
            stream_response(Body::from_stream(stream::iter(bytes)))
        }
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let router = Router::new()
            .route(MODEL_PATH, post(handler))
            .route(CHAT_PATH, post(handler))
            .with_state(captures.clone());
        tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        (base, captures)
    }

    #[test]
    fn stream_diagnostics_expose_structure_without_private_values() {
        let payload = json!({"response":"private-content","reasoning_content":"private-reasoning","message":"private-message","identity":"private-identity","token_usage":{"prompt_tokens":712345},"code":4008,"finish_reason":"private-reason"});
        let shape = stream_event_shape("output", &payload.to_string());
        assert_eq!(shape["event"], "output");
        assert_eq!(shape["code"], 4008);
        assert_eq!(shape["finish_reason_present"], true);
        assert_eq!(shape["output_present"], true);
        let rendered = shape.to_string();
        for secret in [
            "private-content",
            "private-reasoning",
            "private-message",
            "private-identity",
            "712345",
            "private-reason",
        ] {
            assert!(!rendered.contains(secret));
        }
        assert_eq!(
            stream_event_shape("private-event-value", "{}")["event"],
            "other"
        );
        assert_eq!(stream_event_shape("done", "{}")["done_event"], true);
    }

    #[test]
    fn progress_diagnostics_distinguish_payload_types_without_printing_notices() {
        for (payload, kind) in [
            (
                json!("private-notice queue busy waiting").to_string(),
                "string",
            ),
            ("private-notice 排队 等待 资源紧张".to_string(), "non_json"),
            (
                json!({"message":"private-notice queue busy waiting"}).to_string(),
                "object",
            ),
        ] {
            let shape = stream_event_shape("progress_notice", &payload);
            assert_eq!(shape["payload_type"], kind);
            for key in ["queue_hint", "busy_hint", "wait_hint"] {
                assert_eq!(shape[key], true);
            }
            assert!(!shape.to_string().contains("private-notice"));
        }
        let null = stream_event_shape("progress_notice", "null");
        assert_eq!(null["payload_type"], "null");
        assert_eq!(null["queue_hint"], false);
        assert_eq!(null["wait_hint"], false);
        let output =
            stream_event_shape("output", &json!({"response":"queue busy wait"}).to_string());
        for key in ["queue_hint", "busy_hint", "wait_hint"] {
            assert_eq!(output[key], false);
        }
    }

    #[test]
    fn terminal_error_diagnostics_only_expose_approved_hints() {
        let data = json!({"code":4051,"message":"private-error model function invalid unsupported capacity queue timeout","identity":"private-identity"}).to_string();
        let shape = stream_error_shape("error", &data, "private-decoder-message");
        assert_eq!(shape["code"], 4051);
        assert_eq!(
            stream_error_shape(
                "error",
                &json!({"error":{"code":"4051"}}).to_string(),
                "private-error"
            )["code"],
            4051
        );
        for hint in [
            "model",
            "function",
            "invalid",
            "unsupported",
            "capacity",
            "queue",
            "timeout",
        ] {
            assert_eq!(shape["error_hints"][hint], true);
        }
        for secret in [
            "private-error",
            "private-identity",
            "private-decoder-message",
        ] {
            assert!(!shape.to_string().contains(secret));
        }
    }

    #[test]
    fn inner_error_diagnostics_only_expose_known_codes_status_and_hints() {
        for (status, code, message) in [
            (500, "ModelLoading", "private-upstream-message capacity"),
            (
                400,
                "InvalidParameter",
                "private-upstream-message reasoning thinking disabled invalid",
            ),
            (400, "private-error-code", "private-upstream-message"),
        ] {
            let extra = json!({"status_code":status,"body":json!({"error":{"code":code,"message":message},"api_key":"private-key","identity":"private-identity"}).to_string()}).to_string();
            let shape = stream_error_shape(
                "error",
                &json!({"code":3003,"extra":extra}).to_string(),
                "private-decoder-message",
            );
            assert_eq!(shape["inner_error"]["status"], status);
            if code.starts_with("private-") {
                assert!(shape["inner_error"]["code"].is_null());
            } else {
                assert_eq!(shape["inner_error"]["code"], code);
            }
            for hint in ["reasoning", "thinking", "disabled", "invalid"] {
                assert_eq!(
                    shape["inner_error"]["hints"][hint],
                    code == "InvalidParameter"
                );
            }
            assert_eq!(
                shape["inner_error"]["hints"]["capacity"],
                code == "ModelLoading"
            );
            for private in [
                "private-upstream-message",
                "private-error-code",
                "private-key",
                "private-identity",
                "private-decoder-message",
            ] {
                assert!(!shape.to_string().contains(private));
            }
        }
    }

    #[test]
    fn catalog_diagnostics_only_expose_target_structure_and_safe_usage_enums() {
        let row = json!({"config_name":"doubao-for-auto","usage":"chat_completion","hidden":false,"disabled":true,
            "custom_models":["private-model"],"model_detail_list":[{"model_name":"glm-5","name":"private name value","model":"https://private-url","api_key":"private-key","prompt":"private-prompt"}],
            "display_config":{"display_name":"private-display"},"extra_config":"private-extra","account_id":"private-id"});
        let shape = catalog_row_shape(&row);
        assert_eq!(shape["usage_type"], "string");
        assert_eq!(shape["usage_enum"], "chat_completion");
        assert_eq!(shape["usage_chat_completion"], true);
        assert_eq!(shape["custom_models_count"], 1);
        assert_eq!(shape["detail_count"], 1);
        assert_eq!(
            shape["details"][0]["identifiers"],
            json!({"model_name":"glm-5"})
        );
        assert_eq!(shape["display_fields"], json!(["display_name"]));
        assert_eq!(shape["disabled"], true);
        for secret in [
            "private-model",
            "private-key",
            "private-prompt",
            "private-display",
            "private-extra",
            "private-id",
            "private name value",
            "private-url",
        ] {
            assert!(!shape.to_string().contains(secret));
        }
        let mut unsafe_usage = row;
        unsafe_usage["usage"] = json!("private usage value 712345");
        let shape = catalog_row_shape(&unsafe_usage);
        assert!(shape["usage_enum"].is_null());
        assert_eq!(shape["usage_other"], true);
        assert!(!shape.to_string().contains("private usage value"));
    }

    #[test]
    fn headers_keep_saved_identity_and_unique_request_ids_and_region_is_pinned() {
        let mut credentials = credentials(AccountProvider::Trae, "");
        credentials.as_object_mut().unwrap().remove("baseUrl");
        let first = headers(&credentials, false).unwrap();
        let second = headers(&credentials, true).unwrap();
        assert_eq!(first["authorization"], "Cloud-IDE-JWT fixture-access");
        assert_eq!(first["x-device-id"], "saved-device");
        assert_eq!(first["x-machine-id"], "saved-machine");
        assert_eq!(first["x-app-version"], "3.3.83");
        assert_eq!(first["x-ide-version-code"], "20260212");
        assert!(!first.contains_key("x-app-version-code"));
        assert_eq!(first["x-request-id"], first["x-trae-request-id"]);
        assert_ne!(first["x-request-id"], second["x-request-id"]);
        assert_eq!(
            base(AccountProvider::Trae, &credentials).unwrap(),
            "https://coresg-normal.trae.ai"
        );
        credentials["region"] = json!("us");
        assert_eq!(
            base(AccountProvider::Trae, &credentials).unwrap(),
            "https://coresg-normal.trae.ai"
        );
        credentials["region"] = json!("usttp");
        assert_eq!(
            base(AccountProvider::Trae, &credentials).unwrap(),
            "https://core-normal.traeapi.us"
        );
        credentials["buildVersion"] = json!("3.3.83");
        assert_eq!(
            headers(&credentials, false).unwrap()["x-ide-version-code"],
            "20260212"
        );
        assert!(!headers(&credentials, false)
            .unwrap()
            .contains_key("x-app-version-code"));
        credentials["region"] = json!("cn");
        assert_eq!(
            base(AccountProvider::Traecn, &credentials).unwrap(),
            "https://trae-api-cn.mchost.guru"
        );
        assert!(base(AccountProvider::Trae, &credentials).is_err());
        credentials["accessToken"] = json!("invalid\nheader");
        assert!(headers(&credentials, false).is_err());
    }

    #[test]
    fn request_preserves_tool_schema_history_content_and_supported_reasoning() {
        let body = json!({"messages":[{"role":"developer","content":"system"},{"role":"assistant","tool_calls":[{"id":"call_fixture","function":{"name":"echo_probe","arguments":"{}"}}]},{"role":"tool","tool_call_id":"call_fixture","content":"result"}],"tools":[{"type":"function","function":{"name":"echo_probe","parameters":{"type":"object","properties":{"text":{"type":"string"}},"required":["text"]}}}],"reasoning_effort":"high"});
        let output = prepare(&body, "wire-id", &metadata()).unwrap();
        assert_eq!(output["model"], "wire-id");
        assert_eq!(output["config_name"], "wire-id");
        assert_eq!(output["function"], "solo_work_remote");
        assert_eq!(
            output["messages"][0],
            json!({"role":"system","content":[{"type":"text","text":"system"}]})
        );
        assert_eq!(
            output["messages"][1]["tool_calls"][0]["function_call"]["name"],
            "echo_probe"
        );
        assert_eq!(output["messages"][2]["tool_call_id"], "call_fixture");
        assert_eq!(
            serde_json::from_str::<Value>(
                output["tools"][0]["function"]["parameters"]
                    .as_str()
                    .unwrap()
            )
            .unwrap(),
            body["tools"][0]["function"]["parameters"]
        );
        assert_eq!(output["reasoning_effort"], "high");
        assert_eq!(body["messages"][0]["role"], "developer");
        let without_tools = prepare(
            &json!({"messages":[],"tools":body["tools"],"tool_choice":"none"}),
            "wire-id",
            &metadata(),
        )
        .unwrap();
        assert!(without_tools.get("tools").is_none());
        assert!(prepare(
            &json!({"messages":[{"role":"tool","content":"orphan"}]}),
            "wire-id",
            &metadata()
        )
        .is_err());
    }

    #[test]
    fn unsupported_strict_parameters_are_visible_and_native_output_limit_is_respected() {
        for (name, value) in [
            ("temperature", json!(0.2)),
            ("parallel_tool_calls", json!(false)),
            ("tool_choice", json!("required")),
            ("response_format", json!({"type":"json_object"})),
            ("max_tokens", json!(128)),
        ] {
            let mut body = json!({"messages":[]});
            body[name] = value;
            let error = prepare(&body, "wire-id", &metadata()).unwrap_err();
            assert_eq!(error.error_code.as_deref(), Some("unsupported_parameter"));
            assert!(error.model_scope);
        }
        // 未知/缺省档位不再让请求失败，而是静默丢弃该参数。
        for effort in ["low", "medium", "max", "none"] {
            let body = json!({"messages":[],"reasoning_effort":effort});
            let output = prepare(&body, "wire-id", &metadata()).unwrap();
            assert!(
                output.get("reasoning_effort").is_none(),
                "unknown effort {effort} must be dropped"
            );
        }
        let body = json!({"messages":[],"max_tokens":32000});
        let output = prepare(&body, "wire-id", &metadata()).unwrap();
        assert!(output.get("max_tokens").is_none());
        let mut unknown = metadata();
        unknown.max_tokens = None;
        assert!(prepare(&body, "wire-id", &unknown).is_err());
        let image = json!({"messages":[{"role":"user","content":[{"type":"image_url","image_url":{"url":"data:image/png;base64,fixture"}}]}]});
        assert!(prepare(&image, "wire-id", &metadata()).is_err());
        unknown.multimodal = true;
        assert_eq!(
            prepare(&image, "wire-id", &unknown).unwrap()["messages"],
            image["messages"]
        );
    }

    #[test]
    fn reasoning_options_reads_current_config_and_legacy_field() {
        // 新版本：support_thinking + options
        assert_eq!(
            reasoning_options(&json!({
                "reasoning_effort_config": {
                    "support_thinking": true,
                    "options": ["light", "high", "extra_high"],
                    "default_level": "high"
                }
            })),
            Some(vec!["light".into(), "high".into(), "extra_high".into()])
        );
        // support_thinking=false 时应回退到 legacy 字段；两者都没有则 None
        assert_eq!(
            reasoning_options(&json!({
                "reasoning_effort_config": {"support_thinking": false, "options": ["high"]}
            })),
            None
        );
        // 旧版本字段仍兼容
        assert_eq!(
            reasoning_options(&json!({"reasoning_effort_options": ["high"]})),
            Some(vec!["high".into()])
        );
        // 空数组视为未声明
        assert_eq!(reasoning_options(&json!({"reasoning_effort_options": []})), None);
        assert_eq!(reasoning_options(&json!({})), None);
    }

    #[test]
    fn display_config_accepts_object_and_json_string() {
        assert_eq!(
            display_config(&json!({"display_config":{"multimodal":true}}))["multimodal"],
            true
        );
        assert_eq!(
            display_config(&json!({"display_config":"{\"multimodal\":true}"}))["multimodal"],
            true
        );
        assert!(display_config(&json!({})).is_null());
        assert!(display_config(&json!({"display_config":"not-json"})).is_null());
    }

    #[test]
    fn native_output_limit_requires_every_detail_to_declare_a_positive_integer() {
        assert_eq!(
            native_output_limit(
                &json!({"model_detail_list":[{"max_tokens":32000},{"max_tokens":64000}]})
            ),
            Some(64000)
        );
        for detail in [
            json!({}),
            json!({"max_tokens":null}),
            json!({"max_tokens":0}),
            json!({"max_tokens":-1}),
            json!({"max_tokens":1.5}),
        ] {
            let row = json!({"model_detail_list":[{"max_tokens":32000},detail]});
            let mut model = metadata();
            model.max_tokens = native_output_limit(&row);
            assert_eq!(model.max_tokens, None);
            let error = prepare(
                &json!({"messages":[],"max_tokens":64000}),
                "wire-id",
                &model,
            )
            .unwrap_err();
            assert_eq!(error.error_code.as_deref(), Some("unsupported_parameter"));
            assert!(error.model_scope);
        }
        assert_eq!(native_output_limit(&json!({})), None);
        assert_eq!(native_output_limit(&json!({"model_detail_list":[]})), None);
    }

    #[test]
    fn failure_codes_keep_plan_quota_rate_and_model_scopes_distinct() {
        for (code, status, model_scope) in [
            (1005, 402, true),
            (4008, 402, false),
            (4011, 429, false),
            (4001, 404, true),
            (4023, 404, true),
        ] {
            let error = stream_error(
                "error",
                &json!({"code":code,"extra":"{\"plan\":4}","message":"fixture refusal"})
                    .to_string(),
                &HeaderMap::new(),
            )
            .unwrap();
            assert_eq!(error.status.as_u16(), status);
            assert_eq!(error.model_scope, model_scope);
            assert!(error.message.contains("required plan 4"));
            assert_eq!(error.error_code, Some(format!("trae_{code}")));
        }
        assert!(stream_error("metadata", "{}", &HeaderMap::new()).is_none());
        assert!(stream_error("", "{\"code\":4008}", &HeaderMap::new()).is_some());
    }

    #[tokio::test]
    async fn directory_and_chat_redirects_never_forward_account_tokens() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        async fn target(State(hits): State<Arc<AtomicUsize>>) -> Response {
            hits.fetch_add(1, Ordering::SeqCst);
            super::super::json_response(&json!({"fixture":"target"}))
        }
        async fn redirect(State((status, location)): State<(StatusCode, String)>) -> Response {
            Response::builder()
                .status(status)
                .header(header::LOCATION, location)
                .body(Body::empty())
                .unwrap()
        }
        let hits = Arc::new(AtomicUsize::new(0));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let target_url = format!("http://{}/target", listener.local_addr().unwrap());
        let router = Router::new()
            .route("/target", axum::routing::any(target))
            .with_state(hits.clone());
        let target_task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let client = reqwest::Client::new();
        client.post(&target_url).send().await.unwrap();
        assert_eq!(hits.swap(0, Ordering::SeqCst), 1);
        for status in [
            StatusCode::MOVED_PERMANENTLY,
            StatusCode::TEMPORARY_REDIRECT,
        ] {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let base = format!("http://{}", listener.local_addr().unwrap());
            let router = Router::new()
                .route(MODEL_PATH, post(redirect))
                .route(CHAT_PATH, post(redirect))
                .with_state((status, target_url.clone()));
            let source_task =
                tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
            let credentials = credentials(AccountProvider::Trae, &base);
            let error = list_models(AccountProvider::Trae, credentials.clone(), &client)
                .await
                .unwrap_err();
            assert_eq!(error.status, StatusCode::BAD_GATEWAY);
            assert!(error.message.contains("redirect"));
            cache().lock().unwrap().insert(
                key(AccountProvider::Trae, &credentials, &base).unwrap(),
                BTreeMap::from([("test-model".into(), metadata())]),
            );
            let error = route(
                AccountProvider::Trae,
                credentials,
                "test-model",
                json!({"messages":[{"role":"user","content":"fixture"}]}),
                &client,
            )
            .await
            .unwrap_err();
            assert_eq!(error.status, StatusCode::BAD_GATEWAY);
            assert!(error.message.contains("redirect"));
            assert_eq!(hits.load(Ordering::SeqCst), 0);
            source_task.abort();
        }
        target_task.abort();
    }

    #[tokio::test]
    async fn account_region_catalogs_are_isolated_and_function_metadata_is_reused() {
        let (base, captures) = fixture().await;
        let client = reqwest::Client::new();
        let first = credentials(AccountProvider::Trae, &base);
        let first_key = key(AccountProvider::Trae, &first, &base).unwrap();
        assert!(list_models(AccountProvider::Trae, first.clone(), &client)
            .await
            .unwrap()
            .contains(&"test-model".into()));
        let response = route(
            AccountProvider::Trae,
            first.clone(),
            "test-model",
            json!({"messages":[{"role":"user","content":"hello"}],"stream":false}),
            &client,
        )
        .await
        .unwrap();
        let output: Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 1024 * 1024).await.unwrap())
                .unwrap();
        assert_eq!(output["choices"][0]["message"]["content"], "你好🌍");
        assert_eq!(output["usage"]["prompt_tokens"], 10);
        assert_eq!(output["usage"]["prompt_tokens_details"]["cached_tokens"], 8);
        assert_eq!(
            captures
                .lock()
                .await
                .iter()
                .filter(|capture| capture.0 == MODEL_PATH)
                .count(),
            4
        );
        let mut other = first.clone();
        other["userId"] = json!("other-account");
        let response = route(
            AccountProvider::Trae,
            other,
            "test-model",
            json!({"messages":[],"stream":false}),
            &client,
        )
        .await
        .unwrap();
        to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        let mut cn = first;
        cn["region"] = json!("cn");
        list_models(AccountProvider::Traecn, cn, &client)
            .await
            .unwrap();
        let requests = captures.lock().await;
        let chats: Vec<_> = requests
            .iter()
            .filter(|request| request.0 == CHAT_PATH)
            .collect();
        assert_eq!(chats[0].2["function"], "solo_work_remote");
        assert_eq!(chats[1].2["function"], "chat_v3");
        assert_eq!(
            requests
                .iter()
                .filter(|request| request.0 == MODEL_PATH)
                .count(),
            13
        );
        let serialized = serde_json::to_string(&*cache().lock().unwrap()).unwrap();
        let restored: Catalogs = serde_json::from_str(&serialized).unwrap();
        assert_eq!(
            restored[&first_key]["test-model"].function,
            "solo_work_remote"
        );
    }

    #[tokio::test]
    async fn chat_directory_filters_internal_configs_and_rejects_legacy_cached_entries() {
        let (base, captures) = fixture().await;
        let client = reqwest::Client::new();
        for provider in [AccountProvider::Trae, AccountProvider::Traecn] {
            let credentials = credentials(provider, &base);
            let models = list_models(provider, credentials.clone(), &client)
                .await
                .unwrap();
            for id in ["gpt-5.2", "glm-5", "paygo", "auto", "advisor"] {
                assert!(models.iter().any(|model| model == id));
            }
            for id in INTERNAL_FIXTURE_IDS
                .iter()
                .copied()
                .chain(["hidden-model", "disabled-model"])
            {
                assert!(!models.iter().any(|model| model == id));
            }
            let key = key(provider, &credentials, &base).unwrap();
            let mut saved = cache().lock().unwrap().get(&key).unwrap().clone();
            for id in INTERNAL_FIXTURE_IDS {
                saved.insert((*id).into(), metadata());
            }
            let restored: Catalog =
                serde_json::from_str(&serde_json::to_string(&saved).unwrap()).unwrap();
            cache().lock().unwrap().insert(key, restored);
            let before = captures.lock().await.len();
            for id in INTERNAL_FIXTURE_IDS {
                let error = route(
                    provider,
                    credentials.clone(),
                    id,
                    json!({"messages":[]}),
                    &client,
                )
                .await
                .unwrap_err();
                assert_eq!(error.status, StatusCode::NOT_FOUND);
                assert_eq!(error.error_code.as_deref(), Some("model_not_supported"));
                assert!(error.model_scope);
            }
            assert_eq!(captures.lock().await.len(), before);
            for id in ["gpt-5.2", "glm-5"] {
                let response = route(
                    provider,
                    credentials.clone(),
                    id,
                    json!({"messages":[]}),
                    &client,
                )
                .await
                .unwrap();
                let value: Value = serde_json::from_slice(
                    &to_bytes(response.into_body(), 1024 * 1024).await.unwrap(),
                )
                .unwrap();
                assert_eq!(value["choices"][0]["message"]["content"], "你好🌍");
            }
        }
    }

    #[tokio::test]
    async fn early_errors_and_unfinished_streams_never_gain_a_success_terminal() {
        let (base, _) = fixture().await;
        let client = reqwest::Client::new();
        let credentials = credentials(AccountProvider::Trae, &base);
        let response = route(
            AccountProvider::Trae,
            credentials.clone(),
            "early-error",
            json!({"messages":[],"stream":true}),
            &client,
        )
        .await
        .unwrap();
        let mut stream = response.into_body().into_data_stream();
        let error = stream.next().await.unwrap().unwrap_err();
        assert!(error.to_string().contains("rate limited"));
        let response = route(
            AccountProvider::Trae,
            credentials,
            "truncated",
            json!({"messages":[],"stream":false}),
            &client,
        )
        .await
        .unwrap();
        assert!(to_bytes(response.into_body(), 1024 * 1024).await.is_err());
    }

    #[tokio::test]
    async fn tool_calls_and_results_complete_two_real_adapter_turns() {
        let (base, captures) = fixture().await;
        let client = reqwest::Client::new();
        let credentials = credentials(AccountProvider::Traecn, &base);
        let tools = json!([{"type":"function","function":{"name":"echo_probe","parameters":{"type":"object","properties":{"text":{"type":"string"}}}}}]);
        let response = route(AccountProvider::Traecn,credentials.clone(),"tool-model",json!({"messages":[{"role":"user","content":"use echo_probe"}],"tools":tools,"stream":false}),&client).await.unwrap();
        let first: Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 1024 * 1024).await.unwrap())
                .unwrap();
        assert_eq!(first["choices"][0]["finish_reason"], "tool_calls");
        let assistant = first["choices"][0]["message"].clone();
        assert_eq!(assistant["tool_calls"][0]["id"], "call_fixture");
        let response = route(AccountProvider::Traecn,credentials,"tool-model",json!({"messages":[{"role":"user","content":"use echo_probe"},assistant,{"role":"tool","tool_call_id":"call_fixture","content":"fixture-result"}],"tool_choice":"none","stream":false}),&client).await.unwrap();
        let result: Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 1024 * 1024).await.unwrap())
                .unwrap();
        assert_eq!(
            result["choices"][0]["message"]["content"],
            "tool-result-consumed"
        );
        let requests = captures.lock().await;
        let last = &requests.last().unwrap().2;
        assert_eq!(
            last["messages"][1]["tool_calls"][0]["function_call"]["name"],
            "echo_probe"
        );
        assert_eq!(last["messages"][2]["content"][0]["text"], "fixture-result");
        assert!(last.get("tools").is_none());
    }

    #[tokio::test]
    async fn shared_responses_and_anthropic_converters_reuse_the_verified_chat_path() {
        let (base, _) = fixture().await;
        let client = reqwest::Client::new();
        let credentials = credentials(AccountProvider::Traecn, &base);
        let input = json!({"model":"test-model","input":[{"type":"message","role":"user","content":[{"type":"input_text","text":"hello"}]}],"stream":false});
        assert!(super::super::route_responses(
            AccountProvider::Traecn,
            credentials.clone(),
            "test-model",
            input.clone(),
            &client
        )
        .await
        .unwrap()
        .is_none());
        let chat = super::super::responses_to_chat(&input).unwrap();
        let response = route(
            AccountProvider::Traecn,
            credentials.clone(),
            "test-model",
            chat,
            &client,
        )
        .await
        .unwrap();
        let response = super::super::chat_to_responses(response, false).await;
        let output: Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 1024 * 1024).await.unwrap())
                .unwrap();
        assert_eq!(output["status"], "completed");
        assert_eq!(output["output"][0]["content"][0]["text"], "你好🌍");
        let chat = crate::services::local_llm::proxy::anthropic_to_openai(
            &json!({"model":"test-model","max_tokens":32000,"messages":[{"role":"user","content":"hello"}],"stream":false}),
        );
        let response = route(
            AccountProvider::Traecn,
            credentials,
            "test-model",
            chat,
            &client,
        )
        .await
        .unwrap();
        let output: Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 1024 * 1024).await.unwrap())
                .unwrap();
        let anthropic = crate::services::local_llm::proxy::openai_to_anthropic(&output);
        assert_eq!(anthropic["content"][0]["text"], "你好🌍");
        assert_eq!(anthropic["stop_reason"], "end_turn");
    }

    #[tokio::test]
    async fn none_tool_choice_is_enforced_and_directory_auth_errors_remain_auth_errors() {
        let (base, _) = fixture().await;
        let client = reqwest::Client::new();
        let mut credentials = credentials(AccountProvider::Trae, &base);
        let response = route(AccountProvider::Trae, credentials.clone(), "tool-model", json!({"messages":[{"role":"user","content":"force-tool"}],"tool_choice":"none","stream":true}), &client).await.unwrap();
        let mut output = response.into_body().into_data_stream();
        let error = output.next().await.unwrap().unwrap_err();
        assert!(error.to_string().contains("tool_choice=none"));
        credentials["userId"] = json!("auth-failure");
        assert_eq!(
            list_models(AccountProvider::Trae, credentials, &client)
                .await
                .unwrap_err()
                .status,
            StatusCode::UNAUTHORIZED
        );
    }

    #[test]
    fn tool_fragments_use_call_identity_without_merging_separate_id_only_calls() {
        let mut decoder = ChatDecoder::new(WireProtocol::Trae, "test-model");
        for value in [
            json!({"tool_calls":[{"id":"call_a","function_call":{"name":"read","arguments":"{\"p\":"}}]}),
            json!({"tool_calls":[{"id":"call_b","function_call":{"name":"write","arguments":"{}"}}]}),
            json!({"tool_calls":[{"id":"call_a","function_call":{"arguments":"1}"}}]}),
        ] {
            decoder.decode("output", &value.to_string()).unwrap();
        }
        decoder.decode("done", "{}").unwrap();
        let result = decoder.collected();
        let calls = result["choices"][0]["message"]["tool_calls"]
            .as_array()
            .unwrap();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0]["id"], "call_a");
        assert_eq!(calls[0]["function"]["name"], "read");
        assert_eq!(calls[0]["function"]["arguments"], "{\"p\":1}");
        assert_eq!(calls[1]["id"], "call_b");
        assert_eq!(calls[1]["function"]["arguments"], "{}");

        let mut decoder = ChatDecoder::new(WireProtocol::Trae, "test-model");
        decoder.decode("output", &json!({"tool_calls":[{"id":"call_a","function_call":{"name":"read","arguments":"{"}}]}).to_string()).unwrap();
        decoder.decode("output", &json!({"tool_calls":[{"index":0,"id":"call_a","function_call":{"arguments":"\"p\":"}}]}).to_string()).unwrap();
        decoder
            .decode(
                "output",
                &json!({"tool_calls":[{"index":0,"function_call":{"arguments":"1}"}}]}).to_string(),
            )
            .unwrap();
        decoder.decode("done", "{}").unwrap();
        assert_eq!(
            decoder.collected()["choices"][0]["message"]["tool_calls"][0]["function"]["arguments"],
            "{\"p\":1}"
        );
    }

    #[test]
    fn ambiguous_tool_fragments_are_rejected_instead_of_executing_mixed_arguments() {
        let mut decoder = ChatDecoder::new(WireProtocol::Trae, "test-model");
        decoder.decode("output", &json!({"tool_calls":[{"id":"call_a","function_call":{"name":"read","arguments":"{}"}},{"id":"call_b","function_call":{"name":"write","arguments":"{}"}}]}).to_string()).unwrap();
        assert!(decoder
            .decode(
                "output",
                &json!({"tool_calls":[{"function_call":{"arguments":"{}"}}]}).to_string()
            )
            .is_err());
        let mut decoder = ChatDecoder::new(WireProtocol::Trae, "test-model");
        decoder.decode("output", &json!({"tool_calls":[{"index":0,"id":"call_a","function_call":{"name":"read","arguments":"{}"}}]}).to_string()).unwrap();
        assert!(decoder.decode("output", &json!({"tool_calls":[{"index":0,"id":"call_b","function_call":{"name":"write","arguments":"{}"}}]}).to_string()).is_err());
    }

    #[test]
    fn named_events_preserve_split_utf8_tool_deltas_and_explicit_completion() {
        let wire = concat!("event: metadata\r\ndata: {}\r\n\r\n", "event: output\r\ndata: {\"response\":\"你好🌍\"}\r\n\r\n", "event: output\r\ndata: {\"tool_calls\":[{\"index\":0,\"id\":\"call_fixture\",\"function_call\":{\"name\":\"echo_probe\",\"arguments\":\"{\\\"text\\\":\"}}]}\r\n\r\n", "event: output\r\ndata: {\"tool_calls\":[{\"index\":0,\"function_call\":{\"arguments\":\"\\\"fixture\\\"}\"}}]}\r\n\r\n", "event: done\r\ndata: {\"finish_reason\":\"stop\"}\r\n\r\n");
        let mut parser = SseParser::default();
        let mut decoder = ChatDecoder::new(WireProtocol::Trae, "test-model");
        let mut output = Vec::new();
        for byte in wire.bytes() {
            for (event, data) in parser.push(&[byte]) {
                output.extend(decoder.decode(&event, &data).unwrap());
            }
        }
        assert!(decoder.finished);
        let collected = decoder.collected();
        assert_eq!(collected["choices"][0]["message"]["content"], "你好🌍");
        assert_eq!(
            collected["choices"][0]["message"]["tool_calls"][0]["function"]["arguments"],
            "{\"text\":\"fixture\"}"
        );
        assert_eq!(
            output
                .iter()
                .filter(|bytes| bytes.as_ref() == b"data: [DONE]\n\n")
                .count(),
            1
        );
        let mut decoder = ChatDecoder::new(WireProtocol::Trae, "test-model");
        assert!(decoder.decode("metadata", "{}").unwrap().is_empty());
        assert!(decoder
            .decode("request_wait_in_queue", "{\"position\":1}")
            .unwrap()
            .is_empty());
        assert!(!decoder.finished);
        decoder.decode("output","{\"tool_calls\":[{\"id\":\"bad\",\"function_call\":{\"name\":\"echo_probe\",\"arguments\":\"{\"}}]}").unwrap();
        assert!(decoder.decode("done", "{}").is_err());
    }
}
