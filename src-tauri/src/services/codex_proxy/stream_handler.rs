// Chat Completions stream → Responses API SSE translator.
//
// Direct port of tools/codex/lib/stream-handler.cjs. The Node version
// owned a writable HTTP response and called `clientRes.write(...)` for
// each SSE event; the Rust version separates that into:
//
//   1. A pure state machine (`StreamState`) that consumes parsed Chat
//      Completions deltas and emits `SseEvent` values into an internal
//      buffer. Fully synchronous, fully testable.
//   2. An async driver (`drive_chat_stream`) that reads bytes from a
//      reqwest body stream, line-splits SSE, parses JSON, feeds the
//      state machine, and yields events as an `axum::response::sse::Event`
//      stream the handler can return verbatim.
//
// Splitting these two pieces means every translation rule that mattered
// in the .cjs (finish_reason → status, tool-call slot tracking,
// reasoning_content round-trip via SessionStore) has a unit test that
// runs without spinning up a TCP listener or fake HTTP server.

use std::collections::BTreeMap;

use serde_json::{json, Value};

use super::session_store::SessionStore;

// ---------------------------------------------------------------------------
// chat_usage_to_responses_usage — Chat usage shape → Responses usage shape.
// ---------------------------------------------------------------------------
//
// Codex's Rust client parses ResponseCompleted with strict serde and
// crashes with messages like "missing field input_tokens" when fields
// are absent. Chat Completions emits prompt_tokens/completion_tokens;
// Responses API expects the full nested shape:
//
//   {
//     input_tokens: N,
//     input_tokens_details: { cached_tokens: N },
//     output_tokens: N,
//     output_tokens_details: { reasoning_tokens: N },
//     total_tokens: N
//   }
//
// All five top-level fields AND both *_details objects are mandatory.
// We synthesize zeros when upstream omits anything (many third parties
// skip usage on streaming, or only emit prompt_tokens/completion_tokens
// without details).
pub fn chat_usage_to_responses_usage(chat_usage: Option<&Value>) -> Value {
    let u = chat_usage.unwrap_or(&Value::Null);

    let pick_u64 = |obj: &Value, keys: &[&str]| -> u64 {
        for k in keys {
            if let Some(n) = obj.get(*k).and_then(|v| v.as_u64()) {
                return n;
            }
        }
        0
    };

    let input = pick_u64(u, &["input_tokens", "prompt_tokens"]);
    let output = pick_u64(u, &["output_tokens", "completion_tokens"]);
    let total = u
        .get("total_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(input + output);

    // Cached input tokens — newer providers nest under prompt_tokens_details.
    let cached_tokens = u
        .get("input_tokens_details")
        .and_then(|v| v.get("cached_tokens"))
        .and_then(|v| v.as_u64())
        .or_else(|| {
            u.get("prompt_tokens_details")
                .and_then(|v| v.get("cached_tokens"))
                .and_then(|v| v.as_u64())
        })
        .or_else(|| u.get("cached_tokens").and_then(|v| v.as_u64()))
        .unwrap_or(0);

    // Reasoning output tokens — for thinking models. Some providers nest
    // under completion_tokens_details, some emit a flat reasoning_tokens.
    let reasoning_tokens = u
        .get("output_tokens_details")
        .and_then(|v| v.get("reasoning_tokens"))
        .and_then(|v| v.as_u64())
        .or_else(|| {
            u.get("completion_tokens_details")
                .and_then(|v| v.get("reasoning_tokens"))
                .and_then(|v| v.as_u64())
        })
        .or_else(|| u.get("reasoning_tokens").and_then(|v| v.as_u64()))
        .unwrap_or(0);

    json!({
        "input_tokens": input,
        "input_tokens_details": { "cached_tokens": cached_tokens },
        "output_tokens": output,
        "output_tokens_details": { "reasoning_tokens": reasoning_tokens },
        "total_tokens": total,
    })
}

// ---------------------------------------------------------------------------
// chat_error_to_responses_error — wrap an upstream error in Responses shape.
// ---------------------------------------------------------------------------
//
// Translates an upstream /chat/completions error response (or transport
// error) into a /responses-shape error envelope that Codex can render.
// We pull out the upstream message text where possible so users see the
// underlying provider error verbatim (e.g. "Invalid API key", "Model not
// found") instead of a generic 502.
pub fn chat_error_to_responses_error(
    status_code: u16,
    upstream_body: Option<&str>,
    sessions: Option<&SessionStore>,
) -> Value {
    let response_id = match sessions {
        Some(s) => s.new_response_id(),
        None => format!(
            "resp_err_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0)
        ),
    };

    let mut message = format!("Upstream returned {status_code}");
    let mut code = format!("upstream_{status_code}");

    if let Some(body) = upstream_body {
        if !body.is_empty() {
            match serde_json::from_str::<Value>(body) {
                Ok(parsed) => {
                    // OpenAI/DeepSeek/etc. nest under .error.{message,code,type};
                    // some providers return flat .message / .detail at the top.
                    let err_obj = parsed.get("error").unwrap_or(&parsed);
                    if let Some(s) = err_obj.get("message").and_then(|v| v.as_str()) {
                        message = s.to_string();
                    } else if let Some(s) = err_obj.get("detail").and_then(|v| v.as_str()) {
                        message = s.to_string();
                    }
                    if let Some(s) = err_obj.get("code").and_then(|v| v.as_str()) {
                        code = s.to_string();
                    } else if let Some(s) = err_obj.get("type").and_then(|v| v.as_str()) {
                        code = s.to_string();
                    }
                }
                Err(_) => {
                    // Body wasn't JSON — surface the raw text (truncated)
                    // so the user still gets *something* instead of just
                    // the status code.
                    let take = body.len().min(500);
                    message = body[..take].to_string();
                }
            }
        }
    }

    json!({
        "id": response_id,
        "object": "response",
        "status": "failed",
        "error": { "code": code, "message": message },
        "output": [],
    })
}

// ---------------------------------------------------------------------------
// chat_to_responses_non_stream — single-shot translation, no SSE.
// ---------------------------------------------------------------------------
pub fn chat_to_responses_non_stream(
    chat_response: &Value,
    request_messages: Vec<Value>,
    sessions: &SessionStore,
    client_model: Option<&str>,
) -> Value {
    let response_id = sessions.new_response_id();
    let choice = chat_response
        .get("choices")
        .and_then(|v| v.get(0))
        .cloned()
        .unwrap_or(Value::Null);
    let msg = choice.get("message").cloned().unwrap_or(Value::Null);

    let mut output: Vec<Value> = Vec::new();
    if let Some(content) = msg.get("content").and_then(|v| v.as_str()) {
        if !content.is_empty() {
            output.push(json!({
                "id": format!("item_{}_0", response_id),
                "type": "message",
                "role": "assistant",
                "content": [{ "type": "output_text", "text": content }],
            }));
        }
    }
    let tool_calls = msg
        .get("tool_calls")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    for tc in &tool_calls {
        let id = tc.get("id").and_then(|v| v.as_str()).unwrap_or("");
        let name = tc
            .get("function")
            .and_then(|v| v.get("name"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let arguments = tc
            .get("function")
            .and_then(|v| v.get("arguments"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        output.push(json!({
            "id": id,
            "type": "function_call",
            "call_id": id,
            "name": name,
            "arguments": arguments,
        }));
    }

    // Persist reasoning + history (same shape as the streaming path so
    // a follow-up request with previous_response_id replays consistently).
    let mut assistant_msg = serde_json::Map::new();
    assistant_msg.insert("role".into(), Value::String("assistant".into()));
    if tool_calls.is_empty() {
        let content = msg
            .get("content")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        assistant_msg.insert("content".into(), Value::String(content));
    } else {
        assistant_msg.insert("content".into(), Value::Null);
        let normalized: Vec<Value> = tool_calls
            .iter()
            .map(|tc| {
                let id = tc.get("id").and_then(|v| v.as_str()).unwrap_or("");
                let name = tc
                    .get("function")
                    .and_then(|v| v.get("name"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let args = tc
                    .get("function")
                    .and_then(|v| v.get("arguments"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                json!({
                    "id": id,
                    "type": "function",
                    "function": { "name": name, "arguments": args },
                })
            })
            .collect();
        assistant_msg.insert("tool_calls".into(), Value::Array(normalized));
    }

    if let Some(reasoning) = msg.get("reasoning_content").and_then(|v| v.as_str()) {
        if !reasoning.is_empty() {
            assistant_msg.insert(
                "reasoning_content".into(),
                Value::String(reasoning.to_string()),
            );
            if !tool_calls.is_empty() {
                for tc in &tool_calls {
                    if let Some(id) = tc.get("id").and_then(|v| v.as_str()) {
                        sessions.store_reasoning(id, reasoning);
                    }
                }
            }
            if let Some(content) = msg.get("content").and_then(|v| v.as_str()) {
                if !content.is_empty() {
                    sessions.store_turn_reasoning(&Value::String(content.to_string()), reasoning);
                }
            }
        }
    }

    let mut history = request_messages;
    history.push(Value::Object(assistant_msg));
    sessions.save_history(&response_id, history);

    // "length" finish_reason → mark as incomplete so Codex can show the
    // response was truncated rather than treating it as a clean stop.
    let finish_reason = choice
        .get("finish_reason")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let status = if finish_reason == "length" {
        "incomplete"
    } else {
        "completed"
    };

    let mut response = json!({
        "id": response_id,
        "object": "response",
        "status": status,
        "output": output,
    });
    if let Some(m) = client_model {
        response["model"] = Value::String(m.to_string());
    }
    response["usage"] = chat_usage_to_responses_usage(chat_response.get("usage"));
    if finish_reason == "length" {
        response["incomplete_details"] = json!({ "reason": "max_output_tokens" });
    }
    response
}

// ---------------------------------------------------------------------------
// Streaming state machine
// ---------------------------------------------------------------------------

/// One Server-Sent Event. `event` is the `event:` line, `data` becomes
/// the JSON-serialized `data:` line. The axum handler converts these
/// to `axum::response::sse::Event` 1:1.
#[derive(Debug, Clone, PartialEq)]
pub struct SseEvent {
    pub event: String,
    pub data: Value,
}

impl SseEvent {
    pub fn new(event: impl Into<String>, data: Value) -> Self {
        Self {
            event: event.into(),
            data,
        }
    }

    /// Serialize to wire format (`event: ...\ndata: {...}\n\n`). Only
    /// used by tests that want to assert exact bytes — the production
    /// path goes through axum's `Sse` adapter, not this helper.
    #[cfg(test)]
    pub fn to_wire(&self) -> String {
        format!("event: {}\ndata: {}\n\n", self.event, self.data)
    }
}

#[derive(Debug, Clone)]
struct ToolCallSlot {
    id: String,
    name: String,
    arguments: String,
    output_index: i64,
}

/// Streaming translator state. Owns the partial buffer, the current
/// text/tool slots, accumulated usage/finish_reason, and a queue of
/// pending SSE events. Drain via `take_events()` after each `feed_chunk()`.
pub struct StreamState {
    response_id: String,
    client_model: Option<String>,
    request_messages: Vec<Value>,
    text_open: bool,
    text_idx: i64,
    text_buf: String,
    reasoning_buf: String,
    tool_calls: BTreeMap<i64, ToolCallSlot>,
    /// Order of tool-call insertion (chat delta index). Iterating
    /// `tool_calls.values()` in insertion order matters because Codex
    /// keys on it for replay.
    tool_call_order: Vec<i64>,
    next_output_index: i64,
    buffer: String,
    finished: bool,
    usage: Option<Value>,
    finish_reason: Option<String>,
    events: Vec<SseEvent>,
}

impl StreamState {
    pub fn new(
        sessions: &SessionStore,
        client_model: Option<String>,
        request_messages: Vec<Value>,
    ) -> Self {
        Self {
            response_id: sessions.new_response_id(),
            client_model,
            request_messages,
            text_open: false,
            text_idx: -1,
            text_buf: String::new(),
            reasoning_buf: String::new(),
            tool_calls: BTreeMap::new(),
            tool_call_order: Vec::new(),
            next_output_index: 0,
            buffer: String::new(),
            finished: false,
            usage: None,
            finish_reason: None,
            events: Vec::new(),
        }
    }

    #[allow(dead_code)]
    pub fn response_id(&self) -> &str {
        &self.response_id
    }

    /// Emit the opening `response.created` + `response.in_progress`
    /// pair. Codex's Rust client demands both before any output_item.
    pub fn start(&mut self) {
        self.emit(
            "response.created",
            json!({
                "type": "response.created",
                "response": self.stamp_model(json!({
                    "id": self.response_id,
                    "object": "response",
                    "status": "in_progress",
                    "output": [],
                })),
            }),
        );
        self.emit(
            "response.in_progress",
            json!({
                "type": "response.in_progress",
                "response": self.stamp_model(json!({
                    "id": self.response_id,
                    "object": "response",
                    "status": "in_progress",
                })),
            }),
        );
    }

    /// Append a chunk of bytes from the upstream stream. Splits on
    /// newlines, ignores non-`data:` lines, parses each JSON payload,
    /// and feeds the state machine. Returns immediately after `[DONE]`.
    pub fn feed_chunk(&mut self, chunk: &str) {
        if self.finished {
            return;
        }
        self.buffer.push_str(chunk);
        // Split on '\n', keeping the trailing partial line buffered for
        // the next call. We copy lines out as owned Strings so we don't
        // hold an immutable borrow on self.buffer while invoking
        // handle_line (which mutates self).
        let mut lines: Vec<String> = Vec::new();
        let mut remainder = String::new();
        {
            let mut iter = self.buffer.split('\n').peekable();
            while let Some(line) = iter.next() {
                if iter.peek().is_none() {
                    // Last segment may be a partial line — keep it in the
                    // buffer for the next chunk.
                    remainder = line.to_string();
                } else {
                    lines.push(line.to_string());
                }
            }
        }
        self.buffer = remainder;
        for line in lines {
            self.handle_line(&line);
            if self.finished {
                break;
            }
        }
    }

    fn handle_line(&mut self, line: &str) {
        let line = line.trim_end_matches('\r');
        if !line.starts_with("data: ") {
            return;
        }
        let data = line[6..].trim();
        if data.is_empty() {
            return;
        }
        if data == "[DONE]" {
            self.finish_internal();
            return;
        }
        let parsed: Value = match serde_json::from_str(data) {
            Ok(v) => v,
            Err(_) => return,
        };

        // Capture usage / finish_reason on any chunk that includes them.
        // OpenAI emits usage as a trailing event when include_usage=true;
        // other providers attach it to the final delta chunk.
        if let Some(u) = parsed.get("usage") {
            if !u.is_null() {
                self.usage = Some(u.clone());
            }
        }
        if let Some(fr) = parsed
            .get("choices")
            .and_then(|v| v.get(0))
            .and_then(|c| c.get("finish_reason"))
            .and_then(|v| v.as_str())
        {
            self.finish_reason = Some(fr.to_string());
        }

        let delta = match parsed
            .get("choices")
            .and_then(|v| v.get(0))
            .and_then(|c| c.get("delta"))
        {
            Some(d) => d.clone(),
            None => return,
        };

        // reasoning_content delta — DeepSeek-V4 / Kimi-K2.6 / etc. emit
        // these alongside the regular content stream. Accumulate but
        // don't forward (Codex's reasoning summary events aren't
        // synthesized yet; round-trip via SessionStore is what matters).
        if let Some(r) = delta.get("reasoning_content").and_then(|v| v.as_str()) {
            self.reasoning_buf.push_str(r);
        }

        // Text delta
        if let Some(content) = delta.get("content").and_then(|v| v.as_str()) {
            if !content.is_empty() {
                if !self.text_open {
                    self.open_text_item();
                }
                self.text_buf.push_str(content);
                let idx = self.text_idx;
                self.emit(
                    "response.output_text.delta",
                    json!({
                        "type": "response.output_text.delta",
                        "output_index": idx,
                        "content_index": 0,
                        "delta": content,
                    }),
                );
            }
        }

        // Tool-call deltas. Chat splits arguments into multiple delta
        // chunks; we forward each one as a Responses arguments delta.
        if let Some(tcs) = delta.get("tool_calls").and_then(|v| v.as_array()) {
            if self.text_open {
                self.close_text_item();
            }
            for tc in tcs {
                let idx = tc.get("index").and_then(|v| v.as_i64()).unwrap_or(0);
                if !self.tool_calls.contains_key(&idx) {
                    self.open_tool_call(idx, tc);
                }
                // Pull a mutable slot reference. We can't hold it across
                // emit() calls, so do the updates first then emit.
                let mut delta_args: Option<String> = None;
                {
                    let slot = self.tool_calls.get_mut(&idx).expect("just inserted");
                    if let Some(new_id) = tc.get("id").and_then(|v| v.as_str()) {
                        if slot.id.starts_with("call_") && new_id != slot.id {
                            slot.id = new_id.to_string();
                        }
                    }
                    if let Some(name) = tc
                        .get("function")
                        .and_then(|v| v.get("name"))
                        .and_then(|v| v.as_str())
                    {
                        if slot.name.is_empty() {
                            slot.name = name.to_string();
                        }
                    }
                    if let Some(args) = tc
                        .get("function")
                        .and_then(|v| v.get("arguments"))
                        .and_then(|v| v.as_str())
                    {
                        if !args.is_empty() {
                            slot.arguments.push_str(args);
                            delta_args = Some(args.to_string());
                        }
                    }
                }
                if let Some(da) = delta_args {
                    let slot = self.tool_calls.get(&idx).expect("present");
                    let out_idx = slot.output_index;
                    let item_id = slot.id.clone();
                    self.emit(
                        "response.function_call_arguments.delta",
                        json!({
                            "type": "response.function_call_arguments.delta",
                            "output_index": out_idx,
                            "item_id": item_id,
                            "delta": da,
                        }),
                    );
                }
            }
        }
    }

    /// Mark end-of-stream and emit `response.completed` /
    /// `response.incomplete`. Idempotent. Persists assistant history +
    /// reasoning under the response id so a follow-up request with
    /// `previous_response_id` replays cleanly.
    pub fn finish(&mut self, sessions: &SessionStore) {
        if self.finished {
            return;
        }
        self.finish_internal();
        self.persist_history(sessions);
    }

    fn finish_internal(&mut self) {
        if self.finished {
            return;
        }
        self.finished = true;
        if self.text_open {
            self.close_text_item();
        }
        self.close_tool_calls();

        let assembled = self.build_assembled_output();
        let mut completed = self.stamp_model(json!({
            "id": self.response_id,
            "object": "response",
            "status": "completed",
            "output": assembled,
        }));
        completed["usage"] = chat_usage_to_responses_usage(self.usage.as_ref());

        // "length" finish_reason → max_tokens hit → surface as incomplete
        // so Codex shows "(response truncated)" instead of clean stop.
        let is_length = self.finish_reason.as_deref() == Some("length");
        if is_length {
            completed["status"] = Value::String("incomplete".into());
            completed["incomplete_details"] = json!({ "reason": "max_output_tokens" });
        }

        let event_name = if is_length {
            "response.incomplete"
        } else {
            "response.completed"
        };
        self.emit(
            event_name,
            json!({
                "type": event_name,
                "response": completed,
            }),
        );
    }

    /// Emit a `response.failed` event and end the stream. Used when the
    /// upstream connection drops mid-stream — without this, Codex would
    /// receive response.completed with whatever partial output we got
    /// and think the request succeeded.
    pub fn fail(&mut self, message: &str, code: &str) {
        if self.finished {
            return;
        }
        self.finished = true;
        if self.text_open {
            self.close_text_item();
        }
        self.close_tool_calls();
        self.emit(
            "response.failed",
            json!({
                "type": "response.failed",
                "response": {
                    "id": self.response_id,
                    "object": "response",
                    "status": "failed",
                    "error": { "code": code, "message": message },
                    "output": self.build_assembled_output(),
                },
            }),
        );
    }

    /// Drain pending SSE events. Caller forwards these to the wire (or
    /// asserts on them in tests).
    pub fn take_events(&mut self) -> Vec<SseEvent> {
        std::mem::take(&mut self.events)
    }

    fn emit(&mut self, event: &str, data: Value) {
        self.events.push(SseEvent::new(event, data));
    }

    fn stamp_model(&self, mut resp: Value) -> Value {
        if let Some(m) = &self.client_model {
            resp["model"] = Value::String(m.clone());
        }
        resp
    }

    fn open_text_item(&mut self) {
        self.text_idx = self.next_output_index;
        self.next_output_index += 1;
        let idx = self.text_idx;
        let item_id = format!("item_{}_{}", self.response_id, idx);
        self.emit(
            "response.output_item.added",
            json!({
                "type": "response.output_item.added",
                "output_index": idx,
                "item": {
                    "id": item_id,
                    "type": "message",
                    "role": "assistant",
                    "content": [],
                },
            }),
        );
        self.emit(
            "response.content_part.added",
            json!({
                "type": "response.content_part.added",
                "output_index": idx,
                "content_index": 0,
                "part": { "type": "output_text", "text": "" },
            }),
        );
        self.text_open = true;
        self.text_buf.clear();
    }

    fn close_text_item(&mut self) {
        if !self.text_open {
            return;
        }
        let idx = self.text_idx;
        let buf = self.text_buf.clone();
        let item_id = format!("item_{}_{}", self.response_id, idx);
        self.emit(
            "response.output_text.done",
            json!({
                "type": "response.output_text.done",
                "output_index": idx,
                "content_index": 0,
                "text": buf,
            }),
        );
        self.emit(
            "response.content_part.done",
            json!({
                "type": "response.content_part.done",
                "output_index": idx,
                "content_index": 0,
                "part": { "type": "output_text", "text": buf },
            }),
        );
        self.emit(
            "response.output_item.done",
            json!({
                "type": "response.output_item.done",
                "output_index": idx,
                "item": {
                    "id": item_id,
                    "type": "message",
                    "role": "assistant",
                    "content": [{ "type": "output_text", "text": buf }],
                },
            }),
        );
        self.text_open = false;
    }

    fn open_tool_call(&mut self, idx: i64, tc: &Value) {
        let output_index = self.next_output_index;
        self.next_output_index += 1;
        let call_id = tc
            .get("id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| {
                use rand::Rng;
                let mut rng = rand::thread_rng();
                let suffix: String = (0..10)
                    .map(|_| {
                        let n: u8 = rng.gen_range(0..36);
                        if n < 10 {
                            (b'0' + n) as char
                        } else {
                            (b'a' + (n - 10)) as char
                        }
                    })
                    .collect();
                format!("call_{suffix}")
            });
        let name = tc
            .get("function")
            .and_then(|v| v.get("name"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let slot = ToolCallSlot {
            id: call_id.clone(),
            name: name.clone(),
            arguments: String::new(),
            output_index,
        };
        self.tool_calls.insert(idx, slot);
        self.tool_call_order.push(idx);
        self.emit(
            "response.output_item.added",
            json!({
                "type": "response.output_item.added",
                "output_index": output_index,
                "item": {
                    "id": call_id,
                    "type": "function_call",
                    "call_id": call_id,
                    "name": name,
                    "arguments": "",
                },
            }),
        );
    }

    fn close_tool_calls(&mut self) {
        // Snapshot first to avoid borrowing self while emitting.
        let snapshots: Vec<ToolCallSlot> = self
            .tool_call_order
            .iter()
            .filter_map(|i| self.tool_calls.get(i).cloned())
            .collect();
        for slot in snapshots {
            self.emit(
                "response.function_call_arguments.done",
                json!({
                    "type": "response.function_call_arguments.done",
                    "output_index": slot.output_index,
                    "item_id": slot.id,
                    "arguments": slot.arguments,
                }),
            );
            self.emit(
                "response.output_item.done",
                json!({
                    "type": "response.output_item.done",
                    "output_index": slot.output_index,
                    "item": {
                        "id": slot.id,
                        "type": "function_call",
                        "call_id": slot.id,
                        "name": slot.name,
                        "arguments": slot.arguments,
                    },
                }),
            );
        }
    }

    fn build_assembled_output(&self) -> Vec<Value> {
        let mut out: Vec<Value> = Vec::new();
        if !self.text_buf.is_empty() {
            let idx = if self.text_idx >= 0 { self.text_idx } else { 0 };
            out.push(json!({
                "id": format!("item_{}_{}", self.response_id, idx),
                "type": "message",
                "role": "assistant",
                "content": [{ "type": "output_text", "text": self.text_buf }],
            }));
        }
        for i in &self.tool_call_order {
            if let Some(slot) = self.tool_calls.get(i) {
                out.push(json!({
                    "id": slot.id,
                    "type": "function_call",
                    "call_id": slot.id,
                    "name": slot.name,
                    "arguments": slot.arguments,
                }));
            }
        }
        out
    }

    fn persist_history(&self, sessions: &SessionStore) {
        let mut assistant_msg = serde_json::Map::new();
        assistant_msg.insert("role".into(), Value::String("assistant".into()));
        if self.tool_calls.is_empty() {
            assistant_msg.insert("content".into(), Value::String(self.text_buf.clone()));
        } else {
            assistant_msg.insert("content".into(), Value::Null);
            let normalized: Vec<Value> = self
                .tool_call_order
                .iter()
                .filter_map(|i| self.tool_calls.get(i))
                .map(|slot| {
                    json!({
                        "id": slot.id,
                        "type": "function",
                        "function": { "name": slot.name, "arguments": slot.arguments },
                    })
                })
                .collect();
            assistant_msg.insert("tool_calls".into(), Value::Array(normalized));
        }
        if !self.reasoning_buf.is_empty() {
            assistant_msg.insert(
                "reasoning_content".into(),
                Value::String(self.reasoning_buf.clone()),
            );
            // Store reasoning under every tool_call id so any of them
            // resolves on next-turn lookup.
            for i in &self.tool_call_order {
                if let Some(slot) = self.tool_calls.get(i) {
                    sessions.store_reasoning(&slot.id, &self.reasoning_buf);
                }
            }
            // And under a content-fingerprint key, so plain assistant
            // turns (no tool_calls) also round-trip.
            if !self.text_buf.is_empty() {
                sessions.store_turn_reasoning(
                    &Value::String(self.text_buf.clone()),
                    &self.reasoning_buf,
                );
            }
        }
        let mut history = self.request_messages.clone();
        history.push(Value::Object(assistant_msg));
        sessions.save_history(&self.response_id, history);
    }
}

// ---------------------------------------------------------------------------
// Tests — pure state-machine assertions, no I/O.
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    fn collect_events(state: &mut StreamState) -> Vec<SseEvent> {
        state.take_events()
    }

    fn event_names(events: &[SseEvent]) -> Vec<&str> {
        events.iter().map(|e| e.event.as_str()).collect()
    }

    // ---- chat_usage_to_responses_usage ----

    #[test]
    fn usage_translates_prompt_completion_to_input_output() {
        let chat = json!({
            "prompt_tokens": 10,
            "completion_tokens": 20,
            "total_tokens": 30,
        });
        let out = chat_usage_to_responses_usage(Some(&chat));
        assert_eq!(out["input_tokens"], 10);
        assert_eq!(out["output_tokens"], 20);
        assert_eq!(out["total_tokens"], 30);
        // Mandatory nested objects present even when upstream omits them.
        assert_eq!(out["input_tokens_details"]["cached_tokens"], 0);
        assert_eq!(out["output_tokens_details"]["reasoning_tokens"], 0);
    }

    #[test]
    fn usage_passes_through_responses_shape_unchanged() {
        let chat = json!({
            "input_tokens": 5,
            "input_tokens_details": { "cached_tokens": 2 },
            "output_tokens": 7,
            "output_tokens_details": { "reasoning_tokens": 3 },
            "total_tokens": 12,
        });
        let out = chat_usage_to_responses_usage(Some(&chat));
        assert_eq!(out["input_tokens"], 5);
        assert_eq!(out["input_tokens_details"]["cached_tokens"], 2);
        assert_eq!(out["output_tokens"], 7);
        assert_eq!(out["output_tokens_details"]["reasoning_tokens"], 3);
        assert_eq!(out["total_tokens"], 12);
    }

    #[test]
    fn usage_synthesizes_total_when_missing() {
        let chat = json!({ "prompt_tokens": 4, "completion_tokens": 6 });
        let out = chat_usage_to_responses_usage(Some(&chat));
        assert_eq!(out["total_tokens"], 10);
    }

    #[test]
    fn usage_extracts_cached_from_prompt_tokens_details() {
        let chat = json!({
            "prompt_tokens": 100,
            "completion_tokens": 50,
            "prompt_tokens_details": { "cached_tokens": 40 },
        });
        let out = chat_usage_to_responses_usage(Some(&chat));
        assert_eq!(out["input_tokens_details"]["cached_tokens"], 40);
    }

    #[test]
    fn usage_extracts_reasoning_from_completion_tokens_details() {
        let chat = json!({
            "prompt_tokens": 1,
            "completion_tokens": 1,
            "completion_tokens_details": { "reasoning_tokens": 8 },
        });
        let out = chat_usage_to_responses_usage(Some(&chat));
        assert_eq!(out["output_tokens_details"]["reasoning_tokens"], 8);
    }

    #[test]
    fn usage_falls_back_to_flat_cached_and_reasoning() {
        let chat = json!({
            "prompt_tokens": 1,
            "completion_tokens": 1,
            "cached_tokens": 9,
            "reasoning_tokens": 11,
        });
        let out = chat_usage_to_responses_usage(Some(&chat));
        assert_eq!(out["input_tokens_details"]["cached_tokens"], 9);
        assert_eq!(out["output_tokens_details"]["reasoning_tokens"], 11);
    }

    #[test]
    fn usage_zeros_out_when_null() {
        let out = chat_usage_to_responses_usage(None);
        assert_eq!(out["input_tokens"], 0);
        assert_eq!(out["output_tokens"], 0);
        assert_eq!(out["total_tokens"], 0);
        assert_eq!(out["input_tokens_details"]["cached_tokens"], 0);
        assert_eq!(out["output_tokens_details"]["reasoning_tokens"], 0);
    }

    // ---- chat_error_to_responses_error ----

    #[test]
    fn error_extracts_nested_message_from_openai_shape() {
        let body = r#"{"error":{"message":"Invalid API key","code":"invalid_api_key"}}"#;
        let out = chat_error_to_responses_error(401, Some(body), None);
        assert_eq!(out["status"], "failed");
        assert_eq!(out["error"]["message"], "Invalid API key");
        assert_eq!(out["error"]["code"], "invalid_api_key");
        assert_eq!(out["output"], json!([]));
    }

    #[test]
    fn error_extracts_flat_detail_message() {
        let body = r#"{"detail":"Rate limit exceeded","type":"rate_limit"}"#;
        let out = chat_error_to_responses_error(429, Some(body), None);
        assert_eq!(out["error"]["message"], "Rate limit exceeded");
        assert_eq!(out["error"]["code"], "rate_limit");
    }

    #[test]
    fn error_truncates_non_json_body_to_500() {
        let body = "x".repeat(800);
        let out = chat_error_to_responses_error(502, Some(&body), None);
        let msg = out["error"]["message"].as_str().unwrap();
        assert_eq!(msg.len(), 500);
        assert_eq!(out["error"]["code"], "upstream_502");
    }

    #[test]
    fn error_default_message_when_no_body() {
        let out = chat_error_to_responses_error(503, None, None);
        assert_eq!(out["error"]["message"], "Upstream returned 503");
        assert_eq!(out["error"]["code"], "upstream_503");
    }

    // ---- chat_to_responses_non_stream ----

    #[test]
    fn non_stream_translates_text_message() {
        let store = SessionStore::new();
        let chat = json!({
            "choices": [{
                "message": { "role": "assistant", "content": "hello world" },
                "finish_reason": "stop",
            }],
            "usage": { "prompt_tokens": 3, "completion_tokens": 2, "total_tokens": 5 },
        });
        let out = chat_to_responses_non_stream(&chat, vec![], &store, Some("gpt-5.4"));
        assert_eq!(out["status"], "completed");
        assert_eq!(out["model"], "gpt-5.4");
        assert_eq!(out["output"][0]["type"], "message");
        assert_eq!(out["output"][0]["content"][0]["text"], "hello world");
        assert_eq!(out["usage"]["input_tokens"], 3);
        assert_eq!(out["usage"]["output_tokens"], 2);
    }

    #[test]
    fn non_stream_translates_tool_calls() {
        let store = SessionStore::new();
        let chat = json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_abc",
                        "type": "function",
                        "function": { "name": "shell", "arguments": "{\"cmd\":\"ls\"}" },
                    }],
                },
                "finish_reason": "tool_calls",
            }],
        });
        let out = chat_to_responses_non_stream(&chat, vec![], &store, None);
        assert_eq!(out["output"][0]["type"], "function_call");
        assert_eq!(out["output"][0]["call_id"], "call_abc");
        assert_eq!(out["output"][0]["name"], "shell");
        assert_eq!(out["output"][0]["arguments"], "{\"cmd\":\"ls\"}");
    }

    #[test]
    fn non_stream_marks_length_as_incomplete() {
        let store = SessionStore::new();
        let chat = json!({
            "choices": [{
                "message": { "role": "assistant", "content": "partial" },
                "finish_reason": "length",
            }],
        });
        let out = chat_to_responses_non_stream(&chat, vec![], &store, None);
        assert_eq!(out["status"], "incomplete");
        assert_eq!(out["incomplete_details"]["reason"], "max_output_tokens");
    }

    #[test]
    fn non_stream_omits_model_when_no_client_model() {
        let store = SessionStore::new();
        let chat = json!({
            "choices": [{ "message": { "role": "assistant", "content": "x" } }],
        });
        let out = chat_to_responses_non_stream(&chat, vec![], &store, None);
        assert!(out.get("model").is_none());
    }

    // ---- StreamState ----

    fn make_state(model: Option<&str>) -> StreamState {
        let store = SessionStore::new();
        StreamState::new(&store, model.map(|s| s.to_string()), vec![])
    }

    #[test]
    fn stream_start_emits_created_then_in_progress() {
        let mut s = make_state(Some("gpt-5.4"));
        s.start();
        let events = collect_events(&mut s);
        assert_eq!(
            event_names(&events),
            vec!["response.created", "response.in_progress"]
        );
        assert_eq!(events[0].data["response"]["model"], "gpt-5.4");
        assert_eq!(events[0].data["response"]["status"], "in_progress");
    }

    #[test]
    fn stream_text_delta_opens_item_and_emits_delta() {
        let mut s = make_state(None);
        s.start();
        collect_events(&mut s);
        s.feed_chunk("data: {\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}\n");
        let events = collect_events(&mut s);
        assert_eq!(
            event_names(&events),
            vec![
                "response.output_item.added",
                "response.content_part.added",
                "response.output_text.delta",
            ]
        );
        assert_eq!(events[2].data["delta"], "hi");
    }

    #[test]
    fn stream_done_closes_text_and_completes() {
        let mut s = make_state(Some("gpt-5.4"));
        s.start();
        collect_events(&mut s);
        s.feed_chunk("data: {\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}\n");
        s.feed_chunk("data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n");
        s.feed_chunk("data: [DONE]\n");
        let events = collect_events(&mut s);
        let names = event_names(&events);
        assert!(names.contains(&"response.output_text.done"));
        assert!(names.contains(&"response.content_part.done"));
        assert!(names.contains(&"response.output_item.done"));
        assert!(names.contains(&"response.completed"));
        // Find the completed event
        let completed = events
            .iter()
            .find(|e| e.event == "response.completed")
            .unwrap();
        assert_eq!(completed.data["response"]["status"], "completed");
        assert_eq!(completed.data["response"]["model"], "gpt-5.4");
        assert_eq!(completed.data["response"]["output"][0]["type"], "message");
    }

    #[test]
    fn stream_tool_call_delta_emits_added_and_arguments_delta() {
        let mut s = make_state(None);
        s.start();
        collect_events(&mut s);
        s.feed_chunk(
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_x\",\"function\":{\"name\":\"shell\",\"arguments\":\"{\\\"cmd\\\":\\\"ls\"}}]}}]}\n",
        );
        let events = collect_events(&mut s);
        let names = event_names(&events);
        assert!(names.contains(&"response.output_item.added"));
        assert!(names.contains(&"response.function_call_arguments.delta"));
        // Find the args delta
        let args_delta = events
            .iter()
            .find(|e| e.event == "response.function_call_arguments.delta")
            .unwrap();
        assert_eq!(args_delta.data["item_id"], "call_x");
        assert_eq!(args_delta.data["delta"], "{\"cmd\":\"ls");
    }

    #[test]
    fn stream_finish_emits_arguments_done_and_item_done_for_each_tool() {
        let mut s = make_state(None);
        s.start();
        collect_events(&mut s);
        s.feed_chunk("data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_y\",\"function\":{\"name\":\"shell\",\"arguments\":\"{}\"}}]}}]}\n");
        s.feed_chunk("data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n");
        s.feed_chunk("data: [DONE]\n");
        let events = collect_events(&mut s);
        let names = event_names(&events);
        assert!(names.contains(&"response.function_call_arguments.done"));
        assert!(names.contains(&"response.output_item.done"));
        assert!(names.contains(&"response.completed"));
    }

    #[test]
    fn stream_length_finish_reason_emits_incomplete() {
        let mut s = make_state(None);
        s.start();
        collect_events(&mut s);
        s.feed_chunk("data: {\"choices\":[{\"delta\":{\"content\":\"truncated\"},\"finish_reason\":\"length\"}]}\n");
        s.feed_chunk("data: [DONE]\n");
        let events = collect_events(&mut s);
        let names = event_names(&events);
        assert!(names.contains(&"response.incomplete"));
        assert!(!names.contains(&"response.completed"));
        let incomplete = events
            .iter()
            .find(|e| e.event == "response.incomplete")
            .unwrap();
        assert_eq!(incomplete.data["response"]["status"], "incomplete");
        assert_eq!(
            incomplete.data["response"]["incomplete_details"]["reason"],
            "max_output_tokens"
        );
    }

    #[test]
    fn stream_partial_line_buffered_across_chunks() {
        let mut s = make_state(None);
        s.start();
        collect_events(&mut s);
        // Split a single SSE line across two feeds.
        s.feed_chunk("data: {\"choices\":[{\"delta\":{\"con");
        let mid = collect_events(&mut s);
        assert!(mid.is_empty(), "no events should fire on partial line");
        s.feed_chunk("tent\":\"abc\"}}]}\n");
        let events = collect_events(&mut s);
        let names = event_names(&events);
        assert!(names.contains(&"response.output_text.delta"));
    }

    #[test]
    fn stream_ignores_non_data_lines_and_blank_data() {
        let mut s = make_state(None);
        s.start();
        collect_events(&mut s);
        s.feed_chunk(": comment line\n");
        s.feed_chunk("event: ping\n");
        s.feed_chunk("data: \n");
        s.feed_chunk("\n");
        let events = collect_events(&mut s);
        assert!(events.is_empty());
    }

    #[test]
    fn stream_ignores_invalid_json_data() {
        let mut s = make_state(None);
        s.start();
        collect_events(&mut s);
        s.feed_chunk("data: not-json-at-all\n");
        let events = collect_events(&mut s);
        assert!(events.is_empty());
    }

    #[test]
    fn stream_usage_attached_to_completed_event() {
        let mut s = make_state(None);
        s.start();
        collect_events(&mut s);
        s.feed_chunk("data: {\"choices\":[{\"delta\":{\"content\":\"x\"}}]}\n");
        s.feed_chunk(
            "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":7,\"completion_tokens\":1}}\n",
        );
        s.feed_chunk("data: [DONE]\n");
        let events = collect_events(&mut s);
        let completed = events
            .iter()
            .find(|e| e.event == "response.completed")
            .unwrap();
        assert_eq!(completed.data["response"]["usage"]["input_tokens"], 7);
        assert_eq!(completed.data["response"]["usage"]["output_tokens"], 1);
        // Mandatory nested objects always present.
        assert!(completed.data["response"]["usage"]["input_tokens_details"].is_object());
        assert!(completed.data["response"]["usage"]["output_tokens_details"].is_object());
    }

    #[test]
    fn stream_fail_emits_failed_event() {
        let mut s = make_state(None);
        s.start();
        collect_events(&mut s);
        s.feed_chunk("data: {\"choices\":[{\"delta\":{\"content\":\"partial\"}}]}\n");
        collect_events(&mut s);
        s.fail(
            "Upstream stream error: connection reset",
            "upstream_stream_error",
        );
        let events = collect_events(&mut s);
        let failed = events
            .iter()
            .find(|e| e.event == "response.failed")
            .unwrap();
        assert_eq!(failed.data["response"]["status"], "failed");
        assert_eq!(
            failed.data["response"]["error"]["code"],
            "upstream_stream_error"
        );
        assert_eq!(
            failed.data["response"]["error"]["message"],
            "Upstream stream error: connection reset"
        );
        // Partial output preserved.
        assert_eq!(failed.data["response"]["output"][0]["type"], "message");
    }

    #[test]
    fn stream_text_then_tool_call_closes_text_first() {
        let mut s = make_state(None);
        s.start();
        collect_events(&mut s);
        s.feed_chunk("data: {\"choices\":[{\"delta\":{\"content\":\"thinking\"}}]}\n");
        s.feed_chunk("data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_z\",\"function\":{\"name\":\"shell\",\"arguments\":\"{}\"}}]}}]}\n");
        let events = collect_events(&mut s);
        let names = event_names(&events);
        // The text item must close BEFORE the tool item opens — Codex
        // chokes if a function_call.added appears while a message item
        // is still mid-stream.
        let text_done = names
            .iter()
            .position(|n| *n == "response.output_item.done")
            .unwrap();
        let tool_added = names
            .iter()
            .enumerate()
            .filter(|(_, n)| **n == "response.output_item.added")
            .map(|(i, _)| i)
            .next_back()
            .unwrap();
        assert!(text_done < tool_added);
    }

    #[test]
    fn stream_to_wire_serialization_is_event_data_format() {
        let evt = SseEvent::new("response.created", json!({"hello":"world"}));
        let wire = evt.to_wire();
        assert_eq!(
            wire,
            "event: response.created\ndata: {\"hello\":\"world\"}\n\n"
        );
    }
}
