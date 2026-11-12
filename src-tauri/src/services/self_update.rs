//! Explicit self-update downloads: version-matched mirrors and website fallback.
//! Save a verified installer in Downloads. Windows runs it silently (`/S`), the
//! Tauri NSIS template relaunches EchoBird afterwards (`/R`), and a failed
//! silent run falls back to the visible wizard; macOS and
//! Linux reveal the containing folder and keep EchoBird running.

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
static DOWNLOAD_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[derive(serde::Deserialize)]
struct MirrorManifest {
    version: String,
    #[serde(default)]
    files: HashMap<String, MirrorFile>,
}
#[derive(serde::Deserialize, Clone)]
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
    let mut response = tokio::time::timeout(Duration::from_secs(8), client.get(&source.url).send())
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
    let mut head = Vec::new();
    let deadline = tokio::time::Instant::now() + Duration::from_millis(1500);
    while head.len() < 256 * 1024 {
        match tokio::time::timeout_at(deadline, response.chunk()).await {
            Ok(Ok(Some(chunk))) => head.extend_from_slice(&chunk),
            Ok(Ok(None)) => break,
            Ok(Err(e)) => return Err(e.to_string()),
            Err(_) => break,
        }
    }
    let rate = head.len() as f64 / started.elapsed().as_secs_f64().max(0.001);
    Ok(Opened {
        source,
        response,
        head,
        total,
        rate,
    })
}

async fn write_download(opened: Opened, path: &Path, progress: impl Fn(u8)) -> Result<(), String> {
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
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| e.to_string())?;
        file.write_all(&chunk).await.map_err(|e| e.to_string())?;
        hasher.update(&chunk);
        size += chunk.len() as u64;
        if let Some(total) = total.filter(|n| *n > 0) {
            let pct = (size.saturating_mul(100) / total).min(99) as u8;
            if pct != last {
                progress(pct);
                last = pct;
            }
        }
    }
    file.sync_all().await.map_err(|e| e.to_string())?;
    verify(
        opened.source.expected.as_ref(),
        opened.total,
        size,
        &format!("{:x}", hasher.finalize()),
    )
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

/// `cmd` arguments that run the installer silently (`/S`, relaunching with
/// `/R`) and open its visible wizard when the silent run fails, so a failed
/// update never leaves the user without EchoBird or an explanation.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn installer_command_line(path: &Path) -> Result<String, String> {
    let path = path.to_string_lossy();
    // cmd cannot escape these inside a quoted path.
    if path.contains(['"', '%', '\n', '\r']) {
        return Err("Installer path cannot be launched safely".into());
    }
    Ok(format!(
        "/d /c start \"\" /wait \"{path}\" /S /R || start \"\" \"{path}\""
    ))
}

#[cfg(target_os = "windows")]
fn spawn_installer(path: &Path) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    crate::utils::process::command("cmd")
        .raw_arg(installer_command_line(path)?)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Failed to launch installer: {e}"))
}

pub async fn download_and_install(app: AppHandle, version: String) -> Result<(), String> {
    let _guard = DOWNLOAD_LOCK
        .try_lock()
        .map_err(|_| "An update download is already running")?;
    let result = run(&app, &version).await;
    if result.is_err() {
        emit(&app, "error", 0);
    }
    result
}
async fn run(app: &AppHandle, version: &str) -> Result<(), String> {
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
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|e| e.to_string())?;
    let dest = download_destination(&dir, &asset);
    let part = dir.join(format!(".{asset}-{}.part", uuid::Uuid::new_v4()));
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(8))
        .timeout(Duration::from_secs(600))
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
    let mut opened: Vec<_> = join_all(candidates.into_iter().map(|s| probe(&client, s, &asset)))
        .await
        .into_iter()
        .filter_map(Result::ok)
        .collect();
    opened.sort_by(|a, b| b.rate.total_cmp(&a.rate));
    let mut last_error = "No download source is reachable".to_string();
    let mut done = false;
    for source in opened {
        match write_download(source, &part, |pct| emit(app, "downloading", pct)).await {
            Ok(()) => {
                done = true;
                break;
            }
            Err(error) => last_error = error,
        }
    }
    if !done {
        let _ = tokio::fs::remove_file(&part).await;
        return Err(last_error);
    }
    if let Err(error) = tokio::fs::rename(&part, &dest).await {
        let _ = tokio::fs::remove_file(&part).await;
        return Err(format!("Could not save installer: {error}"));
    }
    emit(app, "launching", 100);
    #[cfg(target_os = "windows")]
    {
        spawn_installer(&dest)?;
        tokio::time::sleep(Duration::from_millis(600)).await;
        app.exit(0);
    }
    #[cfg(not(target_os = "windows"))]
    {
        crate::commands::tool_commands::open_folder(dir.to_string_lossy().into_owned()).await?;
        emit(app, "downloaded", 100);
    }
    Ok(())
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
    #[test]
    fn installs_silently_and_falls_back_to_the_wizard() {
        let path = Path::new(r"C:\Users\A B\Downloads\EchoBird_6.3.1_Windows_x64-setup.exe");
        assert_eq!(
            installer_command_line(path).unwrap(),
            r#"/d /c start "" /wait "C:\Users\A B\Downloads\EchoBird_6.3.1_Windows_x64-setup.exe" /S /R || start "" "C:\Users\A B\Downloads\EchoBird_6.3.1_Windows_x64-setup.exe""#
        );
        for unsafe_path in [r#"C:\a"b\setup.exe"#, r"C:\%TEMP%\setup.exe"] {
            assert!(installer_command_line(Path::new(unsafe_path)).is_err());
        }
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
