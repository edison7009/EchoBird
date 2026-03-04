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
const { spawnSync } = require("child_process");

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

    // instructions → system message
    if (body.instructions) {
        messages.push({ role: "system", content: body.instructions });
    }

    // input handling
    if (typeof body.input === "string") {
        messages.push({ role: "user", content: body.input });
    } else if (Array.isArray(body.input)) {
        for (const item of body.input) {
            if (item.type === "message") {
                // Map "developer" role to "system" (third-party APIs don't recognize "developer")
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
            } else if (item.type === "item_reference") {
                // skip references
            }
        }
    }

    // Merge consecutive system messages into next user message
    // Some APIs (e.g. MiniMax) don't support system role at all
    const merged = [];
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

    // Ensure at least one message
    if (merged.length === 0) {
        merged.push({ role: "user", content: "Hello" });
    }

    const chatBody = {
        model: body.model,
        messages: merged,
        stream: body.stream !== false,
    };
    if (body.max_output_tokens) chatBody.max_tokens = body.max_output_tokens;
    if (body.temperature != null) chatBody.temperature = body.temperature;

    return chatBody;
}

// ─── Chat Completions → Responses SSE conversion ───

function generateResponseId() {
    return "resp_" + Math.random().toString(36).slice(2, 14);
}

function chatStreamToResponsesStream(res, clientRes) {
    const responseId = generateResponseId();
    const outputIndex = 0;
    const contentIndex = 0;

    // Send initial events
    const sendSSE = (event, data) => {
        clientRes.write(`event: ${event}\ndata: ${JSON.stringify(data)}\n\n`);
    };

    // response.created
    sendSSE("response.created", {
        type: "response.created",
        response: {
            id: responseId,
            object: "response",
            status: "in_progress",
            output: [],
        },
    });

    // response.in_progress
    sendSSE("response.in_progress", {
        type: "response.in_progress",
        response: { id: responseId, object: "response", status: "in_progress" },
    });

    // output_item.added
    sendSSE("response.output_item.added", {
        type: "response.output_item.added",
        output_index: outputIndex,
        item: {
            id: `item_${responseId}`,
            type: "message",
            role: "assistant",
            content: [],
        },
    });

    // content_part.added
    sendSSE("response.content_part.added", {
        type: "response.content_part.added",
        output_index: outputIndex,
        content_index: contentIndex,
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
                // Finish events
                sendSSE("response.output_text.done", {
                    type: "response.output_text.done",
                    output_index: outputIndex,
                    content_index: contentIndex,
                    text: fullText,
                });
                sendSSE("response.content_part.done", {
                    type: "response.content_part.done",
                    output_index: outputIndex,
                    content_index: contentIndex,
                    part: { type: "output_text", text: fullText },
                });
                sendSSE("response.output_item.done", {
                    type: "response.output_item.done",
                    output_index: outputIndex,
                    item: {
                        id: `item_${responseId}`,
                        type: "message",
                        role: "assistant",
                        content: [{ type: "output_text", text: fullText }],
                    },
                });
                sendSSE("response.completed", {
                    type: "response.completed",
                    response: {
                        id: responseId,
                        object: "response",
                        status: "completed",
                        output: [
                            {
                                id: `item_${responseId}`,
                                type: "message",
                                role: "assistant",
                                content: [{ type: "output_text", text: fullText }],
                            },
                        ],
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
                    sendSSE("response.output_text.delta", {
                        type: "response.output_text.delta",
                        output_index: outputIndex,
                        content_index: contentIndex,
                        delta,
                    });
                }
            } catch {
                // skip malformed chunks
            }
        }
    });

    res.on("end", () => {
        if (!clientRes.writableEnded) {
            clientRes.end();
        }
    });

    res.on("error", (err) => {
        console.error("[Proxy] Upstream error:", err.message);
        if (!clientRes.writableEnded) {
            clientRes.end();
        }
    });
}

// Non-streaming response conversion
function chatToResponsesNonStream(chatResponse) {
    const responseId = generateResponseId();
    const text =
        chatResponse.choices?.[0]?.message?.content || "";
    return {
        id: responseId,
        object: "response",
        status: "completed",
        output: [
            {
                id: `item_${responseId}`,
                type: "message",
                role: "assistant",
                content: [{ type: "output_text", text }],
            },
        ],
    };
}

// ─── Proxy Server ───

