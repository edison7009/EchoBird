use super::{protocol, providers, tools, ProxyError};
use axum::{
    body::Body,
    http::{header, StatusCode},
    response::Response,
};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

const BASE: &str = "https://cli-chat-proxy.grok.com/v1";
const CACHE_LIFETIME: Duration = Duration::from_secs(300);

#[derive(Clone)]
struct Model {
    id: String,
    context: Option<u64>,
}
type Catalog = BTreeMap<String, Model>;
type CachedCatalogs = BTreeMap<String, (Instant, Catalog)>;

fn catalogs() -> &'static Mutex<CachedCatalogs> {
    static CACHE: OnceLock<Mutex<CachedCatalogs>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(BTreeMap::new()))
}

fn unsupported(message: impl ToString) -> ProxyError {
    let mut error = ProxyError::new(StatusCode::BAD_REQUEST, message.to_string());
    error.error_code = Some("unsupported_parameter".into());
    error.model_scope = true;
    error
}

fn required<'a>(credentials: &'a Value, field: &str) -> Result<&'a str, ProxyError> {
    credentials[field]
        .as_str()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            ProxyError::new(
                StatusCode::UNAUTHORIZED,
                "Grok account credentials are incomplete",
            )
        })
}

pub(super) fn base(credentials: &Value) -> Result<String, ProxyError> {
    #[cfg(test)]
    if let Some(base) = credentials["baseUrl"].as_str() {
        let url = url::Url::parse(base).map_err(ProxyError::upstream)?;
        if url.scheme() == "http" && url.host_str() == Some("127.0.0.1") {
            return Ok(base.trim_end_matches('/').into());
        }
        return Err(ProxyError::upstream("Fixture endpoint must be loopback"));
    }
    let _ = credentials;
    Ok(BASE.into())
}

fn cache_key(credentials: &Value) -> Result<String, ProxyError> {
    Ok(json!([
        base(credentials)?,
        required(credentials, "accountId")?,
        required(credentials, "userId")?,
        credentials["authScope"],
        credentials["principalType"],
        credentials["principalId"]
    ])
    .to_string())
}

fn client() -> Result<&'static reqwest::Client, ProxyError> {
    static CLIENT: OnceLock<Result<reqwest::Client, String>> = OnceLock::new();
    CLIENT
        .get_or_init(|| {
            reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .connect_timeout(Duration::from_secs(15))
                .build()
                .map_err(|_| "Could not create Grok client".into())
        })
        .as_ref()
        .map_err(ProxyError::upstream)
}

fn parse_version(output: &str) -> Option<String> {
    output.split_whitespace().find_map(|word| {
        let version = word.strip_prefix('v').unwrap_or(word);
        let core = version.split(['-', '+']).next()?;
        (version.len() <= 64
            && version
                .bytes()
                .all(|ch| ch.is_ascii_alphanumeric() || b".-+".contains(&ch))
            && core.split('.').count() == 3
            && core
                .split('.')
                .all(|part| !part.is_empty() && part.bytes().all(|ch| ch.is_ascii_digit())))
        .then(|| version.to_string())
    })
}

#[cfg(not(test))]
async fn client_version() -> Option<String> {
    type CachedVersion = Option<(Instant, String, Option<String>)>;
    static VERSION: OnceLock<tokio::sync::Mutex<CachedVersion>> = OnceLock::new();
    let path = crate::services::tool_manager::get_tool_exe_path("grok")?;
    if !std::path::Path::new(&path).is_file() {
        return None;
    }
    let mut cached = VERSION
        .get_or_init(|| tokio::sync::Mutex::new(None))
        .lock()
        .await;
    if let Some((at, executable, value)) = cached.as_ref() {
        if executable == &path && at.elapsed() < CACHE_LIFETIME {
            return value.clone();
        }
    }
    let mut command = crate::utils::process::async_command(&path);
    command.arg("--version").kill_on_drop(true);
    let value = match tokio::time::timeout(Duration::from_secs(3), command.output()).await {
        Ok(Ok(output)) if output.status.success() && output.stdout.len() <= 4096 => {
            parse_version(&String::from_utf8_lossy(&output.stdout))
        }
        _ => None,
    };
    *cached = Some((Instant::now(), path, value.clone()));
    value
}

