use axum::{body::Bytes, http::StatusCode};
use serde_json::Value;
use std::time::Duration;

use super::{
    antigravity_voice, codearts_voice, codex_voice, cursor_voice, devin_voice, dumate_voice,
    grokbot_voice, kilo_voice, minimax_voice, qoder_voice, qwenwork_voice, ProxyError,
};
use crate::models::model::{AccountProvider, AccountSource};
use crate::services::account_model_sources;

pub(crate) struct SpeechRequest<'a> {
    pub selector: &'a str,
    pub audio: &'a Bytes,
    pub filename: &'a str,
    pub mime: &'a str,
}

pub(crate) fn media_timeout(pcm_bytes: usize) -> Duration {
    Duration::from_millis(60_000 + pcm_bytes.div_ceil(32).min(60_000) as u64)
}

pub(crate) fn request_timeout(provider: AccountProvider, wav_bytes: usize) -> Duration {
    if matches!(
        provider,
        AccountProvider::QwenWork
            | AccountProvider::QwenWorkCn
            | AccountProvider::Qoder
            | AccountProvider::QoderCn
            | AccountProvider::Traecn
            | AccountProvider::Trae
            | AccountProvider::Cursor
            | AccountProvider::CodeArts
            | AccountProvider::DoubaoWork
    ) {
        media_timeout(wav_bytes.saturating_sub(44))
    } else {
        Duration::from_secs(60)
    }
}

pub(crate) fn parse_selector(selector: &str) -> Result<(AccountSource, Option<&str>), ProxyError> {
    let mut parts = selector.splitn(3, ':');
    let provider = match parts.next() {
        Some("workbuddy") => AccountProvider::WorkBuddy,
        Some("workbuddyai") => AccountProvider::WorkBuddyAi,
        Some("claudecode") => AccountProvider::ClaudeCode,
        Some("codex") => AccountProvider::Codex,
        Some("minimax") => AccountProvider::Minimax,
        Some("kilo") => AccountProvider::Kilo,
        Some("antigravity") => AccountProvider::Antigravity,
        Some("cursor") => AccountProvider::Cursor,
        Some("grokbot") => AccountProvider::GrokBot,
        Some("dumate") => AccountProvider::Dumate,
        Some("devin") => AccountProvider::Devin,
        Some("trae") => AccountProvider::Trae,
        Some("codearts") => AccountProvider::CodeArts,
        Some("doubaowork") => AccountProvider::DoubaoWork,
        Some("traecn") => AccountProvider::Traecn,
        Some("qoder") => AccountProvider::Qoder,
        Some("qodercn") => AccountProvider::QoderCn,
        Some("qwenwork") => AccountProvider::QwenWork,
        Some("qwenworkcn") => AccountProvider::QwenWorkCn,
        _ => return Err(invalid_selector()),
    };
    let account_id = parts.next().ok_or_else(invalid_selector)?;
    let model = parts.next();
    if selector.len() > 512
        || account_id.len() != 64
        || !account_id.bytes().all(|byte| byte.is_ascii_hexdigit())
        || if provider == AccountProvider::Kilo {
            model.is_none_or(|value| value.trim().is_empty())
        } else {
            model.is_some()
        }
    {
        return Err(invalid_selector());
    }
    Ok((
        AccountSource {
            provider,
            account_id: if provider == AccountProvider::CodeArts {
                format!("codearts:{account_id}")
            } else {
                account_id.into()
            },
        },
        model,
    ))
}

fn invalid_selector() -> ProxyError {
    ProxyError::new(StatusCode::BAD_REQUEST, "Invalid saved speech source")
}

