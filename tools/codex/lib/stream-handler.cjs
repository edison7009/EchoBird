// Chat-Completions stream → Responses SSE

function chatStreamToResponsesStream(upstreamRes, clientRes, requestMessages = [], sessions, logger) {
    const warn = logger?.warn || (() => {});
    const responseId = sessions.newResponseId();

    // SSE flush: write headers immediately and disable Nagle so each
    // event hits the wire before the next read tick. Without these,
    // small deltas sit in the kernel send buffer up to ~40ms which
    // makes Codex's progress indicator stutter and (on Linux) causes
    // the TUI to give up on long responses.
    clientRes.flushHeaders();
    if (clientRes.socket) clientRes.socket.setNoDelay(true);

    const sendSSE = (event, data) => {
        clientRes.write(`event: ${event}\ndata: ${JSON.stringify(data)}\n\n`);
    };

    sendSSE("response.created", {
        type: "response.created",
        response: { id: responseId, object: "response", status: "in_progress", output: [] },
    });
    sendSSE("response.in_progress", {
        type: "response.in_progress",
        response: { id: responseId, object: "response", status: "in_progress" },
    });

    let textOpen = false;
    let textIdx = -1;
    let textBuf = "";
    let reasoningBuf = "";  // Accumulate `delta.reasoning_content` from thinking models.
    const toolCalls = new Map();   // chat-delta index → {id, name, arguments, output_index}
    let nextOutputIndex = 0;
    let buffer = "";
    let finished = false;

    const openTextItem = () => {
        textIdx = nextOutputIndex++;
        sendSSE("response.output_item.added", {
            type: "response.output_item.added",
            output_index: textIdx,
            item: { id: `item_${responseId}_${textIdx}`, type: "message", role: "assistant", content: [] },
        });
        sendSSE("response.content_part.added", {
            type: "response.content_part.added",
            output_index: textIdx, content_index: 0,
            part: { type: "output_text", text: "" },
        });
        textOpen = true;
        textBuf = "";
    };

    const closeTextItem = () => {
        if (!textOpen) return;
        sendSSE("response.output_text.done", {
            type: "response.output_text.done",
            output_index: textIdx, content_index: 0, text: textBuf,
        });
        sendSSE("response.content_part.done", {
            type: "response.content_part.done",
            output_index: textIdx, content_index: 0,
            part: { type: "output_text", text: textBuf },
        });
        sendSSE("response.output_item.done", {
            type: "response.output_item.done",
            output_index: textIdx,
            item: {
                id: `item_${responseId}_${textIdx}`, type: "message", role: "assistant",
                content: [{ type: "output_text", text: textBuf }],
            },
        });
        textOpen = false;
    };

    const openToolCall = (idx, tc) => {
        const outputIndex = nextOutputIndex++;
        const callId = tc.id || `call_${Math.random().toString(36).slice(2, 12)}`;
        const slot = {
            id: callId,
            name: tc.function?.name || "",
            arguments: "",
            output_index: outputIndex,
        };
        toolCalls.set(idx, slot);
        sendSSE("response.output_item.added", {
            type: "response.output_item.added",
            output_index: outputIndex,
            item: { id: callId, type: "function_call", call_id: callId, name: slot.name, arguments: "" },
        });
        return slot;
    };

    const closeToolCalls = () => {
        for (const slot of toolCalls.values()) {
            sendSSE("response.function_call_arguments.done", {
                type: "response.function_call_arguments.done",
                output_index: slot.output_index,
                item_id: slot.id,
                arguments: slot.arguments,
            });
            sendSSE("response.output_item.done", {
                type: "response.output_item.done",
                output_index: slot.output_index,
                item: {
                    id: slot.id, type: "function_call", call_id: slot.id,
                    name: slot.name, arguments: slot.arguments,
                },
            });
        }
    };

    const finish = () => {
        if (finished) return;
        finished = true;
        closeTextItem();
        closeToolCalls();
        sendSSE("response.completed", {
            type: "response.completed",
            response: { id: responseId, object: "response", status: "completed", output: [] },
        });
        if (!clientRes.writableEnded) clientRes.end();

        // Persist reasoning_content + assembled history so the next
        // /v1/responses request from Codex (which may replay this turn
        // via input items or previous_response_id) can recover them.
        try {
            const assistantMsg = {
                role: "assistant",
                content: toolCalls.size > 0 ? null : textBuf,
            };
            if (toolCalls.size > 0) {
                assistantMsg.tool_calls = [...toolCalls.values()].map(s => ({
                    id: s.id, type: "function",
                    function: { name: s.name, arguments: s.arguments },
                }));
            }
            if (reasoningBuf) {
                assistantMsg.reasoning_content = reasoningBuf;
                // Store reasoning under every tool_call id so any of them
                // resolves in the next turn's lookup.
                for (const s of toolCalls.values()) {
                    sessions.storeReasoning(s.id, reasoningBuf);
                }
                // And under a content-fingerprint key, so plain assistant
                // turns (no tool_calls) also round-trip.
                if (textBuf) sessions.storeTurnReasoning(textBuf, reasoningBuf);
            }
            sessions.saveHistory(responseId, [...requestMessages, assistantMsg]);
        } catch (e) {
            warn(`session store update failed: ${e.message}`);
        }
    };

    upstreamRes.on("data", (chunk) => {
        buffer += chunk.toString();
        const lines = buffer.split("\n");
        buffer = lines.pop() || "";
        for (const line of lines) {
            if (!line.startsWith("data: ")) continue;
            const data = line.slice(6).trim();
            if (!data) continue;
            if (data === "[DONE]") { finish(); return; }
            let parsed;
            try { parsed = JSON.parse(data); } catch { continue; }

            const delta = parsed.choices?.[0]?.delta;
            if (!delta) continue;

            // reasoning_content delta — DeepSeek-V4-* / Kimi-K2.6 / etc.
            // emit these alongside the regular content stream. We
            // accumulate them but DON'T forward to Codex (Codex's
            // Responses API has its own reasoning summary event family
            // we don't synthesize yet — the round-trip via session
            // store is what matters for context preservation).
            if (typeof delta.reasoning_content === "string") {
                reasoningBuf += delta.reasoning_content;
            }

            // Text delta
            if (delta.content) {
                if (!textOpen) openTextItem();
                textBuf += delta.content;
                sendSSE("response.output_text.delta", {
                    type: "response.output_text.delta",
                    output_index: textIdx, content_index: 0,
                    delta: delta.content,
                });
            }

            // Tool-call deltas. Chat splits arguments into multiple delta
            // chunks; we forward each one as a Responses arguments delta.
            if (Array.isArray(delta.tool_calls)) {
                if (textOpen) closeTextItem();
                for (const tc of delta.tool_calls) {
                    const idx = tc.index ?? 0;
                    let slot = toolCalls.get(idx) || openToolCall(idx, tc);
                    if (tc.id && slot.id.startsWith("call_") && tc.id !== slot.id) slot.id = tc.id;
                    if (tc.function?.name && !slot.name) slot.name = tc.function.name;
                    if (tc.function?.arguments) {
                        slot.arguments += tc.function.arguments;
                        sendSSE("response.function_call_arguments.delta", {
                            type: "response.function_call_arguments.delta",
                            output_index: slot.output_index,
                            item_id: slot.id,
                            delta: tc.function.arguments,
                        });
                    }
                }
            }
        }
    });

    upstreamRes.on("end", finish);
    upstreamRes.on("error", (e) => {
        const err = logger?.err || (() => {});
        err(`[Proxy] Upstream stream error: ${e.message}`);
        finish();
    });
}

