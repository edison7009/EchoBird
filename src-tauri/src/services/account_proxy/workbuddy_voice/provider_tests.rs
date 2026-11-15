use super::*;
use axum::{extract::Multipart, routing::post, Json, Router};
use serde_json::json;
use std::sync::{Arc, Mutex};

type Uploads = Arc<Mutex<Vec<(HeaderMap, String, String, Bytes)>>>;

struct Server {
    url: String,
    uploads: Uploads,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn server(status: StatusCode, result: Value) -> Server {
    let uploads: Uploads = Arc::default();
    let captured = uploads.clone();
    let app = Router::new().route(
        "/agenttool/v1/asr",
        post(move |headers: HeaderMap, mut fields: Multipart| {
            let captured = captured.clone();
            let result = result.clone();
            async move {
                let field = fields.next_field().await.unwrap().unwrap();
                assert_eq!(field.name(), Some("audio"));
                let filename = field.file_name().unwrap().to_owned();
                let mime = field.content_type().unwrap().to_owned();
                let bytes = field.bytes().await.unwrap();
                assert!(fields.next_field().await.unwrap().is_none());
                captured
                    .lock()
                    .unwrap()
                    .push((headers, filename, mime, bytes));
                (status, [(header::RETRY_AFTER, "12")], Json(result))
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    Server { url, uploads, task }
}

fn credentials(server: &Server) -> Value {
    json!({"accessToken":"selected-token","uid":"selected-user",
        "domain":"www.workbuddy.ai","enterpriseId":"selected-enterprise",
        "baseUrl":"http://untrusted-text-endpoint.invalid",
        "_voice_test_endpoint":server.url})
}

#[test]
fn fixes_native_origins_by_provider() {
    assert_eq!(
        endpoint(AccountProvider::WorkBuddy).unwrap(),
        "https://www.workbuddy.cn"
    );
    assert_eq!(
        endpoint(AccountProvider::WorkBuddyAi).unwrap(),
        "https://www.workbuddy.ai"
    );
    assert_eq!(
        endpoint(AccountProvider::Codex).unwrap_err().status,
        StatusCode::BAD_REQUEST
    );
}

#[tokio::test]
async fn international_upload_preserves_audio_and_selected_account_headers() {
    let server = server(StatusCode::OK, json!({"code":0,"text":"完整转写"})).await;
    let audio = Bytes::from_static(b"RIFF\0\xff\x80WAVEselected");
    let result = transcribe_for_provider(
        &reqwest::Client::new(),
        AccountProvider::WorkBuddyAi,
        &credentials(&server),
        &audio,
        "selected.wav",
        "audio/wav",
    )
    .await
    .unwrap();
    assert_eq!(result, "完整转写");
    let uploads = server.uploads.lock().unwrap();
    assert_eq!(uploads.len(), 1);
    let (headers, filename, mime, received) = &uploads[0];
    assert_eq!(headers["authorization"], "Bearer selected-token");
    assert_eq!(headers["x-user-id"], "selected-user");
    assert_eq!(headers["x-domain"], "www.workbuddy.ai");
    assert_eq!(headers["x-enterprise-id"], "selected-enterprise");
    assert_eq!(headers["x-tenant-id"], "selected-enterprise");
    assert_eq!(filename, "selected.wav");
    assert_eq!(mime, "audio/wav");
    assert_eq!(received, &audio);
}

#[tokio::test]
async fn forwards_errors_without_repeating_or_exposing_upstream_content() {
    for (status, body, expected, code) in [
        (
            StatusCode::UNAUTHORIZED,
            json!({"secret":"hidden"}),
            StatusCode::UNAUTHORIZED,
            None,
        ),
        (
            StatusCode::TOO_MANY_REQUESTS,
            json!({"secret":"hidden"}),
            StatusCode::TOO_MANY_REQUESTS,
            None,
        ),
        (
            StatusCode::OK,
            json!({"code":51,"msg":"hidden"}),
            StatusCode::BAD_GATEWAY,
            Some("51"),
        ),
        (
            StatusCode::OK,
            json!({"code":0}),
            StatusCode::BAD_GATEWAY,
            None,
        ),
    ] {
        let server = server(status, body).await;
        let error = transcribe_for_provider(
            &reqwest::Client::new(),
            AccountProvider::WorkBuddyAi,
            &credentials(&server),
            &Bytes::from_static(b"audio"),
            "recording.wav",
            "audio/wav",
        )
        .await
        .unwrap_err();
        assert_eq!(error.status, expected);
        assert_eq!(error.error_code.as_deref(), code);
        assert!(!error.message.contains("hidden"));
        if status == StatusCode::TOO_MANY_REQUESTS {
            assert_eq!(error.retry_after.as_ref().unwrap(), "12");
        }
        assert_eq!(server.uploads.lock().unwrap().len(), 1);
    }
}

#[tokio::test]
async fn unsupported_provider_and_missing_credentials_do_not_upload() {
    let server = server(StatusCode::OK, json!({"code":0,"text":"unused"})).await;
    for (provider, credentials, status) in [
        (
            AccountProvider::Codex,
            credentials(&server),
            StatusCode::BAD_REQUEST,
        ),
        (
            AccountProvider::WorkBuddyAi,
            json!({"_voice_test_endpoint":server.url}),
            StatusCode::UNAUTHORIZED,
        ),
    ] {
        let error = transcribe_for_provider(
            &reqwest::Client::new(),
            provider,
            &credentials,
            &Bytes::from_static(b"audio"),
            "recording.wav",
            "audio/wav",
        )
        .await
        .unwrap_err();
        assert_eq!(error.status, status);
    }
    assert!(server.uploads.lock().unwrap().is_empty());
}
