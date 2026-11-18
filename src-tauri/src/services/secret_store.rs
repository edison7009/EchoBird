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
    machine_fingerprint_with(&FINGERPRINT, probe_machine_fingerprint, String::new)
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
        use winreg::{enums::HKEY_LOCAL_MACHINE, RegKey};
        if let Ok(key) =
            RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey(r"SOFTWARE\Microsoft\Cryptography")
        {
            if let Ok(value) = key.get_value::<String, _>("MachineGuid") {
                if !value.trim().is_empty() {
                    return Some(value);
                }
            }
        }
    }

    #[cfg(target_os = "macos")]
    {
        if let Ok(output) = crate::utils::process::command("/usr/sbin/ioreg")
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

fn encryption_keys(create: bool) -> Result<Vec<[u8; 32]>, String> {
    let path = echobird_dir().join("config/.encryption_key");
    let file_key = if create {
        fs::create_dir_all(path.parent().ok_or("Missing key directory")?)
            .map_err(|e| e.to_string())?;
        let _lock = super::json_store::FileLock::acquire(&path.with_extension("key-lock"), true)?;
        key_at(&path, true)?
    } else {
        key_at(&path, false)?
    };
    let native = crate::utils::platform::native_username().filter(|value| !value.is_empty());
    let fingerprint = get_machine_fingerprint();
    if create && (native.is_none() || fingerprint.is_empty()) {
        return Err(
            "Stable device or OS account identity is unavailable; existing secrets were preserved"
                .into(),
        );
    }
    let mut identities: Vec<String> = native.into_iter().collect();
    if !create {
        identities.extend(
            [std::env::var("USERNAME").ok(), std::env::var("USER").ok()]
                .into_iter()
                .flatten(),
        );
        identities.push("default".into());
    }
    let mut fingerprints = vec![fingerprint];
    if !create {
        fingerprints.push(fallback_machine_fingerprint());
        fingerprints.push("echobird-fallback".into());
    }
    identity_keys(&file_key, &fingerprints, &identities)
}

fn identity_keys(
    file_key: &[u8],
    fingerprints: &[String],
    identities: &[String],
) -> Result<Vec<[u8; 32]>, String> {
    let mut keys = Vec::new();
    for fingerprint in fingerprints {
        for identity in identities {
            let key = derive_encryption_key(file_key, fingerprint, identity)?;
            if !keys.contains(&key) {
                keys.push(key);
            }
        }
    }
    Ok(keys)
}

pub fn encrypt(plain: &str) -> Result<String, String> {
    if plain.is_empty() || plain == "local" || plain.starts_with(ENCRYPTED_PREFIX) {
        return Ok(plain.into());
    }
    let keys = encryption_keys(true)?;
    let key = keys
        .first()
        .ok_or("Stable encryption identity is unavailable")?;
    encrypt_with_key(plain, key)
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
    let keys = encryption_keys(false)?;
    decrypt_with_keys(stored, &keys)
}

fn decrypt_with_keys(stored: &str, keys: &[[u8; 32]]) -> Result<String, String> {
    for key in keys {
        if let Ok(plain) = decrypt_with_key(stored, key) {
            return Ok(plain);
        }
    }
    Err("Secret could not be decrypted with the available local identities; original data was preserved".into())
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
    fn stable_os_identity_and_legacy_launch_identities_remain_compatible_without_rewriting() {
        let file = [4; 32];
        let fingerprint = "fixture-machine".to_string();
        let stable = identity_keys(
            &file,
            std::slice::from_ref(&fingerprint),
            &["os-user".into()],
        )
        .unwrap();
        let stored = encrypt_with_key("fixture-secret", &stable[0]).unwrap();
        let with_changed_env = identity_keys(
            &file,
            std::slice::from_ref(&fingerprint),
            &["os-user".into(), "changed-env".into(), "default".into()],
        )
        .unwrap();
        assert_eq!(stable[0], with_changed_env[0]);
        assert_eq!(
            decrypt_with_keys(&stored, &with_changed_env).unwrap(),
            "fixture-secret"
        );
        let old_default = derive_encryption_key(&file, &fingerprint, "default").unwrap();
        let legacy = encrypt_with_key("legacy-secret", &old_default).unwrap();
        assert_eq!(
            decrypt_with_keys(&legacy, &with_changed_env).unwrap(),
            "legacy-secret"
        );
        let legacy_empty = derive_encryption_key(&file, "", "default").unwrap();
        let empty_stored = encrypt_with_key("legacy-empty-host", &legacy_empty).unwrap();
        let legacy_keys = identity_keys(
            &file,
            &[fingerprint.clone(), String::new()],
            &["os-user".into(), "default".into()],
        )
        .unwrap();
        assert_eq!(
            decrypt_with_keys(&empty_stored, &legacy_keys).unwrap(),
            "legacy-empty-host"
        );
        assert!(decrypt_with_keys(&stored, &[[99; 32]]).is_err());
        assert_eq!(
            decrypt_with_keys(&stored, &stable).unwrap(),
            "fixture-secret"
        );
    }

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
