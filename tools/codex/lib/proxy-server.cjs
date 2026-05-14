const http = require("http");
const https = require("https");
const { responsesToChat } = require("./protocol-converter.cjs");
const {
    chatStreamToResponsesStream,
    chatToResponsesNonStream,
    chatErrorToResponsesError,
} = require("./stream-handler.cjs");
const { readEchobirdRelay, CODEX_PROXY_PORT } = require("./config-manager.cjs");

// Map HTTP status codes from /chat/completions upstreams onto sensible
// "outward" status codes for Codex. The status code itself isn't checked
// by Codex (it reads the JSON envelope's status field), but we keep the
// upstream code so logs and any non-Codex consumer still see the truth.
function passthroughStatus(code) {
    if (!code || code < 400 || code > 599) return 502;
    return code;
}

// Convert a streaming error into a one-shot response.failed SSE event.
// Called when the upstream returned non-200 while the client expected a
// stream — we still need to emit something that looks like a stream to
// Codex, otherwise the client hangs waiting for response.created.
function sendStreamErrorEvent(clientRes, errorEnvelope) {
    if (!clientRes.headersSent) {
        clientRes.writeHead(200, {
            "Content-Type": "text/event-stream",
            "Cache-Control": "no-cache",
            "Connection": "keep-alive",
            "X-Accel-Buffering": "no",
        });
    }
    clientRes.write(`event: response.failed\ndata: ${JSON.stringify({
        type: "response.failed",
        response: errorEnvelope,
    })}\n\n`);
    if (!clientRes.writableEnded) clientRes.end();
}

// Proxy server
//
// Accepts POST /v1/responses (or /responses) from Codex, translates to
// Chat Completions format, forwards to the upstream provider, then
// translates the response back to Responses API format.
//
// Architecture: the proxy binds the fixed port CODEX_PROXY_PORT and
// reads ~/.echobird/codex.json (the relay file) on EVERY incoming
// request. That means EchoBird can switch models by rewriting only the
// relay JSON — no config.toml edit, no Codex restart, no launcher
// restart. The next request the proxy sees forwards to the new model.
//
// config.toml's base_url is permanently "http://127.0.0.1:53682/v1"
// (see CODEX_PROXY_PORT in config-manager.cjs / tool_config_manager.rs),
// so Codex's view never changes either.

function startProxy(sessions, logger) {
    const log = logger?.log || (() => {});
    const err = logger?.err || (() => {});

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

                // Per-request relay read: pick up model/key/baseUrl
                // changes made by EchoBird since the last request,
                // without any restart.
                const relay = readEchobirdRelay();
                if (!relay || !relay.baseUrl || !relay.apiKey) {
                    err("[Proxy] Relay file missing or incomplete — apply a model in EchoBird first");
                    const errorEnvelope = chatErrorToResponsesError(
                        503,
                        JSON.stringify({ error: { message: "No active model configured in EchoBird. Open EchoBird and select a model.", code: "no_active_model" } }),
                        sessions
                    );
                    const wantStream = (reqBody && reqBody.stream) !== false;
                    if (wantStream) {
                        sendStreamErrorEvent(clientRes, errorEnvelope);
                    } else {
                        clientRes.writeHead(503, { "Content-Type": "application/json" });
                        clientRes.end(JSON.stringify(errorEnvelope));
                    }
                    return;
                }
                const realBaseUrl = relay.baseUrl;
                const apiKey = relay.apiKey;
                const realModelId = relay.actualModel || relay.modelName;

                const chatBody = responsesToChat(reqBody, sessions, logger);
                const isStream = chatBody.stream;

                // Symmetric model-id deception ("smart spoof"): whatever
                // model id Codex put in the request is what we echo back
                // in the response. The real provider's model id only
                // exists in the leg between us and the upstream — Codex
                // never sees it, and we never need to know what name
                // Codex picked (gpt-5.4 / gpt-5.5 / anything else).
                const clientModel = chatBody.model;
                if (realModelId && chatBody.model !== realModelId) {
                    log(`[Proxy] Model ID rewrite: ${chatBody.model} → ${realModelId}`);
                    chatBody.model = realModelId;
                }

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
                        upstreamRes.on("data", c => {
                            // Cap accumulation at 16KB — error bodies are
                            // small in practice (a JSON envelope), and we
                            // never want a misbehaving upstream to push us
                            // into unbounded growth.
                            if (errBody.length < 16384) errBody += c;
                        });
                        upstreamRes.on("end", () => {
                            err(`[Proxy] Upstream ${upstreamRes.statusCode}: ${errBody.slice(0, 500)}`);
                            const errorEnvelope = chatErrorToResponsesError(
                                upstreamRes.statusCode, errBody, sessions
                            );
                            if (isStream) {
                                sendStreamErrorEvent(clientRes, errorEnvelope);
                            } else {
                                clientRes.writeHead(
                                    passthroughStatus(upstreamRes.statusCode),
                                    { "Content-Type": "application/json" }
                                );
                                clientRes.end(JSON.stringify(errorEnvelope));
                            }
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
                        chatStreamToResponsesStream(upstreamRes, clientRes, requestMessages, sessions, logger, clientModel);
                    } else {
                        let resBody = "";
                        upstreamRes.on("data", c => resBody += c);
                        upstreamRes.on("end", () => {
                            try {
                                const res = chatToResponsesNonStream(JSON.parse(resBody), requestMessages, sessions, logger, clientModel);
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
                    const errorEnvelope = chatErrorToResponsesError(
                        502, JSON.stringify({ error: { message: e.message, code: e.code || "connect_error" } }),
                        sessions
                    );
                    if (isStream) {
                        sendStreamErrorEvent(clientRes, errorEnvelope);
                    } else {
                        if (!clientRes.headersSent) {
                            clientRes.writeHead(502, { "Content-Type": "application/json" });
                        }
                        clientRes.end(JSON.stringify(errorEnvelope));
                    }
                });
                upstreamReq.write(JSON.stringify(chatBody));
                upstreamReq.end();
            });
        });
        server.on("error", reject);
        server.listen(CODEX_PROXY_PORT, "127.0.0.1", () => {
            log(`Proxy listening on 127.0.0.1:${CODEX_PROXY_PORT}`);
            resolve({ port: CODEX_PROXY_PORT, server });
        });
    });
}

module.exports = { startProxy };
