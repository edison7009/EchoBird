use super::ProxyError;
use axum::{
    body::Bytes,
    http::{HeaderMap, StatusCode},
};
use base64::{engine::general_purpose::STANDARD, Engine};
use cbc::cipher::{block_padding::Pkcs7, BlockEncryptMut, KeyIvInit};
use futures_util::{SinkExt, StreamExt};
use md5::{Digest, Md5};
use rsa::{pkcs8::DecodePublicKey, Pkcs1v15Encrypt, RsaPublicKey};
use serde_json::{json, Value};
use std::time::Duration;
use tokio::time::Instant;
use tokio_tungstenite::tungstenite::{client::IntoClientRequest, Error, Message};

const PATH: &str = "/api/v2/service/ws/asr";
const CHUNK_BYTES: usize = 3_200;
const PUBLIC_KEY: &str = "-----BEGIN PUBLIC KEY-----\nMIGfMA0GCSqGSIb3DQEBAQUAA4GNADCBiQKBgQDA8iMH5c02LilrsERw9t6Pv5Nc\n4k6Pz1EaDicBMpdpxKduSZu5OANqUq8er4GM95omAGIOPOh+Nx0spthYA2BqGz+l\n6HRkPJ7S236FZz73In/KVuLnwI8JJ2CbuJap8kvheCCZpmAWpb/cPx/3Vr/J6I17\nXcW+ML9FoCI6AOvOzwIDAQAB\n-----END PUBLIC KEY-----";

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
            "QwenWork speech requires WAV audio",
        ));
    }
    let pcm = pcm(audio)?;
    tokio::time::timeout(
        super::voice::media_timeout(pcm.len()),
        recognize(credentials, pcm),
    )
    .await
    .map_err(|_| timed_out())?
}

fn pcm(audio: &[u8]) -> Result<&[u8], ProxyError> {
    let invalid = || {
        ProxyError::new(
            StatusCode::BAD_REQUEST,
            "QwenWork speech requires 16 kHz mono PCM16 WAV audio",
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

fn endpoint(credentials: &Value) -> Result<&'static str, ProxyError> {
    match credentials["edition"].as_str() {
        Some("qwenworkcn") => Ok("wss://gateway.qwenwork.cn"),
        Some("qwenwork") => Ok("wss://gateway.qwenwork.ai"),
        _ => Err(login_required()),
    }
}

fn required<'a>(credentials: &'a Value, field: &str) -> Result<&'a str, ProxyError> {
    credentials[field]
        .as_str()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(login_required)
}

fn signed_headers(credentials: &Value, public_key: &RsaPublicKey) -> Result<HeaderMap, ProxyError> {
    let token = required(credentials, "accessToken")?;
    let user_id = required(credentials, "userId")?;
    let version = required(credentials, "version")?;
    let key = uuid::Uuid::new_v4().simple().to_string();
    let key = &key.as_bytes()[..16];
    let user = json!({
        "uid":user_id,"aid":"","name":credentials["name"].as_str().unwrap_or(""),
        "email":credentials["email"].as_str().unwrap_or(""),"security_oauth_token":token,
    });
    let info = STANDARD.encode(
        cbc::Encryptor::<aes::Aes128>::new_from_slices(key, key)
            .map_err(|_| failed())?
            .encrypt_padded_vec_mut::<Pkcs7>(user.to_string().as_bytes()),
    );
    let wrapped = STANDARD.encode(
        public_key
            .encrypt(&mut rand::thread_rng(), Pkcs1v15Encrypt, key)
            .map_err(|_| failed())?,
    );
    let payload = STANDARD.encode(
        json!({"version":"v1","requestId":uuid::Uuid::new_v4().simple().to_string(),
            "info":info,"cosyVersion":"1.0.0","ideVersion":version})
        .to_string(),
    );
    let timestamp = chrono::Utc::now().timestamp().to_string();
    let signature = format!(
        "{:x}",
        Md5::digest(format!("{payload}\n{wrapped}\n{timestamp}\n\n{PATH}"))
    );
    let business_id = uuid::Uuid::new_v4().to_string();
    let business = json!({"product":"qoder_work","type":"asr_chat","id":business_id,
        "begin_at":chrono::Utc::now().timestamp_millis(),"name":format!("asr_chat-{business_id}")});
    let platform = match std::env::consts::OS {
        "windows" => "win32",
        "macos" => "darwin",
        other => other,
    };
    let mut headers = HeaderMap::new();
    for (name, value) in [
        (
            "authorization",
            format!("Bearer COSY.{payload}.{signature}"),
        ),
        ("cosy-user", user_id.to_owned()),
        ("cosy-key", wrapped),
        ("cosy-date", timestamp),
        ("x-request-id", uuid::Uuid::new_v4().simple().to_string()),
        ("x-ide-platform", "qoder_work".to_owned()),
        ("x-version", version.to_owned()),
        ("x-machine-os", platform.to_owned()),
        ("samplerate", "16000".to_owned()),
        ("framedurationms", "100".to_owned()),
        ("bitdepth", "16".to_owned()),
        ("channels", "1".to_owned()),
        ("x-asr-session-id", uuid::Uuid::new_v4().to_string()),
        ("x-business", business.to_string()),
    ] {
        headers.insert(name, value.parse().map_err(|_| login_required())?);
    }
    Ok(headers)
}

