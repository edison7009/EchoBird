use axum::{
    body::Bytes,
    http::{header, HeaderMap, HeaderValue, StatusCode, Version},
};
use bytes::{Buf, BytesMut};
use http_body_util::BodyExt;
use serde_json::Value;
use std::{io, time::Duration};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

use super::ProxyError;

mod catalog;
pub(crate) use catalog::first_model as selected_model;

const ENDPOINT: &str = "https://aicode.googleapis.com";
const PATH: &str = "/google.gca.aicode.v1alpha.PredictionService/BidiGenerateContent";
const PCM_MIME: &[u8] = b"audio/pcm;rate=16000";
const MAX_MESSAGE: usize = 4 * 1024 * 1024;
const FINISH_WAIT: Duration = Duration::from_secs(10);

pub(crate) async fn transcribe(
    client: &reqwest::Client,
    credentials: &Value,
    model: &str,
    audio: &Bytes,
    _filename: &str,
    mime: &str,
) -> Result<String, ProxyError> {
    if model.trim().is_empty() || model.len() > 1024 || model.chars().any(char::is_control) {
        return Err(ProxyError::new(
            StatusCode::BAD_REQUEST,
            "Antigravity speech requires an available model",
        ));
    }
    let token = credentials["accessToken"]
        .as_str()
        .filter(|token| !token.trim().is_empty())
        .ok_or_else(login_required)?;
    let authorization =
        HeaderValue::from_str(&format!("Bearer {token}")).map_err(|_| login_required())?;
    let pcm = validated_pcm(audio, mime)?;
    let result = tokio::time::timeout(
        Duration::from_secs(60),
        recognize(client, credentials, model, pcm, authorization),
    )
    .await
    .unwrap_or_else(|_| Err(timed_out()));
    #[cfg(test)]
    if let Err(error) = &result {
        trace(format_args!(
            "stage=return reason=error status={}",
            error.status.as_u16()
        ));
    }
    result
}

pub(crate) fn validate_audio(audio: &Bytes, mime: &str) -> Result<(), ProxyError> {
    validated_pcm(audio, mime).map(|_| ())
}

fn validated_pcm<'a>(audio: &'a [u8], mime: &str) -> Result<&'a [u8], ProxyError> {
    if !mime.eq_ignore_ascii_case("audio/wav") && !mime.eq_ignore_ascii_case("audio/x-wav") {
        return Err(invalid_audio());
    }
    super::audio_format::pcm(audio).map_err(|_| invalid_audio())
}

