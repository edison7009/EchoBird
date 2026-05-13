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
                    // Recover reasoning_content from prior turn (thinking
                    // models need it round-tripped or context degrades).
                    // We key by the FIRST call_id in the group — store-side
                    // we save under every call_id, so any of them resolves.
                    const reasoningContent = sessions.getReasoning(grouped[0].id);
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

            if (t === "function_call_output" || t === "local_shell_call_output") {
                // local_shell_call_output (Codex 0.130+ for the built-in
                // shell tool) maps to a regular tool result in Chat.
                const callId = item.call_id || item.id || "";
                if (callId && !emittedToolResponses.has(callId)) {
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
                    const reasoningContent = sessions.getReasoning(callId);
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
                // Codex 0.128+ replays reasoning items in input history. The
                // SessionStore round-trips reasoning_content separately
                // (keyed by call_id / content fingerprint), so these
                // standalone reasoning items don't need to map to chat
                // messages — drop them.
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
                        const rc = sessions.getTurnReasoning(content);
                        if (rc) msg.reasoning_content = rc;
                    }
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
