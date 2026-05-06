#!/usr/bin/env node
// Codex Launcher — Dual-spoofing proxy for third-party APIs
// Codex sends /v1/responses → Proxy converts to /v1/chat/completions → Third-party API
// Response is converted back: Chat Completions → Responses format
//
// Usage: node codex-launcher.cjs

const http = require("http");
const https = require("https");
const fs = require("fs");
const path = require("path");
const os = require("os");
const { spawn } = require("child_process");

// ─── Config ───

const ECHOBIRD_CONFIG = path.join(os.homedir(), ".echobird", "codex.json");
const CODEX_CONFIG = path.join(os.homedir(), ".codex", "config.toml");

function loadEchobirdConfig() {
    try {
        return JSON.parse(fs.readFileSync(ECHOBIRD_CONFIG, "utf-8"));
    } catch {
        return null;
    }
}

function isOpenAI(url) {
    return url && url.includes("api.openai.com");
}

// ─── Responses → Chat Completions conversion ───

function responsesToChat(body) {
    const messages = [];

    if (body.instructions) {
        messages.push({ role: "system", content: body.instructions });
    }

    if (typeof body.input === "string") {
        messages.push({ role: "user", content: body.input });
    } else if (Array.isArray(body.input)) {
        for (const item of body.input) {
            if (item.type === "message") {
                let role = item.role || "user";
                if (role === "developer") role = "system";

                if (typeof item.content === "string") {
                    messages.push({ role, content: item.content });
                } else if (Array.isArray(item.content)) {
                    const text = item.content
                        .filter((c) => c.type === "input_text" || c.type === "text")
                        .map((c) => c.text)
                        .join("\n");
                    if (text) messages.push({ role, content: text });
                }
            }
        }
    }

    const isMinimax = (body.model || "").toLowerCase().includes("minimax");

    const merged = [];
    if (isMinimax) {
        // Legacy mode for MiniMax: merge system into user because it ignores/mishandles System instructions
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
        if (pendingSystem) {
            merged.push({ role: "user", content: `[System Instructions]\n${pendingSystem}` });
        }
        if (merged.length === 0) {
            merged.push({ role: "user", content: "Hello" });
        }
    } else {
        // Modern mode (DeepSeek, OpenAI, etc.): Preserve system/user/assistant roles
        for (const msg of messages) {
            if (merged.length > 0 && merged[merged.length - 1].role === msg.role) {
                merged[merged.length - 1].content += "\n\n" + msg.content;
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
    return chatBody;
}

// ─── Chat Completions → Responses SSE conversion ───

function generateResponseId() {
    return "resp_" + Math.random().toString(36).slice(2, 14);
}

function chatStreamToResponsesStream(res, clientRes) {
    const responseId = generateResponseId();
    const oi = 0, ci = 0;

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
    sendSSE("response.output_item.added", {
        type: "response.output_item.added", output_index: oi,
        item: { id: `item_${responseId}`, type: "message", role: "assistant", content: [] },
    });
    sendSSE("response.content_part.added", {
        type: "response.content_part.added", output_index: oi, content_index: ci,
        part: { type: "output_text", text: "" },
    });

    let fullText = "";
    let buffer = "";

    res.on("data", (chunk) => {
        buffer += chunk.toString();
        const lines = buffer.split("\n");
        buffer = lines.pop() || "";

        for (const line of lines) {
            if (!line.startsWith("data: ")) continue;
            const data = line.slice(6).trim();
            if (data === "[DONE]") {
                sendSSE("response.output_text.done", { type: "response.output_text.done", output_index: oi, content_index: ci, text: fullText });
                sendSSE("response.content_part.done", { type: "response.content_part.done", output_index: oi, content_index: ci, part: { type: "output_text", text: fullText } });
                sendSSE("response.output_item.done", {
                    type: "response.output_item.done", output_index: oi,
                    item: { id: `item_${responseId}`, type: "message", role: "assistant", content: [{ type: "output_text", text: fullText }] },
                });
                sendSSE("response.completed", {
                    type: "response.completed",
                    response: {
                        id: responseId, object: "response", status: "completed",
                        output: [{ id: `item_${responseId}`, type: "message", role: "assistant", content: [{ type: "output_text", text: fullText }] }],
                    },
                });
                clientRes.end();
                return;
            }
            try {
                const parsed = JSON.parse(data);
                const delta = parsed.choices?.[0]?.delta?.content;
                if (delta) {
                    fullText += delta;
                    sendSSE("response.output_text.delta", { type: "response.output_text.delta", output_index: oi, content_index: ci, delta });
                }
            } catch { /* skip */ }
        }
    });

    res.on("end", () => { if (!clientRes.writableEnded) clientRes.end(); });
    res.on("error", (err) => { console.error("[Proxy] Upstream error:", err.message); if (!clientRes.writableEnded) clientRes.end(); });
}

function chatToResponsesNonStream(chatResponse) {
    const responseId = generateResponseId();
    const text = chatResponse.choices?.[0]?.message?.content || "";
    return {
        id: responseId, object: "response", status: "completed",
        output: [{ id: `item_${responseId}`, type: "message", role: "assistant", content: [{ type: "output_text", text }] }],
    };
}

// ─── Proxy Server ───

function startProxy(realBaseUrl, apiKey) {
    return new Promise((resolve, reject) => {
        const server = http.createServer((req, clientRes) => {
            if (req.method !== "POST" || !req.url.startsWith("/v1/responses")) {
                clientRes.writeHead(404);
                clientRes.end(JSON.stringify({ error: "Only /v1/responses is proxied" }));
                return;
            }

            let body = "";
            req.on("data", (c) => (body += c));
            req.on("end", () => {
                try {
                    const reqBody = JSON.parse(body);
                    const chatBody = responsesToChat(reqBody);
                    const isStream = chatBody.stream;

                    const url = new URL(realBaseUrl.replace(/\/$/, "") + "/chat/completions");
                    const isHttps = url.protocol === "https:";
                    const transport = isHttps ? https : http;

                    const upstreamReq = transport.request(
                        {
                            hostname: url.hostname,
                            port: url.port || (isHttps ? 443 : 80),
                            path: url.pathname,
                            method: "POST",
                            headers: { "Content-Type": "application/json", Authorization: `Bearer ${apiKey}` },
                        },
                        (upstreamRes) => {
                            if (upstreamRes.statusCode !== 200) {
                                let errBody = "";
                                upstreamRes.on("data", (c) => (errBody += c));
                                upstreamRes.on("end", () => {
                                    console.error(`[Proxy] Upstream ${upstreamRes.statusCode}: ${errBody}`);
                                    clientRes.writeHead(upstreamRes.statusCode, { "Content-Type": "application/json" });
                                    clientRes.end(errBody);
                                });
                                return;
                            }
                            if (isStream) {
                                clientRes.writeHead(200, { "Content-Type": "text/event-stream", "Cache-Control": "no-cache", Connection: "keep-alive" });
                                chatStreamToResponsesStream(upstreamRes, clientRes);
                            } else {
                                let resBody = "";
                                upstreamRes.on("data", (c) => (resBody += c));
                                upstreamRes.on("end", () => {
                                    try {
                                        const responsesRes = chatToResponsesNonStream(JSON.parse(resBody));
                                        clientRes.writeHead(200, { "Content-Type": "application/json" });
                                        clientRes.end(JSON.stringify(responsesRes));
                                    } catch (e) { clientRes.writeHead(500); clientRes.end(JSON.stringify({ error: e.message })); }
                                });
                            }
                        }
                    );
                    upstreamReq.on("error", (e) => { clientRes.writeHead(502); clientRes.end(JSON.stringify({ error: e.message })); });
                    upstreamReq.write(JSON.stringify(chatBody));
                    upstreamReq.end();
                } catch (e) { clientRes.writeHead(400); clientRes.end(JSON.stringify({ error: e.message })); }
            });
        });

        server.listen(0, "127.0.0.1", () => {
            const port = server.address().port;
            console.log(`[Echobird] Proxy started on 127.0.0.1:${port}`);
            resolve({ port, server });
        });
        server.on("error", reject);
    });
}

// ─── Config rewrite ───

function rewriteConfigBaseUrl(port) {
    try {
        let toml = fs.readFileSync(CODEX_CONFIG, "utf-8");
        toml = toml.replace(/base_url\s*=\s*"[^"]*"/, `base_url = "http://127.0.0.1:${port}/v1"`);
        fs.writeFileSync(CODEX_CONFIG, toml, "utf-8");
        console.log(`[Echobird] config.toml base_url → http://127.0.0.1:${port}/v1`);
    } catch (e) { console.error("[Echobird] Failed to rewrite config.toml:", e.message); }
}

function restoreConfigBaseUrl(originalUrl) {
    try {
        let toml = fs.readFileSync(CODEX_CONFIG, "utf-8");
        toml = toml.replace(/base_url\s*=\s*"[^"]*"/, `base_url = "${originalUrl}"`);
        fs.writeFileSync(CODEX_CONFIG, toml, "utf-8");
        console.log(`[Echobird] config.toml base_url restored → ${originalUrl}`);
    } catch { /* ignore */ }
}

// ─── Main ───

async function main() {
    const config = loadEchobirdConfig();
    if (!config) {
        console.log("[Echobird] No config found, launching Codex directly");
        launchCodex();
        return;
    }

    const { apiKey, baseUrl, modelId } = config;

    if (isOpenAI(baseUrl)) {
        console.log("[Echobird] OpenAI API detected, direct connection");
        process.env.OPENAI_API_KEY = apiKey;
        launchCodex();
        return;
    }

    // Third-party API: start proxy
    console.log(`[Echobird] Third-party API: ${baseUrl}`);
    console.log(`[Echobird] Model: ${modelId || "default"}`);

    const { port, server } = await startProxy(baseUrl, apiKey);
    rewriteConfigBaseUrl(port);

    process.env.OPENAI_API_KEY = apiKey;
    process.env.OPENAI_BASE_URL = `http://127.0.0.1:${port}/v1`;

    // Launch Codex async — proxy event loop must stay alive!
    launchCodex((code) => {
        restoreConfigBaseUrl(baseUrl);
        server.close();
        process.exit(code);
    });
}

// Resolve the platform-specific Rust codex binary. Codex v0.107+ ships a native
// Rust executable inside @openai/codex-<platform> — spawning it directly preserves
// the TTY chain. Going through codex.cmd → node codex.js → codex.exe with shell:true
// drops TTY-ness somewhere in the cmd /d /s /c wrapping, causing the Rust TUI to
// abort with "stdin is not a terminal".
function resolveCodexBinary() {
    const platform = process.platform;
    const arch = process.arch;

    // Map to the npm sub-package name and vendor directory triple used by codex.js.
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
    } else {
        return null;
    }

    // Build candidate npm-root paths to look for @openai/codex.
    const codexPkgRoots = [];

    // Strategy 1: derive from `where codex.cmd` / `which codex`.
    try {
        const { execFileSync } = require("child_process");
        const findCmd = platform === "win32" ? "where" : "which";
        const findArg = platform === "win32" ? "codex.cmd" : "codex";
        const stub = execFileSync(findCmd, [findArg], { encoding: "utf8", timeout: 3000 })
            .trim().split(/\r?\n/)[0].trim();
        if (stub) {
            const npmDir = path.dirname(stub);
            codexPkgRoots.push(path.join(npmDir, "node_modules", "@openai", "codex"));
            // Linux/macOS pattern: /usr/local/bin/codex → /usr/local/lib/node_modules/...
            codexPkgRoots.push(path.join(path.dirname(npmDir), "lib", "node_modules", "@openai", "codex"));
        }
    } catch { /* fall through */ }

    // Strategy 2: well-known npm prefix locations.
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

function launchCodex(onExit) {
    // Prefer the direct Rust binary so TTY survives the spawn. Fall back to the
    // npm shim only if we cannot locate the platform binary.
    let codexPath = resolveCodexBinary();
    let useShell = false;

    if (codexPath) {
        console.log(`[Echobird] Launching Codex (direct binary): ${codexPath}`);
    } else {
        const codexCmd = process.platform === "win32" ? "codex.cmd" : "codex";
        if (process.platform === "win32") {
            const appdata = process.env.APPDATA || process.env.LOCALAPPDATA || "";
            if (appdata && appdata.length > 2) {
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
                const result = execFileSync(findCmd, [codexCmd], { encoding: "utf8", timeout: 3000 })
                    .trim().split(/\r?\n/)[0].trim();
                if (result && fs.existsSync(result)) codexPath = result;
            } catch { /* not found via where/which */ }
        }
        if (!codexPath) codexPath = codexCmd;
        useShell = true;
        console.log(`[Echobird] Rust binary not found, falling back to shim: ${codexPath}`);
    }

    // Async spawn — keeps event loop alive for proxy server.
    // shell:false when we have the direct binary path, otherwise we need shell to
    // execute .cmd shims on Windows.
    const child = spawn(codexPath, [], {
        stdio: "inherit",
        env: process.env,
        cwd: os.homedir(),
        shell: useShell,
    });

    process.on("SIGINT", () => child.kill("SIGINT"));
    process.on("SIGTERM", () => child.kill("SIGTERM"));

    child.on("close", (code) => {
        if (onExit) onExit(code || 0);
        else process.exit(code || 0);
    });

    child.on("error", (err) => {
        console.error(`[Echobird] Failed to launch Codex: ${err.message}`);
        process.exit(1);
    });
}

main().catch((e) => {
    console.error("[Echobird] Fatal:", e);
    process.exit(1);
});