async fn signed(
    request: reqwest::RequestBuilder,
    credentials: &Value,
) -> Result<reqwest::RequestBuilder, ProxyError> {
    let mut request = request
        .bearer_auth(required(credentials, "accessToken")?)
        .header("X-XAI-Token-Auth", "xai-grok-cli")
        .header("x-userid", required(credentials, "userId")?)
        .header("x-grok-client-identifier", "EchoBird")
        .header("x-grok-client-mode", "headless")
        .header(
            header::USER_AGENT,
            concat!("EchoBird/", env!("CARGO_PKG_VERSION")),
        );
    if let Some(email) = credentials["email"]
        .as_str()
        .filter(|email| !email.is_empty())
    {
        request = request.header("x-email", email);
    }
    #[cfg(not(test))]
    let version = client_version().await;
    #[cfg(test)]
    let version = credentials["clientVersion"]
        .as_str()
        .and_then(parse_version);
    if let Some(version) = version {
        request = request.header("x-grok-client-version", version);
    }
    Ok(request)
}

fn directory(value: &Value) -> Result<Catalog, ProxyError> {
    let rows = value["data"]
        .as_array()
        .ok_or_else(|| ProxyError::upstream("Invalid Grok model directory"))?;
    let mut models = Catalog::new();
    for row in rows {
        let backend = row.get("api_backend").or_else(|| row.get("apiBackend"));
        if backend.and_then(Value::as_str) != Some("responses")
            || row["disabled"] == true
            || row["enabled"] == false
        {
            continue;
        }
        let Some(id) = ["model", "modelId", "id"]
            .into_iter()
            .find_map(|field| row[field].as_str().filter(|id| !id.trim().is_empty()))
        else {
            continue;
        };
        let listed = row
            .get("context_windows")
            .or_else(|| row.get("contextWindows"))
            .and_then(Value::as_array);
        let context = row
            .get("context_window")
            .or_else(|| row.get("contextWindow"))
            .and_then(Value::as_u64)
            .filter(|value| *value > 0)
            .or_else(|| {
                listed?
                    .iter()
                    .find_map(|value| value.as_u64().filter(|value| *value > 0))
            });
        models.entry(id.into()).or_insert_with(|| Model {
            id: id.into(),
            context,
        });
        for window in listed
            .into_iter()
            .flatten()
            .filter_map(Value::as_u64)
            .filter(|window| *window > 0 && Some(*window) != context)
        {
            let suffix = if window % 1000 == 0 {
                format!("{}k", window / 1000)
            } else {
                window.to_string()
            };
            models
                .entry(format!("{id}@{suffix}"))
                .or_insert_with(|| Model {
                    id: id.into(),
                    context: Some(window),
                });
        }
    }
    Ok(models)
}

pub(super) async fn list_models(
    credentials: Value,
    _client: &reqwest::Client,
) -> Result<Vec<String>, ProxyError> {
    let key = cache_key(&credentials)?;
    let response = signed(
        client()?
            .get(format!("{}/models", base(&credentials)?))
            .timeout(Duration::from_secs(30)),
        &credentials,
    )
    .await?
    .send()
    .await
    .map_err(ProxyError::upstream)?;
    let status = response.status();
    let headers = response.headers().clone();
    let value = response.json::<Value>().await;
    if !status.is_success() {
        return Err(providers::upstream_error(
            status,
            &headers,
            &value.unwrap_or(Value::Null),
        ));
    }
    let models = directory(&value.map_err(ProxyError::upstream)?)?;
    let names = models.keys().cloned().collect();
    if let Ok(mut cache) = catalogs().lock() {
        cache.retain(|_, (at, _)| at.elapsed() < CACHE_LIFETIME);
        cache.insert(key, (Instant::now(), models));
    }
    Ok(names)
}

