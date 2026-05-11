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

const ECHOBIRD_CONFIG = path.join(os.homedir(), ".echobird", "codex.json");
const CODEX_CONFIG = path.join(os.homedir(), ".codex", "config.toml");

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

function extractText(content) {
    if (typeof content === "string") return content;
    if (!Array.isArray(content)) return "";
    return content
        .filter(c => c && (c.type === "input_text" || c.type === "text" || c.type === "output_text"))
        .map(c => c.text || "")
        .join("\n");
}

function responsesToChat(body) {
    const messages = [];
    if (body.instructions) {
        messages.push({ role: "system", content: body.instructions });
    }

    if (typeof body.input === "string") {
        messages.push({ role: "user", content: body.input });
    } else if (Array.isArray(body.input)) {
        for (const item of body.input) {
            switch (item.type) {
                case "message": {
                    let role = item.role || "user";
                    if (role === "developer") role = "system";
                    const text = extractText(item.content);
                    if (text) messages.push({ role, content: text });
                    break;
                }
                case "function_call": {
                    // Assistant tool invocation. Chat puts these on the
                    // assistant message's tool_calls[] array.
                    const callId = item.call_id || item.id
                        || `call_${Math.random().toString(36).slice(2, 12)}`;
                    messages.push({
                        role: "assistant",
                        content: null,
                        tool_calls: [{
                            id: callId,
                            type: "function",
                            function: {
                                name: item.name || "",
                                arguments: typeof item.arguments === "string"
                                    ? item.arguments
                                    : JSON.stringify(item.arguments || {}),
                            },
                        }],
                    });
                    break;
                }
                case "function_call_output": {
                    // Tool result. Chat encodes these as role=tool with
                    // tool_call_id matching the prior assistant call.
                    const callId = item.call_id || item.id || "";
                    const out = typeof item.output === "string"
                        ? item.output
                        : JSON.stringify(item.output ?? "");
                    messages.push({
                        role: "tool",
                        tool_call_id: callId,
                        content: out,
                    });
                    break;
                }
                default:
                    console.error(`[Proxy] Skipping unknown input item type: ${item.type}`);
            }
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
            if (msg.role === "system") {
                pendingSystem += (pendingSystem ? "\n" : "") + msg.content;
            } else {
                if (pendingSystem) {
                    if (msg.role === "user") {
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

    // Pass through tool definitions; without these the model cannot
    // call any tools and Codex stays in plain-chat mode forever.
    if (Array.isArray(body.tools) && body.tools.length > 0) {
        chatBody.tools = body.tools.map(t => {
            // Responses tool shape: {type:"function", name, description, parameters}
            // Chat tool shape:      {type:"function", function:{name, description, parameters}}
            if (t.type === "function" && t.function) return t;
            return {
                type: "function",
                function: {
                    name: t.name,
                    description: t.description,
                    parameters: t.parameters || { type: "object", properties: {} },
                },
            };
        });
        if (body.tool_choice) chatBody.tool_choice = body.tool_choice;
    }

    return chatBody;
}

// ─── Chat-Completions stream → Responses SSE ──────────────────────────

function genResponseId() {
    return "resp_" + Math.random().toString(36).slice(2, 14);
}

function chatStreamToResponsesStream(upstreamRes, clientRes) {
    const responseId = genResponseId();

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
    upstreamRes.on("error", (err) => {
        console.error("[Proxy] Upstream stream error:", err.message);
        finish();
    });
}

function chatToResponsesNonStream(chatResponse) {
    const responseId = genResponseId();
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
    return { id: responseId, object: "response", status: "completed", output };
}

// ─── Proxy server ─────────────────────────────────────────────────────

function startProxy(realBaseUrl, apiKey) {
    return new Promise((resolve, reject) => {
        const server = http.createServer((req, clientRes) => {
            if (req.method !== "POST" || !req.url.startsWith("/v1/responses")) {
                clientRes.writeHead(404, { "Content-Type": "application/json" });
                clientRes.end(JSON.stringify({ error: "Only POST /v1/responses is proxied" }));
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

                const upstream = new URL(realBaseUrl.replace(/\/$/, "") + "/chat/completions");
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
                            console.error(`[Proxy] Upstream ${upstreamRes.statusCode}: ${errBody.slice(0, 500)}`);
                            clientRes.writeHead(upstreamRes.statusCode, { "Content-Type": "application/json" });
                            clientRes.end(errBody);
                        });
                        return;
                    }
                    if (isStream) {
                        clientRes.writeHead(200, {
                            "Content-Type": "text/event-stream",
                            "Cache-Control": "no-cache",
                            "Connection": "keep-alive",
                            "X-Accel-Buffering": "no",
                        });
                        chatStreamToResponsesStream(upstreamRes, clientRes);
                    } else {
                        let resBody = "";
                        upstreamRes.on("data", c => resBody += c);
                        upstreamRes.on("end", () => {
                            try {
                                const res = chatToResponsesNonStream(JSON.parse(resBody));
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
                    console.error("[Proxy] Upstream connect error:", e.message);
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
            console.log(`[Echobird] Proxy started on 127.0.0.1:${port}`);
            resolve({ port, server });
        });
    });
}

// ─── config.toml base_url rewrite ─────────────────────────────────────
//
// apply_codex writes the third-party URL inside a [model_providers.X]
// section. Scope the regex to that section so we never accidentally
// rewrite some other provider the user added by hand.

function buildSectionRegex(section) {
    const escaped = section.replace(/[.[\]\\]/g, "\\$&");
    // Match [<section>] then anything up to the next [section] (or EOF),
    // capturing the base_url assignment inside it.
    return new RegExp(`(\\[${escaped}\\][^[]*?base_url\\s*=\\s*)"[^"]*"`, "m");
}

function rewriteSectionBaseUrl(section, newUrl) {
    try {
        let toml = fs.readFileSync(CODEX_CONFIG, "utf-8");
        const re = buildSectionRegex(section);
        const replaced = toml.replace(re, (_m, prefix) => `${prefix}"${newUrl}"`);
        if (replaced === toml) {
            console.warn(`[Echobird] base_url not found under [${section}] — config left unchanged`);
            return false;
        }
        fs.writeFileSync(CODEX_CONFIG, replaced, "utf-8");
        console.log(`[Echobird] [${section}].base_url → ${newUrl}`);
        return true;
    } catch (e) {
        console.error("[Echobird] config.toml rewrite failed:", e.message);
        return false;
    }
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
            candidates.push(path.join(localAppData, "Programs", "Codex", "Codex.exe"));
        }
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
        const stub = execFileSync(findCmd, [findArg], { encoding: "utf8", timeout: 3000 })
            .trim().split(/\r?\n/)[0].trim();
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

    if (mode === "desktop") {
        codexPath = resolveDesktopBinary();
        if (!codexPath) {
            console.error("[Echobird] Codex Desktop not found in standard install locations.");
            console.error("[Echobird] Install Codex Desktop from https://openai.com/codex first.");
            process.exit(1);
        }
        // Desktop is a GUI app: detach stdio so the launcher doesn't keep
        // a console window pinned to it. We still treat it as a child so
        // exit detection works on macOS / standalone-Windows installs.
        stdio = "ignore";
        console.log(`[Echobird] Launching Codex Desktop: ${codexPath}`);
    } else {
        codexPath = resolveCodexBinary();
        if (codexPath) {
            console.log(`[Echobird] Launching Codex CLI (direct binary): ${codexPath}`);
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
                    const r = execFileSync(findCmd, [codexCmd], { encoding: "utf8", timeout: 3000 })
                        .trim().split(/\r?\n/)[0].trim();
                    if (r && fs.existsSync(r)) codexPath = r;
                } catch { /* not found */ }
            }
            if (!codexPath) codexPath = codexCmd;
            useShell = true;
            console.log(`[Echobird] Rust binary not found, falling back to shim: ${codexPath}`);
        }
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
    child.on("error", (err) => {
        console.error(`[Echobird] Failed to launch Codex: ${err.message}`);
        process.exit(1);
    });
}

// ─── Main ─────────────────────────────────────────────────────────────

async function main() {
    // ECHOBIRD_CODEX_LAUNCH_MODE is set by start_codex_launcher in
    // process_manager.rs. "cli" (default) or "desktop". Restore-to-official
    // doesn't need any special handling here — when the user resets via UI,
    // ~/.echobird/codex.json is deleted, loadEchobirdConfig() returns null,
    // and we fall straight through to a direct launch without a proxy.
    const mode = (process.env.ECHOBIRD_CODEX_LAUNCH_MODE || "cli").toLowerCase();

    const config = loadEchobirdConfig();
    if (!config) {
        console.log(`[Echobird] No relay config — launching Codex ${mode} directly`);
        launchCodex(mode);
        return;
    }

    const { apiKey, baseUrl, modelId, providerId } = config;
    const envKey = config.envKey || "OPENAI_API_KEY";

    if (isOpenAI(baseUrl)) {
        console.log(`[Echobird] OpenAI endpoint detected — no proxy needed (${mode})`);
        if (apiKey) process.env[envKey] = apiKey;
        launchCodex(mode);
        return;
    }

    console.log(`[Echobird] ${mode} mode, third-party endpoint: ${baseUrl}`);
    console.log(`[Echobird] Model: ${modelId || "default"}  Provider section: ${providerId || "(none)"}`);

    const { port, server } = await startProxy(baseUrl, apiKey);
    const localUrl = `http://127.0.0.1:${port}/v1`;

    let rewrote = false;
    if (providerId) {
        rewrote = rewriteSectionBaseUrl(providerId, localUrl);
    } else {
        console.warn("[Echobird] No providerId in relay config — config.toml not rewritten");
    }

    if (apiKey) process.env[envKey] = apiKey;

    launchCodex(mode, (code) => {
        if (rewrote && providerId) rewriteSectionBaseUrl(providerId, baseUrl);
        server.close();
        process.exit(code);
    });
}

// Run main() when invoked as a script; export translation helpers so
// tests can exercise them in isolation without spawning Codex.
if (require.main === module) {
    main().catch(e => { console.error("[Echobird] Fatal:", e); process.exit(1); });
} else {
    module.exports = {
        responsesToChat,
        chatToResponsesNonStream,
        startProxy,
        buildSectionRegex,
        rewriteSectionBaseUrl,
    };
}
