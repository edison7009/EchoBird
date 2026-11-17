use serde_json::Value;
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::Arc,
};

pub(super) struct Archive {
    pub path: PathBuf,
    files: Value,
    start: u64,
    length: u64,
    modified: std::time::SystemTime,
    pub build_id: String,
    pub cowork_prompt: Option<String>,
    unpacked: bool,
}

pub(super) fn installed() -> Result<Option<Arc<Archive>>, String> {
    let Some(executable) = super::super::tool_manager::get_tool_exe_path("claudedesktop") else {
        return Ok(None);
    };
    let executable = std::fs::canonicalize(executable).map_err(|error| error.to_string())?;
    let directory = executable
        .parent()
        .ok_or("Claude executable has no directory")?;
    #[cfg(target_os = "macos")]
    let resources = directory.join("../Resources");
    #[cfg(not(target_os = "macos"))]
    let resources = directory.join("resources");
    let path = resources.join("ion-dist.asar");
    let directory = resources.join("ion-dist");
    let mut archive = if path.is_file() {
        Archive::open(path)?
    } else if directory.is_dir() {
        Archive::open_directory(directory)?
    } else {
        return Ok(None);
    };
    archive.cowork_prompt = match bundled_prompt(&resources) {
        Ok(prompt) => prompt,
        Err(error) => {
            log::warn!("[ClaudeLocal] Cannot read bundled workspace instructions: {error}");
            None
        }
    };
    Ok(Some(Arc::new(archive)))
}

impl Archive {
    #[cfg(test)]
    pub(super) fn prompt_fixture(prompt: &str) -> Self {
        Self {
            path: PathBuf::from("fixture-native-resources"),
            files: Value::Null,
            start: 0,
            length: 0,
            modified: std::time::UNIX_EPOCH,
            build_id: "fixture-build".into(),
            unpacked: false,
            cowork_prompt: Some(prompt.into()),
        }
    }
    fn open(path: PathBuf) -> Result<Self, String> {
        let mut archive = Self::open_container(path)?;
        let html = archive
            .read("/index.html")?
            .ok_or("Claude Desktop shell is missing")?;
        archive.build_id =
            build_id(std::str::from_utf8(&html).map_err(|error| error.to_string())?)?;
        Ok(archive)
    }

    fn open_container(path: PathBuf) -> Result<Self, String> {
        let mut file = File::open(&path).map_err(|error| error.to_string())?;
        let metadata = file.metadata().map_err(|error| error.to_string())?;
        let length = metadata.len();
        let modified = metadata.modified().map_err(|error| error.to_string())?;
        let mut prefix = [0u8; 16];
        file.read_exact(&mut prefix)
            .map_err(|error| error.to_string())?;
        let header_size = u32::from_le_bytes(prefix[4..8].try_into().unwrap()) as u64;
        let json_size = u32::from_le_bytes(prefix[12..16].try_into().unwrap()) as u64;
        let start = 8 + header_size;
        if !(8..=64 * 1024 * 1024).contains(&header_size)
            || json_size > header_size - 8
            || start > length
        {
            return Err("Invalid Claude Desktop asset archive header".into());
        }
        let mut json = vec![0u8; json_size as usize];
        file.read_exact(&mut json)
            .map_err(|error| error.to_string())?;
        let tree: Value = serde_json::from_slice(&json).map_err(|error| error.to_string())?;
        Ok(Self {
            path,
            files: tree["files"].clone(),
            start,
            length,
            modified,
            build_id: String::new(),
            cowork_prompt: None,
            unpacked: false,
        })
    }

    fn open_directory(path: PathBuf) -> Result<Self, String> {
        let path = std::fs::canonicalize(path).map_err(|error| error.to_string())?;
        let metadata =
            std::fs::metadata(path.join("index.html")).map_err(|error| error.to_string())?;
        let mut archive = Self {
            path,
            files: Value::Null,
            start: 0,
            length: metadata.len(),
            modified: metadata.modified().map_err(|error| error.to_string())?,
            build_id: String::new(),
            cowork_prompt: None,
            unpacked: true,
        };
        let html = archive
            .read("/index.html")?
            .ok_or("Claude Desktop shell is missing")?;
        archive.build_id =
            build_id(std::str::from_utf8(&html).map_err(|error| error.to_string())?)?;
        Ok(archive)
    }

    pub fn version(&self) -> (&Path, u64, std::time::SystemTime, Option<&str>) {
        (
            &self.path,
            self.length,
            self.modified,
            self.cowork_prompt.as_deref(),
        )
    }

