// LLM Client — supports OpenAI and Anthropic APIs with SSE streaming
// Unified interface via LlmEvent enum for the Agent Loop
//
// v4.7.0 hardening:
//   • Verbatim upstream errors. 401 / 403 / 429 / 5xx now surface the
//     provider's own `error.message` (e.g. "Invalid API Key") instead
//     of an opaque "Failed to create event source" string.
//   • Per-chunk read timeout. If the upstream goes silent for 300s
//     between SSE events, we stop waiting and surface a clear stall
//     error instead of hanging until the total request timeout.
//   • Transient retry. 5xx + 429 + transport errors are retried up to
//     2 times with exponential backoff (1.5s / 4.5s by default,
//     honoring `Retry-After` when present). Only applies to the OpenAI
//     path — Anthropic's official SDK already retries client-side.

use futures_util::StreamExt;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use reqwest_eventsource::{Event, EventSource};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::Duration;
use tokio::sync::mpsc;

/// Maximum quiet time between SSE chunks before we declare the upstream
/// stalled. Generous (5 minutes) to accommodate thinking-model warmups
/// where DeepSeek-R1 / o1-preview can think silently for several minutes
/// before emitting any tokens.
const SSE_CHUNK_TIMEOUT: Duration = Duration::from_secs(300);

/// Number of attempts (including the first) for retryable upstream errors.
/// 3 attempts = original + 2 retries; the third failure is surfaced to
/// the caller.
const MAX_RETRY_ATTEMPTS: u32 = 3;

