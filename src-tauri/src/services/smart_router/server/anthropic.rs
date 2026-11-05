use futures_util::StreamExt;
// Router responsibility; behavior is covered by the server regression suite.
use super::{openai_error, UPSTREAM_FINISH_ERROR};
use crate::services::smart_router::SMART_ROUTER_MODEL_ID;
use axum::body::Body;
use axum::body::Bytes;
use axum::http::header;
use axum::http::HeaderValue;
use axum::http::StatusCode;
use axum::response::Response;
use http_body_util::BodyExt;
use serde_json::json;
use serde_json::Value;
use std::collections::BTreeMap;
use tokio_stream::wrappers::ReceiverStream;

#[derive(Debug, Default)]
pub(super) struct ToolCallBuffer {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) arguments: String,
}

pub(super) struct AnthropicSseAdapter {
    pub(super) model_id: String,
    pub(super) parser: crate::services::account_proxy::SseParser,
    pub(super) google_thought: crate::services::account_proxy::GoogleThought,
    pub(super) message_id: String,
    pub(super) text_index: Option<usize>,
    pub(super) thinking_index: Option<usize>,
    pub(super) next_block_index: usize,
    pub(super) tool_calls: BTreeMap<u64, ToolCallBuffer>,
    pub(super) finish_reason: Option<String>,
    pub(super) output_tokens: u64,
    pub(super) finished: bool,
}

impl AnthropicSseAdapter {
    pub(super) fn new() -> Self {
        Self {
            model_id: SMART_ROUTER_MODEL_ID.into(),
            parser: crate::services::account_proxy::SseParser::default(),
            google_thought: crate::services::account_proxy::GoogleThought::default(),
            message_id: format!("msg_{}", chrono::Utc::now().timestamp_millis()),
            text_index: None,
            thinking_index: None,
            next_block_index: 0,
            tool_calls: BTreeMap::new(),
            finish_reason: None,
            output_tokens: 0,
            finished: false,
        }
    }

    pub(super) fn start_event(&self) -> Bytes {
        anthropic_sse_event(
            "message_start",
            json!({
                "type": "message_start",
                "message": {
                    "id": self.message_id,
                    "type": "message",
                    "role": "assistant",
                    "content": [],
                    "model": self.model_id,
                    "stop_reason": null,
                    "stop_sequence": null,
                    "usage": {"input_tokens": 0, "output_tokens": 0}
                }
            }),
        )
    }

    pub(super) fn push(&mut self, chunk: &[u8]) -> Result<Vec<Bytes>, String> {
        let mut events = Vec::new();
        for (event, data) in self.parser.push(chunk) {
            events.extend(self.process_frame(&event, &data)?);
        }
        Ok(events)
    }

    pub(super) fn finish(mut self) -> Result<Vec<Bytes>, String> {
        let mut events = Vec::new();
        for (event, data) in self.parser.finish() {
            events.extend(self.process_frame(&event, &data)?);
        }
        if !self.finished && self.finish_reason.is_none() {
            return Err("Smart Router stream ended before completion".to_string());
        }
        events.extend(self.finish_events());
        Ok(events)
    }

    pub(super) fn process_frame(&mut self, event: &str, data: &str) -> Result<Vec<Bytes>, String> {
        if self.finished {
            return Ok(Vec::new());
        }
        if data.trim() == "[DONE]" {
            return Ok(self.finish_events());
        }
        let chunk: Value = serde_json::from_str(data)
            .map_err(|error| format!("Invalid Smart Router stream: {error}"))?;
        if chunk["error"]["code"] == "stream_idle_timeout"
            || chunk["error"]["code"] == UPSTREAM_FINISH_ERROR
        {
            self.finished = true;
            return Ok(vec![anthropic_sse_event(
                "error",
                json!({"type":"error","error":chunk["error"]}),
            )]);
        }
        if event == "error"
            || chunk.get("error").is_some_and(|error| !error.is_null())
            || chunk["type"] == "error"
        {
            return Err(chunk["error"]["message"]
                .as_str()
                .or_else(|| chunk["message"].as_str())
                .unwrap_or("Smart Router upstream stream failed")
                .to_string());
        }

        if let Some(tokens) = chunk
            .get("usage")
            .and_then(|usage| usage.get("completion_tokens"))
            .and_then(Value::as_u64)
        {
            self.output_tokens = tokens;
        }

        let Some(choice) = chunk
            .get("choices")
            .and_then(Value::as_array)
            .and_then(|choices| choices.first())
        else {
            return Ok(Vec::new());
        };
        if let Some(reason) = choice
            .get("finish_reason")
            .and_then(Value::as_str)
            .filter(|reason| !reason.is_empty())
        {
            self.finish_reason = Some(reason.to_string());
        }

        let Some(delta) = choice.get("delta") else {
            return Ok(Vec::new());
        };
        let mut events = Vec::new();
        if let Some(text) = delta["reasoning_content"].as_str() {
            self.push_content(true, text, &mut events);
        }
        if let Some(text) = delta.get("content").and_then(Value::as_str) {
            let marked = delta["extra_content"]["google"]["thought"] == true;
            for (thinking, text) in self.google_thought.push(text, marked) {
                self.push_content(thinking, &text, &mut events);
            }
        }
        if let Some(text) = delta["refusal"].as_str() {
            self.push_content(false, text, &mut events);
        }

        if let Some(tool_calls) = delta.get("tool_calls").and_then(Value::as_array) {
            for tool_call in tool_calls {
                let index = tool_call.get("index").and_then(Value::as_u64).unwrap_or(0);
                let buffered = self.tool_calls.entry(index).or_default();
                if let Some(id) = tool_call.get("id").and_then(Value::as_str) {
                    buffered.id.push_str(id);
                }
                if let Some(function) = tool_call.get("function") {
                    if let Some(name) = function.get("name").and_then(Value::as_str) {
                        buffered.name.push_str(name);
                    }
                    if let Some(arguments) = function.get("arguments").and_then(Value::as_str) {
                        buffered.arguments.push_str(arguments);
                    }
                }
            }
        }
        Ok(events)
    }

