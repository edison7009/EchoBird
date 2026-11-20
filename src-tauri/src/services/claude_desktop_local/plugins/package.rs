use super::{fetch, ArchiveError, MAX_PACKAGE_BYTES};
use serde_json::Value;
use std::collections::HashSet;
use std::io::{Cursor, Read, Write};
use zip::{write::SimpleFileOptions, ZipArchive, ZipWriter};

pub(super) struct Repository {
    pub owner: String,
    pub name: String,
    pub commit: String,
    pub directory: Vec<String>,
}

pub(super) fn name_component(value: &str) -> bool {
    !value.is_empty()
        && !matches!(value, "." | "..")
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

impl Repository {
    pub fn from_entry(entry: &Value) -> Option<Self> {
        let source = &entry["source"];
        if source["type"] != "directory" {
            return None;
        }
        let repository = &source["repository"];
        let url = url::Url::parse(repository["url"].as_str()?).ok()?;
        if url.scheme() != "https"
            || url.host_str() != Some("github.com")
            || !url.username().is_empty()
            || url.password().is_some()
            || url.port().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return None;
        }
        let mut segments = url.path().trim_matches('/').split('/');
        let owner = segments.next()?;
        let name = segments.next()?;
        let name = name.strip_suffix(".git").unwrap_or(name);
        if segments.next().is_some() || !name_component(owner) || !name_component(name) {
            return None;
        }
        let commit = repository["reviewed_commit"].as_str()?;
        if commit.len() != 40 || !commit.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return None;
        }
        let directory = match repository.get("path").filter(|value| !value.is_null()) {
            None => "",
            Some(value) => value.as_str()?.trim_matches('/'),
        };
        let directory: Vec<_> = directory
            .split('/')
            .filter(|_| !directory.is_empty())
            .map(str::to_owned)
            .collect();
        if !directory.iter().all(|part| name_component(part)) {
            return None;
        }
        Some(Self {
            owner: owner.into(),
            name: name.into(),
            commit: commit.to_ascii_lowercase(),
            directory,
        })
    }

    pub fn url(&self) -> String {
        format!("https://github.com/{}/{}", self.owner, self.name)
    }

    pub async fn download(self, client: &reqwest::Client) -> Result<Vec<u8>, ArchiveError> {
        let url = format!(
            "https://codeload.github.com/{}/{}/zip/{}",
            self.owner, self.name, self.commit
        );
        let bytes = fetch(client, url, MAX_PACKAGE_BYTES)
            .await?
            .ok_or(ArchiveError::Upstream)?;
        tokio::task::spawn_blocking(move || build_package(&bytes, &self.directory))
            .await
            .map_err(|_| ArchiveError::Invalid)?
    }
}

