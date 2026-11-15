use super::*;
use axum::{extract::State, http::HeaderMap, routing::post, Json, Router};
use serde_json::json;
use std::sync::{Arc, Mutex};

#[tokio::test]
async fn built_in_trae_speech_loads_credentials_before_any_network_request() {
    for (prefix, expected) in [
        ("trae", AccountProvider::Trae),
        ("traecn", AccountProvider::Traecn),
    ] {
        let selector = format!("{prefix}:{}", "a".repeat(64));
        assert_eq!(parse_selector(&selector).unwrap().0.provider, expected);
        let mut bytes = b"RIFF".to_vec();
        bytes.extend(40_u32.to_le_bytes());
        bytes.extend(b"WAVEfmt \x10\0\0\0\x01\0\x01\0");
        bytes.extend(16000_u32.to_le_bytes());
        bytes.extend(32000_u32.to_le_bytes());
        bytes.extend(b"\x02\0\x10\0data");
        bytes.extend(4_u32.to_le_bytes());
        bytes.extend([1, 2, 3, 4]);
        let audio = Bytes::from(bytes);
        let mut calls = 0;
        let result = transcribe_with(
            &reqwest::Client::new(),
            SpeechRequest {
                selector: &selector,
                audio: &audio,
                filename: "speech.wav",
                mime: "audio/wav",
            },
            |rejected| {
                calls += 1;
                assert!(rejected.is_none());
                std::future::ready(Err("accountError.loginRequired|fixture-credentials".into()))
            },
        )
        .await;
        let error = result.unwrap_err();
        assert_eq!(calls, 1);
        assert_eq!(error.status, StatusCode::UNAUTHORIZED);
        assert!(error.message.contains("fixture-credentials"));
    }
}

#[test]
fn paced_sources_keep_a_server_budget_after_replaying_the_longest_recording() {
    let longest_wav = 44 + 60 * 32_000;
    for provider in [
        AccountProvider::QwenWork,
        AccountProvider::QwenWorkCn,
        AccountProvider::Qoder,
        AccountProvider::QoderCn,
        AccountProvider::Traecn,
        AccountProvider::Trae,
        AccountProvider::Cursor,
        AccountProvider::CodeArts,
        AccountProvider::DoubaoWork,
    ] {
        assert_eq!(
            request_timeout(provider, longest_wav),
            Duration::from_secs(120)
        );
        assert_eq!(
            request_timeout(provider, usize::MAX),
            Duration::from_secs(120)
        );
    }
    for provider in [
        AccountProvider::WorkBuddy,
        AccountProvider::Codex,
        AccountProvider::Kilo,
        AccountProvider::Minimax,
        AccountProvider::ClaudeCode,
    ] {
        assert_eq!(
            request_timeout(provider, longest_wav),
            Duration::from_secs(60)
        );
    }
}

#[test]
fn selectors_only_accept_supported_speech_accounts() {
    let id = "a".repeat(64);
    for (prefix, provider) in [
        ("workbuddy", AccountProvider::WorkBuddy),
        ("workbuddyai", AccountProvider::WorkBuddyAi),
        ("claudecode", AccountProvider::ClaudeCode),
        ("codex", AccountProvider::Codex),
        ("minimax", AccountProvider::Minimax),
        ("antigravity", AccountProvider::Antigravity),
        ("cursor", AccountProvider::Cursor),
        ("grokbot", AccountProvider::GrokBot),
        ("dumate", AccountProvider::Dumate),
        ("devin", AccountProvider::Devin),
        ("trae", AccountProvider::Trae),
        ("doubaowork", AccountProvider::DoubaoWork),
        ("traecn", AccountProvider::Traecn),
        ("qoder", AccountProvider::Qoder),
        ("qodercn", AccountProvider::QoderCn),
        ("qwenwork", AccountProvider::QwenWork),
        ("qwenworkcn", AccountProvider::QwenWorkCn),
    ] {
        let selector = format!("{prefix}:{id}");
        let (source, model) = parse_selector(&selector).unwrap();
        assert_eq!(source.provider, provider);
        assert_eq!(source.account_id, id);
        assert!(model.is_none());
    }
    let selector = format!("codearts:{id}");
    let (source, model) = parse_selector(&selector).unwrap();
    assert_eq!(source.provider, AccountProvider::CodeArts);
    assert_eq!(source.account_id, format!("codearts:{id}"));
    assert!(model.is_none());
    let selector = format!("kilo:{id}:vendor:speech-model");
    let (source, model) = parse_selector(&selector).unwrap();
    assert_eq!(source.provider, AccountProvider::Kilo);
    assert_eq!(model, Some("vendor:speech-model"));
    for selector in [
        format!("copilot:{id}"),
        format!("kimi:{id}"),
        format!("cline:{id}"),
        format!("zcode:{id}"),
        format!("grok:{id}"),
        format!("codearts:codearts:{id}"),
        format!("cursor:{id}:chat-model"),
        format!("workbuddy:{id}:chat-model"),
        format!("workbuddy:../{id}"),
        format!("kilo:{id}"),
        format!("kilo:{id}: "),
        format!("kilo:{id}:{}", "x".repeat(512)),
    ] {
        assert_eq!(
            parse_selector(&selector).unwrap_err().status,
            StatusCode::BAD_REQUEST
        );
    }
}

