// SS Proxy crypto utilities �?mirrors old ssCrypto.ts
// Shadowsocks AEAD cipher implementations

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes128Gcm, Aes256Gcm, Nonce};
use hmac::Mac;
use md5::{Digest, Md5};
use sha1::Sha1;

type HmacSha1 = hmac::Hmac<Sha1>;

/// Cipher configuration
#[derive(Debug, Clone)]
pub struct CipherInfo {
    pub algorithm: &'static str,
    pub key_len: usize,
    pub salt_len: usize,
    pub nonce_len: usize,
    pub tag_len: usize,
}

/// Supported ciphers
pub fn get_cipher_info(name: &str) -> Option<CipherInfo> {
    match name {
        "aes-128-gcm" => Some(CipherInfo {
            algorithm: "aes-128-gcm",
            key_len: 16,
            salt_len: 16,
            nonce_len: 12,
            tag_len: 16,
        }),
        "aes-256-gcm" => Some(CipherInfo {
            algorithm: "aes-256-gcm",
            key_len: 32,
            salt_len: 32,
            nonce_len: 12,
            tag_len: 16,
        }),
        "chacha20-ietf-poly1305" => Some(CipherInfo {
            algorithm: "chacha20-poly1305",
            key_len: 32,
            salt_len: 32,
            nonce_len: 12,
            tag_len: 16,
        }),
        _ => None,
    }
}

pub const HKDF_INFO: &str = "ss-subkey";

/// MD5 KDF (EVP_BytesToKey equivalent)
pub fn kdf(password: &str, key_len: usize) -> Vec<u8> {
    let pass_bytes = password.as_bytes();
    let mut key = Vec::new();
    let mut prev = Vec::new();

    while key.len() < key_len {
        let mut hasher = Md5::new();
        hasher.update(&prev);
        hasher.update(pass_bytes);
        let digest = hasher.finalize().to_vec();
        key.extend_from_slice(&digest);
        prev = digest;
    }

    key.truncate(key_len);
    key
}

/// HKDF-SHA1 key derivation
pub fn hkdf_sha1(secret: &[u8], salt: &[u8], info: &str, length: usize) -> Vec<u8> {
    // Extract phase
    let prk = {
        let mut mac = <HmacSha1 as Mac>::new_from_slice(salt).expect("HMAC key length");
        Mac::update(&mut mac, secret);
        mac.finalize().into_bytes().to_vec()
    };

    // Expand phase
    let info_bytes = info.as_bytes();
    let mut result = Vec::new();
    let mut prev = Vec::new();
    let mut i: u8 = 1;

    while result.len() < length {
        let mut mac = <HmacSha1 as Mac>::new_from_slice(&prk).expect("HMAC key length");
        Mac::update(&mut mac, &prev);
        Mac::update(&mut mac, info_bytes);
        Mac::update(&mut mac, &[i]);
        let digest = mac.finalize().into_bytes().to_vec();
        result.extend_from_slice(&digest);
        prev = digest;
        i += 1;
    }

    result.truncate(length);
    result
}

/// Increment nonce buffer (little-endian)
pub fn increment_nonce(nonce: &mut [u8]) {
    for byte in nonce.iter_mut() {
        *byte = byte.wrapping_add(1);
        if *byte != 0 {
            return;
        }
    }
}

/// Encrypt a chunk using AES-GCM
pub fn encrypt_aead(
    plaintext: &[u8],
    key: &[u8],
    nonce_bytes: &mut [u8],
    algorithm: &str,
) -> Result<Vec<u8>, String> {
    let nonce = Nonce::from_slice(&nonce_bytes[..12]);

    let ciphertext = match algorithm {
        "aes-128-gcm" => {
            let cipher = Aes128Gcm::new_from_slice(key)
                .map_err(|e| format!("AES-128 key error: {}", e))?;
            cipher
                .encrypt(nonce, plaintext)
                .map_err(|e| format!("AES-128 encrypt error: {}", e))?
        }
        "aes-256-gcm" => {
            let cipher = Aes256Gcm::new_from_slice(key)
                .map_err(|e| format!("AES-256 key error: {}", e))?;
            cipher
                .encrypt(nonce, plaintext)
                .map_err(|e| format!("AES-256 encrypt error: {}", e))?
        }
        _ => return Err(format!("Unsupported algorithm: {}", algorithm)),
    };

    increment_nonce(nonce_bytes);
    Ok(ciphertext)
}

/// Decrypt a chunk using AES-GCM
pub fn decrypt_aead(
    ciphertext: &[u8],
    key: &[u8],
    nonce_bytes: &mut [u8],
    algorithm: &str,
) -> Result<Vec<u8>, String> {
    let nonce = Nonce::from_slice(&nonce_bytes[..12]);

    let plaintext = match algorithm {
        "aes-128-gcm" => {
            let cipher = Aes128Gcm::new_from_slice(key)
                .map_err(|e| format!("AES-128 key error: {}", e))?;
            cipher
                .decrypt(nonce, ciphertext)
                .map_err(|e| format!("AES-128 decrypt error: {}", e))?
        }
        "aes-256-gcm" => {
            let cipher = Aes256Gcm::new_from_slice(key)
                .map_err(|e| format!("AES-256 key error: {}", e))?;
            cipher
                .decrypt(nonce, ciphertext)
                .map_err(|e| format!("AES-256 decrypt error: {}", e))?
        }
        _ => return Err(format!("Unsupported algorithm: {}", algorithm)),
    };

    increment_nonce(nonce_bytes);
    Ok(plaintext)
}
