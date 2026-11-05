use super::super::ProxyError;
use axum::http::{HeaderMap, HeaderValue};
use base64::{engine::general_purpose::STANDARD, Engine};
use cbc::cipher::{block_padding::Pkcs7, BlockEncryptMut, KeyIvInit};
use md5::{Digest, Md5};
use rsa::{pkcs8::DecodePublicKey, Pkcs1v15Encrypt, RsaPublicKey};
use serde_json::{json, Value};

const ALPHABET: &[u8; 64] = b"_doRTgHZBKcGVjlvpC,@aFSx#DPuNJme&i*MzLOEn)sUrthbf%Y^w.(kIQyXqWA!";
const PUBLIC_KEY: &str = "-----BEGIN PUBLIC KEY-----\nMIGfMA0GCSqGSIb3DQEBAQUAA4GNADCBiQKBgQDA8iMH5c02LilrsERw9t6Pv5Nc\n4k6Pz1EaDicBMpdpxKduSZu5OANqUq8er4GM95omAGIOPOh+Nx0spthYA2BqGz+l\n6HRkPJ7S236FZz73In/KVuLnwI8JJ2CbuJap8kvheCCZpmAWpb/cPx/3Vr/J6I17\nXcW+ML9FoCI6AOvOzwIDAQAB\n-----END PUBLIC KEY-----";
pub(super) const VERSION: &str = "1.1.49";

fn swap(s: &str) -> String {
    let third = s.len() / 3;
    format!(
        "{}{}{}",
        &s[s.len() - third..],
        &s[third..s.len() - third],
        &s[..third]
    )
}
pub(super) fn encode(text: &str) -> String {
    let mut result = String::new();
    let mut acc = 0u32;
    let mut bits = 0;
    for byte in text.bytes() {
        acc = ((acc << 8) | u32::from(byte)) & 0xffffff;
        bits += 8;
        while bits >= 6 {
            bits -= 6;
            result.push(ALPHABET[((acc >> bits) & 63) as usize] as char);
        }
    }
    if bits > 0 {
        result.push(ALPHABET[((acc << (6 - bits)) & 63) as usize] as char);
    }
    while !result.len().is_multiple_of(4) {
        result.push('$');
    }
    swap(&result)
}
#[cfg(test)]
pub(super) fn decode(wire: &str) -> Result<String, String> {
    if !wire.is_ascii() || !wire.len().is_multiple_of(4) {
        return Err("Invalid request encoding".into());
    }
    let raw = swap(wire);
    let mut bytes = vec![];
    for group in raw.as_bytes().chunks_exact(4) {
        let mut acc = 0u32;
        let mut bits = 0;
        let mut padding = false;
        for byte in group {
            if *byte == b'$' {
                padding = true;
                continue;
            }
            if padding {
                return Err("Invalid padding".into());
            }
            let value = ALPHABET
                .iter()
                .position(|b| b == byte)
                .ok_or("Invalid alphabet")?;
            acc = (acc << 6) | value as u32;
            bits += 6;
        }
        let count = bits / 8;
        for index in 0..count {
            bytes.push(((acc >> (bits - 8 * (index + 1))) & 255) as u8);
        }
    }
    String::from_utf8(bytes).map_err(|_| "Invalid UTF-8".into())
}
fn text<'a>(c: &'a Value, key: &str) -> Result<&'a str, ProxyError> {
    c[key].as_str().filter(|s| !s.is_empty()).ok_or_else(|| {
        ProxyError::new(
            axum::http::StatusCode::UNAUTHORIZED,
            "Qoder account credentials are incomplete",
        )
    })
}
pub(super) fn headers(c: &Value, path: &str, body: &str) -> Result<HeaderMap, ProxyError> {
    headers_with_versions(c, path, body, VERSION, "")
}

pub(super) fn speech_headers(c: &Value, path: &str) -> Result<HeaderMap, ProxyError> {
    headers_with_versions(c, path, "", "1.0.0", text(c, "appVersion")?)
}

