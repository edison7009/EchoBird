#!/usr/bin/env node
// Codex Launcher — Dual-spoofing proxy that bridges Codex's Responses API
// to third-party Chat-only endpoints.
//
// Codex v0.107+ only emits POST /v1/responses; DeepSeek / Moonshot / Qwen /
// OpenRouter / etc. only accept POST /v1/chat/completions. There is no
// config-file path that bridges this gap, so we run an http server on a
// random 127.0.0.1 port and rewrite ~/.codex/config.toml to point Codex at
// it. The proxy:
//   • translates Responses → Chat Completions outbound
//   • translates Chat-Completions stream → Responses-shaped SSE inbound
//   • restores the original base_url in config.toml on exit
//
// Restored 2026-05-11. The bug that killed v4.0.2 — second tool-call turn
// returning 400 "请求失败,请重试" — was caused by `responsesToChat`
// silently dropping `function_call` and `function_call_output` items, which
// emptied the messages array on every follow-up turn. That is now fixed,
// along with SSE flush (setNoDelay + flushHeaders) so deltas reach Codex
// immediately instead of being held by Nagle's algorithm.

const http = require("http");
const https = require("https");
const fs = require("fs");
const path = require("path");
const os = require("os");
const { spawn } = require("child_process");

// Paths are derived from $HOME by default. ECHOBIRD_CODEX_CONFIG_DIR /
// ECHOBIRD_RELAY_DIR overrides exist for smoke tests so we don't touch
// the user's real Codex config when running scripts/codex-launcher-smoke-test.cjs.
const RELAY_DIR = process.env.ECHOBIRD_RELAY_DIR || path.join(os.homedir(), ".echobird");
const CODEX_DIR = process.env.ECHOBIRD_CODEX_CONFIG_DIR || path.join(os.homedir(), ".codex");
const ECHOBIRD_CONFIG = path.join(RELAY_DIR, "codex.json");
const CODEX_CONFIG = path.join(CODEX_DIR, "config.toml");
const LAUNCHER_LOG = path.join(RELAY_DIR, "codex-launcher.log");

// Production launches run inside a hidden cmd window on Windows, so
// console.log output is invisible. Mirror everything into a persistent
// log so we can ask the user to "cat ~/.echobird/codex-launcher.log"
// when something goes wrong. The launcher runs briefly per session,
// so an append-only file with timestamps stays useful even after
// the next launch.
function logLine(level, msg) {
    const ts = new Date().toISOString();
    const line = `${ts} [${level}] ${msg}\n`;
    try {
        fs.mkdirSync(path.dirname(LAUNCHER_LOG), { recursive: true });
        fs.appendFileSync(LAUNCHER_LOG, line, "utf-8");
    } catch { /* log-of-the-log is pointless */ }
    if (level === "ERROR") console.error(`[Echobird] ${msg}`);
    else if (level === "WARN") console.warn(`[Echobird] ${msg}`);
    else console.log(`[Echobird] ${msg}`);
}
const log  = (msg) => logLine("INFO",  msg);
const warn = (msg) => logLine("WARN",  msg);
const err  = (msg) => logLine("ERROR", msg);

function loadEchobirdConfig() {
    try { return JSON.parse(fs.readFileSync(ECHOBIRD_CONFIG, "utf-8")); }
    catch { return null; }
}

function isOpenAI(url) { return !!url && url.includes("api.openai.com"); }

// ─── Responses → Chat Completions ─────────────────────────────────────
//
// Responses API input is a heterogeneous array of items. Faithful
// translation of ALL item types is mandatory — dropping function_call /
// function_call_output produces an empty messages array on tool-call
// follow-up turns, which is what nuked v4.0.2.

// ─── Content translation (Responses parts ↔ Chat parts) ──────────────
//
// Responses API content is either a string OR an array of typed parts:
//   { type: "input_text",  text: "..." }       — user input text
//   { type: "text",        text: "..." }       — generic text
//   { type: "output_text", text: "..." }       — assistant history replay
//   { type: "input_image", image_url: "data:..." }  — image as URL/data URI
//   { type: "image_url",   image_url: "..." | {url:"..."} } — already chat shape
// Chat Completions accepts content as string OR an array of:
//   { type: "text",      text: "..." }
//   { type: "image_url", image_url: {url:"..."} }
// We collapse all-text parts to a plain string (less verbose, more providers
// accept it), otherwise emit the multimodal array form.

