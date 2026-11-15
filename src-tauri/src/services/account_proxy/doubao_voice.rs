//! Domestic VoiceGenie candidate. Account eligibility/ASR still require live validation.
use axum::{
    body::Bytes,
    http::{header, HeaderValue, StatusCode},
};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::time::Duration;
use tokio::time::Instant;
use tokio_tungstenite::tungstenite::{
    client::IntoClientRequest, protocol::WebSocketConfig, Error, Message as WsMessage,
};

use super::ProxyError;
use crate::services::{
    devin_protocol::{self as wire, Field, Message},
    doubao_accounts::speech::SCOPE,
};

const NORMAL_APPKEY: &str = "GOqQpfo1fO7slHv8";
const FEISHU_APPKEY: &str = "F5tdjWzsqEUUQsLJ";
const MAX_MESSAGE: usize = 1024 * 1024;
const CHUNK_BYTES: usize = 640;

pub(crate) async fn transcribe(
    _client: &reqwest::Client,
    credentials: &Value,
    audio: &Bytes,
    _filename: &str,
    mime: &str,
) -> Result<String, ProxyError> {
    let pcm = validated_pcm(audio, mime)?;
    tokio::time::timeout(
        super::voice::media_timeout(pcm.len()),
        recognize(credentials, pcm),
    )
    .await
    .map_err(|_| timed_out())?
}
pub(crate) fn validate_audio(audio: &Bytes, mime: &str) -> Result<(), ProxyError> {
    validated_pcm(audio, mime).map(|_| ())
}
fn validated_pcm<'a>(audio: &'a [u8], mime: &str) -> Result<&'a [u8], ProxyError> {
    if !matches!(mime, "audio/wav" | "audio/x-wav") {
        return Err(invalid_audio());
    }
    super::claude_voice::pcm(audio).map_err(|_| invalid_audio())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Stage {
    Task,
    Session,
    Audio,
    End,
    Finish,
}

