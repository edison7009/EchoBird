// Plugin Manager — scan plugins/ directory for agent plugins
// Follows the same plug-and-play pattern as tools/

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

// ── Types ──

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginConfig {
    pub id: String,
    pub name: String,
    pub protocol: String, // "stdio-json"
    pub cli: Option<CliConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CliConfig {
    pub command: String,
    pub detect_command: Option<String>,
    pub args: Vec<String>,
    pub resume_args: Option<Vec<String>>,
    pub session_arg: Option<String>,
    pub session_mode: Option<String>, // "always" | "existing" | "none"
    pub model_arg: Option<String>,
    pub system_prompt_arg: Option<String>,
    pub system_prompt_when: Option<String>,
    pub agent_arg: Option<String>,
}

// ── Plugin Scanner ──

pub fn plugins_dir() -> PathBuf {
    // Look for plugins/ in the app resource directory or next to the executable
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."));

    // Build candidates using proper parent traversal
    let mut candidates = vec![
        exe_dir.join("plugins"),                                // release: next to exe
        exe_dir.join("_up_").join("plugins"),                  // Tauri NSIS (Windows): resources in _up_/
    ];

    // Walk up from exe dir (handles debug/release nesting, macOS app bundle, Linux AppImage)
    if let Some(p1) = exe_dir.parent() {
        candidates.push(p1.join("plugins"));                    // one up
        candidates.push(p1.join("_up_").join("plugins"));      // one up + _up_
        candidates.push(p1.join("Resources").join("plugins")); // macOS .app: Contents/Resources/plugins
        if let Some(p2) = p1.parent() {
            candidates.push(p2.join("plugins"));                // two up (tauri dev)
            candidates.push(p2.join("Resources").join("plugins")); // macOS: MacOS/../Resources/plugins
            if let Some(p3) = p2.parent() {
                candidates.push(p3.join("plugins"));            // three up (src-tauri/target/debug → project root)
            }
        }
    }

    // Dev mode: use compile-time project path (handles custom build output dirs)
    // CARGO_MANIFEST_DIR = src-tauri/, so parent = project root
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    if let Some(project_root) = manifest_dir.parent() {
        candidates.push(project_root.join("plugins"));
    }

    candidates.push(PathBuf::from("plugins"));                  // CWD fallback

    for candidate in &candidates {
        if candidate.exists() {
            log::info!("[PluginManager] Found plugins dir: {:?}", candidate);
            return candidate.clone();
        }
    }

    log::warn!("[PluginManager] No plugins dir found. Searched: {:?}", candidates);
    // Default (may not exist yet)
    candidates[0].clone()
}

/// Scan plugins/ directory and return all valid plugin configs.
/// Falls back to compile-time embedded configs when no plugins/ directory exists
/// (e.g. on Android where the filesystem path is not accessible).
pub fn scan_plugins() -> Vec<PluginConfig> {
    let dir = plugins_dir();
    if !dir.exists() {
        log::info!("[PluginManager] No plugins directory found at {:?}, using embedded configs", dir);
        return embedded_plugin_configs();
    }

    let mut plugins = Vec::new();

    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }

            let plugin_json = path.join("plugin.json");
            if !plugin_json.exists() {
                continue;
            }

            match std::fs::read_to_string(&plugin_json) {
                Ok(content) => match serde_json::from_str::<PluginConfig>(&content) {
                    Ok(config) => {
                        log::info!("[PluginManager] Found plugin: {} ({})", config.name, config.id);
                        plugins.push(config);
                    }
                    Err(e) => {
                        log::warn!("[PluginManager] Invalid plugin.json in {:?}: {}", path, e);
                    }
                },
                Err(e) => {
                    log::warn!("[PluginManager] Failed to read {:?}: {}", plugin_json, e);
                }
            }
        }
    }

    if plugins.is_empty() {
        log::info!("[PluginManager] No plugins found on disk, using embedded configs");
        return embedded_plugin_configs();
    }

    log::info!("[PluginManager] Scanned {} plugins", plugins.len());
    plugins
}

/// Embedded plugin configs — compiled into the binary.
/// Used as fallback when plugins/ directory is not accessible (Android, minimal installs).
/// MUST be kept in sync with plugins/*/plugin.json files.
fn embedded_plugin_configs() -> Vec<PluginConfig> {
    let configs: &[&str] = &[
        include_str!("../../../plugins/openclaw/plugin.json"),
        include_str!("../../../plugins/zeroclaw/plugin.json"),
        include_str!("../../../plugins/claudecode/plugin.json"),
        include_str!("../../../plugins/nanobot/plugin.json"),
        include_str!("../../../plugins/picoclaw/plugin.json"),
        include_str!("../../../plugins/hermes/plugin.json"),
    ];
    let mut plugins = Vec::new();
    for json_str in configs {
        match serde_json::from_str::<PluginConfig>(json_str) {
            Ok(config) => {
                log::info!("[PluginManager] Embedded plugin: {} ({})", config.name, config.id);
                plugins.push(config);
            }
            Err(e) => {
                log::warn!("[PluginManager] Failed to parse embedded plugin: {}", e);
            }
        }
    }
    plugins
}


/// Get the full path to a plugin's plugin.json file
pub fn get_plugin_json_path(plugin: &PluginConfig) -> Option<PathBuf> {
    let path = plugins_dir().join(&plugin.id).join("plugin.json");
    if path.exists() {
        Some(path)
    } else {
        log::warn!("[PluginManager] plugin.json not found at {:?}", path);
        None
    }
}

/// Build CLI args for a new chat session
pub fn build_chat_args(plugin: &PluginConfig, message: &str, session_id: Option<&str>) -> Vec<String> {
    let cli = match &plugin.cli {
        Some(c) => c,
        None => return vec![message.to_string()],
    };

    let mut args: Vec<String> = if let (Some(sid), Some(resume_args)) = (session_id, &cli.resume_args) {
        // Resume existing session
        resume_args.iter().map(|a| a.replace("{sessionId}", sid)).collect()
    } else {
        // New session
        cli.args.clone()
    };

    // Add session ID for new sessions if sessionMode is "always"
    if session_id.is_none() && cli.session_mode.as_deref() == Some("always") {
        if let Some(session_arg) = &cli.session_arg {
            let new_id = uuid::Uuid::new_v4().to_string();
            args.push(session_arg.clone());
            args.push(new_id);
        }
    }

    // Add message as last arg
    args.push(message.to_string());

    args
}
