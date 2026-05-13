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
    #[serde(default, rename = "closeToTray")]
    pub close_to_tray: Option<bool>,
    #[serde(default, rename = "closeWindowBehaviorSet")]
    pub close_window_behavior_set: Option<bool>,
}

fn settings_path() -> std::path::PathBuf {
    echobird_dir().join("settings.json")
}

/// Read app settings from ~/.echobird/settings.json
#[tauri::command]
pub fn get_settings() -> AppSettings {
    let path = settings_path();
    if !path.exists() {
        return AppSettings::default();
    }
    match std::fs::read_to_string(&path) {
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
