// Chat-Completions stream → Responses SSE

// Translate Chat-Completions usage shape → Responses API usage shape.
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
function chatUsageToResponsesUsage(chatUsage) {
    const u = chatUsage || {};

    const input = u.input_tokens ?? u.prompt_tokens ?? 0;
    const output = u.output_tokens ?? u.completion_tokens ?? 0;
    const total = u.total_tokens ?? (input + output);

    // Cached input tokens — newer providers nest under prompt_tokens_details.
    const cachedTokens =
        u.input_tokens_details?.cached_tokens ??
        u.prompt_tokens_details?.cached_tokens ??
        u.cached_tokens ??
        0;

    // Reasoning output tokens — for thinking models. Some providers nest under
    // completion_tokens_details, some emit a flat reasoning_tokens.
    const reasoningTokens =
        u.output_tokens_details?.reasoning_tokens ??
        u.completion_tokens_details?.reasoning_tokens ??
        u.reasoning_tokens ??
        0;

    return {
        input_tokens: input,
        input_tokens_details: { cached_tokens: cachedTokens },
        output_tokens: output,
        output_tokens_details: { reasoning_tokens: reasoningTokens },
        total_tokens: total,
    };
}

function chatStreamToResponsesStream(upstreamRes, clientRes, requestMessages = [], sessions, logger, clientModel) {
    const warn = logger?.warn || (() => {});
    const responseId = sessions.newResponseId();
    // Mirror back whatever model id Codex asked for. The real upstream
    // model id stays buried in the proxy-to-upstream leg.
    const stampModel = (resp) => {
        if (clientModel) resp.model = clientModel;
        return resp;
    };

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
        response: stampModel({ id: responseId, object: "response", status: "in_progress", output: [] }),
    });
    sendSSE("response.in_progress", {
        type: "response.in_progress",
        response: stampModel({ id: responseId, object: "response", status: "in_progress" }),
    });

    let textOpen = false;
    let textIdx = -1;
    let textBuf = "";
    let reasoningBuf = "";  // Accumulate `delta.reasoning_content` from thinking models.
    const toolCalls = new Map();   // chat-delta index → {id, name, arguments, output_index}
    let nextOutputIndex = 0;
    let buffer = "";
    let finished = false;
    let usage = null;          // Aggregated token usage from final upstream delta.
    let finishReason = null;   // "stop" | "length" | "tool_calls" | "content_filter"

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

    // Build the final assembled output array (mirrors what Codex saw via
    // deltas). Used in response.completed so the response object is complete.
    const buildAssembledOutput = () => {
        const out = [];
        if (textBuf) {
            out.push({
                id: `item_${responseId}_${textIdx >= 0 ? textIdx : 0}`,
                type: "message",
                role: "assistant",
                content: [{ type: "output_text", text: textBuf }],
            });
        }
        for (const slot of toolCalls.values()) {
            out.push({
                id: slot.id, type: "function_call", call_id: slot.id,
                name: slot.name, arguments: slot.arguments,
            });
        }
        return out;
    };

    const finish = () => {
        if (finished) return;
        finished = true;
        closeTextItem();
        closeToolCalls();
        const completedResponse = stampModel({
            id: responseId, object: "response", status: "completed",
            output: buildAssembledOutput(),
        });
        completedResponse.usage = chatUsageToResponsesUsage(usage);
        if (finishReason) completedResponse.incomplete_details =
            finishReason === "length" ? { reason: "max_output_tokens" } : undefined;
        // "length" means the model hit max_tokens — surface that as incomplete
        // so Codex can show "(response truncated)" instead of silently cutting off.
        if (finishReason === "length") completedResponse.status = "incomplete";
        sendSSE(completedResponse.status === "incomplete" ? "response.incomplete" : "response.completed", {
            type: completedResponse.status === "incomplete" ? "response.incomplete" : "response.completed",
            response: completedResponse,
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

    // Send a response.failed SSE event then end the stream. Used when the
    // upstream connection drops mid-stream — without this, Codex would
    // receive response.completed with whatever partial output we got and
    // think the request succeeded.
    const fail = (message, code = "upstream_error") => {
        if (finished) return;
        finished = true;
        closeTextItem();
        closeToolCalls();
        sendSSE("response.failed", {
            type: "response.failed",
            response: {
                id: responseId, object: "response", status: "failed",
                error: { code, message },
                output: buildAssembledOutput(),
            },
        });
        if (!clientRes.writableEnded) clientRes.end();
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

            // Capture usage and finish_reason from any chunk that includes
            // them. Most providers emit usage on the final chunk only;
            // OpenAI emits it as a separate trailing event when
            // stream_options.include_usage=true is requested. We accept
            // both shapes here without forcing a particular caller config.
            if (parsed.usage) usage = parsed.usage;
            const fr = parsed.choices?.[0]?.finish_reason;
            if (fr) finishReason = fr;

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
        fail(`Upstream stream error: ${e.message}`, "upstream_stream_error");
    });
}

function chatToResponsesNonStream(chatResponse, requestMessages = [], sessions, logger, clientModel) {
    const warn = logger?.warn || (() => {});
    const responseId = sessions.newResponseId();
    const choice = chatResponse.choices?.[0] || {};
    const msg = choice.message || {};
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

    // "length" finish_reason → mark as incomplete so Codex can show the
    // response was truncated rather than treating it as a clean stop.
    const status = choice.finish_reason === "length" ? "incomplete" : "completed";
    const response = { id: responseId, object: "response", status, output };
    if (clientModel) response.model = clientModel;
    response.usage = chatUsageToResponsesUsage(chatResponse.usage);
    if (choice.finish_reason === "length") {
        response.incomplete_details = { reason: "max_output_tokens" };
    }
    return response;
}

// Translate an upstream /chat/completions error response (or transport-level
// error) into a /responses-shape error envelope that Codex can render. We
// pull out the upstream message text where possible so users see the
// underlying provider error verbatim (e.g. "Invalid API key", "Model not
// found") instead of a generic 502.
function chatErrorToResponsesError(statusCode, upstreamBody, sessions) {
    const responseId = sessions ? sessions.newResponseId() : `resp_err_${Date.now()}`;
    let message = `Upstream returned ${statusCode}`;
    let code = `upstream_${statusCode}`;
    if (typeof upstreamBody === "string" && upstreamBody.length > 0) {
        try {
            const parsed = JSON.parse(upstreamBody);
            // OpenAI/DeepSeek/etc. nest the error under .error.{message,code,type}.
            // Some providers return a flat .message or .detail at the top level.
            const errObj = parsed.error || parsed;
            if (typeof errObj.message === "string") message = errObj.message;
            else if (typeof errObj.detail === "string") message = errObj.detail;
            if (typeof errObj.code === "string") code = errObj.code;
            else if (typeof errObj.type === "string") code = errObj.type;
        } catch {
            // Body wasn't JSON — surface the raw text (truncated) so the
            // user still gets *something* instead of just the status code.
            message = upstreamBody.slice(0, 500);
        }
    }
    return {
        id: responseId, object: "response", status: "failed",
        error: { code, message },
        output: [],
    };
}

module.exports = {
    chatStreamToResponsesStream,
    chatToResponsesNonStream,
    chatErrorToResponsesError,
};