function startProxy(realBaseUrl, apiKey) {
    return new Promise((resolve, reject) => {
        const server = http.createServer((req, clientRes) => {
            // Only handle /v1/responses
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

                    // Build upstream URL
                    const url = new URL(realBaseUrl.replace(/\/$/, "") + "/chat/completions");
                    const isHttps = url.protocol === "https:";
                    const transport = isHttps ? https : http;

                    const upstreamReq = transport.request(
                        {
                            hostname: url.hostname,
                            port: url.port || (isHttps ? 443 : 80),
                            path: url.pathname,
                            method: "POST",
                            headers: {
                                "Content-Type": "application/json",
                                Authorization: `Bearer ${apiKey}`,
                            },
                        },
                        (upstreamRes) => {
                            // Check status
                            if (upstreamRes.statusCode !== 200) {
                                let errBody = "";
                                upstreamRes.on("data", (c) => (errBody += c));
                                upstreamRes.on("end", () => {
                                    console.error(`[Proxy] Upstream ${upstreamRes.statusCode}: ${errBody}`);
                                    clientRes.writeHead(upstreamRes.statusCode, {
                                        "Content-Type": "application/json",
                                    });
                                    clientRes.end(errBody);
                                });
                                return;
                            }

                            if (isStream) {
                                clientRes.writeHead(200, {
                                    "Content-Type": "text/event-stream",
                                    "Cache-Control": "no-cache",
                                    Connection: "keep-alive",
                                });
                                chatStreamToResponsesStream(upstreamRes, clientRes);
                            } else {
                                let resBody = "";
                                upstreamRes.on("data", (c) => (resBody += c));
                                upstreamRes.on("end", () => {
                                    try {
                                        const chatRes = JSON.parse(resBody);
                                        const responsesRes = chatToResponsesNonStream(chatRes);
                                        clientRes.writeHead(200, {
                                            "Content-Type": "application/json",
                                        });
                                        clientRes.end(JSON.stringify(responsesRes));
                                    } catch (e) {
                                        clientRes.writeHead(500);
                                        clientRes.end(JSON.stringify({ error: e.message }));
                                    }
                                });
                            }
                        }
                    );

                    upstreamReq.on("error", (e) => {
                        console.error("[Proxy] Request error:", e.message);
                        clientRes.writeHead(502);
                        clientRes.end(JSON.stringify({ error: e.message }));
                    });

                    upstreamReq.write(JSON.stringify(chatBody));
                    upstreamReq.end();
                } catch (e) {
                    clientRes.writeHead(400);
                    clientRes.end(JSON.stringify({ error: e.message }));
                }
            });
        });

        server.listen(0, "127.0.0.1", () => {
            const port = server.address().port;
            console.log(`[Echobird] Proxy started on 127.0.0.1:${port}`);
            resolve(port);
        });

        server.on("error", reject);
    });
}

// ─── Rewrite config.toml ───

function rewriteConfigBaseUrl(port) {
    try {
        let toml = fs.readFileSync(CODEX_CONFIG, "utf-8");
        toml = toml.replace(
            /base_url\s*=\s*"[^"]*"/,
            `base_url = "http://127.0.0.1:${port}/v1"`
        );
        fs.writeFileSync(CODEX_CONFIG, toml, "utf-8");
        console.log(`[Echobird] config.toml base_url → http://127.0.0.1:${port}/v1`);
    } catch (e) {
        console.error("[Echobird] Failed to rewrite config.toml:", e.message);
    }
}

function restoreConfigBaseUrl(originalUrl) {
    try {
        let toml = fs.readFileSync(CODEX_CONFIG, "utf-8");
        toml = toml.replace(
            /base_url\s*=\s*"[^"]*"/,
            `base_url = "${originalUrl}"`
        );
        fs.writeFileSync(CODEX_CONFIG, toml, "utf-8");
        console.log(`[Echobird] config.toml base_url restored → ${originalUrl}`);
    } catch {
        // ignore
    }
}

// ─── Main ───

async function main() {
    const config = loadEchobirdConfig();
    if (!config) {
        console.log("[Echobird] No config found, launching Codex directly");
        spawnCodex();
        return;
    }

    const { apiKey, baseUrl, modelId } = config;

    if (isOpenAI(baseUrl)) {
        // OpenAI: direct connection, no proxy needed
        console.log("[Echobird] OpenAI API detected, direct connection");
        process.env.OPENAI_API_KEY = apiKey;
        spawnCodex();
        return;
    }

    // Third-party API: start proxy
    console.log(`[Echobird] Third-party API: ${baseUrl}`);
    console.log(`[Echobird] Model: ${modelId || "default"}`);

    try {
        const port = await startProxy(baseUrl, apiKey);
        rewriteConfigBaseUrl(port);

        // Set env for Codex — it connects to our local proxy
        process.env.OPENAI_API_KEY = apiKey;
        process.env.OPENAI_BASE_URL = `http://127.0.0.1:${port}/v1`;

        spawnCodex();

        // Restore original base_url after Codex exits
        restoreConfigBaseUrl(baseUrl);
    } catch (e) {
        console.error("[Echobird] Failed to start proxy:", e.message);
        process.exit(1);
    }
}

function spawnCodex() {
    // Find codex binary
    const codexCmd = process.platform === "win32" ? "codex.cmd" : "codex";
    const npmGlobal = process.platform === "win32"
        ? path.join(process.env.APPDATA || "", "npm")
        : "/usr/local/bin";
    const codexPath = path.join(npmGlobal, codexCmd);

    if (!fs.existsSync(codexPath)) {
        console.error(`[Echobird] Codex not found: ${codexPath}`);
        process.exit(1);
    }

    console.log(`[Echobird] Launching Codex: ${codexPath}`);
    const result = spawnSync(codexPath, [], {
        stdio: "inherit",
        env: process.env,
        cwd: os.homedir(),
    });

    process.exit(result.status || 0);
}

main().catch((e) => {
    console.error("[Echobird] Fatal:", e);
    process.exit(1);
});
