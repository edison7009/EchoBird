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
    pub(super) expires: i64,
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
    #[cfg(target_os = "macos")]
    native_account_files::write(&directory.join("local-server.der"), &identity.certificate)?;
    match std::fs::remove_file(directory.join("process-ca.pem")) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            log::warn!("[ClaudeLocal] Cannot remove obsolete process certificate: {error}")
        }
    }
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

async fn run(
    command: &mut tokio::process::Command,
    seconds: u64,
) -> Result<std::process::Output, String> {
    tokio::time::timeout(
        std::time::Duration::from_secs(seconds),
        command
            .stdin(std::process::Stdio::null())
            .kill_on_drop(true)
            .output(),
    )
    .await
    .map_err(|_| "Claude certificate command timed out".to_owned())?
    .map_err(|error| error.to_string())
}

#[cfg(any(windows, target_os = "linux", all(test, unix)))]
fn root_der(ca_path: &Path) -> Result<Vec<u8>, String> {
    use base64::{engine::general_purpose::STANDARD, Engine};
    let pem = std::fs::read_to_string(ca_path).map_err(|error| error.to_string())?;
    let encoded: String = pem
        .lines()
        .filter(|line| !line.starts_with("-----"))
        .collect();
    STANDARD.decode(encoded).map_err(|error| error.to_string())
}

#[cfg(windows)]
pub(super) async fn ensure_trusted(ca_path: &Path) -> Result<(), String> {
    use sha1::{Digest, Sha1};
    let thumbprint = hex::encode(Sha1::digest(root_der(ca_path)?));
    if trusted(&thumbprint).await? {
        return Ok(());
    }
    let result = run(
        crate::utils::process::async_command("certutil.exe")
            .args(["-user", "-addstore", "Root"])
            .arg(ca_path),
        120,
    )
    .await?;
    if !result.status.success() || !trusted(&thumbprint).await? {
        return Err("EchoBird Claude local certificate was not trusted. Confirm the Windows certificate prompt, then retry.".into());
    }
    Ok(())
}

#[cfg(windows)]
async fn trusted(thumbprint: &str) -> Result<bool, String> {
    Ok(run(
        crate::utils::process::async_command("certutil.exe")
            .args(["-user", "-store", "Root", thumbprint]),
        10,
    )
    .await?
    .status
    .success())
}

#[cfg(target_os = "macos")]
pub(super) async fn ensure_trusted(ca_path: &Path) -> Result<(), String> {
    let server_path = ca_path.with_file_name("local-server.der");
    if macos_trusted(&server_path).await? {
        return Ok(());
    }
    let keychain = run(
        crate::utils::process::async_command("/usr/bin/security").args([
            "default-keychain",
            "-d",
            "user",
        ]),
        10,
    )
    .await?;
    if !keychain.status.success() {
        return Err("Cannot locate the macOS login keychain.".into());
    }
    let keychain = String::from_utf8(keychain.stdout).map_err(|error| error.to_string())?;
    let keychain = keychain.trim().trim_matches('"');
    if keychain.is_empty() {
        return Err("Cannot locate the macOS login keychain.".into());
    }
    // macOS owns any authorization prompt for changing the user's SSL trust.
    let result = run(
        crate::utils::process::async_command("/usr/bin/security")
            .args(["add-trusted-cert", "-r", "trustRoot", "-p", "ssl", "-k"])
            .arg(keychain)
            .arg(ca_path),
        120,
    )
    .await?;
    if !result.status.success() || !macos_trusted(&server_path).await? {
        return Err("EchoBird Claude local certificate was not trusted. Confirm the macOS keychain prompt, then retry.".into());
    }
    Ok(())
}

#[cfg(target_os = "macos")]
async fn macos_trusted(server_path: &Path) -> Result<bool, String> {
    Ok(run(
        crate::utils::process::async_command("/usr/bin/security")
            .args([
                "verify-cert",
                "-L",
                "-q",
                "-p",
                "ssl",
                "-n",
                "claude.ai",
                "-c",
            ])
            .arg(server_path),
        10,
    )
    .await?
    .status
    .success())
}

#[cfg(any(target_os = "linux", test))]
fn nss_database(home: &Path) -> PathBuf {
    let legacy = home.join(".pki/nssdb");
    let modern = home.join(".local/share/pki/nssdb");
    if legacy.join("cert9.db").exists()
        || legacy.join("cert8.db").exists()
        || !modern.join("cert9.db").exists()
    {
        legacy
    } else {
        modern
    }
}

#[cfg(target_os = "linux")]
pub(super) async fn ensure_trusted(ca_path: &Path) -> Result<(), String> {
    let executable = which::which("certutil")
        .map_err(|_| "NSS certutil is required for Claude Desktop certificate trust. Install libnss3-tools or nss-tools, then retry.")?;
    let home = dirs::home_dir().ok_or("Home directory unavailable")?;
    let directory = nss_database(&home);
    trust_nss(&executable, &directory, ca_path).await
}