function mapContentPart(part) {
    const kind = part?.type;
    switch (kind) {
        case "input_text":
        case "text":
        case "output_text":
            return { type: "text", text: part.text || "" };
        case "input_image": {
            // Responses API: image_url is a plain string (often a data: URL).
            // Chat Completions wants it wrapped: { image_url: { url: "..." } }.
            const url = typeof part.image_url === "string" ? part.image_url : "";
            return { type: "image_url", image_url: { url } };
        }
        case "image_url": {
            // Either already chat-shape ({url:...} object) or flat string.
            const raw = part.image_url;
            const inner = raw && typeof raw === "object"
                ? raw
                : { url: typeof raw === "string" ? raw : "" };
            return { type: "image_url", image_url: inner };
        }
        default:
            // Unknown / future part type: pass through verbatim so providers
            // that accept it can use it, and we don't crash on schemas the
            // launcher hasn't been updated to know about.
            return part;
    }
}

function valueToChatContent(content) {
    if (content == null) return null;
    if (typeof content === "string") return content;
    if (!Array.isArray(content)) {
        // Object / number / etc — stringify defensively rather than drop it.
        try { return JSON.stringify(content); } catch { return String(content); }
    }

    // Pure text array → collapse to a single string (lower-friction shape
    // for providers that don't fully support multimodal content arrays).
    // output_text is treated like text because that's what Codex replays
    // for assistant history items.
    const hasNonText = content.some(p => {
        const k = p?.type;
        return k && k !== "input_text" && k !== "text" && k !== "output_text";
    });
    if (!hasNonText) {
        return content
            .map(p => (p && typeof p.text === "string") ? p.text : "")
            .join("");
    }
    return content.map(mapContentPart);
}

// ─── SessionStore (in-memory) ─────────────────────────────────────────
//
// Ported from MetaFARS/codex-relay's session.rs. Three maps, all
// process-local (the launcher lives only as long as the Codex session
// it spawned, so no need for persistence):
//
//   responseHistory  : response_id  → ChatMessage[]
//       Codex uses `previous_response_id` to continue a conversation;
//       we replay the stored messages so each Chat Completions call is
//       self-contained even when Codex doesn't redundantly send the
//       full input.
//
//   reasoning        : call_id      → reasoning_content (string)
//       For thinking models (DeepSeek-V4-*, Kimi-K2.6, etc.), the
//       upstream returns reasoning_content alongside tool_calls. We
//       save it keyed by the call_id so when Codex replays the same
//       function_call in a subsequent request, we can attach the
//       saved reasoning_content to the assistant message — providers
//       require this round-trip or the next turn degrades.
//
//   turnReasoning    : fingerprint(content) → reasoning_content
//       For pure-text assistant turns (no tool_calls), there's no
//       call_id to key on. We hash the assistant content and use that
//       as the lookup key, so Codex replaying the assistant turn as a
//       message item still recovers the reasoning_content.
//
// Hash uses Node's built-in `crypto` via a tiny stable string hash —
// no external deps. Collisions are not a correctness issue (just
// missed lookups), so a 64-bit FNV-1a is plenty.

const sessionStore = (() => {
    const responseHistory = new Map();
    const reasoning = new Map();
    const turnReasoning = new Map();

    // 64-bit FNV-1a, returned as hex string for Map keying.
    const fnv1a = (s) => {
        let h1 = 0xcbf29ce4 >>> 0;
        let h2 = 0x84222325 >>> 0;
        for (let i = 0; i < s.length; i++) {
            const c = s.charCodeAt(i);
            h1 ^= c & 0xff;
            h2 ^= (c >>> 8) & 0xff;
            // Multiply by FNV prime 0x100000001b3 (mod 2^64), split into halves
            h1 = Math.imul(h1, 0x1b3) >>> 0;
            h2 = Math.imul(h2, 0x1b3) >>> 0;
        }
        return h1.toString(16).padStart(8, "0") + h2.toString(16).padStart(8, "0");
    };

    const contentToString = (content) => {
        if (typeof content === "string") return content;
        if (Array.isArray(content)) {
            return content
                .map(p => (p && typeof p.text === "string") ? p.text : "")
                .join("");
        }
        return "";
    };

    return {
        storeReasoning(callId, text) {
            if (callId && text) reasoning.set(callId, text);
        },
        getReasoning(callId) {
            return reasoning.get(callId) || null;
        },
        storeTurnReasoning(assistantContent, text) {
            const key = contentToString(assistantContent);
            if (key && text) turnReasoning.set(fnv1a(key), text);
        },
        getTurnReasoning(assistantContent) {
            const key = contentToString(assistantContent);
            if (!key) return null;
            return turnReasoning.get(fnv1a(key)) || null;
        },
        saveHistory(responseId, messages) {
            if (responseId && Array.isArray(messages)) {
                responseHistory.set(responseId, messages);
            }
        },
        getHistory(responseId) {
            return responseHistory.get(responseId) || [];
        },
        newResponseId() {
            return "resp_" + Math.random().toString(36).slice(2, 14);
        },
    };
})();

