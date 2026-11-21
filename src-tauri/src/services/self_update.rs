//! Explicit self-update downloads: version-matched mirrors and website fallback.
//! Save a verified installer in Downloads. Windows keeps it ready for an
//! explicit installation. macOS and Linux reveal the folder.

#[cfg(any(windows, test))]
mod windows;

use futures_util::{future::join_all, StreamExt};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use tauri::{AppHandle, Emitter};
use tokio::io::AsyncWriteExt;

const MIRROR_MANIFESTS: &[(&str, &str)] = &[
    ("cnb", "https://cnb.cool/echobird/echobird-releases/-/git/raw/main/manifest.json"),
    ("gitcode", "https://api.gitcode.com/api/v5/repos/edison7009/echobird-releases/raw/manifest.json?ref=main"),
];
const DOWNLOAD_IDLE_TIMEOUT: Duration = Duration::from_secs(15);
static DOWNLOAD_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[derive(serde::Deserialize)]
struct MirrorManifest {
    version: String,
    #[serde(default)]
    files: HashMap<String, MirrorFile>,
}
#[derive(serde::Deserialize, serde::Serialize, Clone)]
struct MirrorFile {
    size: u64,
    sha256: String,
}
#[derive(Clone)]
struct Source {
    url: String,
    expected: Option<MirrorFile>,
}
#[derive(Clone, serde::Serialize)]
struct UpdateProgress {
    status: &'static str,
    percent: u8,
}
fn emit(app: &AppHandle, status: &'static str, percent: u8) {
    let _ = app.emit("self-update-progress", UpdateProgress { status, percent });
}

// IDs and suffixes match the release workflow and website download worker.
fn platform(
    os: &str,
    arch: &str,
    deb: bool,
    rpm: bool,
) -> Result<(&'static str, &'static str), String> {
    let arm = arch == "aarch64";
    match (os, arch) {
        ("windows", "x86_64" | "aarch64") => Ok(("windows", "Windows_x64-setup.exe")),
        ("macos", "aarch64") => Ok(("macos-arm", "macOS_arm64.dmg")),
        ("macos", "x86_64") => Ok(("macos-intel", "macOS_x64.dmg")),
        ("linux", "x86_64" | "aarch64") if deb => Ok(if arm {
            ("linux-arm64-deb", "Linux_arm64.deb")
        } else {
            ("linux-deb", "Linux_x64.deb")
        }),
        ("linux", "x86_64" | "aarch64") if rpm => Ok(if arm {
            ("linux-arm64-rpm", "Linux_arm64.rpm")
        } else {
            ("linux-rpm", "Linux_x64.rpm")
        }),
        _ => Err("No installer is available for this system".into()),
    }
}
fn valid_version(version: &str) -> bool {
    let parts: Vec<_> = version.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
}
fn valid_expectation(file: &MirrorFile) -> bool {
    file.size > 0 && file.sha256.len() == 64 && file.sha256.bytes().all(|b| b.is_ascii_hexdigit())
}
fn verify(
    expected: Option<&MirrorFile>,
    total: Option<u64>,
    size: u64,
    digest: &str,
) -> Result<(), String> {
    if size == 0 || total.is_some_and(|n| n != size) {
        return Err("Incomplete download".into());
    }
    if let Some(file) = expected {
        if size != file.size || !digest.eq_ignore_ascii_case(&file.sha256) {
            return Err("Download checksum or size mismatch".into());
        }
    }
    Ok(())
}

async fn sources(
    client: &reqwest::Client,
    version: &str,
    asset: &str,
    website: &str,
) -> Vec<Source> {
    let results = join_all(MIRROR_MANIFESTS.iter().map(|(id, url)| async move {
        let response = client.get(*url).timeout(Duration::from_secs(4)).send().await.ok()?.error_for_status().ok()?;
        let manifest: MirrorManifest = response.json().await.ok()?;
        if manifest.version.trim_start_matches('v') != version { return None; }
        let file = manifest.files.get(asset)?.clone();
        if !valid_expectation(&file) { return None; }
        let url = if *id == "cnb" {
            format!("https://cnb.cool/echobird/echobird-releases/-/git/raw/main/{asset}")
        } else {
            format!("https://api.gitcode.com/api/v5/repos/edison7009/echobird-releases/raw/{asset}?ref=main")
        };
        Some(Source { url, expected: Some(file) })
    })).await;
    let mut candidates: Vec<_> = results.into_iter().flatten().collect();
    let expected = candidates.first().and_then(|c| c.expected.clone());
    candidates.push(Source {
        url: website.into(),
        expected,
    });
    candidates
}

