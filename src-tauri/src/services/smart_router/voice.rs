use axum::extract::{FromRequest, Multipart, Request};
use std::future::Future;

use super::*;
use crate::models::model::AccountSource;
use crate::services::account_proxy;

struct Upload {
    source: AccountSource,
    selector: String,
    audio: Bytes,
    filename: String,
    mime: String,
    text_response: bool,
}

pub(super) async fn handle(State(state): State<AppState>, request: Request) -> Response {
    let config = match config_for_request(&state) {
        Ok(config) => config,
        Err(error) => return openai_error(StatusCode::INTERNAL_SERVER_ERROR, &error),
    };
    let upload = match read_upload(request, &config.api_key).await {
        Ok(upload) => upload,
        Err(response) => return response,
    };
    let source = &upload.source;
    let result = tokio::time::timeout(
        account_proxy::voice::request_timeout(source.provider, upload.audio.len()),
        transcribe(&state.http_client, &upload, |rejected| async move {
            account_proxy::voice::credentials(source, rejected.as_ref()).await
        }),
    )
    .await;
    match result {
        Ok(Ok(text)) if upload.text_response => text.into_response(),
        Ok(Ok(text)) => Json(json!({"text": text})).into_response(),
        Ok(Err(error)) => error.into_response(),
        Err(_) => openai_error(
            StatusCode::GATEWAY_TIMEOUT,
            "Speech transcription timed out",
        ),
    }
}

// The error is the finished HTTP reply, returned straight to the handler.
#[allow(clippy::result_large_err)]
async fn read_upload(request: Request, key: &str) -> Result<Upload, Response> {
    if !is_authorized(request.headers(), key) {
        return Err(openai_error(
            StatusCode::UNAUTHORIZED,
            "Invalid EchoBird router API key",
        ));
    }
    let mut multipart = Multipart::from_request(request, &())
        .await
        .map_err(|error| openai_error(error.status(), &error.body_text()))?;
    let mut source = None;
    let mut selector = String::new();
    let mut audio = None;
    let mut filename = String::new();
    let mut mime = String::new();
    let mut response_format = None;
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|error| openai_error(error.status(), &error.body_text()))?
    {
        match field.name() {
            Some("model") if source.is_none() => {
                let model = field
                    .text()
                    .await
                    .map_err(|error| openai_error(error.status(), &error.body_text()))?;
                let (saved, _) = account_proxy::voice::parse_selector(&model)
                    .map_err(account_proxy::ProxyError::into_response)?;
                source = Some(saved);
                selector = model;
            }
            Some("file") if audio.is_none() => {
                filename = field.file_name().unwrap_or("audio").to_owned();
                mime = field
                    .content_type()
                    .unwrap_or("application/octet-stream")
                    .to_owned();
                let bytes = field
                    .bytes()
                    .await
                    .map_err(|error| openai_error(error.status(), &error.body_text()))?;
                if bytes.is_empty() {
                    return Err(openai_error(StatusCode::BAD_REQUEST, "Audio file is empty"));
                }
                audio = Some(bytes);
            }
            Some("response_format") if response_format.is_none() => {
                let format = field
                    .text()
                    .await
                    .map_err(|error| openai_error(error.status(), &error.body_text()))?;
                if !matches!(format.as_str(), "json" | "text") {
                    return Err(openai_error(
                        StatusCode::BAD_REQUEST,
                        "response_format must be json or text",
                    ));
                }
                response_format = Some(format);
            }
            _ => {
                return Err(openai_error(
                    StatusCode::BAD_REQUEST,
                    "Unsupported or duplicate transcription field",
                ))
            }
        }
    }
    Ok(Upload {
        source: source.ok_or_else(|| openai_error(StatusCode::BAD_REQUEST, "model is required"))?,
        selector,
        audio: audio.ok_or_else(|| openai_error(StatusCode::BAD_REQUEST, "file is required"))?,
        filename,
        mime,
        text_response: response_format.as_deref() == Some("text"),
    })
}

async fn transcribe<F, Fut>(
    client: &reqwest::Client,
    upload: &Upload,
    credentials: F,
) -> Result<String, account_proxy::ProxyError>
where
    F: FnMut(Option<Value>) -> Fut,
    Fut: Future<Output = Result<Value, String>>,
{
    account_proxy::voice::transcribe_with(
        client,
        account_proxy::voice::SpeechRequest {
            selector: &upload.selector,
            audio: &upload.audio,
            filename: &upload.filename,
            mime: &upload.mime,
        },
        credentials,
    )
    .await
}

#[cfg(test)]
#[path = "voice_tests.rs"]
mod tests;
