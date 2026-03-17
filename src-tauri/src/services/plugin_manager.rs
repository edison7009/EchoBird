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
    pub bridge: Option<BridgePaths>,
    pub cli: Option<CliConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BridgePaths {
    pub linux: Option<String>,
    pub darwin: Option<String>,
    pub win32: Option<String>,
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

/// Scan plugins/ directory and return all valid plugin configs
pub fn scan_plugins() -> Vec<PluginConfig> {
    let dir = plugins_dir();
    if !dir.exists() {
        log::info!("[PluginManager] No plugins directory found at {:?}", dir);
        return Vec::new();
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

    log::info!("[PluginManager] Scanned {} plugins", plugins.len());
    plugins
}

/// Get the bridge/ directory path (central bridge binaries for all platforms)
pub fn bridge_dir() -> PathBuf {
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."));

    let mut candidates = vec![
        exe_dir.join("bridge"),
        exe_dir.join("_up_").join("bridge"),
    ];

    if let Some(p1) = exe_dir.parent() {
        candidates.push(p1.join("bridge"));
        candidates.push(p1.join("_up_").join("bridge"));
        candidates.push(p1.join("Resources").join("bridge"));
        if let Some(p2) = p1.parent() {
            candidates.push(p2.join("bridge"));
            candidates.push(p2.join("Resources").join("bridge"));
            if let Some(p3) = p2.parent() {
                candidates.push(p3.join("bridge"));
            }
        }
    }

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    if let Some(project_root) = manifest_dir.parent() {
        candidates.push(project_root.join("bridge"));
    }

    candidates.push(PathBuf::from("bridge"));

    for candidate in &candidates {
        if candidate.exists() {
            log::info!("[PluginManager] Found bridge dir: {:?}", candidate);
            return candidate.clone();
        }
    }

    candidates[0].clone()
}

/// Get the bridge binary path for the current platform.
/// Checks bridge/ directory first (with arch-specific names), then plugins/{id}/.
pub fn get_bridge_path(plugin: &PluginConfig) -> Option<PathBuf> {
    // 1. Check bridge/ directory with arch-specific binary name
    let arch_name = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "aarch64") => "bridge-linux-aarch64",
        ("linux", _)         => "bridge-linux-x86_64",
        ("macos", "aarch64") => "bridge-darwin-aarch64",
        ("macos", _)         => "bridge-darwin-x86_64",
        _                    => "bridge-win.exe",
    };
    let bridge_dir_path = bridge_dir().join(arch_name);
    if bridge_dir_path.exists() {
        log::info!("[PluginManager] Using bridge from bridge/: {:?}", bridge_dir_path);
        return Some(bridge_dir_path);
    }

    // 2. Fallback: check plugins/{id}/ with plugin.json-defined name
    let bridge = plugin.bridge.as_ref()?;
    let filename = if cfg!(target_os = "linux") {
        bridge.linux.as_ref()
    } else if cfg!(target_os = "macos") {
        bridge.darwin.as_ref()
    } else if cfg!(target_os = "windows") {
        bridge.win32.as_ref()
    } else {
        None
    }?;

    let dir = plugins_dir();
    let path = dir.join(&plugin.id).join(filename);
    if path.exists() {
        Some(path)
    } else {
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
