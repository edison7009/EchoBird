use super::*;
use axum::{extract::Request, http::HeaderMap, routing::post, Json, Router};
use base64::Engine;

fn model(base: &str) -> crate::models::model::ModelConfig {
    serde_json::from_value(serde_json::json!({
        "internalId":"speech-fixture", "name":"Speech fixture", "modelId":"speech-model",
        "baseUrl":base, "apiKey":"fixture-key"
    }))
    .unwrap()
}

#[test]
fn voice_api_options_are_optional_for_old_settings_and_round_trip_without_credentials() {
    let mut config: Config = serde_json::from_value(serde_json::json!({"channels":[]})).unwrap();
    assert!(config.api_options.is_empty());
    config.api_options.insert(
        "speech-fixture".into(),
        config::ApiOptions {
            protocol: "mimo".into(),
        },
    );
    config.validate().unwrap();
    let value = serde_json::to_value(&config).unwrap();
    assert_eq!(value["apiOptions"]["speech-fixture"]["protocol"], "mimo");
    assert!(!value.to_string().contains("apiKey"));
    let restored: Config = serde_json::from_value(value).unwrap();
    assert_eq!(restored.api_options["speech-fixture"].protocol, "mimo");
    config
        .api_options
        .get_mut("speech-fixture")
        .unwrap()
        .protocol = "unknown".into();
    assert!(config.validate().is_err());
    let scope: crate::models::model::ModelScope =
        serde_json::from_value(serde_json::json!("voiceInput")).unwrap();
    assert_ne!(scope, crate::models::model::ModelScope::ModelCenter);
}

#[test]
fn legacy_gemini_voice_source_uses_the_native_path_from_a_text_model() {
    let mut url =
        url::Url::parse("https://generativelanguage.googleapis.com/v1beta/openai").unwrap();
    speech_api::set_gemini_transcription_path(&mut url, "models/gemini-fixture").unwrap();
    assert_eq!(url.path(), "/v1beta/models/gemini-fixture:generateContent");
}

