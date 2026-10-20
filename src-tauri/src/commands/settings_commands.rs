use crate::utils::platform::echobird_dir;
use serde::{Deserialize, Serialize};
use tauri::Manager;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppSettings {
    #[serde(default)]
    pub locale: Option<String>,
    // The TS side sends/expects camelCase `themeMode` (light | dark | undefined).
    // Without the rename + default, serde would drop unknown fields on save and
    // the user's theme choice would silently reset on every restart.
    #[serde(default, rename = "themeMode")]
    pub theme_mode: Option<String>,
    #[serde(default, rename = "colorTheme")]
    pub color_theme: Option<String>,
    #[serde(default, rename = "closeToTray")]
    pub close_to_tray: Option<bool>,
    #[serde(default, rename = "closeWindowBehaviorSet")]
    pub close_window_behavior_set: Option<bool>,
    // "我的AI生涯" profile display name. The avatar is a separate PNG file
    // (see set_avatar / get_avatar) rather than inline base64 to keep
    // settings.json small.
    #[serde(default, rename = "displayName")]
    pub display_name: Option<String>,
    /// Built-in agent's anonymous Parallel tools. Missing means enabled.
    #[serde(default, rename = "agentWebSearch")]
    pub agent_web_search: Option<bool>,
}

impl AppSettings {
    pub(crate) fn agent_search_enabled(&self) -> bool {
        self.agent_web_search.unwrap_or(true)
    }
}

fn settings_path() -> std::path::PathBuf {
    echobird_dir().join("settings.json")
}

/// Read app settings from ~/.echobird/settings.json
#[tauri::command]
pub fn get_settings() -> AppSettings {
    read_settings(&settings_path())
}

fn read_settings(path: &std::path::Path) -> AppSettings {
    if !path.exists() {
        // Only new installations default to dark. An unset themeMode in an
        // existing file still represents the user's follow-system preference.
        return AppSettings {
            theme_mode: Some("dark".to_string()),
            ..AppSettings::default()
        };
    }
    match std::fs::read_to_string(path) {
        Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
        Err(e) => {
            log::warn!("[Settings] Failed to read settings: {}", e);
            AppSettings::default()
        }
    }
}

/// Write app settings to ~/.echobird/settings.json
#[tauri::command]
pub fn save_settings(settings: AppSettings, app: tauri::AppHandle) -> Result<(), String> {
    let dir = echobird_dir();
    if !dir.exists() {
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(&settings).map_err(|e| e.to_string())?;
    std::fs::write(settings_path(), json).map_err(|e| e.to_string())?;

    // Update tray menu locale if locale changed
    if let Some(locale) = &settings.locale {
        let state = app.state::<crate::TrayState>();
        let mut current_locale = state.locale.lock().unwrap();
        if *current_locale != *locale {
            *current_locale = locale.clone();
            drop(current_locale); // Release lock before calling rebuild
            crate::rebuild_tray_menu(&app);
        }
    }

    Ok(())
}

fn my_projects_path() -> std::path::PathBuf {
    echobird_dir().join("projects.json")
}

/// Read the user-authored AI-project registry from ~/.echobird/projects.json.
/// Returns an empty array only when the file is missing. The front-end
/// (`myProjectsStore`) owns the `MyProject` shape — we persist the array
/// verbatim as JSON so the registry survives a webview-storage clear, can be
/// backed up / hand-edited like the rest of ~/.echobird, and can be written by
/// the "一键安装" agent. Replaces the old localStorage-only persistence.
#[tauri::command]
pub fn get_my_projects() -> Result<serde_json::Value, String> {
    read_my_projects(&my_projects_path())
}

fn validate_my_projects(projects: &serde_json::Value) -> Result<(), String> {
    let valid = projects.as_array().is_some_and(|rows| {
        rows.iter().all(|row| {
            ["id", "name", "launcherPath", "modelsJsonPath"]
                .iter()
                .all(|key| row[*key].is_string())
        })
    });
    valid
        .then_some(())
        .ok_or_else(|| "PROJECTS_INVALID_DATA".into())
}

fn read_my_projects(path: &std::path::Path) -> Result<serde_json::Value, String> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            if std::fs::symlink_metadata(path).is_ok() {
                return Err("PROJECTS_READ_FAILED".into());
            }
            return Ok(serde_json::json!([]));
        }
        Err(_) => return Err("PROJECTS_READ_FAILED".into()),
    };
    let projects = serde_json::from_slice(&bytes).map_err(|_| "PROJECTS_INVALID_DATA")?;
    validate_my_projects(&projects)?;
    Ok(projects)
}

