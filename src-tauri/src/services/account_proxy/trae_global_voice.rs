use super::{audio_format, ProxyError};
use axum::{body::Bytes, http::StatusCode};
use futures_util::{SinkExt, StreamExt};
use hmac::{Hmac, Mac};
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, time::Duration};
use tokio_tungstenite::tungstenite::{protocol::frame::coding::CloseCode, Error, Message};

const PATH: &str = "/stream-transcription-websocket";
const FINISH_WAIT: Duration = Duration::from_secs(10);
const MAX_EVENT: usize = 1024 * 1024;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AwsCredentials {
    access_key_id: String,
    secret_access_key: String,
    session_token: String,
}

pub(crate) async fn transcribe(
    client: &reqwest::Client,
    credentials: &Value,
    audio: &Bytes,
    _filename: &str,
    mime: &str,
) -> Result<String, ProxyError> {
    let (grant_base, aws_region) = endpoints(credentials)?;
    let token = credentials["accessToken"]
        .as_str()
        .filter(|token| !token.trim().is_empty())
        .ok_or_else(login_required)?;
    let authorization =
        reqwest::header::HeaderValue::from_str(token).map_err(|_| login_required())?;
    let pcm = validated_pcm(audio, mime)?;
    tokio::time::timeout(
        super::voice::media_timeout(pcm.len()),
        recognize(
            client,
            credentials,
            grant_base,
            aws_region,
            authorization,
            pcm,
        ),
    )
    .await
    .map_err(|_| timed_out())?
}

pub(crate) fn validate_audio(audio: &Bytes, mime: &str) -> Result<(), ProxyError> {
    validated_pcm(audio, mime).map(|_| ())
}

fn validated_pcm<'a>(audio: &'a [u8], mime: &str) -> Result<&'a [u8], ProxyError> {
    if !mime.eq_ignore_ascii_case("audio/wav") && !mime.eq_ignore_ascii_case("audio/x-wav") {
        return Err(invalid_audio());
    }
    audio_format::pcm(audio).map_err(|_| invalid_audio())
}

fn endpoints(credentials: &Value) -> Result<(&'static str, &'static str), ProxyError> {
    let supported_scope = match credentials.get("accountScope") {
        None => true,
        Some(Value::String(scope)) => matches!(scope.as_str(), "" | "marscode"),
        _ => false,
    };
    if credentials["edition"] != "trae" || !supported_scope {
        return Err(login_required());
    }
    let region = match credentials.get("storeRegion") {
        Some(Value::String(region)) => region.as_str(),
        None => credentials["region"].as_str().unwrap_or(""),
        _ => return Err(login_required()),
    };
    match region {
        "SG" => Ok(("https://icube16-normal-sg.trae.ai", "ap-southeast-1")),
        "US" => Ok(("https://icube-normal.trae.ai", "us-west-2")),
        "USTTP" => Ok(("https://icube-normal.traeapi.us", "us-west-2")),
        _ => Err(login_required()),
    }
}

