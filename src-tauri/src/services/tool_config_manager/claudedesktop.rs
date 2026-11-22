//! Claude Desktop Code uses a local first-party identity and the shared model relay.
use super::{echobird_dir, read_json_file, ApplyResult, ModelInfo};
use crate::services::native_account_files::{self, Transaction};
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};

mod egress;

pub(super) const CLAUDE_DESKTOP_PROFILE_ID: &str = "7d8f4e2a-9c3b-4f1a-b0e5-1a2b3c4d5e6f";
pub(crate) static SELECTION_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

pub(crate) struct ClaudeDesktopLayout {
    cfg_official: PathBuf,
    cfg_threep: PathBuf,
    pub(super) lib_dir: PathBuf,
}

#[cfg(any(target_os = "macos", windows, target_os = "linux"))]
pub(crate) fn resolve_claudedesktop_paths() -> Option<ClaudeDesktopLayout> {
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

pub(crate) fn claudedesktop_paths_at(config_root: &Path) -> ClaudeDesktopLayout {
    let official_dir = config_root.join("Claude");
    let threep_dir = config_root.join("Claude-3p");
    ClaudeDesktopLayout {
        cfg_official: official_dir.join("claude_desktop_config.json"),
        cfg_threep: threep_dir.join("claude_desktop_config.json"),
        lib_dir: threep_dir.join("configLibrary"),
    }
}

#[cfg(not(any(target_os = "macos", windows, target_os = "linux")))]
pub(crate) fn resolve_claudedesktop_paths() -> Option<ClaudeDesktopLayout> {
    None
}

fn backup_library(paths: &ClaudeDesktopLayout) -> PathBuf {
    paths
        .lib_dir
        .with_file_name("configLibrary.echobird-local-backup")
}

fn clear_empty_backup_conflict(live: &Path, backup: &Path, conflict: &str) -> Result<(), String> {
    if !live.exists() {
        return Ok(());
    }

    match fs::symlink_metadata(backup) {
        Ok(metadata) if metadata.is_dir() => {
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                use windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
                if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 {
                    return Err(conflict.into());
                }
            }
            // A non-recursive removal only succeeds for an empty directory.
            fs::remove_dir(backup).map_err(|_| conflict.into())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        _ => Err(conflict.into()),
    }
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

pub(super) fn model_parameters(info: &ModelInfo) -> Result<(&str, &str, &str), String> {
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
    Ok((base, key, model))
}

fn apply_at(info: &ModelInfo, paths: &ClaudeDesktopLayout, relay: &Path) -> Result<(), String> {
    let (base, key, model) = model_parameters(info)?;
    let _lock = native_account_files::DirectoryLock::acquire(relay)?;
    let mut official = native_account_files::object(&paths.cfg_official)?;
    let mut threep = native_account_files::object(&paths.cfg_threep)?;
    let egress = egress::retire(&paths.lib_dir)?;
    let mut transaction = Transaction::guarded();
    let backup = backup_library(paths);
    clear_empty_backup_conflict(
        &paths.lib_dir,
        &backup,
        "Claude Desktop has an unrecovered profile backup.",
    )?;
    official["deploymentMode"] = json!("1p");
    threep["deploymentMode"] = json!("1p");
    let config = json!({"mode":"firstParty", "baseUrl":base.trim_end_matches('/'),
        "apiKey":if key.is_empty() { "local-no-auth" } else {key},
        "actualModel":model,"modelName":info.name.as_deref().unwrap_or(model),
        "relayMode":true,"oneMContext":info.one_m_context,"webSearch":info.web_search});
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
    if let Some(egress) = egress {
        egress.commit();
    }
    Ok(())
}

pub(crate) fn relay_path() -> PathBuf {
    echobird_dir().join("claudedesktop.json")
}

pub(crate) fn prepare_cowork_proxy(port: u16) -> Result<(), String> {
    let paths = resolve_claudedesktop_paths().ok_or("Cannot resolve Claude Desktop profiles.")?;
    let _lock = native_account_files::DirectoryLock::acquire(&relay_path())?;
    egress::write_proxy(&paths.lib_dir, port)
}

pub(crate) fn clear_cowork_proxy() -> Result<(), String> {
    let paths = resolve_claudedesktop_paths().ok_or("Cannot resolve Claude Desktop profiles.")?;
    let _lock = native_account_files::DirectoryLock::acquire(&relay_path())?;
    if let Some(egress) = egress::retire(&paths.lib_dir)? {
        egress.commit();
    }
    Ok(())
}

#[cfg(test)]
fn restore_at(paths: &ClaudeDesktopLayout, relay: &Path) -> Result<(), String> {
    restore_with(paths, relay, |_| Ok(()))
}

pub(crate) fn restore_with<T>(
    paths: &ClaudeDesktopLayout,
    relay: &Path,
    finish: impl FnOnce(&mut Transaction) -> Result<T, String>,
) -> Result<T, String> {
    let _lock = native_account_files::DirectoryLock::acquire(relay)?;
    let mut official = native_account_files::object(&paths.cfg_official)?;
    let mut threep = native_account_files::object(&paths.cfg_threep)?;
    let egress = egress::retire(&paths.lib_dir)?;
    let mut transaction = Transaction::guarded();
    let backup = backup_library(paths);
    clear_empty_backup_conflict(
        &paths.lib_dir,
        &backup,
        "Claude Desktop profiles changed while local mode was active.",
    )?;
    official["deploymentMode"] = json!("1p");
    threep["deploymentMode"] = json!("1p");
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
    let moved_library = backup.exists();
    if moved_library {
        fs::rename(&backup, &paths.lib_dir)
            .map_err(|error| format!("Cannot restore Claude profiles: {error}"))?;
    }
    match finish(&mut transaction) {
        Ok(value) => {
            transaction.commit();
            if let Some(egress) = egress {
                egress.commit();
            }
            Ok(value)
        }
        Err(error) => {
            // The transaction's recorded profile paths are in the backup.
            if moved_library {
                fs::rename(&paths.lib_dir, &backup).map_err(|rollback| {
                    format!("{error}. Cannot roll back Claude profiles: {rollback}")
                })?;
            }
            Err(error)
        }
    }
}

pub(super) fn read_for_import() -> Vec<ModelInfo> {
    if let Some(models) = resolve_claudedesktop_paths()
        .as_ref()
        .and_then(read_native_gateway_models)
    {
        return models;
    }
    read_claudedesktop().into_iter().collect()
}

fn read_native_gateway_models(paths: &ClaudeDesktopLayout) -> Option<Vec<ModelInfo>> {
    // Read the active gateway profile. A discovered model catalogue or a signed-in
    // cloud gateway is not sufficient to reconstruct reusable API models.
    let metadata = read_json_file(&paths.lib_dir.join("_meta.json"))?;
    let id = metadata.get("appliedId")?.as_str()?;
    if id.is_empty()
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return None;
    }
    let profile = read_json_file(&paths.lib_dir.join(format!("{id}.json")))?;
    if profile["inferenceProvider"] != "gateway" {
        return None;
    }
    let base = profile["inferenceGatewayBaseUrl"].as_str()?;
    let key = profile["inferenceGatewayApiKey"].as_str()?;
    let list = profile["inferenceModels"].as_array()?;
    Some(
        list.iter()
            .filter_map(|entry| {
                let model = entry.as_str().or_else(|| entry["name"].as_str())?;
                serde_json::from_value(json!({
                    "model": model,
                    "anthropicUrl": base.trim_end_matches('/').trim_end_matches("/v1"),
                    "apiKey": key,
                    "protocol": "anthropic"
                }))
                .ok()
            })
            .collect(),
    )
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
    use crate::services::smart_router::catalog::with_test_models;
    use crate::services::tool_config_manager::switching::{test_info, TestDir};

    #[test]
    fn imports_explicit_models_from_only_the_applied_gateway_profile() {
        let root = tempfile::tempdir().unwrap();
        let paths = claudedesktop_paths_at(root.path());
        fs::create_dir_all(&paths.lib_dir).unwrap();
        fs::write(
            paths.lib_dir.join("_meta.json"),
            br#"{"appliedId":"active"}"#,
        )
        .unwrap();
        let active = paths.lib_dir.join("active.json");
        let original = br#"{"inferenceProvider":"gateway","inferenceGatewayBaseUrl":"https://relay.example/v1","inferenceGatewayApiKey":"fixture","inferenceModels":["model-one",{"name":"model-two"}]}"#;
        fs::write(&active, original).unwrap();
        fs::write(paths.lib_dir.join("other.json"), b"unused").unwrap();
        let models = read_native_gateway_models(&paths).unwrap();
        assert_eq!(
            models
                .iter()
                .filter_map(|model| model.model.as_deref())
                .collect::<Vec<_>>(),
            ["model-one", "model-two"]
        );
        assert!(models
            .iter()
            .all(|model| model.anthropic_url.as_deref() == Some("https://relay.example")));
        assert_eq!(fs::read(&active).unwrap(), original);
    }

    #[tokio::test]
    #[ignore = "explicitly stops an installed, running Claude Desktop without configuration writes"]
    async fn running_client_is_stopped_before_local_identity_switch() {
        assert!(
            crate::services::process_manager::desktop_tool_is_running("claudedesktop")
                .await
                .unwrap()
        );
        let relay = echobird_dir().join("claudedesktop.json");
        let before = fs::metadata(&relay)
            .ok()
            .and_then(|metadata| metadata.modified().ok());
        crate::services::process_manager::stop_desktop_for_account("claudedesktop")
            .await
            .unwrap();
        let after = fs::metadata(&relay)
            .ok()
            .and_then(|metadata| metadata.modified().ok());
        assert_eq!(before, after);
        assert!(
            !crate::services::process_manager::desktop_tool_is_running("claudedesktop")
                .await
                .unwrap()
        );
    }

    #[test]
    fn model_validation_rejects_invalid_input_before_switch_preparation() {
        let mut info = test_info();
        info.base_url = Some("file:///fixture".into());
        assert!(model_parameters(&info).is_err());
        info.base_url = Some("https://fixture.invalid".into());
        info.api_key = None;
        assert!(model_parameters(&info).is_err());
        info.base_url = Some("http://127.0.0.1:53683/v1".into());
        assert!(model_parameters(&info).is_ok());
        info.model = Some(String::new());
        assert!(model_parameters(&info).is_err());
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
    fn retiring_local_mode_restores_the_previous_library_and_rolls_back_on_account_failure() {
        let root = TestDir::new();
        let paths = claudedesktop_paths_at(root.path());
        let relay = root.path().join("state/relay.json");
        fs::create_dir_all(&paths.lib_dir).unwrap();
        fs::write(paths.lib_dir.join("other.json"), b"original profile").unwrap();
        with_test_models(|| apply_at(&test_info(), &paths, &relay)).unwrap();
        egress::write_proxy(&paths.lib_dir, 35801).unwrap();
        let before = fs::read(paths.lib_dir.join("_meta.json")).unwrap();

        let failed = restore_with(&paths, &relay, |_| Err::<(), _>("failed auth".into()));
        assert_eq!(failed.unwrap_err(), "failed auth");
        assert_eq!(fs::read(paths.lib_dir.join("_meta.json")).unwrap(), before);
        assert_eq!(
            fs::read(backup_library(&paths).join("other.json")).unwrap(),
            b"original profile"
        );
        assert!(relay.exists());

        with_test_models(|| apply_at(&test_info(), &paths, &relay)).unwrap();
        assert!(!paths.lib_dir.exists());
        egress::write_proxy(&paths.lib_dir, 35802).unwrap();
        restore_at(&paths, &relay).unwrap();
        assert_eq!(
            fs::read(paths.lib_dir.join("other.json")).unwrap(),
            b"original profile"
        );
        assert_eq!(fs::read_dir(&paths.lib_dir).unwrap().count(), 1);
        assert!(!backup_library(&paths).exists());
    }

    #[test]
    fn switches_with_an_empty_conflicting_backup_and_preserves_live_profiles() {
        let root = TestDir::new();
        let paths = claudedesktop_paths_at(root.path());
        let relay = root.path().join("state/relay.json");
        let backup = backup_library(&paths);
        fs::create_dir_all(&paths.lib_dir).unwrap();
        fs::write(paths.lib_dir.join("other.json"), b"live-profile").unwrap();
        fs::create_dir(&backup).unwrap();

        with_test_models(|| apply_at(&test_info(), &paths, &relay)).unwrap();

        assert!(!paths.lib_dir.exists());
        assert_eq!(
            fs::read(backup.join("other.json")).unwrap(),
            b"live-profile"
        );
        assert!(relay.is_file());

        restore_at(&paths, &relay).unwrap();
        assert_eq!(
            fs::read(paths.lib_dir.join("other.json")).unwrap(),
            b"live-profile"
        );
        assert!(!backup.exists());
    }

    #[test]
    fn restores_official_with_an_empty_conflicting_backup() {
        let root = TestDir::new();
        let paths = claudedesktop_paths_at(root.path());
        let relay = root.path().join("relay.json");
        let backup = backup_library(&paths);
        fs::create_dir_all(&paths.lib_dir).unwrap();
        fs::write(paths.lib_dir.join("other.json"), b"live-profile").unwrap();
        fs::create_dir(&backup).unwrap();
        fs::write(&relay, br#"{"mode":"firstParty"}"#).unwrap();

        restore_at(&paths, &relay).unwrap();

        assert!(!relay.exists());
        assert!(!backup.exists());
        assert_eq!(
            fs::read(paths.lib_dir.join("other.json")).unwrap(),
            b"live-profile"
        );
    }

    #[test]
    fn preserves_both_libraries_when_a_conflicting_backup_contains_files() {
        let root = TestDir::new();
        let paths = claudedesktop_paths_at(root.path());
        let relay = root.path().join("relay.json");
        let backup = backup_library(&paths);
        fs::create_dir_all(&paths.lib_dir).unwrap();
        fs::write(paths.lib_dir.join("other.json"), b"live-profile").unwrap();
        fs::create_dir(&backup).unwrap();
        fs::write(backup.join(".hidden"), b"backup-profile").unwrap();

        assert_eq!(
            with_test_models(|| apply_at(&test_info(), &paths, &relay)).unwrap_err(),
            "Claude Desktop has an unrecovered profile backup."
        );
        assert_eq!(
            restore_at(&paths, &relay).unwrap_err(),
            "Claude Desktop profiles changed while local mode was active."
        );
        assert_eq!(
            fs::read(paths.lib_dir.join("other.json")).unwrap(),
            b"live-profile"
        );
        assert_eq!(fs::read(backup.join(".hidden")).unwrap(), b"backup-profile");
        assert!(!relay.exists());
        assert!(!paths.cfg_official.exists());
        assert!(!paths.cfg_threep.exists());
    }

    #[cfg(unix)]
    #[test]
    fn preserves_a_conflicting_backup_link_to_an_empty_directory() {
        let root = TestDir::new();
        let paths = claudedesktop_paths_at(root.path());
        let backup = backup_library(&paths);
        let target = root.path().join("empty");
        fs::create_dir_all(&paths.lib_dir).unwrap();
        fs::create_dir(&target).unwrap();
        std::os::unix::fs::symlink(&target, &backup).unwrap();

        assert!(clear_empty_backup_conflict(&paths.lib_dir, &backup, "conflict").is_err());

        assert_eq!(fs::read_link(backup).unwrap(), target);
        assert!(target.is_dir());
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