function responsesToChat(body, sessions = sessionStore) {
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

// ─── Chat-Completions stream → Responses SSE ──────────────────────────

function genResponseId() {
    return "resp_" + Math.random().toString(36).slice(2, 14);
}

function chatStreamToResponsesStream(upstreamRes, clientRes, requestMessages = [], sessions = sessionStore) {
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
        err(`[Proxy] Upstream stream error: ${e.message}`);
        finish();
    });
}

function chatToResponsesNonStream(chatResponse, requestMessages = [], sessions = sessionStore) {
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

// ─── Proxy server ─────────────────────────────────────────────────────

function startProxy(realBaseUrl, apiKey) {
    return new Promise((resolve, reject) => {
        const server = http.createServer((req, clientRes) => {
            // Accept both /v1/responses and /responses — Codex's URL
            // construction varies by version (some strip /v1 from
            // base_url when applying wire_api=responses, some don't).
            // Either path lands here and gets translated.
            const path = req.url.split("?")[0];
            const isResponses = path === "/v1/responses" || path === "/responses";
            if (req.method !== "POST" || !isResponses) {
                clientRes.writeHead(404, { "Content-Type": "application/json" });
                clientRes.end(JSON.stringify({ error: `Only POST /(v1/)?responses is proxied, got ${req.method} ${path}` }));
                return;
            }
            let body = "";
            req.on("data", c => body += c);
            req.on("end", () => {
                let reqBody;
                try { reqBody = JSON.parse(body); }
                catch (e) {
                    clientRes.writeHead(400);
                    clientRes.end(JSON.stringify({ error: e.message }));
                    return;
                }
                const chatBody = responsesToChat(reqBody);
                const isStream = chatBody.stream;

                // Normalize upstream URL. Users sometimes enter the bare
                // host (`https://api.deepseek.com`) without `/v1`; we
                // auto-add it so the forward lands on the standard
                // OpenAI-compat endpoint. If they DID include `/v1` (or
                // any `/v<n>`), we leave it alone.
                let baseClean = realBaseUrl.replace(/\/$/, "");
                if (!/\/v\d+$/.test(baseClean)) baseClean += "/v1";
                const upstream = new URL(baseClean + "/chat/completions");
                const transport = upstream.protocol === "https:" ? https : http;
                const upstreamReq = transport.request({
                    hostname: upstream.hostname,
                    port: upstream.port || (upstream.protocol === "https:" ? 443 : 80),
                    path: upstream.pathname + upstream.search,
                    method: "POST",
                    headers: {
                        "Content-Type": "application/json",
                        "Authorization": `Bearer ${apiKey}`,
                        "Accept": isStream ? "text/event-stream" : "application/json",
                    },
                }, (upstreamRes) => {
                    if (upstreamRes.statusCode !== 200) {
                        let errBody = "";
                        upstreamRes.on("data", c => errBody += c);
                        upstreamRes.on("end", () => {
                            err(`[Proxy] Upstream ${upstreamRes.statusCode}: ${errBody.slice(0, 500)}`);
                            clientRes.writeHead(upstreamRes.statusCode, { "Content-Type": "application/json" });
                            clientRes.end(errBody);
                        });
                        return;
                    }
                    // chatBody.messages = the EXACT history we just sent
                    // upstream — pass it through so the converters can save
                    // [...requestMessages, assistantTurn] under the new
                    // response_id for future previous_response_id lookups.
                    const requestMessages = chatBody.messages || [];
                    if (isStream) {
                        clientRes.writeHead(200, {
                            "Content-Type": "text/event-stream",
                            "Cache-Control": "no-cache",
                            "Connection": "keep-alive",
                            "X-Accel-Buffering": "no",
                        });
                        chatStreamToResponsesStream(upstreamRes, clientRes, requestMessages);
                    } else {
                        let resBody = "";
                        upstreamRes.on("data", c => resBody += c);
                        upstreamRes.on("end", () => {
                            try {
                                const res = chatToResponsesNonStream(JSON.parse(resBody), requestMessages);
                                clientRes.writeHead(200, { "Content-Type": "application/json" });
                                clientRes.end(JSON.stringify(res));
                            } catch (e) {
                                clientRes.writeHead(500);
                                clientRes.end(JSON.stringify({ error: e.message }));
                            }
                        });
                    }
                });
                upstreamReq.on("error", e => {
                    err(`[Proxy] Upstream connect error: ${e.message}`);
                    clientRes.writeHead(502);
                    clientRes.end(JSON.stringify({ error: e.message }));
                });
                upstreamReq.write(JSON.stringify(chatBody));
                upstreamReq.end();
            });
        });
        server.on("error", reject);
        server.listen(0, "127.0.0.1", () => {
            const port = server.address().port;
            log(`Proxy listening on 127.0.0.1:${port}`);
            resolve({ port, server });
        });
    });
}

// ─── config.toml base_url rewrite ─────────────────────────────────────
//
// apply_codex writes the third-party URL inside a
// [model_providers.<provider_id>] section. We try three tiers from
// precise to blunt so a config that doesn't look exactly the way
// apply_codex wrote it still gets rewritten correctly:
//
//   1. Section-scoped: find [model_providers.<provider_id>] and
//      rewrite its base_url. Most precise — the case apply_codex
//      produces.
//   2. Host-scoped: find any base_url whose value matches the host
//      we know is the third-party endpoint (from relay JSON).
//      Survives user-edited TOML where section names don't match.
//   3. First-occurrence: replace the first base_url in the file
//      (the v4.0.2 approach). Last-resort for unusual layouts.
//
// Tier 1's regex uses [\s\S]*? rather than [^[]*? so it handles
// inline arrays / tables inside the section body. Bounded by the
// next `\n[` header to avoid leaking into the next section.

function escapeRegex(s) {
    return s.replace(/[.[\]\\^$*+?()|{}]/g, "\\$&");
}

function rewriteBaseUrl(providerId, currentBaseUrlHint, newUrl) {
    let toml;
    try {
        toml = fs.readFileSync(CODEX_CONFIG, "utf-8");
    } catch (e) {
        err(`Cannot read config.toml: ${e.message}`);
        return { ok: false, tier: null };
    }

    const apply = (regex, label) => {
        const replaced = toml.replace(regex, (_m, prefix) => `${prefix}"${newUrl}"`);
        if (replaced === toml) return null;
        try {
            fs.writeFileSync(CODEX_CONFIG, replaced, "utf-8");
            log(`base_url rewritten via ${label} → ${newUrl}`);
            return label;
        } catch (e) {
            err(`config.toml write failed: ${e.message}`);
            return null;
        }
    };

    // Tier 1: section-scoped with the full TOML section name.
    if (providerId) {
        const fullSection = `model_providers.${providerId}`;
        const escaped = escapeRegex(fullSection);
        // Match [section] then non-greedy body until next [header (or EOF)
        // — capture the base_url = " prefix so we can replace just the value.
        const re = new RegExp(
            `(\\[${escaped}\\][\\s\\S]*?\\bbase_url\\s*=\\s*)"[^"]*"`,
            "m"
        );
        const hit = apply(re, `[${fullSection}]`);
        if (hit) return { ok: true, tier: hit };
    }

    // Tier 2: match by host. If we know the third-party endpoint host,
    // rewrite any base_url whose URL contains that host.
    if (currentBaseUrlHint) {
        try {
            const hintHost = new URL(currentBaseUrlHint).hostname;
            if (hintHost) {
                const escapedHost = escapeRegex(hintHost);
                const re = new RegExp(
                    `(\\bbase_url\\s*=\\s*)"https?://${escapedHost}[^"]*"`,
                    "m"
                );
                const hit = apply(re, `host-match ${hintHost}`);
                if (hit) return { ok: true, tier: hit };
            }
        } catch { /* malformed hint URL — skip tier */ }
    }

    // Tier 3: replace the first base_url in the file. Matches v4.0.2's
    // approach. Blunt but reliable when there's only one provider.
    const re = /(\bbase_url\s*=\s*)"[^"]*"/m;
    const hit = apply(re, "first-base_url (fallback)");
    if (hit) return { ok: true, tier: hit };

    warn("No base_url assignment found in config.toml — rewrite skipped");
    return { ok: false, tier: null };
}

