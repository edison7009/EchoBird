use super::health_transitions::{attempt_epoch_is_current, mark_stream_failure};
// Router responsibility; behavior is covered by the server regression suite.
use super::memory_store::persist_response_memory;
use super::{
    now_ms, proxy_error_from_chain, AppState, Candidate, FailureClass, ResponseBinding,
    RouteProtocol, MAX_RESPONSE_BINDINGS, RESPONSES_CAPABILITY_LIFETIME_MS,
    RESPONSE_BINDING_LIFETIME_MS,
};
use axum::http::StatusCode;
use serde_json::json;
use serde_json::Value;

pub(super) fn has_previous_response(body: &Value) -> bool {
    body.get("previous_response_id")
        .is_some_and(|value| !value.is_null())
}

pub(super) fn response_candidates(
    state: &AppState,
    body: &Value,
    candidates: Vec<Candidate>,
    protocol: RouteProtocol,
) -> Result<Vec<Candidate>, &'static str> {
    if protocol != RouteProtocol::Responses || !has_previous_response(body) {
        return Ok(candidates);
    }
    let id = body["previous_response_id"]
        .as_str()
        .filter(|id| !id.is_empty())
        .ok_or("previous_response_id must be a response ID")?;
    let binding = state
        .response_memory
        .lock()
        .ok()
        .and_then(|memory| memory.response_bindings.get(id).cloned())
        .filter(|binding| binding.expires_at_ms > now_ms())
        .ok_or("Unknown or expired response ID; send the complete conversation input")?;
    if binding.ambiguous {
        return Err("Ambiguous response ID; send the complete conversation input");
    }
    let candidate = candidates.into_iter()
        .find(|candidate| candidate.internal_id == binding.candidate_id && candidate.fingerprint == binding.fingerprint)
        .ok_or("The original response provider configuration has changed; send the complete conversation input")?;
    Ok(vec![candidate])
}

pub(super) fn remember_response(state: &AppState, candidate: &Candidate, id: Option<&str>) {
    let Some(id) = id.filter(|id| !id.is_empty() && id.len() <= 1024) else {
        return;
    };
    if !state.route_memory.lock().is_ok_and(|health| {
        // A valid response belongs to the same account after token refresh.
        // Explicit sign-in or model edits still invalidate the old request.
        candidate
            .attempt
            .as_ref()
            .is_none_or(|attempt| attempt_epoch_is_current(&health, attempt))
    }) {
        return;
    }
    let Ok(mut memory) = state.response_memory.lock() else {
        return;
    };
    let now = now_ms();
    memory
        .response_bindings
        .retain(|_, binding| binding.expires_at_ms > now);
    if let Some(binding) = memory.response_bindings.get_mut(id) {
        if binding.ambiguous
            || binding.candidate_id != candidate.internal_id
            || binding.fingerprint != candidate.fingerprint
        {
            binding.ambiguous = true;
            binding.expires_at_ms = now.saturating_add(RESPONSE_BINDING_LIFETIME_MS);
            persist_response_memory(state, memory);
            return;
        }
    }
    if memory.response_bindings.len() >= MAX_RESPONSE_BINDINGS
        && !memory.response_bindings.contains_key(id)
    {
        let oldest = memory
            .response_bindings
            .iter()
            .min_by_key(|(_, binding)| binding.expires_at_ms)
            .map(|(id, _)| id.clone());
        if let Some(oldest) = oldest {
            memory.response_bindings.remove(&oldest);
        }
    }
    memory.response_bindings.insert(
        id.into(),
        ResponseBinding {
            candidate_id: candidate.internal_id.clone(),
            fingerprint: candidate.fingerprint.clone(),
            expires_at_ms: now.saturating_add(RESPONSE_BINDING_LIFETIME_MS),
            ambiguous: false,
        },
    );
    persist_response_memory(state, memory);
}

pub(super) fn remember_unsupported_responses(state: &AppState, candidate: &Candidate) {
    let Ok(mut memory) = state.route_memory.lock() else {
        return;
    };
    let now = now_ms();
    memory.unsupported_responses.retain(|_, until| *until > now);
    if memory.unsupported_responses.len() >= 128 {
        let oldest = memory
            .unsupported_responses
            .iter()
            .min_by_key(|(_, until)| *until)
            .map(|(key, _)| key.clone());
        if let Some(oldest) = oldest {
            memory.unsupported_responses.remove(&oldest);
        }
    }
    memory.unsupported_responses.insert(
        candidate.fingerprint.clone(),
        now.saturating_add(RESPONSES_CAPABILITY_LIFETIME_MS),
    );
}

pub(super) fn responses_endpoint_unsupported(status: StatusCode, body: &[u8]) -> bool {
    let message = String::from_utf8_lossy(body).to_ascii_lowercase();
    if continuation_error(body) || message.contains("safety") || message.contains("content policy")
    {
        return false;
    }
    if matches!(status, StatusCode::NOT_FOUND | StatusCode::BAD_REQUEST)
        && [
            "responses_not_supported",
            "model does not support responses",
            "model does not support the responses",
            "responses is not supported for this model",
            "responses are not supported for this model",
        ]
        .iter()
        .any(|pattern| message.contains(pattern))
    {
        return true;
    }
    if message.contains("model") && !message.contains("endpoint_not_found") {
        return false;
    }
    if matches!(
        status,
        StatusCode::METHOD_NOT_ALLOWED | StatusCode::NOT_IMPLEMENTED
    ) {
        return true;
    }
    if status == StatusCode::NOT_FOUND
        && matches!(message.trim(), "" | "not found" | "404 page not found")
    {
        return true;
    }
    matches!(status, StatusCode::NOT_FOUND | StatusCode::BAD_REQUEST)
        && [
            "endpoint_not_found",
            "unsupported_endpoint",
            "unsupported endpoint",
            "responses is not supported",
            "responses api is not supported",
            "responses endpoint is not supported",
            "post /v1/responses is not supported",
        ]
        .iter()
        .any(|pattern| message.contains(pattern))
}

pub(super) fn continuation_error(body: &[u8]) -> bool {
    let message = String::from_utf8_lossy(body).to_ascii_lowercase();
    [
        "previous_response",
        "previous response",
        "response_not_found",
        "response not found",
        "response has expired",
    ]
    .iter()
    .any(|pattern| message.contains(pattern))
}

pub(super) fn is_continuation_failure(status: StatusCode, body: &[u8]) -> bool {
    matches!(
        status,
        StatusCode::BAD_REQUEST
            | StatusCode::NOT_FOUND
            | StatusCode::CONFLICT
            | StatusCode::UNPROCESSABLE_ENTITY
    ) && continuation_error(body)
}

pub(super) fn mark_response_failure(
    state: &AppState,
    candidate: &Candidate,
    error: &(dyn std::error::Error + 'static),
    bound_continuation: bool,
) -> Option<FailureClass> {
    if bound_continuation && proxy_error_from_chain(error).is_some_and(|proxy| {
        let body = json!({"error":{"message":proxy.message,"code":proxy.error_code,"type":proxy.error_type}}).to_string();
        is_continuation_failure(proxy.status, body.as_bytes())
    }) {
        if let Some(statistics) = &state.request_statistics { statistics.outcome(&candidate.internal_id, Some(5)); }
        return None;
    }
    mark_stream_failure(state, candidate, error)
}
