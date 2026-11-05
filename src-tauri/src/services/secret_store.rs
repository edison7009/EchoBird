//! Versioned EchoBird secret envelope; native client vault formats stay in provider adapters.
use crate::utils::platform::echobird_dir;
use std::{fs, path::Path};
pub(crate) const ENCRYPTED_PREFIX: &str = "enc:v1:";
/// Get stable machine fingerprint (does NOT change on app upgrade/reinstall)
/// Windows: HKLM\SOFTWARE\Microsoft\Cryptography\MachineGuid
/// macOS: IOPlatformUUID via ioreg
/// Linux: /etc/machine-id
fn get_machine_fingerprint() -> String {
    static FINGERPRINT: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);
    machine_fingerprint_with(
        &FINGERPRINT,
        probe_machine_fingerprint,
        fallback_machine_fingerprint,
    )
}

pub(crate) fn machine_fingerprint_with(
    cache: &std::sync::Mutex<Option<String>>,
    probe: impl FnOnce() -> Option<String>,
    fallback: impl FnOnce() -> String,
) -> String {
    let mut cached = cache
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(fingerprint) = cached.as_ref() {
        return fingerprint.clone();
    }
    if let Some(fingerprint) = probe() {
        if !fingerprint.is_empty() {
            *cached = Some(fingerprint.clone());
        }
        return fingerprint;
    }
    drop(cached);
    fallback()
}

fn probe_machine_fingerprint() -> Option<String> {
    #[cfg(windows)]
    {
        if let Ok(output) = crate::utils::process::command("reg")
            .args([
                "query",
                r"HKLM\SOFTWARE\Microsoft\Cryptography",
                "/v",
                "MachineGuid",
            ])
            .output()
        {
            let text = String::from_utf8_lossy(&output.stdout);
            // Output format: "    MachineGuid    REG_SZ    <guid>"
            if let Some(line) = text.lines().find(|l| l.contains("MachineGuid")) {
                if let Some(guid) = line.split_whitespace().last() {
                    return Some(guid.to_string());
                }
            }
        }
    }

    #[cfg(target_os = "macos")]
    {
        if let Ok(output) = crate::utils::process::command("ioreg")
            .args(["-rd1", "-c", "IOPlatformExpertDevice"])
            .output()
        {
            let text = String::from_utf8_lossy(&output.stdout);
            for line in text.lines() {
                if line.contains("IOPlatformUUID") {
                    if let Some(uuid) = line.split('"').nth(3) {
                        return Some(uuid.to_string());
                    }
                }
            }
        }
    }

    #[cfg(target_os = "linux")]
    {
        if let Ok(id) = std::fs::read_to_string("/etc/machine-id") {
            let trimmed = id.trim().to_string();
            if !trimmed.is_empty() {
                return Some(trimmed);
            }
        }
    }

    None
}

fn fallback_machine_fingerprint() -> String {
    // Fallback: hostname via env var
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "echobird-fallback".to_string())
}

pub(crate) fn derive_encryption_key(
    file_key: &[u8],
    fingerprint: &str,
    username: &str,
) -> Result<[u8; 32], String> {
    use hmac::Mac;
    type HmacSha256 = hmac::Hmac<sha2::Sha256>;

    let identity = format!("{}|{}", fingerprint, username);

    let mut mac = <HmacSha256 as Mac>::new_from_slice(file_key)
        .map_err(|e| format!("HMAC init error: {}", e))?;
    hmac::Mac::update(&mut mac, identity.as_bytes());
    let result = mac.finalize().into_bytes();

    let mut derived_key = [0u8; 32];
    derived_key.copy_from_slice(&result);
    Ok(derived_key)
}

fn key_at(path: &Path, create: bool) -> Result<Vec<u8>, String> {
    use std::io::Write;
    match fs::read(path) {
        Ok(bytes) if bytes.len() == 32 => Ok(bytes),
        Ok(_) => Err("Invalid encryption key; existing secrets were preserved".into()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound && create => {
            fs::create_dir_all(path.parent().ok_or("Missing key directory")?)
                .map_err(|e| e.to_string())?;
            let mut options = fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = match options.open(path) {
                Ok(file) => file,
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    return key_at(path, false)
                }
                Err(e) => return Err(e.to_string()),
            };
            use rand::RngCore;
            let mut bytes = vec![0; 32];
            rand::thread_rng().fill_bytes(&mut bytes);
            if let Err(error) = file.write_all(&bytes).and_then(|_| file.sync_all()) {
                drop(file);
                let _ = fs::remove_file(path);
                return Err(error.to_string());
            }
            Ok(bytes)
        }
        Err(e) => Err(format!("Encryption key unavailable: {e}")),
    }
}

