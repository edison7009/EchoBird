// LLM Client — supports OpenAI and Anthropic APIs with SSE streaming
// Unified interface via LlmEvent enum for the Agent Loop

use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use reqwest_eventsource::{Event, EventSource};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use futures_util::StreamExt;
use tokio::sync::mpsc;

// ── Public Types ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmConfig {
    pub provider: LlmProvider,
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub proxy_url: Option<String>,
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
    #[serde(rename = "tool_use")]
    ToolUse { id: String, name: String, input: Value },
    #[serde(rename = "tool_result")]
    ToolResult { tool_use_id: String, content: String },
}

/// Events emitted by the LLM client during streaming
#[derive(Debug, Clone)]
pub enum LlmEvent {
    TextDelta(String),
    Thinking(String),
    ToolCallStart { id: String, name: String },
    ToolCallDelta { id: String, args_chunk: String },
    ToolCallEnd { id: String },
    Done { stop_reason: String },
    Error(String),
}

// ── Client ──

pub struct LlmClient {
    config: LlmConfig,
    http: reqwest::Client,
}

impl LlmClient {
    pub fn new(config: LlmConfig) -> Result<Self, String> {
        let mut builder = reqwest::Client::builder();

        if let Some(ref proxy_url) = config.proxy_url {
            if !proxy_url.is_empty() {
                let proxy = reqwest::Proxy::all(proxy_url)
                    .map_err(|e| format!("Invalid proxy URL: {}", e))?;
                builder = builder.proxy(proxy);
            }
        }

        let http = builder
            .timeout(std::time::Duration::from_secs(120))
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
            LlmProvider::OpenAI => self.chat_stream_openai(messages, tools, system_prompt).await,
            LlmProvider::Anthropic => self.chat_stream_anthropic(messages, tools, system_prompt).await,
        }
    }

    // ── OpenAI ──

    async fn chat_stream_openai(
        &self,
        messages: &[Message],
        tools: &[ToolDef],
        system_prompt: &str,
    ) -> Result<mpsc::Receiver<LlmEvent>, String> {
        let url = format!("{}/chat/completions",
            self.config.base_url.trim_end_matches('/'));

        // Build messages array with system prompt
        let mut msgs = vec![json!({"role": "system", "content": system_prompt})];
        for m in messages {
            msgs.push(message_to_openai_json(m));
        }

        // Build tools array
        let tools_json: Vec<Value> = tools.iter().map(|t| json!({
            "type": "function",
            "function": {
                "name": t.name,
                "description": t.description,
                "parameters": t.parameters,
            }
        })).collect();

        let mut body = json!({
            "model": self.config.model,
            "messages": msgs,
            "stream": true,
        });
        if !tools_json.is_empty() {
            body["tools"] = json!(tools_json);
        }

        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert(AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {}", self.config.api_key))
                .map_err(|e| format!("Invalid API key: {}", e))?);

        let request = self.http.post(&url).headers(headers).json(&body);
        let mut es = EventSource::new(request)
            .map_err(|e| format!("Failed to create event source: {}", e))?;

        let (tx, rx) = mpsc::channel(128);

        tokio::spawn(async move {
            // Track tool calls being assembled
            let mut current_tool_calls: std::collections::HashMap<i64, (String, String, String)> =
                std::collections::HashMap::new(); // index -> (id, name, args)

            while let Some(event) = es.next().await {
                match event {
                    Ok(Event::Message(msg)) => {
                        if msg.data == "[DONE]" {
                            let _ = tx.send(LlmEvent::Done { stop_reason: "stop".into() }).await;
                            break;
                        }
                        if let Ok(chunk) = serde_json::from_str::<Value>(&msg.data) {
                            if let Some(delta) = chunk["choices"][0]["delta"].as_object() {
                                // Text content
                                if let Some(content) = delta.get("content").and_then(|c| c.as_str()) {
                                    let _ = tx.send(LlmEvent::TextDelta(content.to_string())).await;
                                }
                                // OpenAI reasoning/thinking content (e.g. DeepSeek-R1)
                                if let Some(reasoning) = delta.get("reasoning_content").and_then(|c| c.as_str()) {
                                    let _ = tx.send(LlmEvent::Thinking(reasoning.to_string())).await;
                                }
                                // Tool calls
                                if let Some(tool_calls) = delta.get("tool_calls").and_then(|t| t.as_array()) {
                                    for tc in tool_calls {
                                        let idx = tc["index"].as_i64().unwrap_or(0);
                                        if let Some(func) = tc.get("function") {
                                            let id = tc["id"].as_str().unwrap_or("").to_string();
                                            let name = func["name"].as_str().unwrap_or("").to_string();
                                            let args = func["arguments"].as_str().unwrap_or("").to_string();

                                            // Only treat as new tool call if this index hasn't been seen yet
                                            if !id.is_empty() && !current_tool_calls.contains_key(&idx) {
                                                current_tool_calls.insert(idx, (id.clone(), name.clone(), String::new()));
                                                let _ = tx.send(LlmEvent::ToolCallStart { id, name }).await;
                                            }
                                            if !args.is_empty() {
                                                // Try to find entry by index, fallback to first entry
                                                let entry = current_tool_calls.get_mut(&idx)
                                                    .or_else(|| current_tool_calls.values_mut().next());
                                                if let Some(entry) = entry {
                                                    entry.2.push_str(&args);
                                                    let _ = tx.send(LlmEvent::ToolCallDelta {
                                                        id: entry.0.clone(),
                                                        args_chunk: args,
                                                    }).await;
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
                                if reason != "stop" {
                                    let _ = tx.send(LlmEvent::Done { stop_reason: reason.to_string() }).await;
                                }
                            }
                        }
                    }
                    Ok(Event::Open) => {}
                    Err(e) => {
                        let _ = tx.send(LlmEvent::Error(format!("SSE error: {}", e))).await;
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
        let url = format!("{}/messages",
            self.config.base_url.trim_end_matches('/'));

        // Build messages (Anthropic format)
        let msgs: Vec<Value> = messages.iter().map(|m| message_to_anthropic_json(m)).collect();

        // Build tools
        let tools_json: Vec<Value> = tools.iter().map(|t| json!({
            "name": t.name,
            "description": t.description,
            "input_schema": t.parameters,
        })).collect();

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

        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert("x-api-key",
            HeaderValue::from_str(&self.config.api_key)
                .map_err(|e| format!("Invalid API key: {}", e))?);
        headers.insert("anthropic-version",
            HeaderValue::from_static("2023-06-01"));

        let request = self.http.post(&url).headers(headers).json(&body);
        let mut es = EventSource::new(request)
            .map_err(|e| format!("Failed to create event source: {}", e))?;

        let (tx, rx) = mpsc::channel(128);

        tokio::spawn(async move {
            let mut current_tool_id = String::new();
            let mut current_tool_name = String::new();

            while let Some(event) = es.next().await {
                match event {
                    Ok(Event::Message(msg)) => {
                        let event_type = &msg.event;
                        if let Ok(data) = serde_json::from_str::<Value>(&msg.data) {
                            match event_type.as_str() {
                                "content_block_start" => {
                                    if let Some(block) = data.get("content_block") {
                                        match block["type"].as_str() {
                                            Some("tool_use") => {
                                                current_tool_id = block["id"].as_str().unwrap_or("").to_string();
                                                current_tool_name = block["name"].as_str().unwrap_or("").to_string();
                                                let _ = tx.send(LlmEvent::ToolCallStart {
                                                    id: current_tool_id.clone(),
                                                    name: current_tool_name.clone(),
                                                }).await;
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
                                                    let _ = tx.send(LlmEvent::TextDelta(text.to_string())).await;
                                                }
                                            }
                                            Some("input_json_delta") => {
                                                if let Some(json_str) = delta["partial_json"].as_str() {
                                                    let _ = tx.send(LlmEvent::ToolCallDelta {
                                                        id: current_tool_id.clone(),
                                                        args_chunk: json_str.to_string(),
                                                    }).await;
                                                }
                                            }
                                            Some("thinking_delta") => {
                                                if let Some(thinking) = delta["thinking"].as_str() {
                                                    let _ = tx.send(LlmEvent::Thinking(thinking.to_string())).await;
                                                }
                                            }
                                            _ => {}
                                        }
                                    }
                                }
                                "content_block_stop" => {
                                    if !current_tool_id.is_empty() {
                                        let _ = tx.send(LlmEvent::ToolCallEnd {
                                            id: current_tool_id.clone(),
                                        }).await;
                                        current_tool_id.clear();
                                        current_tool_name.clear();
                                    }
                                }
                                "message_delta" => {
                                    if let Some(delta) = data.get("delta") {
                                        if let Some(reason) = delta["stop_reason"].as_str() {
                                            let _ = tx.send(LlmEvent::Done {
                                                stop_reason: reason.to_string(),
                                            }).await;
                                        }
                                    }
                                }
                                "message_stop" => {
                                    // Final event
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
                        let _ = tx.send(LlmEvent::Error(format!("SSE error: {}", e))).await;
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
            if m.role == "tool" {
                if let Some(ContentBlock::ToolResult { tool_use_id, content }) = blocks.first() {
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
                for block in blocks {
                    match block {
                        ContentBlock::Text { text } => text_parts.push(text.clone()),
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
                if !text_parts.is_empty() {
                    msg["content"] = json!(text_parts.join(""));
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

fn message_to_anthropic_json(m: &Message) -> Value {
    match &m.content {
        MessageContent::Text(text) => json!({"role": m.role, "content": text}),
        MessageContent::Blocks(blocks) => {
            let content: Vec<Value> = blocks.iter().map(|b| match b {
                ContentBlock::Text { text } => json!({"type": "text", "text": text}),
                ContentBlock::ToolUse { id, name, input } => json!({
                    "type": "tool_use", "id": id, "name": name, "input": input
                }),
                ContentBlock::ToolResult { tool_use_id, content } => json!({
                    "type": "tool_result", "tool_use_id": tool_use_id, "content": content
                }),
            }).collect();
            json!({"role": m.role, "content": content})
        }
    }
}

