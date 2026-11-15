use super::ProxyError;
use axum::{body::Bytes, http::StatusCode};
use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use std::time::Duration;
use tokio::time::{Instant, MissedTickBehavior};
use tokio_tungstenite::tungstenite::{
    client::IntoClientRequest, protocol::frame::coding::CloseCode, Error, Message,
};

const ENDPOINT: &str = "wss://api.anthropic.com";
const CHUNK_BYTES: usize = 32_000;

pub(crate) async fn transcribe(
    _client: &reqwest::Client,
    credentials: &Value,
    audio: &Bytes,
    _filename: &str,
    mime: &str,
) -> Result<String, ProxyError> {
    if !mime.eq_ignore_ascii_case("audio/wav") && !mime.eq_ignore_ascii_case("audio/x-wav") {
        return Err(ProxyError::new(
            StatusCode::BAD_REQUEST,
            "Claude speech requires WAV audio",
        ));
    }
    let pcm = pcm(audio)?;
    tokio::time::timeout(Duration::from_secs(60), recognize(credentials, pcm))
        .await
        .map_err(|_| timed_out())?
}

pub(super) fn pcm(audio: &[u8]) -> Result<&[u8], ProxyError> {
    let invalid = || {
        ProxyError::new(
            StatusCode::BAD_REQUEST,
            "Claude speech requires 16 kHz mono PCM16 WAV audio",
        )
    };
    if audio.len() <= 44
        || &audio[..4] != b"RIFF"
        || &audio[8..16] != b"WAVEfmt "
        || &audio[36..40] != b"data"
    {
        return Err(invalid());
    }
    let word = |at| u16::from_le_bytes([audio[at], audio[at + 1]]);
    let dword =
        |at| u32::from_le_bytes([audio[at], audio[at + 1], audio[at + 2], audio[at + 3]]) as usize;
    if dword(4) != audio.len() - 8
        || dword(16) != 16
        || word(20) != 1
        || word(22) != 1
        || dword(24) != 16_000
        || dword(28) != 32_000
        || word(32) != 2
        || word(34) != 16
        || dword(40) != audio.len() - 44
        || !(audio.len() - 44).is_multiple_of(2)
    {
        return Err(invalid());
    }
    Ok(&audio[44..])
}

async fn recognize(credentials: &Value, pcm: &[u8]) -> Result<String, ProxyError> {
    let token = credentials["accessToken"]
        .as_str()
        .filter(|value| !value.is_empty())
        .ok_or_else(login_required)?;
    let endpoint = ENDPOINT;
    #[cfg(test)]
    let endpoint = credentials["_voice_test_endpoint"]
        .as_str()
        .unwrap_or(endpoint);
    let mut url = url::Url::parse(endpoint).map_err(|_| failed())?;
    url.set_path("/api/ws/speech_to_text/voice_stream");
    url.set_query(None);
    url.query_pairs_mut()
        .append_pair("encoding", "linear16")
        .append_pair("sample_rate", "16000")
        .append_pair("channels", "1")
        .append_pair("endpointing_ms", "300")
        .append_pair("utterance_end_ms", "1000")
        .append_pair("language", "en")
        .append_pair("use_conversation_engine", "true");
    let mut request = url.as_str().into_client_request().map_err(|_| failed())?;
    for (name, value) in [
        ("authorization", format!("Bearer {token}")),
        (
            "user-agent",
            concat!("EchoBird/", env!("CARGO_PKG_VERSION")).to_owned(),
        ),
        ("x-app", "cli".to_owned()),
        ("anthropic-client-platform", "claude_code_cli".to_owned()),
    ] {
        request
            .headers_mut()
            .insert(name, value.parse().map_err(|_| login_required())?);
    }
    let (mut socket, _) = tokio::time::timeout(
        Duration::from_secs(10),
        tokio_tungstenite::connect_async(request),
    )
    .await
    .map_err(|_| timed_out())?
    .map_err(|error| match error {
        Error::Http(response) => {
            let mut error = ProxyError::new(response.status(), "Claude speech connection failed");
            error.retry_after = response.headers().get("retry-after").cloned();
            error
        }
        _ => failed(),
    })?;
    socket
        .send(Message::Text(r#"{"type":"KeepAlive"}"#.into()))
        .await
        .map_err(|_| failed())?;
    let mut keep_alive = tokio::time::interval_at(
        Instant::now() + Duration::from_secs(8),
        Duration::from_secs(8),
    );
    keep_alive.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let finish = tokio::time::sleep(Duration::from_secs(5));
    tokio::pin!(finish);
    let mut cursor = 0;
    let mut closing = false;
    let mut text = String::new();
    let mut pending = String::new();
    loop {
        tokio::select! {
            _ = tokio::task::yield_now(), if !closing => {
                if cursor < pcm.len() {
                    let end = (cursor + CHUNK_BYTES).min(pcm.len());
                    socket.send(Message::Binary(pcm[cursor..end].to_vec().into()))
                        .await.map_err(|_| failed())?;
                    cursor = end;
                } else {
                    socket.send(Message::Text(r#"{"type":"CloseStream"}"#.into()))
                        .await.map_err(|_| failed())?;
                    closing = true;
                    finish.as_mut().reset(Instant::now() + Duration::from_secs(5));
                }
            }
            _ = keep_alive.tick(), if !closing => {
                socket.send(Message::Text(r#"{"type":"KeepAlive"}"#.into()))
                    .await.map_err(|_| failed())?;
            }
            _ = &mut finish, if closing => return Err(timed_out()),
            message = socket.next() => {
                let message = message.ok_or_else(failed)?.map_err(|_| failed())?;
                let bytes = match message {
                    Message::Text(value) => value.as_bytes().to_vec(),
                    Message::Binary(value) => value.to_vec(),
                    Message::Ping(value) => {
                        socket.send(Message::Pong(value)).await.map_err(|_| failed())?;
                        continue;
                    }
                    Message::Close(frame) => {
                        if !closing || frame.is_some_and(|value| value.code != CloseCode::Normal) {
                            return Err(failed());
                        }
                        append_segment(&mut text, &pending);
                        let _ = socket.close(None).await;
                        return Ok(text);
                    }
                    _ => continue,
                };
                let Ok(event) = serde_json::from_slice::<Value>(&bytes) else { continue };
                match event["type"].as_str() {
                    Some("TranscriptInterim" | "TranscriptText") => {
                        if let Some(value) = event["data"].as_str().filter(|value| !value.is_empty()) {
                            pending.clear();
                            pending.push_str(value);
                        }
                    }
                    Some("TranscriptEndpoint") => {
                        append_segment(&mut text, &pending);
                        pending.clear();
                        if closing {
                            let _ = socket.close(None).await;
                            return Ok(text);
                        }
                    }
                    Some("TranscriptError" | "error") => return Err(failed()),
                    _ => {}
                }
            }
        }
    }
}

fn append_segment(text: &mut String, segment: &str) {
    let segment = segment.trim();
    if !segment.is_empty() {
        if !text.is_empty() {
            text.push(' ');
        }
        text.push_str(segment);
    }
}

fn login_required() -> ProxyError {
    ProxyError::new(StatusCode::UNAUTHORIZED, "Claude speech login required")
}

fn failed() -> ProxyError {
    ProxyError::new(StatusCode::BAD_GATEWAY, "Claude speech recognition failed")
}

fn timed_out() -> ProxyError {
    ProxyError::new(
        StatusCode::GATEWAY_TIMEOUT,
        "Claude speech recognition timed out",
    )
}

#[cfg(test)]
mod tests;
