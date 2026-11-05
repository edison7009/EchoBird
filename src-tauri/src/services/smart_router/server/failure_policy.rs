// Router responsibility; behavior is covered by the server regression suite.
use super::{
    FailureClass, AUTH_OR_BILLING_COOLDOWN, MAX_RETRY_AFTER, MAX_TRANSIENT_COOLDOWN,
    MODEL_UNAVAILABLE_COOLDOWN, RATE_LIMIT_COOLDOWN, TRANSIENT_COOLDOWN,
};
use axum::http::header;
use axum::http::HeaderMap;
use axum::http::StatusCode;
use serde_json::Value;
use std::time::Duration;
use std::time::Instant;

pub(super) fn cooldown_for_failure(
    failure: FailureClass,
    retry_after: Option<Duration>,
    consecutive_failures: u32,
) -> Duration {
    match failure {
        FailureClass::Authentication => AUTH_OR_BILLING_COOLDOWN,
        FailureClass::ModelUnavailable => MODEL_UNAVAILABLE_COOLDOWN,
        FailureClass::RateLimit | FailureClass::Billing => retry_after
            .unwrap_or(RATE_LIMIT_COOLDOWN)
            .min(MAX_RETRY_AFTER),
        FailureClass::Transient => {
            let multiplier = 1_u32 << consecutive_failures.saturating_sub(1).min(3);
            TRANSIENT_COOLDOWN
                .saturating_mul(multiplier)
                .min(MAX_TRANSIENT_COOLDOWN)
        }
        FailureClass::RequestIncompatible => Duration::ZERO,
    }
}

/// Time already spent waiting comes off the next candidate; a quick refusal costs nothing.
pub(super) fn shortened_first_content_timeout(base: Duration, waited: Duration) -> Duration {
    if waited >= base * 7 / 4 {
        base / 2
    } else if waited >= base {
        base * 3 / 4
    } else {
        base
    }
}

pub(super) fn remaining_timeout(
    started: Instant,
    budget: Duration,
    cap: Duration,
) -> Option<Duration> {
    budget
        .checked_sub(started.elapsed())
        .map(|remaining| remaining.min(cap))
        .filter(|remaining| !remaining.is_zero())
}

