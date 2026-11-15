use super::*;
use axum::{http::HeaderMap, routing::post, Router};
use base64::Engine;
use serde_json::json;
use std::sync::{Arc, Mutex};
use tokio_tungstenite::tungstenite::handshake::server::{ErrorResponse, Request, Response};

#[derive(Default)]
struct Record {
    grants: Vec<(HeaderMap, Bytes)>,
    uri: String,
    ws_headers: HeaderMap,
    frames: Vec<Vec<u8>>,
}

struct Server {
    http: String,
    ws: String,
    record: Arc<Mutex<Record>>,
    tasks: Vec<tokio::task::JoinHandle<()>>,
}

impl Drop for Server {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
    }
}

fn transcript(text: &str, partial: bool) -> Vec<u8> {
    response_event(
        json!({"Transcript":{"Results":[{"IsPartial":partial,"Alternatives":[{"Transcript":text}]}]}}),
    )
}

fn response_event(value: Value) -> Vec<u8> {
    event_frame(
        &[
            (":message-type", "event"),
            (":event-type", "TranscriptEvent"),
            (":content-type", "application/json"),
        ],
        value.to_string().as_bytes(),
    )
}

#[allow(clippy::result_large_err)]
async fn server(mode: &'static str) -> Server {
    let record: Arc<Mutex<Record>> = Arc::default();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let ws = format!("ws://{}{PATH}", listener.local_addr().unwrap());
    let capture = record.clone();
    let ws_task = tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.unwrap();
        let observed = capture.clone();
        let socket =
            tokio_tungstenite::accept_hdr_async(tcp, move |req: &Request, res: Response| {
                let mut record = observed.lock().unwrap();
                record.uri = req.uri().to_string();
                record.ws_headers = req.headers().clone();
                if mode == "ws401" || mode == "ws429" {
                    let mut error =
                        ErrorResponse::new(Some("private STS upstream error".to_owned()));
                    *error.status_mut() = if mode == "ws401" {
                        StatusCode::UNAUTHORIZED
                    } else {
                        StatusCode::TOO_MANY_REQUESTS
                    };
                    error
                        .headers_mut()
                        .insert("retry-after", "7".parse().unwrap());
                    return Err(error);
                }
                Ok(res)
            })
            .await;
        let Ok(mut socket) = socket else { return };
        let mut chunks = 0;
        while let Some(Ok(message)) = socket.next().await {
            let Message::Binary(bytes) = message else {
                continue;
            };
            capture.lock().unwrap().frames.push(bytes.to_vec());
            if bytes.is_empty() {
                if mode == "hold" {
                    continue;
                }
                if mode == "partial_close" {
                    socket.close(None).await.unwrap();
                    break;
                }
                if mode == "malformed" {
                    socket
                        .send(Message::Binary(
                            response_event(json!({"Transcript":{}})).into(),
                        ))
                        .await
                        .unwrap();
                    break;
                }
                if mode == "silence" {
                    socket
                        .send(Message::Binary(
                            response_event(json!({"Transcript":{"Results":[]}})).into(),
                        ))
                        .await
                        .unwrap();
                } else {
                    tokio::time::sleep(Duration::from_millis(if mode == "late" {
                        1200
                    } else {
                        20
                    }))
                    .await;
                    let reply = transcript("最终", false);
                    // Exercise EventStream boundaries across WebSocket messages.
                    socket
                        .send(Message::Binary(reply[..3].to_vec().into()))
                        .await
                        .unwrap();
                    socket
                        .send(Message::Binary(reply[3..].to_vec().into()))
                        .await
                        .unwrap();
                }
                socket.close(None).await.unwrap();
                break;
            }
            let (_, payload) = decode_event(&bytes).unwrap();
            if payload.is_empty() {
                continue;
            }
            chunks += 1;
            if mode == "early_close" {
                socket.close(None).await.unwrap();
                break;
            }
            if mode == "exception401" || mode == "exception429" {
                let kind = if mode == "exception401" {
                    "UnrecognizedClientException"
                } else {
                    "LimitExceededException"
                };
                socket
                    .send(Message::Binary(
                        event_frame(
                            &[(":message-type", "exception"), (":exception-type", kind)],
                            b"private secret upstream",
                        )
                        .into(),
                    ))
                    .await
                    .unwrap();
                break;
            }
            if mode == "corrupt" {
                let mut reply = transcript("private", false);
                reply[8] ^= 1;
                socket.send(Message::Binary(reply.into())).await.unwrap();
                break;
            }
            if mode == "truncated" {
                socket
                    .send(Message::Binary(vec![0, 0, 1].into()))
                    .await
                    .unwrap();
                continue;
            }
            if mode != "silence" {
                let reply = if chunks == 1 {
                    transcript("草稿", true)
                } else {
                    transcript("早期", false)
                };
                socket.send(Message::Binary(reply.into())).await.unwrap();
                socket.send(Message::Ping(vec![1, 2].into())).await.unwrap();
            }
        }
    });
    let capture = record.clone();
    let app = Router::new().route("/icube/api/v1/asr/get/b", post(move |headers: HeaderMap, body: Bytes| {
        capture.lock().unwrap().grants.push((headers, body));
        async move {
            let status = match mode { "grant401" => StatusCode::UNAUTHORIZED, "grant429" => StatusCode::TOO_MANY_REQUESTS, _ => StatusCode::OK };
            let payload = match mode {
                "json" => "private invalid upstream JSON".to_owned(),
                "grant_false" => json!({"success":false,"message":"private upstream token"}).to_string(),
                "grant_missing" => json!({"success":true,"data":{"accessKeyId":"fixture-key"}}).to_string(),
                "grant_empty" => json!({"success":true,"data":{"accessKeyId":"fixture-key","secretAccessKey":"fixture-secret","sessionToken":""}}).to_string(),
                _ => json!({"success":true,"data":{"accessKeyId":"fixture-key","secretAccessKey":"fixture-secret","sessionToken":"fixture+/=token","region":"wrong-untrusted-region"}}).to_string(),
            };
            (status, [("content-type", "application/json"), ("retry-after", "7")], payload)
        }
    }));
    let http_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let http = format!("http://{}", http_listener.local_addr().unwrap());
    let http_task = tokio::spawn(async move {
        axum::serve(http_listener, app).await.unwrap();
    });
    Server {
        http,
        ws,
        record,
        tasks: vec![ws_task, http_task],
    }
}

