use super::{audio_format, ProxyError};
use axum::{body::Bytes, http::StatusCode};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::{io::Read, time::Duration};
use tokio_tungstenite::tungstenite::{Error, Message};

const BASE: &str = "https://api.trae.com.cn";
const SPEECH_URL: &str = "wss://openspeech.bytedance.com/api/v3/sauc/bigmodel";

pub(super) fn validate_audio(audio: &Bytes, mime: &str) -> Result<(), ProxyError> {
    if !matches!(mime, "audio/wav" | "audio/x-wav") {
        return Err(invalid_audio());
    }
    audio_format::pcm(audio)
        .map(|_| ())
        .map_err(|_| invalid_audio())
}

pub(crate) async fn transcribe(
    client: &reqwest::Client,
    credentials: &Value,
    audio: &Bytes,
    _filename: &str,
    mime: &str,
) -> Result<String, ProxyError> {
    if credentials["edition"] != "traecn" || credentials["region"] != "CN" {
        return Err(login_required());
    }
    validate_audio(audio, mime)?;
    let pcm = audio_format::pcm(audio).map_err(|_| invalid_audio())?;
    tokio::time::timeout(
        super::voice::media_timeout(pcm.len()),
        recognize(client, credentials, pcm),
    )
    .await
    .map_err(|_| timed_out())?
}