async fn recognize(
    client: &reqwest::Client,
    credentials: &Value,
    grant_base: &str,
    aws_region: &str,
    authorization: reqwest::header::HeaderValue,
    pcm: &[u8],
) -> Result<String, ProxyError> {
    #[cfg(not(test))]
    let _ = credentials;
    #[cfg(test)]
    let grant_base = credentials["_voice_test_endpoint"]
        .as_str()
        .unwrap_or(grant_base);
    let response = client
        .post(format!("{grant_base}/icube/api/v1/asr/get/b"))
        .header("x-icube-token", authorization)
        .header("content-type", "application/json")
        .timeout(Duration::from_secs(15))
        .send()
        .await
        .map_err(|error| {
            if error.is_timeout() {
                timed_out()
            } else {
                failed()
            }
        })?;
    if !response.status().is_success() {
        let mut error = ProxyError::new(response.status(), "TRAE speech authorization failed");
        error.retry_after = response.headers().get("retry-after").cloned();
        return Err(error);
    }
    let grant: Value = response.json().await.map_err(|_| failed())?;
    if grant["success"] != true {
        return Err(failed());
    }
    let grant: AwsCredentials =
        serde_json::from_value(grant["data"].clone()).map_err(|_| failed())?;
    if [
        &grant.access_key_id,
        &grant.secret_access_key,
        &grant.session_token,
    ]
    .iter()
    .any(|value| {
        value.trim().is_empty() || value.len() > 8192 || value.chars().any(char::is_control)
    }) {
        return Err(failed());
    }
    let timestamp = chrono::Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
    let url = signed_url(&grant, aws_region, &timestamp)?;
    #[cfg(test)]
    let url = if let Some(endpoint) = credentials["_voice_test_ws_endpoint"].as_str() {
        let mut endpoint = url::Url::parse(endpoint).map_err(|_| failed())?;
        endpoint.set_query(url::Url::parse(&url).map_err(|_| failed())?.query());
        endpoint.to_string()
    } else {
        url
    };
    let (mut socket, _) = tokio::time::timeout(
        Duration::from_secs(10),
        tokio_tungstenite::connect_async(url.as_str()),
    )
    .await
    .map_err(|_| timed_out())?
    .map_err(connection_error)?;
    let mut tick = tokio::time::interval(Duration::from_millis(100));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let finish_wait = FINISH_WAIT;
    #[cfg(test)]
    let finish_wait = credentials["_voice_test_finish_wait_ms"]
        .as_u64()
        .map(Duration::from_millis)
        .unwrap_or(finish_wait);
    let finish = tokio::time::sleep(finish_wait);
    tokio::pin!(finish);
    let mut cursor = 0;
    let mut sent_end = false;
    let mut transcript = Transcript::default();
    let mut pending = Vec::new();
    loop {
        tokio::select! {
            _ = tick.tick(), if !sent_end => {
                if cursor < pcm.len() {
                    let end = (cursor + 3200).min(pcm.len());
                    socket.send(Message::Binary(audio_event(&pcm[cursor..end]).into())).await.map_err(|_| failed())?;
                    cursor = end;
                } else {
                    socket.send(Message::Binary(audio_event(&[]).into())).await.map_err(|_| failed())?;
                    socket.send(Message::Binary(Vec::new().into())).await.map_err(|_| failed())?;
                    sent_end = true;
                    finish.as_mut().reset(tokio::time::Instant::now() + finish_wait);
                }
            }
            event = socket.next() => {
                match event.ok_or_else(failed)?.map_err(|_| failed())? {
                    Message::Binary(bytes) => {
                        if pending.len().saturating_add(bytes.len()) > MAX_EVENT { return Err(failed()); }
                        pending.extend_from_slice(&bytes);
                        while pending.len() >= 4 {
                            let length = u32::from_be_bytes(pending[..4].try_into().map_err(|_| failed())?) as usize;
                            if !(16..=MAX_EVENT).contains(&length) { return Err(failed()); }
                            if pending.len() < length { break; }
                            let (headers, body) = decode_event(&pending[..length])?;
                            transcript.receive(&headers, body)?;
                            pending.drain(..length);
                        }
                    }
                    Message::Ping(bytes) => { socket.send(Message::Pong(bytes)).await.map_err(|_| failed())?; }
                    Message::Pong(_) => {}
                    Message::Close(frame) => {
                        if !sent_end || !pending.is_empty() || !transcript.seen || !transcript.partial.is_empty()
                            || frame.is_some_and(|frame| frame.code != CloseCode::Normal) { return Err(failed()); }
                        let _ = socket.close(None).await;
                        return Ok(transcript.stable);
                    }
                    _ => return Err(failed()),
                }
            }
            _ = &mut finish, if sent_end => return Err(timed_out()),
        }
    }
}