fn credentials(server: &Server) -> Value {
    json!({"edition":"trae","region":"SG","accountScope":"marscode","accessToken":"fixture-account-token","_voice_test_endpoint":server.http,"_voice_test_ws_endpoint":server.ws})
}

fn wav(pcm: &[u8]) -> Bytes {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + pcm.len() as u32).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&16000_u32.to_le_bytes());
    bytes.extend_from_slice(&32000_u32.to_le_bytes());
    bytes.extend_from_slice(&2_u16.to_le_bytes());
    bytes.extend_from_slice(&16_u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&(pcm.len() as u32).to_le_bytes());
    bytes.extend_from_slice(pcm);
    bytes.into()
}

#[tokio::test]
async fn native_grant_signed_query_and_audio_finish_are_preserved() {
    let fixture = server("normal").await;
    let pcm = (0..6400).map(|i| (i % 251) as u8).collect::<Vec<_>>();
    let text = transcribe(
        &reqwest::Client::new(),
        &credentials(&fixture),
        &wav(&pcm),
        "voice.wav",
        "audio/wav",
    )
    .await
    .unwrap();
    assert_eq!(text, "早期最终");
    let record = fixture.record.lock().unwrap();
    assert_eq!(record.grants.len(), 1);
    let (headers, body) = &record.grants[0];
    assert_eq!(headers["x-icube-token"], "fixture-account-token");
    assert_eq!(headers["content-type"], "application/json");
    assert!(!headers.contains_key("authorization"));
    assert!(body.is_empty());
    assert!(!record.ws_headers.contains_key("x-icube-token"));
    assert!(!record.ws_headers.contains_key("authorization"));
    let query: BTreeMap<_, _> = url::Url::parse(&format!("ws://localhost{}", record.uri))
        .unwrap()
        .query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    assert_eq!(query["X-Amz-Security-Token"], "fixture+/=token");
    assert!(record.uri.contains("fixture%2B%2F%3Dtoken"));
    assert!(!record.uri.contains("%253D"));
    assert!(query["X-Amz-Credential"].ends_with("/ap-southeast-1/transcribe/aws4_request"));
    assert_eq!(query["X-Amz-Expires"], "60");
    assert_eq!(query["X-Amz-SignedHeaders"], "host");
    assert_eq!(query["sample-rate"], "16000");
    assert_eq!(query["media-encoding"], "pcm");
    assert_eq!(query["identify-language"], "true");
    assert_eq!(query["language-options"], "en-US,zh-CN");
    assert_eq!(query["X-Amz-Signature"].len(), 64);
    uuid::Uuid::parse_str(&query["session-id"]).unwrap();
    assert_eq!(record.frames.len(), 4);
    let mut received = Vec::new();
    for frame in &record.frames[..3] {
        let (headers, bytes) = decode_event(frame).unwrap();
        assert_eq!(headers[":event-type"], "AudioEvent");
        assert_eq!(headers[":content-type"], "application/octet-stream");
        assert_eq!(headers[":message-type"], "event");
        received.extend_from_slice(bytes);
    }
    assert_eq!(received, pcm);
    assert!(decode_event(&record.frames[2]).unwrap().1.is_empty());
    assert!(record.frames[3].is_empty());
}