/// Write the user-authored AI-project registry to ~/.echobird/projects.json
/// (pretty-printed). The front-end sends the full array on every CRUD op.
#[tauri::command]
pub fn save_my_projects(projects: serde_json::Value) -> Result<(), String> {
    save_my_projects_at(&my_projects_path(), &projects)
}

fn save_my_projects_at(path: &std::path::Path, projects: &serde_json::Value) -> Result<(), String> {
    validate_my_projects(projects)?;
    // Unknown existing data must never be replaced with an empty-load fallback.
    read_my_projects(path)?;
    let json = serde_json::to_string_pretty(&projects).map_err(|e| e.to_string())?;
    crate::services::native_account_files::write_private_file(path, json.as_bytes())
}

fn avatar_path() -> std::path::PathBuf {
    echobird_dir().join("avatar.png")
}

/// Set the "我的AI生涯" profile avatar from a user-picked image file. The
/// source is decoded, downscaled to fit 256×256 (aspect preserved), and
/// re-encoded as PNG at `~/.echobird/avatar.png` so a large photo can't bloat
/// the stored file or the IPC payload.
#[tauri::command]
pub fn set_avatar(source_path: String) -> Result<(), String> {
    let img = image::open(&source_path).map_err(|e| format!("read image: {e}"))?;
    let thumb = img.thumbnail(256, 256);
    let dir = echobird_dir();
    if !dir.exists() {
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    }
    thumb
        .save_with_format(avatar_path(), image::ImageFormat::Png)
        .map_err(|e| format!("save avatar: {e}"))?;
    Ok(())
}

/// Read the stored avatar as a base64 PNG data URI, or `None` if unset.
#[tauri::command]
pub fn get_avatar() -> Option<String> {
    use base64::Engine as _;
    let bytes = std::fs::read(avatar_path()).ok()?;
    let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
    Some(format!("data:image/png;base64,{b64}"))
}

#[cfg(test)]
mod tests {
    use super::{read_my_projects, read_settings, save_my_projects_at};