fn signed_url(grant: &AwsCredentials, region: &str, timestamp: &str) -> Result<String, ProxyError> {
    let host = format!("transcribestreaming.{region}.amazonaws.com:8443");
    let scope = format!("{}/{region}/transcribe/aws4_request", &timestamp[..8]);
    let mut query = BTreeMap::from([
        ("X-Amz-Algorithm", "AWS4-HMAC-SHA256".to_owned()),
        (
            "X-Amz-Credential",
            format!("{}/{scope}", grant.access_key_id),
        ),
        ("X-Amz-Date", timestamp.to_owned()),
        ("X-Amz-Expires", "60".to_owned()),
        ("X-Amz-Security-Token", grant.session_token.clone()),
        ("X-Amz-SignedHeaders", "host".to_owned()),
        ("identify-language", "true".to_owned()),
        ("language-options", "en-US,zh-CN".to_owned()),
        ("media-encoding", "pcm".to_owned()),
        ("sample-rate", "16000".to_owned()),
        ("session-id", uuid::Uuid::new_v4().to_string()),
    ]);
    let canonical_query = encode_query(&query);
    let canonical = format!(
        "GET\n{PATH}\n{canonical_query}\nhost:{host}\n\nhost\n{}",
        hex::encode(Sha256::digest([]))
    );
    query.insert(
        "X-Amz-Signature",
        signature(grant, timestamp, region, "transcribe", &canonical)?,
    );
    Ok(format!("wss://{host}{PATH}?{}", encode_query(&query)))
}

fn encode_query(query: &BTreeMap<&str, String>) -> String {
    query
        .iter()
        .map(|(key, value)| format!("{}={}", encode(key), encode(value)))
        .collect::<Vec<_>>()
        .join("&")
}

fn encode(value: &str) -> String {
    let mut result = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || b"-_.~".contains(&byte) {
            result.push(char::from(byte));
        } else {
            result.push_str(&format!("%{byte:02X}"));
        }
    }
    result
}

fn signature(
    grant: &AwsCredentials,
    timestamp: &str,
    region: &str,
    service: &str,
    canonical: &str,
) -> Result<String, ProxyError> {
    let date = &timestamp[..8];
    let mut key = format!("AWS4{}", grant.secret_access_key).into_bytes();
    for value in [date, region, service, "aws4_request"] {
        key = hmac(&key, value.as_bytes())?;
    }
    let input = format!(
        "AWS4-HMAC-SHA256\n{timestamp}\n{date}/{region}/{service}/aws4_request\n{}",
        hex::encode(Sha256::digest(canonical.as_bytes()))
    );
    Ok(hex::encode(hmac(&key, input.as_bytes())?))
}

fn hmac(key: &[u8], input: &[u8]) -> Result<Vec<u8>, ProxyError> {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).map_err(|_| failed())?;
    mac.update(input);
    Ok(mac.finalize().into_bytes().to_vec())
}

fn audio_event(payload: &[u8]) -> Vec<u8> {
    event_frame(
        &[
            (":event-type", "AudioEvent"),
            (":message-type", "event"),
            (":content-type", "application/octet-stream"),
        ],
        payload,
    )
}

fn event_frame(headers: &[(&str, &str)], payload: &[u8]) -> Vec<u8> {
    let mut header_bytes = Vec::new();
    for (name, value) in headers {
        header_bytes.push(name.len() as u8);
        header_bytes.extend_from_slice(name.as_bytes());
        header_bytes.push(7);
        header_bytes.extend_from_slice(&(value.len() as u16).to_be_bytes());
        header_bytes.extend_from_slice(value.as_bytes());
    }
    let mut frame = Vec::new();
    frame.extend_from_slice(&((16 + header_bytes.len() + payload.len()) as u32).to_be_bytes());
    frame.extend_from_slice(&(header_bytes.len() as u32).to_be_bytes());
    frame.extend_from_slice(&crc32(&frame).to_be_bytes());
    frame.extend_from_slice(&header_bytes);
    frame.extend_from_slice(payload);
    frame.extend_from_slice(&crc32(&frame).to_be_bytes());
    frame
}