// ── Public Types ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmConfig {
    pub provider: LlmProvider,
    pub base_url: String,
    pub api_key: String,
    pub model: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LlmProvider {
    OpenAI,
    Anthropic,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDef {
    pub name: String,
    pub description: String,
    pub parameters: Value, // JSON Schema
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String, // JSON string
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub content: MessageContent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MessageContent {
    Text(String),
    Blocks(Vec<ContentBlock>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    /// Thinking / reasoning block. Required round-trip for thinking-mode models
    /// (DeepSeek-R1, deepseek-v4-pro, Anthropic extended-thinking, etc.). Without
    /// this stored on the prior assistant message, the next request 400s with
    /// "reasoning_content / content[].thinking must be passed back to the API".
    /// `signature` is Anthropic-only — empty for OpenAI-style reasoning_content.
    #[serde(rename = "thinking")]
    Thinking {
        thinking: String,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        signature: String,
    },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },
    #[serde(rename = "tool_result")]
    ToolResult {
        tool_use_id: String,
        content: String,
    },
}

/// Events emitted by the LLM client during streaming
#[derive(Debug, Clone)]
pub enum LlmEvent {
    TextDelta(String),
    Thinking(String),
    /// Anthropic streams the thinking block's signature as a separate
    /// `signature_delta`. We accumulate it alongside `Thinking` text and
    /// pin it to the saved assistant message so the next turn validates.
    ThinkingSignature(String),
    ToolCallStart {
        id: String,
        name: String,
    },
    ToolCallDelta {
        id: String,
        args_chunk: String,
    },
    ToolCallEnd {
        id: String,
    },
    Done {
        stop_reason: String,
    },
    Error(String),
}

// ── Client ──

#[derive(Clone)]
pub struct LlmClient {
    config: LlmConfig,
    http: reqwest::Client,
}

impl LlmClient {
    pub fn new(config: LlmConfig) -> Result<Self, String> {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(600)) // 10 min: supports thinking models (DeepSeek-R1, etc.)
            .build()
            .map_err(|e| format!("Failed to build HTTP client: {}", e))?;

        Ok(Self { config, http })
    }

    /// Stream a chat completion with tool support.
    /// Returns a channel receiver that emits LlmEvent items.
    pub async fn chat_stream(
        &self,
        messages: &[Message],
        tools: &[ToolDef],
        system_prompt: &str,
    ) -> Result<mpsc::Receiver<LlmEvent>, String> {
        match self.config.provider {
            LlmProvider::OpenAI => {
                self.chat_stream_openai(messages, tools, system_prompt)
                    .await
            }
            LlmProvider::Anthropic => {
                self.chat_stream_anthropic(messages, tools, system_prompt)
                    .await
            }
        }
    }

    // ── OpenAI ──

    async fn chat_stream_openai(
        &self,
        messages: &[Message],
        tools: &[ToolDef],
        system_prompt: &str,
    ) -> Result<mpsc::Receiver<LlmEvent>, String> {
        let url = format!(
            "{}/chat/completions",
            self.config.base_url.trim_end_matches('/')
        );

        // Build messages array with system prompt
        let mut msgs = vec![json!({"role": "system", "content": system_prompt})];
        for m in messages {
            msgs.push(message_to_openai_json(m));
        }

        // Build tools array
        let tools_json: Vec<Value> = tools
            .iter()
            .map(|t| {
                json!({
                    "type": "function",
                    "function": {
                        "name": t.name,
                        "description": t.description,
                        "parameters": t.parameters,
                    }
                })
            })
            .collect();

        let mut body = json!({
            "model": self.config.model,
            "messages": msgs,
            "stream": true,
        });
        if !tools_json.is_empty() {
            body["tools"] = json!(tools_json);
        }

        // Pre-validate the Authorization header so we don't burn retry
        // attempts on a request that can never succeed.
        let auth_value = HeaderValue::from_str(&format!("Bearer {}", self.config.api_key))
            .map_err(|e| format!("Invalid API key: {}", e))?;

        // `open_with_retry` needs to rebuild the request on each
        // attempt (RequestBuilder is consumed by EventSource::new).
        let http = self.http.clone();
        let url_owned = url.clone();
        let body_owned = body.clone();
        let make_request = move || -> Result<reqwest::RequestBuilder, String> {
            let mut headers = HeaderMap::new();
            headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
            headers.insert(AUTHORIZATION, auth_value.clone());
            Ok(http.post(&url_owned).headers(headers).json(&body_owned))
        };

        let OpenedStream { mut es, pending } = open_with_retry(make_request).await?;

        let (tx, rx) = mpsc::channel(128);

        tokio::spawn(async move {
            // Track tool calls being assembled
            let mut current_tool_calls: std::collections::HashMap<i64, (String, String, String)> =
                std::collections::HashMap::new(); // index -> (id, name, args)

            // Drain any events `open_with_retry` consumed during the
            // connection handshake (the rare server that skips Open).
            // We feed them into the same handler via a tiny inline
            // helper to avoid duplicating 80 lines of match logic.
            let mut events: std::collections::VecDeque<Result<Event, reqwest_eventsource::Error>> =
                pending.into_iter().map(Ok).collect();

            // Per-chunk timeout: SSE_CHUNK_TIMEOUT between consecutive
            // events. Stalled upstream → emit Error + break instead of
            // hanging until the 10-minute global timeout.
            loop {
                let event = if let Some(buffered) = events.pop_front() {
                    Some(buffered)
                } else {
                    match tokio::time::timeout(SSE_CHUNK_TIMEOUT, es.next()).await {
                        Ok(maybe_event) => maybe_event,
                        Err(_) => {
                            let _ = tx
                                .send(LlmEvent::Error(format!(
                                    "Upstream stream stalled (no data for {}s)",
                                    SSE_CHUNK_TIMEOUT.as_secs()
                                )))
                                .await;
                            break;
                        }
                    }
                };
                let Some(event) = event else { break };

                match event {
                    Ok(Event::Message(msg)) => {
                        if msg.data == "[DONE]" {
                            let _ = tx
                                .send(LlmEvent::Done {
                                    stop_reason: "stop".into(),
                                })
                                .await;
                            break;
                        }
                        if let Ok(chunk) = serde_json::from_str::<Value>(&msg.data) {
                            if let Some(delta) = chunk["choices"][0]["delta"].as_object() {
                                // Text content
                                if let Some(content) = delta.get("content").and_then(|c| c.as_str())
                                {
                                    let _ = tx.send(LlmEvent::TextDelta(content.to_string())).await;
                                }
                                // OpenAI reasoning/thinking content (e.g. DeepSeek-R1)
                                if let Some(reasoning) =
                                    delta.get("reasoning_content").and_then(|c| c.as_str())
                                {
                                    let _ =
                                        tx.send(LlmEvent::Thinking(reasoning.to_string())).await;
                                }
                                // Tool calls
                                if let Some(tool_calls) =
                                    delta.get("tool_calls").and_then(|t| t.as_array())
                                {
                                    for tc in tool_calls {
                                        let idx = tc["index"].as_i64().unwrap_or(0);
                                        if let Some(func) = tc.get("function") {
                                            let id = tc["id"].as_str().unwrap_or("").to_string();
                                            let name =
                                                func["name"].as_str().unwrap_or("").to_string();
                                            let args = func["arguments"]
                                                .as_str()
                                                .unwrap_or("")
                                                .to_string();

                                            // Only treat as new tool call if this index hasn't been seen yet
                                            if !id.is_empty()
                                                && !current_tool_calls.contains_key(&idx)
                                            {
                                                current_tool_calls.insert(
                                                    idx,
                                                    (id.clone(), name.clone(), String::new()),
                                                );
                                                let _ = tx
                                                    .send(LlmEvent::ToolCallStart { id, name })
                                                    .await;
                                            }
                                            if !args.is_empty() {
                                                // Find the right index: prefer exact match, fallback to first entry
                                                let target_idx =
                                                    if current_tool_calls.contains_key(&idx) {
                                                        Some(idx)
                                                    } else {
                                                        current_tool_calls.keys().next().copied()
                                                    };
                                                if let Some(target_idx) = target_idx {
                                                    if let Some(entry) =
                                                        current_tool_calls.get_mut(&target_idx)
                                                    {
                                                        entry.2.push_str(&args);
                                                        let _ = tx
                                                            .send(LlmEvent::ToolCallDelta {
                                                                id: entry.0.clone(),
                                                                args_chunk: args,
                                                            })
                                                            .await;
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                            // Check finish_reason
                            if let Some(reason) = chunk["choices"][0]["finish_reason"].as_str() {
                                // Emit ToolCallEnd for all pending tool calls
                                for (_, (id, _, _)) in current_tool_calls.drain() {
                                    let _ = tx.send(LlmEvent::ToolCallEnd { id }).await;
                                }
                                let _ = tx
                                    .send(LlmEvent::Done {
                                        stop_reason: reason.to_string(),
                                    })
                                    .await;
                                break; // Always break on finish_reason — some APIs don't send [DONE]
                            }
                        }
                    }
                    Ok(Event::Open) => {}
                    Err(e) => {
                        // Mid-stream error after we've started receiving
                        // events. We don't retry here (would duplicate
                        // any tokens already delivered to the consumer);
                        // we just surface the error verbatim and exit.
                        let msg = match e {
                            reqwest_eventsource::Error::InvalidStatusCode(status, response) => {
                                let body = response.text().await.unwrap_or_default();
                                extract_upstream_error_message(status.as_u16(), &body)
                            }
                            reqwest_eventsource::Error::Transport(t) => {
                                format!("Connection lost mid-stream: {t}")
                            }
                            other => format!("Stream error: {other}"),
                        };
                        let _ = tx.send(LlmEvent::Error(msg)).await;
                        break;
                    }
                }
            }
        });

        Ok(rx)
    }

    // ── Anthropic ──

    async fn chat_stream_anthropic(
        &self,
        messages: &[Message],
        tools: &[ToolDef],
        system_prompt: &str,
    ) -> Result<mpsc::Receiver<LlmEvent>, String> {
        let base = self.config.base_url.trim_end_matches('/');
        // URL resolution:
        //   - already contains "/messages"  → use as-is
        //   - ends with "/anthropic"         → local proxy path, append "/messages"
        //   - otherwise                       → remote Anthropic-compatible, append "/v1/messages"
        let url = if base.contains("/messages") {
            base.to_string()
        } else if base.ends_with("/anthropic") {
            // Local unified proxy: /anthropic is the Anthropic endpoint
            format!("{}/messages", base)
        } else {
            format!("{}/v1/messages", base)
        };

        // Build messages (Anthropic format)
        let msgs: Vec<Value> = messages.iter().map(message_to_anthropic_json).collect();

        // Build tools
        let tools_json: Vec<Value> = tools
            .iter()
            .map(|t| {
                json!({
                    "name": t.name,
                    "description": t.description,
                    "input_schema": t.parameters,
                })
            })
            .collect();

        let mut body = json!({
            "model": self.config.model,
            "system": system_prompt,
            "messages": msgs,
            "max_tokens": 4096,
            "stream": true,
        });
        if !tools_json.is_empty() {
            body["tools"] = json!(tools_json);
        }

        // Pre-validate the api-key header so we don't burn retry
        // attempts on a request that can never succeed.
        let api_key_value = HeaderValue::from_str(&self.config.api_key)
            .map_err(|e| format!("Invalid API key: {}", e))?;

        let http = self.http.clone();
        let url_owned = url.clone();
        let body_owned = body.clone();
        let make_request = move || -> Result<reqwest::RequestBuilder, String> {
            let mut headers = HeaderMap::new();
            headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
            headers.insert("x-api-key", api_key_value.clone());
            headers.insert("anthropic-version", HeaderValue::from_static("2023-06-01"));
            Ok(http.post(&url_owned).headers(headers).json(&body_owned))
        };

        // Anthropic gets verbatim-error + per-chunk timeout via
        // `open_with_retry`; the retry side of the helper still kicks
        // in for 429 / 5xx, which is harmless when Anthropic's own
        // SDK would retry anyway.
        let OpenedStream { mut es, pending } = open_with_retry(make_request).await?;

        let (tx, rx) = mpsc::channel(128);

        tokio::spawn(async move {
            let mut current_tool_id = String::new();
            let mut current_tool_name = String::new();
            let mut done_sent = false;

            // Mirror the OpenAI path: drain buffered events first, then
            // pull from the live stream with a per-chunk timeout.
            let mut events: std::collections::VecDeque<Result<Event, reqwest_eventsource::Error>> =
                pending.into_iter().map(Ok).collect();

            loop {
                let event = if let Some(buffered) = events.pop_front() {
                    Some(buffered)
                } else {
                    match tokio::time::timeout(SSE_CHUNK_TIMEOUT, es.next()).await {
                        Ok(maybe_event) => maybe_event,
                        Err(_) => {
                            let _ = tx
                                .send(LlmEvent::Error(format!(
                                    "Upstream stream stalled (no data for {}s)",
                                    SSE_CHUNK_TIMEOUT.as_secs()
                                )))
                                .await;
                            break;
                        }
                    }
                };
                let Some(event) = event else { break };
                match event {
                    Ok(Event::Message(msg)) => {
                        let event_type = &msg.event;
                        if let Ok(data) = serde_json::from_str::<Value>(&msg.data) {
                            match event_type.as_str() {
                                "content_block_start" => {
                                    if let Some(block) = data.get("content_block") {
                                        match block["type"].as_str() {
                                            Some("tool_use") => {
                                                current_tool_id =
                                                    block["id"].as_str().unwrap_or("").to_string();
                                                current_tool_name = block["name"]
                                                    .as_str()
                                                    .unwrap_or("")
                                                    .to_string();
                                                let _ = tx
                                                    .send(LlmEvent::ToolCallStart {
                                                        id: current_tool_id.clone(),
                                                        name: current_tool_name.clone(),
                                                    })
                                                    .await;
                                            }
                                            Some("thinking") => {
                                                // Anthropic extended thinking block start
                                            }
                                            _ => {}
                                        }
                                    }
                                }
                                "content_block_delta" => {
                                    if let Some(delta) = data.get("delta") {
                                        match delta["type"].as_str() {
                                            Some("text_delta") => {
                                                if let Some(text) = delta["text"].as_str() {
                                                    let _ = tx
                                                        .send(LlmEvent::TextDelta(text.to_string()))
                                                        .await;
                                                }
                                            }
                                            Some("input_json_delta") => {
                                                if let Some(json_str) =
                                                    delta["partial_json"].as_str()
                                                {
                                                    let _ = tx
                                                        .send(LlmEvent::ToolCallDelta {
                                                            id: current_tool_id.clone(),
                                                            args_chunk: json_str.to_string(),
                                                        })
                                                        .await;
                                                }
                                            }
                                            Some("thinking_delta") => {
                                                if let Some(thinking) = delta["thinking"].as_str() {
                                                    let _ = tx
                                                        .send(LlmEvent::Thinking(
                                                            thinking.to_string(),
                                                        ))
                                                        .await;
                                                }
                                            }
                                            Some("signature_delta") => {
                                                if let Some(sig) = delta["signature"].as_str() {
                                                    let _ = tx
                                                        .send(LlmEvent::ThinkingSignature(
                                                            sig.to_string(),
                                                        ))
                                                        .await;
                                                }
                                            }
                                            _ => {}
                                        }
                                    }
                                }
                                "content_block_stop" if !current_tool_id.is_empty() => {
                                    let _ = tx
                                        .send(LlmEvent::ToolCallEnd {
                                            id: current_tool_id.clone(),
                                        })
                                        .await;
                                    current_tool_id.clear();
                                    current_tool_name.clear();
                                }
                                "message_delta" => {
                                    if let Some(delta) = data.get("delta") {
                                        if let Some(reason) = delta["stop_reason"].as_str() {
                                            let _ = tx
                                                .send(LlmEvent::Done {
                                                    stop_reason: reason.to_string(),
                                                })
                                                .await;
                                            done_sent = true;
                                        }
                                    }
                                }
                                "message_stop" => {
                                    // Safety net: emit Done if message_delta didn't already send it
                                    // (some Anthropic-compatible endpoints skip message_delta)
                                    if !done_sent {
                                        let _ = tx
                                            .send(LlmEvent::Done {
                                                stop_reason: "end_turn".into(),
                                            })
                                            .await;
                                    }
                                    break; // Stream complete — exit cleanly without SSE error
                                }
                                "error" => {
                                    let err_msg = data["error"]["message"]
                                        .as_str()
                                        .unwrap_or("Unknown Anthropic error")
                                        .to_string();
                                    let _ = tx.send(LlmEvent::Error(err_msg)).await;
                                    break;
                                }
                                _ => {}
                            }
                        }
                    }
                    Ok(Event::Open) => {}
                    Err(e) => {
                        // Mid-stream error after we've started receiving
                        // events. We don't retry here (would duplicate
                        // any tokens already delivered to the consumer);
                        // we just surface the error verbatim and exit.
                        let msg = match e {
                            reqwest_eventsource::Error::InvalidStatusCode(status, response) => {
                                let body = response.text().await.unwrap_or_default();
                                extract_upstream_error_message(status.as_u16(), &body)
                            }
                            reqwest_eventsource::Error::Transport(t) => {
                                format!("Connection lost mid-stream: {t}")
                            }
                            other => format!("Stream error: {other}"),
                        };
                        let _ = tx.send(LlmEvent::Error(msg)).await;
                        break;
                    }
                }
            }
        });

        Ok(rx)
    }
}

// ── Helpers ──

fn message_to_openai_json(m: &Message) -> Value {
    match &m.content {
        MessageContent::Text(text) => json!({"role": m.role, "content": text}),
        MessageContent::Blocks(blocks) => {
            // For tool results in OpenAI format
            if (m.role == "tool" || m.role == "user") && blocks.len() == 1 {
                if let Some(ContentBlock::ToolResult {
                    tool_use_id,
                    content,
                }) = blocks.first()
                {
                    return json!({
                        "role": "tool",
                        "tool_call_id": tool_use_id,
                        "content": content,
                    });
                }
            }
            if m.role == "tool" {
                if let Some(ContentBlock::ToolResult {
                    tool_use_id,
                    content,
                }) = blocks.first()
                {
                    return json!({
                        "role": "tool",
                        "tool_call_id": tool_use_id,
                        "content": content,
                    });
                }
            }
            // For assistant messages with tool calls
            if m.role == "assistant" {
                let mut tool_calls = Vec::new();
                let mut text_parts = Vec::new();
                let mut reasoning_parts = Vec::new();
                for block in blocks {
                    match block {
                        ContentBlock::Text { text } => text_parts.push(text.clone()),
                        ContentBlock::Thinking { thinking, .. } => {
                            reasoning_parts.push(thinking.clone())
                        }
                        ContentBlock::ToolUse { id, name, input } => {
                            tool_calls.push(json!({
                                "id": id,
                                "type": "function",
                                "function": {
                                    "name": name,
                                    "arguments": input.to_string(),
                                }
                            }));
                        }
                        _ => {}
                    }
                }
                let mut msg = json!({"role": "assistant"});
                msg["content"] = json!(text_parts.join(""));
                // Required by thinking-mode models (deepseek-v4-pro, R1, etc.):
                // the prior turn's reasoning must be echoed back, otherwise the
                // server returns 400 invalid_request_error.
                if !reasoning_parts.is_empty() {
                    msg["reasoning_content"] = json!(reasoning_parts.join(""));
                }
                if !tool_calls.is_empty() {
                    msg["tool_calls"] = json!(tool_calls);
                }
                return msg;
            }
            json!({"role": m.role, "content": m.content.clone()})
        }
    }
}

// ── Retry / Error Helpers ──

/// Status codes we'll retry. 429 is rate-limit, 5xx is server/upstream
/// problems that are typically transient.
///
/// We deliberately do NOT retry 408 (Request Timeout). Codex's own
/// behavior treats that as a hard fail because it likely means the
/// upstream gave up on the streaming body — retrying a streaming
/// request can result in duplicated assistant turns.
fn is_retryable_status(status: u16) -> bool {
    status == 429 || (500..600).contains(&status)
}

/// Honor `Retry-After: <seconds>` from rate-limit responses. Returns
/// None if the header is missing or unparseable; caller falls back to
/// exponential backoff in that case.
fn parse_retry_after(headers: &HeaderMap) -> Option<Duration> {
    let raw = headers.get(reqwest::header::RETRY_AFTER)?.to_str().ok()?;
    let secs: u64 = raw.trim().parse().ok()?;
    // Cap Retry-After at 30s: some servers send absurd values during
    // outages and we'd rather give up than block the agent for minutes.
    Some(Duration::from_secs(secs.min(30)))
}

/// Exponential backoff for retry attempt N (0-indexed). 500ms × 3^N
/// gives 500ms, 1.5s, 4.5s — gentle enough to be polite, aggressive
/// enough to recover from transient blips.
fn backoff_for_attempt(attempt: u32) -> Duration {
    let base_ms: u64 = 500;
    let factor: u64 = 3u64.saturating_pow(attempt);
    Duration::from_millis(base_ms.saturating_mul(factor))
}

/// Extract a user-friendly error message from an upstream error response.
///
/// Format precedence:
///   1. JSON `{ "error": { "message": "..." } }` (OpenAI / DeepSeek / etc.)
///   2. JSON `{ "error": "..." }` (some providers)
///   3. JSON `{ "message": "..." }` (top-level flat)
///   4. HTML body → extract `<title>` contents (nginx / openresty error
///      pages render `<title>500 Internal Server Error</title>` so the
///      title is exactly the human-readable summary). Falls back to
///      "Upstream returned HTTP <N>" if the title is empty or missing.
///   5. Other non-JSON body → first 200 chars verbatim
///   6. Empty body → generic "Upstream returned <status>"
///
/// We deliberately drop the technical "Failed to create event source"
/// wrapper that reqwest_eventsource emits by default — users only see
/// the provider's own message.
fn extract_upstream_error_message(status: u16, body: &str) -> String {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return format!("Upstream returned HTTP {status}");
    }

    // JSON-shaped error envelopes (most providers)
    if let Ok(parsed) = serde_json::from_str::<Value>(trimmed) {
        // Nested OpenAI / DeepSeek / Anthropic shape
        if let Some(msg) = parsed
            .get("error")
            .and_then(|e| e.get("message"))
            .and_then(|v| v.as_str())
        {
            return msg.to_string();
        }
        // Flat error string
        if let Some(msg) = parsed.get("error").and_then(|v| v.as_str()) {
            return msg.to_string();
        }
        // Top-level message
        if let Some(msg) = parsed.get("message").and_then(|v| v.as_str()) {
            return msg.to_string();
        }
    }

    // HTML error pages (nginx / openresty / cloudfront etc. return
    // verbose markup when something upstream of the LLM goes wrong —
    // wrong route, gateway timeout, model not deployed, etc.). The
    // <title> tag is the human-friendly one-liner; surface that
    // instead of dumping HTML at the user.
    if looks_like_html(trimmed) {
        if let Some(title) = extract_html_title(trimmed) {
            return title;
        }
        return format!("Upstream returned HTTP {status}");
    }

    // Other non-JSON shapes — truncate to keep the UI readable.
    let truncated: String = trimmed.chars().take(200).collect();
    if truncated.is_empty() {
        format!("Upstream returned HTTP {status}")
    } else {
        truncated
    }
}

/// Cheap HTML-shape detection. We don't need a real parser — just
/// enough to decide between "JSON error envelope path" and "HTML error
/// page path". Both `<!doctype html>` and bare `<html>` / `<head>` /
/// `<body>` are common across the error pages we see in the wild.
fn looks_like_html(body: &str) -> bool {
    let head: String = body
        .trim_start()
        .chars()
        .take(64)
        .collect::<String>()
        .to_ascii_lowercase();
    head.starts_with("<!doctype html")
        || head.starts_with("<html")
        || head.starts_with("<head")
        || head.starts_with("<body")
        // Edge case: some servers send `<center><h1>` without an outer
        // html element. Catch the most distinctive nginx / openresty
        // prefixes too.
        || head.starts_with("<center>")
}

/// Pull the text inside `<title>...</title>` from an HTML body. Case-
/// insensitive on the tag, trims whitespace inside. Returns None if
/// the title is empty or no title tag is present.
fn extract_html_title(body: &str) -> Option<String> {
    let lower = body.to_ascii_lowercase();
    let start_tag = lower.find("<title")?;
    // Skip past any attributes and the closing `>` of the open tag.
    let after_open = body[start_tag..].find('>').map(|i| start_tag + i + 1)?;
    let end_tag_rel = lower[after_open..].find("</title>")?;
    let title = body[after_open..after_open + end_tag_rel].trim();
    if title.is_empty() {
        None
    } else {
        Some(title.to_string())
    }
}

/// Connection-time outcome from `open_with_retry`. The buffered events
/// vector carries any messages we accidentally consumed while validating
/// the connection (servers that skip the SSE Open preamble go straight
/// to the first content delta).
struct OpenedStream {
    es: EventSource,
    pending: Vec<Event>,
}

/// Open an EventSource with transient-error retry. Returns when:
///   • The server emits SSE `Open` (healthy stream, no buffered events)
///   • The server emits a Message directly (buffered into `pending`)
///   • A non-retryable error fires (returns Err with the upstream's
///     own message extracted from the response body)
///   • All `MAX_RETRY_ATTEMPTS` attempts have been exhausted
async fn open_with_retry<F>(make_request: F) -> Result<OpenedStream, String>
where
    F: Fn() -> Result<reqwest::RequestBuilder, String>,
{
    let mut last_err = String::from("All retry attempts failed");
    for attempt in 0..MAX_RETRY_ATTEMPTS {
        let request = make_request()?;
        let mut es = match EventSource::new(request) {
            Ok(es) => es,
            Err(e) => return Err(format!("Could not build request: {e}")),
        };

        // Pull events until we either confirm the connection (Open or
        // first Message) or hit an error worth retrying / surfacing.
        match es.next().await {
            Some(Ok(Event::Open)) => {
                return Ok(OpenedStream {
                    es,
                    pending: Vec::new(),
                });
            }
            Some(Ok(ev @ Event::Message(_))) => {
                // Server skipped Open — buffer the message so the
                // streaming consumer doesn't lose it.
                return Ok(OpenedStream {
                    es,
                    pending: vec![ev],
                });
            }
            Some(Err(reqwest_eventsource::Error::InvalidStatusCode(status, response))) => {
                let retry_after = parse_retry_after(response.headers());
                let body = response.text().await.unwrap_or_default();
                let message = extract_upstream_error_message(status.as_u16(), &body);
                if is_retryable_status(status.as_u16()) && attempt + 1 < MAX_RETRY_ATTEMPTS {
                    let wait = retry_after.unwrap_or_else(|| backoff_for_attempt(attempt));
                    log::info!(
                        "[LLM] Upstream {} (attempt {}/{}), retrying in {:?}: {}",
                        status,
                        attempt + 1,
                        MAX_RETRY_ATTEMPTS,
                        wait,
                        message
                    );
                    tokio::time::sleep(wait).await;
                    last_err = message;
                    continue;
                }
                return Err(message);
            }
            Some(Err(reqwest_eventsource::Error::Transport(e))) => {
                if attempt + 1 < MAX_RETRY_ATTEMPTS {
                    let wait = backoff_for_attempt(attempt);
                    log::info!(
                        "[LLM] Transport error (attempt {}/{}), retrying in {:?}: {}",
                        attempt + 1,
                        MAX_RETRY_ATTEMPTS,
                        wait,
                        e
                    );
                    tokio::time::sleep(wait).await;
                    last_err = format!("Connection error: {e}");
                    continue;
                }
                return Err(format!("Connection error: {e}"));
            }
            Some(Err(e)) => return Err(format!("SSE setup error: {e}")),
            None => {
                if attempt + 1 < MAX_RETRY_ATTEMPTS {
                    tokio::time::sleep(backoff_for_attempt(attempt)).await;
                    last_err = "Upstream closed connection immediately".to_string();
                    continue;
                }
                return Err("Upstream closed connection immediately".to_string());
            }
        }
    }
    Err(last_err)
}

fn message_to_anthropic_json(m: &Message) -> Value {
    match &m.content {
        MessageContent::Text(text) => json!({"role": m.role, "content": text}),
        MessageContent::Blocks(blocks) => {
            let content: Vec<Value> = blocks
                .iter()
                .map(|b| match b {
                    ContentBlock::Text { text } => json!({"type": "text", "text": text}),
                    ContentBlock::Thinking {
                        thinking,
                        signature,
                    } => {
                        // Anthropic-compat thinking-mode endpoints (e.g. DeepSeek's
                        // /anthropic) require both fields on round-trip or 400.
                        let mut block = json!({"type": "thinking", "thinking": thinking});
                        if !signature.is_empty() {
                            block["signature"] = json!(signature);
                        }
                        block
                    }
                    ContentBlock::ToolUse { id, name, input } => json!({
                        "type": "tool_use", "id": id, "name": name, "input": input
                    }),
                    ContentBlock::ToolResult {
                        tool_use_id,
                        content,
                    } => json!({
                        "type": "tool_result", "tool_use_id": tool_use_id, "content": content
                    }),
                })
                .collect();
            json!({"role": m.role, "content": content})
        }
    }
}

// ── Tests ──
#[cfg(test)]
mod tests {
    use super::*;

    // ---- is_retryable_status ----

    #[test]
    fn retryable_includes_429_and_5xx() {
        assert!(is_retryable_status(429));
        assert!(is_retryable_status(500));
        assert!(is_retryable_status(502));
        assert!(is_retryable_status(503));
        assert!(is_retryable_status(599));
    }

    #[test]
    fn retryable_excludes_4xx_auth_and_2xx() {
        assert!(!is_retryable_status(200));
        assert!(!is_retryable_status(400));
        assert!(!is_retryable_status(401));
        assert!(!is_retryable_status(403));
        assert!(!is_retryable_status(404));
        assert!(!is_retryable_status(409));
        assert!(!is_retryable_status(408)); // see docstring: 408 not retried
        assert!(!is_retryable_status(600));
    }

    // ---- parse_retry_after ----

    #[test]
    fn parses_retry_after_seconds() {
        let mut h = HeaderMap::new();
        h.insert(reqwest::header::RETRY_AFTER, HeaderValue::from_static("5"));
        assert_eq!(parse_retry_after(&h), Some(Duration::from_secs(5)));
    }

    #[test]
    fn caps_retry_after_at_30s() {
        let mut h = HeaderMap::new();
        h.insert(
            reqwest::header::RETRY_AFTER,
            HeaderValue::from_static("999"),
        );
        // Server tried to make us wait 999s; cap at 30s.
        assert_eq!(parse_retry_after(&h), Some(Duration::from_secs(30)));
    }

    #[test]
    fn retry_after_missing_returns_none() {
        let h = HeaderMap::new();
        assert_eq!(parse_retry_after(&h), None);
    }

    #[test]
    fn retry_after_unparseable_returns_none() {
        let mut h = HeaderMap::new();
        // HTTP-date format (legal per RFC 7231) — we don't support it yet,
        // so we fall back to exponential backoff.
        h.insert(
            reqwest::header::RETRY_AFTER,
            HeaderValue::from_static("Wed, 21 Oct 2026 07:28:00 GMT"),
        );
        assert_eq!(parse_retry_after(&h), None);
    }

    // ---- backoff_for_attempt ----

    #[test]
    fn backoff_grows_exponentially() {
        assert_eq!(backoff_for_attempt(0), Duration::from_millis(500));
        assert_eq!(backoff_for_attempt(1), Duration::from_millis(1500));
        assert_eq!(backoff_for_attempt(2), Duration::from_millis(4500));
    }

    #[test]
    fn backoff_does_not_overflow_at_huge_attempts() {
        // u32::MAX would normally overflow u64::pow; saturating_pow + mul
        // protect us.
        let d = backoff_for_attempt(u32::MAX);
        assert!(d.as_millis() > 0); // doesn't panic
    }

    // ---- extract_upstream_error_message ----

    #[test]
    fn extracts_nested_error_message_openai_shape() {
        let body = r#"{"error":{"message":"Invalid API Key","code":"invalid_api_key"}}"#;
        assert_eq!(extract_upstream_error_message(401, body), "Invalid API Key");
    }

    #[test]
    fn extracts_nested_error_message_deepseek_shape() {
        // DeepSeek's actual response shape per the user's e2e logs.
        let body = r#"{
            "error": {
                "message": "Invalid API Key",
                "param": "Please provide valid API Key",
                "code": "401",
                "type": "invalid_key"
            }
        }"#;
        assert_eq!(extract_upstream_error_message(401, body), "Invalid API Key");
    }

    #[test]
    fn extracts_flat_error_string() {
        let body = r#"{"error":"Rate limit exceeded"}"#;
        assert_eq!(
            extract_upstream_error_message(429, body),
            "Rate limit exceeded"
        );
    }

    #[test]
    fn extracts_top_level_message() {
        let body = r#"{"message":"Quota exceeded"}"#;
        assert_eq!(extract_upstream_error_message(403, body), "Quota exceeded");
    }

    #[test]
    fn falls_back_to_truncated_body_when_not_json() {
        let body = "Internal Server Error\nReason: backend timeout";
        let out = extract_upstream_error_message(500, body);
        assert!(out.contains("Internal Server Error"), "got: {out}");
    }

    #[test]
    fn truncates_huge_non_json_body() {
        let body = "x".repeat(5000);
        let out = extract_upstream_error_message(500, &body);
        assert!(out.len() <= 200, "got {} chars", out.len());
    }

    #[test]
    fn empty_body_falls_back_to_generic_message() {
        assert_eq!(
            extract_upstream_error_message(503, ""),
            "Upstream returned HTTP 503"
        );
        assert_eq!(
            extract_upstream_error_message(503, "   "),
            "Upstream returned HTTP 503"
        );
    }

    #[test]
    fn malformed_json_falls_back_to_truncated_body() {
        let body = "{not valid json";
        let out = extract_upstream_error_message(500, body);
        // Not "Failed to parse" — we want the user to see something useful
        assert!(out.contains("not valid json"), "got: {out}");
    }

    // ---- HTML error page handling ----

    #[test]
    fn extracts_title_from_nginx_500_page() {
        let body = "<html>\n<head><title>500 Internal Server Error</title></head>\n<body>\n<center><h1>500 Internal Server Error</h1></center>\n<hr><center>nginx</center>\n</body>\n</html>";
        assert_eq!(
            extract_upstream_error_message(500, body),
            "500 Internal Server Error"
        );
    }

    #[test]
    fn extracts_title_from_openresty_404_page() {
        let body = "<html>\n<head><title>404 Not Found</title></head>\n<body>\n<center><h1>404 Not Found</h1></center>\n<hr><center>openresty</center>\n</body>\n</html>";
        assert_eq!(extract_upstream_error_message(404, body), "404 Not Found");
    }

    #[test]
    fn extracts_title_from_html_with_doctype() {
        let body = "<!DOCTYPE html>\n<html><head><title>502 Bad Gateway</title></head><body>...</body></html>";
        assert_eq!(extract_upstream_error_message(502, body), "502 Bad Gateway");
    }

    #[test]
    fn extracts_title_case_insensitive() {
        let body = "<HTML><HEAD><TITLE>503 Service Unavailable</TITLE></HEAD></HTML>";
        assert_eq!(
            extract_upstream_error_message(503, body),
            "503 Service Unavailable"
        );
    }

    #[test]
    fn extracts_title_with_attributes() {
        let body = "<html><head><title lang=\"en\">524 Origin Timeout</title></head></html>";
        assert_eq!(
            extract_upstream_error_message(524, body),
            "524 Origin Timeout"
        );
    }

    #[test]
    fn html_without_title_falls_back_to_generic() {
        let body = "<html><body><h1>Service error</h1></body></html>";
        assert_eq!(
            extract_upstream_error_message(500, body),
            "Upstream returned HTTP 500"
        );
    }

    #[test]
    fn html_with_empty_title_falls_back_to_generic() {
        let body = "<html><head><title></title></head><body>...</body></html>";
        assert_eq!(
            extract_upstream_error_message(500, body),
            "Upstream returned HTTP 500"
        );
    }

    #[test]
    fn looks_like_html_detects_common_shapes() {
        assert!(looks_like_html("<html>"));
        assert!(looks_like_html("<HTML>"));
        assert!(looks_like_html("<!DOCTYPE html>"));
        assert!(looks_like_html("<head>"));
        assert!(looks_like_html("<body>"));
        assert!(looks_like_html("<center>"));
        assert!(looks_like_html("  \n  <html>"));

        assert!(!looks_like_html("plain text"));
        assert!(!looks_like_html("{\"error\": \"...\"}"));
        assert!(!looks_like_html("Error: connection refused"));
    }

    #[test]
    fn extract_title_handles_whitespace() {
        // Some servers pad the title with whitespace inside the tag.
        let body = "<html><head><title>   500 Internal Server Error   </title></head></html>";
        assert_eq!(
            extract_upstream_error_message(500, body),
            "500 Internal Server Error"
        );
    }
}