async fn recognize(credentials: &Value, pcm: &[u8]) -> Result<String, ProxyError> {
    if credentials["edition"] != "doubaowork" || credentials["cookieScope"] != SCOPE {
        return Err(login_required());
    }
    let uid = credentials["userId"]
        .as_str()
        .filter(|uid| {
            !uid.trim().is_empty() && uid.len() <= 256 && !uid.chars().any(char::is_control)
        })
        .ok_or_else(login_required)?;
    let enterprise = credentials["enterpriseType"]
        .as_str()
        .ok_or_else(login_required)?;
    let appkey = if enterprise == "102" {
        FEISHU_APPKEY
    } else {
        NORMAL_APPKEY
    };
    let mut url = url::Url::parse(SCOPE).map_err(|_| failed())?;
    for (key, value) in [
        ("api_app_key", appkey),
        ("namespace", "VoiceGenie"),
        ("version_code", "20800"),
        ("language", "zh"),
        ("device_platform", "web"),
        ("pkg_type", "release_version"),
        ("pc_version", "3.9.0"),
        ("region", "CN"),
        ("sys_region", "CN"),
        ("samantha_web", "1"),
        ("use-olympus-account", "1"),
    ] {
        url.query_pairs_mut().append_pair(key, value);
    }
    #[cfg(test)]
    if let Some(endpoint) = credentials["_voice_test_endpoint"].as_str() {
        let target = url::Url::parse(endpoint).map_err(|_| failed())?;
        url.set_scheme(target.scheme()).map_err(|_| failed())?;
        url.set_host(target.host_str()).map_err(|_| failed())?;
        url.set_port(target.port()).map_err(|_| failed())?;
    }
    let mut request = url.as_str().into_client_request().map_err(|_| failed())?;
    let cookies = credentials["cookieHeader"]
        .as_str()
        .ok_or_else(login_required)?;
    if !cookies.is_empty() {
        request.headers_mut().insert(
            header::COOKIE,
            HeaderValue::from_str(cookies).map_err(|_| login_required())?,
        );
    }
    // No guessed Bearer or native Origin. This matches the no-Origin public 101
    // probe; a real browser's Origin and current app mode remain unobserved.
    let config = WebSocketConfig::default()
        .max_message_size(Some(MAX_MESSAGE))
        .max_frame_size(Some(MAX_MESSAGE));
    let (socket, response) = tokio::time::timeout(
        Duration::from_secs(15),
        tokio_tungstenite::connect_async_with_config(request, Some(config), false),
    )
    .await
    .map_err(|_| timed_out())?
    .map_err(socket_error)?;
    #[cfg(test)]
    trace(format_args!(
        "stage=handshake status={}",
        response.status().as_u16()
    ));
    #[cfg(not(test))]
    let _ = response;
    let mut socket = socket;
    socket
        .send(WsMessage::Binary(
            frame(appkey, "", "StartTask", None, None, None).into(),
        ))
        .await
        .map_err(socket_error)?;
    let mut stage = Stage::Task;
    let mut task = String::new();
    let mut sent = 0;
    let mut sequence = 1;
    let mut media_start = Instant::now();
    let mut next_audio = media_start;
    let mut heartbeat = tokio::time::interval_at(
        Instant::now() + Duration::from_secs(8),
        Duration::from_secs(8),
    );
    let mut pong_deadline = None;
    let mut transcript = Transcript::default();
    loop {
        tokio::select! {
            biased;
            message=socket.next()=>{
                let message=message.ok_or_else(failed)?.map_err(socket_error)?;
                let bytes=match message{WsMessage::Binary(bytes)=>bytes,WsMessage::Ping(bytes)=>{socket.send(WsMessage::Pong(bytes)).await.map_err(socket_error)?;continue;},WsMessage::Pong(_)=>continue,_=>return Err(failed())};
                let event=decode(&bytes)?;
                #[cfg(test)] trace(format_args!("stage=recv event={} status_code={}",safe_event(event.event),event.status_code));
                if event.event=="Pong"{pong_deadline=None;continue;}
                if matches!(event.event,"TaskFailed"|"SessionFailed"|"SessionCanceled"|"TaskCanceled"|"Error") || (!known_event(event.event)&&event.status_code!=0){return Err(business_error(event.status_code));}
                if !task.is_empty()&&!event.task.is_empty()&&event.task!=task{return Err(failed());}
                match event.event{
                    "TaskStarted" if stage==Stage::Task=>{
                        if event.task.is_empty()||event.task.len()>512{return Err(failed());}
                        task=event.task.to_string();
                        let payload=json!({"business":1,"enable_audio_input":true,"query_mode":2,"interrupt_type":0,"request_type":3,
                            "chat":{"uid":uid},"asr":{"audio_info":{"channel":1,"format":"pcm","sample_rate":16000},"lang":"zh","enable_vad":true,"enable_punctuation":true,"enable_itn":true,"enable_disfluency":true},"extra":{"uid":uid}}).to_string();
                        socket.send(WsMessage::Binary(frame(appkey,&task,"StartSession",Some(&payload),None,None).into())).await.map_err(socket_error)?;stage=Stage::Session;
                    }
                    "SessionStarted" if stage==Stage::Session=>{stage=Stage::Audio;media_start=Instant::now();next_audio=media_start+Duration::from_micros(pcm.len().min(CHUNK_BYTES) as u64*1_000_000/32_000);}
                    "ASRResponse" if matches!(stage,Stage::Audio|Stage::End|Stage::Finish)=>transcript.apply(event.payload.ok_or_else(failed)?)?,
                    "ASREnded" if stage==Stage::End=>{socket.send(WsMessage::Binary(frame(appkey,&task,"FinishSession",None,None,None).into())).await.map_err(socket_error)?;stage=Stage::Finish;}
                    "SessionFinished" if stage==Stage::Finish=>{let text=transcript.confirmed.trim();if text.is_empty(){return Err(failed());}let text=text.to_string();let _=socket.close(None).await;return Ok(text);}
                    "TaskStarted"|"SessionStarted"|"ASREnded"|"SessionFinished"=>return Err(failed()),
                    _=>{}
                }
            }
            _=tokio::time::sleep_until(next_audio),if stage==Stage::Audio=>{
                let end=(sent+CHUNK_BYTES).min(pcm.len());
                socket.send(WsMessage::Binary(frame(appkey,&task,"TaskRequest",None,Some(&pcm[sent..end]),Some(sequence)).into())).await.map_err(socket_error)?;
                sent=end;sequence+=1;
                if sent==pcm.len(){socket.send(WsMessage::Binary(frame(appkey,&task,"EndASR",None,None,None).into())).await.map_err(socket_error)?;stage=Stage::End;}
                else{next_audio=media_start+Duration::from_micros((sent+CHUNK_BYTES).min(pcm.len()) as u64*1_000_000/32_000);}
            }
            _=heartbeat.tick()=>{socket.send(WsMessage::Binary(frame(appkey,&task,"Ping",None,None,None).into())).await.map_err(socket_error)?;pong_deadline.get_or_insert(Instant::now()+Duration::from_secs(12));}
            _=tokio::time::sleep_until(pong_deadline.unwrap_or_else(Instant::now)),if pong_deadline.is_some()=>return Err(timed_out()),
        }
    }
}

