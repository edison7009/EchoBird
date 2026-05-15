// Responses API → Chat Completions request translator.
//
// Direct port of tools/codex/lib/protocol-converter.cjs. Behavior parity
// is the goal — every branch in the JS version has a matching branch
// here, with the same dedup rules, the same MiniMax detour, the same
// reorder pass for tool-message pairing, the same tool-array filter.
//
// Why the dictionary is non-trivial: Codex's Responses API input is a
// heterogeneous array of items where every turn / tool call / model
// reasoning step / context-compaction summary lands as a typed
// dictionary entry. Faithful translation matters because dropping even
// one item type produces a Chat Completions message array the upstream
// rejects (the "insufficient tool messages" class of error documented
// in issue #38).

use serde_json::{json, Value};
use std::collections::{BTreeSet, HashSet};

use super::content_mapper::value_to_chat_content;
use super::session_store::SessionStore;

/// Translate one Codex Responses-API request body into the equivalent
/// Chat Completions request body. Returns a fresh JSON value ready to
/// serialize and POST to the upstream `/v1/chat/completions`.
pub fn responses_to_chat(body: &Value, sessions: &SessionStore) -> Value {
    // 1) Replay history stashed under previous_response_id, if any.
    let mut messages: Vec<Value> = body
        .get("previous_response_id")
        .and_then(|v| v.as_str())
        .map(|id| sessions.get_history(id))
        .unwrap_or_default();

    // 2) System instructions — prepend if the message list doesn't
    // already start with one (avoids duplicating on replays).
    if let Some(instr) = body.get("instructions").and_then(|v| v.as_str()) {
        if !instr.is_empty() {
            let has_leading_system = messages
                .first()
                .and_then(|m| m.get("role"))
                .and_then(|v| v.as_str())
                == Some("system");
            if !has_leading_system {
                messages.insert(0, json!({ "role": "system", "content": instr }));
            }
        }
    }

    // 3) Input: either a plain string ("user said this") or an array of
    // typed items (the heterogeneous turn history).
    if let Some(s) = body.get("input").and_then(|v| v.as_str()) {
        messages.push(json!({ "role": "user", "content": s }));
    } else if let Some(items) = body.get("input").and_then(|v| v.as_array()) {
        process_input_items(&mut messages, items, sessions);
    }

    // 4) Provider-specific message-list shaping.
    let is_minimax = body
        .get("model")
        .and_then(|v| v.as_str())
        .map(|m| m.to_lowercase().contains("minimax"))
        .unwrap_or(false);
    let mut merged = if is_minimax {
        minimax_merge(messages)
    } else {
        coalesce_consecutive(messages)
    };

    // 5) Reorder so every assistant.tool_calls is followed immediately
    // by its matching tool messages.
    merged = reorder_tool_messages(merged);

    // 5b) Defensive guard for thinking-model providers (MiMo, DeepSeek-V4
    // thinking variants, etc.). Anything that flowed in via
    // `previous_response_id` history replay — or a stranger path we don't
    // yet model — could be missing `reasoning_content`. We look one more
    // time across every available SessionStore key before committing.
    // Last-resort placeholder ensures the upstream API contract is met
    // even when our store has nothing; without it some providers 400 with
    // "The reasoning_content in the thinking mode must be passed back".
    ensure_reasoning_for_tool_calls(&mut merged, sessions);

    // 6) Assemble the Chat Completions request body.
    let stream_default = body.get("stream").and_then(|v| v.as_bool()).unwrap_or(true);
    let mut chat_body = json!({
        "model": body.get("model").cloned().unwrap_or(Value::Null),
        "messages": merged,
        "stream": stream_default,
    });

    if let Some(v) = body.get("max_output_tokens") {
        if !v.is_null() {
            chat_body["max_tokens"] = v.clone();
        }
    }
    if let Some(v) = body.get("temperature") {
        if !v.is_null() {
            chat_body["temperature"] = v.clone();
        }
    }
    // stop_sequences is the Responses-API name; some clients pass it
    // through as "stop". Either is fine on the Chat side.
    if let Some(v) = body.get("stop_sequences") {
        if !v.is_null() {
            chat_body["stop"] = v.clone();
        }
    }
    if let Some(v) = body.get("stop") {
        if !v.is_null() {
            chat_body["stop"] = v.clone();
        }
    }

    // 7) Tool definitions filter. Built-in Responses tools (local_shell,
    // web_search, file_search, computer_use_preview, custom, ...) have
    // no Chat Completions analogue — passing them through as
    // type=function would produce `tools[N].function: missing field
    // "name"` 400s upstream. Keep only `function` and unpack `namespace`.
    if let Some(tools) = body.get("tools").and_then(|v| v.as_array()) {
        let mut out: Vec<Value> = Vec::new();
        let mut dropped: Vec<String> = Vec::new();
        for tool in tools {
            let tt = tool.get("type").and_then(|v| v.as_str());
            match tt {
                Some("function") => out.push(normalize_function_tool(tool)),
                Some("namespace") => {
                    if let Some(subs) = tool.get("tools").and_then(|v| v.as_array()) {
                        for sub in subs {
                            if sub.get("type").and_then(|v| v.as_str()) == Some("function") {
                                out.push(normalize_function_tool(sub));
                            }
                        }
                    }
                }
                Some(other) => dropped.push(other.to_string()),
                None => {}
            }
        }
        if !out.is_empty() {
            chat_body["tools"] = Value::Array(out);
            if let Some(tc) = body.get("tool_choice") {
                if !tc.is_null() {
                    chat_body["tool_choice"] = tc.clone();
                }
            }
        }
        if !dropped.is_empty() {
            let unique: BTreeSet<&str> = dropped.iter().map(String::as_str).collect();
            let list: Vec<&str> = unique.into_iter().collect();
            log::warn!(
                "[CodexProxy] Dropped {} non-function tool(s): {}",
                dropped.len(),
                list.join(", ")
            );
        }
    }

    chat_body
}

