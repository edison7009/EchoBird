//! Grok Build's billing read for the selected OAuth identity.

use serde::{Deserialize, Serialize};

pub(super) const URL: &str = "https://grok.com/grok_api_v2.GrokBuildBilling/GetGrokCreditsConfig";
const MAX_RESPONSE_BYTES: usize = 64 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Quota {
    pub remaining_percent: f64,
    pub reset_at: Option<i64>,
}

pub(super) async fn read(client: &reqwest::Client, url: &str, key: &str) -> Option<Quota> {
    let mut response = client
        .post(url)
        .bearer_auth(key)
        .header("Origin", "https://grok.com")
        .header("Content-Type", "application/grpc-web+proto")
        .header("Accept", "application/grpc-web+proto")
        .header("x-grpc-web", "1")
        .header("User-Agent", "EchoBird")
        .body(vec![0; 5])
        .send()
        .await
        .ok()?;
    if !response.status().is_success()
        || response
            .headers()
            .get("grpc-status")
            .is_some_and(|status| status != "0")
    {
        return None;
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.ok()? {
        if chunk.len() > MAX_RESPONSE_BYTES.saturating_sub(bytes.len()) {
            return None;
        }
        bytes.extend(&chunk);
    }
    let content = if response
        .headers()
        .get("content-type")
        .and_then(|header| header.to_str().ok())
        .is_some_and(|value| value.starts_with("application/proto"))
    {
        &bytes
    } else {
        grpc_body(&bytes)?
    };
    parse_usage(content, chrono::Utc::now().timestamp())
}

fn grpc_body(mut bytes: &[u8]) -> Option<&[u8]> {
    let mut message = None;
    while !bytes.is_empty() {
        let (header, frames) = bytes.split_at_checked(5)?;
        let length = u32::from_be_bytes(header[1..].try_into().ok()?) as usize;
        let (frame, rest) = frames.split_at_checked(length)?;
        match header[0] {
            0 if message.is_none() => message = Some(frame),
            0x80 if rest.is_empty() => {
                let trailer = std::str::from_utf8(frame).ok()?;
                if !trailer.lines().any(|line| {
                    line.split_once(':').is_some_and(|(key, value)| {
                        key.eq_ignore_ascii_case("grpc-status") && value.trim() == "0"
                    })
                }) {
                    return None;
                }
            }
            _ => return None,
        }
        bytes = rest;
    }
    message
}

enum Field<'a> {
    Varint(u64),
    Message(&'a [u8]),
    Float(f32),
    Other,
}

fn varint(bytes: &mut &[u8]) -> Option<u64> {
    let mut result = 0;
    for shift in (0..64).step_by(7) {
        let (byte, rest) = bytes.split_first()?;
        *bytes = rest;
        if shift == 63 && *byte > 1 {
            return None;
        }
        result |= u64::from(byte & 0x7f) << shift;
        if *byte < 0x80 {
            return Some(result);
        }
    }
    None
}

fn read_field<'a>(bytes: &mut &'a [u8]) -> Option<(u64, Field<'a>)> {
    let tag = varint(bytes)?;
    let number = tag >> 3;
    if number == 0 {
        return None;
    }
    let field = match tag & 7 {
        0 => Field::Varint(varint(bytes)?),
        1 => {
            *bytes = bytes.get(8..)?;
            Field::Other
        }
        2 => {
            let length = usize::try_from(varint(bytes)?).ok()?;
            let (message, rest) = bytes.split_at_checked(length)?;
            *bytes = rest;
            Field::Message(message)
        }
        5 => {
            let (bits, rest) = bytes.split_at_checked(4)?;
            *bytes = rest;
            Field::Float(f32::from_le_bytes(bits.try_into().ok()?))
        }
        _ => return None,
    };
    Some((number, field))
}

fn field(mut bytes: &[u8], wanted: u64) -> Option<Field<'_>> {
    while !bytes.is_empty() {
        let (number, value) = read_field(&mut bytes)?;
        if number == wanted {
            return Some(value);
        }
    }
    None
}

fn parse_usage(bytes: &[u8], now: i64) -> Option<Quota> {
    let Field::Message(mut usage) = field(bytes, 1)? else {
        return None;
    };
    let mut used = None;
    let mut reset_at = None;
    let mut has_billing_period = false;
    while !usage.is_empty() {
        match read_field(&mut usage)? {
            (1, Field::Float(percent)) if (0.0..=100.0).contains(&percent) => {
                used = Some(f64::from(percent));
            }
            (1, _) => return None,
            (5, Field::Message(timestamp)) => {
                if let Some(Field::Varint(seconds)) = field(timestamp, 1) {
                    reset_at = i64::try_from(seconds).ok().filter(|seconds| {
                        *seconds > now && *seconds <= now.saturating_add(370 * 86400)
                    });
                }
            }
            (6, Field::Message(period)) => {
                has_billing_period |= matches!(field(period, 1), Some(Field::Varint(1..)));
            }
            (8, Field::Message(period)) => {
                has_billing_period |= matches!(field(period, 1), Some(Field::Varint(1 | 2)));
            }
            _ => {}
        }
    }
    // Proto3 omits default floats, so only a recognizable dated window implies 0% used.
    let used = used.or_else(|| (has_billing_period && reset_at.is_some()).then_some(0.0))?;
    Some(Quota {
        remaining_percent: 100.0 - used,
        reset_at,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grpc_frame(flags: u8, data: &[u8]) -> Vec<u8> {
        let mut frame = vec![flags];
        frame.extend((data.len() as u32).to_be_bytes());
        frame.extend(data);
        frame
    }

    #[test]
    fn reads_only_the_billing_windows_percent_and_time() {
        let usage = [
            0x0a, 0x12, 0x12, 0x05, 0x0d, 0, 0, 0xc6, 0x42, 0x0d, 0, 0, 0x16, 0x42, 0x2a, 0x04,
            0x08, 0x80, 0xc2, 0x07,
        ];
        let quota = parse_usage(&usage, 120_000).unwrap();
        assert_eq!(quota.remaining_percent, 62.5);
        assert_eq!(quota.reset_at, Some(123_136));
        let without_usage_field = [0x0a, 0x07, 0x12, 0x05, 0x0d, 0, 0, 0xc6, 0x42];
        assert!(parse_usage(&without_usage_field, 120_000).is_none());
    }

    #[test]
    fn only_zero_with_a_billing_window_and_successful_framing_is_reported() {
        let usage = [
            0x0a, 0x0a, 0x2a, 0x04, 0x08, 0x80, 0xc2, 0x07, 0x32, 0x02, 0x08, 0x03,
        ];
        let mut frame = grpc_frame(0, &usage);
        assert_eq!(
            parse_usage(grpc_body(&frame).unwrap(), 120_000)
                .unwrap()
                .remaining_percent,
            100.0,
        );
        assert!(parse_usage(&usage, 123_200).is_none());
        frame.extend(grpc_frame(0x80, b"grpc-status: 16\r\n"));
        assert!(grpc_body(&frame).is_none());
        assert!(grpc_body(&frame[..4]).is_none());
    }
}