function chatToResponsesNonStream(chatResponse, requestMessages = [], sessions, logger) {
    const warn = logger?.warn || (() => {});
    const responseId = sessions.newResponseId();
    const msg = chatResponse.choices?.[0]?.message || {};
    const output = [];
    if (msg.content) {
        output.push({
            id: `item_${responseId}_0`, type: "message", role: "assistant",
            content: [{ type: "output_text", text: msg.content }],
        });
    }
    if (Array.isArray(msg.tool_calls)) {
        for (const tc of msg.tool_calls) {
            output.push({
                id: tc.id, type: "function_call", call_id: tc.id,
                name: tc.function?.name || "",
                arguments: tc.function?.arguments || "",
            });
        }
    }

    // Persist reasoning + history (same shape as the streaming path).
    try {
        const assistantMsg = {
            role: "assistant",
            content: msg.tool_calls?.length ? null : (msg.content || ""),
        };
        if (msg.tool_calls?.length) {
            assistantMsg.tool_calls = msg.tool_calls.map(tc => ({
                id: tc.id, type: "function",
                function: { name: tc.function?.name || "", arguments: tc.function?.arguments || "" },
            }));
        }
        if (msg.reasoning_content) {
            assistantMsg.reasoning_content = msg.reasoning_content;
            if (msg.tool_calls?.length) {
                for (const tc of msg.tool_calls) sessions.storeReasoning(tc.id, msg.reasoning_content);
            }
            if (msg.content) sessions.storeTurnReasoning(msg.content, msg.reasoning_content);
        }
        sessions.saveHistory(responseId, [...requestMessages, assistantMsg]);
    } catch (e) {
        warn(`session store update failed: ${e.message}`);
    }

    return { id: responseId, object: "response", status: "completed", output };
}

module.exports = { chatStreamToResponsesStream, chatToResponsesNonStream };