    pub(super) fn push_content(&mut self, thinking: bool, text: &str, events: &mut Vec<Bytes>) {
        if text.is_empty() {
            return;
        }
        let previous = if thinking {
            self.text_index.take()
        } else {
            self.thinking_index.take()
        };
        if let Some(index) = previous {
            events.push(anthropic_sse_event(
                "content_block_stop",
                json!({"type":"content_block_stop","index":index}),
            ));
        }
        let current = if thinking {
            &mut self.thinking_index
        } else {
            &mut self.text_index
        };
        let index = match *current {
            Some(index) => index,
            None => {
                let index = self.next_block_index;
                self.next_block_index += 1;
                *current = Some(index);
                events.push(anthropic_sse_event(
                    "content_block_start",
                    json!({"type":"content_block_start","index":index,"content_block":if thinking { json!({"type":"thinking","thinking":""}) } else { json!({"type":"text","text":""}) }}),
                ));
                index
            }
        };
        events.push(anthropic_sse_event(
            "content_block_delta",
            json!({"type":"content_block_delta","index":index,"delta":if thinking { json!({"type":"thinking_delta","thinking":text}) } else { json!({"type":"text_delta","text":text}) }}),
        ));
    }

    pub(super) fn finish_events(&mut self) -> Vec<Bytes> {
        if self.finished {
            return Vec::new();
        }
        self.finished = true;
        let mut events = Vec::new();

        if let Some((thinking, text)) = self.google_thought.finish() {
            self.push_content(thinking, &text, &mut events);
        }

        for index in [self.thinking_index, self.text_index].into_iter().flatten() {
            events.push(anthropic_sse_event(
                "content_block_stop",
                json!({"type": "content_block_stop", "index": index}),
            ));
        }

        for tool_call in self.tool_calls.values() {
            let index = self.next_block_index;
            self.next_block_index += 1;
            let id = if tool_call.id.is_empty() {
                format!("call_{index}")
            } else {
                tool_call.id.clone()
            };
            events.push(anthropic_sse_event(
                "content_block_start",
                json!({
                    "type": "content_block_start",
                    "index": index,
                    "content_block": {
                        "type": "tool_use",
                        "id": id,
                        "name": tool_call.name,
                        "input": {}
                    }
                }),
            ));
            events.push(anthropic_sse_event(
                "content_block_delta",
                json!({
                    "type": "content_block_delta",
                    "index": index,
                    "delta": {
                        "type": "input_json_delta",
                        "partial_json": if tool_call.arguments.is_empty() {
                            "{}"
                        } else {
                            tool_call.arguments.as_str()
                        }
                    }
                }),
            ));
            events.push(anthropic_sse_event(
                "content_block_stop",
                json!({"type": "content_block_stop", "index": index}),
            ));
        }

        if self.next_block_index == 0 && self.tool_calls.is_empty() {
            events.push(anthropic_sse_event(
                "content_block_start",
                json!({
                    "type": "content_block_start",
                    "index": 0,
                    "content_block": {"type": "text", "text": ""}
                }),
            ));
            events.push(anthropic_sse_event(
                "content_block_stop",
                json!({"type": "content_block_stop", "index": 0}),
            ));
        }

        let stop_reason = if !self.tool_calls.is_empty() {
            "tool_use"
        } else {
            match self.finish_reason.as_deref() {
                Some("length") => "max_tokens",
                Some("tool_calls") => "tool_use",
                _ => "end_turn",
            }
        };
        events.push(anthropic_sse_event(
            "message_delta",
            json!({
                "type": "message_delta",
                "delta": {"stop_reason": stop_reason, "stop_sequence": null},
                "usage": {"output_tokens": self.output_tokens}
            }),
        ));
        events.push(anthropic_sse_event(
            "message_stop",
            json!({"type": "message_stop"}),
        ));
        events
    }
}

