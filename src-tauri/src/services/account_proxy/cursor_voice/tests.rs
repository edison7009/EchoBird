use super::*;
use axum::{body::Body, http::Request, routing::post, Router};
use serde_json::json;
use std::sync::{Arc, Mutex};
use tokio::sync::Notify;
use tokio_stream::wrappers::ReceiverStream;

#[derive(Default)]
struct Observed {
    headers: Mutex<HeaderMap>,
    frames: Mutex<Vec<Vec<u8>>>,
    audio_times: Mutex<Vec<Duration>>,
    first_audio: Notify,
    dropped: Notify,
    input_closed: Mutex<bool>,
}
struct Signal(Arc<Observed>);
impl Drop for Signal {
    fn drop(&mut self) {
        self.0.dropped.notify_one();
    }
}
struct Fixture {
    credentials: Value,
    observed: Arc<Observed>,
    server: tokio::task::JoinHandle<()>,
    workers: Arc<Mutex<Vec<tokio::task::JoinHandle<()>>>>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.server.abort();
        for worker in self.workers.lock().unwrap().iter() {
            worker.abort();
        }
    }
}

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .http2_prior_knowledge()
        .http2_initial_stream_window_size(65_535)
        .http2_initial_connection_window_size(65_535)
        .build()
        .unwrap()
}