#[tokio::test]
async fn new_sources_validate_audio_before_loading_or_refreshing_saved_credentials() {
    let audio = Bytes::from_static(b"invalid-wave");
    for provider in ["cursor", "grokbot", "dumate", "devin", "codearts"] {
        let selector = format!("{provider}:{}", "a".repeat(64));
        let result = transcribe_with(
            &reqwest::Client::new(),
            SpeechRequest {
                selector: &selector,
                audio: &audio,
                filename: "audio.wav",
                mime: "audio/wav",
            },
            |_| async {
                panic!("invalid audio must not load credentials or prepare a network session")
            },
        )
        .await;
        assert_eq!(
            result.unwrap_err().status,
            StatusCode::BAD_REQUEST,
            "{provider}"
        );
    }
}

#[tokio::test]
async fn rejects_unknown_speech_sources_before_credentials_or_native_state() {
    let audio = Bytes::from_static(b"fixture");
    let result = transcribe_with(
        &reqwest::Client::new(),
        SpeechRequest {
            selector: "copilot:unavailable",
            audio: &audio,
            filename: "audio.wav",
            mime: "audio/wav",
        },
        |_| async { panic!("unsupported source must not load credentials") },
    )
    .await;
    assert_eq!(result.unwrap_err().status, StatusCode::BAD_REQUEST);
    assert!(list_models(&AccountSource {
        provider: AccountProvider::Copilot,
        account_id: "not-saved".into(),
    })
    .await
    .is_err());
}

