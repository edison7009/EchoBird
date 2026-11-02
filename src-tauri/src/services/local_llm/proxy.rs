// Unified Proxy Server
//
// Listens on the user-facing port and routes:
//   /v1/*        → direct passthrough to llama-server (OpenAI native)
//   /anthropic/* → Anthropic→OpenAI format conversion, then forward
//
// Bug fixes applied here:
//   Bug 1: read_full_http_request() reads complete HTTP body (not just 64 KB)
//   Bug 2: API key is extracted from incoming request and forwarded to llama-server

use tokio::net::TcpListener;
use tokio::sync::watch;

// ─── Entry Point ───

pub async fn run_unified_proxy(
    listen_port: u16,
    target_port: u16,
    mut shutdown_rx: watch::Receiver<bool>,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    use tauri::Emitter;

    let addr = format!("0.0.0.0:{}", listen_port);
    let listener = TcpListener::bind(&addr)
        .await
        .map_err(|e| format!("Failed to bind proxy on {}: {}", addr, e))?;

    // Binding succeeded — notify the frontend STDOUT panel
    log::info!("[Proxy] Unified proxy bound to port {}", listen_port);
    let msg1 = "Unified Proxy started:".to_string();
    let msg2 = format!("  OpenAI:    http://127.0.0.1:{}/v1", listen_port);
    let msg3 = format!("  Anthropic: http://127.0.0.1:{}/anthropic", listen_port);
    let _ = app_handle.emit("local-llm-stdout", &msg1);
    let _ = app_handle.emit("local-llm-stdout", &msg2);
    let _ = app_handle.emit("local-llm-stdout", &msg3);
    // Also write to server.logs so frontend polling picks them up
    {
        let server = super::server::get_server_arc().await;
        let mut srv = server.lock().await;
        srv.add_log(&msg1);
        srv.add_log(&msg2);
        srv.add_log(&msg3);
    }

    loop {
        tokio::select! {
            _ = shutdown_rx.changed() => {
                if *shutdown_rx.borrow() {
                    log::info!("[Proxy] Shutdown received, stopping proxy");
                    break;
                }
            }
            accept = listener.accept() => {
                match accept {
                    Ok((stream, peer)) => {
                        let tp = target_port;
                        let h = app_handle.clone();
                        tokio::spawn(async move {
                            log::debug!("[Proxy] New connection from {:?}", peer);
                            if let Err(e) = handle_proxy_connection(stream, tp, h).await {
                                log::warn!("[Proxy] Connection error: {}", e);
                            }
                        });
                    }
                    Err(e) => {
                        log::warn!("[Proxy] Accept error: {}", e);
                    }
                }
            }
        }
    }

    Ok(())
}

// ─── Connection Handler ───

/// Handle a single proxy connection
async fn handle_proxy_connection(
    mut stream: tokio::net::TcpStream,
    target_port: u16,
    _app_handle: tauri::AppHandle,
) -> Result<(), String> {
    use tokio::io::AsyncWriteExt;

    // Bug 1 Fix: read the complete HTTP request (headers + full body)
    let raw = read_full_http_request(&mut stream).await?;
    if raw.is_empty() {
        return Ok(());
    }

    let raw_str = String::from_utf8_lossy(&raw);

    // Parse request line
    let first_line = raw_str.lines().next().unwrap_or("");
    let parts: Vec<&str> = first_line.split_whitespace().collect();
    if parts.len() < 2 {
        let resp = "HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\n\r\n";
        let _ = stream.write_all(resp.as_bytes()).await;
        return Ok(());
    }

    let method = parts[0];
    let path = parts[1];

    log::info!(
        "[Proxy] {} {} → forwarding to :{}",
        method,
        path,
        target_port
    );

    // CORS preflight
    if method == "OPTIONS" {
        let resp = "HTTP/1.1 204 No Content\r\n\
            Access-Control-Allow-Origin: *\r\n\
            Access-Control-Allow-Methods: GET, POST, OPTIONS\r\n\
            Access-Control-Allow-Headers: *\r\n\
            Content-Length: 0\r\n\r\n";
        let _ = stream.write_all(resp.as_bytes()).await;
        return Ok(());
    }

    if path.starts_with("/anthropic") {
        handle_anthropic_proxy(&mut stream, &raw, target_port).await
    } else {
        handle_passthrough(&mut stream, &raw, target_port).await
    }
}

// ─── Bug 1 Fix: Complete HTTP Request Reader ───