fn frame(
    appkey: &str,
    task: &str,
    event: &str,
    payload: Option<&str>,
    data: Option<&[u8]>,
    sequence: Option<u64>,
) -> Vec<u8> {
    let mut value = Message::default()
        .bytes(2, appkey)
        .bytes(3, "VoiceGenie")
        .bytes(5, event)
        .bytes(6, payload.unwrap_or("{}"));
    if let Some(data) = data {
        value = value.bytes(7, data);
    }
    if !task.is_empty() {
        value = value.bytes(8, task);
    }
    if let Some(sequence) = sequence {
        value = value.num(9, sequence);
    }
    value.0
}
struct Event<'a> {
    task: &'a str,
    event: &'a str,
    status_code: i64,
    payload: Option<&'a str>,
}
fn decode(bytes: &[u8]) -> Result<Event<'_>, ProxyError> {
    let mut result = Event {
        task: "",
        event: "",
        status_code: 0,
        payload: None,
    };
    for field in wire::fields(bytes).map_err(|_| failed())? {
        match field {
            Field::Bytes(1, bytes) => {
                result.task = std::str::from_utf8(bytes).map_err(|_| failed())?
            }
            Field::Bytes(4, bytes) => {
                result.event = std::str::from_utf8(bytes).map_err(|_| failed())?
            }
            Field::Number(5, value) => result.status_code = value as i32 as i64,
            Field::Bytes(7, bytes) => {
                result.payload = Some(std::str::from_utf8(bytes).map_err(|_| failed())?)
            }
            _ => {}
        }
    }
    Ok(result)
}
#[derive(Default)]
struct Transcript {
    confirmed: String,
    pending: String,
}
impl Transcript {
    fn apply(&mut self, payload: &str) -> Result<(), ProxyError> {
        let value: Value = serde_json::from_str(payload).map_err(|_| failed())?;
        let Some(result) = value["results"]
            .as_array()
            .and_then(|results| results.first())
        else {
            return Ok(());
        };
        let Some(text) = result["text"].as_str() else {
            return Ok(());
        };
        let final_result = result["is_interim"]
            .as_bool()
            .map(|interim| !interim)
            .or_else(|| result["is_final"].as_bool())
            .or_else(|| result["isFinal"].as_bool())
            .or_else(|| value["is_final"].as_bool())
            .or_else(|| value["isFinal"].as_bool())
            .unwrap_or(false);
        if self.confirmed.len().saturating_add(text.len()) > MAX_MESSAGE {
            return Err(failed());
        }
        if final_result {
            self.confirmed.push_str(text);
            self.pending.clear();
        } else {
            self.pending = text.to_string();
        }
        Ok(())
    }
}
fn known_event(event: &str) -> bool {
    matches!(
        event,
        "TaskStarted"
            | "SessionStarted"
            | "SessionFinished"
            | "SessionCanceled"
            | "SessionFailed"
            | "ASRInfo"
            | "ASRResponse"
            | "ASREnded"
            | "TaskFinished"
            | "TaskCanceled"
            | "TaskFailed"
            | "Pong"
    )
}
#[cfg(test)]
fn safe_event(event: &str) -> &str {
    if known_event(event) {
        event
    } else {
        "Other"
    }
}
fn business_error(code: i64) -> ProxyError {
    let mut error = failed();
    if code != 0 {
        error.error_code = Some(code.to_string());
    }
    error
}
fn socket_error(error: Error) -> ProxyError {
    if let Error::Http(response) = error {
        let mut error = ProxyError::new(response.status(), "Doubao speech handshake failed");
        error.retry_after = response.headers().get(header::RETRY_AFTER).cloned();
        error
    } else {
        failed()
    }
}
fn invalid_audio() -> ProxyError {
    ProxyError::new(
        StatusCode::BAD_REQUEST,
        "Doubao speech requires 16 kHz mono PCM16 WAV audio",
    )
}
fn login_required() -> ProxyError {
    ProxyError::new(
        StatusCode::UNAUTHORIZED,
        "Doubao speech requires a domestic saved account",
    )
}
fn failed() -> ProxyError {
    ProxyError::new(StatusCode::BAD_GATEWAY, "Doubao speech recognition failed")
}
fn timed_out() -> ProxyError {
    ProxyError::new(
        StatusCode::GATEWAY_TIMEOUT,
        "Doubao speech recognition timed out",
    )
}
#[cfg(test)]
fn trace(args: std::fmt::Arguments<'_>) {
    if std::env::var("ECHOBIRD_ACCOUNT_VOICE_TEST_TRACE").as_deref() == Ok("1") {
        eprintln!("doubao_voice {args}");
    }
}
#[cfg(test)]
mod tests;