    fn read_directory(&self, member: &str) -> Result<Option<Vec<u8>>, String> {
        let path = match std::fs::canonicalize(self.path.join(member)) {
            Ok(path) => path,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.to_string()),
        };
        if !path.starts_with(&self.path) {
            return Ok(None);
        }
        let file = File::open(path).map_err(|error| error.to_string())?;
        let metadata = file.metadata().map_err(|error| error.to_string())?;
        if !metadata.is_file() || metadata.len() > 64 * 1024 * 1024 {
            return Ok(None);
        }
        let mut bytes = Vec::new();
        file.take(64 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| error.to_string())?;
        if bytes.len() > 64 * 1024 * 1024 {
            return Err("Claude asset is too large".into());
        }
        Ok(Some(bytes))
    }

    pub fn read(&self, path: &str) -> Result<Option<Vec<u8>>, String> {
        let Some(member) = path.strip_prefix('/') else {
            return Ok(None);
        };
        if member.contains(['\\', '%', '\0'])
            || member
                .split('/')
                .any(|part| part.is_empty() || matches!(part, "." | ".."))
        {
            return Ok(None);
        }
        let public = ["assets/", "audio/", "images/", "i18n/"]
            .iter()
            .any(|prefix| member.starts_with(prefix))
            || matches!(
                member,
                "index.html"
                    | "client-shell-v1.html"
                    | "frame-connect.html"
                    | "frame-manage.html"
                    | "frame-shell.html"
                    | "favicon.ico"
                    | "favicon-internal.ico"
                    | "favicon-staging.ico"
                    | "robots.txt"
                    | "scc.json"
                    | "third-party-notices.txt"
            );
        if !public {
            return Ok(None);
        }
        self.read_member(member)
    }

    fn read_member(&self, member: &str) -> Result<Option<Vec<u8>>, String> {
        if self.unpacked {
            return self.read_directory(member);
        }
        let mut parts = member.split('/');
        let mut entry = &self.files[parts.next().unwrap()];
        for part in parts {
            entry = &entry["files"][part];
        }
        if entry.is_null() || entry.get("link").is_some() || entry["unpacked"] == true {
            return Ok(None);
        }
        let size = entry["size"]
            .as_u64()
            .filter(|size| *size <= 64 * 1024 * 1024)
            .ok_or("Invalid Claude asset length")?;
        let offset = entry["offset"]
            .as_str()
            .and_then(|value| value.parse::<u64>().ok())
            .and_then(|offset| self.start.checked_add(offset))
            .filter(|offset| {
                offset
                    .checked_add(size)
                    .is_some_and(|end| end <= self.length)
            })
            .ok_or("Invalid Claude asset offset")?;
        let mut file = File::open(&self.path).map_err(|error| error.to_string())?;
        file.seek(SeekFrom::Start(offset))
            .map_err(|error| error.to_string())?;
        let mut bytes = vec![0u8; size as usize];
        file.read_exact(&mut bytes)
            .map_err(|error| error.to_string())?;
        Ok(Some(bytes))
    }
}

fn script_members(files: &Value, prefix: &str, result: &mut Vec<String>) {
    let Some(files) = files.as_object() else {
        return;
    };
    for (name, entry) in files {
        let path = format!("{prefix}{name}");
        if entry.get("files").is_some() {
            if name != "node_modules" {
                script_members(&entry["files"], &format!("{path}/"), result);
            }
        } else if matches!(
            Path::new(name).extension().and_then(|value| value.to_str()),
            Some("js" | "cjs" | "mjs")
        ) {
            result.push(path);
        }
    }
}

fn bundled_prompt(resources: &Path) -> Result<Option<String>, String> {
    let path = resources.join("app.asar");
    if path.is_file() {
        let archive = Archive::open_container(path)?;
        let mut scripts = Vec::new();
        script_members(&archive.files, "", &mut scripts);
        for path in scripts {
            let Some(bytes) = archive.read_member(&path)? else {
                continue;
            };
            let Ok(source) = std::str::from_utf8(&bytes) else {
                continue;
            };
            if !source.contains("DESKTOP_3P_BASE_PROMPT") {
                continue;
            }
            if let Some(prompt) = super::workspace_prompt::from_module(source) {
                return Ok(Some(prompt));
            }
        }
        return Ok(None);
    }
    let root = resources.join("app");
    if !root.is_dir() {
        return Ok(None);
    }
    let canonical = std::fs::canonicalize(&root).map_err(|error| error.to_string())?;
    let mut directories = vec![canonical.clone()];
    while let Some(directory) = directories.pop() {
        for entry in std::fs::read_dir(directory).map_err(|error| error.to_string())? {
            let entry = entry.map_err(|error| error.to_string())?;
            let kind = entry.file_type().map_err(|error| error.to_string())?;
            if kind.is_dir() && entry.file_name() != "node_modules" {
                directories.push(entry.path());
            }
            if kind.is_file()
                && matches!(
                    entry.path().extension().and_then(|value| value.to_str()),
                    Some("js" | "cjs" | "mjs")
                )
            {
                let file = File::open(entry.path()).map_err(|error| error.to_string())?;
                let mut bytes = Vec::new();
                file.take(64 * 1024 * 1024 + 1)
                    .read_to_end(&mut bytes)
                    .map_err(|error| error.to_string())?;
                if bytes.len() > 64 * 1024 * 1024 {
                    continue;
                }
                let Ok(source) = std::str::from_utf8(&bytes) else {
                    continue;
                };
                if !source.contains("DESKTOP_3P_BASE_PROMPT") {
                    continue;
                }
                if let Some(prompt) = super::workspace_prompt::from_module(source) {
                    return Ok(Some(prompt));
                }
            }
        }
    }
    Ok(None)
}