struct Opened {
    source: Source,
    response: reqwest::Response,
    head: Vec<u8>,
    total: Option<u64>,
    rate: f64,
}
async fn probe(client: &reqwest::Client, source: Source, asset: &str) -> Result<Opened, String> {
    let started = Instant::now();
    let mut opened = open_source(client, source, asset).await?;
    let deadline = tokio::time::Instant::now() + Duration::from_millis(1500);
    while opened.head.len() < 256 * 1024 {
        match tokio::time::timeout_at(deadline, opened.response.chunk()).await {
            Ok(Ok(Some(chunk))) => opened.head.extend_from_slice(&chunk),
            Ok(Ok(None)) => break,
            Ok(Err(e)) => return Err(e.to_string()),
            Err(_) => break,
        }
    }
    opened.rate = opened.head.len() as f64 / started.elapsed().as_secs_f64().max(0.001);
    Ok(opened)
}

async fn open_source(
    client: &reqwest::Client,
    source: Source,
    asset: &str,
) -> Result<Opened, String> {
    let response = tokio::time::timeout(Duration::from_secs(8), client.get(&source.url).send())
        .await
        .map_err(|_| "Download source timed out".to_string())?
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?;
    if response
        .headers()
        .get("content-type")
        .and_then(|h| h.to_str().ok())
        .is_some_and(|s| s.contains("text/html"))
    {
        return Err("Download source returned a page instead of an installer".into());
    }
    if let Some(name) = response
        .headers()
        .get("content-disposition")
        .and_then(|h| h.to_str().ok())
    {
        if name.contains("filename=") && !name.contains(asset) {
            return Err("Download source returned a different release".into());
        }
    }
    let total = response.content_length();
    Ok(Opened {
        source,
        response,
        head: Vec::new(),
        total,
        rate: 0.0,
    })
}

async fn write_download(
    opened: Opened,
    path: &Path,
    progress: impl Fn(u8),
) -> Result<MirrorFile, String> {
    let mut file = tokio::fs::File::create(path)
        .await
        .map_err(|e| e.to_string())?;
    let mut hasher = Sha256::new();
    let mut size = opened.head.len() as u64;
    file.write_all(&opened.head)
        .await
        .map_err(|e| e.to_string())?;
    hasher.update(&opened.head);
    let total = opened
        .source
        .expected
        .as_ref()
        .map(|e| e.size)
        .or(opened.total);
    let mut stream = opened.response.bytes_stream();
    let mut last = 0;
    progress(0);
    loop {
        if let Some(total) = total.filter(|n| *n > 0) {
            let pct = (size.saturating_mul(100) / total).min(99) as u8;
            if pct != last {
                progress(pct);
                last = pct;
            }
        }
        let Some(chunk) = stream.next().await else {
            break;
        };
        let chunk = chunk.map_err(|e| e.to_string())?;
        file.write_all(&chunk).await.map_err(|e| e.to_string())?;
        hasher.update(&chunk);
        size += chunk.len() as u64;
    }
    file.sync_all().await.map_err(|e| e.to_string())?;
    let sha256 = format!("{:x}", hasher.finalize());
    verify(opened.source.expected.as_ref(), opened.total, size, &sha256)?;
    Ok(MirrorFile { size, sha256 })
}

async fn download_ranked(
    client: &reqwest::Client,
    asset: &str,
    mut opened: Vec<Opened>,
    path: &Path,
    progress: impl Fn(u8),
) -> Result<MirrorFile, String> {
    opened.sort_by(|a, b| b.rate.total_cmp(&a.rate));
    let mut ranked = opened.into_iter();
    let first = ranked.next().ok_or("No download source is reachable")?;
    // Drop idle probe responses before starting the download. A fallback gets
    // a fresh request and its own timeout budget.
    let backups: Vec<_> = ranked.map(|opened| opened.source).collect();
    let mut result = write_download(first, path, &progress).await;
    for source in backups {
        if result.is_ok() {
            break;
        }
        result = match open_source(client, source, asset).await {
            Ok(opened) => write_download(opened, path, &progress).await,
            Err(error) => Err(error),
        };
    }
    result
}

