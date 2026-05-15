// /v1/responses HTTP handler.
//
// Accepts POST /v1/responses (or /responses), reads ~/.echobird/codex.json
// for the active model / API key / upstream base_url, translates the
// Codex Responses-API request into a Chat Completions request, forwards
// it to the upstream provider, then translates the response back to
// Responses API SSE / JSON.
//
// Per-request relay read: the file is fetched fresh every time so
// EchoBird model switches take effect without restarting Codex or the
// proxy. config.toml's base_url is permanently `http://127.0.0.1:53682/v1`
// (see `apply_codex` in tool_config_manager.rs), so Codex's view never
// changes either.

use std::convert::Infallible;
use std::net::SocketAddr;
use std::time::Duration;

use axum::{
    body::Bytes,
    extract::State,
    http::{header, StatusCode},
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse, Json, Response,
    },
    routing::post,
    Router,
};
use futures_util::{Stream, StreamExt};
use serde_json::{json, Value};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

use super::config_manager::{default_relay_dir, is_openai, read_echobird_relay, RELAY_FILENAME};
use super::protocol_converter::responses_to_chat;
use super::session_store::SessionStore;
use super::stream_handler::{
    chat_error_to_responses_error, chat_to_responses_non_stream, SseEvent, StreamState,
};

/// Cap on how many bytes of upstream error body we accumulate before
/// truncating. Error envelopes are small (a JSON `{ "error": ... }`);
/// we never want a misbehaving upstream pushing us into unbounded growth.
const UPSTREAM_ERROR_BODY_CAP: usize = 16 * 1024;

/// Maximum time we'll wait for the upstream to deliver the next chunk
/// of a streaming response. A stalled upstream (TCP open but silent)
/// would otherwise hold a reqwest connection + spawned tokio task open
/// forever, leaking file descriptors over many sessions. 5 minutes is
/// generous enough to cover slow thinking-model warmups but tight
/// enough that a truly dead connection releases its resources.
const UPSTREAM_CHUNK_TIMEOUT: Duration = Duration::from_secs(300);

#[derive(Clone)]
struct AppState {
    sessions: SessionStore,
    http_client: reqwest::Client,
}

pub async fn run(port: u16) -> Result<(), String> {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));

    let http_client = reqwest::Client::builder()
        // No global timeout: streaming responses can run for many minutes.
        // We rely on TCP-level disconnect detection instead.
        .connect_timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| format!("reqwest client build failed: {e}"))?;

    let state = AppState {
        sessions: SessionStore::new(),
        http_client,
    };

    let app = Router::new()
        .route("/v1/responses", post(handle_responses))
        .route("/responses", post(handle_responses))
        .route("/v1/responses/compact", post(handle_compact))
        .route("/responses/compact", post(handle_compact))
        .with_state(state);

    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(l) => l,
        Err(e) => {
            // Most common cause: another EchoBird instance is still
            // running and holding the port. We log + bail so Tauri
            // keeps starting; the proxy is reachable via that other
            // instance's listener.
            return Err(format!("bind 127.0.0.1:{port} failed: {e}"));
        }
    };

    log::info!("[CodexProxy] listening on 127.0.0.1:{port}");
    axum::serve(listener, app)
        .await
        .map_err(|e| format!("serve failed: {e}"))
}

// ---------------------------------------------------------------------------
// Handler
// ---------------------------------------------------------------------------