/// Read a complete HTTP request from a TCP stream.
///
/// HTTP requests may arrive in multiple TCP segments. A single `read()` call
/// may return an incomplete body — especially for large Anthropic requests
/// containing tool definitions (can easily exceed 64 KB).
///
/// This function reads until \r\n\r\n (end of headers), parses Content-Length,
/// then reads the exact number of body bytes.
async fn read_full_http_request(stream: &mut tokio::net::TcpStream) -> Result<Vec<u8>, String> {
    use tokio::io::AsyncReadExt;

    let mut buf: Vec<u8> = Vec::with_capacity(8192);
    let mut tmp = vec![0u8; 4096];

    // Phase 1: read until we have the full headers (\r\n\r\n)
    let header_end;
    loop {
        let n = stream
            .read(&mut tmp)
            .await
            .map_err(|e| format!("Read error: {}", e))?;
        if n == 0 {
            // Connection closed before headers finished
            return Ok(buf);
        }
        buf.extend_from_slice(&tmp[..n]);

        // Look for end-of-headers marker
        if let Some(pos) = find_subsequence(&buf, b"\r\n\r\n") {
            header_end = pos + 4; // position after \r\n\r\n
            break;
        }

        // Safety: don't let headers grow unbounded (16 MB cap)
        if buf.len() > 16 * 1024 * 1024 {
            return Err("Request headers too large".to_string());
        }
    }

    // Phase 2: determine body length from Content-Length header
    let headers_raw = String::from_utf8_lossy(&buf[..header_end]);
    let content_length: usize = headers_raw
        .lines()
        .find(|l| l.to_lowercase().starts_with("content-length:"))
        .and_then(|l| l.split(':').nth(1))
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(0);

    // How many body bytes we already have in buf
    let body_already = buf.len().saturating_sub(header_end);

    // Phase 3: read remaining body bytes
    if content_length > body_already {
        let remaining = content_length - body_already;
        let mut body_rest = vec![0u8; remaining];
        let mut read_so_far = 0;

        while read_so_far < remaining {
            let n = stream
                .read(&mut body_rest[read_so_far..])
                .await
                .map_err(|e| format!("Read body error: {}", e))?;
            if n == 0 {
                break;
            }
            read_so_far += n;
        }

        buf.extend_from_slice(&body_rest[..read_so_far]);
    }

    Ok(buf)
}

/// Find the first occurrence of `needle` in `haystack`, returning the start index.
fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

// ─── Passthrough (OpenAI path) ───

/// Direct passthrough: forward request to llama-server as-is.
///
/// Uses copy_bidirectional for full-duplex TCP pipe — this correctly handles
/// SSE streaming responses where the connection stays open indefinitely.
/// The old read() loop would return 0 on an idle-but-alive SSE stream, causing
/// premature disconnection ("AI 断开连接" in third-party apps like Reversi).
async fn handle_passthrough(
    stream: &mut tokio::net::TcpStream,
    raw_request: &[u8],
    target_port: u16,
) -> Result<(), String> {
    use tokio::io::{copy_bidirectional, AsyncWriteExt};
    use tokio::net::TcpStream;

    let mut target = TcpStream::connect(format!("127.0.0.1:{}", target_port))
        .await
        .map_err(|e| format!("Connect to llama-server failed: {}", e))?;

    // Send the full HTTP request to llama-server
    target
        .write_all(raw_request)
        .await
        .map_err(|e| format!("Write to target: {}", e))?;

    // Bidirectional pipe: handles chunked, SSE streaming, keep-alive, etc.
    // Terminates naturally when either side closes the connection.
    copy_bidirectional(stream, &mut target)
        .await
        .map_err(|e| format!("Proxy pipe error: {}", e))?;

    Ok(())
}

// ─── Anthropic Proxy ───

