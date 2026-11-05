// Router responsibility; behavior is covered by the server regression suite.
use super::{RouteProtocol, UPSTREAM_FINISH_ERROR};
use axum::body::Bytes;
use axum::http::StatusCode;
use serde_json::json;
use serde_json::Value;

pub(super) fn nonempty_text(value: &Value) -> bool {
    value.as_str().is_some_and(|text| !text.is_empty())
}

pub(super) fn positive_token_usage(usage: &Value) -> bool {
    [
        "input_tokens",
        "output_tokens",
        "prompt_tokens",
        "completion_tokens",
        "total_tokens",
    ]
    .iter()
    .any(|field| usage[*field].as_f64().is_some_and(|tokens| tokens > 0.0))
}

pub(super) fn content_part_has_content(part: &Value) -> bool {
    nonempty_text(&part["text"])
        || nonempty_text(&part["refusal"])
        || nonempty_text(&part["thinking"])
        || part["type"].as_str().is_some_and(|kind| {
            !kind.is_empty()
                && !matches!(
                    kind,
                    "text" | "input_text" | "output_text" | "summary_text" | "refusal" | "thinking"
                )
        })
}

pub(super) fn output_item_has_content(item: &Value) -> bool {
    match item["type"].as_str() {
        Some("message" | "reasoning") => {
            nonempty_text(&item["content"])
                || nonempty_text(&item["encrypted_content"])
                || (item["type"] == "reasoning" && nonempty_text(&item["id"]))
                || ["content", "summary"].iter().any(|field| {
                    item[*field]
                        .as_array()
                        .is_some_and(|parts| parts.iter().any(content_part_has_content))
                })
        }
        Some(kind) => !kind.is_empty(),
        None => false,
    }
}

pub(super) fn response_has_content(response: &Value) -> bool {
    response["output"]
        .as_array()
        .is_some_and(|items| items.iter().any(output_item_has_content))
}

pub(super) fn chat_message_has_content(message: &Value) -> bool {
    nonempty_text(&message["content"])
        || nonempty_text(&message["reasoning_content"])
        || nonempty_text(&message["refusal"])
        || nonempty_text(&message["audio"]["data"])
        || nonempty_text(&message["audio"]["transcript"])
        || message["content"]
            .as_array()
            .is_some_and(|parts| parts.iter().any(content_part_has_content))
        || message["images"]
            .as_array()
            .is_some_and(|parts| parts.iter().any(content_part_has_content))
        || message["tool_calls"]
            .as_array()
            .is_some_and(|calls| !calls.is_empty())
        || message["function_call"].is_object()
}

pub(super) fn empty_completed_response(response: &Value) -> bool {
    response["status"] == "completed"
        && !response_has_content(response)
        && !positive_token_usage(&response["usage"])
        && !legitimate_empty_response(response)
}

pub(super) fn empty_chat_response(response: &Value) -> bool {
    let Some(choices) = response["choices"].as_array() else {
        return false;
    };
    !positive_token_usage(&response["usage"])
        && choices.iter().all(|choice| {
            let Some(message) = choice["message"].as_object() else {
                return false;
            };
            message.keys().all(|key| {
                matches!(
                    key.as_str(),
                    "role"
                        | "content"
                        | "reasoning_content"
                        | "refusal"
                        | "tool_calls"
                        | "function_call"
                        | "audio"
                        | "images"
                )
            }) && !chat_message_has_content(&choice["message"])
                && !matches!(
                    choice["finish_reason"].as_str(),
                    Some("length" | "content_filter")
                )
        })
}

pub(super) fn legitimate_empty_response(response: &Value) -> bool {
    response["status"] == "incomplete"
        || matches!(
            response["incomplete_details"]["reason"].as_str(),
            Some("max_output_tokens" | "content_filter")
        )
}

pub(super) fn stream_timeout_error(
    code: &str,
    message: &str,
) -> crate::services::account_proxy::ProxyError {
    let mut error =
        crate::services::account_proxy::ProxyError::new(StatusCode::GATEWAY_TIMEOUT, message);
    error.error_code = Some(code.into());
    error.error_type = Some("timeout_error".into());
    error
}

pub(super) fn chat_finished_with_error(response: &Value) -> bool {
    response["choices"].as_array().is_some_and(|choices| {
        choices
            .iter()
            .any(|choice| choice["finish_reason"] == "error")
    })
}

pub(super) fn upstream_finish_error() -> crate::services::account_proxy::ProxyError {
    let mut error = crate::services::account_proxy::ProxyError::new(
        StatusCode::BAD_GATEWAY,
        "Upstream stream finished with an error",
    );
    error.error_code = Some(UPSTREAM_FINISH_ERROR.into());
    error.error_type = Some("server_error".into());
    error
}

pub(super) fn stream_failure_event(
    protocol: RouteProtocol,
    error: &crate::services::account_proxy::ProxyError,
) -> Bytes {
    if protocol == RouteProtocol::Responses {
        return Bytes::from(format!(
            "event: error\ndata: {}\n\n",
            json!({"type":"error","code":error.error_code,"message":error.message,"param":null})
        ));
    }
    Bytes::from(format!(
        "data: {}\n\n",
        json!({"error":{"type":error.error_type,"code":error.error_code,"message":error.message}})
    ))
}

pub(super) fn empty_response_error() -> crate::services::account_proxy::ProxyError {
    let mut error = crate::services::account_proxy::ProxyError::new(
        StatusCode::BAD_GATEWAY,
        "Upstream completed without any output or token usage",
    );
    error.error_code = Some("empty_response".into());
    error
}
