use axum::{
    body::Bytes,
    http::{header, HeaderMap, HeaderValue, StatusCode},
};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::{json, Value};
use std::time::Duration;

use super::ProxyError;

const ENDPOINT: &str = "https://ai-gateway.kilo.ai";

pub(crate) async fn list_models(
    client: &reqwest::Client,
    credentials: &Value,
) -> Result<Vec<String>, ProxyError> {
    let response = send(
        client
            .get(format!(
                "{}/api/v1/transcription-models",
                endpoint(credentials)
            ))
            .headers(headers(credentials)?)
            .timeout(Duration::from_secs(30)),
    )
    .await?;
    let value: Value = response.json().await.map_err(request_error)?;
    let rows = value["data"].as_array().ok_or_else(failed)?;
    let mut models = Vec::new();
    for row in rows {
        if let (Some(id), Some(_)) = (
            row["id"].as_str().filter(|id| !id.trim().is_empty()),
            row["name"].as_str().filter(|name| !name.trim().is_empty()),
        ) {
            if !models.iter().any(|model| model == id) {
                models.push(id.to_owned());
            }
        }
    }
    if models.is_empty() {
        return Err(failed());
    }
    Ok(models)
}

pub(crate) async fn transcribe(
    client: &reqwest::Client,
    credentials: &Value,
    model: &str,
    audio: &Bytes,
    _filename: &str,
    mime: &str,
) -> Result<String, ProxyError> {
    if !matches!(mime, "audio/wav" | "audio/x-wav") {
        return Err(ProxyError::new(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "Kilo speech requests require WAV audio",
        ));
    }
    if audio.len() <= 44
        || &audio[..4] != b"RIFF"
        || &audio[8..24] != b"WAVEfmt \x10\0\0\0\x01\0\x01\0"
        || audio[24..28] != 16000_u32.to_le_bytes()
        || audio[28..32] != 32000_u32.to_le_bytes()
        || &audio[32..40] != b"\x02\0\x10\0data"
        || audio[4..8] != ((audio.len() - 8) as u32).to_le_bytes()
        || audio[40..44] != ((audio.len() - 44) as u32).to_le_bytes()
        || !(audio.len() - 44).is_multiple_of(2)
        || model.trim().is_empty()
    {
        return Err(ProxyError::new(
            StatusCode::BAD_REQUEST,
            "Kilo speech requires a model and 16 kHz mono PCM16 WAV audio",
        ));
    }
    let response = send(
        client
            .post(format!("{}/api/v1/audio/transcriptions", endpoint(credentials)))
            .headers(headers(credentials)?)
            .timeout(Duration::from_secs(45))
            .json(&json!({"model":model,"input_audio":{"data":STANDARD.encode(audio),"format":"wav"}})),
    )
    .await?;
    let result: Value = response.json().await.map_err(request_error)?;
    result["text"]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(failed)
}

fn endpoint(credentials: &Value) -> &str {
    #[cfg(test)]
    if let Some(endpoint) = credentials["_voice_test_endpoint"].as_str() {
        return endpoint.trim_end_matches('/');
    }
    let _ = credentials;
    ENDPOINT
}

fn headers(credentials: &Value) -> Result<HeaderMap, ProxyError> {
    let token = credentials["accessToken"]
        .as_str()
        .filter(|token| !token.trim().is_empty())
        .ok_or_else(login_required)?;
    let mut headers = HeaderMap::new();
    headers.insert(
        header::AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {token}")).map_err(|_| login_required())?,
    );
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    headers.insert(
        "x-kilocode-feature",
        HeaderValue::from_static("vscode-extension"),
    );
    headers.insert(
        "x-kilocode-editorname",
        HeaderValue::from_static("EchoBird"),
    );
    if let Some(organization) = credentials["organizationId"]
        .as_str()
        .filter(|value| !value.is_empty())
    {
        headers.insert(
            "x-kilocode-organizationid",
            HeaderValue::from_str(organization).map_err(|_| login_required())?,
        );
    }
    Ok(headers)
}

async fn send(request: reqwest::RequestBuilder) -> Result<reqwest::Response, ProxyError> {
    let response = request.send().await.map_err(request_error)?;
    if !response.status().is_success() {
        let mut error = ProxyError::new(response.status(), "Kilo speech request failed");
        error.retry_after = response.headers().get(header::RETRY_AFTER).cloned();
        return Err(error);
    }
    Ok(response)
}

fn request_error(error: reqwest::Error) -> ProxyError {
    if error.is_timeout() {
        ProxyError::new(StatusCode::GATEWAY_TIMEOUT, "Kilo speech request timed out")
    } else {
        failed()
    }
}

fn failed() -> ProxyError {
    ProxyError::new(StatusCode::BAD_GATEWAY, "Kilo speech response is invalid")
}

fn login_required() -> ProxyError {
    ProxyError::new(StatusCode::UNAUTHORIZED, "Kilo speech login required")
}

#[cfg(test)]
mod tests;