fn wav(bytes: usize) -> Bytes {
    let mut out = b"RIFF".to_vec();
    out.extend_from_slice(&((bytes + 36) as u32).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt \x10\0\0\0\x01\0\x01\0");
    out.extend_from_slice(&16_000u32.to_le_bytes());
    out.extend_from_slice(&32_000u32.to_le_bytes());
    out.extend_from_slice(b"\x02\0\x10\0data");
    out.extend_from_slice(&(bytes as u32).to_le_bytes());
    out.extend((0..bytes).map(|i| (i % 251) as u8));
    out.into()
}

// Fixture writer intentionally does not call the production protobuf/framing code.
fn text_frame(text: &str, kind: u8) -> Bytes {
    let mut message = vec![10];
    let mut length = text.len();
    loop {
        let next = length >> 7;
        message.push((length & 127) as u8 | if next > 0 { 128 } else { 0 });
        length = next;
        if length == 0 {
            break;
        }
    }
    message.extend_from_slice(text.as_bytes());
    message.extend_from_slice(&[24, kind]);
    let mut out = vec![0];
    out.extend_from_slice(&(message.len() as u32).to_be_bytes());
    out.extend(message);
    out.into()
}

fn end_frame(value: Value) -> Bytes {
    let bytes = value.to_string();
    let mut out = vec![2];
    out.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
    out.extend_from_slice(bytes.as_bytes());
    out.into()
}

async fn worker(
    mut body: Body,
    tx: mpsc::Sender<Result<Bytes, io::Error>>,
    observed: Arc<Observed>,
    mode: &'static str,
) {
    let _signal = Signal(observed.clone());
    let start = Instant::now();
    let mut pending = Vec::new();
    let mut audio_count = 0;
    loop {
        let data = match body.frame().await {
            Some(Ok(frame)) => match frame.into_data() {
                Ok(data) => data,
                Err(_) => continue,
            },
            _ => break,
        };
        pending.extend_from_slice(&data);
        while pending.len() >= 5 {
            assert_eq!(pending[0], 0);
            let len = u32::from_be_bytes(pending[1..5].try_into().unwrap()) as usize;
            if pending.len() < len + 5 {
                break;
            }
            let message = pending[5..5 + len].to_vec();
            pending.drain(..5 + len);
            observed.frames.lock().unwrap().push(message.clone());
            if message.first() == Some(&0x12) {
                audio_count += 1;
                observed.audio_times.lock().unwrap().push(start.elapsed());
                observed.first_audio.notify_one();
                if audio_count == 1 {
                    let reply = if mode == "duplex" {
                        text_frame(&"x".repeat(130_000), 1)
                    } else {
                        text_frame("完整输入之后的末尾 coffee", 1)
                    };
                    for chunk in reply.chunks(1024) {
                        if tx.send(Ok(Bytes::copy_from_slice(chunk))).await.is_err() {
                            return;
                        }
                    }
                    if mode == "cancel" {
                        tx.closed().await;
                        return;
                    }
                    if mode == "early_end" {
                        let _ = tx.send(Ok(end_frame(json!({})))).await;
                        return;
                    }
                }
            }
        }
    }
    *observed.input_closed.lock().unwrap() = true;
    {
        let frames = observed.frames.lock().unwrap();
        assert_eq!(frames.last().unwrap(), b"\x1a\x02\x08\x01");
    }
    if (mode == "duplex" || mode == "final_error")
        && tx
            .send(Ok(text_frame("完整输入和蓝色咖啡", 3)))
            .await
            .is_err()
    {
        return;
    }
    if mode == "fallback" {
        tx.closed().await;
        return;
    }
    if mode == "stalled_frame" {
        // Valid partial was already sent above. Keep a second frame incomplete
        // without EOF so the final-wait timer, rather than EOF, settles the call.
        if tx
            .send(Ok(Bytes::from_static(b"\0\0\0\0\x09\x0a")))
            .await
            .is_ok()
        {
            tx.closed().await;
        }
        return;
    }
    if mode == "truncated" {
        let _ = tx.send(Ok(Bytes::from_static(b"\0\0\0\0\x09\x0a"))).await;
        return;
    }
    let end = if mode == "rate_error" {
        json!({"error":{"code":"resource_exhausted","message":"private-upstream-secret"},"metadata":{"retry-after":["8"]}})
    } else if mode == "error" || mode == "final_error" {
        json!({"error":{"code":"unauthenticated","message":"private-upstream-secret"}})
    } else {
        json!({})
    };
    let _ = tx.send(Ok(end_frame(end))).await;
}

async fn fixture(mode: &'static str) -> Fixture {
    let observed = Arc::new(Observed::default());
    let workers: Arc<Mutex<Vec<tokio::task::JoinHandle<()>>>> = Arc::default();
    let capture = observed.clone();
    let tasks = workers.clone();
    let app = Router::new().route(
        PATH,
        post(move |request: Request<Body>| {
            let capture = capture.clone();
            let tasks = tasks.clone();
            async move {
                assert_eq!(request.version(), Version::HTTP_2);
                assert_eq!(request.uri().path_and_query().unwrap().as_str(), PATH);
                *capture.headers.lock().unwrap() = request.headers().clone();
                let mut response = if matches!(mode, "401" | "429") {
                    let mut response =
                        axum::http::Response::new(Body::from("private-upstream-secret"));
                    *response.status_mut() = if mode == "401" {
                        StatusCode::UNAUTHORIZED
                    } else {
                        StatusCode::TOO_MANY_REQUESTS
                    };
                    response
                } else {
                    let (tx, rx) = mpsc::channel(2);
                    tasks.lock().unwrap().push(tokio::spawn(worker(
                        request.into_body(),
                        tx,
                        capture,
                        mode,
                    )));
                    axum::http::Response::new(Body::from_stream(ReceiverStream::new(rx)))
                };
                response.headers_mut().insert(
                    header::CONTENT_TYPE,
                    HeaderValue::from_static("application/connect+proto"),
                );
                response
                    .headers_mut()
                    .insert(header::RETRY_AFTER, HeaderValue::from_static("7"));
                response
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    Fixture {
        credentials: json!({"accessToken":"fixture-secret","teamId":42,"_voice_test_endpoint":base,"baseUrl":"https://must-not-use.invalid","language":"en"}),
        observed,
        server,
        workers,
    }
}

#[tokio::test]
async fn native_h2_is_duplex_paced_and_sends_exact_config_audio_stop() {
    let fixture = fixture("duplex").await;
    let audio = wav(70_400);
    let text = transcribe(
        &client(),
        &fixture.credentials,
        &audio,
        "voice.wav",
        "audio/wav",
    )
    .await
    .unwrap();
    assert_eq!(text, "完整输入和蓝色咖啡");
    assert!(*fixture.observed.input_closed.lock().unwrap());
    let headers = fixture.observed.headers.lock().unwrap();
    assert_eq!(headers[header::AUTHORIZATION], "Bearer fixture-secret");
    assert_eq!(headers["x-cursor-team-id"], "42");
    assert_eq!(headers["x-cursor-client-type"], "ide");
    assert_eq!(headers["x-ghost-mode"], "true");
    assert!(!headers.contains_key("x-cursor-checksum"));
    drop(headers);
    let frames = fixture.observed.frames.lock().unwrap();
    assert_eq!(frames[0], b"\x0a\0");
    assert_eq!(frames.last().unwrap(), b"\x1a\x02\x08\x01");
    // First 640-byte nested PCM message: tag/lengths are fixed independent vectors.
    assert_eq!(&frames[1][..6], b"\x12\x83\x05\x0a\x80\x05");
    assert_eq!(&frames[1][6..], &audio[44..684]);
    assert_eq!(frames.len(), 112);
    drop(frames);
    // The span, not absolute times: the fixture's clock starts after the request reaches it.
    let times = fixture.observed.audio_times.lock().unwrap();
    assert!(*times.last().unwrap() - times[0] >= Duration::from_millis(2100));
}

#[tokio::test]
async fn native_partial_fallback_requires_completed_input_and_no_service_error() {
    let partial = fixture("partial").await;
    assert_eq!(
        transcribe(
            &client(),
            &partial.credentials,
            &wav(640),
            "voice.wav",
            "audio/wav"
        )
        .await
        .unwrap(),
        "完整输入之后的末尾 coffee"
    );
    for mode in ["error", "final_error", "truncated", "early_end"] {
        let fixture = fixture(mode).await;
        let error = transcribe(
            &client(),
            &fixture.credentials,
            &wav(6400),
            "voice.wav",
            "audio/wav",
        )
        .await
        .unwrap_err();
        assert!(!error.to_string().contains("private-upstream-secret"));
        assert!(!error.to_string().contains("coffee"));
        assert_eq!(
            error.status,
            if matches!(mode, "truncated" | "early_end") {
                StatusCode::BAD_GATEWAY
            } else {
                StatusCode::UNAUTHORIZED
            }
        );
    }
}

#[tokio::test]
async fn http_auth_and_rate_errors_keep_status_and_retry_after() {
    for (mode, status) in [
        ("401", StatusCode::UNAUTHORIZED),
        ("429", StatusCode::TOO_MANY_REQUESTS),
    ] {
        let fixture = fixture(mode).await;
        let error = transcribe(
            &client(),
            &fixture.credentials,
            &wav(640),
            "voice.wav",
            "audio/wav",
        )
        .await
        .unwrap_err();
        assert_eq!(error.status, status);
        assert_eq!(error.retry_after.unwrap(), "7");
        assert!(!error.message.contains("fixture-secret"));
    }
    let fixture = fixture("rate_error").await;
    let error = transcribe(
        &client(),
        &fixture.credentials,
        &wav(640),
        "voice.wav",
        "audio/wav",
    )
    .await
    .unwrap_err();
    assert_eq!(error.status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(error.retry_after.unwrap(), "8");
}

#[tokio::test]
async fn native_final_wait_allows_partial_only_after_the_complete_stop_body() {
    let fixture = fixture("fallback").await;
    let started = Instant::now();
    let text = transcribe(
        &client(),
        &fixture.credentials,
        &wav(640),
        "voice.wav",
        "audio/wav",
    )
    .await
    .unwrap();
    assert_eq!(text, "完整输入之后的末尾 coffee");
    assert!(*fixture.observed.input_closed.lock().unwrap());
    assert!(started.elapsed() >= FINAL_WAIT);
}

#[tokio::test]
async fn native_final_wait_rejects_a_stalled_incomplete_frame_after_valid_partial() {
    let fixture = fixture("stalled_frame").await;
    let started = Instant::now();
    let error = transcribe(
        &client(),
        &fixture.credentials,
        &wav(640),
        "voice.wav",
        "audio/wav",
    )
    .await
    .unwrap_err();
    assert_eq!(error.status, StatusCode::GATEWAY_TIMEOUT);
    assert!(*fixture.observed.input_closed.lock().unwrap());
    assert!(started.elapsed() >= FINAL_WAIT);
    assert!(!error.to_string().contains("coffee"));
    assert!(!error.to_string().contains("完整输入"));
}

#[tokio::test]
async fn cancelling_drops_upload_and_response_without_a_detached_sender() {
    let fixture = fixture("cancel").await;
    let credentials = fixture.credentials.clone();
    let observed = fixture.observed.clone();
    let call = tokio::spawn(async move {
        transcribe(
            &client(),
            &credentials,
            &wav(320_000),
            "voice.wav",
            "audio/wav",
        )
        .await
    });
    tokio::time::timeout(Duration::from_secs(2), observed.first_audio.notified())
        .await
        .unwrap();
    call.abort();
    let _ = call.await;
    tokio::time::timeout(Duration::from_secs(2), observed.dropped.notified())
        .await
        .unwrap();
    assert!(!*observed.input_closed.lock().unwrap());
}

#[test]
fn checksum_vector_and_invalid_audio_are_local() {
    assert_eq!(checksum("machine", 0x01020304), "pqOkqa6vmachine");
    assert!(validated_pcm(&wav(0), "audio/wav").is_err());
    assert!(validated_pcm(&wav(2), "audio/mp3").is_err());
    let mut audio = wav(2).to_vec();
    audio[22] = 2;
    assert!(validated_pcm(&audio, "audio/wav").is_err());
    assert!(fields(b"\x0a\x80").is_err());
    assert!(fields(b"\0").is_err());
}
