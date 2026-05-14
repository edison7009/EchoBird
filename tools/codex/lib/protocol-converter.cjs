const { valueToChatContent } = require("./content-mapper.cjs");

// Responses → Chat Completions
//
// Responses API input is a heterogeneous array of items. Faithful
// translation of ALL item types is mandatory — dropping function_call /
// function_call_output produces an empty messages array on tool-call
// follow-up turns, which is what nuked v4.0.2.

function responsesToChat(body, sessions, logger) {
    const warn = logger?.warn || (() => {});

    // Replay any prior history we stashed under previous_response_id.
    // If the lookup misses (e.g., process restarted, or Codex sent a
    // response_id we never created), we degrade to an empty history
    // and rely on Codex including the full input items array.
    const messages = body.previous_response_id
        ? sessions.getHistory(body.previous_response_id).slice()
        : [];

    if (body.instructions) {
        // Only prepend if there isn't already a system message at the top
        // (mirrors codex-relay's behaviour — avoids duplicating instructions
        // on replays).
        if (messages.length === 0 || messages[0].role !== "system") {
            messages.unshift({ role: "system", content: body.instructions });
        }
    }

    if (typeof body.input === "string") {
        messages.push({ role: "user", content: body.input });
    } else if (Array.isArray(body.input)) {
        // Track call_ids we've already emitted as tool_calls or tool
        // responses, so when Codex's previous_response_id + input both
        // replay the same items we don't duplicate them.
        const emittedCallIds = new Set();
        const emittedToolResponses = new Set();
        // reasoning_content from a preceding reasoning item that Codex
        // replays from its persistent session. When the in-memory store
        // is cold (proxy restart), this bridges the gap so DeepSeek/
        // Moonshot/Kimi thinking models still get their required
        // reasoning_content round-trip on the next assistant message.
        let pendingReasoning = null;
        const items = body.input;
        let i = 0;
        while (i < items.length) {
            const item = items[i];
            const t = item?.type;

            if (t === "function_call") {
                // Group ALL consecutive function_calls into one assistant
                // message. Chat Completions spec requires that all tool
                // calls produced in one turn live in a single assistant
                // message's tool_calls[] array — emitting them as separate
                // messages breaks providers (DeepSeek/Moonshot/etc).
                const grouped = [];
                while (i < items.length && items[i]?.type === "function_call") {
                    const cur = items[i];
                    const callId = cur.call_id || cur.id
                        || `call_${Math.random().toString(36).slice(2, 12)}`;
                    if (!emittedCallIds.has(callId)) {
                        emittedCallIds.add(callId);
                        grouped.push({
                            id: callId,
                            type: "function",
                            function: {
                                name: cur.name || "",
                                arguments: typeof cur.arguments === "string"
                                    ? cur.arguments
                                    : JSON.stringify(cur.arguments || {}),
                            },
                        });
                    }
                    i++;
                }
                if (grouped.length > 0) {
                    // Recover reasoning_content. Prefer the store (warm
                    // path — filled during this proxy session). Fall back
                    // to pendingReasoning set by a preceding reasoning
                    // item Codex replayed from its persistent session
                    // (cold-start path after proxy restart).
                    let reasoningContent = sessions.getReasoning(grouped[0].id);
                    if (!reasoningContent && pendingReasoning) {
                        reasoningContent = pendingReasoning;
                        pendingReasoning = null;
                        // Persist it into the store under every call_id
                        // so subsequent turns' lookups succeed.
                        for (const g of grouped) sessions.storeReasoning(g.id, reasoningContent);
                    }
                    const assistantMsg = {
                        role: "assistant",
                        content: null,
                        tool_calls: grouped,
                    };
                    if (reasoningContent) assistantMsg.reasoning_content = reasoningContent;
                    messages.push(assistantMsg);
                }
                continue;
            }

            // Any tool-result item with a call_id maps to a Chat
            // Completions tool message. The Responses API has a growing
            // list of these — function_call_output, local_shell_call_output,
            // custom_tool_call_output, function_shell_tool_call_output,
            // apply_patch_tool_call_output, computer_tool_call_output,
            // tool_search_output (no `_call_` infix!), etc.
            //
            // The suffix is just `_output` because some types (notably
            // tool_search_output) skip the `_call` infix. We require
            // call_id to avoid matching content parts like `output_text`
            // or `output_image` which aren't tool results.
            //
            // If we missed any of these, the matching *_call assistant
            // message above would still be emitted, leaving the upstream
            // with unanswered tool_call_ids:
            //   "An assistant message with 'tool_calls' must be followed
            //    by tool messages responding to each 'tool_call_id'"
            if (t && t.endsWith("_output") && item.call_id) {
                const callId = item.call_id;
                if (!emittedToolResponses.has(callId)) {
                    emittedToolResponses.add(callId);
                    const out = typeof item.output === "string"
                        ? item.output
                        : JSON.stringify(item.output ?? "");
                    messages.push({
                        role: "tool",
                        tool_call_id: callId,
                        content: out,
                    });
                }
                i++;
                continue;
            }

            if (t === "local_shell_call") {
                // Codex emits these when the model decides to invoke the
                // built-in local_shell tool. Chat-side, surface it as an
                // assistant tool_call so the upstream model sees the
                // history correctly.
                const callId = item.call_id || item.id
                    || `call_${Math.random().toString(36).slice(2, 12)}`;
                if (!emittedCallIds.has(callId)) {
                    emittedCallIds.add(callId);
                    let reasoningContent = sessions.getReasoning(callId);
                    if (!reasoningContent && pendingReasoning) {
                        reasoningContent = pendingReasoning;
                        pendingReasoning = null;
                        sessions.storeReasoning(callId, reasoningContent);
                    }
                    const msg = {
                        role: "assistant",
                        content: null,
                        tool_calls: [{
                            id: callId,
                            type: "function",
                            function: {
                                name: "local_shell",
                                arguments: JSON.stringify(item.action || {}),
                            },
                        }],
                    };
                    if (reasoningContent) msg.reasoning_content = reasoningContent;
                    messages.push(msg);
                }
                i++;
                continue;
            }

            if (t === "reasoning") {
                // Codex 0.128+ replays reasoning items in input history.
                // When the in-memory SessionStore is cold (proxy restarted),
                // getReasoning() returns null and we'd lose reasoning_content
                // on the assistant message — DeepSeek/Moonshot/Kimi reject
                // the turn because thinking models require this round-trip.
                // Instead, buffer the reasoning text now and inject it into
                // the NEXT function_call or local_shell_call assistant message
                // we construct, keyed by the item's summary or ID.
                const summary = item.summary || item.text || item.content || "";
                if (summary) {
                    // Stash so the NEXT function_call/local_shell_call block
                    // (which must appear immediately after this reasoning
                    // item, same as the original model output order) can pick
                    // it up. Also persist to the store so getReasoning() can
                    // find it regardless of the key used.
                    pendingReasoning = summary;
                }
                i++;
                continue;
            }

            if (t === "message") {
                let role = item.role || "user";
                if (role === "developer") role = "system";
                // valueToChatContent returns string for pure-text content
                // or a multimodal parts array when images / other parts are
                // present. Both shapes are valid Chat Completions content.
                const content = valueToChatContent(item.content);
                const hasContent = typeof content === "string"
                    ? content.length > 0
                    : Array.isArray(content) && content.length > 0;
                if (hasContent) {
                    const msg = { role, content };
                    // For assistant replays (Codex sending its prior turn
                    // back as a message item), try to recover the original
                    // reasoning_content via the turn-fingerprint index —
                    // thinking models need this on every turn.
                    if (role === "assistant") {
                        let rc = sessions.getTurnReasoning(content);
                        if (!rc && pendingReasoning) {
                            rc = pendingReasoning;
                            pendingReasoning = null;
                            if (rc) sessions.storeTurnReasoning(content, rc);
                        }
                        if (rc) msg.reasoning_content = rc;
                    }
                    messages.push(msg);
                }
                i++;
                continue;
            }

            // Codex 0.130+ context compaction. Items have type "compaction"
            // with an encrypted_content blob that ONLY OpenAI's servers can
            // decode (uses OpenAI's keys). We can't translate the actual
            // content for a third-party upstream, but we emit a placeholder
            // system message so the model knows context was truncated —
            // otherwise it might confidently claim "you didn't mention X"
            // when X was in the compacted portion.
            if (t === "compaction") {
                messages.push({
                    role: "system",
                    content: "[Earlier portion of this conversation was compacted by Codex and is not available to the model.]",
                });
                i++;
                continue;
            }

            // Generic non-function tool-call handler. Catches custom_tool_call,
            // apply_patch_tool_call, computer_tool_call, function_shell_tool_call,
            // code_interpreter_tool_call, file_search_tool_call, tool_search_call,
            // etc. — every Responses item type ending in `_call` that isn't
            // already handled by the specific branches above (function_call,
            // local_shell_call).
            //
            // The upstream Chat Completions model only understands
            // function-style tool_calls, so we wrap each non-function call
            // as `{type:"function", function:{name, arguments}}`. The matching
            // *_output item above closes the assistant→tool round-trip.
            //
            // Argument fields vary by item type: function_call uses
            // `arguments`, custom_tool_call uses `input`, local_shell_call
            // uses `action`. We try them in priority order and stringify if
            // the value isn't already a string.
            if (t && t.endsWith("_call") && item.call_id) {
                const callId = item.call_id;
                if (!emittedCallIds.has(callId)) {
                    emittedCallIds.add(callId);
                    const toolName = item.name || t.replace(/_call$/, "");
                    const rawArgs = item.arguments ?? item.input ?? item.action ?? {};
                    const args = typeof rawArgs === "string"
                        ? rawArgs
                        : JSON.stringify(rawArgs);
                    let reasoningContent = sessions.getReasoning(callId);
                    if (!reasoningContent && pendingReasoning) {
                        reasoningContent = pendingReasoning;
                        pendingReasoning = null;
                        sessions.storeReasoning(callId, reasoningContent);
                    }
                    const msg = {
                        role: "assistant",
                        content: null,
                        tool_calls: [{
                            id: callId,
                            type: "function",
                            function: { name: toolName, arguments: args },
                        }],
                    };
                    if (reasoningContent) msg.reasoning_content = reasoningContent;
                    messages.push(msg);
                }
                i++;
                continue;
            }

            warn(`[Proxy] Skipping unknown input item type: ${t}`);
            i++;
        }
    }

    // MiniMax legacy mode: merges system into the first user message
    // because MiniMax mishandles standalone system roles. Other providers
    // get coalesced same-role merging that respects tool boundaries.
    const isMinimax = (body.model || "").toLowerCase().includes("minimax");
    let merged;
    if (isMinimax) {
        merged = [];
        let pendingSystem = "";
        for (const msg of messages) {
            if (msg.role === "system" && typeof msg.content === "string") {
                pendingSystem += (pendingSystem ? "\n" : "") + msg.content;
            } else {
                if (pendingSystem) {
                    // Prepend the bundled-up system text. If the next user
                    // message is a multimodal array, push the system blob
                    // as its own user message *before* it instead of trying
                    // to concatenate string + array (which would stringify
                    // the array into "[object Object]" garbage).
                    if (msg.role === "user" && typeof msg.content === "string") {
                        msg.content = `[System Instructions]\n${pendingSystem}\n\n${msg.content}`;
                    } else {
                        merged.push({ role: "user", content: `[System Instructions]\n${pendingSystem}` });
                    }
                    pendingSystem = "";
                }
                merged.push(msg);
            }
        }
        if (pendingSystem) merged.push({ role: "user", content: `[System Instructions]\n${pendingSystem}` });
        if (merged.length === 0) merged.push({ role: "user", content: "Hello" });
    } else {
        // Coalesce consecutive same-role plain-text messages. Never merge
        // anything involving tool_calls or role=tool — those are
        // structurally distinct slots in Chat Completions.
        merged = [];
        for (const msg of messages) {
            const last = merged[merged.length - 1];
            const canMerge = last
                && last.role === msg.role
                && msg.role !== "tool"
                && !last.tool_calls && !msg.tool_calls
                && typeof last.content === "string"
                && typeof msg.content === "string";
            if (canMerge) {
                last.content += "\n\n" + msg.content;
            } else {
                merged.push(msg);
            }
        }
    }

    // Reorder pass: pull tool messages forward to immediately follow their
    // matching assistant.tool_calls.
    //
    // Codex's input items sometimes interleave a `developer`/`user` message
    // BETWEEN a `function_call` and its `function_call_output` (observed in
    // browser-use / shell skill turns where Codex emits a fresh system note
    // mid-sequence). In Responses-API this is fine — the items are
    // independent linked-by-call_id. In Chat Completions it is NOT:
    //
    //   "An assistant message with 'tool_calls' must be followed by tool
    //    messages responding to each 'tool_call_id'.
    //    (insufficient tool messages following tool_calls message)"
    //
    // We move every matching tool message to sit RIGHT AFTER its
    // assistant.tool_calls, and append any interleaved non-matching
    // messages immediately after the last consumed tool message.
    merged = reorderToolMessages(merged);

    const chatBody = {
        model: body.model,
        messages: merged,
        stream: body.stream !== false,
    };
    if (body.max_output_tokens) chatBody.max_tokens = body.max_output_tokens;
    if (body.temperature != null) chatBody.temperature = body.temperature;
    if (body.stop_sequences) chatBody.stop = body.stop_sequences;
    if (body.stop) chatBody.stop = body.stop;

    // Convert Responses-API tools to Chat Completions tools.
    //
    // Strategy (ported from MetaFARS/codex-relay's convert_tools): keep
    // ONLY `type: "function"` and `type: "namespace"` (a container of
    // functions). Drop every built-in type that has no Chat-side analogue
    // — local_shell, web_search, file_search, computer_use_preview,
    // custom (grammar-constrained), etc. — because including them as
    // type=function with a missing name field gets the request 400'd
    // upstream ("tools[3].function: missing field `name`").
    //
    // Codex CLI normally registers a regular `function`-shaped shell tool
    // alongside the built-in local_shell, so dropping local_shell from
    // the tools list still leaves the model with a way to call shell —
    // it just routes through the function path instead.
    if (Array.isArray(body.tools) && body.tools.length > 0) {
        const out = [];
        const dropped = [];
        for (const tool of body.tools) {
            const tt = tool?.type;
            if (tt === "function") {
                out.push(normalizeFunctionTool(tool));
            } else if (tt === "namespace" && Array.isArray(tool.tools)) {
                for (const sub of tool.tools) {
                    if (sub?.type === "function") out.push(normalizeFunctionTool(sub));
                }
            } else if (tt) {
                dropped.push(tt);
            }
        }
        if (out.length > 0) {
            chatBody.tools = out;
            if (body.tool_choice) chatBody.tool_choice = body.tool_choice;
        }
        if (dropped.length > 0) {
            warn(`[Proxy] Dropped ${dropped.length} non-function tool(s): ${[...new Set(dropped)].join(", ")}`);
        }
    }

    return chatBody;
}