// ─── Binary resolution ────────────────────────────────────────────────
//
// CLI mode: Codex v0.107+ ships as a Rust binary inside
// @openai/codex-<platform>. Direct-spawning preserves the TTY chain;
// going through codex.cmd → node codex.js → codex.exe with shell:true
// drops TTY-ness inside the cmd /d /s /c wrapper and the Rust TUI aborts
// with "stdin is not a terminal".
//
// Desktop mode: looks for the standalone Codex app (.exe on Windows,
// .app on macOS). The desktop installer is independent of npm, so we
// search the well-known install locations from tools/codexdesktop/paths.json.

function resolveDesktopBinary() {
    const platform = process.platform;
    const candidates = [];
    if (platform === "win32") {
        const localAppData = process.env.LOCALAPPDATA;
        if (localAppData) {
            // 1. Standalone installer (default location).
            candidates.push(path.join(localAppData, "Programs", "Codex", "Codex.exe"));
            // 2. Microsoft Store install — Windows 10+ exposes an
            //    executable alias here that resolves to the Store
            //    package, so we can spawn it like a normal exe.
            candidates.push(path.join(localAppData, "Microsoft", "WindowsApps", "Codex.exe"));
        }
        // 3. PATH lookup as a last resort.
        try {
            const { execFileSync } = require("child_process");
            // Silence stderr: `where` writes localized "not found" messages
            // to stderr in the system's ANSI codepage (e.g. GBK on
            // zh-CN), which then bleeds into our launcher console as
            // mojibake. We only care about stdout for the resolved path.
            const found = execFileSync("where", ["Codex.exe"], {
                encoding: "utf-8", timeout: 3000,
                stdio: ["ignore", "pipe", "ignore"],
            }).trim().split(/\r?\n/)[0].trim();
            if (found) candidates.push(found);
        } catch { /* not in PATH */ }
    } else if (platform === "darwin") {
        candidates.push("/Applications/Codex.app/Contents/MacOS/Codex");
        candidates.push(path.join(os.homedir(), "Applications", "Codex.app", "Contents", "MacOS", "Codex"));
    }
    // Codex Desktop has no Linux build as of 2026-05.
    for (const c of candidates) {
        if (fs.existsSync(c)) return c;
    }
    return null;
}

