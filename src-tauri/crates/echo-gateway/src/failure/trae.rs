//! Trae's own stream error events and codes.

use axum::http::{header, HeaderMap, StatusCode};
use serde_json::Value;

use crate::error::ProxyError;

pub fn stream_error(event: &str, data: &str, headers: &HeaderMap) -> Option<ProxyError> {
    let value: Value = serde_json::from_str(data).ok()?;
    failure(event, &value, headers)
}

pub fn failure(event: &str, value: &Value, headers: &HeaderMap) -> Option<ProxyError> {
    let detail = value
        .get("error")
        .filter(|value| value.is_object())
        .unwrap_or(value);
    let code = detail["code"]
        .as_u64()
        .or_else(|| detail["code"].as_str()?.parse().ok());
    if event != "error"
        && !code.is_some_and(|code| code >= 4000 || code == 1005)
        && value.get("error").is_none_or(|value| value.is_null())
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
