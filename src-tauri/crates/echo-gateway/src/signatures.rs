//! Gemini thought signatures that must survive a tool round: the signature is kept on disk under a short
//! reference, and the tool-call id carries the reference. Where the files go is the app's choice
//! (`configure`); until it says, a temporary directory is used.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Deserialize, Serialize)]
struct Signature {
    id: String,
    signature: String,
}

fn reference(value: &Signature) -> String {
    let bytes = serde_json::to_vec(value).expect("string fields serialize");
    hex::encode(Sha256::digest(bytes))[..32].to_string()
}

/** Where signatures are kept and how a file is written (the app may want private, synced writes). */
pub struct Store {
    pub directory: PathBuf,
    pub write: fn(&Path, &[u8]) -> Result<(), String>,
}

static STORE: std::sync::OnceLock<Store> = std::sync::OnceLock::new();

/// Sets where signatures are kept. Call once at startup; later calls are ignored.
pub fn configure(store: Store) {
    let _ = STORE.set(store);
}

fn store() -> &'static Store {
    STORE.get_or_init(|| Store {
        directory: std::env::temp_dir()
            .join(format!("echo-gateway-signatures-{}", std::process::id())),
        write: |path, bytes| {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).map_err(|error| error.to_string())?;
            }
            fs::write(path, bytes).map_err(|error| error.to_string())
        },
    })
}

pub fn encode(id: &str, signature: &str) -> Result<String, String> {
    if signature.is_empty() {
        return Ok(id.to_string());
    }
    encode_at(&store().directory, id, signature)
}

fn encode_at(root: &Path, id: &str, signature: &str) -> Result<String, String> {
    let value = Signature {
        id: id.into(),
        signature: signature.into(),
    };
    let key = reference(&value);
    let path = root.join(format!("{key}.json"));
    (store().write)(
        &path,
        &serde_json::to_vec(&value).map_err(|_| "Could not encode a thought signature")?,
    )
    .map_err(|_| "Could not save the thought signature for this tool call")?;
    prune(root);
    Ok(format!("call_eb_{key}"))
}

pub fn decode(id: &str) -> Option<(String, String)> {
    decode_at(&store().directory, id)
}

fn decode_at(root: &Path, id: &str) -> Option<(String, String)> {
    let key = id.strip_prefix("call_eb_")?;
    if key.len() != 32 || !key.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let path = root.join(format!("{key}.json"));
    let value: Signature = serde_json::from_slice(&fs::read(&path).ok()?).ok()?;
    if reference(&value) != key {
        return None;
    }
    if let Ok(file) = fs::OpenOptions::new().write(true).open(path) {
        let _ = file.set_modified(SystemTime::now());
    }
    Some((value.id, value.signature))
}

fn prune(root: &Path) {
    static LAST: Mutex<Option<Instant>> = Mutex::new(None);
    let Ok(mut last) = LAST.lock() else { return };
    if last.is_some_and(|last| last.elapsed() < Duration::from_secs(3600)) {
        return;
    }
    *last = Some(Instant::now());
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let Some(key) = name.strip_suffix(".json") else {
            continue;
        };
        if key.len() != 32 || !key.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            continue;
        }
        let expired = entry
            .metadata()
            .ok()
            .and_then(|metadata| metadata.modified().ok())
            .and_then(|modified| modified.elapsed().ok())
            .is_some_and(|age| age > Duration::from_secs(90 * 86_400));
        if expired {
            let _ = fs::remove_file(entry.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signature_reference_survives_a_fresh_read_and_rejects_corruption() {
        let root = std::env::temp_dir().join(format!(
            "echo-gateway-signature-test-{}-{}",
            std::process::id(),
            line!()
        ));
        let signature = "thought".repeat(1024);
        let id = encode_at(&root, "original-call", &signature).unwrap();
        assert!(id.len() <= 64);
        assert_eq!(
            decode_at(&root, &id),
            Some(("original-call".into(), signature))
        );
        let file = root.join(format!("{}.json", id.strip_prefix("call_eb_").unwrap()));
        fs::write(&file, br#"{"id":"different","signature":"tampered"}"#).unwrap();
        assert!(decode_at(&root, &id).is_none());
        fs::remove_file(&file).unwrap();
        assert!(decode_at(&root, &id).is_none());
        fs::write(&file, b"occupied").unwrap();
        assert!(encode_at(&file, "call", "signature").is_err());
        fs::remove_file(file).unwrap();
        fs::remove_dir(root).unwrap();
    }
}
