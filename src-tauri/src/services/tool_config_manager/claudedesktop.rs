//! Claude Desktop Code uses a local first-party identity and the shared model relay.
use super::{echobird_dir, read_json_file, ApplyResult, ModelInfo};
use crate::services::native_account_files::{self, Transaction};
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};

pub(super) const CLAUDE_DESKTOP_PROFILE_ID: &str = "7d8f4e2a-9c3b-4f1a-b0e5-1a2b3c4d5e6f";

pub(super) struct ClaudeDesktopLayout {
    cfg_official: PathBuf,
    cfg_threep: PathBuf,
    pub(super) lib_dir: PathBuf,
}

#[cfg(any(target_os = "macos", windows, target_os = "linux"))]
pub(super) fn resolve_claudedesktop_paths() -> Option<ClaudeDesktopLayout> {
    #[cfg(any(target_os = "macos", windows))]
    let home = dirs::home_dir()?;

    #[cfg(target_os = "macos")]
    let config_root = home.join("Library").join("Application Support");

    #[cfg(windows)]
    let config_root = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join("AppData").join("Local"));

    // Electron uses $XDG_CONFIG_HOME, falling back to ~/.config on Linux.
    #[cfg(target_os = "linux")]
    let config_root = dirs::config_dir()?;

    Some(claudedesktop_paths_at(&config_root))
}

fn claudedesktop_paths_at(config_root: &Path) -> ClaudeDesktopLayout {
    let official_dir = config_root.join("Claude");
    let threep_dir = config_root.join("Claude-3p");
    ClaudeDesktopLayout {
        cfg_official: official_dir.join("claude_desktop_config.json"),
        cfg_threep: threep_dir.join("claude_desktop_config.json"),
        lib_dir: threep_dir.join("configLibrary"),
    }
}

#[cfg(not(any(target_os = "macos", windows, target_os = "linux")))]
pub(super) fn resolve_claudedesktop_paths() -> Option<ClaudeDesktopLayout> {
    None
}

fn backup_library(paths: &ClaudeDesktopLayout) -> PathBuf {
    paths
        .lib_dir
        .with_file_name("configLibrary.echobird-local-backup")
}

pub(crate) fn local_mode_enabled() -> bool {
    read_json_file(&echobird_dir().join("claudedesktop.json"))
        .is_some_and(|value| value["mode"] == "firstParty")
}

pub(super) fn apply_claudedesktop(model_info: &ModelInfo) -> ApplyResult {
    let Some(paths) = resolve_claudedesktop_paths() else {
        return result(Err(
            "Cannot resolve Claude Desktop configuration directory.".into(),
        ));
    };
    result(apply_at(
        model_info,
        &paths,
        &echobird_dir().join("claudedesktop.json"),
    ))
}

fn result(value: Result<(), String>) -> ApplyResult {
    match value {
        Ok(()) => ApplyResult {
            success: true,
            message: "Claude Desktop configured. Restart Claude Desktop to apply the selection."
                .into(),
        },
        Err(message) => ApplyResult {
            success: false,
            message,
        },
    }
}

fn apply_at(info: &ModelInfo, paths: &ClaudeDesktopLayout, relay: &Path) -> Result<(), String> {
    let _lock = native_account_files::DirectoryLock::acquire(relay)?;
    let base = info
        .anthropic_url
        .as_deref()
        .or(info.base_url.as_deref())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or("Base URL is empty.")?;
    let url = url::Url::parse(base).map_err(|_| "Invalid model endpoint.")?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err("Invalid model endpoint.".into());
    }
    let key = info.api_key.as_deref().unwrap_or_default();
    let local = matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "[::1]"));
    if key.is_empty() && !local {
        return Err("API Key is empty.".into());
    }
    let model = info
        .model
        .as_deref()
        .or(info.name.as_deref())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or("Select a model first.")?;
    // Resolve a Smart Access catalog before publishing native configuration.
    super::switching::models(info)?;
    let mut official = native_account_files::object(&paths.cfg_official)?;
    let mut threep = native_account_files::object(&paths.cfg_threep)?;
    let backup = backup_library(paths);
    if backup.exists() && paths.lib_dir.exists() {
        return Err("Claude Desktop has an unrecovered profile backup.".into());
    }
    official["deploymentMode"] = json!("1p");
    threep["deploymentMode"] = json!("1p");
    let config = json!({"mode":"firstParty", "baseUrl":base.trim_end_matches('/'),
        "apiKey":if key.is_empty() { "local-no-auth" } else {key},
        "actualModel":model,"modelName":info.name.as_deref().unwrap_or(model),
        "relayMode":true,"oneMContext":info.one_m_context,"webSearch":info.web_search});
    let mut transaction = Transaction::guarded();
    transaction.replace(&paths.cfg_official, &native_account_files::json(&official)?)?;
    transaction.replace(&paths.cfg_threep, &native_account_files::json(&threep)?)?;
    transaction.replace(relay, &native_account_files::json(&config)?)?;
    // A gateway profile can force 3p even when deploymentMode is 1p. Retain
    // the complete directory for restoring native profiles and unknown fields.
    if paths.lib_dir.exists() {
        fs::rename(&paths.lib_dir, &backup)
            .map_err(|error| format!("Cannot preserve Claude profiles: {error}"))?;
    }
    transaction.commit();
    Ok(())
}

pub(super) fn restore_claudedesktop_to_official() -> ApplyResult {
    let Some(paths) = resolve_claudedesktop_paths() else {
        return result(Err(
            "Cannot resolve Claude Desktop configuration directory.".into(),
        ));
    };
    result(restore_at(
        &paths,
        &echobird_dir().join("claudedesktop.json"),
    ))
}

