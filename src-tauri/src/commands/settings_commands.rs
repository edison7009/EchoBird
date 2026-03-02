use serde::{Deserialize, Serialize};
use crate::utils::platform::echobird_dir;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppSettings {
    #[serde(default)]
    pub locale: Option<String>,
    #[serde(default, rename = "closeBehavior")]
    pub close_behavior: Option<String>,
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
        Err(_) => AppSettings::default(),
    }
}

/// Write app settings to ~/.echobird/settings.json
#[tauri::command]
pub fn save_settings(settings: AppSettings) -> Result<(), String> {
    let dir = echobird_dir();
    if !dir.exists() {
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(&settings).map_err(|e| e.to_string())?;
    std::fs::write(settings_path(), json).map_err(|e| e.to_string())
}