fn safe_path_component(part: &str) -> bool {
    if part.is_empty()
        || matches!(part, "." | "..")
        || part.ends_with(['.', ' '])
        || part.contains(['\\', ':', '<', '>', '"', '|', '?', '*'])
        || part.chars().any(char::is_control)
    {
        return false;
    }
    let stem = part.split('.').next().unwrap_or("").to_ascii_uppercase();
    !matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        && !stem
            .strip_prefix("COM")
            .or_else(|| stem.strip_prefix("LPT"))
            .is_some_and(|suffix| {
                matches!(suffix, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9")
            })
}

pub(super) fn build_package(bytes: &[u8], directory: &[String]) -> Result<Vec<u8>, ArchiveError> {
    let mut source = ZipArchive::new(Cursor::new(bytes)).map_err(|_| ArchiveError::Invalid)?;
    if source.len() > 8192 {
        return Err(ArchiveError::Invalid);
    }
    let mut result = ZipWriter::new(Cursor::new(Vec::new()));
    let mut repository_root = None;
    let mut files = HashSet::new();
    let mut bytes_written = 0;
    let mut manifest_found = false;
    for index in 0..source.len() {
        let file = source.by_index(index).map_err(|_| ArchiveError::Invalid)?;
        if file.is_dir() {
            continue;
        }
        let name = std::str::from_utf8(file.name_raw()).map_err(|_| ArchiveError::Invalid)?;
        let mut components = name.split('/');
        let root = components.next().ok_or(ArchiveError::Invalid)?;
        if !safe_path_component(root) {
            return Err(ArchiveError::Invalid);
        }
        if repository_root.get_or_insert_with(|| root.to_owned()) != root {
            return Err(ArchiveError::Invalid);
        }
        let path: Vec<_> = components.collect();
        let Some(inner) = path.get(directory.len()..).filter(|_| {
            path.iter()
                .zip(directory)
                .all(|(part, expected)| *part == expected)
        }) else {
            continue;
        };
        if inner.is_empty() {
            continue;
        }
        let mode = file.unix_mode().unwrap_or(0o644);
        if !inner.iter().all(|part| safe_path_component(part))
            || !matches!(mode & 0o170000, 0 | 0o100000)
        {
            return Err(ArchiveError::Invalid);
        }
        let inner = inner.join("/");
        let file_key = inner.to_lowercase();
        if inner.len() > 512
            || file_key
                .match_indices('/')
                .any(|(offset, _)| files.contains(&file_key[..offset]))
            || files.iter().any(|name: &String| {
                name.strip_prefix(&file_key)
                    .is_some_and(|suffix| suffix.starts_with('/'))
            })
            || !files.insert(file_key)
        {
            return Err(ArchiveError::Invalid);
        }
        let remaining = MAX_PACKAGE_BYTES - bytes_written;
        if file.size() > remaining as u64 {
            return Err(ArchiveError::Invalid);
        }
        let permissions = if mode & 0o111 == 0 { 0o644 } else { 0o755 };
        let options = SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .unix_permissions(permissions);
        result
            .start_file(&inner, options)
            .map_err(|_| ArchiveError::Invalid)?;
        if inner == ".claude-plugin/plugin.json" {
            if file.size() > 256 * 1024 {
                return Err(ArchiveError::Invalid);
            }
            let mut manifest = Vec::new();
            file.take(remaining as u64 + 1)
                .read_to_end(&mut manifest)
                .map_err(|_| ArchiveError::Invalid)?;
            let parsed: Value =
                serde_json::from_slice(&manifest).map_err(|_| ArchiveError::Invalid)?;
            if !parsed["name"].as_str().is_some_and(name_component) || manifest.len() > remaining {
                return Err(ArchiveError::Invalid);
            }
            result
                .write_all(&manifest)
                .map_err(|_| ArchiveError::Invalid)?;
            bytes_written += manifest.len();
            manifest_found = true;
        } else {
            let count = std::io::copy(&mut file.take(remaining as u64 + 1), &mut result)
                .map_err(|_| ArchiveError::Invalid)?;
            if count > remaining as u64 {
                return Err(ArchiveError::Invalid);
            }
            bytes_written += count as usize;
        }
    }
    if !manifest_found {
        return Err(ArchiveError::Invalid);
    }
    let bytes = result
        .finish()
        .map_err(|_| ArchiveError::Invalid)?
        .into_inner();
    (bytes.len() <= MAX_PACKAGE_BYTES)
        .then_some(bytes)
        .ok_or(ArchiveError::Invalid)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn archive(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut archive = ZipWriter::new(Cursor::new(Vec::new()));
        for (name, bytes) in entries {
            archive
                .start_file(*name, SimpleFileOptions::default())
                .unwrap();
            archive.write_all(bytes).unwrap();
        }
        archive.finish().unwrap().into_inner()
    }

    #[test]
    fn publishes_only_the_reviewed_subdirectory_and_requires_a_manifest() {
        let manifest = br#"{"name":"fixture"}"#;
        let source = archive(&[
            (
                "repo-hash/plugins/example/.claude-plugin/plugin.json",
                manifest,
            ),
            (
                "repo-hash/plugins/example/skills/demo/SKILL.md",
                b"Fixture skill",
            ),
            (
                "repo-hash/plugins/other/instructions.txt",
                b"Another plugin",
            ),
        ]);
        let zip = build_package(&source, &["plugins".into(), "example".into()]).unwrap();
        let zip = ZipArchive::new(Cursor::new(zip)).unwrap();
        let mut names: Vec<_> = zip.file_names().map(Result::unwrap).collect();
        names.sort();
        assert_eq!(
            names,
            [".claude-plugin/plugin.json", "skills/demo/SKILL.md"]
        );
        assert!(build_package(&source, &["plugins".into(), "other".into()]).is_err());
    }

    #[test]
    fn refuses_unsafe_or_overlapping_paths_in_source_archives() {
        let manifest = br#"{"name":"fixture"}"#;
        for invalid in [
            "repo/../outside",
            "repo/skills/NUL.txt",
            "repo/skills/demo/../example",
            "repo/Skills/demo",
            "repo/skills/demo/child.md",
        ] {
            let zip = archive(&[
                ("repo/.claude-plugin/plugin.json", manifest),
                ("repo/skills/demo", b"existing"),
                (invalid, b"not safe to publish"),
            ]);
            assert!(build_package(&zip, &[]).is_err(), "{invalid}");
        }
    }
}
