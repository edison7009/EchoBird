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
use sha2::{Digest, Sha256};
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LlmProvider {
    OpenAI,
    Anthropic,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThinkingOrigin {
    pub provider: LlmProvider,
    pub endpoint: String,
    pub model: String,
}

impl ThinkingOrigin {
    fn from_config(config: &LlmConfig) -> Self {
        let base = config.base_url.trim().trim_end_matches('/');
        let normalized = url::Url::parse(base)
            .map(|mut url| {
                url.set_fragment(None);
                url.to_string().trim_end_matches('/').to_string()
            })
            .unwrap_or_else(|_| base.to_string());
        Self {
            provider: config.provider,
            endpoint: hex::encode(Sha256::digest(normalized.as_bytes())),
            model: config.model.clone(),
        }
    }
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
        #[serde(default, skip_serializing_if = "Option::is_none")]
        origin: Option<ThinkingOrigin>,
    },
    #[serde(rename = "redacted_thinking")]
    RedactedThinking {
        data: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        origin: Option<ThinkingOrigin>,
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
    ThinkingBlockStart {
        thinking: String,
        signature: String,
    },
    RedactedThinking(String),
    Thinking(String),
    /// Anthropic's `signature_delta` carries the current block's full signature.
    /// Replace that block's signature rather than concatenate snapshots.
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

    pub fn thinking_origin(&self) -> ThinkingOrigin {
        ThinkingOrigin::from_config(&self.config)
    }

    pub fn summary_client(&self) -> Self {
        let mut client = self.clone();
        client.config.reasoning_effort = None;
        client
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
        msgs.extend(messages_to_openai_json(messages, &self.config));

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
        super::reasoning_effort::apply(&mut body, &self.config)?;

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
            // App-identification for usage-analytics aggregators. OpenRouter
            // introduced HTTP-Referer + X-Title (now X-OpenRouter-Title) and
            // Helicone / LangSmith / similar proxies copied the convention —
            // these headers let those dashboards attribute traffic to
            // EchoBird instead of bucketing it as "unknown". OpenRouter still
            // accepts X-Title as a backwards-compatible alias, so we send both
            // the old and new names; endpoints that don't recognize them
            // silently ignore them, so safe to send always.
            headers.insert(
                "HTTP-Referer",
                HeaderValue::from_static("https://echobird.ai"),
            );
            headers.insert("X-Title", HeaderValue::from_static("EchoBird"));
            headers.insert("X-OpenRouter-Title", HeaderValue::from_static("EchoBird"));
            Ok(http.post(&url_owned).headers(headers).json(&body_owned))
        };

        // OpenAI is the workhorse path: full retry budget on transient
        // 429 / 5xx / transport blips.
        let OpenedStream { mut es, pending } =
            open_with_retry(make_request, MAX_RETRY_ATTEMPTS).await?;

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
                if tx.is_closed() {
                    es.close();
                    break;
                }
                let event = if let Some(buffered) = events.pop_front() {
                    Some(buffered)
                } else {
                    tokio::select! {
                        _ = tx.closed() => {
                            es.close();
                            None
                        }
                        event = tokio::time::timeout(SSE_CHUNK_TIMEOUT, es.next()) => match event {
                            Ok(maybe_event) => maybe_event,
                            Err(_) => {
                                let _ = tx.send(LlmEvent::Error(format!(
                                    "Upstream stream stalled (no data for {}s)",
                                    SSE_CHUNK_TIMEOUT.as_secs()
                                ))).await;
                                None
                            }
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
        // URL resolution — handle every shape users (and our model
        // directory templates) realistically put in `anthropic_url`:
        //
        //   1. already includes "/messages"          → use as-is
        //   2. local unified proxy /anthropic        → append "/messages"
        //      (our local_llm/proxy.rs strips /v1
        //      internally; remote vendors do NOT)
        //   3. ends with "/v1"                        → append "/messages"
        //      (vLLM / sglang / OpenAI-compat style
        //      where users routinely include /v1 in
        //      the user-facing baseURL)
        //   4. otherwise (bare host, /anthropic on
        //      remote, /v2/something, etc.)           → append "/v1/messages"
        //      (standard Anthropic-style path; works
        //      for Anthropic official, DeepSeek
        //      /anthropic, Xiaomi /anthropic, GLM
        //      /anthropic — vendors who once tolerated
        //      our /anthropic+/messages shortcut now
        //      get a fully-spec'd path)
        let is_local_proxy = base.contains("127.0.0.1") || base.contains("localhost");
        let url = if base.contains("/messages") {
            base.to_string()
        } else if (base.ends_with("/anthropic") && is_local_proxy) || base.ends_with("/v1") {
            // Local unified proxy /anthropic shortcut, OR remote URL
            // already terminating in /v1 — append /messages only.
            format!("{}/messages", base)
        } else {
            // Bare host or remote /anthropic path-prefix → canonical
            // Anthropic-spec path.
            format!("{}/v1/messages", base)
        };

        // Build messages (Anthropic format). Normalize first so consecutive
        // user{tool_result} messages collapse into one — required by the
        // Anthropic API's tool_use↔tool_result pairing rule, see
        // `normalize_anthropic_messages`.
        let msgs = messages_to_anthropic_json(messages, &self.config);

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
        super::reasoning_effort::apply(&mut body, &self.config)?;

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
            // Mirrors the OpenAI-path injection — see comment above.
            headers.insert(
                "HTTP-Referer",
                HeaderValue::from_static("https://echobird.ai"),
            );
            headers.insert("X-Title", HeaderValue::from_static("EchoBird"));
            Ok(http.post(&url_owned).headers(headers).json(&body_owned))
        };

        // Anthropic path is a "probe before falling back to OpenAI":
        // when the user has both `anthropicUrl` + `baseUrl` configured,
        // agent_loop tries Anthropic first and downgrades on failure.
        // Retrying a non-existent /anthropic/messages endpoint here
        // would burn 30-60s of connect_timeout before the downgrade
        // can even fire (observed: mimo's /anthropic returns transport
        // error then 404). Single attempt = fail fast = the user sees
        // their first response in seconds instead of a minute.
        let OpenedStream { mut es, pending } = open_with_retry(make_request, 1).await?;

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
                if tx.is_closed() {
                    es.close();
                    break;
                }
                let event = if let Some(buffered) = events.pop_front() {
                    Some(buffered)
                } else {
                    tokio::select! {
                        _ = tx.closed() => {
                            es.close();
                            None
                        }
                        event = tokio::time::timeout(SSE_CHUNK_TIMEOUT, es.next()) => match event {
                            Ok(maybe_event) => maybe_event,
                            Err(_) => {
                                let _ = tx.send(LlmEvent::Error(format!(
                                    "Upstream stream stalled (no data for {}s)",
                                    SSE_CHUNK_TIMEOUT.as_secs()
                                ))).await;
                                None
                            }
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
                                                let _ = tx
                                                    .send(LlmEvent::ThinkingBlockStart {
                                                        thinking: block["thinking"]
                                                            .as_str()
                                                            .unwrap_or("")
                                                            .to_string(),
                                                        signature: block["signature"]
                                                            .as_str()
                                                            .unwrap_or("")
                                                            .to_string(),
                                                    })
                                                    .await;
                                            }
                                            Some("redacted_thinking") => {
                                                if let Some(data) = block["data"].as_str() {
                                                    let _ = tx
                                                        .send(LlmEvent::RedactedThinking(
                                                            data.to_string(),
                                                        ))
                                                        .await;
                                                }
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

fn uses_deepseek_history(config: &LlmConfig) -> bool {
    matches!(
        config.model.trim().to_ascii_lowercase().as_str(),
        "deepseek-flash" | "deepseek-v4-pro" | "deepseek-v4-flash" | "deepseek-v4.1-flash"
    ) || url::Url::parse(&config.base_url)
        .is_ok_and(|endpoint| endpoint.host_str() == Some("api.deepseek.com"))
}

/// Project reasoning for this request without changing saved conversation history.
fn compatible_history(messages: &[Message], config: &LlmConfig) -> Vec<Message> {
    let target = ThinkingOrigin::from_config(config);
    let result_ids: std::collections::HashSet<&str> = messages
        .iter()
        .filter_map(|message| match &message.content {
            MessageContent::Blocks(blocks) => Some(blocks),
            _ => None,
        })
        .flatten()
        .filter_map(|block| match block {
            ContentBlock::ToolResult { tool_use_id, .. } => Some(tool_use_id.as_str()),
            _ => None,
        })
        .collect();
    messages
        .iter()
        .filter_map(|message| {
            let mut projected = message.clone();
            if message.role == "assistant" {
                if let MessageContent::Blocks(blocks) = &mut projected.content {
                    blocks.retain(|block| match block {
                        ContentBlock::Thinking {
                            signature, origin, ..
                        } => match origin {
                            Some(origin) => origin == &target,
                            None => match config.provider {
                                LlmProvider::OpenAI => signature.is_empty(),
                                LlmProvider::Anthropic => {
                                    !signature.is_empty() || uses_deepseek_history(config)
                                }
                            },
                        },
                        ContentBlock::RedactedThinking { origin, .. } => {
                            config.provider == LlmProvider::Anthropic
                                && !uses_deepseek_history(config)
                                && origin.as_ref().is_none_or(|origin| origin == &target)
                        }
                        _ => true,
                    });
                    if uses_deepseek_history(config)
                        && !blocks.iter().any(|block| matches!(block, ContentBlock::Thinking { .. }))
                        && blocks.iter().any(|block| matches!(block, ContentBlock::ToolUse { id, .. } if result_ids.contains(id.as_str())))
                    {
                        // A completed tool turn imported from another model has no
                        // local reasoning. Keep the required carrier request-local.
                        blocks.insert(0, ContentBlock::Thinking {
                            thinking: String::new(),
                            signature: String::new(),
                            origin: Some(target.clone()),
                        });
                    }
                    if blocks.is_empty() {
                        return None;
                    }
                }
            }
            Some(projected)
        })
        .collect()
}

fn messages_to_openai_json(messages: &[Message], config: &LlmConfig) -> Vec<Value> {
    let mut out = Vec::new();
    for message in compatible_history(messages, config) {
        if message.role == "user" || message.role == "tool" {
            if let MessageContent::Blocks(blocks) = &message.content {
                if blocks
                    .iter()
                    .any(|block| matches!(block, ContentBlock::ToolResult { .. }))
                {
                    // One OpenAI tool message per result, including batched results
                    // saved by a different protocol. Keep ordinary user text too.
                    for block in blocks {
                        if let ContentBlock::ToolResult {
                            tool_use_id,
                            content,
                        } = block
                        {
                            out.push(json!({
                                "role": "tool", "tool_call_id": tool_use_id,
                                "content": content
                            }));
                        }
                    }
                    let text: Vec<&str> = blocks
                        .iter()
                        .filter_map(|block| match block {
                            ContentBlock::Text { text } => Some(text.as_str()),
                            _ => None,
                        })
                        .collect();
                    if !text.is_empty() {
                        out.push(json!({"role":"user", "content":text.join("")}));
                    }
                    continue;
                }
            }
        }
        let mut serialized = message_to_openai_json(&message);
        if message.role == "assistant"
            && uses_deepseek_history(config)
            && serialized.get("reasoning_content").is_none()
        {
            // DeepSeek requires a reasoning field on every prior assistant reply
            // when tools are sent, including final replies imported from another model.
            serialized["reasoning_content"] = json!("");
        }
        out.push(serialized);
    }
    out
}

fn messages_to_anthropic_json(messages: &[Message], config: &LlmConfig) -> Vec<Value> {
    normalize_anthropic_messages(&compatible_history(messages, config))
        .iter()
        .map(message_to_anthropic_json)
        .collect()
}

fn message_to_openai_json(m: &Message) -> Value {
    match &m.content {
        MessageContent::Text(text) => json!({"role": m.role, "content": text}),
        MessageContent::Blocks(blocks) => {
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
///   • All `max_attempts` attempts have been exhausted
///
/// `max_attempts` controls the retry budget. Pass `MAX_RETRY_ATTEMPTS`
/// (3) for the OpenAI workhorse path where retries genuinely help with
/// transient blips. Pass `1` for the Anthropic probe path where the
/// endpoint is just a "first try before falling back" — retrying a
/// non-existent /anthropic/messages route 3 times wastes ~30-60s per
/// connect_timeout before the agent loop downgrades to OpenAI.
async fn open_with_retry<F>(make_request: F, max_attempts: u32) -> Result<OpenedStream, String>
where
    F: Fn() -> Result<reqwest::RequestBuilder, String>,
{
    let mut last_err = String::from("All retry attempts failed");
    for attempt in 0..max_attempts {
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
                if is_retryable_status(status.as_u16()) && attempt + 1 < max_attempts {
                    let wait = retry_after.unwrap_or_else(|| backoff_for_attempt(attempt));
                    log::info!(
                        "[LLM] Upstream {} (attempt {}/{}), retrying in {:?}: {}",
                        status,
                        attempt + 1,
                        max_attempts,
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
                if attempt + 1 < max_attempts {
                    let wait = backoff_for_attempt(attempt);
                    log::info!(
                        "[LLM] Transport error (attempt {}/{}), retrying in {:?}: {}",
                        attempt + 1,
                        max_attempts,
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
                if attempt + 1 < max_attempts {
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

/// Merge consecutive `user` or `tool` messages whose content is *only* `tool_result`
/// blocks into a single user message. Anthropic's Messages API requires
/// that every `tool_use` from the prior assistant turn has its matching
/// `tool_result` in the *immediately following* user message — splitting
/// the results across N separate user messages produces:
///     "messages.<i>.tool_use ids were found without tool_result blocks
///      immediately after: <ids>. Each tool_use block must have a
///      corresponding tool_result block in the next message."
///
/// (issue #42: 5 PowerShell tool_uses → 5 separate user{tool_result}
/// messages → 400 about the 4 unmatched ids.)
///
/// Non-tool-result user messages (plain text, or mixed content) and all
/// non-user messages pass through unchanged. Order within the merged
/// block list is preserved.
fn normalize_anthropic_messages(messages: &[Message]) -> Vec<Message> {
    fn is_pure_tool_result(m: &Message) -> bool {
        if m.role != "user" && m.role != "tool" {
            return false;
        }
        match &m.content {
            MessageContent::Blocks(blocks) => {
                !blocks.is_empty()
                    && blocks
                        .iter()
                        .all(|b| matches!(b, ContentBlock::ToolResult { .. }))
            }
            _ => false,
        }
    }

    let mut out: Vec<Message> = Vec::with_capacity(messages.len());
    let mut i = 0;
    while i < messages.len() {
        if is_pure_tool_result(&messages[i]) {
            // Walk forward collecting every consecutive pure-tool_result user.
            let mut merged_blocks: Vec<ContentBlock> = Vec::new();
            while i < messages.len() && is_pure_tool_result(&messages[i]) {
                if let MessageContent::Blocks(blocks) = &messages[i].content {
                    merged_blocks.extend(blocks.iter().cloned());
                }
                i += 1;
            }
            out.push(Message {
                role: "user".into(),
                content: MessageContent::Blocks(merged_blocks),
            });
        } else {
            out.push(messages[i].clone());
            i += 1;
        }
    }
    out
}

fn message_to_anthropic_json(m: &Message) -> Value {
    let role = if m.role == "tool" { "user" } else { &m.role };
    match &m.content {
        MessageContent::Text(text) => json!({"role": role, "content": text}),
        MessageContent::Blocks(blocks) => {
            let content: Vec<Value> = blocks
                .iter()
                .map(|b| match b {
                    ContentBlock::Text { text } => json!({"type": "text", "text": text}),
                    ContentBlock::Thinking {
                        thinking,
                        signature,
                        ..
                    } => {
                        // Anthropic-compat thinking-mode endpoints (e.g. DeepSeek's
                        // /anthropic) require both fields on round-trip or 400.
                        json!({"type": "thinking", "thinking": thinking, "signature": signature})
                    }
                    ContentBlock::RedactedThinking { data, .. } => {
                        json!({"type": "redacted_thinking", "data": data})
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
            json!({"role": role, "content": content})
        }
    }
}

// ── Tests ──
#[cfg(test)]
mod tests {
    use super::*;

    fn protocol_config(provider: LlmProvider) -> LlmConfig {
        LlmConfig {
            provider,
            base_url: "https://fixture.invalid/v1".into(),
            api_key: "fixture-key".into(),
            model: "fixture-model".into(),
            reasoning_effort: None,
        }
    }

    #[test]
    fn protocol_switch_preserves_batched_results_and_user_text_for_openai() {
        let mut result = tool_result_msg(&["a", "b"]);
        if let MessageContent::Blocks(blocks) = &mut result.content {
            blocks.push(ContentBlock::Text {
                text: "Continue with the saved results".into(),
            });
        }
        let input = vec![assistant_with_tool_uses(&["a", "b"]), result];
        let before = serde_json::to_value(&input).unwrap();
        let projected = messages_to_openai_json(&input, &protocol_config(LlmProvider::OpenAI));
        assert_eq!(projected.len(), 4);
        assert_eq!(projected[1]["tool_call_id"], "a");
        assert_eq!(projected[2]["tool_call_id"], "b");
        assert_eq!(projected[3]["role"], "user");
        assert_eq!(projected[3]["content"], "Continue with the saved results");
        assert_eq!(serde_json::to_value(&input).unwrap(), before);
    }

    #[test]
    fn protocol_switch_keeps_reasoning_only_for_its_original_model_and_endpoint() {
        for provider in [LlmProvider::OpenAI, LlmProvider::Anthropic] {
            let config = protocol_config(provider);
            let signature = if provider == LlmProvider::Anthropic {
                "opaque-signature"
            } else {
                ""
            };
            let mut input = vec![Message {
                role: "assistant".into(),
                content: MessageContent::Blocks(vec![
                    ContentBlock::Thinking {
                        thinking: "private reasoning".into(),
                        signature: signature.into(),
                        origin: Some(ThinkingOrigin::from_config(&config)),
                    },
                    ContentBlock::Text {
                        text: "answer".into(),
                    },
                ]),
            }];
            if provider == LlmProvider::Anthropic {
                if let MessageContent::Blocks(blocks) = &mut input[0].content {
                    blocks.insert(
                        1,
                        ContentBlock::RedactedThinking {
                            data: "opaque-fixture".into(),
                            origin: Some(ThinkingOrigin::from_config(&config)),
                        },
                    );
                }
            }
            let before = serde_json::to_value(&input).unwrap();
            let replayed = compatible_history(&input, &config);
            assert_eq!(serde_json::to_value(&replayed).unwrap(), before);
            let mut changed = config.clone();
            for change in ["provider", "endpoint", "model"] {
                changed.clone_from(&config);
                match change {
                    "provider" => {
                        changed.provider = if provider == LlmProvider::OpenAI {
                            LlmProvider::Anthropic
                        } else {
                            LlmProvider::OpenAI
                        }
                    }
                    "endpoint" => changed.base_url = "https://other.invalid/v1".into(),
                    "model" => changed.model = "other-model".into(),
                    _ => unreachable!(),
                }
                let projected = compatible_history(&input, &changed);
                if let MessageContent::Blocks(blocks) = &projected[0].content {
                    assert_eq!(blocks.len(), 1);
                    assert!(matches!(&blocks[0], ContentBlock::Text { text } if text == "answer"));
                } else {
                    panic!("text must survive incompatible reasoning");
                }
            }
            assert_eq!(serde_json::to_value(&input).unwrap(), before);
        }
    }

    #[test]
    fn protocol_switch_loads_legacy_reasoning_and_omits_foreign_reasoning_only_turns() {
        let legacy: Message = serde_json::from_value(json!({
            "role":"assistant", "content":[{
                "type":"thinking", "thinking":"legacy reasoning", "signature":""
            }]
        }))
        .unwrap();
        let input = vec![legacy];
        assert_eq!(
            compatible_history(&input, &protocol_config(LlmProvider::OpenAI)).len(),
            1
        );
        assert!(
            messages_to_anthropic_json(&input, &protocol_config(LlmProvider::Anthropic)).is_empty()
        );
        let mut compatible = protocol_config(LlmProvider::Anthropic);
        compatible.model = "deepseek-v4-pro".into();
        let projected = messages_to_anthropic_json(&input, &compatible);
        assert_eq!(projected[0]["content"][0]["signature"], "");
    }

    #[test]
    fn legacy_redacted_reasoning_survives_native_replay_without_entering_other_protocols() {
        let input: Vec<Message> = serde_json::from_value(json!([{
            "role":"assistant", "content":[
                {"type":"redacted_thinking", "data":"opaque-legacy-fixture"},
                {"type":"text", "text":"visible answer"}
            ]
        }]))
        .unwrap();
        let before = serde_json::to_value(&input).unwrap();
        let config = protocol_config(LlmProvider::Anthropic);
        let replay = compatible_history(&input, &config);
        assert_eq!(message_to_anthropic_json(&replay[0]), before[0]);
        let mut deepseek = config.clone();
        deepseek.model = "deepseek-v4-pro".into();
        for config in [protocol_config(LlmProvider::OpenAI), deepseek] {
            let projected = compatible_history(&input, &config);
            assert_eq!(
                serde_json::to_value(&projected).unwrap()[0]["content"],
                json!([{"type":"text", "text":"visible answer"}])
            );
        }
        assert_eq!(serde_json::to_value(&input).unwrap(), before);
    }

    #[test]
    fn thinking_origin_normalizes_endpoint_and_does_not_store_url_credentials() {
        let mut config = protocol_config(LlmProvider::OpenAI);
        config.base_url = "https://Fixture.invalid/v1/".into();
        let normalized = ThinkingOrigin::from_config(&config);
        assert_eq!(
            normalized,
            ThinkingOrigin::from_config(&protocol_config(LlmProvider::OpenAI))
        );
        config.base_url = "https://fixture.invalid/v1?api_key=secret".into();
        let serialized = serde_json::to_string(&ThinkingOrigin::from_config(&config)).unwrap();
        assert!(!serialized.contains("secret"));
        assert!(!serialized.contains("fixture.invalid"));
    }

    #[test]
    fn protocol_switch_adds_deepseek_carrier_only_for_completed_tool_turns() {
        let source_config = protocol_config(LlmProvider::OpenAI);
        let mut source = assistant_with_tool_uses(&["a"]);
        if let MessageContent::Blocks(blocks) = &mut source.content {
            blocks.insert(
                0,
                ContentBlock::Thinking {
                    thinking: "foreign reasoning".into(),
                    signature: String::new(),
                    origin: Some(ThinkingOrigin::from_config(&source_config)),
                },
            );
        }
        let input = vec![source.clone(), tool_result_msg(&["a"])];
        let before = serde_json::to_value(&input).unwrap();
        for provider in [LlmProvider::OpenAI, LlmProvider::Anthropic] {
            let mut config = protocol_config(provider);
            config.model = "deepseek-v4-pro".into();
            let projected = if provider == LlmProvider::OpenAI {
                messages_to_openai_json(&input, &config)
            } else {
                messages_to_anthropic_json(&input, &config)
            };
            if provider == LlmProvider::OpenAI {
                assert_eq!(projected[0]["reasoning_content"], "");
                assert_eq!(projected[0]["tool_calls"][0]["id"], "a");
            } else {
                assert_eq!(
                    projected[0]["content"][0],
                    json!({"type":"thinking", "thinking":"", "signature":""})
                );
                assert_eq!(projected[0]["content"][1]["id"], "a");
            }
            assert!(!projected[0].to_string().contains("foreign reasoning"));
            let without_results = compatible_history(&[source.clone()], &config);
            if let MessageContent::Blocks(blocks) = &without_results[0].content {
                assert!(blocks
                    .iter()
                    .all(|block| !matches!(block, ContentBlock::Thinking { .. })));
            } else {
                panic!("tool uses remain blocks");
            }
            for unsupported in ["unknown", "deepseek-unrecognized", "claude-opus-4-6"] {
                config.model = unsupported.into();
                let projected = compatible_history(&input, &config);
                if let MessageContent::Blocks(blocks) = &projected[0].content {
                    assert!(blocks
                        .iter()
                        .all(|block| !matches!(block, ContentBlock::Thinking { .. })));
                }
            }
        }
        assert_eq!(serde_json::to_value(&input).unwrap(), before);
    }

    #[test]
    fn summary_client_keeps_model_and_transport_without_explicit_reasoning_effort() {
        let mut config = protocol_config(LlmProvider::OpenAI);
        config.reasoning_effort = Some("high".into());
        let client = LlmClient::new(config).unwrap();
        let summary = client.summary_client();
        assert_eq!(summary.thinking_origin(), client.thinking_origin());
        assert!(summary.config.reasoning_effort.is_none());
        assert_eq!(client.config.reasoning_effort.as_deref(), Some("high"));
    }

    #[tokio::test]
    async fn abandoned_receiver_closes_stalled_upstream_for_both_protocols() {
        use axum::{body::Body, extract::State, response::Response, routing::post, Json, Router};
        use bytes::Bytes;
        use std::convert::Infallible;
        use tokio_stream::wrappers::ReceiverStream;

        async fn stalled(
            State(disconnected): State<mpsc::UnboundedSender<bool>>,
            Json(body): Json<Value>,
        ) -> Response {
            let anthropic = body.get("system").is_some();
            let data = if anthropic {
                "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"started\"}}\n\n"
            } else {
                "data: {\"choices\":[{\"delta\":{\"content\":\"started\"}}]}\n\n"
            };
            let (tx, rx) = mpsc::channel::<Result<Bytes, Infallible>>(1);
            tx.send(Ok(Bytes::from_static(data.as_bytes())))
                .await
                .unwrap();
            tokio::spawn(async move {
                tx.closed().await;
                let _ = disconnected.send(anthropic);
            });
            Response::builder()
                .header("content-type", "text/event-stream")
                .body(Body::from_stream(ReceiverStream::new(rx)))
                .unwrap()
        }

        let (tx, mut disconnected) = mpsc::unbounded_channel();
        let router = Router::new()
            .route("/v1/chat/completions", post(stalled))
            .route("/v1/messages", post(stalled))
            .with_state(tx);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base_url = format!("http://{}/v1", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        for provider in [LlmProvider::OpenAI, LlmProvider::Anthropic] {
            let mut config = protocol_config(provider);
            config.base_url.clone_from(&base_url);
            let client = LlmClient::new(config).unwrap();
            let mut events = client
                .chat_stream(&[], &[], "fixture system")
                .await
                .unwrap();
            let first = tokio::time::timeout(Duration::from_secs(2), events.recv())
                .await
                .unwrap();
            assert!(matches!(first, Some(LlmEvent::TextDelta(text)) if text == "started"));
            // Positive control: the server remains live while the consumer exists.
            assert!(
                tokio::time::timeout(Duration::from_millis(100), disconnected.recv())
                    .await
                    .is_err()
            );
            drop(events);
            assert_eq!(
                tokio::time::timeout(Duration::from_secs(2), disconnected.recv())
                    .await
                    .unwrap(),
                Some(provider == LlmProvider::Anthropic)
            );
        }
        server.abort();
    }

    #[tokio::test]
    async fn protocol_switch_preserves_live_and_reloaded_history_over_http() {
        use axum::{extract::State, response::IntoResponse, routing::post, Json, Router};
        use std::sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        };

        async fn capture(
            State((tx, sequence)): State<(mpsc::UnboundedSender<Value>, Arc<AtomicUsize>)>,
            Json(body): Json<Value>,
        ) -> axum::response::Response {
            let anthropic = body.get("system").is_some();
            tx.send(body.clone()).unwrap();
            let messages = body["messages"].as_array().unwrap();
            let missing_reasoning = !anthropic
                && body["model"] == "deepseek-v4-pro"
                && body["tools"]
                    .as_array()
                    .is_some_and(|tools| !tools.is_empty())
                && messages.iter().any(|message| {
                    message["role"] == "assistant"
                        && message["reasoning_content"].as_str().is_none()
                });
            let valid = messages.iter().all(|message| {
                if anthropic {
                    matches!(message["role"].as_str(), Some("user" | "assistant"))
                        && message["content"].as_array().is_none_or(|blocks| {
                            blocks.iter().all(|block| {
                                block["type"] != "thinking" || block["signature"].as_str().is_some()
                            })
                        })
                } else {
                    message["content"]
                        .as_array()
                        .is_none_or(|blocks| blocks.iter().all(|block| block["type"] == "text"))
                }
            });
            if !valid || missing_reasoning {
                return (
                    axum::http::StatusCode::BAD_REQUEST,
                    Json(json!({
                        "error":{"message":if missing_reasoning {
                            "The reasoning_content in the thinking mode must be passed back to the API."
                        } else {
                            "invalid history projection"
                        }}
                    })),
                )
                    .into_response();
            }
            let id = format!("fixture-next-{}", sequence.fetch_add(1, Ordering::Relaxed));
            let data = if anthropic {
                [
                    ("content_block_delta", json!({"type":"content_block_delta", "delta":{"type":"thinking_delta", "thinking":"stream reasoning"}})),
                    ("content_block_delta", json!({"type":"content_block_delta", "delta":{"type":"signature_delta", "signature":"stream signature"}})),
                    ("content_block_delta", json!({"type":"content_block_delta", "delta":{"type":"text_delta", "text":"stream answer"}})),
                    ("content_block_start", json!({"type":"content_block_start", "content_block":{"type":"tool_use", "id":id, "name":"fixture_tool"}})),
                    ("content_block_delta", json!({"type":"content_block_delta", "delta":{"type":"input_json_delta", "partial_json":"{}"}})),
                    ("content_block_stop", json!({"type":"content_block_stop"})),
                    ("message_stop", json!({"type":"message_stop"})),
                ].into_iter().map(|(event, data)| format!("event: {event}\ndata: {data}\n\n")).collect()
            } else {
                let chunk = json!({"choices":[{"delta":{
                    "reasoning_content":"stream reasoning", "content":"stream answer",
                    "tool_calls":[{"index":0, "id":id, "function":{"name":"fixture_tool", "arguments":"{}"}}]
                }, "finish_reason":"tool_calls"}]});
                format!("data: {chunk}\n\ndata: [DONE]\n\n")
            };
            ([("content-type", "text/event-stream")], data).into_response()
        }

        let (tx, mut captured) = mpsc::unbounded_channel();
        let router = Router::new()
            .route("/v1/chat/completions", post(capture))
            .route("/v1/messages", post(capture))
            .with_state((tx, Arc::new(AtomicUsize::new(0))));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base_url = format!("http://{}/v1", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let tools = [ToolDef {
            name: "fixture_tool".into(),
            description: "Verify the saved installation".into(),
            parameters: json!({"type":"object", "properties":{}}),
        }];
        for (initial, model) in [
            (LlmProvider::OpenAI, "fixture-model"),
            (LlmProvider::Anthropic, "fixture-model"),
            (LlmProvider::OpenAI, "deepseek-v4-pro"),
            (LlmProvider::Anthropic, "deepseek-v4-pro"),
        ] {
            let mut config = protocol_config(initial);
            config.base_url.clone_from(&base_url);
            config.model = model.into();
            let mut source = assistant_with_tool_uses(&["a", "b"]);
            if let MessageContent::Blocks(blocks) = &mut source.content {
                for block in blocks.iter_mut() {
                    if let ContentBlock::ToolUse { input, .. } = block {
                        *input = json!({"path":"saved.txt", "server_id":"server-1"});
                    }
                }
                blocks.insert(
                    0,
                    ContentBlock::Text {
                        text: "Checking installation".into(),
                    },
                );
                blocks.insert(
                    0,
                    ContentBlock::Thinking {
                        thinking: "initial reasoning".into(),
                        signature: if initial == LlmProvider::Anthropic {
                            "initial signature".into()
                        } else {
                            String::new()
                        },
                        origin: Some(ThinkingOrigin::from_config(&config)),
                    },
                );
            }
            let mut messages = vec![
                Message {
                    role: "user".into(),
                    content: MessageContent::Text("Install on the saved server".into()),
                },
                source,
            ];
            if initial == LlmProvider::OpenAI {
                for id in ["a", "b"] {
                    let mut result = tool_result_msg(&[id]);
                    result.role = "tool".into();
                    messages.push(result);
                }
            } else {
                messages.push(tool_result_msg(&["a", "b"]));
            }
            messages.push(Message {
                role: "assistant".into(),
                content: MessageContent::Text("Installation verified".into()),
            });
            messages.push(Message {
                role: "user".into(),
                content: MessageContent::Text("Check the verified installation".into()),
            });
            messages.push(Message {
                role: "assistant".into(),
                content: MessageContent::Blocks(vec![
                    ContentBlock::Thinking {
                        thinking: "final reasoning".into(),
                        signature: if initial == LlmProvider::Anthropic {
                            "final signature".into()
                        } else {
                            String::new()
                        },
                        origin: Some(ThinkingOrigin::from_config(&config)),
                    },
                    ContentBlock::Text {
                        text: "Verification complete".into(),
                    },
                ]),
            });
            let opposite = if initial == LlmProvider::OpenAI {
                LlmProvider::Anthropic
            } else {
                LlmProvider::OpenAI
            };
            for provider in [opposite, initial, opposite, initial] {
                // Exercise the same persisted canonical format on every continuation.
                messages =
                    serde_json::from_value(serde_json::to_value(&messages).unwrap()).unwrap();
                messages.push(Message {
                    role: "user".into(),
                    content: MessageContent::Text("Continue with previous results".into()),
                });
                let before = serde_json::to_value(&messages).unwrap();
                config.provider = provider;
                let client = LlmClient::new(config.clone()).unwrap();
                let mut stream = client
                    .chat_stream(&messages, &tools, "fixture system")
                    .await
                    .unwrap();
                let mut text = String::new();
                let mut thinking = String::new();
                let mut signature = String::new();
                let mut call_id = String::new();
                while let Some(event) = stream.recv().await {
                    match event {
                        LlmEvent::TextDelta(delta) => text.push_str(&delta),
                        LlmEvent::Thinking(delta) => thinking.push_str(&delta),
                        LlmEvent::ThinkingSignature(delta) => signature.push_str(&delta),
                        LlmEvent::ToolCallStart { id, .. } => call_id = id,
                        LlmEvent::Error(error) => panic!("fixture rejected history: {error}"),
                        _ => {}
                    }
                }
                assert_eq!(text, "stream answer");
                assert!(!call_id.is_empty());
                assert_eq!(serde_json::to_value(&messages).unwrap(), before);
                let body = captured.recv().await.unwrap();
                assert_eq!(body["tools"].as_array().unwrap().len(), 1);
                let serialized = body.to_string();
                for retained in [
                    "Install on the saved server",
                    "Checking installation",
                    "Installation verified",
                    "Verification complete",
                    "result for a",
                    "result for b",
                    "Continue with previous results",
                    "saved.txt",
                    "server-1",
                ] {
                    assert!(serialized.contains(retained), "missing {retained}");
                }
                assert!(!serialized.contains("endpoint"));
                assert_eq!(serialized.contains("final reasoning"), provider == initial);
                let wire = body["messages"].as_array().unwrap();
                if provider == LlmProvider::Anthropic {
                    let results = wire
                        .iter()
                        .flat_map(|message| message["content"].as_array().into_iter().flatten())
                        .filter(|block| block["type"] == "tool_result")
                        .collect::<Vec<_>>();
                    assert!(results.iter().any(|block| block["tool_use_id"] == "a"));
                    assert!(results.iter().any(|block| block["tool_use_id"] == "b"));
                    let first = wire
                        .iter()
                        .find(|message| message["role"] == "assistant")
                        .unwrap();
                    let replayed = first["content"].as_array().unwrap().iter().any(|block| {
                        block["type"] == "thinking" && block["thinking"] == "initial reasoning"
                    });
                    assert_eq!(replayed, provider == initial);
                    if model == "deepseek-v4-pro" && provider != initial {
                        assert_eq!(
                            first["content"][0],
                            json!({"type":"thinking", "thinking":"", "signature":""})
                        );
                    }
                } else {
                    assert!(wire.iter().any(|message| message["tool_call_id"] == "a"));
                    assert!(wire.iter().any(|message| message["tool_call_id"] == "b"));
                    let plain_final = wire
                        .iter()
                        .find(|message| message["content"] == "Installation verified")
                        .unwrap();
                    if model == "deepseek-v4-pro" {
                        assert_eq!(plain_final["reasoning_content"], "");
                    } else {
                        assert!(plain_final.get("reasoning_content").is_none());
                    }
                    let thinking_final = wire
                        .iter()
                        .find(|message| message["content"] == "Verification complete")
                        .unwrap();
                    if provider == initial {
                        assert_eq!(thinking_final["reasoning_content"], "final reasoning");
                    } else if model == "deepseek-v4-pro" {
                        assert_eq!(thinking_final["reasoning_content"], "");
                    } else {
                        assert!(thinking_final.get("reasoning_content").is_none());
                    }
                    let first = wire
                        .iter()
                        .find(|message| message["role"] == "assistant")
                        .unwrap();
                    assert_eq!(
                        first["reasoning_content"] == "initial reasoning",
                        provider == initial
                    );
                    if model == "deepseek-v4-pro" && provider != initial {
                        assert_eq!(first["reasoning_content"], "");
                    }
                }
                messages.push(Message {
                    role: "assistant".into(),
                    content: MessageContent::Blocks(vec![
                        ContentBlock::Thinking {
                            thinking,
                            signature,
                            origin: Some(client.thinking_origin()),
                        },
                        ContentBlock::Text { text },
                        ContentBlock::ToolUse {
                            id: call_id.clone(),
                            name: "fixture_tool".into(),
                            input: json!({}),
                        },
                    ]),
                });
                messages.push(Message {
                    role: if provider == LlmProvider::OpenAI {
                        "tool".into()
                    } else {
                        "user".into()
                    },
                    content: MessageContent::Blocks(vec![ContentBlock::ToolResult {
                        tool_use_id: call_id,
                        content: "stream result".into(),
                    }]),
                });
            }
        }
        server.abort();
    }

    #[tokio::test]
    async fn reasoning_reaches_http_on_each_tool_loop_call_and_default_stays_unchanged() {
        use axum::{extract::State, routing::post, Json, Router};
        use tokio::sync::mpsc;

        async fn capture(
            State(tx): State<mpsc::UnboundedSender<Value>>,
            Json(body): Json<Value>,
        ) -> impl axum::response::IntoResponse {
            let anthropic = body.get("system").is_some();
            tx.send(body).unwrap();
            let data = if anthropic {
                "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"fixture answer\"}}\n\nevent: message_stop\ndata: {\"type\":\"message_stop\"}\n\n"
            } else {
                "data: {\"choices\":[{\"delta\":{\"content\":\"fixture answer\"},\"finish_reason\":null}]}\n\ndata: [DONE]\n\n"
            };
            ([("content-type", "text/event-stream")], data)
        }

        let (tx, mut captured) = mpsc::unbounded_channel();
        let router = Router::new()
            .route("/v1/chat/completions", post(capture))
            .route("/v1/messages", post(capture))
            .with_state(tx);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base_url = format!("http://{}/v1", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let tools = vec![ToolDef {
            name: "fixture_tool".into(),
            description: "fixture".into(),
            parameters: json!({"type":"object", "properties":{}}),
        }];
        let mut messages = vec![Message {
            role: "user".into(),
            content: MessageContent::Text("fixture prompt".into()),
        }];

        for (provider, model) in [
            (LlmProvider::OpenAI, "gpt-5.2-pro"),
            (LlmProvider::OpenAI, "custom-model"),
            (LlmProvider::Anthropic, "claude-opus-4-5"),
            (LlmProvider::Anthropic, "custom-model"),
        ] {
            for effort in [
                None,
                Some("low"),
                Some("medium"),
                Some("high"),
                Some("xhigh"),
                Some("max"),
            ] {
                let client = LlmClient::new(LlmConfig {
                    provider,
                    model: model.into(),
                    base_url: base_url.clone(),
                    api_key: "fixture-key".into(),
                    reasoning_effort: effort.map(str::to_owned),
                })
                .unwrap();
                for _ in 0..2 {
                    let mut stream = client
                        .chat_stream(&messages, &tools, "fixture system")
                        .await
                        .unwrap();
                    let mut text = String::new();
                    while let Some(event) = stream.recv().await {
                        if let LlmEvent::TextDelta(delta) = event {
                            text.push_str(&delta);
                        }
                    }
                    assert_eq!(text, "fixture answer");
                    let body = captured.recv().await.unwrap();
                    assert_eq!(body["model"], model);
                    assert!(!body["tools"].as_array().unwrap().is_empty());
                    match (provider, effort) {
                        (LlmProvider::OpenAI, Some(effort)) => {
                            assert_eq!(body["reasoning_effort"], effort);
                            assert!(body.get("output_config").is_none());
                        }
                        (LlmProvider::Anthropic, Some(effort)) => {
                            assert_eq!(body["output_config"]["effort"], effort);
                            assert!(body.get("reasoning_effort").is_none());
                        }
                        (_, None) => {
                            assert!(body.get("reasoning_effort").is_none());
                            assert!(body.get("output_config").is_none());
                        }
                    }
                    assert!(body.get("thinking").is_none());
                    if provider == LlmProvider::Anthropic {
                        assert_eq!(body["max_tokens"], 4096);
                    }
                    messages = vec![
                        Message {
                            role: "assistant".into(),
                            content: MessageContent::Blocks(vec![ContentBlock::ToolUse {
                                id: "fixture-call".into(),
                                name: "fixture_tool".into(),
                                input: json!({}),
                            }]),
                        },
                        Message {
                            role: "user".into(),
                            content: MessageContent::Blocks(vec![ContentBlock::ToolResult {
                                tool_use_id: "fixture-call".into(),
                                content: "fixture result".into(),
                            }]),
                        },
                    ];
                }
            }
        }
        server.abort();
    }

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

    // ---- normalize_anthropic_messages ----

    fn tool_result_msg(ids: &[&str]) -> Message {
        Message {
            role: "user".into(),
            content: MessageContent::Blocks(
                ids.iter()
                    .map(|id| ContentBlock::ToolResult {
                        tool_use_id: (*id).to_string(),
                        content: format!("result for {id}"),
                    })
                    .collect(),
            ),
        }
    }

    fn assistant_with_tool_uses(ids: &[&str]) -> Message {
        Message {
            role: "assistant".into(),
            content: MessageContent::Blocks(
                ids.iter()
                    .map(|id| ContentBlock::ToolUse {
                        id: (*id).to_string(),
                        name: "shell_exec".into(),
                        input: serde_json::json!({}),
                    })
                    .collect(),
            ),
        }
    }

    #[test]
    fn protocol_switch_projects_openai_tool_roles_for_anthropic() {
        let mut first = tool_result_msg(&["a"]);
        first.role = "tool".into();
        let mut second = tool_result_msg(&["b"]);
        second.role = "tool".into();
        let input = vec![assistant_with_tool_uses(&["a", "b"]), first, second];
        let before = serde_json::to_value(&input).unwrap();
        let projected = normalize_anthropic_messages(&input);
        assert_eq!(projected.len(), 2);
        let result = message_to_anthropic_json(&projected[1]);
        assert_eq!(result["role"], "user");
        assert_eq!(result["content"].as_array().unwrap().len(), 2);
        assert_eq!(serde_json::to_value(&input).unwrap(), before);
    }

    #[test]
    fn normalize_merges_consecutive_tool_result_users_into_one() {
        // issue #42 repro: assistant emits N tool_use blocks, agent_loop
        // pushes N separate user{tool_result} messages.
        let input = vec![
            Message {
                role: "user".into(),
                content: MessageContent::Text("go".into()),
            },
            assistant_with_tool_uses(&["c1", "c2", "c3", "c4", "c5"]),
            tool_result_msg(&["c1"]),
            tool_result_msg(&["c2"]),
            tool_result_msg(&["c3"]),
            tool_result_msg(&["c4"]),
            tool_result_msg(&["c5"]),
        ];
        let out = normalize_anthropic_messages(&input);
        assert_eq!(out.len(), 3, "user, assistant, merged-user");
        assert_eq!(out[2].role, "user");
        if let MessageContent::Blocks(blocks) = &out[2].content {
            assert_eq!(blocks.len(), 5);
            for (i, b) in blocks.iter().enumerate() {
                match b {
                    ContentBlock::ToolResult { tool_use_id, .. } => {
                        assert_eq!(tool_use_id, &format!("c{}", i + 1));
                    }
                    _ => panic!("expected ToolResult"),
                }
            }
        } else {
            panic!("expected Blocks");
        }
    }

    #[test]
    fn normalize_preserves_plain_text_user_unchanged() {
        let input = vec![Message {
            role: "user".into(),
            content: MessageContent::Text("hello".into()),
        }];
        let out = normalize_anthropic_messages(&input);
        assert_eq!(out.len(), 1);
        match &out[0].content {
            MessageContent::Text(s) => assert_eq!(s, "hello"),
            _ => panic!("plain text user must not be wrapped"),
        }
    }

    #[test]
    fn normalize_does_not_merge_across_assistant_boundary() {
        // tool_result → assistant → tool_result must not collapse to one user.
        let input = vec![
            assistant_with_tool_uses(&["a"]),
            tool_result_msg(&["a"]),
            Message {
                role: "assistant".into(),
                content: MessageContent::Text("thinking...".into()),
            },
            assistant_with_tool_uses(&["b"]),
            tool_result_msg(&["b"]),
        ];
        let out = normalize_anthropic_messages(&input);
        assert_eq!(out.len(), 5);
        assert_eq!(out[1].role, "user");
        assert_eq!(out[2].role, "assistant");
        assert_eq!(out[4].role, "user");
    }

    #[test]
    fn normalize_does_not_merge_with_plain_text_user() {
        // Mixed: tool_result-user followed by plain-text user must stay separate
        // (plain-text user isn't a tool_result message — coalescing them would
        // discard the text).
        let input = vec![
            assistant_with_tool_uses(&["a", "b"]),
            tool_result_msg(&["a"]),
            tool_result_msg(&["b"]),
            Message {
                role: "user".into(),
                content: MessageContent::Text("follow-up".into()),
            },
        ];
        let out = normalize_anthropic_messages(&input);
        assert_eq!(out.len(), 3);
        match &out[1].content {
            MessageContent::Blocks(b) => assert_eq!(b.len(), 2),
            _ => panic!(),
        }
        match &out[2].content {
            MessageContent::Text(s) => assert_eq!(s, "follow-up"),
            _ => panic!(),
        }
    }

    #[test]
    fn normalize_handles_empty_input() {
        let out = normalize_anthropic_messages(&[]);
        assert!(out.is_empty());
    }

    #[test]
    fn normalize_single_tool_result_passes_through() {
        let input = vec![
            assistant_with_tool_uses(&["solo"]),
            tool_result_msg(&["solo"]),
        ];
        let out = normalize_anthropic_messages(&input);
        assert_eq!(out.len(), 2);
        match &out[1].content {
            MessageContent::Blocks(b) => assert_eq!(b.len(), 1),
            _ => panic!(),
        }
    }
}