fn decode_event(bytes: &[u8]) -> Result<(BTreeMap<String, String>, &[u8]), ProxyError> {
    if bytes.len() < 16 {
        return Err(failed());
    }
    let number = |range| {
        bytes
            .get(range)
            .and_then(|value: &[u8]| value.try_into().ok())
            .map(u32::from_be_bytes)
            .ok_or_else(failed)
    };
    let length = number(0..4)? as usize;
    let headers_len = number(4..8)? as usize;
    if length != bytes.len()
        || headers_len > length - 16
        || number(8..12)? != crc32(&bytes[..8])
        || number(length - 4..length)? != crc32(&bytes[..length - 4])
    {
        return Err(failed());
    }
    let header_end = 12 + headers_len;
    let mut cursor = 12;
    let mut headers = BTreeMap::new();
    while cursor < header_end {
        let name_len = usize::from(bytes[cursor]);
        cursor += 1;
        let name = std::str::from_utf8(
            bytes
                .get(cursor..cursor + name_len)
                .filter(|_| cursor + name_len < header_end)
                .ok_or_else(failed)?,
        )
        .map_err(|_| failed())?;
        cursor += name_len;
        let kind = bytes[cursor];
        cursor += 1;
        let size = match kind {
            0 | 1 => 0,
            2 => 1,
            3 => 2,
            4 => 4,
            5 | 8 => 8,
            9 => 16,
            6 | 7 => {
                let raw = bytes
                    .get(cursor..cursor + 2)
                    .filter(|_| cursor + 2 <= header_end)
                    .ok_or_else(failed)?;
                cursor += 2;
                usize::from(u16::from_be_bytes(raw.try_into().map_err(|_| failed())?))
            }
            _ => return Err(failed()),
        };
        let value = bytes
            .get(cursor..cursor + size)
            .filter(|_| cursor + size <= header_end)
            .ok_or_else(failed)?;
        if kind == 7 {
            let value = std::str::from_utf8(value).map_err(|_| failed())?;
            if headers.insert(name.to_owned(), value.to_owned()).is_some() {
                return Err(failed());
            }
        }
        cursor += size;
    }
    Ok((headers, &bytes[header_end..length - 4]))
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut value = u32::MAX;
    for byte in bytes {
        value ^= u32::from(*byte);
        for _ in 0..8 {
            value = (value >> 1) ^ (0xedb8_8320_u32.wrapping_mul(value & 1));
        }
    }
    !value
}

#[derive(Default)]
struct Transcript {
    seen: bool,
    stable: String,
    partial: String,
}

impl Transcript {
    fn receive(
        &mut self,
        headers: &BTreeMap<String, String>,
        body: &[u8],
    ) -> Result<(), ProxyError> {
        if headers.get(":message-type").map(String::as_str) == Some("exception") {
            return Err(ProxyError::new(
                match headers.get(":exception-type").map(String::as_str) {
                    Some("UnrecognizedClientException") => StatusCode::UNAUTHORIZED,
                    Some("LimitExceededException") => StatusCode::TOO_MANY_REQUESTS,
                    Some("BadRequestException") => StatusCode::BAD_REQUEST,
                    _ => StatusCode::BAD_GATEWAY,
                },
                "TRAE speech stream failed",
            ));
        }
        if headers.get(":message-type").map(String::as_str) != Some("event")
            || headers.get(":event-type").map(String::as_str) != Some("TranscriptEvent")
        {
            return Err(failed());
        }
        let event: Value = serde_json::from_slice(body).map_err(|_| failed())?;
        let results = event["Transcript"]["Results"]
            .as_array()
            .ok_or_else(failed)?;
        self.seen = true;
        if let Some(result) = results.first() {
            let partial = result["IsPartial"].as_bool().ok_or_else(failed)?;
            let text = result["Alternatives"][0]["Transcript"]
                .as_str()
                .ok_or_else(failed)?;
            if partial {
                self.partial = text.to_owned();
            } else {
                self.stable.push_str(text);
                self.partial.clear();
            }
        }
        Ok(())
    }
}

fn connection_error(error: Error) -> ProxyError {
    if let Error::Http(response) = error {
        let mut error = ProxyError::new(response.status(), "TRAE speech connection failed");
        error.retry_after = response.headers().get("retry-after").cloned();
        return error;
    }
    failed()
}

fn login_required() -> ProxyError {
    ProxyError::new(
        StatusCode::UNAUTHORIZED,
        "TRAE speech requires a supported international account",
    )
}
fn invalid_audio() -> ProxyError {
    ProxyError::new(
        StatusCode::UNSUPPORTED_MEDIA_TYPE,
        "TRAE speech requires 16 kHz mono PCM16 WAV audio",
    )
}
fn failed() -> ProxyError {
    ProxyError::new(StatusCode::BAD_GATEWAY, "TRAE speech request failed")
}
fn timed_out() -> ProxyError {
    ProxyError::new(StatusCode::GATEWAY_TIMEOUT, "TRAE speech request timed out")
}

#[cfg(test)]
mod tests;
