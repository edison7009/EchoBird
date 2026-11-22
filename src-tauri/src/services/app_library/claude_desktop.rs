use super::{children, markdown_field, real_directory, Entry};
use crate::services::{
    claude_desktop_accounts,
    native_account_files::{self, DirectoryLock},
    tool_config_manager::claudedesktop,
};
use chrono::{SecondsFormat, Utc};
use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
};

pub(super) fn data_directory() -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os("CLAUDE_USER_DATA_DIR").filter(|path| !path.is_empty()) {
        return Ok(path.into());
    }
    let official = claude_desktop_accounts::data_dir().map_err(|_| "unsupported")?;
    let config = native_account_files::object(&official.join("claude_desktop_config.json"));
    if config.is_ok_and(|config| config["deploymentMode"] == "3p") {
        return claudedesktop::resolve_claudedesktop_paths()
            .and_then(|layout| layout.cfg_threep.parent().map(Path::to_path_buf))
            .ok_or_else(|| "unsupported".into());
    }
    Ok(official)
}

fn profiles(directory: &Path) -> Result<Vec<PathBuf>, String> {
    let config =
        native_account_files::object(&claude_desktop_accounts::native_config_path(directory))
            .map_err(|_| "readFailed")?;
    let Some(account) = config["lastKnownAccountUuid"]
        .as_str()
        .filter(|value| value.len() == 36 && uuid::Uuid::parse_str(value).is_ok())
    else {
        return Ok(Vec::new());
    };
    let session_root = directory.join("local-agent-mode-sessions");
    let skill_root = session_root.join("skills-plugin");
    if !real_directory(&session_root) || !real_directory(&skill_root) {
        return Ok(Vec::new());
    }
    let mut profiles = Vec::new();
    for organization in children(&skill_root)? {
        if !real_directory(&organization) {
            continue;
        }
        let profile = organization.join(account);
        if real_directory(&profile) && regular_file(&profile.join("manifest.json")) {
            profiles.push(profile);
        }
    }
    Ok(profiles)
}

fn regular_file(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|metadata| metadata.is_file())
}

fn parse_manifest(raw: &[u8]) -> Result<Value, &'static str> {
    let value: Value = serde_json::from_slice(raw).map_err(|_| "invalidConfig")?;
    if !value["skills"].is_array() {
        return Err("invalidConfig");
    }
    Ok(value)
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && !id.starts_with('.')
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
}

pub(super) fn skills(directory: &Path) -> Result<Vec<Entry>, String> {
    let mut entries = Vec::new();
    for profile in profiles(directory)? {
        let root = profile.join("skills");
        if !real_directory(&root) {
            continue;
        }
        let raw = fs::read(profile.join("manifest.json")).map_err(|_| "readFailed")?;
        let manifest = parse_manifest(&raw)?;
        let scope = profile
            .strip_prefix(directory.join("local-agent-mode-sessions/skills-plugin"))
            .map_err(|_| "readFailed")?;
        for skill in manifest["skills"].as_array().ok_or("invalidConfig")? {
            let Some(id) = skill["skillId"].as_str().filter(|id| valid_id(id)) else {
                continue;
            };
            let path = root.join(id);
            if !real_directory(&path) || !regular_file(&path.join("SKILL.md")) {
                continue;
            }
            let mut entry = Entry::new(skill["name"].as_str().unwrap_or(id).into(), Some(path));
            entry.id = format!("{}/{id}", scope.display());
            entry.description = skill["description"].as_str().map(str::to_owned);
            entry.enabled = skill["enabled"].as_bool();
            entry.can_copy = true;
            entries.push(entry);
        }
    }
    Ok(entries)
}

pub(super) fn copy_skill(directory: &Path, source: &Path, name: &str) -> Result<(), &'static str> {
    let profiles = profiles(directory).map_err(|_| "readFailed")?;
    let profile = match profiles.as_slice() {
        [] => return Err("coworkProfileMissing"),
        [profile] => profile,
        _ => return Err("coworkProfileAmbiguous"),
    };
    let id = source
        .file_name()
        .and_then(|id| id.to_str())
        .filter(|id| valid_id(id))
        .ok_or("invalidName")?;
    let path = profile.join("manifest.json");
    let _lock = DirectoryLock::acquire_existing(&path).map_err(|_| "writeFailed")?;
    if !regular_file(&path) {
        return Err("unsupported");
    }
    let raw = fs::read(&path).map_err(|_| "readFailed")?;
    let mut manifest = parse_manifest(&raw)?;
    let skills = manifest["skills"].as_array_mut().ok_or("invalidConfig")?;
    if skills.iter().any(|skill| {
        skill["skillId"]
            .as_str()
            .is_some_and(|other| other.eq_ignore_ascii_case(id))
    }) {
        return Err("exists");
    }
    let metadata = source.join("SKILL.md");
    let now = Utc::now();
    let updated = now.to_rfc3339_opts(SecondsFormat::Millis, true);
    skills.push(json!({
        "skillId": id,
        "name": markdown_field(&metadata, "name").as_deref().unwrap_or(name),
        "description": markdown_field(&metadata, "description").unwrap_or_default(),
        "creatorType": "user",
        "syncManaged": false,
        "updatedAt": updated,
        "enabled": true,
    }));
    manifest["lastUpdated"] = if manifest["lastUpdated"].is_string() {
        json!(updated)
    } else {
        json!(now.timestamp_millis())
    };
    let bytes = native_account_files::json(&manifest).map_err(|_| "writeFailed")?;

    let root = profile.join("skills");
    if fs::symlink_metadata(&root).is_ok() && !real_directory(&root) {
        return Err("unsupported");
    }
    super::copy_skill(source, &root, id)?;
    let result = if fs::read(&path).is_ok_and(|current| current == raw) {
        native_account_files::write_private_file(&path, &bytes).map_err(|_| "writeFailed")
    } else {
        Err("writeFailed")
    };
    if result.is_err() {
        let _ = fs::remove_dir_all(root.join(id));
    }
    result
}
