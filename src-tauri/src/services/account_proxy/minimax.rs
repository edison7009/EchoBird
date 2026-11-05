use super::{
    protocol::{self, WireProtocol},
    providers, ProxyError,
};
use axum::{
    http::{header, StatusCode},
    response::Response,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{BTreeSet, HashMap},
    sync::{Mutex, OnceLock},
    time::Duration,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ModelMetadata {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) context: Option<u64>,
    pub(crate) output: Option<u64>,
    pub(crate) images: bool,
    pub(crate) thinking_mode: Option<String>,
    pub(crate) efforts: Vec<String>,
}

static CATALOGS: OnceLock<Mutex<HashMap<String, Vec<ModelMetadata>>>> = OnceLock::new();

fn client() -> Result<&'static reqwest::Client, ProxyError> {
    static CLIENT: OnceLock<Result<reqwest::Client, String>> = OnceLock::new();
    CLIENT
        .get_or_init(|| {
            reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .connect_timeout(Duration::from_secs(15))
                .build()
                .map_err(|error| error.to_string())
        })
        .as_ref()
        .map_err(ProxyError::upstream)
}

pub(super) fn base(credentials: &Value) -> Result<String, ProxyError> {
    let region = credentials["region"].as_str().ok_or_else(|| {
        ProxyError::new(StatusCode::UNAUTHORIZED, "MiniMax account has no region")
    })?;
    let expected = match region {
        "cn" => "https://agent.minimax.cn",
        "global" => "https://agent.minimax.io",
        _ => {
            return Err(ProxyError::new(
                StatusCode::UNAUTHORIZED,
                "Invalid MiniMax account region",
            ))
        }
    };
    let supplied = credentials["baseUrl"].as_str().unwrap_or(expected);
    #[cfg(test)]
    if let Ok(url) = url::Url::parse(supplied) {
        if url.scheme() == "http" && url.host_str() == Some("127.0.0.1") {
            return Ok(supplied.trim_end_matches('/').into());
        }
    }
    if supplied.trim_end_matches('/') != expected {
        return Err(ProxyError::new(
            StatusCode::UNAUTHORIZED,
            "MiniMax account region does not match its endpoint",
        ));
    }
    Ok(expected.into())
}

fn signed(
    request: reqwest::RequestBuilder,
    credentials: &Value,
    session: Option<&str>,
) -> Result<reqwest::RequestBuilder, ProxyError> {
    let token = credentials["accessToken"]
        .as_str()
        .filter(|token| !token.trim().is_empty())
        .ok_or_else(|| {
            ProxyError::new(
                StatusCode::UNAUTHORIZED,
                "MiniMax account has no access token",
            )
        })?;
    let session = session
        .map(str::to_string)
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    Ok(request
        .bearer_auth(token)
        .header("x-api-key", "sk-xxx")
        .header("user-agent", "MiniMaxAgent")
        .header("x-mavis-agent-id", "main")
        .header(
            "x-mavis-timezone-offset",
            chrono::Local::now().offset().local_minus_utc(),
        )
        .header("x-mavis-session-id", session)
        .header("anthropic-version", "2023-06-01"))
}

fn cache_key(credentials: &Value) -> Result<String, ProxyError> {
    Ok(format!(
        "{}\0{}\0{}",
        base(credentials)?,
        credentials["region"].as_str().unwrap_or_default(),
        credentials["accountId"].as_str().unwrap_or_default()
    ))
}