async fn recognize(
    client: &reqwest::Client,
    credentials: &Value,
    pcm: &[u8],
) -> Result<String, ProxyError> {
    let token = credentials["accessToken"]
        .as_str()
        .filter(|value| !value.is_empty())
        .ok_or_else(login_required)?;
    let authorization =
        reqwest::header::HeaderValue::from_str(token).map_err(|_| login_required())?;
    let base = BASE;
    #[cfg(test)]
    let base = credentials["_voice_test_endpoint"].as_str().unwrap_or(base);
    let response = client
        .post(format!("{base}/icube/api/v2/asr/get/a"))
        .header("x-icube-token", authorization)
        .header("content-type", "application/json")
        .timeout(Duration::from_secs(15))
        .send()
        .await
        .map_err(|error| {
            #[cfg(test)]
            trace("grant_transport", json!({"timeout":error.is_timeout(),"connect":error.is_connect(),"body":error.is_body(),"decode":error.is_decode()}));
            if error.is_timeout() {
                timed_out()
            } else {
                failed()
            }
        })?;
    #[cfg(test)]
    trace("grant_http", json!({"status":response.status().as_u16()}));
    if !response.status().is_success() {
        let mut error = ProxyError::new(response.status(), "TRAE speech authorization failed");
        error.retry_after = response.headers().get("retry-after").cloned();
        return Err(error);
    }
    let grant: Value = response.json().await.map_err(|_| {
        #[cfg(test)]
        trace("grant_json", json!({"decoded":false}));
        failed()
    })?;
    #[cfg(test)]
    trace(
        "grant_shape",
        json!({
            "success_type":field_type(grant.get("success")), "success_is_true":grant["success"]==true,
            "code":grant["code"].as_i64(), "error_code":grant["error_code"].as_i64(),
            "data_type":field_type(grant.get("data")), "jwt_type":field_type(grant["data"].get("jwt_token")),
            "jwt_nonempty":grant["data"]["jwt_token"].as_str().is_some_and(|value|!value.is_empty()),
            "app_id_type":field_type(grant["data"].get("app_id")), "uid_type":field_type(grant["data"].get("uid")),
            "ws_url_type":field_type(grant["data"].get("ws_url"))
        }),
    );
    if grant["success"] != true {
        return Err(failed());
    }
    let jwt = grant["data"]["jwt_token"]
        .as_str()
        .filter(|value| !value.is_empty())
        .ok_or_else(failed)?;
    let app = grant["data"]["app_id"]
        .as_str()
        .filter(|value| !value.is_empty())
        .unwrap_or("4089030846");
    let uid = grant["data"]["uid"].as_str().unwrap_or("");
    let mut url = url::Url::parse(
        grant["data"]["ws_url"]
            .as_str()
            .filter(|value| !value.is_empty())
            .unwrap_or(SPEECH_URL),
    )
    .map_err(|_| failed())?;
    #[cfg(test)]
    trace(
        "ws_target",
        json!({"scheme_is_wss":url.scheme()=="wss", "host_is_official":url.host_str()==Some("openspeech.bytedance.com"), "path_is_bigmodel":url.path()=="/api/v3/sauc/bigmodel", "has_port":url.port().is_some(), "has_userinfo":!url.username().is_empty()||url.password().is_some(), "has_query":url.query().is_some()}),
    );
    if url.scheme() != "wss"
        || url.host_str() != Some("openspeech.bytedance.com")
        || url.path() != "/api/v3/sauc/bigmodel"
        || url.port().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(failed());
    }
    #[cfg(test)]
    if let Some(endpoint) = credentials["_voice_test_ws_endpoint"].as_str() {
        url = url::Url::parse(endpoint).map_err(|_| failed())?;
    }
    url.set_query(None);
    url.set_fragment(None);
    url.query_pairs_mut()
        .append_pair("api_resource_id", "volc.bigasr.sauc.duration")
        .append_pair("api_app_key", app)
        .append_pair("api_access_key", &format!("Jwt; {jwt}"));
    let (mut socket, _) = tokio::time::timeout(
        Duration::from_secs(10),
        tokio_tungstenite::connect_async(url.as_str()),
    )
    .await
    .map_err(|_| {
        #[cfg(test)]
        trace(
            "ws_handshake",
            json!({"success":false,"error_kind":"timeout"}),
        );
        timed_out()
    })?
    .map_err(|error| {
        #[cfg(test)]
        trace_ws_error(&error);
        match error {
            Error::Http(response) => {
                let mut error = ProxyError::new(response.status(), "TRAE speech connection failed");
                error.retry_after = response.headers().get("retry-after").cloned();
                error
            }
            _ => failed(),
        }
    })?;
    #[cfg(test)]
    trace("ws_handshake", json!({"success":true,"status":101}));
    let init = json!({"user":{"uid":uid},"audio":{"format":"pcm","rate":16000,"bits":16,"channel":1},"request":{"model_name":"bigmodel","show_utterances":true}});
    socket
        .send(Message::Binary(
            frame(0x10, 0x10, init.to_string().as_bytes()).into(),
        ))
        .await
        .map_err(|_| failed())?;
    #[cfg(test)]
    trace("init_sent", json!({"sent":true}));
    let mut tick = tokio::time::interval(Duration::from_millis(100));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut cursor = 0;
    let mut sent_end = false;
    let mut text = String::new();
    loop {
        tokio::select! {
            _ = tick.tick(), if !sent_end => {
                if cursor < pcm.len() {
                    let end = (cursor + 3200).min(pcm.len());
                    socket.send(Message::Binary(frame(0x20, 0, &pcm[cursor..end]).into())).await.map_err(|_| failed())?;
                    cursor = end;
                } else {
                    socket.send(Message::Binary(frame(0x22, 0, &[]).into())).await.map_err(|_| failed())?;
                    sent_end = true;
                    #[cfg(test)]
                    trace("end_sent", json!({"pcm_bytes":cursor}));
                }
            }
            event = socket.next() => {
                let event = event.ok_or_else(failed)?.map_err(|_| failed())?;
                match event {
                    Message::Binary(bytes) => {
                        #[cfg(test)]
                        trace_frame(&bytes);
                        let (event, finished) = decode(&bytes)?;
                        #[cfg(test)]
                        trace("event", json!({"code":event["code"].as_i64(),"result_type":field_type(event.get("result")),"result_text_is_string":event["result"]["text"].is_string(),"finished":finished,"sent_end":sent_end}));
                        if event.get("code").is_some_and(|value| value != 0) { return Err(failed()); }
                        if let Some(value) = event["result"]["text"].as_str() { text = value.to_owned(); }
                        if finished {
                            if !sent_end || !event["result"]["text"].is_string() { return Err(failed()); }
                            let _ = socket.close(None).await; return Ok(text);
                        }
                    }
                    Message::Ping(value) => { socket.send(Message::Pong(value)).await.map_err(|_| failed())?; }
                    Message::Close(_frame) => {
                        #[cfg(test)]
                        trace("close", json!({"code":_frame.as_ref().map(|frame|u16::from(frame.code)),"sent_end":sent_end,"has_text":!text.is_empty()}));
                        return Err(failed());
                    }
                    Message::Pong(_) => {}
                    _ => return Err(failed()),
                }
            }
        }
    }
}

fn frame(kind: u8, encoding: u8, payload: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0x11, kind, encoding, 0];
    bytes.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    bytes.extend_from_slice(payload);
    bytes
}