#[tokio::test]
async fn selected_account_refreshes_only_once_after_401_and_never_after_other_failures() {
    for first_status in [StatusCode::UNAUTHORIZED, StatusCode::TOO_MANY_REQUESTS] {
        let calls: Arc<Mutex<Vec<String>>> = Arc::default();
        let app = Router::new()
            .route(
                "/transcribe",
                post(
                    move |State(calls): State<Arc<Mutex<Vec<String>>>>, headers: HeaderMap| async move {
                        let mut calls = calls.lock().unwrap();
                        calls.push(headers["authorization"].to_str().unwrap().into());
                        (
                            if calls.len() == 1 {
                                first_status
                            } else {
                                StatusCode::OK
                            },
                            Json(json!({"code":0,"text":"complete transcript"})),
                        )
                    },
                ),
            )
            .with_state(calls.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let mut loaded = Vec::new();
        let audio = Bytes::from_static(b"fixture");
        let selector = format!("codex:{}", "a".repeat(64));
        let result = transcribe_with(
            &reqwest::Client::new(),
            SpeechRequest {
                selector: &selector,
                audio: &audio,
                filename: "audio.wav",
                mime: "audio/wav",
            },
            |rejected| {
                let token = if rejected.is_some() { "new" } else { "old" };
                loaded.push(rejected);
                std::future::ready(Ok(json!({
                    "accessToken":token,"accountId":"selected-user","_voice_test_endpoint":endpoint,
                })))
            },
        )
        .await;
        if first_status == StatusCode::UNAUTHORIZED {
            assert_eq!(result.unwrap(), "complete transcript");
            assert_eq!(loaded.len(), 2);
            assert_eq!(loaded[1].as_ref().unwrap()["accessToken"], "old");
            assert_eq!(*calls.lock().unwrap(), ["Bearer old", "Bearer new"]);
        } else {
            assert_eq!(result.unwrap_err().status, first_status);
            assert_eq!(loaded.len(), 1);
            assert_eq!(calls.lock().unwrap().len(), 1);
        }
        task.abort();
    }
}

#[tokio::test]
#[ignore = "Reads the user's saved account directory; run explicitly for voice integration"]
async fn saved_speech_account_inventory() {
    for provider in [
        AccountProvider::WorkBuddy,
        AccountProvider::WorkBuddyAi,
        AccountProvider::ClaudeCode,
        AccountProvider::Kilo,
        AccountProvider::Codex,
        AccountProvider::Minimax,
        AccountProvider::Antigravity,
        AccountProvider::Traecn,
        AccountProvider::Qoder,
        AccountProvider::QoderCn,
        AccountProvider::QwenWork,
        AccountProvider::QwenWorkCn,
    ] {
        let rows = account_model_sources::list_accounts(provider)
            .await
            .unwrap();
        eprintln!(
            "speech account inventory: {provider:?}, saved={}",
            rows.len()
        );
    }
}

#[tokio::test]
#[ignore = "Sends only an explicitly supplied synthetic WAV using an existing saved account"]
async fn live_saved_account_speech_transcription() {
    let provider: AccountProvider = serde_json::from_value(json!(std::env::var(
        "ECHOBIRD_ACCOUNT_VOICE_TEST_PROVIDER"
    )
    .unwrap()))
    .unwrap();
    let rows = account_model_sources::list_accounts(provider)
        .await
        .unwrap();
    let account_id = std::env::var("ECHOBIRD_ACCOUNT_VOICE_TEST_ACCOUNT").unwrap_or_else(|_| {
        assert_eq!(
            rows.len(),
            1,
            "Specify a saved account when not uniquely determined"
        );
        rows[0].source.account_id.clone()
    });
    let source = rows
        .into_iter()
        .find(|row| row.source.account_id == account_id)
        .unwrap()
        .source;
    let selectors = list_models(&source).await.unwrap();
    let selector = std::env::var("ECHOBIRD_ACCOUNT_VOICE_TEST_MODEL")
        .unwrap_or_else(|_| selectors.first().unwrap().clone());
    assert!(selectors.contains(&selector));
    let path = std::env::var("ECHOBIRD_ACCOUNT_VOICE_TEST_WAV").unwrap();
    let audio = Bytes::from(std::fs::read(path).unwrap());
    assert!(audio.len() > 44);
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let transcription = transcribe(
        &client,
        SpeechRequest {
            selector: &selector,
            audio: &audio,
            filename: "synthetic-speech-test.wav",
            mime: "audio/wav",
        },
    );
    let text = if provider == AccountProvider::Minimax {
        crate::services::minimax_accounts::LIVE_ACCOUNT_CRYPTO
            .scope((), transcription)
            .await
    } else {
        transcription.await
    }
    .unwrap();
    if std::env::var("ECHOBIRD_ACCOUNT_VOICE_TEST_TRACE").as_deref() == Ok("1") {
        eprintln!("synthetic speech test result: {text:?}");
    }
    assert!(!text.trim().is_empty());
    if let Ok(expected) = std::env::var("ECHOBIRD_ACCOUNT_VOICE_TEST_EXPECT_PREFIX") {
        assert!(
            text.to_lowercase().contains(&expected.to_lowercase()),
            "Required synthetic speech beginning is missing"
        );
    }
    if let Ok(expected) = std::env::var("ECHOBIRD_ACCOUNT_VOICE_TEST_EXPECT") {
        assert!(
            text.to_lowercase().contains(&expected.to_lowercase()),
            "Required synthetic speech tail is missing"
        );
    }
    eprintln!(
        "speech account live: {provider:?}, transcript_chars={}",
        text.chars().count()
    );
}