pub(crate) fn parse_models(value: &Value) -> Vec<ModelMetadata> {
    let Some(provider) = value["providers"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|provider| provider["providerId"] == "minimax")
    else {
        return Vec::new();
    };
    let config = &provider["config"];
    let Some(models) = config["models"].as_object() else {
        return Vec::new();
    };
    let mut ids: Vec<&str> = config["model_order"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .filter(|id| models.contains_key(*id))
        .collect();
    ids.extend(models.keys().map(String::as_str).filter(|id| {
        !config["model_order"]
            .as_array()
            .is_some_and(|order| order.iter().any(|entry| entry.as_str() == Some(*id)))
    }));
    let mut seen = BTreeSet::new();
    ids.into_iter()
        .filter(|id| !id.trim().is_empty() && seen.insert(*id))
        .filter_map(|id| {
            let model = &models[id];
            if !model.is_object() || model["enabled"] == false || model["disabled"] == true {
                return None;
            }
            let efforts = model["thinking"]
                .get("effortOptions")
                .or_else(|| model["thinking"].get("effort_options"))
                .or_else(|| model.get("effort_options"))
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .filter(|effort| !effort.is_empty() && !matches!(*effort, "default" | "none"))
                .map(str::to_string)
                .collect();
            Some(ModelMetadata {
                id: id.into(),
                name: model["name"].as_str().unwrap_or(id).into(),
                context: model["limit"]["context"]
                    .as_u64()
                    .or_else(|| model["limit"]["context"].as_str()?.parse().ok()),
                output: model["limit"]["output"]
                    .as_u64()
                    .or_else(|| model["limit"]["output"].as_str()?.parse().ok()),
                images: model["attachment"] == true
                    || model["modalities"]["input"]
                        .as_array()
                        .is_some_and(|inputs| inputs.iter().any(|input| input == "image")),
                thinking_mode: model["thinking_config"]["mode"]
                    .as_str()
                    .map(str::to_string),
                efforts,
            })
        })
        .collect()
}

pub(super) async fn list_models(
    credentials: Value,
    _client: &reqwest::Client,
) -> Result<Vec<String>, ProxyError> {
    let models = list_model_details(credentials, _client).await?;
    Ok(models.into_iter().map(|model| model.id).collect())
}

pub(crate) async fn list_model_details(
    credentials: Value,
    _client: &reqwest::Client,
) -> Result<Vec<ModelMetadata>, ProxyError> {
    let api = base(&credentials)?;
    let client = client()?;
    let token = credentials["accessToken"]
        .as_str()
        .filter(|token| !token.is_empty())
        .ok_or_else(|| {
            ProxyError::new(
                StatusCode::UNAUTHORIZED,
                "MiniMax account has no access token",
            )
        })?;
    let region = if credentials["region"] == "cn" {
        "cn"
    } else {
        "en"
    };
    let response = client
        .get(format!("{api}/mavis/api/v1/models"))
        .query(&[("region", region), ("buildEnv", "prod")])
        .bearer_auth(token)
        .header("accept", "application/json")
        .timeout(Duration::from_secs(15))
        .send()
        .await
        .map_err(ProxyError::upstream)?;
    if response.status().is_redirection() {
        return Err(ProxyError::new(
            StatusCode::BAD_GATEWAY,
            "MiniMax model directory returned an unexpected redirect",
        ));
    }
    if !response.status().is_success() {
        let status = response.status();
        let headers = response.headers().clone();
        let body = response.json::<Value>().await.unwrap_or(Value::Null);
        return Err(providers::upstream_error(status, &headers, &body));
    }
    let body = response
        .json::<Value>()
        .await
        .map_err(ProxyError::upstream)?;
    let models = parse_models(&body);
    if models.is_empty() {
        return Err(ProxyError::new(
            StatusCode::BAD_GATEWAY,
            "MiniMax model directory has no available models",
        ));
    }
    CATALOGS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .map_err(|_| ProxyError::new(StatusCode::SERVICE_UNAVAILABLE, "Model directory is busy"))?
        .insert(cache_key(&credentials)?, models.clone());
    Ok(models)
}

fn unsupported(parameter: &str) -> ProxyError {
    let mut error = ProxyError::new(
        StatusCode::BAD_REQUEST,
        format!("MiniMax does not support request parameter: {parameter}"),
    );
    error.error_code = Some("unsupported_parameter".into());
    error.model_scope = true;
    error
}

fn request(credentials: &Value, model: &str, body: &Value) -> Result<Value, ProxyError> {
    protocol::reject_structured_output(body)?;
    for parameter in ["frequency_penalty", "presence_penalty", "top_logprobs"] {
        if body
            .get(parameter)
            .is_some_and(|value| !value.is_null() && value.as_f64() != Some(0.0))
        {
            return Err(unsupported(parameter));
        }
    }
    if body
        .get("logprobs")
        .is_some_and(|value| !value.is_null() && value != false)
    {
        return Err(unsupported("logprobs"));
    }
    for parameter in ["seed", "function_call", "prediction", "audio"] {
        if body.get(parameter).is_some_and(|value| !value.is_null()) {
            return Err(unsupported(parameter));
        }
    }
    if body
        .get("functions")
        .is_some_and(|value| !value.is_null() && !value.as_array().is_some_and(Vec::is_empty))
    {
        return Err(unsupported("functions"));
    }
    if body.get("logit_bias").is_some_and(|value| {
        !value.is_null() && !value.as_object().is_some_and(|value| value.is_empty())
    }) {
        return Err(unsupported("logit_bias"));
    }
    if body
        .get("modalities")
        .is_some_and(|value| !value.is_null() && *value != json!(["text"]))
    {
        return Err(unsupported("modalities"));
    }
    for parameter in ["stop", "stop_sequences"] {
        if let Some(value) = body.get(parameter).filter(|value| !value.is_null()) {
            if value.as_str().is_some_and(|value| value.is_empty())
                || value.as_array().is_some_and(Vec::is_empty)
            {
                continue;
            }
            return Err(unsupported(parameter));
        }
    }
    if body
        .get("n")
        .filter(|value| !value.is_null())
        .is_some_and(|value| value.as_u64() != Some(1))
    {
        return Err(unsupported("n"));
    }
    if let Some(tools) = body.get("tools").filter(|value| !value.is_null()) {
        let tools = tools.as_array().ok_or_else(|| unsupported("tools"))?;
        if tools.iter().any(|tool| {
            tool["type"] != "function"
                || tool["function"]["strict"] == true
                || tool["function"]["name"]
                    .as_str()
                    .is_none_or(|name| name.trim().is_empty())
        }) {
            return Err(unsupported("tools"));
        }
    }
    let mut normalized = body.clone();
    for message in normalized["messages"]
        .as_array_mut()
        .ok_or_else(|| unsupported("messages"))?
    {
        let role = message["role"]
            .as_str()
            .filter(|role| {
                matches!(
                    *role,
                    "system" | "developer" | "user" | "assistant" | "tool"
                )
            })
            .ok_or_else(|| unsupported("messages.role"))?;
        let content = &message["content"];
        if content.is_null() || content.is_string() {
            continue;
        }
        let blocks = content
            .as_array()
            .ok_or_else(|| unsupported("messages.content"))?;
        for block in blocks {
            match block["type"].as_str() {
                Some("text") if block["text"].is_string() => {}
                Some("image_url")
                    if role == "user"
                        && block["image_url"]["url"].as_str().is_some_and(|url| {
                            !url.is_empty()
                                && (!url.starts_with("data:")
                                    || url[5..].split_once(";base64,").is_some())
                        }) => {}
                _ => return Err(unsupported("messages.content attachment")),
            }
        }
        if role == "assistant" {
            message["content"] = Value::String(
                blocks
                    .iter()
                    .filter_map(|block| block["text"].as_str())
                    .collect(),
            );
        }
    }
    let mut request = providers::anthropic_request(&normalized, model);
    if let Some(maximum) = body["max_completion_tokens"]
        .as_u64()
        .or_else(|| body["max_tokens"].as_u64())
    {
        request["max_tokens"] = json!(maximum);
    }
    for name in ["temperature", "top_p", "thinking", "output_config"] {
        if let Some(value) = body.get(name) {
            request[name] = value.clone();
        }
    }
    if let Some(effort) = body["reasoning_effort"].as_str() {
        if effort == "none" {
            request["thinking"] = json!({"type":"disabled"});
        } else {
            request["thinking"] = json!({"type":"adaptive"});
            if !request["output_config"].is_object() {
                request["output_config"] = json!({});
            }
            request["output_config"]["effort"] = json!(effort);
        }
    }
    let metadata = CATALOGS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .map_err(|_| ProxyError::new(StatusCode::SERVICE_UNAVAILABLE, "Model directory is busy"))?
        .get(&cache_key(credentials)?)
        .and_then(|models| models.iter().find(|entry| entry.id == model))
        .cloned();
    if let Some(metadata) = metadata {
        if metadata.thinking_mode.as_deref() == Some("forced_on")
            && request["thinking"]["type"] == "disabled"
        {
            return Err(ProxyError::new(
                StatusCode::BAD_REQUEST,
                "Selected model requires thinking",
            ));
        }
        if let Some(effort) = request["output_config"]["effort"].as_str() {
            if !metadata.efforts.is_empty() && !metadata.efforts.iter().any(|entry| entry == effort)
            {
                return Err(ProxyError::new(
                    StatusCode::BAD_REQUEST,
                    "Selected model does not support this reasoning effort",
                ));
            }
        }
    }
    Ok(request)
}

pub(super) async fn route(
    credentials: Value,
    model: &str,
    body: Value,
    _client: &reqwest::Client,
) -> Result<Response, ProxyError> {
    let streaming = body["stream"].as_bool().unwrap_or(false);
    let session = body["metadata"]["session_id"]
        .as_str()
        .or_else(|| body["user"].as_str());
    let api = base(&credentials)?;
    let response = signed(
        client()?
            .post(format!("{api}/mavis/api/v1/llm/v1/messages"))
            .header("accept", "text/event-stream")
            .json(&request(&credentials, model, &body)?),
        &credentials,
        session,
    )?
    .send()
    .await
    .map_err(ProxyError::upstream)?;
    let status = response.status();
    if status.is_redirection() {
        return Err(ProxyError::new(
            StatusCode::BAD_GATEWAY,
            "MiniMax account request returned an unexpected redirect",
        ));
    }
    if !status.is_success() {
        let headers = response.headers().clone();
        let body = response.json::<Value>().await.unwrap_or(Value::Null);
        let mut error = providers::upstream_error(status, &headers, &body);
        let message = error.message.to_ascii_lowercase();
        if matches!(status, StatusCode::PAYMENT_REQUIRED | StatusCode::FORBIDDEN)
            && [
                "insufficient",
                "balance",
                "credit",
                "quota",
                "exhaust",
                "余额",
                "积分",
                "额度",
                "不足",
                "用完",
            ]
            .iter()
            .any(|word| message.contains(word))
        {
            error.status = StatusCode::TOO_MANY_REQUESTS;
            error.error_type = Some("rate_limit_error".into());
        }
        error.retry_after = headers
            .get(header::RETRY_AFTER)
            .cloned()
            .or(error.retry_after);
        return Err(error);
    }
    providers::converted(response, WireProtocol::Anthropic, model, streaming, true).await
}

#[cfg(test)]
#[path = "minimax_tests.rs"]
mod tests;