fn decode(bytes: &[u8]) -> Result<(Value, bool), ProxyError> {
    if bytes.len() < 8
        || bytes[0] >> 4 != 1
        || bytes[1] >> 4 != 9
        || bytes[1] & 15 > 3
        || bytes[2] >> 4 != 1
    {
        return Err(failed());
    }
    let flags = bytes[1] & 15;
    let header = usize::from(bytes[0] & 15) * 4;
    let offset = header + usize::from(flags & 1 != 0) * 4;
    if header < 4 || offset + 4 > bytes.len() {
        return Err(failed());
    }
    let size =
        u32::from_be_bytes(bytes[offset..offset + 4].try_into().map_err(|_| failed())?) as usize;
    if size > 2 * 1024 * 1024 || size != bytes.len() - offset - 4 {
        return Err(failed());
    }
    let payload = &bytes[offset + 4..];
    let value = match bytes[2] & 15 {
        0 => serde_json::from_slice(payload).map_err(|_| failed())?,
        1 => {
            let mut decoded = Vec::new();
            flate2::read::GzDecoder::new(payload)
                .take(2 * 1024 * 1024 + 1)
                .read_to_end(&mut decoded)
                .map_err(|_| failed())?;
            if decoded.len() > 2 * 1024 * 1024 {
                return Err(failed());
            }
            serde_json::from_slice(&decoded).map_err(|_| failed())?
        }
        _ => return Err(failed()),
    };
    Ok((value, flags & 2 != 0))
}

fn invalid_audio() -> ProxyError {
    ProxyError::new(
        StatusCode::BAD_REQUEST,
        "TRAE speech requires 16 kHz mono PCM16 WAV audio",
    )
}

fn login_required() -> ProxyError {
    ProxyError::new(StatusCode::UNAUTHORIZED, "TRAE CN speech login required")
}

fn failed() -> ProxyError {
    ProxyError::new(StatusCode::BAD_GATEWAY, "TRAE speech recognition failed")
}

fn timed_out() -> ProxyError {
    ProxyError::new(
        StatusCode::GATEWAY_TIMEOUT,
        "TRAE speech recognition timed out",
    )
}

#[cfg(test)]
fn trace(stage: &str, facts: Value) {
    if std::env::var("ECHOBIRD_ACCOUNT_VOICE_TEST_TRACE").as_deref() == Ok("1") {
        eprintln!("TRAE_VOICE_TRACE {}", json!({"stage":stage,"facts":facts}));
    }
}

#[cfg(test)]
fn field_type(value: Option<&Value>) -> &'static str {
    match value {
        None => "missing",
        Some(Value::Null) => "null",
        Some(Value::Bool(_)) => "bool",
        Some(Value::Number(_)) => "number",
        Some(Value::String(_)) => "string",
        Some(Value::Array(_)) => "array",
        Some(Value::Object(_)) => "object",
    }
}

#[cfg(test)]
fn trace_ws_error(error: &Error) {
    let kind = match error {
        Error::Http(_) => "http",
        Error::Io(_) => "io",
        Error::Tls(_) => "tls",
        Error::Protocol(_) => "protocol",
        Error::Url(_) => "url",
        _ => "other",
    };
    let status = match error {
        Error::Http(response) => Some(response.status().as_u16()),
        _ => None,
    };
    trace(
        "ws_handshake",
        json!({"success":false,"error_kind":kind,"status":status}),
    );
}

#[cfg(test)]
fn trace_frame(bytes: &[u8]) {
    let header = bytes.first().map(|value| usize::from(value & 15) * 4);
    let kind = bytes.get(1).map(|value| value >> 4);
    let error_code = header
        .filter(|size| *size >= 4)
        .filter(|_| kind == Some(15))
        .and_then(|offset| bytes.get(offset..offset + 4))
        .and_then(|value| <[u8; 4]>::try_from(value).ok())
        .map(u32::from_be_bytes);
    trace(
        "frame",
        json!({"len":bytes.len(),"version":bytes.first().map(|value|value>>4),"header_words":bytes.first().map(|value|value&15),"message_type":kind,"flags":bytes.get(1).map(|value|value&15),"serialization":bytes.get(2).map(|value|value>>4),"compression":bytes.get(2).map(|value|value&15),"upstream_error_code":error_code}),
    );
}

#[cfg(test)]
mod tests;