#[test]
fn regions_follow_saved_store_region_and_never_text_base_or_grant_region() {
    for (region, host, aws) in [
        ("SG", "https://icube16-normal-sg.trae.ai", "ap-southeast-1"),
        ("US", "https://icube-normal.trae.ai", "us-west-2"),
        ("USTTP", "https://icube-normal.traeapi.us", "us-west-2"),
    ] {
        assert_eq!(
            endpoints(&json!({"edition":"trae","region":"SG","storeRegion":region})).unwrap(),
            (host, aws)
        );
        let grant = AwsCredentials {
            access_key_id: "key".into(),
            secret_access_key: "secret".into(),
            session_token: "token".into(),
        };
        let url = url::Url::parse(&signed_url(&grant, aws, "20261026T000000Z").unwrap()).unwrap();
        assert_eq!(url.scheme(), "wss");
        assert_eq!(
            url.host_str().unwrap(),
            format!("transcribestreaming.{aws}.amazonaws.com")
        );
        assert_eq!(url.port(), Some(8443));
        assert_eq!(url.path(), PATH);
    }
}

#[tokio::test]
async fn preflight_rejects_wrong_editions_scopes_regions_and_audio_without_network() {
    let fixture = server("normal").await;
    let good = credentials(&fixture);
    for (key, value) in [
        ("edition", json!("traecn")),
        ("region", json!("CN")),
        ("region", json!("unknown")),
        ("storeRegion", json!("CN")),
        ("storeRegion", json!("")),
        ("storeRegion", json!(1)),
        ("accountScope", json!("saas")),
        ("accountScope", json!("bytedance")),
        ("accountScope", json!("unknown")),
        ("accountScope", json!(1)),
        ("accessToken", json!("")),
        ("accessToken", json!("bad\r\nheader")),
    ] {
        let mut account = good.clone();
        account[key] = value;
        assert_eq!(
            transcribe(
                &reqwest::Client::new(),
                &account,
                &wav(&[0, 0]),
                "voice.wav",
                "audio/wav"
            )
            .await
            .unwrap_err()
            .status,
            StatusCode::UNAUTHORIZED
        );
    }
    for (audio, mime) in [
        (wav(&[0, 0]), "audio/mpeg"),
        (Bytes::from_static(b"invalid"), "audio/wav"),
    ] {
        assert_eq!(
            transcribe(&reqwest::Client::new(), &good, &audio, "voice", mime)
                .await
                .unwrap_err()
                .status,
            StatusCode::UNSUPPORTED_MEDIA_TYPE
        );
    }
    assert!(fixture.record.lock().unwrap().grants.is_empty());
    assert!(fixture.record.lock().unwrap().uri.is_empty());
}

#[tokio::test]
async fn grant_and_handshake_failures_keep_status_retry_after_and_redact_secrets() {
    for (mode, status) in [
        ("grant401", 401),
        ("grant429", 429),
        ("ws401", 401),
        ("ws429", 429),
    ] {
        let fixture = server(mode).await;
        let error = transcribe(
            &reqwest::Client::new(),
            &credentials(&fixture),
            &wav(&[0, 0]),
            "voice",
            "audio/wav",
        )
        .await
        .unwrap_err();
        assert_eq!(error.status.as_u16(), status);
        assert_eq!(error.retry_after.unwrap(), "7");
        assert!(!error.message.contains("private"));
        assert!(!error.message.contains("fixture"));
        assert_eq!(fixture.record.lock().unwrap().grants.len(), 1);
        if mode.starts_with("grant") {
            assert!(fixture.record.lock().unwrap().uri.is_empty());
        }
    }
}

