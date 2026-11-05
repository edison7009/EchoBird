use axum::{
    body::Bytes,
    http::{header, HeaderValue, StatusCode},
};
use futures_util::StreamExt;
use serde_json::{json, Value};
use std::time::Duration;

use super::{ProxyError, SseParser};

pub(crate) async fn transcribe(
    client: &reqwest::Client,
    credentials: &Value,
    audio: &Bytes,
    _filename: &str,
    mime: &str,
) -> Result<String, ProxyError> {
    if !matches!(mime, "audio/wav" | "audio/x-wav") {
        return Err(ProxyError::new(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "MiniMax speech requests require WAV audio",
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
    {
        return Err(ProxyError::new(
            StatusCode::BAD_REQUEST,
            "MiniMax speech requests require 16 kHz mono PCM16 WAV audio",
        ));
    }
    let (base, region) = endpoint(credentials)?;
    let token = credentials["accessToken"]
        .as_str()
        .filter(|token| !token.is_empty())
        .ok_or_else(login_error)?;
    let authorization =
        HeaderValue::from_str(&format!("Bearer {token}")).map_err(|_| login_error())?;

    tokio::time::timeout(Duration::from_secs(60), async {
        let grant: Value = send(
            client
                .post(format!("{base}/matrix/api/v1/asr/session_token"))
                .header(header::AUTHORIZATION, authorization)
                .timeout(Duration::from_secs(15))
                .json(&json!({"provider":"amadeus", "region":region})),
        )
        .await?
        .json()
        .await
        .map_err(request_error)?;
        let session_token = grant["token"]
            .as_str()
            .map(str::trim)
            .filter(|token| !token.is_empty())
            .ok_or_else(upstream_error)?;
        let session_authorization = HeaderValue::from_str(&format!("Bearer {session_token}"))
            .map_err(|_| upstream_error())?;
        let response = send(
            client
                .post(format!("{base}/matrix/asr/audio/transcribe"))
                .header(header::AUTHORIZATION, session_authorization)
                .header(header::CONTENT_TYPE, "audio/wav")
                .header(header::ACCEPT, "text/event-stream")
                .timeout(Duration::from_secs(45))
                .body(audio.clone()),
        )
        .await?;
        if !response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.starts_with("text/event-stream"))
        {
            return Err(upstream_error());
        }
        let mut parser = SseParser::default();
        let mut stream = response.bytes_stream();
        let mut text = String::new();
        let mut expected_index = 0;
        let mut finished = false;
        while let Some(chunk) = stream.next().await {
            for (_, raw) in parser.push(&chunk.map_err(request_error)?) {
                finished |= append_event(&raw, &mut expected_index, &mut text)?;
            }
        }
        for (_, raw) in parser.finish() {
            finished |= append_event(&raw, &mut expected_index, &mut text)?;
        }
        if !finished {
            return Err(upstream_error());
        }
        Ok(text)
    })
    .await
    .map_err(|_| timeout_error())?
}

fn endpoint(credentials: &Value) -> Result<(&str, &str), ProxyError> {
    let region = credentials["region"].as_str().ok_or_else(login_error)?;
    let base = match region {
        "cn" => "https://agent.minimax.cn",
        "global" => "https://agent.minimax.io",
        _ => return Err(login_error()),
    };
    #[cfg(test)]
    let base = credentials["_voice_test_endpoint"].as_str().unwrap_or(base);
    Ok((base.trim_end_matches('/'), region))
}

async fn send(request: reqwest::RequestBuilder) -> Result<reqwest::Response, ProxyError> {
    let response = request.send().await.map_err(request_error)?;
    if !response.status().is_success() {
        let mut error = ProxyError::new(response.status(), "MiniMax speech request failed");
        error.retry_after = response.headers().get(header::RETRY_AFTER).cloned();
        return Err(error);
    }
    Ok(response)
}

fn append_event(raw: &str, expected: &mut usize, text: &mut String) -> Result<bool, ProxyError> {
    let value: Value = serde_json::from_str(raw).map_err(|_| upstream_error())?;
    if !value.is_object() {
        return Err(upstream_error());
    }
    check_business(&value)?;
    if let Some(nested) = value["data_list"][0]["data_value"]
        .as_str()
        .filter(|nested| !nested.is_empty())
    {
        let nested: Value = serde_json::from_str(nested).map_err(|_| upstream_error())?;
        check_business(&nested)?;
    }
    if value
        .get("index")
        .filter(|index| !index.is_null())
        .is_some_and(|index| index.as_f64() != Some(*expected as f64))
    {
        return Err(upstream_error());
    }
    *expected += 1;
    if let Some(delta) = value["data"].as_str() {
        text.push_str(delta);
    }
    Ok(value["finish"] == true)
}

fn check_business(value: &Value) -> Result<(), ProxyError> {
    if value["success"] == false
        || value.get("status_code").is_some_and(|code| !zero(code))
        || value["base_resp"]
            .get("status_code")
            .is_some_and(|code| !zero(code))
    {
        return Err(upstream_error());
    }
    Ok(())
}

fn zero(value: &Value) -> bool {
    value.as_f64() == Some(0.0)
        || value
            .as_str()
            .and_then(|code| code.trim().parse::<f64>().ok())
            == Some(0.0)
}

fn upstream_error() -> ProxyError {
    ProxyError::new(
        StatusCode::BAD_GATEWAY,
        "MiniMax speech response was invalid or incomplete",
    )
}

fn login_error() -> ProxyError {
    ProxyError::new(StatusCode::UNAUTHORIZED, "MiniMax speech login required")
}

fn timeout_error() -> ProxyError {
    ProxyError::new(
        StatusCode::GATEWAY_TIMEOUT,
        "MiniMax speech request timed out",
    )
}

fn request_error(error: reqwest::Error) -> ProxyError {
    if error.is_timeout() {
        timeout_error()
    } else {
        upstream_error()
    }
}

#[cfg(test)]
mod tests;