// ────────────────────────────────────────────────────────────────────
// Item-array processing: heterogeneous Responses items → flat messages.
// ────────────────────────────────────────────────────────────────────

fn process_input_items(messages: &mut Vec<Value>, items: &[Value], sessions: &SessionStore) {
    // Per-call dedup: when previous_response_id + input both replay the
    // same items we don't want them twice in the upstream history.
    let mut emitted_call_ids: HashSet<String> = HashSet::new();
    let mut emitted_tool_responses: HashSet<String> = HashSet::new();
    // Reasoning text from a preceding `reasoning` item, waiting to be
    // attached to the next assistant.tool_calls message we construct.
    let mut pending_reasoning: Option<String> = None;

    let mut i = 0;
    while i < items.len() {
        let item = &items[i];
        let t = item.get("type").and_then(|v| v.as_str()).unwrap_or("");

        // ── function_call (group all consecutive into one assistant) ──
        if t == "function_call" {
            let mut grouped: Vec<Value> = Vec::new();
            while i < items.len()
                && items[i].get("type").and_then(|v| v.as_str()) == Some("function_call")
            {
                let cur = &items[i];
                let call_id = extract_call_id(cur).unwrap_or_else(random_call_id);
                if !emitted_call_ids.contains(&call_id) {
                    emitted_call_ids.insert(call_id.clone());
                    let name = cur
                        .get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let args = stringify_args(cur.get("arguments"));
                    grouped.push(json!({
                        "id": call_id,
                        "type": "function",
                        "function": { "name": name, "arguments": args },
                    }));
                }
                i += 1;
            }
            if !grouped.is_empty() {
                push_assistant_tool_calls(messages, grouped, sessions, &mut pending_reasoning);
            }
            continue;
        }

        // ── tool-result items (function_call_output, *_call_output, tool_search_output, ...) ──
        // Suffix is `_output` rather than `_call_output` because some
        // newer types (tool_search_output) drop the `_call_` infix.
        // call_id required to avoid catching content parts like
        // `output_text` / `output_image`.
        if t.ends_with("_output") && item.get("call_id").is_some() {
            if let Some(call_id) = item
                .get("call_id")
                .and_then(|v| v.as_str())
                .map(String::from)
            {
                if !emitted_tool_responses.contains(&call_id) {
                    emitted_tool_responses.insert(call_id.clone());
                    let content = stringify_output(item.get("output"));
                    messages.push(json!({
                        "role": "tool",
                        "tool_call_id": call_id,
                        "content": content,
                    }));
                }
            }
            i += 1;
            continue;
        }

        // ── local_shell_call (Codex's built-in shell tool) ──
        if t == "local_shell_call" {
            let call_id = extract_call_id(item).unwrap_or_else(random_call_id);
            if !emitted_call_ids.contains(&call_id) {
                emitted_call_ids.insert(call_id.clone());
                let args = match item.get("action") {
                    Some(v) => serde_json::to_string(v).unwrap_or_else(|_| "{}".to_string()),
                    None => "{}".to_string(),
                };
                let tool_calls = vec![json!({
                    "id": call_id,
                    "type": "function",
                    "function": { "name": "local_shell", "arguments": args },
                })];
                push_assistant_tool_calls(messages, tool_calls, sessions, &mut pending_reasoning);
            }
            i += 1;
            continue;
        }

        // ── reasoning (buffer for next assistant.tool_calls) ──
        if t == "reasoning" {
            let summary = item
                .get("summary")
                .or_else(|| item.get("text"))
                .or_else(|| item.get("content"));
            let summary_str = match summary {
                Some(Value::String(s)) => s.clone(),
                Some(other) => other.to_string(),
                None => String::new(),
            };
            if !summary_str.is_empty() {
                pending_reasoning = Some(summary_str);
            }
            i += 1;
            continue;
        }

        // ── message (user / assistant / system / developer→system) ──
        if t == "message" {
            let mut role = item
                .get("role")
                .and_then(|v| v.as_str())
                .unwrap_or("user")
                .to_string();
            if role == "developer" {
                role = "system".to_string();
            }
            let content = value_to_chat_content(item.get("content"));
            let has_content = match &content {
                Value::String(s) => !s.is_empty(),
                Value::Array(a) => !a.is_empty(),
                _ => false,
            };
            if has_content {
                let mut msg = json!({ "role": role, "content": content });
                if role == "assistant" {
                    let rc_from_store = sessions.get_turn_reasoning(&msg["content"]);
                    let rc = if let Some(r) = rc_from_store {
                        Some(r)
                    } else if let Some(r) = pending_reasoning.take() {
                        sessions.store_turn_reasoning(&msg["content"], &r);
                        Some(r)
                    } else {
                        None
                    };
                    if let Some(r) = rc {
                        msg["reasoning_content"] = Value::String(r);
                    }
                }
                messages.push(msg);
            }
            i += 1;
            continue;
        }

        // ── compaction (Codex 0.130+ context compaction; opaque blob) ──
        if t == "compaction" {
            messages.push(json!({
                "role": "system",
                "content": "[Earlier portion of this conversation was compacted by Codex and is not available to the model.]",
            }));
            i += 1;
            continue;
        }

        // ── generic *_call (custom_tool_call, apply_patch_tool_call, etc.) ──
        // Wrapped as function-style tool_calls so the upstream understands
        // the assistant→tool round-trip. Tool name derives from item.type
        // by stripping the trailing `_call`, unless an explicit `name`
        // is present. Argument field varies (`arguments` for function_call,
        // `input` for custom_tool_call, `action` for local_shell_call).
        if t.ends_with("_call") && item.get("call_id").is_some() {
            if let Some(call_id) = item
                .get("call_id")
                .and_then(|v| v.as_str())
                .map(String::from)
            {
                if !emitted_call_ids.contains(&call_id) {
                    emitted_call_ids.insert(call_id.clone());
                    let tool_name = item
                        .get("name")
                        .and_then(|v| v.as_str())
                        .map(String::from)
                        .unwrap_or_else(|| t.trim_end_matches("_call").to_string());
                    let raw_args = item
                        .get("arguments")
                        .or_else(|| item.get("input"))
                        .or_else(|| item.get("action"));
                    let args = match raw_args {
                        Some(Value::String(s)) => s.clone(),
                        Some(other) => {
                            serde_json::to_string(other).unwrap_or_else(|_| "{}".to_string())
                        }
                        None => "{}".to_string(),
                    };
                    let tool_calls = vec![json!({
                        "id": call_id,
                        "type": "function",
                        "function": { "name": tool_name, "arguments": args },
                    })];
                    push_assistant_tool_calls(
                        messages,
                        tool_calls,
                        sessions,
                        &mut pending_reasoning,
                    );
                }
            }
            i += 1;
            continue;
        }

        log::warn!("[CodexProxy] Skipping unknown input item type: {t}");
        i += 1;
    }
}