// Read tools/codexdesktop/paths.json to get the Windows shell:AppsFolder
// URI. We use this only as a fallback when resolveDesktopBinary fails —
// the URI launch is fire-and-forget (no child process to track), so we
// have to poll for the Codex process to know when to tear down.
function resolveDesktopLaunchUri() {
    if (process.platform !== "win32") return null;
    try {
        const desktopPathsJson = path.join(__dirname, "..", "codexdesktop", "paths.json");
        if (!fs.existsSync(desktopPathsJson)) return null;
        const cfg = JSON.parse(fs.readFileSync(desktopPathsJson, "utf-8"));
        return typeof cfg.launchUri === "string" ? cfg.launchUri : null;
    } catch { return null; }
}

// Block until either: Codex.exe appears and then disappears (normal exit),
// or we've waited the full deadline without ever seeing it. Used for the
// launchUri path where we don't own a child process.
async function waitForCodexProcessLifecycle() {
    const isRunning = () => {
        try {
            const { execFileSync } = require("child_process");
            const out = execFileSync("tasklist", ["/FI", "IMAGENAME eq Codex.exe", "/FO", "CSV", "/NH"],
                { encoding: "utf-8", timeout: 3000, stdio: ["ignore", "pipe", "ignore"] });
            return out.toLowerCase().includes("codex.exe");
        } catch { return false; }
    };
    const sleep = ms => new Promise(r => setTimeout(r, ms));

    // Phase 1: wait up to 30s for Codex.exe to appear.
    const startupDeadline = Date.now() + 30_000;
    while (Date.now() < startupDeadline) {
        if (isRunning()) break;
        await sleep(1000);
    }
    if (!isRunning()) {
        warn("Codex Desktop process never appeared after 30s; tearing down proxy anyway.");
        return;
    }
    log("Codex Desktop process detected; watching for exit.");

    // Phase 2: poll until codex.exe disappears for 2 consecutive checks.
    let absent = 0;
    while (absent < 2) {
        await sleep(3000);
        if (isRunning()) absent = 0;
        else absent++;
    }
}