async fn resolve_model(
    credentials: &Value,
    model: &str,
    client: &reqwest::Client,
) -> Result<Model, ProxyError> {
    let key = cache_key(credentials)?;
    let cached = catalogs().lock().ok().and_then(|cache| {
        cache
            .get(&key)
            .filter(|(at, _)| at.elapsed() < CACHE_LIFETIME)
            .map(|(_, catalog)| catalog.clone())
    });
    let catalog = if let Some(catalog) = cached {
        catalog
    } else {
        list_models(credentials.clone(), client).await?;
        catalogs()
            .lock()
            .map_err(ProxyError::upstream)?
            .get(&key)
            .map(|(_, catalog)| catalog.clone())
            .unwrap_or_default()
    };
    catalog.get(model).cloned().ok_or_else(|| {
        unsupported("This Grok model or context window is not available in the account directory")
    })
}

fn validate_responses(body: &Value) -> Result<(), ProxyError> {
    if !body.is_object() {
        return Err(unsupported("The Responses request must be an object"));
    }
    if let Some(tools) = body.get("tools").filter(|tools| !tools.is_null()) {
        let tools = tools
            .as_array()
            .ok_or_else(|| unsupported("Invalid Responses tools"))?;
        for tool in tools {
            if !matches!(
                tool["type"].as_str(),
                Some("function" | "web_search" | "x_search")
            ) {
                return Err(unsupported(
                    "This Grok account adapter does not support the requested tool type",
                ));
            }
            if tool["type"] == "function"
                && tool["name"].as_str().is_none_or(|name| name.is_empty())
            {
                return Err(unsupported("A function tool must have a name"));
            }
        }
    }
    Ok(())
}

fn chat_request(body: &Value, model: &str) -> Result<Value, ProxyError> {
    if let Some(original) = body.get(tools::RESPONSES_REQUEST) {
        validate_responses(original)?;
        let mut request = original.clone();
        request["model"] = json!(model);
        request["stream"] = json!(true);
        return Ok(request);
    }
    let object = body
        .as_object()
        .ok_or_else(|| unsupported("The Chat request must be an object"))?;
    for (key, value) in object {
        if value.is_null() {
            continue;
        }
        match key.as_str() {
            "model"
            | "messages"
            | "tools"
            | "tool_choice"
            | "parallel_tool_calls"
            | "stream"
            | "stream_options"
            | "response_format"
            | "reasoning"
            | "reasoning_effort"
            | "text"
            | "temperature"
            | "top_p"
            | "max_tokens"
            | "max_completion_tokens"
            | "max_output_tokens"
            | "prompt_cache_key"
            | "metadata"
            | "user"
            | "store"
            | "service_tier"
            | "truncation"
            | "include" => (),
            "n" if value == 1 => (),
            "logprobs" if value == false => (),
            "presence_penalty" | "frequency_penalty" if value.as_f64() == Some(0.0) => (),
            _ => {
                return Err(unsupported(format!(
                    "The Grok Responses adapter cannot preserve Chat parameter {key}"
                )))
            }
        }
    }
    let messages = body["messages"]
        .as_array()
        .ok_or_else(|| unsupported("Chat messages must be an array"))?;
    for message in messages {
        let role = message["role"].as_str().unwrap_or_default();
        if !matches!(role, "system" | "developer" | "user" | "assistant" | "tool")
            || !message["name"].is_null()
            || !message["reasoning_content"].is_null()
            || !message["audio"].is_null()
            || !message["function_call"].is_null()
        {
            return Err(unsupported(
                "The Grok Responses adapter cannot preserve this Chat message",
            ));
        }
        if let Some(parts) = message["content"].as_array() {
            for part in parts {
                let text = matches!(
                    part["type"].as_str(),
                    Some("text" | "input_text" | "output_text")
                );
                if !text
                    && (matches!(role, "system" | "developer")
                        || !matches!(
                            part["type"].as_str(),
                            Some("image_url" | "input_image" | "input_file")
                        ))
                {
                    return Err(unsupported(
                        "This Chat content cannot be converted to Grok Responses",
                    ));
                }
            }
        } else if !message["content"].is_null() && !message["content"].is_string() {
            return Err(unsupported("Invalid Chat message content"));
        }
        for call in message["tool_calls"].as_array().into_iter().flatten() {
            if call["type"] != "function" || !call["function"].is_object() {
                return Err(unsupported(
                    "Only function calls can be converted from Chat",
                ));
            }
        }
    }
    if let Some(tools) = body.get("tools").filter(|tools| !tools.is_null()) {
        for tool in tools
            .as_array()
            .ok_or_else(|| unsupported("Chat tools must be an array"))?
        {
            if tool["type"] != "function" || !tool["function"].is_object() {
                return Err(unsupported(
                    "Only function tools can be converted from Chat",
                ));
            }
        }
    }
    if let Some(choice) = body.get("tool_choice").filter(|choice| choice.is_object()) {
        if choice["type"] != "function" || !choice["function"]["name"].is_string() {
            return Err(unsupported("Unsupported Chat tool choice"));
        }
    }
    if body["stream_options"]
        .as_object()
        .is_some_and(|options| options.keys().any(|key| key != "include_usage"))
    {
        return Err(unsupported("Unsupported Chat stream options"));
    }
    let mut request = protocol::chat_to_responses(body, model);
    for key in [
        "temperature",
        "top_p",
        "max_output_tokens",
        "prompt_cache_key",
        "metadata",
        "user",
        "store",
        "service_tier",
        "truncation",
        "include",
    ] {
        if let Some(value) = body.get(key) {
            request[key] = value.clone();
        }
    }
    for key in ["max_tokens", "max_completion_tokens"] {
        if let Some(value) = body.get(key).filter(|value| !value.is_null()) {
            if request
                .get("max_output_tokens")
                .is_some_and(|existing| existing != value)
            {
                return Err(unsupported("Conflicting Chat output token limits"));
            }
            request["max_output_tokens"] = value.clone();
        }
    }
    validate_responses(&request)?;
    Ok(request)
}

