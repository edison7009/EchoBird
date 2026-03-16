// Role metadata — reads pre-built JSON files (roles-en.json, roles-zh-Hans.json)
// No directory scanning, no frontmatter parsing — just deserialize and return.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

// ── Types ──

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoleCategory {
    pub id: String,        // directory name: "engineering"
    pub label: String,     // display: "工程部"
    pub order: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoleEntry {
    pub id: String,            // "engineering-frontend-developer"
    pub name: String,          // "前端开发者"
    pub description: String,   // from frontmatter
    pub category: String,      // "engineering"
    pub file_path: String,     // "engineering/engineering-frontend-developer.md"
    pub img: Option<String>,   // CDN image URL
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fallback_img: Option<String>, // local placeholder fallback (added at runtime)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoleScanResult {
    pub categories: Vec<RoleCategory>,
    pub roles: Vec<RoleEntry>,
    #[serde(default)]
    pub locale: String,
    pub all_label: String,
}

// ── Placeholder images (cycle through for fallback) ──

fn placeholder_images() -> Vec<String> {
    vec![
        "1119642287_IGDB-285x380.jpg".into(),
        "116747788-285x380.png".into(),
        "1329153872_IGDB-285x380.jpg".into(),
        "1435206302_IGDB-285x380.jpg".into(),
        "1597660489_IGDB-285x380.jpg".into(),
        "1630982727_IGDB-285x380.jpg".into(),
        "21779-285x380.jpg".into(),
        "29307_IGDB-285x380.jpg".into(),
        "509538_IGDB-285x380.jpg".into(),
        "509658-285x380.jpg".into(),
        "511224-285x380.jpg".into(),
        "512864_IGDB-285x380.jpg".into(),
        "513181_IGDB-285x380.jpg".into(),
        "515025-285x380.png".into(),
        "516575-285x380.png".into(),
        "55453844_IGDB-285x380.jpg".into(),
        "66082-285x380.jpg".into(),
        "66366_IGDB-285x380.jpg".into(),
    ]
}

// ── Roles directory resolution ──

fn roles_dir() -> PathBuf {
    let candidates = [
        // Dev: workspace root
        PathBuf::from("roles"),
        // Tauri bundle: next to executable
        std::env::current_exe()
            .unwrap_or_default()
            .parent()
            .unwrap_or(std::path::Path::new("."))
            .join("roles"),
        // macOS .app bundle
        std::env::current_exe()
            .unwrap_or_default()
            .parent()
            .unwrap_or(std::path::Path::new("."))
            .parent()
            .unwrap_or(std::path::Path::new("."))
            .join("Resources")
            .join("roles"),
    ];

    for candidate in &candidates {
        if candidate.exists() {
            return candidate.clone();
        }
    }

    PathBuf::from("roles")
}

// ── Scan Command — reads JSON ──

/// Load roles from pre-built JSON file for the given locale
#[tauri::command]
pub fn scan_roles(locale: String) -> RoleScanResult {
    let base = roles_dir();
    let file_name = if locale.starts_with("zh") { "roles-zh-Hans.json" } else { "roles-en.json" };
    let json_path = base.join(file_name);

    log::info!("[Roles] Loading from {:?}", json_path);

    let content = match std::fs::read_to_string(&json_path) {
        Ok(c) => c,
        Err(e) => {
            log::error!("[Roles] Failed to read {}: {}", file_name, e);
            return RoleScanResult {
                categories: vec![],
                roles: vec![],
                locale: locale.clone(),
                all_label: if locale.starts_with("zh") { "全部".to_string() } else { "All".to_string() },
            };
        }
    };

    let mut result: RoleScanResult = match serde_json::from_str(&content) {
        Ok(r) => r,
        Err(e) => {
            log::error!("[Roles] Failed to parse {}: {}", file_name, e);
            return RoleScanResult {
                categories: vec![],
                roles: vec![],
                locale: locale.clone(),
                all_label: "All".to_string(),
            };
        }
    };

    // Set locale
    result.locale = if locale.starts_with("zh") { "zh-Hans".to_string() } else { "en".to_string() };

    // Add fallback images (cycling placeholder)
    let images = placeholder_images();
    for (i, role) in result.roles.iter_mut().enumerate() {
        role.fallback_img = Some(format!("/role/{}", images[i % images.len()]));
    }

    log::info!("[Roles] Loaded {}: {} categories, {} roles", file_name, result.categories.len(), result.roles.len());
    result
}

/// Load the full content of a role .md file (for system_prompt injection)
#[tauri::command]
pub fn load_role_content(locale: String, file_path: String) -> Result<String, String> {
    let base = roles_dir();
    let dir_name = if locale.starts_with("zh") { "zh-Hans" } else { "en" };
    let full_path = base.join(dir_name).join(&file_path);

    if !full_path.exists() {
        return Err(format!("Role file not found: {}", file_path));
    }

    std::fs::read_to_string(&full_path)
        .map_err(|e| format!("Failed to read role file: {}", e))
}

// ── Local Agent Detection ──

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentStatus {
    pub id: String,
    pub name: String,
    pub installed: bool,
    pub path: Option<String>,
}

/// Detect which Agent CLI tools are installed on this machine
#[tauri::command]
pub fn detect_local_agents() -> Vec<AgentStatus> {
    let agents = [
        ("claudecode", "Claude Code", "claude"),
        ("opencode",   "OpenCode",    "opencode"),
        ("openclaw",   "OpenClaw",    "openclaw"),
        ("zeroclaw",   "ZeroClaw",    "zeroclaw"),
    ];

    agents.iter().map(|&(id, name, cmd)| {
        let (installed, path) = check_command_installed(cmd);
        AgentStatus {
            id: id.to_string(),
            name: name.to_string(),
            installed,
            path,
        }
    }).collect()
}

fn check_command_installed(cmd: &str) -> (bool, Option<String>) {
    let result = if cfg!(target_os = "windows") {
        std::process::Command::new("where.exe").arg(cmd).output()
    } else {
        std::process::Command::new("which").arg(cmd).output()
    };

    match result {
        Ok(output) if output.status.success() => {
            let path = String::from_utf8_lossy(&output.stdout)
                .lines().next().unwrap_or("").trim().to_string();
            if path.is_empty() { (false, None) } else { (true, Some(path)) }
        }
        _ => (false, None),
    }
}
