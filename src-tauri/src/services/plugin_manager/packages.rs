use super::{
    manifest::{self, Manifest},
    Candidate, PluginManager, Registry, RegistryRelease, MAX_PACKAGE_BYTES,
};
use base64::Engine;
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    fs,
    io::{Cursor, Read},
    path::Path,
    time::Duration,
};
use tokio_util::sync::CancellationToken;

fn copy_tree(
    source: &Path,
    target: &Path,
    budget: &mut (u64, usize),
    depth: usize,
) -> Result<(), String> {
    if depth > 32 {
        return Err("Plugin directory is too deep".into());
    }
    fs::create_dir_all(target).map_err(|e| e.to_string())?;
    for item in fs::read_dir(source).map_err(|e| e.to_string())? {
        let item = item.map_err(|e| e.to_string())?;
        let name = item
            .file_name()
            .into_string()
            .map_err(|_| "Invalid plugin filename")?;
        if [".git", "node_modules"].contains(&name.as_str()) {
            continue;
        }
        if !manifest::relative_path(&name) {
            return Err("Invalid plugin filename".into());
        }
        let kind = item.file_type().map_err(|e| e.to_string())?;
        budget.1 += 1;
        if budget.1 > 2000 || kind.is_symlink() {
            return Err("Plugin contains links or too many files".into());
        }
        if kind.is_dir() {
            copy_tree(&item.path(), &target.join(name), budget, depth + 1)?;
        } else if kind.is_file() {
            let mut bytes = Vec::new();
            fs::File::open(item.path())
                .map_err(|e| e.to_string())?
                .take(MAX_PACKAGE_BYTES - budget.0 + 1)
                .read_to_end(&mut bytes)
                .map_err(|e| e.to_string())?;
            budget.0 += bytes.len() as u64;
            if budget.0 > MAX_PACKAGE_BYTES {
                return Err("Plugin exceeds 20 MiB".into());
            }
            fs::write(target.join(name), bytes).map_err(|e| e.to_string())?;
        } else {
            return Err("Plugin contains a special file".into());
        }
    }
    Ok(())
}