async fn send(
    credentials: &Value,
    model: &Model,
    mut body: Value,
) -> Result<reqwest::Response, ProxyError> {
    body["model"] = json!(model.id);
    let conversation = body["prompt_cache_key"]
        .as_str()
        .filter(|key| !key.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let mut request = signed(
        client()?.post(format!("{}/responses", base(credentials)?)),
        credentials,
    )
    .await?
    .header("x-grok-model-override", &model.id)
    .header("x-grok-conv-id", &conversation)
    .header("x-grok-session-id", &conversation)
    .header("x-grok-req-id", uuid::Uuid::new_v4().to_string())
    .header("x-grok-agent-id", "EchoBird");
    if let Some(context) = model.context {
        request = request.header("x-grok-context-window", context);
    }
    let response = request
        .json(&body)
        .send()
        .await
        .map_err(ProxyError::upstream)?;
    if !response.status().is_success() {
        let status = response.status();
        let headers = response.headers().clone();
        let value = response.json::<Value>().await.unwrap_or(Value::Null);
        let mut error = providers::upstream_error(status, &headers, &value);
        if status == StatusCode::BAD_REQUEST {
            error.model_scope = true;
        }
        return Err(error);
    }
    Ok(response)
}

pub(super) async fn route(
    credentials: Value,
    model: &str,
    body: Value,
    client: &reqwest::Client,
) -> Result<Response, ProxyError> {
    let request = chat_request(&body, model)?;
    let selected = resolve_model(&credentials, model, client).await?;
    let response = send(&credentials, &selected, request).await?;
    providers::converted(
        response,
        protocol::WireProtocol::Responses,
        model,
        body["stream"] == true,
        true,
    )
    .await
}

pub(super) async fn route_responses(
    credentials: Value,
    model: &str,
    mut body: Value,
    client: &reqwest::Client,
) -> Result<Response, ProxyError> {
    validate_responses(&body)?;
    let selected = resolve_model(&credentials, model, client).await?;
    let streaming = body["stream"] == true;
    body["model"] = json!(selected.id);
    let response = send(&credentials, &selected, body).await?;
    let status = response.status();
    let headers = response.headers().clone();
    let mut output = Response::new(Body::from_stream(response.bytes_stream()));
    *output.status_mut() = status;
    *output.headers_mut() = headers;
    providers::responses_response(output, streaming)
}

#[cfg(test)]
#[path = "grok_tests.rs"]
mod tests;
