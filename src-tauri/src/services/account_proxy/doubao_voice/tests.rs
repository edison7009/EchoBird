use super::*;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex,
};
use tokio::sync::Notify;
use tokio_tungstenite::{
    accept_hdr_async,
    tungstenite::handshake::server::{ErrorResponse, Request, Response},
};

#[derive(Default)]
struct Observed {
    connections: AtomicUsize,
    frames: Mutex<Vec<Vec<u8>>>,
    headers: Mutex<axum::http::HeaderMap>,
    audio: Mutex<Vec<u8>>,
    times: Mutex<Vec<Duration>>,
    first_audio: Notify,
    dropped: Notify,
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
    worker: tokio::task::JoinHandle<()>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.worker.abort();
    }
}

fn pb(number: u8, data: &[u8]) -> Vec<u8> {
    let mut out = vec![number * 8 + 2];
    let mut length = data.len();
    loop {
        let rest = length >> 7;
        out.push((length & 127) as u8 | if rest == 0 { 0 } else { 128 });
        length = rest;
        if length == 0 {
            break;
        }
    }
    out.extend_from_slice(data);
    out
}
fn event(name: &str, payload: Option<Value>, code: u64) -> WsMessage {
    let mut out = pb(1, b"fixture-task");
    out.extend(pb(4, name.as_bytes()));
    out.push(40);
    let mut code = code;
    loop {
        let rest = code >> 7;
        out.push((code & 127) as u8 | if rest == 0 { 0 } else { 128 });
        code = rest;
        if code == 0 {
            break;
        }
    }
    out.extend(pb(6, b"private-upstream-status"));
    if let Some(payload) = payload {
        out.extend(pb(7, payload.to_string().as_bytes()));
    }
    WsMessage::Binary(out.into())
}
fn asr(text: &str, interim: bool) -> WsMessage {
    event(
        "ASRResponse",
        Some(json!({"results":[{"text":text,"is_interim":interim}]})),
        0,
    )
}
fn text(fields: &[Field<'_>], number: u32) -> String {
    fields
        .iter()
        .find_map(|field| match field {
            Field::Bytes(n, value) if *n == number => {
                Some(String::from_utf8(value.to_vec()).unwrap())
            }
            _ => None,
        })
        .unwrap_or_default()
}

async fn fixture(mode: &'static str, enterprise: &'static str) -> Fixture {
    let observed = Arc::new(Observed::default());
    let captured = observed.clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("ws://{}", listener.local_addr().unwrap());
    let worker = tokio::spawn(async move {
        let _signal = Signal(captured.clone());
        let (stream, _) = listener.accept().await.unwrap();
        captured.connections.fetch_add(1, Ordering::SeqCst);
        let capture = captured.clone();
        let socket = accept_hdr_async(stream, move |request: &Request, response: Response| {
            *capture.headers.lock().unwrap() = request.headers().clone();
            let url = url::Url::parse(&format!("ws://fixture{}", request.uri())).unwrap();
            assert_eq!(url.path(), "/api/v2/sami/voicegenie");
            let params: std::collections::BTreeMap<_, _> = url.query_pairs().into_owned().collect();
            assert_eq!(params["namespace"], "VoiceGenie");
            assert_eq!(
                params["api_app_key"],
                if enterprise == "102" {
                    FEISHU_APPKEY
                } else {
                    NORMAL_APPKEY
                }
            );
            assert!(!params.contains_key("uid"));
            if matches!(mode, "http401" | "http429") {
                let mut error = ErrorResponse::new(Some("private-upstream-status".into()));
                *error.status_mut() = if mode == "http401" {
                    StatusCode::UNAUTHORIZED
                } else {
                    StatusCode::TOO_MANY_REQUESTS
                };
                error
                    .headers_mut()
                    .insert(header::RETRY_AFTER, HeaderValue::from_static("7"));
                return Err(error);
            }
            Ok(response)
        })
        .await;
        let Ok(mut socket) = socket else { return };
        let started = Instant::now();
        let mut next_seq = 1;
        while let Some(Ok(message)) = socket.next().await {
            let WsMessage::Binary(bytes) = message else {
                break;
            };
            captured.frames.lock().unwrap().push(bytes.to_vec());
            let fields = wire::fields(&bytes).unwrap();
            let name = text(&fields, 5);
            assert_eq!(text(&fields, 3), "VoiceGenie");
            assert!(!fields
                .iter()
                .any(|field| matches!(field, Field::Bytes(1, _))));
            match name.as_str() {
                "StartTask" => {
                    assert_eq!(text(&fields, 6), "{}");
                    assert_eq!(text(&fields, 8), "");
                    if mode == "business" {
                        let _ = socket.send(event("", None, 40200001)).await;
                        return;
                    }
                    if mode == "malformed" {
                        let _ = socket
                            .send(WsMessage::Binary(Bytes::from_static(b"\x22\xff")))
                            .await;
                        return;
                    }
                    if mode == "oversize" {
                        let _ = socket
                            .send(WsMessage::Binary(vec![0; MAX_MESSAGE + 1].into()))
                            .await;
                        return;
                    }
                    if socket
                        .send(event("TaskStarted", None, 20000000))
                        .await
                        .is_err()
                    {
                        return;
                    }
                }
                "StartSession" => {
                    assert_eq!(text(&fields, 8), "fixture-task");
                    let payload: Value = serde_json::from_str(&text(&fields, 6)).unwrap();
                    assert_eq!(payload["chat"]["uid"], "saved-uid");
                    assert_eq!(payload["extra"]["uid"], "saved-uid");
                    assert_eq!(payload["request_type"], 3);
                    assert_eq!(
                        payload["asr"]["audio_info"],
                        json!({"channel":1,"format":"pcm","sample_rate":16000})
                    );
                    if socket.send(event("SessionStarted", None, 0)).await.is_err() {
                        return;
                    }
                }
                "TaskRequest" => {
                    assert_eq!(text(&fields, 6), "{}");
                    assert_eq!(text(&fields, 8), "fixture-task");
                    assert_eq!(wire::number(&fields, 9), Some(next_seq));
                    next_seq += 1;
                    let audio = fields
                        .iter()
                        .find_map(|field| match field {
                            Field::Bytes(7, data) => Some(*data),
                            _ => None,
                        })
                        .unwrap();
                    captured.audio.lock().unwrap().extend_from_slice(audio);
                    captured.times.lock().unwrap().push(started.elapsed());
                    captured.first_audio.notify_one();
                    if next_seq == 2 {
                        if socket.send(asr("private-interim", true)).await.is_err() {
                            return;
                        }
                        if mode == "cancel" {
                            while socket.next().await.is_some() {}
                            return;
                        }
                        if mode == "early_end" {
                            let _ = socket.send(event("ASREnded", None, 0)).await;
                            return;
                        }
                        if mode == "canceled" {
                            let _ = socket.send(event("SessionCanceled", None, 0)).await;
                            return;
                        }
                        if mode == "error_after_final" {
                            let _ = socket.send(asr("must-not-return", false)).await;
                            let _ = socket.send(event("SessionFailed", None, 40200001)).await;
                            return;
                        }
                    }
                }
                "EndASR" => {
                    if mode != "partial" && socket.send(asr("Hello ", false)).await.is_err() {
                        return;
                    }
                    if socket.send(event("ASREnded", None, 0)).await.is_err() {
                        return;
                    }
                }
                "FinishSession" => {
                    if mode != "partial" {
                        if socket.send(asr("蓝色咖啡", false)).await.is_err() {
                            return;
                        }
                        let _ = socket.send(asr("", false)).await;
                    }
                    let _ = socket.send(event("SessionFinished", None, 0)).await;
                    let _ = socket.next().await;
                    return;
                }
                "Ping" => {
                    let _ = socket.send(event("Pong", None, 0)).await;
                }
                _ => panic!("unexpected fixture event"),
            }
        }
    });
    Fixture {
        credentials: json!({"edition":"doubaowork","userId":"saved-uid","enterpriseType":enterprise,"cookieScope":SCOPE,"cookieHeader":"sessionid=scope-only","_voice_test_endpoint":endpoint,"baseUrl":"https://www.dola.com"}),
        observed,
        worker,
    }
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

#[tokio::test]
async fn native_protobuf_session_paced_pcm_and_tail_segments_are_complete() {
    let fixture = fixture("ok", "0").await;
    let audio = wav(1920);
    let text = transcribe(
        &reqwest::Client::new(),
        &fixture.credentials,
        &audio,
        "voice.wav",
        "audio/wav",
    )
    .await
    .unwrap();
    assert_eq!(text, "Hello 蓝色咖啡");
    let headers = fixture.observed.headers.lock().unwrap();
    assert_eq!(headers[header::COOKIE], "sessionid=scope-only");
    assert!(!headers.contains_key(header::AUTHORIZATION));
    assert!(!headers.contains_key(header::ORIGIN));
    drop(headers);
    let frames = fixture.observed.frames.lock().unwrap();
    assert_eq!(
        frames[0],
        b"\x12\x10GOqQpfo1fO7slHv8\x1a\x0aVoiceGenie\x2a\x09StartTask\x32\x02{}"
    );
    let mut expected =
        b"\x12\x10GOqQpfo1fO7slHv8\x1a\x0aVoiceGenie\x2a\x0bTaskRequest\x32\x02{}\x3a\x80\x05"
            .to_vec();
    expected.extend_from_slice(&audio[44..684]);
    expected.extend_from_slice(b"\x42\x0cfixture-task\x48\x01");
    assert_eq!(frames[2], expected);
    drop(frames);
    assert_eq!(*fixture.observed.audio.lock().unwrap(), audio[44..]);
    let times = fixture.observed.times.lock().unwrap();
    assert_eq!(times.len(), 3);
    assert!(times[0] >= Duration::from_millis(15));
    assert!(times[2] >= Duration::from_millis(55));
}

#[tokio::test]
async fn enterprise_102_uses_feishu_and_101_still_uses_normal_public_appkey() {
    for enterprise in ["102", "101"] {
        let fixture = fixture("ok", enterprise).await;
        assert_eq!(
            transcribe(
                &reqwest::Client::new(),
                &fixture.credentials,
                &wav(640),
                "voice.wav",
                "audio/wav"
            )
            .await
            .unwrap(),
            "Hello 蓝色咖啡"
        );
    }
}

#[tokio::test]
async fn handshake_and_business_errors_do_not_invent_http_402_or_return_text() {
    for (mode, status, retry, code) in [
        ("http401", StatusCode::UNAUTHORIZED, Some("7"), None),
        ("http429", StatusCode::TOO_MANY_REQUESTS, Some("7"), None),
        ("business", StatusCode::BAD_GATEWAY, None, Some("40200001")),
        (
            "error_after_final",
            StatusCode::BAD_GATEWAY,
            None,
            Some("40200001"),
        ),
        ("early_end", StatusCode::BAD_GATEWAY, None, None),
        ("canceled", StatusCode::BAD_GATEWAY, None, None),
        ("malformed", StatusCode::BAD_GATEWAY, None, None),
        ("oversize", StatusCode::BAD_GATEWAY, None, None),
        ("partial", StatusCode::BAD_GATEWAY, None, None),
    ] {
        let fixture = fixture(mode, "0").await;
        let error = transcribe(
            &reqwest::Client::new(),
            &fixture.credentials,
            &wav(1920),
            "voice.wav",
            "audio/wav",
        )
        .await
        .unwrap_err();
        assert_eq!(error.status, status);
        assert_eq!(
            error
                .retry_after
                .as_ref()
                .and_then(|value| value.to_str().ok()),
            retry
        );
        assert_eq!(error.error_code.as_deref(), code);
        assert!(!error.message.contains("private"));
        assert!(!error.message.contains("saved-uid"));
        assert!(!error.message.contains("must-not-return"));
    }
}

#[tokio::test]
async fn cancellation_drops_the_owned_websocket_and_stops_upload() {
    let fixture = fixture("cancel", "0").await;
    let observed = fixture.observed.clone();
    let credentials = fixture.credentials.clone();
    let call = tokio::spawn(async move {
        transcribe(
            &reqwest::Client::new(),
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
    assert!(observed.audio.lock().unwrap().len() < 320_000);
}

#[tokio::test]
async fn dola_legacy_www_credentials_and_bad_wav_never_open_a_socket() {
    let fixture = fixture("ok", "0").await;
    let mut dola = fixture.credentials.clone();
    dola["edition"] = json!("dola");
    dola["cookieHeader"] = json!("dola-secret");
    dola["userId"] = json!("dola-uid");
    let mut www = fixture.credentials.clone();
    www.as_object_mut().unwrap().remove("cookieScope");
    www["cookieHeader"] = json!("www-flat");
    for credentials in [dola, www] {
        assert_eq!(
            transcribe(
                &reqwest::Client::new(),
                &credentials,
                &wav(2),
                "voice.wav",
                "audio/wav"
            )
            .await
            .unwrap_err()
            .status,
            StatusCode::UNAUTHORIZED
        );
    }
    assert_eq!(
        transcribe(
            &reqwest::Client::new(),
            &Value::Null,
            &wav(0),
            "voice.wav",
            "audio/wav"
        )
        .await
        .unwrap_err()
        .status,
        StatusCode::BAD_REQUEST
    );
    assert!(validated_pcm(&wav(2), "audio/mp3").is_err());
    assert!(decode(b"\x22\xff").is_err());
    assert_eq!(fixture.observed.connections.load(Ordering::SeqCst), 0);
}

#[test]
fn transcript_precedence_replaces_interim_and_appends_final_without_invented_spaces() {
    let mut transcript = Transcript::default();
    transcript
        .apply(r#"{"results":[{"text":"first temporary","is_interim":true,"is_final":true}]}"#)
        .unwrap();
    transcript
        .apply(r#"{"results":[{"text":"second temporary","is_interim":true}]}"#)
        .unwrap();
    assert_eq!(transcript.pending, "second temporary");
    assert_eq!(transcript.confirmed, "");
    transcript
        .apply(r#"{"results":[{"text":"Hello ","is_interim":false,"is_final":false}]}"#)
        .unwrap();
    transcript
        .apply(r#"{"results":[{"text":"咖啡","is_final":true}]}"#)
        .unwrap();
    transcript
        .apply(r#"{"results":[{"text":"","isFinal":true}]}"#)
        .unwrap();
    assert_eq!(transcript.confirmed, "Hello 咖啡");
    assert_eq!(transcript.pending, "");
}