// ────────────────────────────────────────────────────────────────────
// Helpers
// ────────────────────────────────────────────────────────────────────

fn extract_call_id(item: &Value) -> Option<String> {
    item.get("call_id")
        .or_else(|| item.get("id"))
        .and_then(|v| v.as_str())
        .map(String::from)
}

fn random_call_id() -> String {
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
    format!("call_{}", suffix)
}

fn stringify_args(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => s.clone(),
        Some(other) => serde_json::to_string(other).unwrap_or_else(|_| "{}".to_string()),
        None => "{}".to_string(),
    }
}

fn stringify_output(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Null) | None => "\"\"".to_string(),
        Some(other) => serde_json::to_string(other).unwrap_or_else(|_| "\"\"".to_string()),
    }
}

fn push_assistant_tool_calls(
    messages: &mut Vec<Value>,
    tool_calls: Vec<Value>,
    sessions: &SessionStore,
    pending_reasoning: &mut Option<String>,
) {
    let first_id = tool_calls
        .first()
        .and_then(|tc| tc.get("id"))
        .and_then(|v| v.as_str())
        .map(String::from)
        .unwrap_or_default();
    let mut reasoning = if !first_id.is_empty() {
        sessions.get_reasoning(&first_id)
    } else {
        None
    };
    if reasoning.is_none() {
        if let Some(pending) = pending_reasoning.take() {
            // Persist under every call_id so subsequent turns find it.
            for tc in &tool_calls {
                if let Some(id) = tc.get("id").and_then(|v| v.as_str()) {
                    sessions.store_reasoning(id, &pending);
                }
            }
            reasoning = Some(pending);
        }
    }
    let mut msg = json!({
        "role": "assistant",
        "content": Value::Null,
        "tool_calls": tool_calls,
    });
    if let Some(r) = reasoning {
        msg["reasoning_content"] = Value::String(r);
    }
    messages.push(msg);
}

