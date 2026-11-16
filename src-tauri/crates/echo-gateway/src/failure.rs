//! What an upstream failure means: turns error bodies, stream error events and headers into a `ProxyError`
//! with the right status, code, scope and retry time, so routing can tell rate limits, exhausted quotas,
//! oversized contexts and incompatible requests apart.

pub mod trae;
mod workbuddy;

use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use serde_json::{json, Value};

use crate::error::ProxyError;

pub fn upstream_error(status: StatusCode, headers: &HeaderMap, value: &Value) -> ProxyError {
    let mut error = ProxyError::new(status, safe_error(value));
    let detail = value
        .get("error")
        .filter(|error| error.is_object())
        .unwrap_or(value);
    error.error_code = detail["code"]
        .as_str()
        .or_else(|| detail["status"].as_str())
        .or_else(|| detail["error_code"].as_str())
        .map(str::to_string)
        .or_else(|| detail["code"].as_i64().map(|code| code.to_string()));
    error.error_type = detail["type"].as_str().map(str::to_string);
    error.model_scope = detail["scope"] == "model"
        || matches!(
            error.error_code.as_deref(),
            Some("model_not_found" | "model_not_supported" | "tools_not_supported")
        );
    error.retry_after = headers.get(header::RETRY_AFTER).cloned();
    if error.retry_after.is_none() && status == StatusCode::TOO_MANY_REQUESTS {
        if detail["type"] == "usage_limit_reached" {
            let seconds = detail["resets_in_seconds"].as_u64().or_else(|| {
                let reset = detail["resets_at"].as_u64()?;
                let now = chrono::Utc::now().timestamp().max(0) as u64;
                Some(reset.saturating_sub(now))
            });
            error.retry_after =
                seconds.and_then(|seconds| HeaderValue::from_str(&seconds.to_string()).ok());
        }
        if error.retry_after.is_none() {
            let seconds = detail["details"]
                .as_array()
                .into_iter()
                .flatten()
                .find_map(|hint| {
                    if hint["@type"] != "type.googleapis.com/google.rpc.RetryInfo" {
                        return None;
                    }
                    let delay = hint["retryDelay"]
                        .as_str()?
                        .strip_suffix('s')?
                        .parse::<f64>()
                        .ok()?;
                    (delay.is_finite() && delay >= 0.0).then_some(delay.ceil() as u64)
                });
            error.retry_after =
                seconds.and_then(|seconds| HeaderValue::from_str(&seconds.to_string()).ok());
        }
    }
    workbuddy::normalize(error)
}

pub fn context_limit_error(code: Option<&str>, message: &str) -> bool {
    let message = message.to_ascii_lowercase();
    matches!(
        code,
        Some(
            "context_length_exceeded"
                | "context_window_exceeded"
                | "model_context_window_exceeded"
                | "prompt_too_long"
                | "input_too_long"
        )
    ) || [
        "maximum context length",
        "context length exceeded",
        "context window exceeded",
        "prompt is too long",
        "input is too long",
        "exceeds the context window",
    ]
    .iter()
    .any(|pattern| message.contains(pattern))
}

pub fn stream_error(event: &str, data: &str, headers: &HeaderMap) -> Option<ProxyError> {
    let value: Value = serde_json::from_str(data).ok()?;
    let detail = if event == "response.failed" || value["type"] == "response.failed" {
        value
            .pointer("/response/error")
            .filter(|error| !error.is_null())
            .or_else(|| value.get("error").filter(|error| !error.is_null()))
            .unwrap_or(&value)
    } else if (matches!(event, "response.completed" | "response.incomplete")
        || matches!(
            value["type"].as_str(),
            Some("response.completed" | "response.incomplete")
        ))
        && value
            .pointer("/response/error")
            .is_some_and(|error| !error.is_null())
    {
        &value["response"]["error"]
    } else if let Some(error) = value.get("error").filter(|error| !error.is_null()) {
        error
    } else if event == "error"
        || value["type"] == "error"
        || (value["error_code"].is_string() && value["error_msg"].is_string())
    {
        &value
    } else {
        return None;
    };
    let code = detail["code"]
        .as_str()
        .or_else(|| detail["status"].as_str())
        .or_else(|| detail["error_code"].as_str())
        .or_else(|| detail["type"].as_str());
    let status = if context_limit_error(code, &safe_error(&json!({"error":detail}))) {
        StatusCode::BAD_REQUEST
    } else {
        detail["status"]
            .as_u64()
            .and_then(|status| u16::try_from(status).ok())
            .and_then(|status| StatusCode::from_u16(status).ok())
            .filter(|status| status.is_client_error() || status.is_server_error())
            .unwrap_or(match code {
                Some(
                    "rate_limit_exceeded"
                    | "rate_limit_error"
                    | "usage_limit_reached"
                    | "RESOURCE_EXHAUSTED",
                ) => StatusCode::TOO_MANY_REQUESTS,
                Some(
                    "ModelArts.81101" | "ModelArts.81111" | "ModelArts.81114" | "BusyException",
                ) => StatusCode::TOO_MANY_REQUESTS,
                Some("TM.00020005") => StatusCode::BAD_REQUEST,
                Some("insufficient_quota" | "billing_error" | "billing_hard_limit_reached") => {
                    StatusCode::PAYMENT_REQUIRED
                }
                Some(
                    "invalid_api_key" | "authentication_error" | "unauthorized" | "invalid_grant",
                ) => StatusCode::UNAUTHORIZED,
                Some(
                    "model_not_found"
                    | "model_not_supported"
                    | "tools_not_supported"
                    | "response_not_found",
                ) => StatusCode::NOT_FOUND,
                Some(
                    "invalid_previous_response_id"
                    | "unsupported_parameter"
                    | "request_incompatible",
                ) => StatusCode::BAD_REQUEST,
                Some("server_error" | "overloaded_error" | "service_unavailable_error") => {
                    StatusCode::SERVICE_UNAVAILABLE
                }
                _ => StatusCode::BAD_GATEWAY,
            })
    };
    let mut error = upstream_error(status, headers, &json!({"error":detail}));
    if error.retry_after.is_none() {
        error.retry_after = detail["retry_after"]
            .as_str()
            .and_then(|value| HeaderValue::from_str(value).ok());
    }
    Some(error)
}

pub fn safe_error(value: &Value) -> String {
    value["error"]["message"]
        .as_str()
        .or_else(|| value["error"]["msg"].as_str())
        .or_else(|| value["error"]["error_msg"].as_str())
        .or_else(|| value["message"].as_str())
        .or_else(|| value["error_msg"].as_str())
        .or_else(|| value["msg"].as_str())
        .unwrap_or("Account upstream refused the request")
        .chars()
        .take(800)
        .collect()
}