#[tokio::test]
async fn malformed_grants_frames_and_early_termination_do_not_return_text() {
    for mode in [
        "json",
        "grant_false",
        "grant_missing",
        "grant_empty",
        "malformed",
        "corrupt",
        "early_close",
        "partial_close",
        "truncated",
    ] {
        let fixture = server(mode).await;
        let error = transcribe(
            &reqwest::Client::new(),
            &credentials(&fixture),
            &wav(&[0, 0]),
            "voice",
            "audio/wav",
        )
        .await
        .unwrap_err();
        assert_eq!(error.status, StatusCode::BAD_GATEWAY, "{mode}");
        assert!(!error.message.contains("private"));
    }
}

#[tokio::test]
async fn exception_metadata_is_mapped_without_exposing_payload() {
    for (mode, status) in [("exception401", 401), ("exception429", 429)] {
        let fixture = server(mode).await;
        let error = transcribe(
            &reqwest::Client::new(),
            &credentials(&fixture),
            &wav(&[0, 0]),
            "voice",
            "audio/wav",
        )
        .await
        .unwrap_err();
        assert_eq!(error.status.as_u16(), status);
        assert!(!error.message.contains("private"));
        assert!(error.retry_after.is_none());
    }
}

#[tokio::test]
async fn waits_for_late_final_and_preserves_empty_success() {
    for (mode, expected) in [("late", "最终"), ("silence", "")] {
        let fixture = server(mode).await;
        assert_eq!(
            transcribe(
                &reqwest::Client::new(),
                &credentials(&fixture),
                &wav(&[0, 0]),
                "voice",
                "audio/wav"
            )
            .await
            .unwrap(),
            expected
        );
    }
    let fixture = server("hold").await;
    let mut account = credentials(&fixture);
    account["_voice_test_finish_wait_ms"] = json!(40);
    assert_eq!(
        transcribe(
            &reqwest::Client::new(),
            &account,
            &wav(&[0, 0]),
            "voice",
            "audio/wav"
        )
        .await
        .unwrap_err()
        .status,
        StatusCode::GATEWAY_TIMEOUT
    );
}

#[test]
fn ieee_crc_and_official_aws_eventstream_vector_are_independent_controls() {
    assert_eq!(crc32(b"123456789"), 0xcbf4_3926);
    // Public AWS Transcribe documentation end frame, independent of our encoder.
    let vector=base64::engine::general_purpose::STANDARD.decode("AAAAUwAAAEP1RHpYBTpkYXRlCAAAAWiXUkMLEDpjaHVuay1zaWduYXR1cmUGACCt6Zy+uymwEK2SrLp/zVBI5eGn83jdBwCaRUBJA+eaDafqjqI=").unwrap();
    assert!(decode_event(&vector).unwrap().1.is_empty());
    for offset in [0, 4, 8, 12, vector.len() - 1] {
        let mut bad = vector.clone();
        bad[offset] ^= 1;
        assert!(decode_event(&bad).is_err());
    }
    assert!(decode_event(&vector[..15]).is_err());
}

#[test]
fn official_smithy_presign_signature_is_an_independent_control() {
    // smithy-typescript SignatureV4.spec.ts: presign request without body.
    let grant = AwsCredentials {
        access_key_id: "foo".into(),
        secret_access_key: "bar".into(),
        session_token: String::new(),
    };
    let query = BTreeMap::from([
        ("X-Amz-Algorithm", "AWS4-HMAC-SHA256".to_owned()),
        (
            "X-Amz-Credential",
            "foo/20000101/us-bar-1/foo/aws4_request".to_owned(),
        ),
        ("X-Amz-Date", "20000101T000000Z".to_owned()),
        ("X-Amz-Expires", "1800".to_owned()),
        ("X-Amz-SignedHeaders", "host".to_owned()),
    ]);
    let canonical = format!(
        "POST\n/\n{}\nhost:foo.us-bar-1.amazonaws.com\n\nhost\n{}",
        encode_query(&query),
        hex::encode(Sha256::digest([]))
    );
    assert_eq!(
        signature(&grant, "20000101T000000Z", "us-bar-1", "foo", &canonical).unwrap(),
        "46f0091f3e84cbd4552a184f43830a4f8b42fd18ceaefcdc2c225be1efd9e00e"
    );
    assert_eq!(
        encode("+/= !'()*~中"),
        "%2B%2F%3D%20%21%27%28%29%2A~%E4%B8%AD"
    );
}