pub(crate) async fn list_models(source: &AccountSource) -> Result<Vec<String>, String> {
    let prefix = match source.provider {
        AccountProvider::WorkBuddy => "workbuddy",
        AccountProvider::WorkBuddyAi => "workbuddyai",
        AccountProvider::ClaudeCode => "claudecode",
        AccountProvider::Codex => "codex",
        AccountProvider::Minimax => "minimax",
        AccountProvider::Kilo => "kilo",
        AccountProvider::Antigravity => "antigravity",
        AccountProvider::Cursor => "cursor",
        AccountProvider::GrokBot => "grokbot",
        AccountProvider::Dumate => "dumate",
        AccountProvider::Devin => "devin",
        AccountProvider::Trae => "trae",
        AccountProvider::CodeArts => "codearts",
        AccountProvider::DoubaoWork => "doubaowork",
        AccountProvider::Traecn => "traecn",
        AccountProvider::Qoder => "qoder",
        AccountProvider::QoderCn => "qodercn",
        AccountProvider::QwenWork => "qwenwork",
        AccountProvider::QwenWorkCn => "qwenworkcn",
        _ => return Err("voice.error.missingSource".into()),
    };
    account_model_sources::validate_source(source).await?;
    let id = if source.provider == AccountProvider::CodeArts {
        source
            .account_id
            .strip_prefix("codearts:")
            .ok_or("voice.error.missingSource")?
    } else {
        source.account_id.as_str()
    };
    let selector = format!("{prefix}:{id}");
    if source.provider != AccountProvider::Kilo {
        return Ok(vec![selector]);
    }
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| "accountError.network")?;
    let mut credentials = account_model_sources::credentials(source, false).await?;
    let models = match kilo_voice::list_models(&client, &credentials).await {
        Err(error) if error.status == StatusCode::UNAUTHORIZED => {
            credentials =
                account_model_sources::credentials_after_rejection(source, &credentials).await?;
            kilo_voice::list_models(&client, &credentials).await
        }
        result => result,
    }
    .map_err(|error| error.to_string())?;
    Ok(models
        .into_iter()
        .map(|model| format!("{selector}:{model}"))
        .filter(|selector| parse_selector(selector).is_ok())
        .collect())
}

pub(crate) async fn transcribe(
    client: &reqwest::Client,
    request: SpeechRequest<'_>,
) -> Result<String, ProxyError> {
    let (source, _) = parse_selector(request.selector)?;
    let result = tokio::time::timeout(
        request_timeout(source.provider, request.audio.len()),
        transcribe_with(client, request, |rejected| {
            let source = &source;
            async move { credentials(source, rejected.as_ref()).await }
        }),
    )
    .await
    .unwrap_or_else(|_| {
        Err(ProxyError::new(
            StatusCode::GATEWAY_TIMEOUT,
            "Speech transcription timed out",
        ))
    });
    if let Err(error) = &result {
        log::warn!(
            "[VoiceInput] account transcription failed provider={:?} status={}",
            source.provider,
            error.status.as_u16()
        );
    }
    result
}

pub(crate) async fn credentials(
    source: &AccountSource,
    rejected: Option<&Value>,
) -> Result<Value, String> {
    if source.provider == AccountProvider::Dumate {
        return crate::services::dumate_accounts::voice_credentials(&source.account_id, rejected)
            .await;
    }
    if matches!(
        source.provider,
        AccountProvider::Qoder | AccountProvider::QoderCn
    ) {
        let edition = if source.provider == AccountProvider::QoderCn {
            crate::services::qoder_accounts::Edition::QoderCn
        } else {
            crate::services::qoder_accounts::Edition::Qoder
        };
        return crate::services::qoder_accounts::voice_credentials(
            edition,
            &source.account_id,
            rejected,
        )
        .await;
    }
    match rejected {
        Some(rejected) => {
            account_model_sources::credentials_after_rejection(source, rejected).await
        }
        None => account_model_sources::credentials(source, false).await,
    }
}