#[cfg(any(target_os = "linux", all(test, unix)))]
async fn trust_nss(executable: &Path, directory: &Path, ca_path: &Path) -> Result<(), String> {
    use sha2::{Digest, Sha256};
    std::fs::create_dir_all(directory).map_err(|error| error.to_string())?;
    let _lock = DirectoryLock::acquire(&directory.join(".echobird-certificate"))?;
    let database = format!("sql:{}", directory.display());
    if !directory.join("cert9.db").exists() && !directory.join("cert8.db").exists() {
        let initialized = run(
            crate::utils::process::async_command(executable).args([
                "-N",
                "--empty-password",
                "-d",
                &database,
            ]),
            10,
        )
        .await?;
        if !initialized.status.success() {
            return Err("Cannot initialize the Chromium NSS certificate database.".into());
        }
    }
    let der = root_der(ca_path)?;
    let nickname = format!(
        "EchoBird Claude Local CA {}",
        hex::encode(Sha256::digest(&der))
    );
    let existing = run(
        crate::utils::process::async_command(executable)
            .args(["-L", "-r", "-d", &database, "-n", &nickname]),
        10,
    )
    .await?;
    if !existing.status.success() || existing.stdout != der {
        let imported = run(
            crate::utils::process::async_command(executable)
                .args(["-A", "-t", "C,,", "-d", &database, "-n", &nickname, "-i"])
                .arg(ca_path),
            10,
        )
        .await?;
        if !imported.status.success() {
            return Err("Cannot trust the Claude local certificate in Chromium NSS.".into());
        }
    }
    let verified = run(
        crate::utils::process::async_command(executable)
            .args(["-V", "-u", "L", "-d", &database, "-n", &nickname]),
        10,
    )
    .await?;
    if !verified.status.success() {
        return Err("The Claude local certificate is not trusted by Chromium NSS.".into());
    }
    Ok(())
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub(super) async fn ensure_trusted(_ca_path: &Path) -> Result<(), String> {
    Err("Claude Desktop certificate trust is unavailable on this platform.".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nss_database_respects_existing_chromium_storage() {
        let root = std::env::temp_dir().join(format!("claude-nss-{}", uuid::Uuid::new_v4()));
        let legacy = root.join(".pki/nssdb");
        let modern = root.join(".local/share/pki/nssdb");
        assert_eq!(nss_database(&root), legacy);
        std::fs::create_dir_all(&modern).unwrap();
        std::fs::write(modern.join("cert9.db"), []).unwrap();
        assert_eq!(nss_database(&root), modern);
        std::fs::create_dir_all(&legacy).unwrap();
        std::fs::write(legacy.join("cert9.db"), []).unwrap();
        assert_eq!(nss_database(&root), legacy);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn nss_import_is_idempotent_and_failed_verification_stays_visible() {
        use std::os::unix::fs::PermissionsExt;
        let root = std::env::temp_dir().join(format!("claude-nss-import-{}", uuid::Uuid::new_v4()));
        let (_, ca_path) = load(&root).unwrap();
        std::fs::write(root.join("root.der"), root_der(&ca_path).unwrap()).unwrap();
        let executable = root.join("certutil-fixture");
        std::fs::write(
            &executable,
            r#"#!/bin/sh
base=$(dirname "$0")
printf '%s\n' "$1" >> "$base/calls"
case "$1" in
  -N) touch "$base/database/cert9.db" ;;
  -L) cat "$base/installed.der" 2>/dev/null ;;
  -A) cp "$base/root.der" "$base/installed.der" ;;
  -V) test ! -f "$base/reject" ;;
  *) exit 1 ;;
esac
"#,
        )
        .unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        let database = root.join("database");
        trust_nss(&executable, &database, &ca_path).await.unwrap();
        trust_nss(&executable, &database, &ca_path).await.unwrap();
        assert_eq!(
            std::fs::read_to_string(root.join("calls")).unwrap(),
            "-N\n-L\n-A\n-V\n-L\n-V\n"
        );
        std::fs::write(root.join("reject"), []).unwrap();
        assert!(trust_nss(&executable, &database, &ca_path).await.is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn local_certificate_is_stable_and_has_a_verifiable_server_chain() {
        let directory = std::env::temp_dir().join(format!("claude-tls-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join("process-ca.pem"), b"obsolete").unwrap();
        let (first, ca_path) = load(&directory).unwrap();
        assert!(!directory.join("process-ca.pem").exists());
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
        let identity_path = directory.join("tls-identity.json");
        let mut expired: Identity =
            serde_json::from_slice(&std::fs::read(&identity_path).unwrap()).unwrap();
        expired.expires = 0;
        native_account_files::write(&identity_path, &serde_json::to_vec(&expired).unwrap())
            .unwrap();
        let (replacement, _) = load(&directory).unwrap();
        assert_ne!(replacement.certificate, second.certificate);
        std::fs::remove_dir_all(directory).unwrap();
    }
}