fn headers_with_versions(
    c: &Value,
    path: &str,
    body: &str,
    cosy_version: &str,
    ide_version: &str,
) -> Result<HeaderMap, ProxyError> {
    let user = text(c, "userId")?;
    let token = text(c, "accessToken")?;
    let machine = text(c, "machineId")?;
    let key = uuid::Uuid::new_v4().simple().to_string();
    let key = &key.as_bytes()[..16];
    let blob=json!({"uid":user,"aid":"","name":c["name"].as_str().unwrap_or(""),"email":c["email"].as_str().unwrap_or(""),"security_oauth_token":token}).to_string();
    let info = STANDARD.encode(
        cbc::Encryptor::<aes::Aes128>::new_from_slices(key, key)
            .map_err(ProxyError::upstream)?
            .encrypt_padded_vec_mut::<Pkcs7>(blob.as_bytes()),
    );
    let rsa = RsaPublicKey::from_public_key_pem(PUBLIC_KEY).map_err(ProxyError::upstream)?;
    let wrapped = STANDARD.encode(
        rsa.encrypt(&mut rand::thread_rng(), Pkcs1v15Encrypt, key)
            .map_err(ProxyError::upstream)?,
    );
    let payload=STANDARD.encode(json!({"version":"v1","requestId":uuid::Uuid::new_v4().simple().to_string(),"info":info,"cosyVersion":cosy_version,"ideVersion":ide_version}).to_string());
    let timestamp = chrono::Utc::now().timestamp().to_string();
    let path = path
        .split('?')
        .next()
        .unwrap_or(path)
        .strip_prefix("/algo")
        .unwrap_or(path.split('?').next().unwrap_or(path));
    let signature = format!(
        "{:x}",
        Md5::digest(format!("{payload}\n{wrapped}\n{timestamp}\n{body}\n{path}"))
    );
    let authorization = format!("Bearer COSY.{payload}.{signature}");
    let os = match std::env::consts::OS {
        "windows" => "win32",
        "macos" => "darwin",
        os => os,
    };
    let architecture = std::env::consts::ARCH;
    let machine_os = format!("{architecture}_{os}");
    let mut headers = HeaderMap::new();
    for (key, value) in [
        ("accept", "application/json"),
        ("accept-encoding", "identity"),
        ("content-type", "application/json"),
        ("authorization", authorization.as_str()),
        ("cosy-business-product", "app"),
        ("cosy-business-type", "agent"),
        ("cosy-clientip", machine),
        ("cosy-clienttype", "10"),
        ("cosy-data-policy", "disagree"),
        ("cosy-date", timestamp.as_str()),
        ("cosy-key", wrapped.as_str()),
        ("cosy-machineid", machine),
        ("cosy-machinetoken", machine),
        ("cosy-machinetype", "5"),
        ("cosy-machineos", machine_os.as_str()),
        ("cosy-scene", "app"),
        ("cosy-user", user),
        ("cosy-version", cosy_version),
        ("login-version", "v2"),
    ] {
        headers.insert(
            key,
            HeaderValue::from_str(value).map_err(|_| {
                ProxyError::new(
                    axum::http::StatusCode::UNAUTHORIZED,
                    "Qoder account identity contains invalid header data",
                )
            })?,
        );
    }
    Ok(headers)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn body_encoding_handles_utf8_padding_and_fixed_vectors() {
        for text in [
            "",
            "a",
            "ab",
            "abc",
            "模型与 tools 🦜",
            "{\"messages\":[\"test\"]}",
        ] {
            assert_eq!(decode(&encode(text)).unwrap(), text);
        }
        assert_eq!(encode("abc"), "MSK#");
        assert!(decode("éééé").is_err());
        assert!(decode("????").is_err());
    }
    #[test]
    fn envelope_signs_encoded_body_and_path_with_bound_identity() {
        let c = json!({"userId":"user","accessToken":"private-token","machineId":"machine","name":"name","email":"mail"});
        let body = encode("{}");
        let h = headers(&c, "/algo/api/v2/model/list?Encode=1", &body).unwrap();
        assert_eq!(h["cosy-machineid"], "machine");
        assert_eq!(h["cosy-user"], "user");
        assert!(!h["authorization"]
            .to_str()
            .unwrap()
            .contains("private-token"));
        let raw = h["authorization"]
            .to_str()
            .unwrap()
            .strip_prefix("Bearer COSY.")
            .unwrap();
        let (payload, signature) = raw.rsplit_once('.').unwrap();
        let expected = format!(
            "{:x}",
            Md5::digest(format!(
                "{payload}\n{}\n{}\n{body}\n/api/v2/model/list",
                h["cosy-key"].to_str().unwrap(),
                h["cosy-date"].to_str().unwrap()
            ))
        );
        assert_eq!(signature, expected);
        let payload: Value = serde_json::from_slice(&STANDARD.decode(payload).unwrap()).unwrap();
        assert_eq!(payload["cosyVersion"], VERSION);
        assert_eq!(payload["ideVersion"], "");
        assert!(payload["info"].is_string());
    }

    #[test]
    fn speech_envelope_uses_native_app_versions_and_signs_empty_body_without_changing_chat() {
        let c = json!({"userId":"user","accessToken":"device-token","machineId":"machine","appVersion":"0.4.9"});
        let path = "/api/v2/service/ws/asr";
        let h = speech_headers(&c, path).unwrap();
        let raw = h["authorization"]
            .to_str()
            .unwrap()
            .strip_prefix("Bearer COSY.")
            .unwrap();
        let (payload, signature) = raw.rsplit_once('.').unwrap();
        let expected = format!(
            "{:x}",
            Md5::digest(format!(
                "{payload}\n{}\n{}\n\n{path}",
                h["cosy-key"].to_str().unwrap(),
                h["cosy-date"].to_str().unwrap()
            ))
        );
        assert_eq!(signature, expected);
        let payload: Value = serde_json::from_slice(&STANDARD.decode(payload).unwrap()).unwrap();
        assert_eq!(payload["cosyVersion"], "1.0.0");
        assert_eq!(payload["ideVersion"], "0.4.9");
        assert_eq!(h["cosy-version"], "1.0.0");
        assert_eq!(headers(&c, path, "").unwrap()["cosy-version"], VERSION);
    }
}