async fn recognize(
    client: &reqwest::Client,
    credentials: &Value,
    model: &str,
    pcm: &[u8],
    authorization: HeaderValue,
) -> Result<String, ProxyError> {
    let (tx, rx) = mpsc::channel::<Result<Bytes, io::Error>>(2);
    let mut setup = field(1, model.as_bytes());
    // Noncontinuous native mode: disable automatic activity detection.
    setup.extend(field(5, &field(6, &[0x08, 0x01])));
    setup.extend(field(8, &[]));
    tx.send(Ok(framed(field(1, &setup))))
        .await
        .map_err(|_| failed())?;
    let request = client
        .post(format!("{}{PATH}", endpoint(credentials)))
        .version(Version::HTTP_2)
        .header(header::AUTHORIZATION, authorization)
        .header(header::CONTENT_TYPE, "application/grpc")
        .header(header::ACCEPT_ENCODING, "identity")
        .header(header::TE, "trailers")
        .timeout(Duration::from_secs(60))
        .body(reqwest::Body::wrap_stream(ReceiverStream::new(rx)));
    let response = tokio::time::timeout(Duration::from_secs(10), request.send())
        .await
        .map_err(|_| timed_out())?
        .map_err(request_error)?;
    #[cfg(test)]
    trace(format_args!(
        "stage=http status={} version={:?}",
        response.status().as_u16(),
        response.version()
    ));
    if !response.status().is_success() {
        let mut error = ProxyError::new(response.status(), "Antigravity speech request failed");
        error.retry_after = response.headers().get(header::RETRY_AFTER).cloned();
        return Err(error);
    }
    let status_seen = grpc_status(response.headers())?;
    if response.version() != Version::HTTP_2
        || !response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| {
                matches!(
                    value.split(';').next(),
                    Some("application/grpc" | "application/grpc+proto")
                )
            })
    {
        return Err(failed());
    }
    // This conversion retains HTTP/2 trailers; bytes_stream() would discard them.
    let response: axum::http::Response<reqwest::Body> = response.into();
    let mut reader = GrpcReader {
        body: response.into_body(),
        pending: BytesMut::new(),
        status_seen,
    };
    let ready = reader.next().await?.ok_or_else(failed)?;
    let ready_fields = proto_fields(&ready)?;
    #[cfg(test)]
    trace(format_args!(
        "stage=response field_ids={:?}",
        ready_fields
            .iter()
            .map(|(number, _)| *number)
            .collect::<Vec<_>>()
    ));
    let Some((1, ready)) = ready_fields
        .into_iter()
        .rfind(|(number, _)| (1..=6).contains(number))
    else {
        return Err(failed());
    };
    proto_fields(ready)?;
    #[cfg(test)]
    trace(format_args!("stage=setup_complete"));

    // No detached task: cancellation drops both this sender and the response body.
    let upload = async move {
        tx.send(Ok(framed(field(3, &field(5, &[])))))
            .await
            .map_err(|_| failed())?;
        for chunk in pcm.chunks(32_000) {
            let mut blob = field(1, PCM_MIME);
            blob.extend(field(2, chunk));
            tx.send(Ok(framed(field(3, &field(1, &blob)))))
                .await
                .map_err(|_| failed())?;
        }
        tx.send(Ok(framed(field(3, &field(6, &[])))))
            .await
            .map_err(|_| failed())?;
        drop(tx);
        #[cfg(test)]
        trace(format_args!(
            "stage=input_end bytes={} end_sent=1",
            pcm.len()
        ));
        Ok::<_, ProxyError>(())
    };
    tokio::pin!(upload);
    let finish = tokio::time::sleep(FINISH_WAIT);
    tokio::pin!(finish);
    let mut sent = false;
    let mut transcript = Transcript::default();
    loop {
        tokio::select! {
            biased;
            result = &mut upload, if !sent => {
                result?;
                sent = true;
                finish.as_mut().reset(tokio::time::Instant::now() + FINISH_WAIT);
            }
            message = reader.next() => {
                match message? {
                    Some(message) => transcript.apply(&message)?,
                    None if sent => {
                        #[cfg(test)]
                        trace(format_args!("stage=return reason=EOF"));
                        return Ok(transcript.draft());
                    }
                    None => return Err(failed()),
                }
            }
            // Drain ready responses/errors before taking the native fallback.
            _ = &mut finish, if sent => {
                if !reader.pending.is_empty() {
                    return Err(timed_out());
                }
                #[cfg(test)]
                trace(format_args!("stage=return reason=fallback"));
                return Ok(transcript.draft());
            }
        }
    }
}

struct GrpcReader {
    body: reqwest::Body,
    pending: BytesMut,
    status_seen: bool,
}

impl GrpcReader {
    async fn next(&mut self) -> Result<Option<Bytes>, ProxyError> {
        loop {
            if self.pending.len() >= 5 {
                let length = u32::from_be_bytes(self.pending[1..5].try_into().unwrap()) as usize;
                if self.pending[0] != 0 || length > MAX_MESSAGE {
                    return Err(failed());
                }
                if self.pending.len() >= length + 5 {
                    self.pending.advance(5);
                    return Ok(Some(self.pending.split_to(length).freeze()));
                }
            }
            match self.body.frame().await {
                Some(Ok(frame)) => match frame.into_data() {
                    Ok(data) => {
                        self.pending.extend_from_slice(&data);
                    }
                    Err(frame) => {
                        if let Ok(trailers) = frame.into_trailers() {
                            self.status_seen |= grpc_status(&trailers)?;
                        }
                    }
                },
                Some(Err(_)) => return Err(failed()),
                None if self.status_seen && self.pending.is_empty() => return Ok(None),
                None => return Err(failed()),
            }
        }
    }
}