function resolveCodexBinary() {
    const platform = process.platform;
    const arch = process.arch;
    let platPkg, triple, exeName;
    if (platform === "win32") {
        if (arch === "arm64") { platPkg = "@openai/codex-win32-arm64"; triple = "aarch64-pc-windows-msvc"; }
        else                  { platPkg = "@openai/codex-win32-x64";   triple = "x86_64-pc-windows-msvc"; }
        exeName = "codex.exe";
    } else if (platform === "darwin") {
        if (arch === "arm64") { platPkg = "@openai/codex-darwin-arm64"; triple = "aarch64-apple-darwin"; }
        else                  { platPkg = "@openai/codex-darwin-x64";   triple = "x86_64-apple-darwin"; }
        exeName = "codex";
    } else if (platform === "linux") {
        if (arch === "arm64") { platPkg = "@openai/codex-linux-arm64"; triple = "aarch64-unknown-linux-musl"; }
        else                  { platPkg = "@openai/codex-linux-x64";   triple = "x86_64-unknown-linux-musl"; }
        exeName = "codex";
    } else return null;

    const codexPkgRoots = [];
    try {
        const { execFileSync } = require("child_process");
        const findCmd = platform === "win32" ? "where" : "which";
        const findArg = platform === "win32" ? "codex.cmd" : "codex";
        const stub = execFileSync(findCmd, [findArg], {
            encoding: "utf8", timeout: 3000,
            stdio: ["ignore", "pipe", "ignore"],
        }).trim().split(/\r?\n/)[0].trim();
        if (stub) {
            const npmDir = path.dirname(stub);
            codexPkgRoots.push(path.join(npmDir, "node_modules", "@openai", "codex"));
            codexPkgRoots.push(path.join(path.dirname(npmDir), "lib", "node_modules", "@openai", "codex"));
        }
    } catch { /* fall through */ }

    if (platform === "win32") {
        const appdata = process.env.APPDATA || process.env.LOCALAPPDATA;
        if (appdata && appdata.length > 2) {
            codexPkgRoots.push(path.join(appdata, "npm", "node_modules", "@openai", "codex"));
        }
    } else {
        codexPkgRoots.push("/usr/local/lib/node_modules/@openai/codex");
        codexPkgRoots.push("/usr/lib/node_modules/@openai/codex");
        codexPkgRoots.push(path.join(os.homedir(), ".npm-global", "lib", "node_modules", "@openai", "codex"));
    }

    for (const pkgRoot of codexPkgRoots) {
        const candidate = path.join(pkgRoot, "node_modules", platPkg, "vendor", triple, "codex", exeName);
        if (fs.existsSync(candidate)) return candidate;
    }
    return null;
}

// Resolve the right binary based on launch mode, then spawn it. Both
// CLI and Desktop go through the same "wait for child to exit" path so
// the proxy lifetime matches the Codex session — when the user closes
// Codex, the launcher tears down the proxy and restores config.toml.
function launchCodex(mode, onExit) {
    let codexPath;
    let useShell = false;
    let stdio = "inherit"; // CLI needs an attached TTY; Desktop doesn't care.
    let desktopViaUri = false;

    if (mode === "desktop") {
        codexPath = resolveDesktopBinary();
        if (!codexPath) {
            // Direct exe not found — try the Store launchUri. Common on
            // Microsoft Store installs whose binary lives under a locked
            // WindowsApps directory we can't always resolve.
            const uri = resolveDesktopLaunchUri();
            if (uri) {
                log(`Direct Codex.exe not found; launching via Store URI: ${uri}`);
                desktopViaUri = true;
                codexPath = "cmd";
                useShell = false;
            } else {
                err("Codex Desktop not found in standard install locations.");
                err("Install Codex Desktop from https://openai.com/codex or the Microsoft Store first.");
                process.exit(1);
            }
        } else {
            log(`Launching Codex Desktop: ${codexPath}`);
        }
        // Desktop is a GUI app: detach stdio so the launcher doesn't keep
        // a console window pinned to it.
        stdio = "ignore";
    } else {
        codexPath = resolveCodexBinary();
        if (codexPath) {
            log(`Launching Codex CLI (direct binary): ${codexPath}`);
        } else {
            const codexCmd = process.platform === "win32" ? "codex.cmd" : "codex";
            if (process.platform === "win32") {
                const appdata = process.env.APPDATA || process.env.LOCALAPPDATA || "";
                if (appdata.length > 2) {
                    const candidate = path.join(appdata, "npm", codexCmd);
                    if (fs.existsSync(candidate)) codexPath = candidate;
                }
            } else {
                const candidate = "/usr/local/bin/" + codexCmd;
                if (fs.existsSync(candidate)) codexPath = candidate;
            }
            if (!codexPath) {
                try {
                    const { execFileSync } = require("child_process");
                    const findCmd = process.platform === "win32" ? "where" : "which";
                    const r = execFileSync(findCmd, [codexCmd], {
                        encoding: "utf8", timeout: 3000,
                        stdio: ["ignore", "pipe", "ignore"],
                    }).trim().split(/\r?\n/)[0].trim();
                    if (r && fs.existsSync(r)) codexPath = r;
                } catch { /* not found */ }
            }
            if (!codexPath) codexPath = codexCmd;
            useShell = true;
            log(`Rust binary not found, falling back to shim: ${codexPath}`);
        }
    }

    if (desktopViaUri) {
        // Fire-and-forget via cmd.exe: `start "" "shell:AppsFolder\..."`
        // hands the URI to the Shell which dispatches to the Store-app
        // activation pipeline. We don't get a child process back, so we
        // run a background poller that watches for Codex.exe to appear
        // and then disappear, then triggers onExit.
        const uri = resolveDesktopLaunchUri();
        try {
            const launcher = spawn("cmd", ["/C", "start", "", uri], {
                stdio: "ignore",
                env: process.env,
                cwd: os.homedir(),
                detached: true,
            });
            launcher.unref();
        } catch (e) {
            err(`Failed to invoke Store launch URI: ${e.message}`);
            process.exit(1);
        }
        waitForCodexProcessLifecycle().then(() => {
            if (onExit) onExit(0);
            else process.exit(0);
        });
        return;
    }

    const child = spawn(codexPath, [], {
        stdio,
        env: process.env,
        cwd: os.homedir(),
        shell: useShell,
    });
    process.on("SIGINT",  () => child.kill("SIGINT"));
    process.on("SIGTERM", () => child.kill("SIGTERM"));
    child.on("close", (code) => { if (onExit) onExit(code || 0); else process.exit(code || 0); });
    child.on("error", (e) => {
        err(`Failed to launch Codex: ${e.message}`);
        process.exit(1);
    });
}