#[tokio::test]
async fn speech_protocols_send_their_own_address_auth_audio_and_parse_transcripts() {
    let captured = Arc::new(Mutex::new(Vec::new()));
    let requests = captured.clone();
    let app = Router::new().fallback(post(move |request: Request| {
        let requests = requests.clone();
        async move {
            let path = request.uri().path().to_owned();
            let headers = request.headers().clone();
            let body = axum::body::to_bytes(request.into_body(), 20 * 1024 * 1024).await.unwrap();
            requests.lock().unwrap().push((path.clone(), headers, body));
            let text = "语音协议实际回包";
            Json(if path.contains("generateContent") {
                serde_json::json!({"candidates":[{"content":{"parts":[{"thought":true,"text":"hidden"},{"text":text}]}}]})
            } else if path.ends_with("flash") {
                serde_json::json!({"result":{"text":text}})
            } else if path.ends_with("chat/completions") {
                serde_json::json!({"choices":[{"message":{"content":text}}]})
            } else { serde_json::json!({"text":text}) })
        }
    }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let wav = audio::wav(&vec![0.25; 1600], 16000);
    let encoded = base64::engine::general_purpose::STANDARD.encode(&wav);
    for (protocol, suffix, header, value) in [
        (
            "openai",
            "audio/transcriptions",
            "authorization",
            "Bearer fixture-key",
        ),
        (
            "minimax",
            "speech_to_text",
            "authorization",
            "Bearer fixture-key",
        ),
        (
            "chatAudio",
            "chat/completions",
            "authorization",
            "Bearer fixture-key",
        ),
        ("mimo", "chat/completions", "api-key", "fixture-key"),
        (
            "openrouter",
            "chat/completions",
            "authorization",
            "Bearer fixture-key",
        ),
        (
            "gemini",
            "models/speech-model:generateContent",
            "x-goog-api-key",
            "fixture-key",
        ),
        (
            "doubao",
            "api/v3/auc/bigmodel/recognize/flash",
            "X-Api-Key",
            "fixture-key",
        ),
    ] {
        let options = config::ApiOptions {
            protocol: protocol.into(),
        };
        let mut model = model(&format!("http://{address}/proxy/v1"));
        assert_eq!(
            speech_api::transcribe(
                &model,
                &options,
                &wav.clone().into(),
                &reqwest::Client::new()
            )
            .await
            .unwrap(),
            "语音协议实际回包"
        );
        // Full endpoints must not be appended a second time.
        model.base_url = format!("http://{address}/proxy/v1/{suffix}");
        assert!(speech_api::transcribe(
            &model,
            &options,
            &wav.clone().into(),
            &reqwest::Client::new()
        )
        .await
        .is_ok());
        let requests = captured.lock().unwrap();
        for (path, headers, body) in requests.iter().rev().take(2) {
            assert_eq!(path, &format!("/proxy/v1/{suffix}"));
            assert_eq!(headers.get(header).unwrap(), value);
            if protocol != "openai" && protocol != "minimax" {
                let body: serde_json::Value = serde_json::from_slice(body).unwrap();
                match protocol {
                    "doubao" => {
                        assert_eq!(body["audio"]["data"], encoded);
                        assert!(!headers.contains_key("X-Api-App-Key"));
                        assert!(!headers.contains_key("X-Api-Access-Key"));
                        assert_eq!(headers["X-Api-Resource-Id"], "speech-model");
                        assert_eq!(headers["X-Api-Sequence"], "-1");
                        uuid::Uuid::parse_str(headers["X-Api-Request-Id"].to_str().unwrap())
                            .unwrap();
                    }
                    "gemini" => assert_eq!(
                        body["contents"][0]["parts"][1]["inlineData"]["data"],
                        encoded
                    ),
                    _ => {
                        let content = body["messages"][0]["content"].as_array().unwrap();
                        let audio = content
                            .iter()
                            .find(|part| part["type"] == "input_audio")
                            .unwrap();
                        assert_eq!(
                            audio["input_audio"]["data"],
                            if protocol == "openrouter" {
                                encoded.clone()
                            } else {
                                format!("data:audio/wav;base64,{encoded}")
                            }
                        );
                        assert_eq!(body["stream"], false);
                    }
                }
            } else {
                assert!(body.windows(wav.len()).any(|part| part == wav));
            }
        }
    }
    assert_eq!(captured.lock().unwrap().len(), 14);
    task.abort();
}

#[tokio::test]
async fn doubao_api_key_business_errors_are_handled_without_exposing_credentials() {
    let app = Router::new().fallback(post(|headers: HeaderMap| async move {
        assert_eq!(headers["X-Api-Key"], "fixture-key");
        assert!(!headers.contains_key("X-Api-App-Key"));
        assert!(!headers.contains_key("X-Api-Access-Key"));
        assert!(!headers.contains_key("authorization"));
        (
            [("X-Api-Status-Code", "45000001")],
            Json(serde_json::json!({"result":{"text":"must not succeed"},"secret":"fixture-key"})),
        )
    }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let options = config::ApiOptions {
        protocol: "doubao".into(),
    };
    let error = speech_api::transcribe(
        &model(&format!("http://{address}")),
        &options,
        &vec![0; 44].into(),
        &reqwest::Client::new(),
    )
    .await
    .unwrap_err();
    assert_eq!(error, "voice.error.transcribe");
    task.abort();
}

#[tokio::test]
async fn glm_long_recordings_are_split_into_valid_ordered_wav_uploads() {
    use axum::extract::Multipart;
    let captured = Arc::new(Mutex::new(Vec::new()));
    let uploads = captured.clone();
    let app = Router::new().fallback(post(move |mut multipart: Multipart| {
        let uploads = uploads.clone();
        async move {
            while let Some(field) = multipart.next_field().await.unwrap() {
                if field.name() == Some("file") {
                    let bytes = field.bytes().await.unwrap();
                    uploads.lock().unwrap().push(bytes);
                }
            }
            Json(serde_json::json!({"text":"一段。"}))
        }
    }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let wav = audio::wav(&vec![0.25; 61 * 16000], 16000);
    let mut model = model(&format!("http://{address}/v1"));
    model.model_id = Some("glm-asr-2512".into());
    assert_eq!(
        speech_api::transcribe(
            &model,
            &config::ApiOptions::default(),
            &wav.clone().into(),
            &reqwest::Client::new()
        )
        .await
        .unwrap(),
        "一段。一段。一段。"
    );
    let uploads = captured.lock().unwrap();
    assert_eq!(uploads.len(), 3);
    let mut samples = Vec::new();
    for part in uploads.iter() {
        assert!(part.len() <= 44 + 29 * 16000 * 2);
        assert_eq!(&part[..4], b"RIFF");
        assert_eq!(
            u32::from_le_bytes(part[40..44].try_into().unwrap()) as usize,
            part.len() - 44
        );
        samples.extend_from_slice(&part[44..]);
    }
    assert_eq!(samples, wav[44..]);
    task.abort();
}