    #[test]
    fn project_registry_refuses_to_overwrite_unreadable_or_malformed_data() {
        let root = std::env::temp_dir().join(format!("echobird-projects-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("projects.json");
        assert_eq!(read_my_projects(&path).unwrap(), serde_json::json!([]));
        assert!(!path.exists());
        for original in ["[", "{}", "[null]", r#"[{"id":"missing-fields"}]"#] {
            std::fs::write(&path, original).unwrap();
            assert!(read_my_projects(&path).is_err());
            assert!(save_my_projects_at(&path, &serde_json::json!([])).is_err());
            assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
        }
        std::fs::remove_file(&path).unwrap();
        std::fs::create_dir(&path).unwrap();
        assert!(read_my_projects(&path).is_err());
        assert!(save_my_projects_at(&path, &serde_json::json!([])).is_err());
        assert!(path.is_dir());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn project_registry_saves_valid_arrays_atomically_and_keeps_unknown_fields() {
        let root = std::env::temp_dir().join(format!("echobird-projects-{}", uuid::Uuid::new_v4()));
        let path = root.join("projects.json");
        let projects = serde_json::json!([{"id":"one","name":"One","launcherPath":"/fixture/run",
            "modelsJsonPath":"/fixture/models.json","extra":{"keep":true}}]);
        save_my_projects_at(&path, &projects).unwrap();
        assert_eq!(read_my_projects(&path).unwrap(), projects);
        let previous = root.join("previous.json");
        std::fs::hard_link(&path, &previous).unwrap();
        save_my_projects_at(&path, &serde_json::json!([])).unwrap();
        assert_eq!(read_my_projects(&path).unwrap(), serde_json::json!([]));
        assert_eq!(read_my_projects(&previous).unwrap(), projects);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn project_registry_preserves_an_existing_symbolic_link_and_directory_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let root =
            std::env::temp_dir().join(format!("echobird-project-link-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o750)).unwrap();
        let target = root.join("actual.json");
        let path = root.join("projects.json");
        let projects = serde_json::json!([{"id":"one","name":"One","launcherPath":"/fixture/run",
            "modelsJsonPath":"/fixture/models.json","extra":{"keep":true}}]);
        std::fs::write(&target, serde_json::to_vec(&projects).unwrap()).unwrap();
        std::os::unix::fs::symlink("actual.json", &path).unwrap();
        assert_eq!(read_my_projects(&path).unwrap(), projects);
        save_my_projects_at(&path, &serde_json::json!([])).unwrap();
        assert!(std::fs::symlink_metadata(&path)
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(read_my_projects(&target).unwrap(), serde_json::json!([]));
        assert_eq!(
            std::fs::metadata(&root).unwrap().permissions().mode() & 0o777,
            0o750
        );
        std::fs::remove_file(&target).unwrap();
        assert!(read_my_projects(&path).is_err());
        assert!(save_my_projects_at(&path, &projects).is_err());
        assert!(!target.exists());
        assert!(std::fs::symlink_metadata(&path)
            .unwrap()
            .file_type()
            .is_symlink());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn first_install_defaults_to_dark_without_creating_settings() {
        let path =
            std::env::temp_dir().join(format!("echobird-theme-{}.json", uuid::Uuid::new_v4()));
        let settings = read_settings(&path);
        assert_eq!(settings.theme_mode.as_deref(), Some("dark"));
        assert!(settings.color_theme.is_none());
        assert!(!path.exists());
    }

    #[test]
    fn parallel_search_default_and_saved_disable_are_read_without_writes() {
        let path =
            std::env::temp_dir().join(format!("echobird-search-{}.json", uuid::Uuid::new_v4()));
        assert!(read_settings(&path).agent_search_enabled());
        assert!(!path.exists());
        for (content, enabled) in [
            (r#"{}"#, true),
            (r#"{"agentWebSearch":false,"themeMode":"light"}"#, false),
            (r#"{"agentWebSearch":true}"#, true),
        ] {
            std::fs::write(&path, content).unwrap();
            let settings = read_settings(&path);
            assert_eq!(settings.agent_search_enabled(), enabled);
            let saved = serde_json::to_string(&settings).unwrap();
            let reloaded: AppSettings = serde_json::from_str(&saved).unwrap();
            assert_eq!(reloaded.agent_search_enabled(), enabled);
            assert_eq!(std::fs::read_to_string(&path).unwrap(), content);
        }
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn saved_theme_preferences_are_preserved_without_rewriting() {
        let path =
            std::env::temp_dir().join(format!("echobird-theme-{}.json", uuid::Uuid::new_v4()));
        for (content, mode, palette) in [
            (r#"{}"#, None, None),
            (r#"{"themeMode":null}"#, None, None),
            (r#"{"themeMode":"light"}"#, Some("light"), None),
            (r#"{"themeMode":"dark"}"#, Some("dark"), None),
            (
                r#"{"themeMode":null,"colorTheme":"aurora"}"#,
                None,
                Some("aurora"),
            ),
        ] {
            std::fs::write(&path, content).unwrap();
            let settings = read_settings(&path);
            assert_eq!(settings.theme_mode.as_deref(), mode);
            assert_eq!(settings.color_theme.as_deref(), palette);
            assert_eq!(std::fs::read_to_string(&path).unwrap(), content);
        }
        std::fs::remove_file(path).unwrap();
    }
}
