const http = require("http");
const https = require("https");
const { responsesToChat } = require("./protocol-converter.cjs");
const { chatStreamToResponsesStream, chatToResponsesNonStream } = require("./stream-handler.cjs");

// Proxy server
//
// Accepts POST /v1/responses (or /responses) from Codex, translates to
// Chat Completions format, forwards to the upstream provider, then
// translates the response back to Responses API format.

function startProxy(realBaseUrl, apiKey, sessions, logger) {
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
                const chatBody = responsesToChat(reqBody, sessions, logger);
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
                        chatStreamToResponsesStream(upstreamRes, clientRes, requestMessages, sessions, logger);
                    } else {
                        let resBody = "";
                        upstreamRes.on("data", c => resBody += c);
                        upstreamRes.on("end", () => {
                            try {
                                const res = chatToResponsesNonStream(JSON.parse(resBody), requestMessages, sessions, logger);
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

module.exports = { startProxy };