// ─── Provider history sync (pre-launch) ──────────────────────────────
//
// Codex tags every conversation with the active `model_provider`. When the
// user switches providers via EchoBird's apply_codex, prior conversations
// stay tagged with the OLD provider and Codex Desktop / `/resume` hide them.
// The vendored codex-provider-sync CLI rewrites that metadata to the new
// provider so historical chats stay visible across switches.
//
// We run it HERE — in the launcher, BEFORE spawning Codex — because:
//   1. Codex isn't running yet, so it doesn't hold state_5.sqlite's WAL lock.
//      provider-sync's exclusive directory lock acquires cleanly.
//   2. The retag finishes before Codex starts reading session metadata.
//
// (apply_codex previously fired sync fire-and-forget too, but that path is
// racy because the user is often still using Codex when they apply a new
// model. The lock fails silently and the user sees no merged history. The
// launcher pre-step is the reliable path.)
//
// Bounded with a 10s timeout — sync is usually <2s; if it hangs we'd
// rather launch Codex with stale tags than make the user wait forever.

async function runProviderSync(providerId) {
    if (!providerId) return;
    // The vendored CLI lives as a SIBLING of this launcher (both under
    // tools/codex/). In dev mode Tauri mirrors tools/ to <target>/_up_/tools/
    // — but only at startup, so a freshly-vendored subdir may not be in
    // the mirror yet. Try several candidate paths so dev workflows that
    // skip a full restart still work:
    //   1. <launcher_dir>/codex-provider-sync (production / synced dev)
    //   2. <launcher_dir>/../../codex/codex-provider-sync (defensive)
    //   3. ECHOBIRD_PROVIDER_SYNC_CLI env override (escape hatch)
    const candidates = [
        process.env.ECHOBIRD_PROVIDER_SYNC_CLI,
        path.join(__dirname, "codex-provider-sync", "src", "cli.js"),
        path.join(__dirname, "..", "codex", "codex-provider-sync", "src", "cli.js"),
    ].filter(Boolean);
    const cliJs = candidates.find(p => { try { return fs.existsSync(p); } catch { return false; } });
    if (!cliJs) {
        warn(`provider-sync CLI not found in any of: ${candidates.join(" | ")} — skipping history retag`);
        return;
    }
    log(`provider-sync: retag historical sessions to provider=${providerId}`);
    return new Promise((resolve) => {
        const child = spawn("node", [cliJs, "sync", "--provider", providerId, "--keep", "5"], {
            stdio: ["ignore", "pipe", "pipe"],
            env: process.env,
            windowsHide: true,
        });
        let outBuf = "";
        let errBuf = "";
        child.stdout.on("data", c => outBuf += c.toString());
        child.stderr.on("data", c => errBuf += c.toString());

        const timer = setTimeout(() => {
            if (!child.killed) {
                child.kill();
                warn(`provider-sync exceeded 10s timeout, killed`);
            }
        }, 10_000);

        child.on("close", (code) => {
            clearTimeout(timer);
            if (code === 0) {
                const tail = outBuf.trim().split("\n").slice(-5).join(" | ");
                log(`provider-sync OK: ${tail || "(no stdout)"}`);
            } else {
                warn(`provider-sync exited code=${code}; stderr=${errBuf.slice(0, 400).trim()}`);
            }
            resolve();
        });
        child.on("error", e => {
            clearTimeout(timer);
            warn(`provider-sync spawn error: ${e.message}`);
            resolve();
        });
    });
}