async fn handle_responses(State(state): State<AppState>, body: Bytes) -> Response {
    // 1) Parse the request body.
    let req_body: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": { "message": e.to_string(), "code": "invalid_json" } })),
            )
                .into_response();
        }
    };

    let want_stream = req_body
        .get("stream")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);

    // 2) Read the relay file fresh. EchoBird's apply_codex writes this
    //    JSON whenever the user picks a model; we never cache it.
    let relay = match read_relay_or_error(&state.sessions, want_stream) {
        Ok(r) => r,
        Err(resp) => return *resp,
    };
    let RelayConfig {
        base_url,
        api_key,
        real_model_id,
    } = relay;

    // 3) Translate Responses → Chat. The translator uses SessionStore
    //    for previous_response_id replay + reasoning recovery.
    let mut chat_body = responses_to_chat(&req_body, &state.sessions);
    let client_model = chat_body
        .get("model")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    // Symmetric model-id deception: whatever model id Codex put in the
    // request is what we'll mirror back in the SSE response. The real
    // provider's model id only exists between us and the upstream;
    // Codex never sees it.
    if let Some(real) = real_model_id.as_deref() {
        let current = chat_body
            .get("model")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if !real.is_empty() && real != current {
            log::info!("[CodexProxy] Model ID rewrite: {current} → {real}");
            chat_body["model"] = Value::String(real.to_string());
        }
    }

    // 4) Build the upstream URL. Users sometimes enter the bare host
    //    without /v1; auto-add when missing so the forward lands on the
    //    standard OpenAI-compat endpoint.
    let upstream_url = normalize_upstream_url(&base_url);

    // 5) Forward. The request messages we just translated also need to
    //    persist alongside the assistant turn for future replays.
    let request_messages: Vec<Value> = chat_body
        .get("messages")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let is_stream = chat_body
        .get("stream")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);

    let accept_header = if is_stream {
        "text/event-stream"
    } else {
        "application/json"
    };

    let upstream_req = state
        .http_client
        .post(&upstream_url)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {api_key}"))
        .header(header::ACCEPT, accept_header)
        .json(&chat_body);

    let upstream_resp = match upstream_req.send().await {
        Ok(r) => r,
        Err(e) => {
            log::error!("[CodexProxy] Upstream connect error: {e}");
            let body = json!({
                "error": {
                    "message": e.to_string(),
                    "code": "connect_error",
                }
            })
            .to_string();
            let envelope = chat_error_to_responses_error(502, Some(&body), Some(&state.sessions));
            return error_response(envelope, 502, is_stream);
        }
    };

    let status = upstream_resp.status();
    if !status.is_success() {
        // Drain the body (capped) so we can surface the upstream's error
        // verbatim. Codex renders the JSON envelope's `error.message`
        // field — users see e.g. "Invalid API key" instead of a bare 401.
        let body_text = read_capped_body(upstream_resp).await;
        log::error!(
            "[CodexProxy] Upstream {}: {}",
            status.as_u16(),
            body_text.chars().take(500).collect::<String>()
        );
        let envelope =
            chat_error_to_responses_error(status.as_u16(), Some(&body_text), Some(&state.sessions));
        return error_response(envelope, passthrough_status(status.as_u16()), is_stream);
    }

    if is_stream {
        stream_response(
            upstream_resp,
            request_messages,
            client_model,
            state.sessions.clone(),
        )
    } else {
        non_stream_response(
            upstream_resp,
            request_messages,
            client_model,
            state.sessions.clone(),
        )
        .await
    }
}