pub(super) fn extract(bytes: &[u8], target: &Path) -> Result<(), String> {
    if bytes.len() as u64 > MAX_PACKAGE_BYTES {
        return Err("Plugin archive exceeds 20 MiB".into());
    }
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| e.to_string())?;
    if archive.len() > 2000 {
        return Err("Too many archive entries".into());
    }
    let mut seen = HashSet::new();
    let mut items = HashSet::new();
    let mut size = 0;
    for index in 0..archive.len() {
        let mut file = archive.by_index(index).map_err(|e| e.to_string())?;
        let raw_name = file.name().map_err(|e| e.to_string())?.into_owned();
        let name = raw_name.trim_end_matches('/');
        if !manifest::relative_path(name)
            || !seen.insert(name.to_lowercase())
            || file.unix_mode().is_some_and(|m| m & 0o170000 == 0o120000)
        {
            return Err("Unsafe or duplicate archive path".into());
        }
        let mut prefix = String::new();
        for (depth, part) in name.split('/').enumerate() {
            if depth >= 32 {
                return Err("Plugin directory is too deep".into());
            }
            if !prefix.is_empty() {
                prefix.push('/');
            }
            prefix.push_str(part);
            items.insert(prefix.to_lowercase());
            if items.len() > 2000 {
                return Err("Plugin contains too many files or directories".into());
            }
        }
        let destination = target.join(name);
        if file.is_dir() {
            fs::create_dir_all(destination).map_err(|e| e.to_string())?;
            continue;
        }
        if size + file.size() > MAX_PACKAGE_BYTES {
            return Err("Expanded plugin exceeds 20 MiB".into());
        }
        fs::create_dir_all(destination.parent().unwrap()).map_err(|e| e.to_string())?;
        let mut content = Vec::new();
        file.by_ref()
            .take(MAX_PACKAGE_BYTES + 1)
            .read_to_end(&mut content)
            .map_err(|e| e.to_string())?;
        size += content.len() as u64;
        if size > MAX_PACKAGE_BYTES {
            return Err("Expanded plugin exceeds 20 MiB".into());
        }
        fs::write(destination, content).map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub(super) fn verified_archive(bytes: &[u8], digest: &str, target: &Path) -> Result<(), String> {
    if hex::encode(Sha256::digest(bytes)) != digest {
        return Err("Plugin package SHA-256 mismatch".into());
    }
    extract(bytes, target)
}

pub(super) fn icon(root: &Path, manifest: &Manifest) -> Option<String> {
    let path = manifest.icon.as_ref()?;
    let mime = match Path::new(path).extension()?.to_str()? {
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "webp" => "image/webp",
        _ => return None,
    };
    let bytes = fs::read(root.join(path)).ok()?;
    if bytes.len() > 128 * 1024 {
        return None;
    }
    Some(format!(
        "data:{mime};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    ))
}

pub async fn download(url: &str, limit: u64) -> Result<Vec<u8>, String> {
    if !manifest::public_https(url) {
        return Err("Expected a public HTTPS URL".into());
    }
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .user_agent("EchoBird-Plugin-Hub")
        .build()
        .map_err(|e| e.to_string())?;
    let mut response = client
        .get(url)
        .send()
        .await
        .map_err(|e| error_details(&e))?
        .error_for_status()
        .map_err(|e| error_details(&e))?;
    if response
        .content_length()
        .is_some_and(|length| length > limit)
    {
        return Err("Download exceeds limit".into());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|e| error_details(&e))? {
        if bytes.len() as u64 + chunk.len() as u64 > limit {
            return Err("Download exceeds limit".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

pub(super) fn error_details(error: &dyn std::error::Error) -> String {
    let mut details = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        let cause_text = cause.to_string();
        if !details.contains(&cause_text) {
            details.push_str(": ");
            details.push_str(&cause_text);
        }
        source = cause.source();
    }
    details
}

pub(super) async fn download_with_fallback(
    url: &str,
    fallback: Option<&str>,
    limit: u64,
) -> Result<Vec<u8>, String> {
    let primary_error = match download(url, limit).await {
        Ok(bytes) => return Ok(bytes),
        Err(error) => error,
    };
    match fallback {
        Some(fallback) => download(fallback, limit).await.map_err(|fallback_error| {
            format!(
                "GitCode: {primary_error}
GitHub: {fallback_error}"
            )
        }),
        None => Err(primary_error),
    }
}

pub fn bundled_registry() -> Result<Registry, String> {
    Registry::parse(include_bytes!("../../../../src/data/pluginIndex.json"))
}

pub async fn registry(region: &str, refresh: bool) -> Result<Registry, String> {
    let url = match region {
        "international" => super::INTERNATIONAL_INDEX,
        "china" => super::CHINA_INDEX,
        _ => return Err("Unknown plugin region".into()),
    };
    if refresh {
        let fallback = (region == "china").then_some(super::INTERNATIONAL_INDEX);
        let bytes = download_with_fallback(url, fallback, 1024 * 1024).await?;
        Registry::parse(&bytes)
    } else {
        bundled_registry()
    }
}

async fn git(args: &[&str]) -> Result<std::process::Output, String> {
    let mut command = crate::utils::process::async_command("git");
    command
        .args([
            "-c",
            "protocol.allow=never",
            "-c",
            "protocol.https.allow=always",
            "-c",
            "credential.helper=",
        ])
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env(
            "GIT_CONFIG_GLOBAL",
            if cfg!(windows) { "NUL" } else { "/dev/null" },
        )
        .env_remove("GIT_CONFIG_COUNT")
        .env_remove("GIT_CONFIG_PARAMETERS")
        .kill_on_drop(true);
    let output = tokio::time::timeout(Duration::from_secs(120), command.output())
        .await
        .map_err(|_| "Git import timed out")?
        .map_err(|_| "Git is required to import a repository")?;
    if !output.status.success() {
        return Err("Git could not read the public repository".into());
    }
    Ok(output)
}

impl PluginManager {
    pub async fn preview(
        &self,
        kind: &str,
        value: &str,
        region: &str,
        cancelled: CancellationToken,
    ) -> Result<Candidate, String> {
        struct Staging<'a> {
            manager: &'a PluginManager,
            path: std::path::PathBuf,
            retained: bool,
        }
        impl Drop for Staging<'_> {
            fn drop(&mut self) {
                if !self.retained {
                    if let Err(error) = self.manager.remove_owned(&self.path) {
                        log::warn!("Could not clear a cancelled plugin preview: {error}");
                    }
                }
            }
        }
        if cancelled.is_cancelled() {
            return Err("Developer preview is paused".into());
        }
        let token = uuid::Uuid::new_v4().to_string();
        let stage = self.staging(&token);
        let mut staging = Staging {
            manager: self,
            path: stage.clone(),
            retained: false,
        };
        fs::create_dir_all(stage.join("package")).map_err(|e| e.to_string())?;
        let result = tokio::select! {
            biased;
            _ = cancelled.cancelled() => return Err("Developer preview is paused".into()),
            result = self.prepare(&stage, kind, value, region) => result,
        };
        match result {
            Ok((source, revision, expected)) => {
                let manifest = match Manifest::read(&stage.join("package")) {
                    Ok(manifest) => manifest,
                    Err(error) => {
                        let _ = self.remove_owned(&stage);
                        return Err(error);
                    }
                };
                if expected.is_some_and(|release| release.manifest != manifest) {
                    let _ = self.remove_owned(&stage);
                    return Err("Downloaded manifest differs from the reviewed release".into());
                }
                let candidate = Candidate {
                    token: token.clone(),
                    manifest,
                    source,
                    revision,
                };
                let mut candidates = self.candidates.lock().unwrap();
                if cancelled.is_cancelled() {
                    return Err("Developer preview is paused".into());
                }
                candidates.insert(token, candidate.clone());
                staging.retained = true;
                Ok(candidate)
            }
            Err(error) => {
                let _ = self.remove_owned(&stage);
                Err(error)
            }
        }
    }

    async fn prepare(
        &self,
        stage: &Path,
        kind: &str,
        value: &str,
        region: &str,
    ) -> Result<(String, Option<String>, Option<RegistryRelease>), String> {
        let target = stage.join("package");
        match kind {
            "directory" => {
                let source = Path::new(value).canonicalize().map_err(|e| e.to_string())?;
                let storage = self.root.canonicalize().map_err(|e| e.to_string())?;
                if source.starts_with(&storage) || storage.starts_with(&source) {
                    return Err("Choose a source directory outside plugin storage".into());
                }
                copy_tree(&source, &target, &mut (0, 0), 0)?;
                Ok((value.to_string(), None, None))
            }
            "repository" => {
                let mut url = url::Url::parse(value).map_err(|_| "Invalid repository URL")?;
                let subdirectory = url.fragment().unwrap_or("").to_string();
                url.set_fragment(None);
                if !manifest::public_https(url.as_str())
                    || (!subdirectory.is_empty() && !manifest::relative_path(&subdirectory))
                {
                    return Err(
                        "Use a public HTTPS repository URL, optionally #path/to/plugin".into(),
                    );
                }
                let bare = stage.join("source.git").to_string_lossy().into_owned();
                git(&[
                    "clone",
                    "--bare",
                    "--depth=1",
                    "--single-branch",
                    "--template=",
                    "--",
                    url.as_str(),
                    &bare,
                ])
                .await?;
                let revision =
                    String::from_utf8(git(&["-C", &bare, "rev-parse", "HEAD"]).await?.stdout)
                        .map_err(|e| e.to_string())?
                        .trim()
                        .to_string();
                if !super::hex_value(&revision, &[40, 64]) {
                    return Err("Invalid repository revision".into());
                }
                let tree = if subdirectory.is_empty() {
                    "HEAD".into()
                } else {
                    format!("HEAD:{subdirectory}")
                };
                let archive = stage.join("source.zip").to_string_lossy().into_owned();
                git(&[
                    "-C",
                    &bare,
                    "archive",
                    "--format=zip",
                    "--output",
                    &archive,
                    &tree,
                ])
                .await?;
                if fs::metadata(&archive).map_err(|e| e.to_string())?.len() > MAX_PACKAGE_BYTES {
                    return Err("Plugin archive exceeds 20 MiB".into());
                }
                extract(&fs::read(archive).map_err(|e| e.to_string())?, &target)?;
                Ok((value.to_string(), Some(revision), None))
            }
            "catalog" => {
                let index = registry(region, true).await?;
                let plugin = index
                    .plugins
                    .into_iter()
                    .find(|p| p.id == value)
                    .ok_or("Plugin is not in the official index")?;
                let release = plugin
                    .releases
                    .into_iter()
                    .next()
                    .ok_or("No compatible plugin release")?;
                let fallback = (region == "china")
                    .then(|| release.downloads.get("international"))
                    .flatten();
                let bytes = download_with_fallback(
                    release
                        .downloads
                        .get(region)
                        .ok_or("Missing regional download")?,
                    fallback.map(String::as_str),
                    MAX_PACKAGE_BYTES,
                )
                .await?;
                verified_archive(&bytes, &release.sha256, &target)?;
                Ok((
                    plugin.repository,
                    Some(release.commit.clone()),
                    Some(release),
                ))
            }
            _ => Err("Unknown plugin source".into()),
        }
    }
}