// ─── Main ─────────────────────────────────────────────────────────────

async function main() {
    // ECHOBIRD_CODEX_LAUNCH_MODE is set by start_codex_launcher in
    // process_manager.rs. "cli" (default) or "desktop". Restore-to-official
    // doesn't need any special handling here — when the user resets via UI,
    // ~/.echobird/codex.json is deleted, loadEchobirdConfig() returns null,
    // and we fall straight through to a direct launch without a proxy.
    //
    // Provider-sync (history retag) is NOT run here — it moved back to
    // apply_codex in tool_config_manager.rs, which runs synchronously on
    // every model switch regardless of how the user later starts Codex
    // (our "Open" button, desktop shortcut, Start menu, etc.). Running
    // it here too would double the work.
    const mode = (process.env.ECHOBIRD_CODEX_LAUNCH_MODE || "cli").toLowerCase();
    log(`──── launcher start, mode=${mode}, pid=${process.pid} ────`);

    // For desktop mode we need EITHER a direct Codex.exe / Codex.app path
    // (preferred — child process tracking works) OR a Store launchUri
    // (fallback — fire-and-forget + tasklist polling). Only abort if
    // neither is available, which means Codex Desktop simply isn't
    // installed.
    if (mode === "desktop" && !resolveDesktopBinary() && !resolveDesktopLaunchUri()) {
        err("Codex Desktop not installed (no Codex.exe at the standard");
        err("paths and no Store launchUri available either).");
        err("Install Codex Desktop from https://openai.com/codex or the Microsoft Store.");
        process.exit(1);
    }

    const config = loadEchobirdConfig();
    if (!config) {
        log(`No relay config at ${ECHOBIRD_CONFIG} — launching Codex ${mode} directly`);
        launchCodex(mode);
        return;
    }

    const { apiKey, baseUrl, modelId, providerId } = config;
    const envKey = config.envKey || "OPENAI_API_KEY";
    log(`relay: baseUrl=${baseUrl} model=${modelId || "(default)"} provider=${providerId || "(none)"} envKey=${envKey}`);

    // Retag historical sessions to the active provider BEFORE Codex starts —
    // this is what actually makes "switch model and still see old chats" work.
    // Awaited so the retag finishes before Codex opens state_5.sqlite.
    await runProviderSync(providerId);

    if (isOpenAI(baseUrl)) {
        log(`OpenAI endpoint detected — no proxy needed (${mode})`);
        if (apiKey) process.env[envKey] = apiKey;
        launchCodex(mode);
        return;
    }

    log(`${mode} mode, third-party endpoint: ${baseUrl}`);

    const { port, server } = await startProxy(baseUrl, apiKey);
    const localUrl = `http://127.0.0.1:${port}/v1`;

    const rewriteResult = rewriteBaseUrl(providerId, baseUrl, localUrl);
    if (!rewriteResult.ok) {
        err("config.toml base_url was NOT rewritten — Codex will bypass the proxy and hit the upstream directly.");
        err(`Check ${CODEX_CONFIG} — expected to find a base_url line we could replace.`);
    }

    if (apiKey) process.env[envKey] = apiKey;

    launchCodex(mode, (code) => {
        if (rewriteResult.ok) rewriteBaseUrl(providerId, localUrl, baseUrl);
        server.close();
        process.exit(code);
    });
}

// Run main() when invoked as a script; export translation helpers so
// tests can exercise them in isolation without spawning Codex.
if (require.main === module) {
    main().catch(e => { err(`Fatal: ${e.stack || e}`); process.exit(1); });
} else {
    module.exports = {
        responsesToChat,
        chatToResponsesNonStream,
        startProxy,
        rewriteBaseUrl,
        valueToChatContent,
        mapContentPart,
        sessionStore,
        CODEX_CONFIG,
        ECHOBIRD_CONFIG,
    };
}