pub(super) fn classify_failure(status: StatusCode, body: &[u8]) -> Option<FailureClass> {
    if is_context_limit_body(body) {
        return None;
    }
    let message = String::from_utf8_lossy(body).to_ascii_lowercase();
    if [
        "content policy",
        "safety policy",
        "content moderation",
        "blocked by safety",
    ]
    .iter()
    .any(|pattern| message.contains(pattern))
    {
        return None;
    }
    let value = serde_json::from_slice::<Value>(body).unwrap_or(Value::Null);
    let error = value
        .pointer("/response/error")
        .filter(|error| error.is_object())
        .or_else(|| value.get("error").filter(|error| error.is_object()))
        .unwrap_or(&value);
    if ["code", "type"].iter().any(|key| {
        error[*key].as_str().is_some_and(|code| {
            [
                "content_policy_violation",
                "content_filter",
                "safety_policy_violation",
            ]
            .iter()
            .any(|policy| code.eq_ignore_ascii_case(policy))
        })
    }) {
        return None;
    }
    if matches!(
        status,
        StatusCode::BAD_REQUEST | StatusCode::UNPROCESSABLE_ENTITY
    ) || status.is_server_error()
    {
        for key in ["code", "type", "status"] {
            let failure = match error[key]
                .as_str()
                .unwrap_or_default()
                .to_ascii_lowercase()
                .as_str()
            {
                "request_incompatible" | "unsupported_parameter" => {
                    Some(FailureClass::RequestIncompatible)
                }
                "model_not_found" | "model_not_supported" | "model_unavailable" => {
                    Some(FailureClass::ModelUnavailable)
                }
                "invalid_api_key"
                | "authentication_error"
                | "unauthorized"
                | "unauthenticated"
                | "permission_denied" => Some(FailureClass::Authentication),
                "insufficient_quota" | "billing_error" | "billing_hard_limit_reached" => {
                    Some(FailureClass::Billing)
                }
                "rate_limit_exceeded"
                | "rate_limit_error"
                | "usage_limit_reached"
                | "resource_exhausted"
                | "too_many_requests" => Some(FailureClass::RateLimit),
                "server_error"
                | "internal_error"
                | "internal_server_error"
                | "overloaded_error"
                | "service_unavailable"
                | "service_unavailable_error"
                | "unavailable"
                | "internal"
                | "request_timeout"
                | "deadline_exceeded"
                | "upstream_timeout"
                | "empty_response"
                | "upstream_empty_response"
                | "stream_early_eof"
                | "stream_timeout" => Some(FailureClass::Transient),
                _ => None,
            };
            if failure.is_some() {
                return failure;
            }
        }
    }
    if [
        "does not support tool",
        "tools are not supported",
        "tool use is not supported",
        "function calling is not supported",
        "tools_not_supported",
        "does not support image",
        "images are not supported",
        "vision is not supported",
        "responses_conversion_unsupported",
        "unsupported_parameter",
    ]
    .iter()
    .any(|pattern| message.contains(pattern))
    {
        return Some(FailureClass::RequestIncompatible);
    }
    if [
        "model_not_found",
        "model not found",
        "model does not exist",
        "model is not available",
        "model unavailable",
        "unknown model",
        "invalid model",
        "model_not_supported",
        "model has been deprecated",
        "model has been decommissioned",
    ]
    .iter()
    .any(|pattern| message.contains(pattern))
    {
        return Some(FailureClass::ModelUnavailable);
    }
    if [
        "insufficient quota",
        "insufficient_quota",
        "quota exceeded",
        "credit balance",
        "credits exhausted",
    ]
    .iter()
    .any(|pattern| message.contains(pattern))
    {
        return Some(FailureClass::Billing);
    }
    let status_failure = match status.as_u16() {
        401 | 403 => Some(FailureClass::Authentication),
        402 => Some(FailureClass::Billing),
        404 => Some(FailureClass::ModelUnavailable),
        405 | 415 => Some(FailureClass::RequestIncompatible),
        408 | 409 => Some(FailureClass::Transient),
        429 => Some(FailureClass::RateLimit),
        500..=599 => Some(FailureClass::Transient),
        _ => None,
    };
    if status_failure.is_some() {
        return status_failure;
    }
    if ["rate limit", "rate_limit", "too many requests"]
        .iter()
        .any(|pattern| message.contains(pattern))
    {
        return Some(FailureClass::RateLimit);
    }
    None
}

pub(super) fn is_context_limit_body(body: &[u8]) -> bool {
    let value = serde_json::from_slice::<Value>(body).unwrap_or(Value::Null);
    let error = value
        .pointer("/response/error")
        .filter(|error| !error.is_null())
        .or_else(|| value.get("error").filter(|error| !error.is_null()))
        .unwrap_or(&value);
    let code = error["code"].as_str().or_else(|| error["type"].as_str());
    let message = String::from_utf8_lossy(body);
    crate::services::account_proxy::context_limit_error(code, &message)
}

pub(super) fn retry_after(headers: &HeaderMap) -> Option<Duration> {
    let value = headers.get(header::RETRY_AFTER)?.to_str().ok()?.trim();
    if let Ok(seconds) = value.parse::<u64>() {
        return Some(Duration::from_secs(seconds).min(MAX_RETRY_AFTER));
    }

    let retry_at = chrono::DateTime::parse_from_rfc2822(value).ok()?;
    let wait = retry_at.signed_duration_since(chrono::Utc::now());
    Some(wait.to_std().unwrap_or_default().min(MAX_RETRY_AFTER))
}