// ---------------------------------------------------------------------------
// /v1/responses/compact — server-side conversation compaction.
// ---------------------------------------------------------------------------
//
// OpenAI's Responses API exposes a `compact` endpoint that returns an
// opaque `encrypted_content` blob the model server can later decrypt to
// recover the original "latent state" of a conversation using far fewer
// tokens (see https://developers.openai.com/api/docs/guides/compaction).
// Third-party providers (DeepSeek, MiMo, etc.) have no such mechanism —
// no encryption, no decoder, no stateful continuation.
//
// To make Codex's "压缩此线程" menu button work against third-party
// upstreams, we translate the compaction request into a plain
// "summarize this conversation" Chat Completions call. The resulting
// summary text rides back as `encrypted_content`. When Codex echoes it
// in the next /v1/responses request, `process_input_items` reads the
// text field back out and uses it as a system-message body instead of
// the generic placeholder.
//
// Tradeoffs vs OpenAI's native compaction:
//   • Quality depends on the upstream model's summarization
//   • Not as token-efficient as a true encrypted latent blob
//   • But: Codex's compact button no longer 404s, and the user retains
//     a real summary of pre-compaction context for the next turn
async fn handle_compact(State(state): State<AppState>, body: Bytes) -> Response {
    let req_body: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": { "message": e.to_string(), "code": "invalid_json" } })),
            )
                .into_response();
        }
    };

    // Compaction is non-streaming.
    let relay = match read_relay_or_error(&state.sessions, false) {
        Ok(r) => r,
        Err(resp) => return *resp,
    };
    let RelayConfig {
        base_url,
        api_key,
        real_model_id,
    } = relay;

    // Translate input → Chat messages (same path as /v1/responses).
    let mut chat_body = responses_to_chat(&req_body, &state.sessions);

    // Append a summarization instruction so the upstream model produces
    // a summary instead of a chat response. Goes as the LAST user
    // message so the model treats the prior conversation as input.
    let summary_prompt = "Please write a concise summary of the conversation above. Preserve key decisions, pending tasks, code changes, file paths, and any important context the user will need to continue this work in a follow-up turn. Output only the summary, no preamble.";
    if let Some(messages) = chat_body.get_mut("messages").and_then(|v| v.as_array_mut()) {
        messages.push(json!({ "role": "user", "content": summary_prompt }));
    }

    // Compaction must be one-shot — disable streaming, drop tools so
    // the model can't decide to call shell etc., cap output length.
    chat_body["stream"] = Value::Bool(false);
    chat_body.as_object_mut().map(|o| o.remove("tools"));
    chat_body.as_object_mut().map(|o| o.remove("tool_choice"));
    chat_body["max_tokens"] = json!(2048);

    if let Some(real) = real_model_id.as_deref() {
        let current = chat_body
            .get("model")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if !real.is_empty() && real != current {
            log::info!("[CodexProxy] (compact) Model ID rewrite: {current} → {real}");
            chat_body["model"] = Value::String(real.to_string());
        }
    }

    let upstream_url = normalize_upstream_url(&base_url);
    let upstream_req = state
        .http_client
        .post(&upstream_url)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {api_key}"))
        .header(header::ACCEPT, "application/json")
        .json(&chat_body);

    let upstream_resp = match upstream_req.send().await {
        Ok(r) => r,
        Err(e) => {
            log::error!("[CodexProxy] (compact) Upstream connect error: {e}");
            let body = json!({
                "error": { "message": e.to_string(), "code": "connect_error" }
            })
            .to_string();
            let envelope = chat_error_to_responses_error(502, Some(&body), Some(&state.sessions));
            return error_response(envelope, 502, false);
        }
    };

    let status = upstream_resp.status();
    if !status.is_success() {
        let body_text = read_capped_body(upstream_resp).await;
        log::error!(
            "[CodexProxy] (compact) Upstream {}: {}",
            status.as_u16(),
            body_text.chars().take(500).collect::<String>()
        );
        let envelope =
            chat_error_to_responses_error(status.as_u16(), Some(&body_text), Some(&state.sessions));
        return error_response(envelope, passthrough_status(status.as_u16()), false);
    }

    let resp_json: Value = match upstream_resp.json().await {
        Ok(v) => v,
        Err(e) => {
            log::error!("[CodexProxy] (compact) Upstream JSON parse failed: {e}");
            let envelope = chat_error_to_responses_error(
                502,
                Some(&format!(
                    r#"{{"error":{{"message":"{e}","code":"parse_error"}}}}"#
                )),
                Some(&state.sessions),
            );
            return error_response(envelope, 502, false);
        }
    };

    // Extract the summary text from the upstream response.
    let summary = resp_json
        .get("choices")
        .and_then(|v| v.get(0))
        .and_then(|c| c.get("message"))
        .and_then(|m| m.get("content"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    if summary.is_empty() {
        log::warn!("[CodexProxy] (compact) Upstream returned empty summary content");
    }

    // Build the compaction-shape Responses-API envelope. The summary
    // text rides as `encrypted_content` — see process_input_items
    // in protocol_converter.rs for the read-back path.
    let response_id = state.sessions.new_response_id();
    let envelope = json!({
        "id": response_id,
        "object": "response.compaction",
        "status": "completed",
        "output": [
            {
                "type": "compaction",
                "encrypted_content": summary,
            }
        ],
        "usage": resp_json.get("usage").cloned().unwrap_or(Value::Null),
    });

    (StatusCode::OK, Json(envelope)).into_response()
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

struct RelayConfig {
    base_url: String,
    api_key: String,
    real_model_id: Option<String>,
}

/// Pull the relay file. On miss / malformed / missing required fields,
/// return a fully-rendered error response (SSE or JSON) instead of a
/// RelayConfig. We box the Err variant because axum's `Response` is
/// ~hundreds of bytes — `Result<small, big>` triggers clippy's
/// `result_large_err` lint, and boxing keeps the happy-path size small.
fn read_relay_or_error(
    sessions: &SessionStore,
    want_stream: bool,
) -> Result<RelayConfig, Box<Response>> {
    let relay_dir = match default_relay_dir() {
        Some(d) => d,
        None => {
            let envelope = chat_error_to_responses_error(
                503,
                Some(
                    &json!({
                        "error": {
                            "message": "Could not resolve home directory to read EchoBird relay file.",
                            "code": "no_home_dir",
                        }
                    })
                    .to_string(),
                ),
                Some(sessions),
            );
            return Err(Box::new(error_response(envelope, 503, want_stream)));
        }
    };
    let relay_path = relay_dir.join(RELAY_FILENAME);

    let parsed = match read_echobird_relay(&relay_path) {
        Some(v) => v,
        None => {
            let envelope = chat_error_to_responses_error(
                503,
                Some(
                    &json!({
                        "error": {
                            "message": "No active model configured in EchoBird. Open EchoBird and select a model.",
                            "code": "no_active_model",
                        }
                    })
                    .to_string(),
                ),
                Some(sessions),
            );
            return Err(Box::new(error_response(envelope, 503, want_stream)));
        }
    };

    let base_url = parsed
        .get("baseUrl")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let api_key = parsed
        .get("apiKey")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if base_url.is_empty() || api_key.is_empty() {
        let envelope = chat_error_to_responses_error(
            503,
            Some(
                &json!({
                    "error": {
                        "message": "EchoBird relay file is missing baseUrl or apiKey — re-apply a model.",
                        "code": "incomplete_relay",
                    }
                })
                .to_string(),
            ),
            Some(sessions),
        );
        return Err(Box::new(error_response(envelope, 503, want_stream)));
    }

    let real_model_id = parsed
        .get("actualModel")
        .and_then(|v| v.as_str())
        .or_else(|| parsed.get("modelName").and_then(|v| v.as_str()))
        .map(|s| s.to_string());

    Ok(RelayConfig {
        base_url,
        api_key,
        real_model_id,
    })
}

/// Build the final POST URL from the user's `baseUrl`. Strips a
/// trailing slash and auto-appends `/v1` if no `/v<n>` suffix is
/// already present — many users enter the bare host.
fn normalize_upstream_url(base_url: &str) -> String {
    let mut base = base_url.trim_end_matches('/').to_string();
    // Look for `/v<digit>` at the end. If absent, append `/v1`.
    let has_version = base
        .rsplit('/')
        .next()
        .map(|seg| seg.starts_with('v') && seg[1..].chars().all(|c| c.is_ascii_digit()))
        .unwrap_or(false);
    if !has_version {
        base.push_str("/v1");
    }
    // OpenAI's official endpoint accepts our request shape verbatim;
    // log for visibility but no behavior change.
    if is_openai(&base) {
        log::debug!("[CodexProxy] Routing to official OpenAI endpoint");
    }
    base.push_str("/chat/completions");
    base
}

/// Map upstream HTTP status onto a sensible client-facing status. We
/// keep 4xx/5xx codes verbatim so logs and any non-Codex consumer see
/// the truth; anything outside that range collapses to 502.
fn passthrough_status(code: u16) -> u16 {
    if !(400..=599).contains(&code) {
        502
    } else {
        code
    }
}

/// Drain a response body up to UPSTREAM_ERROR_BODY_CAP bytes. Used on
/// non-200 upstream responses so we can extract `error.message`.
async fn read_capped_body(resp: reqwest::Response) -> String {
    let mut buf = Vec::with_capacity(2048);
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = match chunk {
            Ok(c) => c,
            Err(_) => break,
        };
        let remaining = UPSTREAM_ERROR_BODY_CAP.saturating_sub(buf.len());
        if remaining == 0 {
            break;
        }
        let take = chunk.len().min(remaining);
        buf.extend_from_slice(&chunk[..take]);
        if buf.len() >= UPSTREAM_ERROR_BODY_CAP {
            break;
        }
    }
    String::from_utf8_lossy(&buf).into_owned()
}

/// Wrap a Responses-shape error envelope into an HTTP response, choosing
/// between an SSE single-event stream (stream clients) or JSON (non-stream).
///
/// SSE path emits three events back-to-back: `response.created`,
/// `response.in_progress`, `response.failed`. The created+in_progress
/// preamble matches what the happy path sends before any output, so
/// Codex's stream client sees the same opening contract whether the
/// turn ends in success or failure. The body flows over axum's `Sse`
/// adapter (chunked transfer encoding) — not a fixed-Content-Length
/// String — so the connection lifecycle matches a real streaming turn.
fn error_response(envelope: Value, http_status: u16, is_stream: bool) -> Response {
    let status_code = StatusCode::from_u16(http_status).unwrap_or(StatusCode::BAD_GATEWAY);
    if is_stream {
        let response_id = envelope
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("resp_err")
            .to_string();
        let events = build_error_sse_events(&envelope, &response_id);
        // Always 200 OK on the SSE channel; the error sits in the
        // payload. Codex inspects the JSON envelope, not the HTTP code.
        let stream = tokio_stream::iter(
            events
                .into_iter()
                .map(|e| Ok::<Event, Infallible>(sse_event_to_axum(&e))),
        );
        Sse::new(stream).into_response()
    } else {
        (status_code, Json(envelope)).into_response()
    }
}

/// Build the three-event SSE envelope for the error path. Matches the
/// happy-path opening contract (response.created → response.in_progress)
/// so Codex's stream client doesn't sit waiting for the missing preamble
/// before parsing the failure.
fn build_error_sse_events(envelope: &Value, response_id: &str) -> Vec<SseEvent> {
    vec![
        SseEvent::new(
            "response.created",
            json!({
                "type": "response.created",
                "response": {
                    "id": response_id,
                    "object": "response",
                    "status": "in_progress",
                    "output": [],
                },
            }),
        ),
        SseEvent::new(
            "response.in_progress",
            json!({
                "type": "response.in_progress",
                "response": {
                    "id": response_id,
                    "object": "response",
                    "status": "in_progress",
                },
            }),
        ),
        SseEvent::new(
            "response.failed",
            json!({
                "type": "response.failed",
                "response": envelope,
            }),
        ),
    ]
}

// ---------------------------------------------------------------------------
// Streaming path
// ---------------------------------------------------------------------------

/// Start an async task that consumes the upstream byte stream, drives a
/// StreamState through it, and pipes the emitted SseEvents into a
/// channel. Returns the channel as an SSE response.
fn stream_response(
    upstream_resp: reqwest::Response,
    request_messages: Vec<Value>,
    client_model: Option<String>,
    sessions: SessionStore,
) -> Response {
    // Channel capacity is small — SSE events are tiny and the consumer
    // (axum's writer) drains them as fast as the TCP socket allows.
    let (tx, rx) = mpsc::channel::<Result<Event, Infallible>>(32);

    tokio::spawn(async move {
        let mut state = StreamState::new(&sessions, client_model, request_messages);
        state.start();
        // Drain initial response.created / response.in_progress events.
        if !forward_events(&mut state, &tx).await {
            return;
        }

        let mut bytes_stream = upstream_resp.bytes_stream();
        loop {
            // Per-chunk timeout: if the upstream goes silent for longer
            // than UPSTREAM_CHUNK_TIMEOUT we close the stream rather than
            // leak the connection / task / FD indefinitely.
            let next_chunk =
                tokio::time::timeout(UPSTREAM_CHUNK_TIMEOUT, bytes_stream.next()).await;
            match next_chunk {
                Ok(Some(Ok(b))) => {
                    if let Ok(s) = std::str::from_utf8(&b) {
                        state.feed_chunk(s);
                    } else {
                        // Non-UTF-8 byte run — surface as upstream error
                        // and bail. Real SSE bodies are always UTF-8.
                        state.fail("Upstream sent non-UTF-8 bytes", "upstream_encoding_error");
                        let _ = forward_events(&mut state, &tx).await;
                        return;
                    }
                    if !forward_events(&mut state, &tx).await {
                        return;
                    }
                }
                Ok(Some(Err(e))) => {
                    state.fail(
                        &format!("Upstream stream error: {e}"),
                        "upstream_stream_error",
                    );
                    let _ = forward_events(&mut state, &tx).await;
                    return;
                }
                Ok(None) => {
                    // Clean EOF — break out so finish() runs below.
                    break;
                }
                Err(_elapsed) => {
                    // Read timeout — upstream went silent past the cap.
                    state.fail(
                        &format!(
                            "Upstream stream stalled (no data for {}s)",
                            UPSTREAM_CHUNK_TIMEOUT.as_secs()
                        ),
                        "upstream_stall",
                    );
                    let _ = forward_events(&mut state, &tx).await;
                    return;
                }
            }
        }

        // Upstream closed cleanly. Drive `finish()` so we persist
        // history + emit response.completed (or response.incomplete).
        state.finish(&sessions);
        let _ = forward_events(&mut state, &tx).await;
    });

    let stream = ReceiverStream::new(rx);
    Sse::new(stream)
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
        .into_response()
}

/// Pull pending events out of `state` and shove them down the channel.
/// Returns false if the receiver has dropped (client disconnected).
async fn forward_events(
    state: &mut StreamState,
    tx: &mpsc::Sender<Result<Event, Infallible>>,
) -> bool {
    for ev in state.take_events() {
        let axum_event = sse_event_to_axum(&ev);
        if tx.send(Ok(axum_event)).await.is_err() {
            return false;
        }
    }
    true
}

fn sse_event_to_axum(ev: &SseEvent) -> Event {
    // axum's Event::default().data() expects a String. We serialize the
    // Value ourselves rather than letting axum stringify because
    // serde_json::to_string preserves field order on Map values.
    Event::default()
        .event(ev.event.clone())
        .data(ev.data.to_string())
}

/// Stream-as-trait export so we can name the return type in error_response.
#[allow(dead_code)]
fn _assert_event_stream_is_stream<S>(_: S)
where
    S: Stream<Item = Result<Event, Infallible>> + Send + 'static,
{
}

// ---------------------------------------------------------------------------
// Non-stream path
// ---------------------------------------------------------------------------

async fn non_stream_response(
    upstream_resp: reqwest::Response,
    request_messages: Vec<Value>,
    client_model: Option<String>,
    sessions: SessionStore,
) -> Response {
    let body_text = match upstream_resp.text().await {
        Ok(t) => t,
        Err(e) => {
            log::error!("[CodexProxy] Upstream body read failed: {e}");
            let envelope =
                chat_error_to_responses_error(502, Some(&e.to_string()), Some(&sessions));
            return (StatusCode::BAD_GATEWAY, Json(envelope)).into_response();
        }
    };
    let chat_resp: Value = match serde_json::from_str(&body_text) {
        Ok(v) => v,
        Err(e) => {
            log::error!("[CodexProxy] Upstream response not valid JSON: {e}");
            let envelope = chat_error_to_responses_error(
                502,
                Some(
                    &json!({
                        "error": {
                            "message": format!("Upstream returned non-JSON body: {e}"),
                            "code": "upstream_invalid_json",
                        }
                    })
                    .to_string(),
                ),
                Some(&sessions),
            );
            return (StatusCode::BAD_GATEWAY, Json(envelope)).into_response();
        }
    };
    let resp = chat_to_responses_non_stream(
        &chat_resp,
        request_messages,
        &sessions,
        client_model.as_deref(),
    );
    (StatusCode::OK, Json(resp)).into_response()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_url_appends_v1_when_missing() {
        assert_eq!(
            normalize_upstream_url("https://api.deepseek.com"),
            "https://api.deepseek.com/v1/chat/completions"
        );
    }

    #[test]
    fn normalize_url_strips_trailing_slash() {
        assert_eq!(
            normalize_upstream_url("https://api.deepseek.com/"),
            "https://api.deepseek.com/v1/chat/completions"
        );
    }

    #[test]
    fn normalize_url_preserves_existing_v1() {
        assert_eq!(
            normalize_upstream_url("https://api.deepseek.com/v1"),
            "https://api.deepseek.com/v1/chat/completions"
        );
    }

    #[test]
    fn normalize_url_preserves_existing_v2() {
        assert_eq!(
            normalize_upstream_url("https://api.example.com/v2"),
            "https://api.example.com/v2/chat/completions"
        );
    }

    #[test]
    fn normalize_url_does_not_double_append_v1() {
        // Trailing slash on /v1 was historically a source of /v1//v1.
        assert_eq!(
            normalize_upstream_url("https://api.deepseek.com/v1/"),
            "https://api.deepseek.com/v1/chat/completions"
        );
    }

    #[test]
    fn passthrough_status_keeps_4xx_5xx() {
        assert_eq!(passthrough_status(401), 401);
        assert_eq!(passthrough_status(429), 429);
        assert_eq!(passthrough_status(500), 500);
        assert_eq!(passthrough_status(503), 503);
    }

    #[test]
    fn passthrough_status_clamps_unknown() {
        assert_eq!(passthrough_status(200), 502);
        assert_eq!(passthrough_status(0), 502);
        assert_eq!(passthrough_status(900), 502);
    }

    #[test]
    fn error_response_stream_returns_sse_envelope() {
        let envelope = json!({ "id": "resp_x", "status": "failed" });
        let resp = error_response(envelope, 503, true);
        assert_eq!(resp.status(), StatusCode::OK);
        let ct = resp
            .headers()
            .get(axum::http::header::CONTENT_TYPE)
            .map(|v| v.to_str().unwrap().to_string())
            .unwrap_or_default();
        assert!(ct.starts_with("text/event-stream"), "got: {ct}");
    }

    #[test]
    fn error_response_non_stream_returns_json_with_status() {
        let envelope = json!({ "id": "resp_x", "status": "failed" });
        let resp = error_response(envelope, 503, false);
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
        let ct = resp
            .headers()
            .get(axum::http::header::CONTENT_TYPE)
            .map(|v| v.to_str().unwrap().to_string())
            .unwrap_or_default();
        assert!(ct.starts_with("application/json"), "got: {ct}");
    }

    #[test]
    fn build_error_sse_events_emits_three_event_preamble_then_failure() {
        let envelope = json!({
            "id": "resp_abc",
            "object": "response",
            "status": "failed",
            "error": { "code": "no_active_model", "message": "no model" },
            "output": [],
        });
        let events = build_error_sse_events(&envelope, "resp_abc");
        assert_eq!(events.len(), 3, "expected 3 events, got {}", events.len());
        assert_eq!(events[0].event, "response.created");
        assert_eq!(events[1].event, "response.in_progress");
        assert_eq!(events[2].event, "response.failed");
        // Preamble events must carry status=in_progress (matches happy path)
        assert_eq!(events[0].data["response"]["status"], "in_progress");
        assert_eq!(events[1].data["response"]["status"], "in_progress");
        // The failed event carries the caller-supplied envelope verbatim
        assert_eq!(events[2].data["response"], envelope);
        // All three reference the same response_id so Codex can stitch them
        assert_eq!(events[0].data["response"]["id"], "resp_abc");
        assert_eq!(events[1].data["response"]["id"], "resp_abc");
    }

    #[test]
    fn sse_event_to_axum_preserves_event_name() {
        let ev = SseEvent::new("response.created", json!({"hello":"world"}));
        let axum = sse_event_to_axum(&ev);
        // Event doesn't expose its name accessor publicly; assert
        // round-trip via the wire format.
        let formatted = format!("{axum:?}");
        assert!(formatted.contains("response.created"), "got: {formatted}");
    }
}