pub(crate) async fn transcribe_with<F, Fut>(
    client: &reqwest::Client,
    request: SpeechRequest<'_>,
    mut credentials: F,
) -> Result<String, ProxyError>
where
    F: FnMut(Option<Value>) -> Fut,
    Fut: std::future::Future<Output = Result<Value, String>>,
{
    let (source, model) = parse_selector(request.selector)?;
    if let Some(application) = crate::services::plugin_accounts::application(source.provider) {
        use base64::Engine;
        account_model_sources::validate_source(&source)
            .await
            .map_err(account_model_sources::credential_error)?;
        let value=crate::services::plugin_accounts::call(application,"voice",serde_json::json!({"accountId":source.account_id,"audio":base64::engine::general_purpose::STANDARD.encode(request.audio),"filename":request.filename,"mime":request.mime})).await.map_err(account_model_sources::credential_error)?;
        return serde_json::from_value(value).map_err(ProxyError::upstream);
    }

    match source.provider {
        AccountProvider::Dumate => dumate_voice::validate_audio(request.audio, request.mime)?,
        AccountProvider::Cursor => cursor_voice::validate_audio(request.audio, request.mime)?,
        AccountProvider::GrokBot => grokbot_voice::validate_audio(request.audio, request.mime)?,
        AccountProvider::Devin => devin_voice::validate_audio(request.audio, request.mime)?,
        AccountProvider::CodeArts => codearts_voice::validate_audio(request.audio, request.mime)?,
        _ => {}
    }
    let mut current = credentials(None)
        .await
        .map_err(account_model_sources::credential_error)?;
    for attempt in 0..2 {
        let result = match source.provider {
            AccountProvider::Dumate => {
                dumate_voice::transcribe(
                    client,
                    &current,
                    request.audio,
                    request.filename,
                    request.mime,
                )
                .await
            }
            AccountProvider::Cursor => {
                cursor_voice::transcribe(
                    client,
                    &current,
                    request.audio,
                    request.filename,
                    request.mime,
                )
                .await
            }
            AccountProvider::GrokBot => {
                grokbot_voice::transcribe(
                    client,
                    &current,
                    request.audio,
                    request.filename,
                    request.mime,
                )
                .await
            }
            AccountProvider::Devin => {
                devin_voice::transcribe(
                    client,
                    &current,
                    request.audio,
                    request.filename,
                    request.mime,
                )
                .await
            }
            AccountProvider::CodeArts => {
                codearts_voice::transcribe(
                    client,
                    &current,
                    request.audio,
                    request.filename,
                    request.mime,
                )
                .await
            }
            AccountProvider::Antigravity => {
                async {
                    antigravity_voice::validate_audio(request.audio, request.mime)?;
                    let model = antigravity_voice::selected_model(client, &current).await?;
                    antigravity_voice::transcribe(
                        client,
                        &current,
                        &model,
                        request.audio,
                        request.filename,
                        request.mime,
                    )
                    .await
                }
                .await
            }
            AccountProvider::QwenWork | AccountProvider::QwenWorkCn => {
                qwenwork_voice::transcribe(
                    client,
                    &current,
                    request.audio,
                    request.filename,
                    request.mime,
                )
                .await
            }
            AccountProvider::Qoder | AccountProvider::QoderCn => {
                qoder_voice::transcribe(
                    client,
                    &current,
                    request.audio,
                    request.filename,
                    request.mime,
                )
                .await
            }
            AccountProvider::Kilo => {
                kilo_voice::transcribe(
                    client,
                    &current,
                    model.ok_or_else(invalid_selector)?,
                    request.audio,
                    request.filename,
                    request.mime,
                )
                .await
            }
            AccountProvider::Codex => {
                codex_voice::transcribe(
                    client,
                    &current,
                    request.audio,
                    request.filename,
                    request.mime,
                )
                .await
            }
            AccountProvider::Minimax => {
                minimax_voice::transcribe(
                    client,
                    &current,
                    request.audio,
                    request.filename,
                    request.mime,
                )
                .await
            }
            _ => unreachable!(),
        };
        match result {
            Err(error) if error.status == StatusCode::UNAUTHORIZED && attempt == 0 => {
                current = credentials(Some(current))
                    .await
                    .map_err(account_model_sources::credential_error)?;
            }
            result => return result,
        }
    }
    unreachable!()
}

#[cfg(test)]
#[path = "voice/tests.rs"]
mod tests;

#[cfg(test)]
#[path = "voice/batch_live_tests.rs"]
mod batch_live_tests;
