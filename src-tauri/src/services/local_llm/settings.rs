// Model settings persistence (~/.echobird/config/local-model-settings.json)

use super::types::{GgufFile, HfModelEntry, ModelSettings};
use std::path::PathBuf;

fn settings_path() -> PathBuf {
    crate::utils::platform::echobird_dir()
        .join("config")
        .join("local-model-settings.json")
}

pub fn load_model_settings() -> ModelSettings {
    let path = settings_path();
    if path.exists() {
        if let Ok(content) = std::fs::read_to_string(&path) {
            if let Ok(settings) = serde_json::from_str(&content) {
                return settings;
            }
        }
    }
    ModelSettings::default()
}

pub fn save_model_settings(settings: &ModelSettings) {
    let path = settings_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let content = serde_json::to_string_pretty(settings).unwrap_or_default();
    let _ = std::fs::write(path, content);
}

/// Get model directories
pub fn get_models_dirs() -> Vec<String> {
    let settings = load_model_settings();
    if !settings.models_dirs.is_empty() {
        return settings.models_dirs;
    }
    let default_dir = dirs::home_dir()
        .unwrap_or_default()
        .join("Models")
        .to_string_lossy()
        .to_string();
    vec![default_dir]
}

/// Get download directory
pub fn get_download_dir() -> String {
    let settings = load_model_settings();
    if let Some(dir) = settings.download_dir {
        return dir;
    }
    dirs::home_dir()
        .unwrap_or_default()
        .join("Models")
        .to_string_lossy()
        .to_string()
}

/// Set download directory
pub fn set_download_dir(dir: &str) {
    let mut settings = load_model_settings();
    settings.download_dir = Some(dir.to_string());
    save_model_settings(&settings);
}

/// Scan for GGUF files in model directories
pub fn scan_gguf_files(dir: &str, max_depth: u32) -> Vec<GgufFile> {
    let mut results = Vec::new();
    scan_gguf_recursive(std::path::Path::new(dir), max_depth, &mut results);
    results
}

fn scan_gguf_recursive(dir: &std::path::Path, depth: u32, results: &mut Vec<GgufFile>) {
    if depth == 0 || !dir.is_dir() {
        return;
    }

    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                if let Some(ext) = path.extension() {
                    if ext.to_string_lossy().to_lowercase() == "gguf" {
                        let file_size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                        results.push(GgufFile {
                            file_name: path
                                .file_name()
                                .map(|n| n.to_string_lossy().to_string())
                                .unwrap_or_default(),
                            file_path: path.to_string_lossy().to_string(),
                            file_size,
                        });
                    }
                }
            } else if path.is_dir() {
                scan_gguf_recursive(&path, depth - 1, results);
            }
        }
    }
}

/// Scan for HuggingFace model directories (folders containing config.json)
pub fn scan_hf_models(dir: &str, max_depth: u32) -> Vec<HfModelEntry> {
    let mut results = Vec::new();
    scan_hf_recursive(std::path::Path::new(dir), max_depth, &mut results);
    results
}

fn scan_hf_recursive(dir: &std::path::Path, depth: u32, results: &mut Vec<HfModelEntry>) {
    if depth == 0 || !dir.is_dir() {
        return;
    }

    let config_path = dir.join("config.json");
    if config_path.exists() {
        let model_name = std::fs::read_to_string(&config_path)
            .ok()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .and_then(|v| {
                v.get("_name_or_path")
                    .and_then(|n| n.as_str().map(String::from))
                    .or_else(|| {
                        v.get("model_type")
                            .and_then(|n| n.as_str().map(String::from))
                    })
            })
            .unwrap_or_else(|| {
                dir.file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default()
            });

        let total_size = std::fs::read_dir(dir)
            .map(|entries| {
                entries
                    .flatten()
                    .filter(|e| e.path().is_file())
                    .filter(|e| {
                        e.path()
                            .extension()
                            .map(|ext| {
                                let ext = ext.to_string_lossy().to_lowercase();
                                ext == "safetensors" || ext == "bin" || ext == "pt"
                            })
                            .unwrap_or(false)
                    })
                    .map(|e| e.metadata().map(|m| m.len()).unwrap_or(0))
                    .sum()
            })
            .unwrap_or(0);

        results.push(HfModelEntry {
            model_name,
            model_path: dir.to_string_lossy().to_string(),
            total_size,
        });
        return;
    }

    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            if entry.path().is_dir() {
                scan_hf_recursive(&entry.path(), depth - 1, results);
            }
        }
    }
}