pub(super) fn anthropic_sse_event(event: &str, data: Value) -> Bytes {
    Bytes::from(format!("event: {event}\ndata: {data}\n\n"))
}

#[cfg(test)]
pub(super) fn openai_stream_to_anthropic(response: Response) -> Response {
    openai_stream_to_anthropic_for_model(response, SMART_ROUTER_MODEL_ID.into())
}

pub(super) fn openai_stream_to_anthropic_for_model(
    response: Response,
    model_id: String,
) -> Response {
    let routed_via = response.headers().get("X-EchoBird-Routed-Via").cloned();
    let mut source = response.into_body().into_data_stream();
    let (tx, rx) = tokio::sync::mpsc::channel::<Result<Bytes, std::io::Error>>(32);

    tokio::spawn(async move {
        let mut adapter = AnthropicSseAdapter::new();
        adapter.model_id = model_id;
        if tx.send(Ok(adapter.start_event())).await.is_err() {
            return;
        }
        loop {
            let item = tokio::select! {
                _ = tx.closed() => return,
                item = source.next() => item,
            };
            let Some(item) = item else { break };
            match item {
                Ok(chunk) => {
                    let events = match adapter.push(&chunk) {
                        Ok(events) => events,
                        Err(error) => {
                            let _ = tx.send(Err(std::io::Error::other(error))).await;
                            return;
                        }
                    };
                    for event in events {
                        if tx.send(Ok(event)).await.is_err() {
                            return;
                        }
                    }
                }
                Err(error) => {
                    let _ = tx.send(Err(std::io::Error::other(error))).await;
                    return;
                }
            }
        }
        let events = match adapter.finish() {
            Ok(events) => events,
            Err(error) => {
                let _ = tx.send(Err(std::io::Error::other(error))).await;
                return;
            }
        };
        for event in events {
            if tx.send(Ok(event)).await.is_err() {
                return;
            }
        }
    });

    anthropic_response(
        StatusCode::OK,
        "text/event-stream",
        Body::from_stream(ReceiverStream::new(rx)),
        routed_via,
    )
}

pub(super) async fn openai_response_to_anthropic(response: Response) -> Response {
    let routed_via = response.headers().get("X-EchoBird-Routed-Via").cloned();
    let body = match response.into_body().collect().await {
        Ok(collected) => collected.to_bytes(),
        Err(error) => {
            return anthropic_error(
                StatusCode::BAD_GATEWAY,
                &format!("Failed to read Smart Router response: {error}"),
            )
        }
    };
    let data: Value = match serde_json::from_slice(&body) {
        Ok(data) => data,
        Err(error) => {
            return anthropic_error(
                StatusCode::BAD_GATEWAY,
                &format!("Invalid Smart Router response: {error}"),
            )
        }
    };
    let converted = crate::services::local_llm::proxy::openai_to_anthropic(&data);
    anthropic_response(
        StatusCode::OK,
        "application/json",
        Body::from(converted.to_string()),
        routed_via,
    )
}

pub(super) async fn openai_error_to_anthropic(response: Response) -> Response {
    let status = response.status();
    let body = response
        .into_body()
        .collect()
        .await
        .map(|collected| collected.to_bytes())
        .unwrap_or_default();
    let parsed: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    let message = parsed
        .pointer("/error/message")
        .and_then(Value::as_str)
        .unwrap_or("Auto Router request failed");
    anthropic_error(status, message)
}

pub(super) fn anthropic_error(status: StatusCode, message: &str) -> Response {
    let error_type = match status {
        StatusCode::BAD_REQUEST => "invalid_request_error",
        StatusCode::PAYLOAD_TOO_LARGE => "request_too_large",
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => "authentication_error",
        StatusCode::TOO_MANY_REQUESTS => "rate_limit_error",
        _ => "api_error",
    };
    anthropic_response(
        status,
        "application/json",
        Body::from(
            json!({
                "type": "error",
                "error": {"type": error_type, "message": message}
            })
            .to_string(),
        ),
        None,
    )
}

pub(super) fn anthropic_response(
    status: StatusCode,
    content_type: &'static str,
    body: Body,
    routed_via: Option<HeaderValue>,
) -> Response {
    let mut response = Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CACHE_CONTROL, "no-cache");
    if let Some(routed_via) = routed_via {
        response = response.header("X-EchoBird-Routed-Via", routed_via);
    }
    response.body(body).unwrap_or_else(|_| {
        openai_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to build Anthropic router response",
        )
    })
}
