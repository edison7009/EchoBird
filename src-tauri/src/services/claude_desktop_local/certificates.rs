use super::super::native_account_files::{self, DirectoryLock};
use chrono::Datelike;
use rcgen::{
    BasicConstraints, CertificateParams, DnType, ExtendedKeyUsagePurpose, IsCa, Issuer, KeyPair,
    KeyUsagePurpose,
};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize)]
pub(super) struct Identity {
    pub certificate: Vec<u8>,
    pub key: Vec<u8>,
    ca: Vec<u8>,
    pem: String,
    expires: i64,
}

pub(super) fn load(directory: &Path) -> Result<(Identity, PathBuf), String> {
    let path = directory.join("tls-identity.json");
    let _lock = DirectoryLock::acquire(&path)?;
    if std::fs::symlink_metadata(&path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        return Err("Claude local TLS identity must be a regular file".into());
    }
    let existing: Option<Identity> = native_account_files::read_optional(&path)?
        .map(|bytes| serde_json::from_slice(&bytes).map_err(|error| error.to_string()))
        .transpose()?;
    let identity = match existing {
        Some(identity) if identity.expires > chrono::Utc::now().timestamp() => identity,
        _ => {
            let identity = generate()?;
            native_account_files::write(
                &path,
                &serde_json::to_vec(&identity).map_err(|error| error.to_string())?,
            )?;
            identity
        }
    };
    let ca_path = directory.join("local-ca.pem");
    native_account_files::write(&ca_path, identity.pem.as_bytes())?;
    Ok((identity, ca_path))
}

fn generate() -> Result<Identity, String> {
    let now = chrono::Utc::now();
    let start = now - chrono::Duration::days(1);
    let end = now + chrono::Duration::days(365);
    let mut ca = CertificateParams::default();
    ca.distinguished_name
        .push(DnType::CommonName, "EchoBird Claude Local CA");
    ca.is_ca = IsCa::Ca(BasicConstraints::Constrained(0));
    ca.key_usages = vec![KeyUsagePurpose::KeyCertSign];
    ca.not_before = rcgen::date_time_ymd(start.year(), start.month() as u8, start.day() as u8);
    ca.not_after = rcgen::date_time_ymd(end.year(), end.month() as u8, end.day() as u8);
    let ca_key = KeyPair::generate().map_err(|error| error.to_string())?;
    let root = ca.self_signed(&ca_key).map_err(|error| error.to_string())?;
    let mut leaf = CertificateParams::new(vec![
        "claude.ai".into(),
        "www.claude.ai".into(),
        "claude.com".into(),
        "api.anthropic.com".into(),
        "api.claude.ai".into(),
    ])
    .map_err(|error| error.to_string())?;
    leaf.distinguished_name
        .push(DnType::CommonName, "claude.ai");
    leaf.not_before = ca.not_before;
    leaf.not_after = ca.not_after;
    leaf.key_usages = vec![KeyUsagePurpose::DigitalSignature];
    leaf.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
    let key = KeyPair::generate().map_err(|error| error.to_string())?;
    let certificate = leaf
        .signed_by(&key, &Issuer::new(ca, &ca_key))
        .map_err(|error| error.to_string())?;
    // Retain only the fixed server key. The CA signing key is discarded.
    Ok(Identity {
        certificate: certificate.der().to_vec(),
        key: key.serialize_der(),
        ca: root.der().to_vec(),
        pem: root.pem(),
        expires: end
            .date_naive()
            .and_hms_opt(0, 0, 0)
            .unwrap()
            .and_utc()
            .timestamp(),
    })
}

pub(super) async fn ensure_trusted(ca_path: &Path) -> Result<(), String> {
    use base64::{engine::general_purpose::STANDARD, Engine};
    use sha1::{Digest, Sha1};
    let pem = std::fs::read_to_string(ca_path).map_err(|error| error.to_string())?;
    let encoded: String = pem
        .lines()
        .filter(|line| !line.starts_with("-----"))
        .collect();
    let der = STANDARD
        .decode(encoded)
        .map_err(|error| error.to_string())?;
    let thumbprint = hex::encode(Sha1::digest(&der));
    if trusted(&thumbprint).await? {
        return Ok(());
    }
    // The native Windows confirmation belongs to the user. Never force acceptance.
    let mut child = crate::utils::process::async_command("certutil.exe")
        .args(["-user", "-addstore", "Root"])
        .arg(ca_path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| error.to_string())?;
    let status = tokio::time::timeout(std::time::Duration::from_secs(120), child.wait())
        .await
        .map_err(|_| "Windows certificate confirmation timed out.".to_string())?
        .map_err(|error| error.to_string())?;
    if !status.success() || !trusted(&thumbprint).await? {
        return Err("EchoBird Claude local certificate was not trusted. Confirm the Windows certificate prompt, then retry.".into());
    }
    Ok(())
}

async fn trusted(thumbprint: &str) -> Result<bool, String> {
    crate::utils::process::async_command("certutil.exe")
        .args(["-user", "-store", "Root", thumbprint])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await
        .map(|status| status.success())
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_certificate_is_stable_and_has_a_verifiable_server_chain() {
        let directory = std::env::temp_dir().join(format!("claude-tls-{}", uuid::Uuid::new_v4()));
        let (first, ca_path) = load(&directory).unwrap();
        let (second, second_path) = load(&directory).unwrap();
        assert_eq!(first.certificate, second.certificate);
        assert_eq!(first.key, second.key);
        assert_eq!(first.ca, second.ca);
        assert_eq!(ca_path, second_path);
        let mut roots = tokio_rustls::rustls::RootCertStore::empty();
        roots.add(first.ca.into()).unwrap();
        let verifier = tokio_rustls::rustls::client::WebPkiServerVerifier::builder_with_provider(
            std::sync::Arc::new(roots),
            std::sync::Arc::new(tokio_rustls::rustls::crypto::ring::default_provider()),
        )
        .build()
        .unwrap();
        use tokio_rustls::rustls::client::danger::ServerCertVerifier;
        verifier
            .verify_server_cert(
                &first.certificate.clone().into(),
                &[],
                &"claude.ai".try_into().unwrap(),
                &[],
                tokio_rustls::rustls::pki_types::UnixTime::now(),
            )
            .unwrap();
        assert!(verifier
            .verify_server_cert(
                &first.certificate.into(),
                &[],
                &"unrelated.example".try_into().unwrap(),
                &[],
                tokio_rustls::rustls::pki_types::UnixTime::now(),
            )
            .is_err());
        std::fs::remove_dir_all(directory).unwrap();
    }
}
