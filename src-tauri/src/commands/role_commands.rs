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
        "1.jpg".into(), "2.jpg".into(), "3.jpg".into(), "4.jpg".into(),
        "5.jpg".into(), "6.jpg".into(), "7.jpg".into(), "8.jpg".into(),
        "9.png".into(), "10.png".into(), "11.jpg".into(), "12.png".into(),
        "13.jpg".into(), "14.jpg".into(), "15.jpg".into(), "16.jpg".into(),
        "17.jpg".into(),
    ]
}

// ── Roles directory resolution ──

fn roles_dir() -> PathBuf {
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."));

    let mut candidates = vec![
        exe_dir.join("roles"),                                 // release: next to exe
        exe_dir.join("_up_").join("roles"),                   // Tauri NSIS (Windows): resources in _up_/
    ];

    // Walk up from exe dir (handles debug/release nesting, macOS app bundle, Linux AppImage)
    if let Some(p1) = exe_dir.parent() {
        candidates.push(p1.join("roles"));                     // one up
        candidates.push(p1.join("_up_").join("roles"));       // one up + _up_
        candidates.push(p1.join("Resources").join("roles"));  // macOS .app: Contents/Resources/roles
        if let Some(p2) = p1.parent() {
            candidates.push(p2.join("roles"));                 // two up (tauri dev)
            candidates.push(p2.join("Resources").join("roles"));
            if let Some(p3) = p2.parent() {
                candidates.push(p3.join("roles"));             // three up (src-tauri/target/debug → project root)
            }
        }
    }

    // Dev mode: use compile-time project path (handles custom build output dirs)
    // CARGO_MANIFEST_DIR = src-tauri/, so parent = project root
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    if let Some(project_root) = manifest_dir.parent() {
        candidates.push(project_root.join("roles"));
    }

    candidates.push(PathBuf::from("roles"));                   // CWD fallback

    for candidate in &candidates {
        if candidate.exists() {
            log::info!("[Roles] Found roles dir: {:?}", candidate);
            return candidate.clone();
        }
    }

    log::warn!("[Roles] No roles dir found. Searched: {:?}", candidates);
    candidates[0].clone()
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