/// Anthropic proxy: convert Anthropic Messages API → OpenAI Chat Completions API.
///
/// Bug 1 Fix: `raw_request` is now the complete request (guaranteed by read_full_http_request).
/// Bug 2 Fix: Extract API key from client headers and forward to llama-server.
async fn handle_anthropic_proxy(
    stream: &mut tokio::net::TcpStream,
    raw_request: &[u8],
    target_port: u16,
) -> Result<(), String> {
    use tokio::io::AsyncWriteExt;

    let raw_str = String::from_utf8_lossy(raw_request);

    // Bug 2 Fix: extract API key from client to forward to llama-server
    // API key extraction no longer needed — llama-server runs without authentication
    // extract_api_key(&raw_str);

    // Find body (after \r\n\r\n)
    let body_str = if let Some(pos) = raw_str.find("\r\n\r\n") {
        &raw_str[pos + 4..]
    } else {
        ""
    };

    // Parse Anthropic request body
    let anthropic_req: serde_json::Value = serde_json::from_str(body_str).unwrap_or_else(|e| {
        log::warn!(
            "[Proxy] Failed to parse Anthropic body: {} (body_len={})",
            e,
            body_str.len()
        );
        serde_json::json!({})
    });

    let is_stream = anthropic_req
        .get("stream")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    // Convert Anthropic request → OpenAI format
    let openai_req = anthropic_to_openai(&anthropic_req);

    log::info!(
        "[Proxy] Anthropic→OpenAI: {} messages, stream={}",
        openai_req
            .get("messages")
            .and_then(|m| m.as_array())
            .map(|a| a.len())
            .unwrap_or(0),
        is_stream
    );

    let target_url = format!("http://127.0.0.1:{}/v1/chat/completions", target_port);

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .build()
        .map_err(|e| format!("Build client: {}", e))?;

    // llama-server no longer requires api_key authentication (local-only, no
    // auth needed).
    //
    // Retry loop for 503 — llama-server opens its HTTP port as soon as the
    // process starts, but model loading (mmap'ing a multi-GB GGUF into VRAM)
    // can take 5–30 seconds depending on disk speed + model size. During
    // that window every request comes back as
    // 503 {"message": "Loading model"}. Without this retry the first
    // Mother Agent request after `Start` always shows that 503 in chat
    // and looks like the app is broken. Exponential backoff: 1+2+4+8+16 = 31s
    // total max wait, which covers the realistic cold-start window for
    // models up to ~30B Q4_K_M on consumer hardware.
    const MAX_LOADING_RETRIES: u32 = 5;
    let mut retry_count = 0_u32;
    let response = loop {
        let res = client
            .post(&target_url)
            .header("Content-Type", "application/json")
            .json(&openai_req)
            .send()
            .await
            .map_err(|e| format!("Request to llama-server: {}", e))?;

        // Any non-503 (success or non-loading error) breaks out — the
        // existing success path and error reporter below take over.
        if res.status().as_u16() != 503 || retry_count >= MAX_LOADING_RETRIES {
            break res;
        }

        retry_count += 1;
        let delay = std::time::Duration::from_secs(2_u64.pow((retry_count - 1).min(4)));
        log::info!(
            "[Proxy] llama-server 503 (model likely still loading), retry {}/{} after {:?}",
            retry_count,
            MAX_LOADING_RETRIES,
            delay
        );
        drop(res); // discard the in-flight 503 body; next iteration re-sends
        tokio::time::sleep(delay).await;
    };

    let upstream_status = response.status();
    if retry_count > 0 {
        log::info!(
            "[Proxy] llama-server HTTP {} (after {} loading retries)",
            upstream_status,
            retry_count
        );
    } else {
        log::info!("[Proxy] llama-server HTTP {}", upstream_status);
    }

    // If upstream returned a 4xx/5xx, the body is a JSON error object, NOT an
    // SSE stream. Reading it as SSE would discard the actual diagnostic and
    // silently end the response with no content — which is exactly the
    // "Mother Agent shows no response" symptom users hit. Pull the full body,
    // log it, and surface it to the client as a single text_delta so users
    // see the upstream error in chat rather than silence. Also short-circuit
    // before entering the streaming loop.
    if !upstream_status.is_success() {
        let error_body = response
            .text()
            .await
            .unwrap_or_else(|e| format!("(failed to read error body: {})", e));
        log::error!(
            "[Proxy] llama-server returned {}: {}",
            upstream_status,
            error_body
        );
        let truncated: String = error_body.chars().take(1024).collect();
        if is_stream {
            // Emit an Anthropic-style SSE error so the agent can surface it.
            let sse_headers = "HTTP/1.1 200 OK\r\n\
                Content-Type: text/event-stream\r\n\
                Cache-Control: no-cache\r\n\
                Connection: keep-alive\r\n\
                Access-Control-Allow-Origin: *\r\n\r\n";
            let _ = stream.write_all(sse_headers.as_bytes()).await;
            let msg_id = format!("msg_{}", chrono::Utc::now().timestamp_millis());
            let msg_start = serde_json::json!({
                "type": "message_start",
                "message": {
                    "id": msg_id, "type": "message", "role": "assistant", "content": [],
                    "model": "local-model", "stop_reason": null, "stop_sequence": null,
                    "usage": {"input_tokens": 0, "output_tokens": 0}
                }
            });
            let _ = stream
                .write_all(format!("event: message_start\ndata: {}\n\n", msg_start).as_bytes())
                .await;
            let block_start = serde_json::json!({
                "type": "content_block_start", "index": 0,
                "content_block": {"type": "text", "text": ""}
            });
            let _ = stream
                .write_all(
                    format!("event: content_block_start\ndata: {}\n\n", block_start).as_bytes(),
                )
                .await;
            let err_text = format!(
                "[Local LLM error] upstream returned {}: {}",
                upstream_status, truncated
            );
            let delta = serde_json::json!({
                "type": "content_block_delta", "index": 0,
                "delta": {"type": "text_delta", "text": err_text}
            });
            let _ = stream
                .write_all(format!("event: content_block_delta\ndata: {}\n\n", delta).as_bytes())
                .await;
            let _ = stream
                .write_all(
                    format!(
                        "event: content_block_stop\ndata: {}\n\n",
                        serde_json::json!({"type": "content_block_stop", "index": 0})
                    )
                    .as_bytes(),
                )
                .await;
            let _ = stream
                .write_all(
                    format!(
                        "event: message_delta\ndata: {}\n\n",
                        serde_json::json!({
                            "type": "message_delta",
                            "delta": {"stop_reason": "end_turn", "stop_sequence": null},
                            "usage": {"output_tokens": 0}
                        })
                    )
                    .as_bytes(),
                )
                .await;
            let _ = stream
                .write_all(
                    format!(
                        "event: message_stop\ndata: {}\n\n",
                        serde_json::json!({"type": "message_stop"})
                    )
                    .as_bytes(),
                )
                .await;
            let _ = stream.flush().await;
        } else {
            // Non-streaming: return a regular JSON Anthropic-shaped error.
            let body = serde_json::to_string(&serde_json::json!({
                "type": "error",
                "error": {
                    "type": "api_error",
                    "message": format!("Upstream llama-server returned {}: {}", upstream_status, truncated),
                }
            }))
            .unwrap_or_default();
            let resp = format!(
                "HTTP/1.1 502 Bad Gateway\r\n\
                 Content-Type: application/json\r\n\
                 Access-Control-Allow-Origin: *\r\n\
                 Content-Length: {}\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = stream.write_all(resp.as_bytes()).await;
        }
        return Ok(());
    }

    if is_stream {
        // ── STREAMING PATH ──
        let sse_headers = "HTTP/1.1 200 OK\r\n\
            Content-Type: text/event-stream\r\n\
            Cache-Control: no-cache\r\n\
            Connection: keep-alive\r\n\
            Access-Control-Allow-Origin: *\r\n\r\n";
        stream
            .write_all(sse_headers.as_bytes())
            .await
            .map_err(|e| format!("Write SSE headers: {}", e))?;

        let msg_id = format!("msg_{}", chrono::Utc::now().timestamp_millis());
        let msg_start = serde_json::json!({
            "type": "message_start",
            "message": {
                "id": msg_id, "type": "message", "role": "assistant", "content": [],
                "model": "local-model", "stop_reason": null, "stop_sequence": null,
                "usage": {"input_tokens": 0, "output_tokens": 0}
            }
        });
        stream
            .write_all(format!("event: message_start\ndata: {}\n\n", msg_start).as_bytes())
            .await
            .ok();

        let block_start = serde_json::json!({
            "type": "content_block_start", "index": 0,
            "content_block": {"type": "text", "text": ""}
        });
        stream
            .write_all(format!("event: content_block_start\ndata: {}\n\n", block_start).as_bytes())
            .await
            .ok();
        stream.flush().await.ok();

        use futures_util::StreamExt;
        let mut bytes_stream = response.bytes_stream();
        let mut partial = String::new();
        let mut done = false;

        while let Some(chunk_result) = bytes_stream.next().await {
            match chunk_result {
                Ok(chunk) => {
                    partial.push_str(&String::from_utf8_lossy(&chunk));
                    while let Some(nl) = partial.find('\n') {
                        let line = partial[..nl].trim_end_matches('\r').to_string();
                        partial = partial[nl + 1..].to_string();
                        if !line.starts_with("data: ") {
                            continue;
                        }
                        let json_str = &line[6..];
                        if json_str == "[DONE]" {
                            done = true;
                            break;
                        }
                        if let Ok(cj) = serde_json::from_str::<serde_json::Value>(json_str) {
                            let delta = cj
                                .get("choices")
                                .and_then(|c| c.as_array())
                                .and_then(|arr| arr.first())
                                .and_then(|c| c.get("delta"));
                            // Text delta
                            if let Some(text) = delta
                                .and_then(|d| d.get("content"))
                                .and_then(|c| c.as_str())
                            {
                                if !text.is_empty() {
                                    let ev = serde_json::json!({
                                        "type": "content_block_delta", "index": 0,
                                        "delta": {"type": "text_delta", "text": text}
                                    });
                                    if stream
                                        .write_all(
                                            format!("event: content_block_delta\ndata: {}\n\n", ev)
                                                .as_bytes(),
                                        )
                                        .await
                                        .is_err()
                                    {
                                        return Ok(());
                                    }
                                }
                            }
                            // Tool call delta
                            if let Some(tool_calls) = delta
                                .and_then(|d| d.get("tool_calls"))
                                .and_then(|t| t.as_array())
                            {
                                for tc in tool_calls {
                                    let idx = tc.get("index").and_then(|v| v.as_u64()).unwrap_or(0);
                                    if let Some(id) = tc.get("id").and_then(|v| v.as_str()) {
                                        let name = tc
                                            .get("function")
                                            .and_then(|f| f.get("name"))
                                            .and_then(|v| v.as_str())
                                            .unwrap_or("");
                                        if idx > 0 {
                                            let s = serde_json::json!({"type": "content_block_stop", "index": idx - 1});
                                            let _ = stream
                                                .write_all(
                                                    format!(
                                                        "event: content_block_stop\ndata: {}\n\n",
                                                        s
                                                    )
                                                    .as_bytes(),
                                                )
                                                .await;
                                        }
                                        let s = serde_json::json!({
                                            "type": "content_block_start", "index": idx,
                                            "content_block": {"type": "tool_use", "id": id, "name": name, "input": {}}
                                        });
                                        let _ = stream
                                            .write_all(
                                                format!(
                                                    "event: content_block_start\ndata: {}\n\n",
                                                    s
                                                )
                                                .as_bytes(),
                                            )
                                            .await;
                                    }
                                    if let Some(args) = tc
                                        .get("function")
                                        .and_then(|f| f.get("arguments"))
                                        .and_then(|v| v.as_str())
                                    {
                                        if !args.is_empty() {
                                            let ev = serde_json::json!({
                                                "type": "content_block_delta", "index": idx,
                                                "delta": {"type": "input_json_delta", "partial_json": args}
                                            });
                                            let _ = stream
                                                .write_all(
                                                    format!(
                                                        "event: content_block_delta\ndata: {}\n\n",
                                                        ev
                                                    )
                                                    .as_bytes(),
                                                )
                                                .await;
                                        }
                                    }
                                }
                            }
                        }
                    }
                    stream.flush().await.ok();
                }
                Err(e) => {
                    log::warn!("[Proxy] Stream error: {}", e);
                    break;
                }
            }
            if done {
                break;
            }
        }

        // Closing events
        stream
            .write_all(
                format!(
                    "event: content_block_stop\ndata: {}\n\n",
                    serde_json::json!({"type": "content_block_stop", "index": 0})
                )
                .as_bytes(),
            )
            .await
            .ok();
        stream
            .write_all(
                format!(
                    "event: message_delta\ndata: {}\n\n",
                    serde_json::json!({
                        "type": "message_delta",
                        "delta": {"stop_reason": "end_turn", "stop_sequence": null},
                        "usage": {"output_tokens": 0}
                    })
                )
                .as_bytes(),
            )
            .await
            .ok();
        stream
            .write_all(
                format!(
                    "event: message_stop\ndata: {}\n\n",
                    serde_json::json!({"type": "message_stop"})
                )
                .as_bytes(),
            )
            .await
            .ok();
        stream.flush().await.ok();
    } else {
        // ── NON-STREAMING PATH ──
        let openai_data = response
            .json::<serde_json::Value>()
            .await
            .unwrap_or_else(|_| serde_json::json!({}));

        let anthropic_resp = openai_to_anthropic(&openai_data);
        let body = serde_json::to_string(&anthropic_resp).unwrap_or_default();
        let resp = format!(
            "HTTP/1.1 200 OK\r\n\
             Content-Type: application/json\r\n\
             Access-Control-Allow-Origin: *\r\n\
             Content-Length: {}\r\n\r\n{}",
            body.len(),
            body
        );
        stream
            .write_all(resp.as_bytes())
            .await
            .map_err(|e| format!("Write response: {}", e))?;
    }

    Ok(())
}

// ─── Format Conversion: Anthropic ↔ OpenAI ───

/// Convert Anthropic Messages request → OpenAI Chat Completions request
fn anthropic_user_content_part(block: &serde_json::Value) -> Option<serde_json::Value> {
    match block.get("type").and_then(|value| value.as_str()) {
        Some("text") => block
            .get("text")
            .and_then(|value| value.as_str())
            .map(|text| serde_json::json!({"type": "text", "text": text})),
        Some("image") => {
            let source = block.get("source")?;
            let url = match source.get("type").and_then(|value| value.as_str()) {
                Some("base64") => {
                    let media_type = source.get("media_type")?.as_str()?;
                    let data = source.get("data")?.as_str()?;
                    format!("data:{media_type};base64,{data}")
                }
                Some("url") => source.get("url")?.as_str()?.to_string(),
                _ => return None,
            };
            Some(serde_json::json!({
                "type": "image_url",
                "image_url": {"url": url}
            }))
        }
        Some("document") => {
            let source = block.get("source")?;
            match source.get("type").and_then(|value| value.as_str()) {
                Some("base64") => {
                    let data = source.get("data")?.as_str()?;
                    let filename = block
                        .get("title")
                        .and_then(|value| value.as_str())
                        .filter(|title| !title.trim().is_empty())
                        .unwrap_or("document.pdf");
                    Some(serde_json::json!({
                        "type": "file",
                        "file": {"file_data": data, "filename": filename}
                    }))
                }
                Some("text") => source
                    .get("data")
                    .and_then(|value| value.as_str())
                    .map(|text| serde_json::json!({"type": "text", "text": text})),
                Some("url") => source
                    .get("url")
                    .and_then(|value| value.as_str())
                    .map(|url| {
                        serde_json::json!({
                            "type": "text",
                            "text": format!("Document URL: {url}")
                        })
                    }),
                Some("file") => {
                    source
                        .get("file_id")
                        .and_then(|value| value.as_str())
                        .map(|file_id| {
                            serde_json::json!({
                                "type": "text",
                                "text": format!("Anthropic file reference: {file_id}")
                            })
                        })
                }
                _ => None,
            }
        }
        _ => None,
    }
}

fn openai_user_content(parts: Vec<serde_json::Value>) -> serde_json::Value {
    let text_parts: Option<Vec<&str>> = parts
        .iter()
        .map(|part| {
            if part.get("type").and_then(|value| value.as_str()) == Some("text") {
                part.get("text").and_then(|value| value.as_str())
            } else {
                None
            }
        })
        .collect();

    match text_parts {
        Some(text_parts) => serde_json::Value::String(text_parts.concat()),
        None => serde_json::Value::Array(parts),
    }
}

fn anthropic_tool_result_content(
    tool_result: &serde_json::Value,
) -> (String, Vec<serde_json::Value>) {
    let Some(content) = tool_result.get("content") else {
        return (String::new(), Vec::new());
    };
    if let Some(text) = content.as_str() {
        return (text.to_string(), Vec::new());
    }

    let mut text = String::new();
    let mut attachments = Vec::new();
    if let Some(blocks) = content.as_array() {
        for block in blocks {
            if block.get("type").and_then(|value| value.as_str()) == Some("text") {
                if let Some(part) = block.get("text").and_then(|value| value.as_str()) {
                    text.push_str(part);
                }
            } else if block.get("type").and_then(|value| value.as_str()) == Some("tool_reference") {
                if let Some(name) = block
                    .get("tool_name")
                    .and_then(|value| value.as_str())
                    .filter(|name| !name.is_empty())
                {
                    if !text.is_empty() {
                        text.push('\n');
                    }
                    text.push_str(&format!("Tool {name} is loaded and can be called now."));
                }
            } else if let Some(part) = anthropic_user_content_part(block) {
                attachments.push(part);
            }
        }
    }
    (text, attachments)
}

pub(crate) fn anthropic_to_openai(body: &serde_json::Value) -> serde_json::Value {
    let mut messages = Vec::new();

    // System message
    if let Some(system) = body.get("system") {
        let system_text = if let Some(s) = system.as_str() {
            s.to_string()
        } else if let Some(arr) = system.as_array() {
            arr.iter()
                .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
                .collect::<Vec<_>>()
                .join("")
        } else {
            String::new()
        };
        if !system_text.is_empty() {
            messages.push(serde_json::json!({"role": "system", "content": system_text}));
        }
    }

    // Conversation messages — handle text, tool_use, and tool_result blocks
    if let Some(msgs) = body.get("messages").and_then(|m| m.as_array()) {
        for msg in msgs {
            let role = msg.get("role").and_then(|r| r.as_str()).unwrap_or("user");

            match msg.get("content") {
                // Simple string content
                Some(c) if c.is_string() => {
                    if role == "assistant" && c.as_str().is_some_and(str::is_empty) {
                        continue;
                    }
                    messages.push(
                        serde_json::json!({"role": role, "content": c.as_str().unwrap_or("")}),
                    );
                }
                // Array of content blocks
                Some(c) if c.is_array() => {
                    let blocks = c.as_array().unwrap();

                    let text_parts: Vec<&str> = blocks
                        .iter()
                        .filter(|b| b.get("type").and_then(|t| t.as_str()) == Some("text"))
                        .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
                        .collect();

                    let tool_use_blocks: Vec<&serde_json::Value> = blocks
                        .iter()
                        .filter(|b| b.get("type").and_then(|t| t.as_str()) == Some("tool_use"))
                        .collect();

                    let tool_result_blocks: Vec<&serde_json::Value> = blocks
                        .iter()
                        .filter(|b| b.get("type").and_then(|t| t.as_str()) == Some("tool_result"))
                        .collect();

                    if !tool_use_blocks.is_empty() {
                        let tool_calls: Vec<serde_json::Value> = tool_use_blocks.iter().map(|tu| {
                            let id = tu.get("id").and_then(|v| v.as_str()).unwrap_or("call_0");
                            let name = tu.get("name").and_then(|v| v.as_str()).unwrap_or("");
                            let input = tu.get("input").cloned().unwrap_or(serde_json::json!({}));
                            serde_json::json!({
                                "id": id, "type": "function",
                                "function": {
                                    "name": name,
                                    "arguments": serde_json::to_string(&input).unwrap_or_else(|_| "{}".to_string())
                                }
                            })
                        }).collect();
                        let text_content = text_parts.join("");
                        let mut assistant_msg = serde_json::json!({
                            "role": "assistant",
                            "tool_calls": tool_calls
                        });
                        if !text_content.is_empty() {
                            assistant_msg["content"] = serde_json::json!(text_content);
                        }
                        messages.push(assistant_msg);
                    } else if !tool_result_blocks.is_empty() {
                        let mut follow_up_content = Vec::new();
                        for tr in &tool_result_blocks {
                            let tool_call_id =
                                tr.get("tool_use_id").and_then(|v| v.as_str()).unwrap_or("");
                            let (mut result_content, attachments) =
                                anthropic_tool_result_content(tr);
                            if result_content.is_empty() && !attachments.is_empty() {
                                result_content =
                                    "Attachments are included in the following user message."
                                        .to_string();
                            }
                            messages.push(serde_json::json!({
                                "role": "tool",
                                "tool_call_id": tool_call_id,
                                "content": result_content
                            }));
                            if !attachments.is_empty() {
                                follow_up_content.push(serde_json::json!({
                                    "type": "text",
                                    "text": format!("Attachments from tool result {tool_call_id}:")
                                }));
                                follow_up_content.extend(attachments);
                            }
                        }
                        follow_up_content.extend(
                            blocks
                                .iter()
                                .filter(|block| {
                                    !matches!(
                                        block.get("type").and_then(|value| value.as_str()),
                                        Some("tool_result" | "tool_use")
                                    )
                                })
                                .filter_map(anthropic_user_content_part),
                        );
                        if !follow_up_content.is_empty() {
                            messages.push(serde_json::json!({
                                "role": role,
                                "content": openai_user_content(follow_up_content)
                            }));
                        }
                    } else {
                        let content: Vec<serde_json::Value> = blocks
                            .iter()
                            .filter_map(anthropic_user_content_part)
                            .collect();
                        // Strict Chat upstreams refuse an interrupted turn's empty reply.
                        if role == "assistant" && content.iter().all(|part| part["text"] == "") {
                            continue;
                        }
                        messages.push(serde_json::json!({
                            "role": role,
                            "content": openai_user_content(content)
                        }));
                    }
                }
                _ if role == "assistant" => {}
                _ => {
                    messages.push(serde_json::json!({"role": role, "content": ""}));
                }
            }
        }
    }

    let mut req = serde_json::json!({
        "model": body.get("model").and_then(|m| m.as_str()).unwrap_or("local-model"),
        "messages": messages,
        "max_tokens": body.get("max_tokens").and_then(|v| v.as_u64()).unwrap_or(4096),
        "stream": body.get("stream").and_then(|v| v.as_bool()).unwrap_or(false),
    });

    if let Some(temperature) = body.get("temperature") {
        req["temperature"] = temperature.clone();
    }

    if let Some(tools) = body.get("tools").and_then(|t| t.as_array()) {
        if !tools.is_empty() {
            let openai_tools: Vec<serde_json::Value> = tools.iter().map(|t| {
                serde_json::json!({
                    "type": "function",
                    "function": {
                        "name": t.get("name").and_then(|v| v.as_str()).unwrap_or(""),
                        "description": t.get("description").and_then(|v| v.as_str()).unwrap_or(""),
                        "parameters": t.get("input_schema").cloned()
                            .unwrap_or(serde_json::json!({"type": "object", "properties": {}}))
                    }
                })
            }).collect();
            req["tools"] = serde_json::json!(openai_tools);
            if let Some(tc) = body.get("tool_choice") {
                let choice_type = tc.get("type").and_then(|v| v.as_str()).unwrap_or("auto");
                req["tool_choice"] = match choice_type {
                    "any" => serde_json::json!("required"),
                    "none" => serde_json::json!("none"),
                    "tool" => serde_json::json!({
                        "type": "function",
                        "function": {
                            "name": tc.get("name").and_then(|v| v.as_str()).unwrap_or("")
                        }
                    }),
                    _ => serde_json::json!("auto"),
                };
                if tc
                    .get("disable_parallel_tool_use")
                    .and_then(|v| v.as_bool())
                    == Some(true)
                {
                    req["parallel_tool_calls"] = serde_json::json!(false);
                }
            }
        }
    }

    if let Some(stop) = body.get("stop_sequences") {
        req["stop"] = stop.clone();
    }
    if let Some(top_p) = body.get("top_p") {
        req["top_p"] = top_p.clone();
    }

    req
}

/// Convert OpenAI Chat Completions non-streaming response → Anthropic Messages response
pub(crate) fn openai_to_anthropic(data: &serde_json::Value) -> serde_json::Value {
    let model = data
        .get("model")
        .and_then(|m| m.as_str())
        .unwrap_or("local-model");
    let choice = data
        .get("choices")
        .and_then(|c| c.as_array())
        .and_then(|a| a.first());
    let message = choice.and_then(|c| c.get("message"));
    let finish_reason = choice
        .and_then(|c| c.get("finish_reason"))
        .and_then(|v| v.as_str())
        .unwrap_or("end_turn");

    let mut content_blocks: Vec<serde_json::Value> = Vec::new();

    if let Some(text) = message.and_then(|m| {
        m.get("content")
            .and_then(|c| c.as_str())
            .filter(|text| !text.is_empty())
            .or_else(|| m.get("refusal").and_then(|value| value.as_str()))
    }) {
        if !text.is_empty() {
            content_blocks.push(serde_json::json!({"type": "text", "text": text}));
        }
    }

    if let Some(tool_calls) = message
        .and_then(|m| m.get("tool_calls"))
        .and_then(|t| t.as_array())
    {
        for tc in tool_calls {
            let id = tc.get("id").and_then(|v| v.as_str()).unwrap_or("call_0");
            let name = tc
                .get("function")
                .and_then(|f| f.get("name"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let args_str = tc
                .get("function")
                .and_then(|f| f.get("arguments"))
                .and_then(|v| v.as_str())
                .unwrap_or("{}");
            let input: serde_json::Value =
                serde_json::from_str(args_str).unwrap_or(serde_json::json!({}));
            content_blocks.push(serde_json::json!({
                "type": "tool_use", "id": id, "name": name, "input": input
            }));
        }
    }

    if content_blocks.is_empty() {
        content_blocks.push(serde_json::json!({"type": "text", "text": ""}));
    }

    let stop_reason = match finish_reason {
        "tool_calls" => "tool_use",
        "stop" | "end_turn" => "end_turn",
        "length" => "max_tokens",
        other => other,
    };

    serde_json::json!({
        "id": format!("msg_{}", chrono::Utc::now().timestamp_millis()),
        "type": "message",
        "role": "assistant",
        "content": content_blocks,
        "model": model,
        "stop_reason": stop_reason,
        "stop_sequence": null,
        "usage": {
            "input_tokens": data.get("usage").and_then(|u| u.get("prompt_tokens")).and_then(|v| v.as_u64()).unwrap_or(0),
            "output_tokens": data.get("usage").and_then(|u| u.get("completion_tokens")).and_then(|v| v.as_u64()).unwrap_or(0),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_interrupted_empty_reply_is_not_sent_to_chat() {
        let converted = anthropic_to_openai(&serde_json::json!({
            "messages": [
                {"role": "user", "content": "First"},
                {"role": "assistant", "content": ""},
                {"role": "assistant", "content": [{"type": "thinking", "thinking": "partial"}]},
                {"role": "assistant", "content": [{"type": "text", "text": ""}]},
                {"role": "assistant"},
                {"role": "user", "content": "Second"},
                {"role": "assistant", "content": [{"type": "text", "text": "Kept"}]}
            ]
        }));
        let roles: Vec<_> = converted["messages"]
            .as_array()
            .unwrap()
            .iter()
            .map(|message| message["role"].as_str().unwrap())
            .collect();
        assert_eq!(roles, ["user", "user", "assistant"]);
    }

    #[test]
    fn converts_anthropic_image_and_document_inputs() {
        let converted = anthropic_to_openai(&serde_json::json!({
            "model": "claude-test",
            "messages": [{
                "role": "user",
                "content": [
                    {"type": "text", "text": "Inspect these"},
                    {"type": "image", "source": {
                        "type": "base64", "media_type": "image/png", "data": "aW1hZ2U="
                    }},
                    {"type": "image", "source": {
                        "type": "url", "url": "https://example.com/image.png"
                    }},
                    {"type": "document", "title": "report.pdf", "source": {
                        "type": "base64", "media_type": "application/pdf", "data": "cGRm"
                    }}
                ]
            }]
        }));

        let content = converted["messages"][0]["content"].as_array().unwrap();
        assert_eq!(
            content[0],
            serde_json::json!({"type": "text", "text": "Inspect these"})
        );
        assert_eq!(
            content[1]["image_url"]["url"],
            "data:image/png;base64,aW1hZ2U="
        );
        assert_eq!(
            content[2]["image_url"]["url"],
            "https://example.com/image.png"
        );
        assert_eq!(content[3]["file"]["file_data"], "cGRm");
        assert_eq!(content[3]["file"]["filename"], "report.pdf");
    }

    #[test]
    fn collapses_anthropic_text_blocks_for_openai_compatibility() {
        let converted = anthropic_to_openai(&serde_json::json!({
            "messages": [{
                "role": "user",
                "content": [
                    {"type": "text", "text": "Hello, "},
                    {"type": "text", "text": "world"}
                ]
            }]
        }));

        assert_eq!(converted["messages"][0]["content"], "Hello, world");
        assert!(converted.get("temperature").is_none());
    }

    #[test]
    fn preserves_tool_result_attachments_and_follow_up_text() {
        let converted = anthropic_to_openai(&serde_json::json!({
            "messages": [{
                "role": "user",
                "content": [
                    {"type": "tool_result", "tool_use_id": "call_1", "content": [
                        {"type": "text", "text": "Screenshot captured."},
                        {"type": "image", "source": {
                            "type": "base64", "media_type": "image/jpeg", "data": "anBlZw=="
                        }}
                    ]},
                    {"type": "text", "text": "What is wrong?"}
                ]
            }]
        }));

        let messages = converted["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0]["role"], "tool");
        assert_eq!(messages[0]["tool_call_id"], "call_1");
        assert_eq!(messages[0]["content"], "Screenshot captured.");
        assert_eq!(messages[1]["role"], "user");
        let follow_up = messages[1]["content"].as_array().unwrap();
        assert_eq!(
            follow_up[1]["image_url"]["url"],
            "data:image/jpeg;base64,anBlZw=="
        );
        assert_eq!(follow_up[2]["text"], "What is wrong?");
    }

    #[test]
    fn tool_reference_results_keep_loaded_tool_names_and_pairing() {
        for content in [
            serde_json::json!([
                {"type":"tool_reference","tool_name":"mcp__fixture__read"},
                {"type":"tool_reference","tool_name":"mcp__fixture__write"}
            ]),
            serde_json::json!([
                {"type":"text","text":"Loaded tools:"},
                {"type":"tool_reference","tool_name":"mcp__fixture__read"},
                {"type":"tool_reference","tool_name":"mcp__fixture__write"}
            ]),
        ] {
            let converted = anthropic_to_openai(&serde_json::json!({"messages":[
                {"role":"assistant","content":[{"type":"tool_use","id":"search_call","name":"ToolSearch","input":{"query":"fixture"}}]},
                {"role":"user","content":[{"type":"tool_result","tool_use_id":"search_call","content":content}]}
            ]}));
            let messages = converted["messages"].as_array().unwrap();
            assert_eq!(messages.len(), 2);
            assert_eq!(messages[0]["tool_calls"][0]["id"], "search_call");
            assert_eq!(messages[1]["role"], "tool");
            assert_eq!(messages[1]["tool_call_id"], "search_call");
            let text = messages[1]["content"].as_str().unwrap();
            assert!(text.contains("mcp__fixture__read"));
            assert!(text.contains("mcp__fixture__write"));
            assert!(!text.is_empty());
            if content[0]["type"] == "text" {
                assert!(text.starts_with("Loaded tools:"));
            }
        }
    }

    #[test]
    fn json_refusals_remain_visible_when_answer_content_is_missing() {
        for message in [
            serde_json::json!({"role":"assistant","refusal":"I cannot help with that."}),
            serde_json::json!({"role":"assistant","content":null,"refusal":"I cannot help with that."}),
            serde_json::json!({"role":"assistant","content":"","refusal":"I cannot help with that."}),
        ] {
            let converted = openai_to_anthropic(
                &serde_json::json!({"choices":[{"message":message,"finish_reason":"stop"}]}),
            );
            assert_eq!(
                converted["content"],
                serde_json::json!([{"type":"text","text":"I cannot help with that."}])
            );
        }
        let converted = openai_to_anthropic(
            &serde_json::json!({"choices":[{"message":{"content":"Answer already present","refusal":"Other explanation"},"finish_reason":"stop"}]}),
        );
        assert_eq!(converted["content"][0]["text"], "Answer already present");
    }

    #[test]
    fn historical_tool_calls_do_not_include_stream_indexes() {
        let converted = anthropic_to_openai(&serde_json::json!({
            "messages": [{
                "role": "assistant",
                "content": [{
                    "type": "tool_use",
                    "id": "call_1",
                    "name": "read_file",
                    "input": {"path": "a.txt"}
                }]
            }]
        }));

        let tool_call = &converted["messages"][0]["tool_calls"][0];
        assert_eq!(tool_call["id"], "call_1");
        assert!(tool_call.get("index").is_none());
    }
}