fn encryption_key(create: bool) -> Result<[u8; 32], String> {
    let path = echobird_dir().join("config/.encryption_key");
    let file_key = if create {
        fs::create_dir_all(path.parent().ok_or("Missing key directory")?)
            .map_err(|e| e.to_string())?;
        let _lock = super::json_store::FileLock::acquire(&path.with_extension("key-lock"), true)?;
        key_at(&path, true)?
    } else {
        key_at(&path, false)?
    };
    let username = std::env::var("USERNAME")
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_else(|_| "default".into());
    derive_encryption_key(&file_key, &get_machine_fingerprint(), &username)
}

pub fn encrypt(plain: &str) -> Result<String, String> {
    if plain.is_empty() || plain == "local" || plain.starts_with(ENCRYPTED_PREFIX) {
        return Ok(plain.into());
    }
    encrypt_with_key(plain, &encryption_key(true)?)
}

fn encrypt_with_key(plain: &str, key: &[u8; 32]) -> Result<String, String> {
    use aes_gcm::{aead::Aead, Aes256Gcm, KeyInit};
    use rand::RngCore;
    let mut nonce = [0; 12];
    rand::thread_rng().fill_bytes(&mut nonce);
    let mut bytes = nonce.to_vec();
    bytes.extend(
        Aes256Gcm::new(key.into())
            .encrypt(aes_gcm::Nonce::from_slice(&nonce), plain.as_bytes())
            .map_err(|_| "Secret encryption failed")?,
    );
    Ok(format!("{ENCRYPTED_PREFIX}{}", hex::encode(bytes)))
}

pub fn decrypt(stored: &str) -> Result<String, String> {
    if !stored.starts_with(ENCRYPTED_PREFIX) {
        return Ok(stored.into());
    }
    decrypt_with_key(stored, &encryption_key(false)?)
}

fn decrypt_with_key(stored: &str, key: &[u8; 32]) -> Result<String, String> {
    use aes_gcm::{aead::Aead, Aes256Gcm, KeyInit};
    let bytes = hex::decode(
        stored
            .strip_prefix(ENCRYPTED_PREFIX)
            .ok_or("Invalid secret envelope")?,
    )
    .map_err(|_| "Invalid secret encoding")?;
    if bytes.len() < 28 {
        return Err("Invalid secret envelope".into());
    }
    let plain = Aes256Gcm::new(key.into())
        .decrypt(aes_gcm::Nonce::from_slice(&bytes[..12]), &bytes[12..])
        .map_err(|_| "Secret decryption failed")?;
    String::from_utf8(plain).map_err(|_| "Invalid secret text".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn legacy_envelopes_round_trip_and_malformed_input_never_panics() {
        let key = [7; 32];
        let stored = encrypt_with_key("synthetic-secret", &key).unwrap();
        assert!(stored.starts_with("enc:v1:"));
        assert_eq!(decrypt_with_key(&stored, &key).unwrap(), "synthetic-secret");
        assert!(decrypt_with_key(&stored, &[8; 32]).is_err());
        for value in ["enc:v1:1", "enc:v1:界", "enc:v1:zz", "enc:v1:"] {
            assert!(decrypt_with_key(value, &key).is_err());
        }
        let root = std::env::temp_dir().join(format!("echobird-secret-{}", uuid::Uuid::new_v4()));
        let path = root.join("key");
        assert!(key_at(&path, false).is_err());
        assert!(!root.exists());
        let original = key_at(&path, true).unwrap();
        assert_eq!(key_at(&path, true).unwrap(), original);
        fs::write(&path, b"damaged").unwrap();
        assert!(key_at(&path, true).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"damaged");
        fs::remove_dir_all(root).unwrap();
    }
}