async fn recognize(credentials: &Value, pcm: &[u8]) -> Result<String, ProxyError> {
    #[cfg(test)]
    let trace_start = Instant::now();
    let endpoint = endpoint(credentials)?;
    #[cfg(test)]
    trace_event(
        trace_start,
        "start",
        json!({"region":credentials["edition"],"pcm_bytes":pcm.len(),"audio_ms":pcm.len() as u64 * 1000 / 32_000,"paced":true}),
    );
    #[cfg(test)]
    let endpoint = credentials["_voice_test_endpoint"]
        .as_str()
        .unwrap_or(endpoint);
    let mut url = url::Url::parse(endpoint).map_err(|_| failed())?;
    url.set_path(PATH);
    url.set_query(None);
    let public_key = RsaPublicKey::from_public_key_pem(PUBLIC_KEY).map_err(|_| failed())?;
    let mut request = url.as_str().into_client_request().map_err(|_| failed())?;
    request
        .headers_mut()
        .extend(signed_headers(credentials, &public_key)?);
    #[cfg(test)]
    let trace_session = request.headers()["x-asr-session-id"].clone();
    let (mut socket, _) = tokio::time::timeout(
        Duration::from_secs(10),
        tokio_tungstenite::connect_async(request),
    )
    .await
    .map_err(|_| {
        #[cfg(test)]
        trace_event(trace_start, "return", json!({"reason":"handshake_timeout"}));
        timed_out()
    })?
    .map_err(|error| match error {
        Error::Http(response) => {
            #[cfg(test)]
            trace_event(
                trace_start,
                "return",
                json!({"reason":"handshake_rejected","status":response.status().as_u16()}),
            );
            let mut error = ProxyError::new(response.status(), "QwenWork speech connection failed");
            error.retry_after = response.headers().get("retry-after").cloned();
            error
        }
        _ => {
            #[cfg(test)]
            trace_event(trace_start, "return", json!({"reason":"handshake_error"}));
            failed()
        }
    })?;
    #[cfg(test)]
    trace_event(trace_start, "connected", json!({}));
    let media_start = Instant::now();
    let finish = tokio::time::sleep(Duration::from_secs(3));
    tokio::pin!(finish);
    let mut cursor = 0;
    let mut closing = false;
    let mut connection_ended = false;
    let mut final_text: Option<String> = None;
    let mut has_text = false;
    #[cfg(test)]
    let mut trace_settle = false;
    loop {
        let end = (cursor + CHUNK_BYTES).min(pcm.len());
        let media_time = Duration::from_micros(end as u64 * 1_000_000 / 32_000);
        tokio::select! {
            _ = tokio::time::sleep_until(media_start + media_time), if !closing => {
                if cursor < pcm.len() {
                    socket.send(Message::Binary(pcm[cursor..end].to_vec().into()))
                        .await.map_err(|_| failed())?;
                    #[cfg(test)]
                    if cursor == 0 || end == pcm.len() {
                        trace_event(trace_start, "audio", json!({"first":cursor==0,"last":end==pcm.len(),"frame_bytes":end-cursor,"sent_bytes":end,"frame":end.div_ceil(CHUNK_BYTES)}));
                    }
                    cursor = end;
                } else {
                    socket.send(Message::Text(
                        r#"{"type":"voice_completed","message":"close by user"}"#.into(),
                    )).await.map_err(|_| failed())?;
                    closing = true;
                    #[cfg(test)]
                    trace_event(trace_start, "completion_sent", json!({"sent_bytes":cursor,"pcm_bytes":pcm.len(),"frames":cursor.div_ceil(CHUNK_BYTES),"final_chars":final_text.as_deref().map(str::chars).map(Iterator::count)}));
                    finish.as_mut().reset(Instant::now() + Duration::from_secs(3));
                }
            }
            _ = &mut finish, if closing => {
                #[cfg(test)]
                trace_event(trace_start, "return", json!({"reason":if final_text.is_some() {if trace_settle {"final_settle"}else{"final_at_hard_deadline"}}else{"final_timeout"},"final_chars":final_text.as_deref().map(str::chars).map(Iterator::count)}));
                let text = final_text.ok_or_else(timed_out)?;
                let _ = socket.close(None).await;
                return Ok(text.trim().to_owned());
            }
            message = socket.next(), if !connection_ended => {
                let message = match message {
                    Some(Ok(message)) => message,
                    _ if closing && final_text.is_some() => {
                        #[cfg(test)]
                        trace_event(trace_start, "transport_after_final", json!({"final_chars":final_text.as_deref().map(str::chars).map(Iterator::count)}));
                        // Native retains its final-result settle timer after a transport close.
                        connection_ended = true;
                        continue;
                    }
                    _ => {
                        #[cfg(test)]
                        trace_event(trace_start, "return", json!({"reason":"transport_without_final","closing":closing,"sent_bytes":cursor}));
                        return Err(failed());
                    }
                };
                let bytes = match message {
                    Message::Text(value) => value.as_bytes().to_vec(),
                    Message::Binary(value) => value.to_vec(),
                    Message::Ping(value) => {
                        socket.send(Message::Pong(value)).await.map_err(|_| failed())?;
                        continue;
                    }
                    Message::Close(_) if closing && final_text.is_some() => {
                        #[cfg(test)]
                        trace_event(trace_start, "close_after_final", json!({"final_chars":final_text.as_deref().map(str::chars).map(Iterator::count)}));
                        connection_ended = true;
                        continue;
                    }
                    Message::Close(_) => {
                        #[cfg(test)]
                        trace_event(trace_start, "return", json!({"reason":"close_without_final","closing":closing,"sent_bytes":cursor}));
                        return Err(failed());
                    }
                    _ => continue,
                };
                let Ok(event) = serde_json::from_slice::<Value>(&bytes) else {
                    #[cfg(test)]
                    trace_event(trace_start, "non_json", json!({"wire_bytes":bytes.len()}));
                    continue;
                };
                #[cfg(test)]
                {
                    let text = event["message"].as_str();
                    let kind = match event["type"].as_str() {
                        Some("speech_completed") => "speech_completed",
                        Some("speech_done") => "speech_done",
                        Some("speech_err") => "speech_err",
                        Some("speech_cost") => "speech_cost",
                        Some("speech_interim") => "speech_interim",
                        Some("speech_partial") => "speech_partial",
                        Some("error") => "error",
                        _ => "other",
                    };
                    let status = match event["status"].as_str() {
                        Some("completed"|"success"|"ok") => "success",
                        Some("failed"|"error") => "failure",
                        Some(_) => "other",
                        None => "absent_or_numeric",
                    };
                    trace_event(trace_start, "received", json!({
                        "type":kind,"wire_bytes":bytes.len(),
                        "keys":event.as_object().map(|value|value.keys().take(32).collect::<Vec<_>>()),
                        "message_present":event.get("message").is_some(),"message_is_string":text.is_some(),
                        "message_chars":text.map(|value|value.chars().count()),"message_empty":text.map(str::is_empty),
                        "has_synthetic_tail":text.is_some_and(|value|value.to_lowercase().contains("coffee")||value.contains("咖啡")),
                        "equals_previous_final":text.is_some_and(|value|final_text.as_deref()==Some(value.trim())),
                        "status":status,"status_number":event["status"].as_i64(),"code_number":event["code"].as_i64(),
                        "session_matches":event["asr_session_id"].as_str().map(|value|value.as_bytes()==trace_session.as_bytes()),
                        "closing":closing,"sent_bytes":cursor,"final_before_chars":final_text.as_deref().map(str::chars).map(Iterator::count)
                    }));
                }
                match event["type"].as_str() {
                    Some("speech_err" | "error") => {
                        #[cfg(test)]
                        trace_event(trace_start, "return", json!({"reason":"speech_err"}));
                        return Err(failed());
                    }
                    Some("speech_done") => {
                        if !closing || (final_text.is_none() && has_text) {
                            #[cfg(test)]
                            trace_event(trace_start, "return", json!({"reason":"done_without_final","closing":closing,"had_text":has_text}));
                            return Err(failed());
                        }
                        #[cfg(test)]
                        trace_event(trace_start, "return", json!({"reason":"speech_done","final_chars":final_text.as_deref().map(str::chars).map(Iterator::count)}));
                        let _ = socket.close(None).await;
                        return Ok(final_text.unwrap_or_default().trim().to_owned());
                    }
                    Some("speech_completed") => {
                        if let Some(text) = event["message"].as_str().filter(|text|!text.is_empty()) {
                            final_text.get_or_insert_with(String::new).push_str(text);
                            if closing {
                                finish.as_mut().reset(Instant::now() + Duration::from_secs(1));
                                #[cfg(test)]
                                { trace_settle = true; }
                            }
                        }
                    }
                    Some("speech_cost") => {}
                    _ => {
                        has_text |= event["message"].as_str().is_some_and(|text| !text.is_empty());
                    }
                }
            }
        }
    }
}

#[cfg(test)]
fn trace_event(start: Instant, event: &str, details: Value) {
    if matches!(
        std::env::var("ECHOBIRD_ACCOUNT_VOICE_TEST_TRACE").as_deref(),
        Ok("1")
    ) {
        eprintln!(
            "{}",
            json!({"qwenwork_speech":event,"ms":start.elapsed().as_millis(),"details":details})
        );
    }
}

fn login_required() -> ProxyError {
    ProxyError::new(StatusCode::UNAUTHORIZED, "QwenWork speech login required")
}

fn failed() -> ProxyError {
    ProxyError::new(
        StatusCode::BAD_GATEWAY,
        "QwenWork speech recognition failed",
    )
}

fn timed_out() -> ProxyError {
    ProxyError::new(
        StatusCode::GATEWAY_TIMEOUT,
        "QwenWork speech recognition timed out",
    )
}

#[cfg(test)]
mod tests;
