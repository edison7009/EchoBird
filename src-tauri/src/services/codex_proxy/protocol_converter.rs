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
    // already start with one; REPLACE the head if a different
    // instructions value is supplied. The spec says `instructions`
    // applies only to the current call — when Codex changes
    // instructions mid-conversation, the new value must take effect
    // rather than the old one persisting from `previous_response_id`
    // history replay (L10).
    if let Some(instr) = body.get("instructions").and_then(|v| v.as_str()) {
        if !instr.is_empty() {
            let head_is_system = messages
                .first()
                .and_then(|m| m.get("role"))
                .and_then(|v| v.as_str())
                == Some("system");
            if head_is_system {
                // Replace existing head system content with the new
                // instructions text. Keeps message ordering stable.
                if let Some(first) = messages.first_mut() {
                    first["content"] = Value::String(instr.to_string());
                }
            } else {
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

    // 5a) Orphan tool-call backstop. When Codex sends function_call items
    // without their matching function_call_output (e.g. user interrupted
    // mid-tool-execution, or a Codex client bug like openai/codex#8479),
    // the upstream gets assistant{tool_calls} with no role:tool follow-up
    // and 400s with "tool_calls require matching tool messages". Synth
    // a placeholder tool message for every unmatched call_id so the
    // conversation stays alive — the model sees "(no result)" and can
    // re-plan rather than dying hard.
    ensure_tool_outputs_paired(&mut merged);

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

    // 6b) Pass-through fields that Chat Completions accepts verbatim.
    // Previously dropped silently — Codex sends these on every request,
    // and ignoring them gave users no effect when they tuned settings.
    //
    //   • reasoning.effort     → `reasoning_effort` (Chat side name).
    //                            OpenAI o-series + many third-parties
    //                            honor it. We translate `summary`
    //                            separately by emitting reasoning
    //                            summary events ourselves (H1).
    //   • parallel_tool_calls  → passthrough; defaults to true upstream
    //   • top_p, frequency_penalty, presence_penalty, seed, user,
    //     prompt_cache_key, service_tier, safety_identifier,
    //     max_tool_calls, metadata
    //                          → straight passthrough on non-null
    //   • text.format          → structured outputs (json_schema /
    //                            text). Mapped to Chat's
    //                            `response_format`.
    if let Some(reasoning) = body.get("reasoning") {
        if let Some(effort) = reasoning.get("effort").and_then(|v| v.as_str()) {
            chat_body["reasoning_effort"] = Value::String(effort.to_string());
        }
    }
    for key in &[
        "parallel_tool_calls",
        "top_p",
        "frequency_penalty",
        "presence_penalty",
        "seed",
        "user",
        "prompt_cache_key",
        "service_tier",
        "safety_identifier",
        "max_tool_calls",
        "metadata",
        // M12: `truncation` — "auto" lets the upstream silently drop
        //   oldest turns when prompt exceeds context. Chat API also
        //   accepts it now (mid-2026); pass through verbatim.
        "truncation",
        // L6: `include` — array of additional fields to surface on
        //   the response (e.g. ["reasoning.encrypted_content",
        //   "message.output_text.logprobs"]). Upstream Chat tolerates
        //   the field as unknown; OpenAI-direct uses it natively.
        "include",
    ] {
        if let Some(v) = body.get(*key) {
            if !v.is_null() {
                chat_body[*key] = v.clone();
            }
        }
    }
    // text.format → response_format. The Responses-API shape nests under
    // `text.format`; Chat expects `response_format` at the top. We pass
    // through json_schema, json_object, and text variants verbatim.
    if let Some(text) = body.get("text") {
        if let Some(format) = text.get("format") {
            let format_type = format.get("type").and_then(|v| v.as_str()).unwrap_or("");
            match format_type {
                "json_schema" => {
                    let mut inner = serde_json::Map::new();
                    for k in &["name", "schema", "strict", "description"] {
                        if let Some(v) = format.get(*k) {
                            if !v.is_null() {
                                inner.insert((*k).to_string(), v.clone());
                            }
                        }
                    }
                    chat_body["response_format"] = json!({
                        "type": "json_schema",
                        "json_schema": Value::Object(inner),
                    });
                }
                "json_object" | "text" => {
                    chat_body["response_format"] = json!({ "type": format_type });
                }
                _ => {}
            }
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
        //
        // Read order:
        //   1. `encrypted_content` (preferred — our /compact handler
        //      writes the upstream summary here, and OpenAI's native
        //      flow stores latent state here when `include` was set)
        //   2. `summary[].text` array  (OpenAI's normal reasoning item
        //      shape: `[{type:"summary_text",text:"..."}]`)
        //   3. `summary` raw string  (legacy / our older synthesizer)
        //   4. `text` / `content` fallback
        if t == "reasoning" {
            let mut summary_str = String::new();
            if let Some(enc) = item.get("encrypted_content").and_then(|v| v.as_str()) {
                if !enc.is_empty() && !enc.starts_with("gAAAAA") {
                    summary_str = enc.to_string();
                }
            }
            if summary_str.is_empty() {
                if let Some(arr) = item.get("summary").and_then(|v| v.as_array()) {
                    let parts: Vec<&str> = arr
                        .iter()
                        .filter_map(|p| p.get("text").and_then(|v| v.as_str()))
                        .collect();
                    if !parts.is_empty() {
                        summary_str = parts.join("");
                    }
                }
            }
            if summary_str.is_empty() {
                let raw = item
                    .get("summary")
                    .or_else(|| item.get("text"))
                    .or_else(|| item.get("content"));
                summary_str = match raw {
                    Some(Value::String(s)) => s.clone(),
                    Some(other) => other.to_string(),
                    None => String::new(),
                };
            }
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

        // ── compaction (Codex 0.130+ context compaction) ──
        //
        // Codex sends a compaction item back in the input when it has
        // previously called /v1/responses/compact. OpenAI's native flow
        // ships `encrypted_content` as an opaque blob the model server
        // decrypts. Our /v1/responses/compact handler instead writes a
        // plain-text upstream-generated summary into the same field
        // (the upstream we proxy can't do real encrypted compaction).
        //
        // So: prefer the summary text from `encrypted_content` when it
        // looks like real text; fall back to the generic placeholder
        // when it's empty or actually-opaque (a real OpenAI blob that
        // somehow ended up here).
        // Alias: Codex's newer enum has `context_compaction` (with
        // underscore) as a distinct variant for the standalone-endpoint
        // compaction. Treat identically.
        if t == "compaction" || t == "context_compaction" {
            let summary = item
                .get("encrypted_content")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let content = if !summary.is_empty()
                && !summary.starts_with("gAAAAA")  // common encrypted prefix
                && summary.is_char_boundary(summary.len().min(8))
            {
                format!("[Summary of earlier conversation (from compaction)]\n{summary}")
            } else {
                "[Earlier portion of this conversation was compacted by Codex and is not available to the model.]".to_string()
            };
            messages.push(json!({
                "role": "system",
                "content": content,
            }));
            i += 1;
            continue;
        }

        // ── tool_search_call / web_search_call / image_generation_call ──
        // Codex echoes these in input when the prior turn used the
        // built-in search/generation tools. They don't have a Chat
        // Completions analogue — we synthesize a system note so the
        // model knows a search/generation happened without hard-erroring
        // on "unknown input item type". When the tool result is
        // available (call_id present + matching *_output), the result
        // text is what actually mattered.
        if t == "web_search_call"
            || t == "tool_search_call"
            || t == "image_generation_call"
            || t == "file_search_call"
        {
            let kind = t.trim_end_matches("_call");
            let action = item.get("action").or_else(|| item.get("query"));
            let action_str = match action {
                Some(Value::String(s)) => s.clone(),
                Some(other) => serde_json::to_string(other).unwrap_or_default(),
                None => String::new(),
            };
            let note = if action_str.is_empty() {
                format!("[Codex used the built-in {kind} tool earlier; result is in the next tool output below.]")
            } else {
                format!("[Codex used the built-in {kind} tool: {action_str}]")
            };
            messages.push(json!({
                "role": "system",
                "content": note,
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
        // Spec: function_call_output.output can be a structured array of
        // content items (`[{type:"output_text",text:...}]` or
        // `[{type:"input_text",text:...}]`) rather than a plain string.
        // Join the text parts so the upstream sees natural text, not a
        // JSON-encoded array the model has to parse.
        Some(Value::Array(parts)) => {
            let collected: Vec<&str> = parts
                .iter()
                .filter_map(|p| {
                    p.get("text")
                        .and_then(|v| v.as_str())
                        .filter(|s| !s.is_empty())
                })
                .collect();
            if collected.is_empty() {
                // Array but no text parts → fall back to JSON stringify
                // so structured non-text content (e.g. image refs) at
                // least lands as readable JSON.
                serde_json::to_string(v.unwrap()).unwrap_or_else(|_| "\"\"".to_string())
            } else {
                collected.join("")
            }
        }
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

/// Orphan tool-call backstop. Every `id` listed in an assistant message's
/// `tool_calls` array must have a matching `role: "tool"` message present
/// somewhere in the request — otherwise the upstream Chat Completions API
/// rejects with "messages with tool_calls require matching tool messages"
/// (or a 400 phrased similarly). Codex normally pairs them, but real-world
/// failure modes leave orphans behind:
///
///   • User cancels mid-parallel-tool-call → some outputs never sent
///   • Codex client desync (e.g. openai/codex#8479) → tool_call emitted
///     but matching function_call_output omitted on next turn
///   • previous_response_id history replay racing input items
///
/// For each orphaned call_id we splice in a `{role: "tool", tool_call_id,
/// content: "(no result)"}` placeholder right after the assistant message
/// that introduced it. The model sees the gap explicitly and can re-plan
/// rather than the whole conversation dying with a 400.
fn ensure_tool_outputs_paired(messages: &mut Vec<Value>) {
    use std::collections::HashSet;

    // First pass: collect every tool_call_id that already has a tool
    // message somewhere. (A tool message in any position counts — Anthropic
    // is strict about adjacency, Chat Completions APIs are lenient.)
    let mut satisfied: HashSet<String> = HashSet::new();
    for m in messages.iter() {
        if m.get("role").and_then(|v| v.as_str()) == Some("tool") {
            if let Some(id) = m.get("tool_call_id").and_then(|v| v.as_str()) {
                satisfied.insert(id.to_string());
            }
        }
    }

    // Second pass: for every assistant tool_calls message, find any
    // call_id that isn't satisfied, and remember where to splice the
    // synthetic tool message (right after this assistant message).
    let mut inserts: Vec<(usize, Vec<String>)> = Vec::new();
    for (i, m) in messages.iter().enumerate() {
        if m.get("role").and_then(|v| v.as_str()) != Some("assistant") {
            continue;
        }
        let Some(tcs) = m.get("tool_calls").and_then(|v| v.as_array()) else {
            continue;
        };
        let missing: Vec<String> = tcs
            .iter()
            .filter_map(|tc| tc.get("id").and_then(|v| v.as_str()).map(String::from))
            .filter(|id| !satisfied.contains(id))
            .collect();
        if !missing.is_empty() {
            // Mark as now-satisfied so the same call_id doesn't get a
            // second placeholder if it appears in a later assistant.
            for id in &missing {
                satisfied.insert(id.clone());
            }
            inserts.push((i, missing));
        }
    }

    if inserts.is_empty() {
        return;
    }

    // Splice in reverse so earlier indices stay valid.
    for (idx, ids) in inserts.into_iter().rev() {
        log::warn!(
            "[CodexProxy] Synthesizing placeholder tool messages for orphan call_ids {:?} \
             after assistant at index {}",
            ids,
            idx
        );
        let placeholders: Vec<Value> = ids
            .into_iter()
            .map(|id| {
                json!({
                    "role": "tool",
                    "tool_call_id": id,
                    "content": "(no result — tool execution was interrupted)",
                })
            })
            .collect();
        // Insert each placeholder at idx+1; insertion is contiguous so
        // they end up in `tool_calls` order right after the assistant.
        for (offset, p) in placeholders.into_iter().enumerate() {
            messages.insert(idx + 1 + offset, p);
        }
    }
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
        // One assistant message groups both tool_calls; the orphan-tool
        // backstop then appends two placeholder tool messages because no
        // function_call_output was sent — see ensure_tool_outputs_paired.
        assert_eq!(msgs.len(), 3);
        assert_eq!(msgs[0]["role"], "assistant");
        assert_eq!(msgs[0]["tool_calls"].as_array().unwrap().len(), 2);
        assert_eq!(msgs[0]["tool_calls"][0]["id"], "c1");
        assert_eq!(msgs[0]["tool_calls"][1]["id"], "c2");
        assert_eq!(msgs[1]["role"], "tool");
        assert_eq!(msgs[2]["role"], "tool");
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
    fn compaction_with_plain_summary_uses_it_verbatim() {
        // Our /v1/responses/compact handler writes the upstream-generated
        // summary into encrypted_content. process_input_items should
        // surface that as a system message body so the model sees the
        // real summary, not a generic placeholder.
        let body = json!({
            "model": "deepseek-chat",
            "input": [
                {
                    "type": "compaction",
                    "encrypted_content": "User asked about Rust borrow checker. We discussed lifetimes and pointed at the Nomicon."
                },
            ],
        });
        let out = responses_to_chat(&body, &store());
        assert_eq!(out["messages"][0]["role"], "system");
        let content = out["messages"][0]["content"].as_str().unwrap();
        assert!(content.contains("Summary of earlier conversation"));
        assert!(content.contains("Rust borrow checker"));
    }

    #[test]
    fn compaction_with_empty_encrypted_content_falls_back_to_placeholder() {
        let body = json!({
            "model": "deepseek-chat",
            "input": [
                { "type": "compaction", "encrypted_content": "" },
            ],
        });
        let out = responses_to_chat(&body, &store());
        let content = out["messages"][0]["content"].as_str().unwrap();
        assert!(content.contains("compacted"));
    }

    #[test]
    fn compaction_with_openai_opaque_blob_falls_back_to_placeholder() {
        // A real OpenAI encrypted blob shouldn't be surfaced as if it
        // were a readable summary. The "gAAAAA" prefix is fernet-style
        // base64 prefix that any encrypted blob will start with.
        let body = json!({
            "model": "deepseek-chat",
            "input": [
                {
                    "type": "compaction",
                    "encrypted_content": "gAAAAABabcdef1234567890opaqueblob..."
                },
            ],
        });
        let out = responses_to_chat(&body, &store());
        let content = out["messages"][0]["content"].as_str().unwrap();
        assert!(content.contains("compacted"));
        assert!(!content.contains("gAAAAA"));
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

    // ── orphan tool-call backstop ──

    #[test]
    fn orphan_tool_call_gets_placeholder_tool_message() {
        // Codex sent function_call without matching function_call_output
        // (e.g. user interrupted mid-execution).
        let body = json!({
            "model": "deepseek-chat",
            "input": [
                { "type": "message", "role": "user", "content": "ls /tmp" },
                { "type": "function_call", "call_id": "orphan_1", "name": "shell", "arguments": "{}" },
                // NB: no function_call_output for orphan_1
            ],
        });
        let out = responses_to_chat(&body, &store());
        let msgs = out["messages"].as_array().unwrap();
        // user, assistant{tool_calls}, tool{placeholder}
        assert_eq!(msgs.len(), 3);
        assert_eq!(msgs[2]["role"], "tool");
        assert_eq!(msgs[2]["tool_call_id"], "orphan_1");
        assert!(msgs[2]["content"].as_str().unwrap().contains("no result"));
    }

    #[test]
    fn matched_tool_calls_get_no_placeholder() {
        // Healthy pair — no synthesis should happen.
        let body = json!({
            "model": "deepseek-chat",
            "input": [
                { "type": "function_call", "call_id": "ok_1", "name": "shell", "arguments": "{}" },
                { "type": "function_call_output", "call_id": "ok_1", "output": "result" },
            ],
        });
        let out = responses_to_chat(&body, &store());
        let msgs = out["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[1]["content"], "result");
        // Not the placeholder text
        assert!(!msgs[1]["content"].as_str().unwrap().contains("no result"));
    }

    #[test]
    fn partial_orphan_among_grouped_tool_calls_gets_only_missing_placeholder() {
        // Three tool_calls grouped into one assistant, only two have outputs.
        let body = json!({
            "model": "deepseek-chat",
            "input": [
                { "type": "function_call", "call_id": "c1", "name": "a", "arguments": "{}" },
                { "type": "function_call", "call_id": "c2", "name": "b", "arguments": "{}" },
                { "type": "function_call", "call_id": "c3", "name": "c", "arguments": "{}" },
                { "type": "function_call_output", "call_id": "c1", "output": "r1" },
                { "type": "function_call_output", "call_id": "c3", "output": "r3" },
                // c2 orphaned
            ],
        });
        let out = responses_to_chat(&body, &store());
        let msgs = out["messages"].as_array().unwrap();
        // assistant, tool(c1), tool(c3), placeholder(c2) — order is the
        // assistant's tool_calls order. Placeholder lands after the
        // assistant message before next non-tool segment.
        let tool_ids: Vec<&str> = msgs
            .iter()
            .filter(|m| m["role"] == "tool")
            .map(|m| m["tool_call_id"].as_str().unwrap())
            .collect();
        assert_eq!(tool_ids.len(), 3);
        assert!(tool_ids.contains(&"c1"));
        assert!(tool_ids.contains(&"c2"));
        assert!(tool_ids.contains(&"c3"));
    }

    #[test]
    fn orphan_backstop_skips_when_other_position_already_satisfies() {
        // Tool message exists somewhere in the request — even if not
        // immediately adjacent — so we should not synthesize.
        // (reorder_tool_messages pulls it adjacent anyway; this asserts
        // we never *over*-synthesize.)
        let body = json!({
            "model": "deepseek-chat",
            "input": [
                { "type": "function_call", "call_id": "c1", "name": "a", "arguments": "{}" },
                { "type": "message", "role": "developer", "content": "side note" },
                { "type": "function_call_output", "call_id": "c1", "output": "ok" },
            ],
        });
        let out = responses_to_chat(&body, &store());
        let placeholder_count = out["messages"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|m| {
                m["role"] == "tool"
                    && m["content"]
                        .as_str()
                        .map(|s| s.contains("no result"))
                        .unwrap_or(false)
            })
            .count();
        assert_eq!(placeholder_count, 0);
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

    // ── H5: reasoning.effort pass-through ──
    #[test]
    fn reasoning_effort_passes_through_as_reasoning_effort() {
        let body = json!({
            "model": "gpt-5",
            "input": "hi",
            "reasoning": { "effort": "high", "summary": "auto" },
        });
        let out = responses_to_chat(&body, &store());
        assert_eq!(out["reasoning_effort"], "high");
        // summary is intentionally NOT passed through — we synthesize
        // summary events ourselves in stream_handler.
        assert!(out.get("summary").is_none());
    }

    // ── H6: text.format → response_format ──
    #[test]
    fn text_format_json_schema_maps_to_response_format() {
        let body = json!({
            "model": "gpt-5",
            "input": "give me JSON",
            "text": {
                "format": {
                    "type": "json_schema",
                    "name": "weather",
                    "strict": true,
                    "schema": { "type": "object" },
                }
            },
        });
        let out = responses_to_chat(&body, &store());
        assert_eq!(out["response_format"]["type"], "json_schema");
        assert_eq!(out["response_format"]["json_schema"]["name"], "weather");
        assert_eq!(out["response_format"]["json_schema"]["strict"], true);
    }

    #[test]
    fn text_format_json_object_maps_to_response_format() {
        let body = json!({
            "model": "gpt-5",
            "input": "hi",
            "text": { "format": { "type": "json_object" } },
        });
        let out = responses_to_chat(&body, &store());
        assert_eq!(out["response_format"]["type"], "json_object");
    }

    // ── H7: parallel_tool_calls + M10: misc pass-through ──
    #[test]
    fn parallel_tool_calls_and_misc_fields_pass_through() {
        let body = json!({
            "model": "gpt-5",
            "input": "hi",
            "parallel_tool_calls": false,
            "top_p": 0.9,
            "frequency_penalty": 0.5,
            "presence_penalty": 0.3,
            "seed": 42,
            "user": "u-1",
            "prompt_cache_key": "ck-1",
            "service_tier": "flex",
            "metadata": { "k": "v" },
        });
        let out = responses_to_chat(&body, &store());
        assert_eq!(out["parallel_tool_calls"], false);
        assert_eq!(out["top_p"], 0.9);
        assert_eq!(out["frequency_penalty"], 0.5);
        assert_eq!(out["presence_penalty"], 0.3);
        assert_eq!(out["seed"], 42);
        assert_eq!(out["user"], "u-1");
        assert_eq!(out["prompt_cache_key"], "ck-1");
        assert_eq!(out["service_tier"], "flex");
        assert_eq!(out["metadata"]["k"], "v");
    }

    // ── H8: structured function_call_output content array ──
    #[test]
    fn function_call_output_with_text_part_array_joins_text() {
        let body = json!({
            "model": "deepseek-chat",
            "input": [
                {
                    "type": "function_call",
                    "call_id": "c1",
                    "name": "shell",
                    "arguments": "{}"
                },
                {
                    "type": "function_call_output",
                    "call_id": "c1",
                    "output": [
                        { "type": "output_text", "text": "line one\n" },
                        { "type": "output_text", "text": "line two" }
                    ]
                },
            ],
        });
        let out = responses_to_chat(&body, &store());
        let tool_msg = out["messages"]
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["role"] == "tool")
            .unwrap();
        assert_eq!(tool_msg["content"], "line one\nline two");
    }

    // ── M14: reasoning.encrypted_content roundtrip on input ──
    #[test]
    fn input_reasoning_item_with_encrypted_content_buffers_for_next_tool_call() {
        let body = json!({
            "model": "deepseek-chat",
            "input": [
                {
                    "type": "reasoning",
                    "encrypted_content": "I should look at the files first.",
                    "summary": []
                },
                {
                    "type": "function_call",
                    "call_id": "c1",
                    "name": "shell",
                    "arguments": "{}"
                },
            ],
        });
        let out = responses_to_chat(&body, &store());
        let assistant = out["messages"]
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["role"] == "assistant")
            .unwrap();
        assert_eq!(
            assistant["reasoning_content"],
            "I should look at the files first."
        );
    }

    #[test]
    fn input_reasoning_item_summary_array_text_concatenated() {
        let body = json!({
            "model": "deepseek-chat",
            "input": [
                {
                    "type": "reasoning",
                    "summary": [
                        { "type": "summary_text", "text": "Step 1. " },
                        { "type": "summary_text", "text": "Step 2." }
                    ]
                },
                {
                    "type": "function_call",
                    "call_id": "c1",
                    "name": "shell",
                    "arguments": "{}"
                },
            ],
        });
        let out = responses_to_chat(&body, &store());
        let assistant = out["messages"]
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["role"] == "assistant")
            .unwrap();
        assert_eq!(assistant["reasoning_content"], "Step 1. Step 2.");
    }

    // ── L15-L17: handle web_search_call etc. + context_compaction alias ──
    #[test]
    fn web_search_call_input_item_becomes_system_note() {
        let body = json!({
            "model": "deepseek-chat",
            "input": [
                { "type": "web_search_call", "call_id": "ws_1", "action": { "query": "rust async" } },
            ],
        });
        let out = responses_to_chat(&body, &store());
        let msg = &out["messages"].as_array().unwrap()[0];
        assert_eq!(msg["role"], "system");
        let content = msg["content"].as_str().unwrap();
        assert!(content.contains("web_search"));
        assert!(content.contains("rust async"));
    }

    #[test]
    fn context_compaction_aliases_to_compaction() {
        let body = json!({
            "model": "deepseek-chat",
            "input": [
                {
                    "type": "context_compaction",
                    "encrypted_content": "Summary: user wanted X."
                },
            ],
        });
        let out = responses_to_chat(&body, &store());
        let msg = &out["messages"].as_array().unwrap()[0];
        let content = msg["content"].as_str().unwrap();
        assert!(content.contains("Summary"));
    }

    // ── M12: truncation passthrough ──
    #[test]
    fn truncation_parameter_passes_through() {
        let body = json!({
            "model": "gpt-5",
            "input": "hi",
            "truncation": "auto",
        });
        let out = responses_to_chat(&body, &store());
        assert_eq!(out["truncation"], "auto");
    }

    // ── L6: include array passthrough ──
    #[test]
    fn include_array_passes_through() {
        let body = json!({
            "model": "gpt-5",
            "input": "hi",
            "include": ["reasoning.encrypted_content", "message.output_text.logprobs"],
        });
        let out = responses_to_chat(&body, &store());
        let include = out["include"].as_array().unwrap();
        assert_eq!(include.len(), 2);
    }

    // ── L10: instructions override replaces head system on continuations ──
    #[test]
    fn instructions_replaces_existing_head_system_on_continuation() {
        let s = store();
        // Prior turn left a system message at head of history.
        s.save_history(
            "resp_prev",
            vec![
                json!({ "role": "system", "content": "old instructions" }),
                json!({ "role": "user", "content": "hi" }),
                json!({ "role": "assistant", "content": "hello" }),
            ],
        );
        let body = json!({
            "model": "gpt-5",
            "previous_response_id": "resp_prev",
            "input": "follow up",
            "instructions": "NEW instructions",
        });
        let out = responses_to_chat(&body, &s);
        let msgs = out["messages"].as_array().unwrap();
        // Head must be the NEW instructions, not the old one.
        assert_eq!(msgs[0]["role"], "system");
        assert_eq!(msgs[0]["content"], "NEW instructions");
    }
}