fn restore_at(paths: &ClaudeDesktopLayout, relay: &Path) -> Result<(), String> {
    let _lock = native_account_files::DirectoryLock::acquire(relay)?;
    let mut official = native_account_files::object(&paths.cfg_official)?;
    let mut threep = native_account_files::object(&paths.cfg_threep)?;
    let backup = backup_library(paths);
    if backup.exists() && paths.lib_dir.exists() {
        return Err("Claude Desktop profiles changed while local mode was active.".into());
    }
    official["deploymentMode"] = json!("1p");
    threep["deploymentMode"] = json!("1p");
    let mut transaction = Transaction::guarded();
    transaction.replace(&paths.cfg_official, &native_account_files::json(&official)?)?;
    transaction.replace(&paths.cfg_threep, &native_account_files::json(&threep)?)?;
    // Retire EchoBird's legacy 3p profile during upgrade as well as restore.
    let library = if backup.exists() {
        &backup
    } else {
        &paths.lib_dir
    };
    let meta_path = library.join("_meta.json");
    if meta_path.exists() {
        let mut meta = native_account_files::object(&meta_path)?;
        if let Some(entries) = meta.get_mut("entries") {
            let entries = entries
                .as_array_mut()
                .ok_or("Invalid Claude profile entries.")?;
            entries.retain(|entry| entry["id"] != CLAUDE_DESKTOP_PROFILE_ID);
        }
        if meta["appliedId"] == CLAUDE_DESKTOP_PROFILE_ID {
            meta.as_object_mut().unwrap().remove("appliedId");
        }
        transaction.replace(&meta_path, &native_account_files::json(&meta)?)?;
    }
    transaction.remove(&library.join(format!("{CLAUDE_DESKTOP_PROFILE_ID}.json")))?;
    transaction.remove(relay)?;
    if backup.exists() {
        fs::rename(&backup, &paths.lib_dir)
            .map_err(|error| format!("Cannot restore Claude profiles: {error}"))?;
    }
    transaction.commit();
    Ok(())
}

pub(super) fn read_claudedesktop() -> Option<ModelInfo> {
    let relay = read_json_file(&echobird_dir().join("claudedesktop.json"))?;
    Some(ModelInfo {
        name: relay["modelName"].as_str().map(String::from),
        model: relay["actualModel"].as_str().map(String::from),
        base_url: None,
        api_key: relay["apiKey"].as_str().map(String::from),
        anthropic_url: relay["baseUrl"].as_str().map(String::from),
        protocol: Some("anthropic".into()),
        display_model: None,
        relay_mode: None,
        web_search: relay["webSearch"].as_bool(),
        image_input: None,
        one_m_context: relay["oneMContext"].as_bool(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::tool_config_manager::switching::{test_info, TestDir};

    #[tokio::test]
    #[ignore = "requires an installed, running Claude Desktop and performs no configuration writes"]
    async fn running_client_requires_manual_exit_before_local_identity_switch() {
        assert!(
            crate::services::process_manager::desktop_tool_is_running("claudedesktop")
                .await
                .unwrap()
        );
        let relay = echobird_dir().join("claudedesktop.json");
        let before = fs::metadata(&relay)
            .ok()
            .and_then(|metadata| metadata.modified().ok());
        let mut info = test_info();
        info.base_url = Some("https://fixture.invalid/anthropic".into());
        info.model = Some("fixture-model".into());
        let result = super::super::apply_model_to_tool("claudedesktop", info).await;
        assert!(!result.success);
        assert_eq!(result.message, "accountError.closeClient");
        let after = fs::metadata(&relay)
            .ok()
            .and_then(|metadata| metadata.modified().ok());
        assert_eq!(before, after);
        assert!(
            crate::services::process_manager::desktop_tool_is_running("claudedesktop")
                .await
                .unwrap()
        );
    }

    #[test]
    fn applies_first_party_and_preserves_profiles_across_repeated_switches() {
        let root = TestDir::new();
        let paths = claudedesktop_paths_at(root.path());
        fs::create_dir_all(&paths.lib_dir).unwrap();
        fs::write(paths.lib_dir.join("other.json"), b"original-profile").unwrap();
        let relay = root.path().join("state/relay.json");
        let mut info = test_info();
        info.base_url = Some("https://example.com/anthropic".into());
        info.model = Some("custom-model".into());
        apply_at(&info, &paths, &relay).unwrap();
        apply_at(&info, &paths, &relay).unwrap();
        assert_eq!(
            native_account_files::object(&paths.cfg_official).unwrap()["deploymentMode"],
            "1p"
        );
        assert_eq!(
            native_account_files::object(&relay).unwrap()["actualModel"],
            "custom-model"
        );
        assert!(!paths.lib_dir.exists());
        restore_at(&paths, &relay).unwrap();
        assert_eq!(
            fs::read(paths.lib_dir.join("other.json")).unwrap(),
            b"original-profile"
        );
        assert!(!relay.exists());
    }

    #[test]
    fn invalid_native_config_does_not_change_other_files() {
        let root = TestDir::new();
        let paths = claudedesktop_paths_at(root.path());
        fs::create_dir_all(paths.cfg_threep.parent().unwrap()).unwrap();
        fs::write(&paths.cfg_threep, b"broken").unwrap();
        let relay = root.path().join("relay.json");
        assert!(apply_at(&test_info(), &paths, &relay).is_err());
        assert!(!paths.cfg_official.exists());
        assert!(!relay.exists());
    }

    #[test]
    fn rejects_invalid_upstream_without_publishing_configuration() {
        let root = TestDir::new();
        let paths = claudedesktop_paths_at(root.path());
        let relay = root.path().join("relay.json");
        let mut info = test_info();
        info.base_url = Some("file:///tmp/model".into());
        assert!(apply_at(&info, &paths, &relay).is_err());
        assert!(!relay.exists());
    }
}
