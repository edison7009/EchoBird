use crate::utils::platform::echobird_dir;
use serde::{Deserialize, Serialize};
use tauri::Manager;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppSettings {
    #[serde(
        default,
        rename = "developerMode",
        skip_serializing_if = "Option::is_none"
    )]
    pub developer_mode: Option<bool>,
    #[serde(
        default,
        rename = "aiNewsEnabled",
        skip_serializing_if = "Option::is_none"
    )]
    pub ai_news_enabled: Option<bool>,
    #[serde(
        default,
        rename = "aiCareerEnabled",
        skip_serializing_if = "Option::is_none"
    )]
    pub ai_career_enabled: Option<bool>,
    #[serde(
        default,
        rename = "localLlmEnabled",
        skip_serializing_if = "Option::is_none"
    )]
    pub local_llm_enabled: Option<bool>,
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
    #[serde(
        default,
        rename = "sensitiveInfoProtection",
        skip_serializing_if = "Option::is_none"
    )]
    pub sensitive_info_protection: Option<bool>,
}

impl AppSettings {
    pub fn developer_mode_enabled(&self) -> bool {
        self.developer_mode.unwrap_or(true)
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
pub async fn save_settings(
    settings: serde_json::Value,
    app: tauri::AppHandle,
) -> Result<(), String> {
    let settings = patch_settings_at(&settings_path(), settings)?;
    if settings.local_llm_enabled == Some(false) {
        crate::services::local_llm::server::stop_server_if_running().await?;
    }
    crate::services::developer_preview::configure(&app, settings.developer_mode_enabled());

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

fn patch_settings_at(
    path: &std::path::Path,
    patch: serde_json::Value,
) -> Result<AppSettings, String> {
    let patch = patch.as_object().ok_or("Invalid settings patch")?;
    for key in [
        "developerMode",
        "aiNewsEnabled",
        "aiCareerEnabled",
        "localLlmEnabled",
        "sensitiveInfoProtection",
    ] {
        if patch.get(key).is_some_and(|value| !value.is_boolean()) {
            return Err("Invalid settings flag".into());
        }
    }
    crate::services::json_store::mutate(path, |saved: &mut serde_json::Value| {
        if saved.is_null() {
            *saved = serde_json::to_value(read_settings(path)).map_err(|e| e.to_string())?;
        }
        let object = saved.as_object_mut().ok_or("Invalid saved settings")?;
        for (key, value) in patch {
            object.insert(key.clone(), value.clone());
        }
        let settings: AppSettings =
            serde_json::from_value(saved.clone()).map_err(|_| "Invalid settings")?;
        Ok((settings, true))
    })
}

#[cfg(test)]
fn preserve_protection_setting(
    path: &std::path::Path,
    settings: &mut AppSettings,
) -> Result<(), String> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(_) => return Err("Unable to read settings".into()),
    };
    let saved: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|_| "Invalid settings")?;
    if saved
        .get("sensitiveInfoProtection")
        .is_some_and(|value| !value.is_boolean())
    {
        return Err("Invalid protection setting".into());
    }
    let saved: AppSettings = serde_json::from_value(saved).map_err(|_| "Invalid settings")?;
    if settings.sensitive_info_protection.is_none() {
        settings.sensitive_info_protection = saved.sensitive_info_protection;
    }
    if settings.developer_mode.is_none() {
        settings.developer_mode = saved.developer_mode;
    }
    if settings.ai_news_enabled.is_none() {
        settings.ai_news_enabled = saved.ai_news_enabled;
    }
    if settings.ai_career_enabled.is_none() {
        settings.ai_career_enabled = saved.ai_career_enabled;
    }
    if settings.local_llm_enabled.is_none() {
        settings.local_llm_enabled = saved.local_llm_enabled;
    }
    Ok(())
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
    use super::read_settings;

