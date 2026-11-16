use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

use echo_gateway::signatures::{self, Store};
use sha2::{Digest, Sha256};

static WRITES: AtomicUsize = AtomicUsize::new(0);

fn write_signature(path: &Path, bytes: &[u8]) -> Result<(), String> {
    WRITES.fetch_add(1, Ordering::SeqCst);
    fs::create_dir_all(path.parent().unwrap()).map_err(|error| error.to_string())?;
    fs::write(path, bytes).map_err(|error| error.to_string())
}

#[test]
fn configured_store_preserves_legacy_signatures_and_uses_the_app_writer() {
    let directory = std::env::temp_dir().join(format!(
        "echo-gateway-configured-signatures-{}",
        uuid::Uuid::new_v4()
    ));
    fs::create_dir_all(&directory).unwrap();

    // This is the on-disk shape written before the shared-crate extraction.
    let legacy = br#"{"id":"original-call","signature":"original-thought"}"#;
    let key = hex::encode(Sha256::digest(legacy))[..32].to_string();
    let legacy_id = format!("call_eb_{key}");
    let legacy_path = directory.join(format!("{key}.json"));
    fs::write(&legacy_path, legacy).unwrap();

    signatures::configure(Store {
        directory: directory.clone(),
        write: write_signature,
    });
    assert_eq!(
        signatures::decode(&legacy_id),
        Some(("original-call".into(), "original-thought".into()))
    );
    assert_eq!(WRITES.load(Ordering::SeqCst), 0);

    let repeated = signatures::encode("original-call", "original-thought").unwrap();
    assert_eq!(repeated, legacy_id);
    assert_eq!(fs::read(&legacy_path).unwrap(), legacy);
    assert_eq!(WRITES.load(Ordering::SeqCst), 1);

    let next = signatures::encode("next-call", "next-thought").unwrap();
    assert_eq!(
        signatures::decode(&next),
        Some(("next-call".into(), "next-thought".into()))
    );
    let next_key = next.strip_prefix("call_eb_").unwrap();
    assert!(directory.join(format!("{next_key}.json")).is_file());
    assert_eq!(WRITES.load(Ordering::SeqCst), 2);

    assert_eq!(
        signatures::encode("unsigned-call", "").unwrap(),
        "unsigned-call"
    );
    assert_eq!(WRITES.load(Ordering::SeqCst), 2);
    fs::remove_dir_all(directory).unwrap();
}
