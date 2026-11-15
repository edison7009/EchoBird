use axum::body::Bytes;
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use reqwest::multipart::{Form, Part};
use serde_json::Value;
use std::time::Duration;

use super::ProxyError;
use crate::models::model::AccountProvider;

pub(crate) mod stream;

#[cfg(test)]
mod provider_tests;

const ENDPOINT: &str = "https://www.workbuddy.cn";

fn endpoint(provider: AccountProvider) -> Result<&'static str, ProxyError> {
    match provider {
        AccountProvider::WorkBuddy => Ok(ENDPOINT),
        AccountProvider::WorkBuddyAi => Ok("https://www.workbuddy.ai"),
        _ => Err(ProxyError::new(
            StatusCode::BAD_REQUEST,
            "Speech recognition is unavailable for this account provider",
        )),
    }
}

pub(crate) async fn transcribe_for_provider(
    client: &reqwest::Client,
    provider: AccountProvider,
    credentials: &Value,
    audio: &Bytes,
    filename: &str,
    mime: &str,
) -> Result<String, ProxyError> {
    let endpoint = endpoint(provider)?;
    #[cfg(test)]
    let endpoint = credentials["_voice_test_endpoint"]
        .as_str()
        .unwrap_or(endpoint);
    let part = Part::stream_with_length(audio.clone(), audio.len() as u64)
        .file_name(filename.to_owned())
        .mime_str(mime)
        .map_err(|_| ProxyError::new(StatusCode::BAD_REQUEST, "Invalid audio content type"))?;
    let request = client
        .post(format!("{endpoint}/agenttool/v1/asr"))
        .headers(auth_headers(credentials)?)
        .timeout(Duration::from_secs(30))
        .multipart(Form::new().part("audio", part));
    let response = request.send().await.map_err(request_error)?;
    if !response.status().is_success() {
        let mut error = ProxyError::new(response.status(), "WorkBuddy speech request failed");
        error.retry_after = response.headers().get(header::RETRY_AFTER).cloned();
        return Err(error);
    }
    let result: Value = response.json().await.map_err(request_error)?;
    if result.get("code").is_some_and(|code| code != 0) {
        let mut error = ProxyError::new(
            StatusCode::BAD_GATEWAY,
            "WorkBuddy speech recognition failed",
        );
        error.error_code = result["code"].as_i64().map(|code| code.to_string());
        return Err(error);
    }
    result["text"].as_str().map(str::to_owned).ok_or_else(|| {
        ProxyError::new(
            StatusCode::BAD_GATEWAY,
            "WorkBuddy speech response has no text",
        )
    })
}

fn auth_headers(credentials: &Value) -> Result<HeaderMap, ProxyError> {
    let token = credentials["accessToken"]
        .as_str()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| ProxyError::new(StatusCode::UNAUTHORIZED, "WorkBuddy login required"))?;
    let uid = credentials["uid"]
        .as_str()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| ProxyError::new(StatusCode::UNAUTHORIZED, "WorkBuddy user id missing"))?;
    let mut headers = HeaderMap::new();
    for (name, value) in [
        ("Authorization", format!("Bearer {token}")),
        ("X-User-Id", uid.to_owned()),
    ] {
        headers.insert(
            name,
            HeaderValue::from_str(&value).map_err(|_| {
                ProxyError::new(StatusCode::UNAUTHORIZED, "Invalid WorkBuddy authentication")
            })?,
        );
    }
    for (field, names) in [
        ("domain", &["X-Domain"][..]),
        ("enterpriseId", &["X-Enterprise-Id", "X-Tenant-Id"][..]),
    ] {
        if let Some(value) = credentials[field]
            .as_str()
            .filter(|value| !value.is_empty())
        {
            for name in names {
                headers.insert(
                    *name,
                    HeaderValue::from_str(value).map_err(|_| {
                        ProxyError::new(
                            StatusCode::UNAUTHORIZED,
                            "Invalid WorkBuddy authentication",
                        )
                    })?,
                );
            }
        }
    }
    Ok(headers)
}

fn request_error(error: reqwest::Error) -> ProxyError {
    if error.is_timeout() {
        ProxyError::new(
            StatusCode::GATEWAY_TIMEOUT,
            "WorkBuddy speech request timed out",
        )
    } else {
        ProxyError::new(StatusCode::BAD_GATEWAY, "WorkBuddy speech request failed")
    }
}
