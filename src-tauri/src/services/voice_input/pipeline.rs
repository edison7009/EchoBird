use super::config::Channel;
use crate::models::model::ModelConfig;
use crate::services::{account_model_sources, account_proxy, llm_client, model_manager};
use axum::body::Bytes;

pub fn resolve_model(id: &str) -> Result<ModelConfig, String> {
    model_manager::get_models()
        .into_iter()
        .chain(
            model_manager::get_user_models()
                .into_iter()
                .filter(|model| model.scope == crate::models::model::ModelScope::VoiceInput),
        )
        .find(|model| model.internal_id == id)
        .ok_or_else(|| "voice.error.missingSource".into())
}

pub(super) fn recognition_timeout(channel: &Channel, audio: &Bytes) -> std::time::Duration {
    if channel.kind == "account" {
        if let Ok((source, _)) = account_proxy::voice::parse_selector(&channel.id) {
            return account_proxy::voice::request_timeout(source.provider, audio.len());
        }
    }
    std::time::Duration::from_secs(60)
}

pub(super) async fn first_transcript<F, Fut>(
    channels: &[Channel],
    mut streamed: Option<Result<String, String>>,
    mut request: F,
) -> Result<String, String>
where
    F: FnMut(Channel) -> Fut,
    Fut: std::future::Future<Output = Result<String, String>>,
{
    let mut no_transcript = false;
    for channel in channels {
        let result = match streamed.take() {
            Some(Ok(text)) => Ok(text),
            Some(Err(error)) if error == "voice.error.noTranscript" => Err(error),
            result => {
                if result.is_some() {
                    log::warn!("[VoiceInput] realtime unavailable; falling back to complete WAV");
                }
                request(channel.clone()).await
            }
        };
        match result {
            Ok(text) if !text.trim().is_empty() => return Ok(text),
            Ok(_) => no_transcript = true,
            Err(error) => no_transcript |= error == "voice.error.noTranscript",
        }
    }
    Err(if no_transcript {
        "voice.error.noTranscript"
    } else {
        "voice.error.allChannels"
    }
    .into())
}

pub(super) async fn recognize(
    channel: &Channel,
    options: &std::collections::BTreeMap<String, super::config::ApiOptions>,
    audio: &Bytes,
    client: &reqwest::Client,
) -> Result<String, String> {
    if channel.kind == "workbuddy" {
        return account_proxy::voice::transcribe(
            client,
            account_proxy::voice::SpeechRequest {
                selector: &format!("workbuddy:{}", channel.id),
                audio,
                filename: "recording.wav",
                mime: "audio/wav",
            },
        )
        .await
        .map_err(|_| "voice.error.transcribe".into());
    }
    if channel.kind == "account" {
        return account_proxy::voice::transcribe(
            client,
            account_proxy::voice::SpeechRequest {
                selector: &channel.id,
                audio,
                filename: "recording.wav",
                mime: "audio/wav",
            },
        )
        .await
        .map_err(|_| "voice.error.transcribe".into());
    }
    let model = resolve_model(&channel.id)?;
    super::speech_api::transcribe(
        &model,
        options
            .get(&channel.id)
            .unwrap_or(&super::config::ApiOptions::default()),
        audio,
        client,
    )
    .await
}

#[cfg(test)]
pub(super) async fn recognize_model(
    model: &ModelConfig,
    audio: &Bytes,
    client: &reqwest::Client,
) -> Result<String, String> {
    super::speech_api::transcribe(model, &super::config::ApiOptions::default(), audio, client).await
}
pub(super) fn transcription_url(base: &str) -> Result<String, String> {
    let mut url = url::Url::parse(base).map_err(|_| "voice.error.speechApi")?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err("voice.error.speechApi".into());
    }
    let path = url.path().trim_end_matches('/');
    let path = path.strip_suffix("/chat/completions").unwrap_or(path);
    if !path.ends_with("/audio/transcriptions") {
        url.set_path(&format!("{path}/audio/transcriptions"));
    }
    Ok(url.into())
}

pub(super) async fn text_model(
    model: ModelConfig,
    text: &str,
    prompt: &str,
) -> Result<String, String> {
    let model_id = model.model_id.ok_or("voice.error.optimizationModel")?;
    if let Some(source) = model.account_source {
        let client = reqwest::Client::new();
        let body = serde_json::json!({"model":model_id,"messages":[{"role":"system","content":prompt},{"role":"user","content":text}],"stream":false});
        let credentials = account_model_sources::credentials(&source, false).await?;
        let response = account_proxy::route(source.provider, credentials, &model_id, body, &client)
            .await
            .map_err(|_| "voice.error.optimization")?;
        if !response.status().is_success() {
            return Err("voice.error.optimization".into());
        }
        let body = axum::body::to_bytes(response.into_body(), 2 * 1024 * 1024)
            .await
            .map_err(|_| "voice.error.optimization")?;
        let result: serde_json::Value =
            serde_json::from_slice(&body).map_err(|_| "voice.error.optimization")?;
        return result["choices"][0]["message"]["content"]
            .as_str()
            .filter(|text| !text.trim().is_empty())
            .map(str::to_owned)
            .ok_or_else(|| "voice.error.optimization".into());
    }
    let anthropic = model.base_url.is_empty();
    let client = llm_client::LlmClient::new(llm_client::LlmConfig {
        provider: if anthropic {
            llm_client::LlmProvider::Anthropic
        } else {
            llm_client::LlmProvider::OpenAI
        },
        base_url: if anthropic {
            model.anthropic_url.ok_or("voice.error.optimizationModel")?
        } else {
            model.base_url
        },
        api_key: model_manager::decrypt_key_for_use(&model.api_key),
        model: model_id,
        reasoning_effort: None,
    })?;
    let mut events = client
        .chat_stream(
            &[llm_client::Message {
                role: "user".into(),
                content: llm_client::MessageContent::Text(text.into()),
            }],
            &[],
            prompt,
        )
        .await?;
    let mut result = String::new();
    while let Some(event) = events.recv().await {
        match event {
            llm_client::LlmEvent::TextDelta(delta) => result.push_str(&delta),
            llm_client::LlmEvent::Error(_) => return Err("voice.error.optimization".into()),
            _ => {}
        }
    }
    if result.trim().is_empty() {
        Err("voice.error.optimization".into())
    } else {
        Ok(result)
    }
}