    #[test]
    fn field_patches_are_atomic_preserve_unknowns_and_clear_system_theme_explicitly() {
        let root = std::env::temp_dir().join(format!("echobird-settings-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("settings.json");
        std::fs::write(&path, r#"{"themeMode":"dark","future":{"keep":true}}"#).unwrap();
        let first = path.clone();
        let a = std::thread::spawn(move || {
            super::patch_settings_at(&first, serde_json::json!({"locale":"ja"})).unwrap()
        });
        let second = path.clone();
        let b = std::thread::spawn(move || {
            super::patch_settings_at(&second, serde_json::json!({"aiNewsEnabled":false})).unwrap()
        });
        a.join().unwrap();
        b.join().unwrap();
        super::patch_settings_at(&path, serde_json::json!({"themeMode":null})).unwrap();
        let saved: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(saved["locale"], "ja");
        assert_eq!(saved["aiNewsEnabled"], false);
        assert_eq!(saved["future"]["keep"], true);
        assert!(saved["themeMode"].is_null());
        let bytes = std::fs::read(&path).unwrap();
        assert!(super::patch_settings_at(
            &path,
            serde_json::json!({"sensitiveInfoProtection":null})
        )
        .is_err());
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        std::fs::write(&path, "broken").unwrap();
        assert!(super::patch_settings_at(&path, serde_json::json!({"locale":"en"})).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"broken");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn optional_page_preferences_round_trip_and_other_updates_preserve_disabled_choices() {
        let root = std::env::temp_dir().join(format!("optional-pages-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("settings.json");
        let defaults = read_settings(&path);
        assert_ne!(defaults.ai_news_enabled, Some(false));
        assert_ne!(defaults.ai_career_enabled, Some(false));
        assert_ne!(defaults.local_llm_enabled, Some(false));
        assert!(!path.exists());
        let original = br#"{"aiNewsEnabled":false,"aiCareerEnabled":false,"localLlmEnabled":false,"locale":"ja"}"#;
        std::fs::write(&path, original).unwrap();
        let saved = read_settings(&path);
        assert_eq!(saved.ai_news_enabled, Some(false));
        assert_eq!(saved.ai_career_enabled, Some(false));
        assert_eq!(saved.local_llm_enabled, Some(false));
        let mut update = super::AppSettings {
            locale: Some("en".into()),
            ..Default::default()
        };
        super::preserve_protection_setting(&path, &mut update).unwrap();
        let value = serde_json::to_value(&update).unwrap();
        for key in ["aiNewsEnabled", "aiCareerEnabled", "localLlmEnabled"] {
            assert_eq!(value[key], false);
        }
        update.ai_news_enabled = Some(true);
        super::preserve_protection_setting(&path, &mut update).unwrap();
        assert_eq!(update.ai_news_enabled, Some(true));
        assert_eq!(update.ai_career_enabled, Some(false));
        assert_eq!(std::fs::read(&path).unwrap(), original);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn developer_mode_defaults_on_and_preserves_explicit_choices() {
        let root =
            std::env::temp_dir().join(format!("developer-settings-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let path = root.join("settings.json");
        assert!(read_settings(&path).developer_mode_enabled());
        assert!(!path.exists());
        std::fs::write(&path, r#"{"locale":"ja"}"#).unwrap();
        assert!(read_settings(&path).developer_mode_enabled());
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            r#"{"locale":"ja"}"#
        );
        std::fs::write(&path, r#"{"developerMode":true,"locale":"ja"}"#).unwrap();
        assert_eq!(read_settings(&path).developer_mode, Some(true));
        let mut update = super::AppSettings {
            locale: Some("en".into()),
            ..Default::default()
        };
        super::preserve_protection_setting(&path, &mut update).unwrap();
        assert_eq!(update.developer_mode, Some(true));
        update.developer_mode = Some(false);
        super::preserve_protection_setting(&path, &mut update).unwrap();
        assert_eq!(
            serde_json::to_value(update).unwrap()["developerMode"],
            false
        );
        std::fs::write(&path, r#"{"developerMode":false}"#).unwrap();
        assert!(!read_settings(&path).developer_mode_enabled());
        let mut update = super::AppSettings::default();
        super::preserve_protection_setting(&path, &mut update).unwrap();
        assert!(!update.developer_mode_enabled());
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(root).unwrap();
    }

    #[test]
    fn developer_preview_is_the_master_switch_and_legacy_statistics_flags_are_ignored() {
        let root =
            std::env::temp_dir().join(format!("statistics-settings-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let path = root.join("settings.json");
        assert!(read_settings(&path).developer_mode_enabled());
        assert!(!path.exists());
        std::fs::write(
            &path,
            r#"{"routerStatistics":false,"sensitiveInfoProtection":false,"developerMode":true}"#,
        )
        .unwrap();
        let mut update = super::AppSettings {
            locale: Some("ja".into()),
            ..Default::default()
        };
        super::preserve_protection_setting(&path, &mut update).unwrap();
        assert_eq!(update.developer_mode, Some(true));
        assert_eq!(update.sensitive_info_protection, Some(false));
        assert!(serde_json::to_value(&update)
            .unwrap()
            .get("routerStatistics")
            .is_none());
        update.developer_mode = Some(false);
        super::preserve_protection_setting(&path, &mut update).unwrap();
        assert_eq!(update.developer_mode, Some(false));
        std::fs::write(&path, r#"{"routerStatistics":true,"developerMode":false}"#).unwrap();
        assert_eq!(read_settings(&path).developer_mode, Some(false));
        let original = std::fs::read(&path).unwrap();
        let settings = read_settings(&path);
        assert!(serde_json::to_value(settings)
            .unwrap()
            .get("routerStatistics")
            .is_none());
        assert_eq!(
            std::fs::read(&path).unwrap(),
            original,
            "no passive migration writes"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn protection_is_opt_in_and_other_settings_cannot_drop_the_saved_flag() {
        let root =
            std::env::temp_dir().join(format!("protection-setting-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let path = root.join("settings.json");
        let default = super::AppSettings::default();
        assert!(serde_json::to_value(default)
            .unwrap()
            .get("sensitiveInfoProtection")
            .is_none());
        std::fs::write(&path, r#"{"sensitiveInfoProtection":true,"locale":"ja"}"#).unwrap();
        let mut update = super::AppSettings {
            locale: Some("en".into()),
            ..Default::default()
        };
        super::preserve_protection_setting(&path, &mut update).unwrap();
        assert_eq!(update.sensitive_info_protection, Some(true));
        update.sensitive_info_protection = Some(false);
        super::preserve_protection_setting(&path, &mut update).unwrap();
        assert_eq!(update.sensitive_info_protection, Some(false));
        std::fs::write(&path, "invalid settings").unwrap();
        assert!(super::preserve_protection_setting(&path, &mut update).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "invalid settings");
        std::fs::write(&path, r#"{"sensitiveInfoProtection":null}"#).unwrap();
        assert!(super::preserve_protection_setting(&path, &mut update).is_err());
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(root).unwrap();
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