fn download_client(idle_timeout: Duration) -> reqwest::ClientBuilder {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(8))
        .read_timeout(idle_timeout)
        .timeout(Duration::from_secs(600))
}

fn download_destination(dir: &Path, asset: &str) -> PathBuf {
    let path = dir.join(asset);
    if !path.exists() {
        return path;
    }
    // Preserve existing downloads, including an installer still open on Windows.
    let stem = Path::new(asset)
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy();
    let ext = Path::new(asset)
        .extension()
        .unwrap_or_default()
        .to_string_lossy();
    dir.join(format!("{stem}-{}.{ext}", uuid::Uuid::new_v4()))
}

#[cfg(any(target_os = "windows", all(test, unix)))]
async fn run_windows_installer(path: &Path) -> Result<(), String> {
    // Open the full wizard and let it close EchoBird when installation begins.
    // Keep the update lock held until the wizard is closed or EchoBird exits.
    let status = crate::utils::process::async_command(path)
        .status()
        .await
        .map_err(|e| format!("Failed to launch installer: {e}"))?;
    // NSIS exit code 1 means the user cancelled the wizard.
    if !matches!(status.code(), Some(0 | 1)) {
        return Err(format!("Installer stopped without completing: {status}"));
    }
    Ok(())
}

pub async fn download(app: AppHandle, version: String) -> Result<bool, String> {
    let _guard = DOWNLOAD_LOCK
        .try_lock()
        .map_err(|_| "An update download is already running")?;
    let result = run(&app, &version).await;
    if let Err(error) = &result {
        log::warn!("[SelfUpdate] {error}");
        emit(&app, "error", 0);
    }
    result
}
fn update_location(version: &str) -> Result<(PathBuf, String, &'static str), String> {
    if !valid_version(version) {
        return Err("Invalid update version".into());
    }
    let (id, suffix) = platform(
        std::env::consts::OS,
        std::env::consts::ARCH,
        cfg!(target_os = "linux") && which::which("dpkg").is_ok(),
        cfg!(target_os = "linux") && which::which("rpm").is_ok(),
    )?;
    let asset = format!("EchoBird_{version}_{suffix}");
    let dir = dirs::download_dir()
        .or_else(|| dirs::home_dir().map(|p| p.join("Downloads")))
        .ok_or("Could not locate the system Downloads folder")?;
    Ok((dir, asset, id))
}

pub async fn is_downloaded(version: String) -> Result<bool, String> {
    #[cfg(windows)]
    {
        let (dir, asset, id) = update_location(&version)?;
        Ok(windows::find(&dir, &asset, &version, id).await?.is_some())
    }
    #[cfg(not(windows))]
    {
        let _ = version;
        Ok(false)
    }
}

pub async fn install(app: AppHandle, version: String) -> Result<bool, String> {
    #[cfg(windows)]
    {
        let _guard = DOWNLOAD_LOCK
            .try_lock()
            .map_err(|_| "An update is already running")?;
        let (dir, asset, id) = update_location(&version)?;
        let Some(package) = windows::find(&dir, &asset, &version, id).await? else {
            return Ok(false);
        };
        windows::remember(&asset, &package.fingerprint);
        emit(&app, "launching", 100);
        if let Err(error) = run_windows_installer(&package.path).await {
            log::warn!("[SelfUpdate] {error}");
            return Err(error);
        }
        Ok(true)
    }
    #[cfg(not(windows))]
    {
        let _ = (app, version);
        Ok(false)
    }
}

