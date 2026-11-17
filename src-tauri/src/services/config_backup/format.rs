use aes_gcm::{
    aead::{Aead, Payload},
    Aes256Gcm, KeyInit,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use rand::RngCore;
use serde::{Deserialize, Serialize};

pub(super) const LIMIT: u64 = 32 * 1024 * 1024;
const ROUNDS: u32 = 600_000;
const AAD: &[u8] = b"EchoBird configuration backup v1";
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    format: String,
    version: u32,
    kdf: String,
    salt: String,
    nonce: String,
    data: String,
}
fn key(password: &str, salt: &[u8]) -> Result<[u8; 32], String> {
    if password.is_empty() || password.len() > 1024 {
        return Err("backupError.password".into());
    }
    let mut key = [0; 32];
    pbkdf2::pbkdf2_hmac::<sha2::Sha256>(password.as_bytes(), salt, ROUNDS, &mut key);
    Ok(key)
}
pub(super) fn encode(bytes: &[u8], password: &str) -> Result<Vec<u8>, String> {
    if bytes.len() as u64 > LIMIT / 2 {
        return Err("backupError.size".into());
    }
    let mut salt = [0; 16];
    let mut nonce = [0; 12];
    rand::rngs::OsRng.fill_bytes(&mut salt);
    rand::rngs::OsRng.fill_bytes(&mut nonce);
    let secret = key(password, &salt)?;
    let cipher = Aes256Gcm::new(&secret.into())
        .encrypt(
            (&nonce).into(),
            Payload {
                msg: bytes,
                aad: AAD,
            },
        )
        .map_err(|_| "backupError.format")?;
    serde_json::to_vec(&Envelope {
        format: "echobird-configuration-backup".into(),
        version: 1,
        kdf: "pbkdf2-sha256-600000".into(),
        salt: STANDARD.encode(salt),
        nonce: STANDARD.encode(nonce),
        data: STANDARD.encode(cipher),
    })
    .map_err(|_| "backupError.format".into())
}
pub(super) fn decode(bytes: &[u8], password: &str) -> Result<Vec<u8>, String> {
    if bytes.len() as u64 > LIMIT {
        return Err("backupError.size".into());
    }
    let envelope: Envelope = serde_json::from_slice(bytes).map_err(|_| "backupError.format")?;
    if envelope.format != "echobird-configuration-backup"
        || envelope.version != 1
        || envelope.kdf != "pbkdf2-sha256-600000"
    {
        return Err("backupError.version".into());
    }
    let salt = STANDARD
        .decode(envelope.salt)
        .map_err(|_| "backupError.format")?;
    let nonce = STANDARD
        .decode(envelope.nonce)
        .map_err(|_| "backupError.format")?;
    let cipher = STANDARD
        .decode(envelope.data)
        .map_err(|_| "backupError.format")?;
    if salt.len() != 16
        || nonce.len() != 12
        || cipher.len() < 16
        || cipher.len() as u64 > LIMIT / 2 + 16
    {
        return Err("backupError.format".into());
    }
    let secret = key(password, &salt)?;
    Aes256Gcm::new(&secret.into())
        .decrypt(
            nonce.as_slice().into(),
            Payload {
                msg: &cipher,
                aad: AAD,
            },
        )
        .map_err(|_| "backupError.password".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn encrypted_backup_requires_password_and_authenticates_every_byte() {
        let encrypted = encode(b"private-api-key", "fixture backup password").unwrap();
        assert!(!String::from_utf8_lossy(&encrypted).contains("private-api-key"));
        assert_eq!(
            decode(&encrypted, "fixture backup password").unwrap(),
            b"private-api-key"
        );
        assert_eq!(
            decode(&encrypted, "wrong password").unwrap_err(),
            "backupError.password"
        );
        let mut envelope: Envelope = serde_json::from_slice(&encrypted).unwrap();
        let mut bytes = STANDARD.decode(&envelope.data).unwrap();
        bytes[0] ^= 1;
        envelope.data = STANDARD.encode(bytes);
        assert!(decode(
            &serde_json::to_vec(&envelope).unwrap(),
            "fixture backup password"
        )
        .is_err());
        envelope.version = 99;
        assert_eq!(
            decode(
                &serde_json::to_vec(&envelope).unwrap(),
                "fixture backup password"
            )
            .unwrap_err(),
            "backupError.version"
        );
        assert!(decode(b"truncated", "fixture").is_err());
    }
}