// Enforce Chat Completions' strict pairing rule: every assistant message
// with `tool_calls` must be IMMEDIATELY followed by `role: "tool"`
// messages for each tool_call_id, with no other messages between them.
//
// Two-phase algorithm:
//   Phase 1: index every tool message by its tool_call_id.
//   Phase 2: walk messages in order. Skip tool messages (they get pulled
//            into place). For each assistant.tool_calls, strip out any
//            tool_call.id that has no matching tool message (drops the
//            whole message if no valid tool_calls remain AND no plain
//            content), then emit the assistant followed immediately by
//            its matching tool messages.
//
// Defends against three real-world misorderings observed in Codex
// rollouts and theoretical cases:
//   (a) developer/user/system message interleaved between function_call
//       and function_call_output (real, hit in user's rollout 2026-05-15)
//   (b) orphan tool messages whose assistant.tool_calls was never emitted
//       (theoretical, would happen on partial input history)
//   (c) orphan assistant.tool_calls whose matching tool message is
//       missing from input (theoretical, partial history again)
//
// Non-tool, non-assistant-tool_calls messages keep their relative order.
function reorderToolMessages(messages) {
    // Phase 1: tool message → call_id index. First occurrence wins on
    // the off chance there are duplicates (dedup already happens at
    // the converter level; this is belt-and-braces).
    const toolByCallId = new Map();
    for (const m of messages) {
        if (m.role === "tool" && m.tool_call_id && !toolByCallId.has(m.tool_call_id)) {
            toolByCallId.set(m.tool_call_id, m);
        }
    }

    const result = [];
    for (const m of messages) {
        // Tool messages are emitted along with their assistant, not here.
        // Orphans (no matching assistant) get silently dropped — better
        // than letting upstream reject the whole request.
        if (m.role === "tool") continue;

        // Assistant with tool_calls: filter out orphan tool_call.ids,
        // then emit + its tools in one tight block.
        if (m.role === "assistant" && Array.isArray(m.tool_calls) && m.tool_calls.length > 0) {
            const valid = m.tool_calls.filter(tc => tc?.id && toolByCallId.has(tc.id));
            if (valid.length === 0) {
                // Every tool_call is orphan. If the assistant has plain
                // content, keep it sans tool_calls; otherwise drop entirely.
                if (typeof m.content === "string" && m.content.length > 0) {
                    const { tool_calls: _, ...rest } = m;
                    result.push(rest);
                }
                continue;
            }
            result.push(valid.length === m.tool_calls.length ? m : { ...m, tool_calls: valid });
            for (const tc of valid) {
                result.push(toolByCallId.get(tc.id));
            }
            continue;
        }

        result.push(m);
    }
    return result;
}

function normalizeFunctionTool(tool) {
    // Already in Chat shape: {type:"function", function:{...}}
    if (tool.function && typeof tool.function === "object") {
        return { type: "function", function: tool.function };
    }
    // Responses flat shape: {type:"function", name, description, parameters, strict}
    const fn = {};
    if (tool.name) fn.name = tool.name;
    if (tool.description) fn.description = tool.description;
    if (tool.parameters) fn.parameters = tool.parameters;
    if (tool.strict !== undefined) fn.strict = tool.strict;
    return { type: "function", function: fn };
}

module.exports = { responsesToChat, normalizeFunctionTool };
