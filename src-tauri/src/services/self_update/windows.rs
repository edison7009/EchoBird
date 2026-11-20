use super::{valid_expectation, MirrorFile};
use sha2::{Digest, Sha256};
use std::{
    io,
    path::{Path, PathBuf},
};
use tokio::io::AsyncReadExt;

pub(super) struct VerifiedInstaller {
    pub path: PathBuf,
    pub fingerprint: MirrorFile,
    // On Windows this also prevents modification or replacement before launch.
    _file: tokio::fs::File,
}

fn matches_asset(name: &str, asset: &str) -> bool {
    if name == asset {
        return true;
    }
    // Older downloads used a UUID suffix when the ordinary name was occupied.
    let Some(stem) = asset.strip_suffix(".exe") else {
        return false;
    };
    name.strip_prefix(stem)
        .and_then(|suffix| suffix.strip_prefix('-'))
        .and_then(|suffix| suffix.strip_suffix(".exe"))
        .is_some_and(|suffix| uuid::Uuid::parse_str(suffix).is_ok())
}

async fn candidates(dir: &Path, asset: &str) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let Ok(mut entries) = tokio::fs::read_dir(dir).await else {
        return paths;
    };
    while let Ok(Some(entry)) = entries.next_entry().await {
        if entry
            .file_name()
            .to_str()
            .is_some_and(|name| matches_asset(name, asset))
            && entry.file_type().await.is_ok_and(|kind| kind.is_file())
        {
            paths.push(entry.path());
        }
    }
    paths.sort_unstable();
    paths
}

async fn verify_local(
    paths: Vec<PathBuf>,
    expected: MirrorFile,
) -> io::Result<Option<VerifiedInstaller>> {
    if !valid_expectation(&expected) {
        return Ok(None);
    }
    let mut unreadable = None;
    for path in paths {
        match open_verified(&path, &expected).await {
            Ok(Some(file)) => {
                return Ok(Some(VerifiedInstaller {
                    path,
                    fingerprint: expected,
                    _file: file,
                }));
            }
            // A file removed after scanning is absent. Other read errors may be
            // temporary, so do not start another download just because it is busy.
            Err(error) if error.kind() != io::ErrorKind::NotFound => unreadable = Some(error),
            _ => {}
        }
    }
    unreadable.map_or(Ok(None), Err)
}

async fn open_verified(
    path: &Path,
    expected: &MirrorFile,
) -> std::io::Result<Option<tokio::fs::File>> {
    let mut options = tokio::fs::OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    options.share_mode(::windows::Win32::Storage::FileSystem::FILE_SHARE_READ.0);
    let mut file = options.open(path).await?;
    if file.metadata().await?.len() != expected.size {
        return Ok(None);
    }
    let mut hasher = Sha256::new();
    let mut buffer = vec![0; 128 * 1024];
    loop {
        let count = file.read(&mut buffer).await?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    let actual = format!("{:x}", hasher.finalize());
    Ok(actual
        .eq_ignore_ascii_case(&expected.sha256)
        .then_some(file))
}

#[cfg(windows)]
fn receipt(asset: &str) -> PathBuf {
    crate::utils::platform::echobird_dir()
        .join("self-update")
        .join(format!("{asset}.json"))
}

#[cfg(windows)]
pub(super) fn remember(asset: &str, fingerprint: &MirrorFile) {
    let result = serde_json::to_vec(fingerprint)
        .map_err(|error| error.to_string())
        .and_then(|bytes| super::super::native_account_files::write(&receipt(asset), &bytes));
    if let Err(error) = result {
        log::warn!("[SelfUpdate] Could not save installer verification: {error}");
    }
}

#[cfg(windows)]
pub(super) async fn find(
    dir: &Path,
    asset: &str,
    version: &str,
    platform: &str,
) -> Result<Option<VerifiedInstaller>, String> {
    let paths = candidates(dir, asset).await;
    if paths.is_empty() {
        return Ok(None);
    }
    let report_error = |error| format!("Could not check the downloaded installer: {error}");
    if let Ok(bytes) = tokio::fs::read(receipt(asset)).await {
        if let Ok(expected) = serde_json::from_slice(&bytes) {
            return verify_local(paths, expected).await.map_err(report_error);
        }
    }

    // Files from older versions have no receipt. Confirm them against release
    // metadata before offering to launch anything from Downloads.
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(4))
        .build()
        .map_err(|error| error.to_string())?;
    let website = format!("https://echobird.ai/download/{platform}");
    let expected = match super::sources(&client, version, asset, &website)
        .await
        .into_iter()
        .find_map(|source| source.expected)
    {
        Some(expected) => expected,
        None => {
            let Some(expected) = github_fingerprint(&client, version, asset).await else {
                return Ok(None);
            };
            expected
        }
    };
    verify_local(paths, expected).await.map_err(report_error)
}

#[cfg(windows)]
async fn github_fingerprint(
    client: &reqwest::Client,
    version: &str,
    asset: &str,
) -> Option<MirrorFile> {
    #[derive(serde::Deserialize)]
    struct Release {
        assets: Vec<Asset>,
    }
    #[derive(serde::Deserialize)]
    struct Asset {
        name: String,
        size: u64,
        digest: Option<String>,
    }

    let url = format!("https://api.github.com/repos/edison7009/EchoBird/releases/tags/v{version}");
    let release = client
        .get(url)
        .header("User-Agent", "EchoBird")
        .send()
        .await
        .ok()?
        .error_for_status()
        .ok()?
        .json::<Release>()
        .await
        .ok()?;
    let matched = release
        .assets
        .into_iter()
        .find(|entry| entry.name == asset)?;
    Some(MirrorFile {
        size: matched.size,
        sha256: matched.digest?.strip_prefix("sha256:")?.into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn reuses_only_matching_version_and_verified_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let asset = "EchoBird_6.4.3_Windows_x64-setup.exe";
        let good = dir.path().join(format!(
            "{}-{}.exe",
            asset.strip_suffix(".exe").unwrap(),
            uuid::Uuid::new_v4()
        ));
        tokio::fs::write(&good, b"valid").await.unwrap();
        for filename in [
            asset,
            "EchoBird_6.4.2_Windows_x64-setup.exe",
            ".installer.part",
        ] {
            tokio::fs::write(dir.path().join(filename), b"other")
                .await
                .unwrap();
        }
        let fingerprint = MirrorFile {
            size: 5,
            sha256: format!("{:x}", Sha256::digest(b"valid")),
        };
        let receipt = serde_json::to_vec(&fingerprint).unwrap();
        let matched = verify_local(
            candidates(dir.path(), asset).await,
            serde_json::from_slice(&receipt).unwrap(),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(matched.path, good);
        assert_eq!(matched.fingerprint.sha256, fingerprint.sha256);
        drop(matched);
        tokio::fs::write(&good, b"wrong").await.unwrap();
        assert!(
            verify_local(candidates(dir.path(), asset).await, fingerprint)
                .await
                .unwrap()
                .is_none()
        );
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn reports_a_locked_package_instead_of_requesting_another_copy() {
        use std::os::windows::fs::OpenOptionsExt;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("installer.exe");
        std::fs::write(&path, b"valid").unwrap();
        let locked = std::fs::OpenOptions::new()
            .write(true)
            .share_mode(0)
            .open(&path)
            .unwrap();
        let expected = MirrorFile {
            size: 5,
            sha256: format!("{:x}", Sha256::digest(b"valid")),
        };
        assert!(verify_local(vec![path.clone()], expected.clone())
            .await
            .is_err());
        drop(locked);
        assert!(verify_local(vec![path], expected).await.unwrap().is_some());
    }
}