fn grpc_status(headers: &HeaderMap) -> Result<bool, ProxyError> {
    let mut present = false;
    for value in headers.get_all("grpc-status").iter() {
        present = true;
        let code = value
            .to_str()
            .ok()
            .filter(|value| !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()))
            .and_then(|value| value.parse::<u32>().ok())
            .filter(|code| *code <= 16)
            .ok_or_else(failed)?;
        #[cfg(test)]
        trace(format_args!("stage=grpc_status code={code}"));
        if code != 0 {
            let status = match code {
                4 => StatusCode::GATEWAY_TIMEOUT,
                7 => StatusCode::FORBIDDEN,
                8 => StatusCode::TOO_MANY_REQUESTS,
                14 => StatusCode::SERVICE_UNAVAILABLE,
                16 => StatusCode::UNAUTHORIZED,
                _ => StatusCode::BAD_GATEWAY,
            };
            let mut error = ProxyError::new(status, "Antigravity speech request failed");
            error.error_code = Some(format!("grpc_{code}"));
            error.retry_after = headers.get(header::RETRY_AFTER).cloned();
            return Err(error);
        }
    }
    Ok(present)
}

#[derive(Default)]
struct Transcript {
    finals: Vec<String>,
    partial: String,
}

impl Transcript {
    fn apply(&mut self, message: &[u8]) -> Result<(), ProxyError> {
        let fields = proto_fields(message)?;
        #[cfg(test)]
        trace(format_args!(
            "stage=response field_ids={:?}",
            fields.iter().map(|(number, _)| *number).collect::<Vec<_>>()
        ));
        let Some((2, content)) = fields
            .into_iter()
            .rfind(|(number, _)| (1..=6).contains(number))
        else {
            return Ok(());
        };
        let fields = proto_fields(content)?;
        for (number, final_segment) in [(11, false), (6, true)] {
            if let Some((_, value)) = fields.iter().rev().find(|(field, _)| *field == number) {
                if let Some((_, text)) = proto_fields(value)?
                    .into_iter()
                    .rev()
                    .find(|(field, _)| *field == 1)
                {
                    let text = std::str::from_utf8(text).map_err(|_| failed())?;
                    #[cfg(test)]
                    trace(format_args!(
                        "stage=transcription final={} chars={}",
                        final_segment,
                        text.chars().count()
                    ));
                    if !text.is_empty() {
                        if final_segment {
                            self.finals.push(text.to_owned());
                            self.partial.clear();
                        } else {
                            self.partial = text.to_owned();
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn draft(&self) -> String {
        let mut parts: Vec<&str> = self.finals.iter().map(String::as_str).collect();
        parts.push(&self.partial);
        collapse_cjk_spaces(parts.join(" ").trim())
    }
}

fn field(number: u32, data: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(data.len() + 10);
    for mut value in [u64::from(number) << 3 | 2, data.len() as u64] {
        while value >= 128 {
            bytes.push(value as u8 | 128);
            value >>= 7;
        }
        bytes.push(value as u8);
    }
    bytes.extend_from_slice(data);
    bytes
}

fn framed(message: Vec<u8>) -> Bytes {
    let mut bytes = Vec::with_capacity(message.len() + 5);
    bytes.push(0);
    bytes.extend_from_slice(&(message.len() as u32).to_be_bytes());
    bytes.extend(message);
    bytes.into()
}

fn proto_fields(mut bytes: &[u8]) -> Result<Vec<(u32, &[u8])>, ProxyError> {
    fn varint(bytes: &mut &[u8]) -> Result<u64, ProxyError> {
        let mut value = 0;
        for shift in (0..=63).step_by(7) {
            let (&byte, tail) = bytes.split_first().ok_or_else(failed)?;
            *bytes = tail;
            if shift == 63 && byte > 1 {
                return Err(failed());
            }
            value |= u64::from(byte & 127) << shift;
            if byte < 128 {
                return Ok(value);
            }
        }
        Err(failed())
    }
    let mut fields = Vec::new();
    while !bytes.is_empty() {
        let tag = varint(&mut bytes)?;
        let number = u32::try_from(tag >> 3).map_err(|_| failed())?;
        if number == 0 || number > 0x1fff_ffff {
            return Err(failed());
        }
        let length = match tag & 7 {
            0 => {
                varint(&mut bytes)?;
                continue;
            }
            1 => 8,
            2 => usize::try_from(varint(&mut bytes)?).map_err(|_| failed())?,
            5 => 4,
            _ => return Err(failed()),
        };
        let value = bytes.get(..length).ok_or_else(failed)?;
        if tag & 7 == 2 {
            fields.push((number, value));
        }
        bytes = &bytes[length..];
    }
    Ok(fields)
}

fn collapse_cjk_spaces(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    let mut previous = None;
    while let Some(character) = chars.next() {
        if character == ' ' {
            let mut count = 1;
            while chars.peek() == Some(&' ') {
                chars.next();
                count += 1;
            }
            if previous.is_some_and(spaceless_cjk)
                && chars.peek().copied().is_some_and(spaceless_cjk)
            {
                continue;
            }
            output.extend(std::iter::repeat_n(' ', count));
        } else {
            output.push(character);
            previous = Some(character);
        }
    }
    output
}

fn spaceless_cjk(character: char) -> bool {
    let rune = u32::from(character);
    CJK_RANGES.iter().any(|&(first, last, stride)| {
        (first..=last).contains(&rune) && (rune - first) % stride == 0
    })
}

fn endpoint(credentials: &Value) -> &str {
    #[cfg(test)]
    if let Some(endpoint) = credentials["_voice_test_endpoint"].as_str() {
        return endpoint.trim_end_matches('/');
    }
    let _ = credentials;
    ENDPOINT
}

fn invalid_audio() -> ProxyError {
    ProxyError::new(
        StatusCode::BAD_REQUEST,
        "Antigravity speech requires 16 kHz mono PCM16 WAV audio",
    )
}

fn login_required() -> ProxyError {
    ProxyError::new(
        StatusCode::UNAUTHORIZED,
        "Antigravity speech login required",
    )
}

fn timed_out() -> ProxyError {
    ProxyError::new(
        StatusCode::GATEWAY_TIMEOUT,
        "Antigravity speech request timed out",
    )
}

fn failed() -> ProxyError {
    ProxyError::new(
        StatusCode::BAD_GATEWAY,
        "Antigravity speech response is invalid",
    )
}

fn request_error(error: reqwest::Error) -> ProxyError {
    if error.is_timeout() {
        timed_out()
    } else {
        failed()
    }
}

#[cfg(test)]
fn trace(arguments: std::fmt::Arguments<'_>) {
    if std::env::var("ECHOBIRD_ACCOUNT_VOICE_TEST_TRACE").as_deref() == Ok("1") {
        eprintln!("ANTIGRAVITY_VOICE_TRACE {arguments}");
    }
}

// Exact App 2.26.0 Unicode tables and punctuation additions.
// Hangul is deliberately absent from the native four-table predicate.
const CJK_RANGES: &[(u32, u32, u32)] = &[
    (0x2e80, 0x2e99, 1),
    (0x2e9b, 0x2ef3, 1),
    (0x2f00, 0x2fd5, 1),
    (0x3005, 0x3007, 2),
    (0x3021, 0x3029, 1),
    (0x3038, 0x303b, 1),
    (0x3400, 0x4dbf, 1),
    (0x4e00, 0x9fff, 1),
    (0xf900, 0xfa6d, 1),
    (0xfa70, 0xfad9, 1),
    (0x16fe2, 0x16fe3, 1),
    (0x16ff0, 0x16ff6, 1),
    (0x20000, 0x2a6df, 1),
    (0x2a700, 0x2b81d, 1),
    (0x2b820, 0x2cead, 1),
    (0x2ceb0, 0x2ebe0, 1),
    (0x2ebf0, 0x2ee5d, 1),
    (0x2f800, 0x2fa1d, 1),
    (0x30000, 0x3134a, 1),
    (0x31350, 0x33479, 1),
    (0x3041, 0x3096, 1),
    (0x309d, 0x309f, 1),
    (0x1b001, 0x1b11f, 1),
    (0x1b132, 0x1b150, 30),
    (0x1b151, 0x1b152, 1),
    (0x1f200, 0x1f200, 1),
    (0x30a1, 0x30fa, 1),
    (0x30fd, 0x30ff, 1),
    (0x31f0, 0x31ff, 1),
    (0x32d0, 0x32fe, 1),
    (0x3300, 0x3357, 1),
    (0xff66, 0xff6f, 1),
    (0xff71, 0xff9d, 1),
    (0x1aff0, 0x1aff3, 1),
    (0x1aff5, 0x1affb, 1),
    (0x1affd, 0x1affe, 1),
    (0x1b000, 0x1b120, 288),
    (0x1b121, 0x1b122, 1),
    (0x1b155, 0x1b164, 15),
    (0x1b165, 0x1b167, 1),
    (0x02ea, 0x02eb, 1),
    (0x3105, 0x312f, 1),
    (0x31a0, 0x31bf, 1),
    (0x3001, 0x303f, 1),
    (0xff01, 0xff60, 1),
    (0xff61, 0xff9f, 1),
];

#[cfg(test)]
mod tests;