fn build_id(html: &str) -> Result<String, String> {
    html.split_once("data-build-id=\"")
        .and_then(|(_, rest)| rest.split_once('"'))
        .map(|(id, _)| id.to_owned())
        .filter(|id| !id.is_empty() && id.len() <= 128)
        .ok_or_else(|| "Claude Desktop shell build ID is missing".into())
}

pub(super) fn content_type(path: &str) -> &'static str {
    match Path::new(path).extension().and_then(|value| value.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("js" | "mjs") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("json") => "application/json; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        Some("gif") => "image/gif",
        Some("ico") => "image/x-icon",
        Some("woff") => "font/woff",
        Some("woff2") => "font/woff2",
        Some("wasm") => "application/wasm",
        Some("mp3") => "audio/mpeg",
        Some("wav") => "audio/wav",
        Some("mp4") => "video/mp4",
        Some("txt") => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    #[ignore = "read-only verification against an installed Claude Desktop package"]
    fn installed_workspace_prompt_is_decoded_without_executing_vendor_code() {
        let assets = installed()
            .unwrap()
            .expect("Claude Desktop must be installed");
        let prompt = assets
            .cowork_prompt
            .as_deref()
            .expect("Bundled workspace prompt must be readable");
        assert!(prompt.len() > 1000);
        assert!(prompt.contains("{{modelName}}"));
        let config = super::super::desktop::prompt_configuration(Some(prompt));
        assert!(!config["cowork_system_prompt"]["value"]["prompt"]
            .as_str()
            .unwrap()
            .trim()
            .is_empty());
        eprintln!(
            "Installed workspace instructions decoded: {} bytes",
            prompt.len()
        );
    }

    #[test]
    fn unpacked_resources_use_the_same_public_member_rules() {
        let root = std::env::temp_dir().join(format!("echobird-unpacked-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("assets/v1")).unwrap();
        std::fs::write(
            root.join("index.html"),
            b"<html data-build-id=\"fixture\"></html>",
        )
        .unwrap();
        std::fs::write(root.join("assets/v1/test.js"), b"export default 1").unwrap();
        std::fs::write(root.join("secret.json"), b"private").unwrap();
        let archive = Archive::open_directory(root.clone()).unwrap();
        assert_eq!(archive.build_id, "fixture");
        assert_eq!(
            archive.read("/assets/v1/test.js").unwrap().unwrap(),
            b"export default 1"
        );
        for path in [
            "/secret.json",
            "/assets/../secret.json",
            "/assets/%2e%2e/secret.json",
            "/missing.js",
        ] {
            assert!(archive.read(path).unwrap().is_none());
        }
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(root.parent().unwrap(), root.join("assets/outside"))
                .unwrap();
            assert!(archive
                .read("/assets/outside/secret.json")
                .unwrap()
                .is_none());
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn archive_serves_public_resources_and_rejects_non_public_or_out_of_bounds_entries() {
        let directory =
            std::env::temp_dir().join(format!("claude-assets-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("ion-dist.asar");
        let html = b"<html data-build-id=\"fixture-build\"></html>";
        let tree = json!({"files":{
            "index.html":{"size":html.len(),"offset":"0"},
            "assets":{"files":{"app.js":{"size":2,"offset":html.len().to_string()},"invalid.js":{"size":4,"offset":"999999"}}},
            "private.json":{"size":2,"offset":html.len().to_string()}
        }});
        let json = serde_json::to_vec(&tree).unwrap();
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&4u32.to_le_bytes());
        bytes.extend_from_slice(&((json.len() + 8) as u32).to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&(json.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&json);
        bytes.extend_from_slice(html);
        bytes.extend_from_slice(b"js");
        std::fs::write(&path, bytes).unwrap();
        let archive = Archive::open(path.clone()).unwrap();
        assert_eq!(
            archive.version(),
            Archive::open(path.clone()).unwrap().version()
        );
        let mut updated = std::fs::read(&path).unwrap();
        updated.extend_from_slice(b"new-release");
        std::fs::write(&path, &updated).unwrap();
        assert_ne!(archive.version(), Archive::open(path).unwrap().version());
        assert_eq!(archive.build_id, "fixture-build");
        assert_eq!(archive.read("/index.html").unwrap().unwrap(), html);
        assert_eq!(archive.read("/assets/app.js").unwrap().unwrap(), b"js");
        for path in [
            "/private.json",
            "/assets/../private.json",
            "/assets/%2e%2e/private.json",
            "/assets\\app.js",
        ] {
            assert!(archive.read(path).unwrap().is_none());
        }
        assert!(archive.read("/assets/invalid.js").is_err());
        std::fs::remove_dir_all(directory).unwrap();
    }
}