/// Last-mile guard: every assistant message with `tool_calls` must carry
/// a non-empty `reasoning_content` field before we send to upstream.
/// Required by thinking-mode providers (MiMo, DeepSeek-V4 thinking, etc.)
/// per their multi-turn API contract — see issue #42 + #40 + #41.
///
/// Lookup is exhaustive: we try every tool_call.id against the reasoning
/// store, not just the first one. Falls back to a single-space placeholder
/// when nothing is found so the upstream API doesn't 400 — the model loses
/// some prior context but the conversation stays alive instead of dying
/// hard. Logs a warning when the placeholder fires so we can spot which
/// branches still leak.
fn ensure_reasoning_for_tool_calls(messages: &mut [Value], sessions: &SessionStore) {
    // Substantive (not whitespace) — MiMo specifically goes silent when
    // fed a single-space stub: the model treats it as "I had a thought
    // but it was empty," then declines to continue. A short neutral
    // sentence keeps the contract satisfied AND gives the model
    // something coherent to anchor against without leaking implementation
    // details into the user-visible context.
    const PLACEHOLDER: &str = "Continuing from previous tool call.";

    for msg in messages.iter_mut() {
        let is_assistant_with_tool_calls = msg.get("role").and_then(|v| v.as_str())
            == Some("assistant")
            && msg
                .get("tool_calls")
                .and_then(|v| v.as_array())
                .is_some_and(|a| !a.is_empty());
        if !is_assistant_with_tool_calls {
            continue;
        }

        // Already present and non-empty → leave alone.
        if let Some(existing) = msg.get("reasoning_content").and_then(|v| v.as_str()) {
            if !existing.is_empty() {
                continue;
            }
        }

        let tool_call_ids: Vec<String> = msg
            .get("tool_calls")
            .and_then(|v| v.as_array())
            .map(|tcs| {
                tcs.iter()
                    .filter_map(|tc| tc.get("id").and_then(|v| v.as_str()).map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        let mut recovered: Option<String> = None;
        for id in &tool_call_ids {
            if let Some(r) = sessions.get_reasoning(id) {
                if !r.is_empty() {
                    recovered = Some(r);
                    break;
                }
            }
        }

        match recovered {
            Some(r) => {
                msg["reasoning_content"] = Value::String(r);
            }
            None => {
                log::warn!(
                    "[CodexProxy] reasoning_content missing for assistant tool_calls {:?}; \
                     injecting placeholder to satisfy thinking-model API contract",
                    tool_call_ids
                );
                msg["reasoning_content"] = Value::String(PLACEHOLDER.to_string());
            }
        }
    }
}

// ── MiniMax legacy mode: merge system text into the first user message ──
// MiniMax mishandles standalone system roles; we bundle all consecutive
// system content into a "[System Instructions]\n..." prefix on the next
// user message.
fn minimax_merge(messages: Vec<Value>) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::new();
    let mut pending_system = String::new();
    for mut msg in messages {
        let role = msg.get("role").and_then(|v| v.as_str()).unwrap_or("");
        if role == "system" {
            if let Some(s) = msg.get("content").and_then(|v| v.as_str()) {
                if !pending_system.is_empty() {
                    pending_system.push('\n');
                }
                pending_system.push_str(s);
                continue;
            }
        }
        if !pending_system.is_empty() {
            // If the next message is a string-content user, prefix in place.
            let is_string_user =
                role == "user" && msg.get("content").and_then(|v| v.as_str()).is_some();
            if is_string_user {
                let original = msg["content"].as_str().unwrap_or("").to_string();
                msg["content"] = Value::String(format!(
                    "[System Instructions]\n{pending_system}\n\n{original}"
                ));
            } else {
                out.push(json!({
                    "role": "user",
                    "content": format!("[System Instructions]\n{pending_system}"),
                }));
            }
            pending_system.clear();
        }
        out.push(msg);
    }
    if !pending_system.is_empty() {
        out.push(json!({
            "role": "user",
            "content": format!("[System Instructions]\n{pending_system}"),
        }));
    }
    if out.is_empty() {
        out.push(json!({ "role": "user", "content": "Hello" }));
    }
    out
}

// ── Coalesce consecutive same-role plain-text messages ──
// Never merges anything involving tool_calls or role=tool (those are
// structurally distinct slots in Chat Completions).
fn coalesce_consecutive(messages: Vec<Value>) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::with_capacity(messages.len());
    for msg in messages {
        let can_merge = out.last().is_some_and(|last| {
            let last_role = last.get("role").and_then(|v| v.as_str());
            let cur_role = msg.get("role").and_then(|v| v.as_str());
            last_role == cur_role
                && last_role != Some("tool")
                && last.get("tool_calls").is_none()
                && msg.get("tool_calls").is_none()
                && last.get("content").and_then(|v| v.as_str()).is_some()
                && msg.get("content").and_then(|v| v.as_str()).is_some()
        });
        if can_merge {
            let last = out.last_mut().unwrap();
            let last_content = last["content"].as_str().unwrap_or("").to_string();
            let cur_content = msg["content"].as_str().unwrap_or("");
            last["content"] = Value::String(format!("{last_content}\n\n{cur_content}"));
        } else {
            out.push(msg);
        }
    }
    out
}

// ── Reorder: pull every tool message right after its matching assistant ──
// Two-phase: index tool messages by id, then walk skipping tools we'll
// emit alongside their assistant. Orphans on either side stay where
// they were (partial-history inputs / test fixtures rely on this).
fn reorder_tool_messages(messages: Vec<Value>) -> Vec<Value> {
    use std::collections::HashMap;

    let mut tool_by_call_id: HashMap<String, Value> = HashMap::new();
    for m in &messages {
        if m.get("role").and_then(|v| v.as_str()) == Some("tool") {
            if let Some(id) = m.get("tool_call_id").and_then(|v| v.as_str()) {
                tool_by_call_id
                    .entry(id.to_string())
                    .or_insert_with(|| m.clone());
            }
        }
    }

    let mut emitted: HashSet<String> = HashSet::new();
    let mut result: Vec<Value> = Vec::with_capacity(messages.len());
    for m in messages {
        if m.get("role").and_then(|v| v.as_str()) == Some("tool") {
            if let Some(id) = m.get("tool_call_id").and_then(|v| v.as_str()) {
                if emitted.contains(id) {
                    continue; // already pulled forward
                }
            }
        }
        let is_tool_call_assistant = m.get("role").and_then(|v| v.as_str()) == Some("assistant")
            && m.get("tool_calls")
                .and_then(|v| v.as_array())
                .is_some_and(|a| !a.is_empty());

        // Snapshot tool_call ids before moving the message into result.
        let pending_ids: Vec<String> = if is_tool_call_assistant {
            m.get("tool_calls")
                .and_then(|v| v.as_array())
                .map(|tcs| {
                    tcs.iter()
                        .filter_map(|tc| tc.get("id").and_then(|v| v.as_str()).map(String::from))
                        .collect()
                })
                .unwrap_or_default()
        } else {
            Vec::new()
        };

        result.push(m);

        for id in pending_ids {
            if !emitted.contains(&id) {
                if let Some(tool_msg) = tool_by_call_id.get(&id) {
                    result.push(tool_msg.clone());
                    emitted.insert(id);
                }
            }
        }
    }
    result
}

// ── Tool-definition normalizer ──
// Accepts both nested (`{type:"function", function:{...}}`) and flat
// (`{type:"function", name, description, parameters, strict}`) shapes
// and returns the nested Chat Completions form.
fn normalize_function_tool(tool: &Value) -> Value {
    if let Some(inner) = tool.get("function") {
        if inner.is_object() {
            return json!({ "type": "function", "function": inner });
        }
    }
    let mut fn_obj = serde_json::Map::new();
    if let Some(name) = tool.get("name") {
        if !name.is_null() {
            fn_obj.insert("name".to_string(), name.clone());
        }
    }
    if let Some(desc) = tool.get("description") {
        if !desc.is_null() {
            fn_obj.insert("description".to_string(), desc.clone());
        }
    }
    if let Some(params) = tool.get("parameters") {
        if !params.is_null() {
            fn_obj.insert("parameters".to_string(), params.clone());
        }
    }
    if let Some(strict) = tool.get("strict") {
        if !strict.is_null() {
            fn_obj.insert("strict".to_string(), strict.clone());
        }
    }
    json!({ "type": "function", "function": Value::Object(fn_obj) })
}

// ────────────────────────────────────────────────────────────────────
// Tests — direct port of the most load-bearing cases from the
// original responses-to-chat suite, plus session-store-backed cases
// (history replay, reasoning round-trip).
// ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> SessionStore {
        SessionStore::new()
    }

    #[test]
    fn pure_string_input_becomes_one_user_message() {
        let body = json!({
            "model": "deepseek-chat",
            "input": "hello",
        });
        let out = responses_to_chat(&body, &store());
        assert_eq!(out["messages"][0]["role"], "user");
        assert_eq!(out["messages"][0]["content"], "hello");
        assert_eq!(out["model"], "deepseek-chat");
        assert_eq!(out["stream"], true);
    }

    #[test]
    fn instructions_become_leading_system_message() {
        let body = json!({
            "model": "deepseek-chat",
            "instructions": "you are helpful",
            "input": "hi",
        });
        let out = responses_to_chat(&body, &store());
        assert_eq!(out["messages"][0]["role"], "system");
        assert_eq!(out["messages"][0]["content"], "you are helpful");
        assert_eq!(out["messages"][1]["role"], "user");
    }

    #[test]
    fn consecutive_function_calls_group_into_one_assistant() {
        let body = json!({
            "model": "deepseek-chat",
            "input": [
                { "type": "function_call", "call_id": "c1", "name": "a", "arguments": "{}" },
                { "type": "function_call", "call_id": "c2", "name": "b", "arguments": "{}" },
            ],
        });
        let out = responses_to_chat(&body, &store());
        let msgs = out["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0]["role"], "assistant");
        assert_eq!(msgs[0]["tool_calls"].as_array().unwrap().len(), 2);
        assert_eq!(msgs[0]["tool_calls"][0]["id"], "c1");
        assert_eq!(msgs[0]["tool_calls"][1]["id"], "c2");
    }

    #[test]
    fn function_call_output_becomes_tool_message() {
        let body = json!({
            "model": "deepseek-chat",
            "input": [
                { "type": "function_call_output", "call_id": "c1", "output": "result" },
            ],
        });
        let out = responses_to_chat(&body, &store());
        let msgs = out["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0]["role"], "tool");
        assert_eq!(msgs[0]["tool_call_id"], "c1");
        assert_eq!(msgs[0]["content"], "result");
    }

    #[test]
    fn local_shell_call_becomes_assistant_tool_call() {
        let body = json!({
            "model": "deepseek-chat",
            "input": [
                { "type": "local_shell_call", "call_id": "c1", "action": { "cmd": "ls" } },
            ],
        });
        let out = responses_to_chat(&body, &store());
        let msgs = out["messages"].as_array().unwrap();
        assert_eq!(msgs[0]["role"], "assistant");
        let tcs = msgs[0]["tool_calls"].as_array().unwrap();
        assert_eq!(tcs[0]["function"]["name"], "local_shell");
        assert_eq!(tcs[0]["function"]["arguments"], "{\"cmd\":\"ls\"}");
    }

    #[test]
    fn interleaved_developer_msg_gets_pulled_past_tool() {
        // The exact misordering that caused issue #38's
        // "insufficient tool messages following tool_calls" error.
        let body = json!({
            "model": "deepseek-chat",
            "input": [
                { "type": "function_call", "call_id": "c1", "name": "sh", "arguments": "{}" },
                { "type": "message", "role": "developer", "content": "side note" },
                { "type": "function_call_output", "call_id": "c1", "output": "ok" },
            ],
        });
        let out = responses_to_chat(&body, &store());
        let roles: Vec<&str> = out["messages"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m.get("role").and_then(|v| v.as_str()).unwrap_or(""))
            .collect();
        assert_eq!(roles, vec!["assistant", "tool", "system"]);
    }

    #[test]
    fn generic_custom_tool_call_emits_assistant_tool_calls() {
        let body = json!({
            "model": "deepseek-chat",
            "input": [
                { "type": "custom_tool_call", "call_id": "c1", "name": "browser", "input": "click(1)" },
            ],
        });
        let out = responses_to_chat(&body, &store());
        let tcs = out["messages"][0]["tool_calls"].as_array().unwrap();
        assert_eq!(tcs[0]["function"]["name"], "browser");
        assert_eq!(tcs[0]["function"]["arguments"], "click(1)");
    }

    #[test]
    fn compaction_emits_placeholder_system() {
        let body = json!({
            "model": "deepseek-chat",
            "input": [
                { "type": "compaction", "encrypted_content": "opaque" },
            ],
        });
        let out = responses_to_chat(&body, &store());
        assert_eq!(out["messages"][0]["role"], "system");
        let content = out["messages"][0]["content"].as_str().unwrap();
        assert!(content.contains("compacted"));
    }

    #[test]
    fn developer_role_collapses_to_system() {
        let body = json!({
            "model": "deepseek-chat",
            "input": [
                { "type": "message", "role": "developer", "content": "note" },
            ],
        });
        let out = responses_to_chat(&body, &store());
        assert_eq!(out["messages"][0]["role"], "system");
        assert_eq!(out["messages"][0]["content"], "note");
    }

    #[test]
    fn tools_filter_drops_non_function_types() {
        let body = json!({
            "model": "deepseek-chat",
            "input": "hi",
            "tools": [
                { "type": "function", "name": "foo", "description": "f", "parameters": {} },
                { "type": "web_search" },
                { "type": "local_shell" },
            ],
        });
        let out = responses_to_chat(&body, &store());
        let tools = out["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["function"]["name"], "foo");
    }

    #[test]
    fn minimax_merges_system_into_user() {
        let body = json!({
            "model": "minimax-chat",
            "input": [
                { "type": "message", "role": "developer", "content": "system note" },
                { "type": "message", "role": "user", "content": "hello" },
            ],
        });
        let out = responses_to_chat(&body, &store());
        let msgs = out["messages"].as_array().unwrap();
        assert_eq!(msgs[0]["role"], "user");
        let c = msgs[0]["content"].as_str().unwrap();
        assert!(c.contains("[System Instructions]"));
        assert!(c.contains("system note"));
        assert!(c.contains("hello"));
    }

    #[test]
    fn coalesce_merges_consecutive_user_strings() {
        let body = json!({
            "model": "deepseek-chat",
            "input": [
                { "type": "message", "role": "user", "content": "one" },
                { "type": "message", "role": "user", "content": "two" },
            ],
        });
        let out = responses_to_chat(&body, &store());
        let msgs = out["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0]["content"], "one\n\ntwo");
    }

    #[test]
    fn normalize_function_tool_handles_flat_shape() {
        let flat = json!({
            "type": "function",
            "name": "foo",
            "description": "d",
            "parameters": { "type": "object" },
        });
        let out = normalize_function_tool(&flat);
        assert_eq!(out["type"], "function");
        assert_eq!(out["function"]["name"], "foo");
        assert_eq!(out["function"]["description"], "d");
    }

    #[test]
    fn unknown_item_type_is_skipped() {
        let body = json!({
            "model": "deepseek-chat",
            "input": [
                { "type": "totally_made_up", "junk": 1 },
                { "type": "message", "role": "user", "content": "real" },
            ],
        });
        let out = responses_to_chat(&body, &store());
        let msgs = out["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0]["content"], "real");
    }

    // ── reasoning_content defensive injection (issues #40 / #41 / #42) ──

    #[test]
    fn reasoning_recovered_from_session_store_by_first_call_id() {
        // History replay path: prior turn's assistant came back through
        // previous_response_id but lost reasoning_content. The store still
        // has it under the tool-call id — last-mile guard recovers it.
        let s = store();
        s.store_reasoning("call_abc", "deep thought");
        s.save_history(
            "resp_prev",
            vec![
                json!({ "role": "user", "content": "go" }),
                json!({
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_abc",
                        "type": "function",
                        "function": { "name": "shell", "arguments": "{}" },
                    }],
                    // NB: reasoning_content omitted to simulate the leak.
                }),
            ],
        );
        let body = json!({
            "model": "deepseek-v4-flash",
            "previous_response_id": "resp_prev",
            "input": [
                { "type": "function_call_output", "call_id": "call_abc", "output": "/home" },
            ],
        });
        let out = responses_to_chat(&body, &s);
        let msgs = out["messages"].as_array().unwrap();
        let assistant = msgs.iter().find(|m| m["role"] == "assistant").unwrap();
        assert_eq!(assistant["reasoning_content"], "deep thought");
    }

    #[test]
    fn reasoning_recovered_from_any_tool_call_id_not_just_first() {
        // First id missing, second id has stored reasoning — should still recover.
        let s = store();
        s.store_reasoning("call_b", "second-id reasoning");
        s.save_history(
            "resp_x",
            vec![json!({
                "role": "assistant",
                "content": null,
                "tool_calls": [
                    { "id": "call_a", "type": "function", "function": { "name": "f1", "arguments": "{}" } },
                    { "id": "call_b", "type": "function", "function": { "name": "f2", "arguments": "{}" } },
                ],
            })],
        );
        let body = json!({
            "model": "mimo-v2.5-pro",
            "previous_response_id": "resp_x",
            "input": [],
        });
        let out = responses_to_chat(&body, &s);
        let assistant = out["messages"]
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["role"] == "assistant")
            .unwrap();
        assert_eq!(assistant["reasoning_content"], "second-id reasoning");
    }

    #[test]
    fn reasoning_placeholder_when_store_has_nothing() {
        // Worst case: history replay lost reasoning_content AND store
        // has nothing under any tool-call id. We inject a placeholder so
        // the thinking-mode API contract is satisfied — the conversation
        // continues even when our state tracking has a hole.
        let s = store();
        s.save_history(
            "resp_orphan",
            vec![json!({
                "role": "assistant",
                "content": null,
                "tool_calls": [{
                    "id": "call_lost",
                    "type": "function",
                    "function": { "name": "shell", "arguments": "{}" },
                }],
            })],
        );
        let body = json!({
            "model": "mimo-v2.5-pro",
            "previous_response_id": "resp_orphan",
            "input": [],
        });
        let out = responses_to_chat(&body, &s);
        let assistant = out["messages"]
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["role"] == "assistant")
            .unwrap();
        // Must be a non-empty string (thinking-mode providers reject
        // missing-or-empty reasoning_content).
        let r = assistant["reasoning_content"].as_str().unwrap();
        assert!(!r.is_empty());
    }

    #[test]
    fn reasoning_injection_skips_plain_assistant_messages() {
        // Assistant turns without tool_calls don't need reasoning_content.
        // Injecting one would pollute non-thinking conversations.
        let body = json!({
            "model": "gpt-4o",
            "input": [
                { "type": "message", "role": "assistant", "content": "hi there" },
            ],
        });
        let out = responses_to_chat(&body, &store());
        let assistant = &out["messages"].as_array().unwrap()[0];
        assert_eq!(assistant["role"], "assistant");
        assert!(
            assistant.get("reasoning_content").is_none(),
            "should not inject reasoning_content on plain assistant"
        );
    }

    #[test]
    fn reasoning_already_present_is_preserved() {
        // If history replay already carries reasoning_content, we must
        // not clobber it with a placeholder.
        let s = store();
        s.save_history(
            "resp_with",
            vec![json!({
                "role": "assistant",
                "content": null,
                "tool_calls": [{ "id": "call_z", "type": "function", "function": { "name": "f", "arguments": "{}" } }],
                "reasoning_content": "original detailed reasoning",
            })],
        );
        let body = json!({
            "model": "mimo-v2.5-pro",
            "previous_response_id": "resp_with",
            "input": [],
        });
        let out = responses_to_chat(&body, &s);
        let assistant = out["messages"]
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["role"] == "assistant")
            .unwrap();
        assert_eq!(
            assistant["reasoning_content"],
            "original detailed reasoning"
        );
    }
}