async fn run(app: &AppHandle, version: &str) -> Result<bool, String> {
    let (dir, asset, id) = update_location(version)?;
    #[cfg(windows)]
    if let Some(package) = windows::find(&dir, &asset, version, id).await? {
        windows::remember(&asset, &package.fingerprint);
        return Ok(true);
    }
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|e| e.to_string())?;
    let dest = download_destination(&dir, &asset);
    let part = dir.join(format!(".{asset}-{}.part", uuid::Uuid::new_v4()));
    let client = download_client(DOWNLOAD_IDLE_TIMEOUT)
        .build()
        .map_err(|e| e.to_string())?;
    emit(app, "speed_test", 0);
    let candidates = sources(
        &client,
        version,
        &asset,
        &format!("https://echobird.ai/download/{id}"),
    )
    .await;
    let opened = join_all(candidates.into_iter().map(|s| probe(&client, s, &asset)))
        .await
        .into_iter()
        .filter_map(Result::ok)
        .collect();
    let fingerprint = match download_ranked(&client, &asset, opened, &part, |pct| {
        emit(app, "downloading", pct);
    })
    .await
    {
        Ok(fingerprint) => fingerprint,
        Err(error) => {
            let _ = tokio::fs::remove_file(&part).await;
            return Err(error);
        }
    };
    if let Err(error) = tokio::fs::rename(&part, &dest).await {
        let _ = tokio::fs::remove_file(&part).await;
        return Err(format!("Could not save installer: {error}"));
    }
    #[cfg(windows)]
    {
        windows::remember(&asset, &fingerprint);
        Ok(true)
    }
    #[cfg(not(windows))]
    {
        let _ = fingerprint;
        crate::commands::tool_commands::open_folder(dir.to_string_lossy().into_owned()).await?;
        emit(app, "downloaded", 100);
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selects_exact_platform_assets_and_package_formats() {
        for (os, arch, deb, rpm, expected) in [
            (
                "windows",
                "x86_64",
                false,
                false,
                ("windows", "Windows_x64-setup.exe"),
            ),
            (
                "macos",
                "aarch64",
                false,
                false,
                ("macos-arm", "macOS_arm64.dmg"),
            ),
            (
                "macos",
                "x86_64",
                false,
                false,
                ("macos-intel", "macOS_x64.dmg"),
            ),
            (
                "linux",
                "x86_64",
                true,
                true,
                ("linux-deb", "Linux_x64.deb"),
            ),
            (
                "linux",
                "aarch64",
                true,
                false,
                ("linux-arm64-deb", "Linux_arm64.deb"),
            ),
            (
                "linux",
                "x86_64",
                false,
                true,
                ("linux-rpm", "Linux_x64.rpm"),
            ),
            (
                "linux",
                "aarch64",
                false,
                true,
                ("linux-arm64-rpm", "Linux_arm64.rpm"),
            ),
        ] {
            assert_eq!(platform(os, arch, deb, rpm).unwrap(), expected);
        }
        assert!(platform("linux", "x86_64", false, false).is_err());
        assert!(platform("macos", "riscv64", false, false).is_err());
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn launches_the_installer_directly_and_waits_for_its_result() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let installer = dir.path().join("installer with spaces & percent %");
        assert!(run_windows_installer(&installer)
            .await
            .unwrap_err()
            .contains("Failed to launch installer"));

        std::fs::write(
            &installer,
            r#"#!/bin/sh
printf '%s\n' "$#" > "$0.args"
if [ -e "$0.status" ]; then
    exit "$(cat "$0.status")"
fi
"#,
        )
        .unwrap();
        std::fs::set_permissions(&installer, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert!(run_windows_installer(&installer).await.is_ok());
        assert_eq!(
            std::fs::read_to_string(installer.with_extension("args")).unwrap(),
            "0\n"
        );
        std::fs::write(installer.with_extension("status"), "1").unwrap();
        assert!(run_windows_installer(&installer).await.is_ok());
        std::fs::write(installer.with_extension("status"), "2").unwrap();
        assert!(run_windows_installer(&installer)
            .await
            .unwrap_err()
            .contains("exit status: 2"));
    }
    #[test]
    fn rejects_unsafe_versions_and_incomplete_or_corrupt_files() {
        for version in ["../6.2.0", "6.2", "6.2.0/path", "6.2.0-beta"] {
            assert!(!valid_version(version));
        }
        assert!(valid_version("6.2.0"));
        let digest = format!("{:x}", Sha256::digest(b"installer"));
        let expected = MirrorFile {
            size: 9,
            sha256: digest.clone(),
        };
        assert!(valid_expectation(&expected));
        assert!(verify(Some(&expected), Some(9), 9, &digest).is_ok());
        assert!(verify(Some(&expected), None, 8, &digest).is_err());
        assert!(verify(Some(&expected), None, 9, "wrong").is_err());
        assert!(verify(None, Some(10), 9, &digest).is_err());
        assert!(verify(None, None, 0, &digest).is_err());
    }
    #[tokio::test]
    async fn stalled_download_reopens_the_next_ranked_source() {
        use axum::{body::Body, http::header, routing::get, Router};
        use futures_util::stream;
        use std::{
            cell::RefCell,
            sync::{
                atomic::{AtomicUsize, Ordering},
                Arc,
            },
        };

        const SAMPLE_SIZE: usize = 256 * 1024;
        let installer = vec![42; SAMPLE_SIZE * 2];
        let expected = MirrorFile {
            size: installer.len() as u64,
            sha256: format!("{:x}", Sha256::digest(&installer)),
        };
        let backup_requests = Arc::new(AtomicUsize::new(0));
        let app = Router::new()
            .route(
                "/primary",
                get(|| async {
                    let head = stream::iter([Ok::<_, std::io::Error>(vec![42; SAMPLE_SIZE])]);
                    (
                        [(header::CONTENT_LENGTH, (SAMPLE_SIZE * 2).to_string())],
                        Body::from_stream(head.chain(stream::pending())),
                    )
                }),
            )
            .route(
                "/backup",
                get({
                    let requests = backup_requests.clone();
                    let installer = installer.clone();
                    move || async move {
                        requests.fetch_add(1, Ordering::Relaxed);
                        installer
                    }
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let client = download_client(Duration::from_millis(250))
            .no_proxy()
            .build()
            .unwrap();
        let mut opened = Vec::new();
        for (name, rate) in [("backup", 1.0), ("primary", 2.0)] {
            let mut candidate = probe(
                &client,
                Source {
                    url: format!("{base}/{name}"),
                    expected: Some(expected.clone()),
                },
                "fixture.exe",
            )
            .await
            .unwrap();
            assert!(candidate.head.len() >= SAMPLE_SIZE);
            candidate.rate = rate;
            opened.push(candidate);
        }

        let dir = tempfile::tempdir().unwrap();
        let part = dir.path().join("fixture.part");
        let progress = RefCell::new(Vec::new());
        let result = tokio::time::timeout(
            Duration::from_secs(5),
            download_ranked(&client, "fixture.exe", opened, &part, |pct| {
                progress.borrow_mut().push(pct);
            }),
        )
        .await
        .expect("the stalled download should time out and switch sources")
        .unwrap();
        assert_eq!(backup_requests.load(Ordering::Relaxed), 2);
        assert!(progress.into_inner().starts_with(&[0, 50, 0]));
        assert_eq!(result.sha256, expected.sha256);
        assert_eq!(tokio::fs::read(&part).await.unwrap(), installer);
        server.abort();
    }

    #[tokio::test]
    async fn streams_real_http_response_and_preserves_complete_download_on_failure() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let dir = std::env::temp_dir().join(format!("echobird-update-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let saved = dir.as_path().join("fixture.exe");
        std::fs::write(&saved, b"previous").unwrap();
        let dest = download_destination(dir.as_path(), "fixture.exe");
        assert_ne!(dest, saved);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/fixture.exe", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            for _ in 0..2 {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut buf = [0; 4096];
                let _ = stream.read(&mut buf).await;
                stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 9\r\nConnection: close\r\n\r\ninstaller").await.unwrap();
            }
        });
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let part = dir.as_path().join("fixture.part");
        for corrupt in [false, true] {
            let expected = MirrorFile {
                size: 9,
                sha256: if corrupt {
                    "0".repeat(64)
                } else {
                    format!("{:x}", Sha256::digest(b"installer"))
                },
            };
            let opened = probe(
                &client,
                Source {
                    url: url.clone(),
                    expected: Some(expected),
                },
                "fixture.exe",
            )
            .await
            .unwrap();
            let result = write_download(opened, &part, |_| {}).await;
            assert_eq!(result.is_ok(), !corrupt);
            if result.is_ok() {
                tokio::fs::rename(&part, &dest).await.unwrap();
            }
        }
        assert_eq!(std::fs::read(&saved).unwrap(), b"previous");
        assert_eq!(std::fs::read(&dest).unwrap(), b"installer");
        server.await.unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
