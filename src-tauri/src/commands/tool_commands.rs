// Tauri Commands for tool operations �?exposed to frontend via invoke()

use crate::models::tool::DetectedTool;
use crate::services::tool_config_manager::{self, ApplyResult, ModelInfo};
use crate::services::tool_manager;

/// Scan all installed tools and return detection results
#[tauri::command]
pub async fn scan_tools() -> Result<Vec<DetectedTool>, String> {
    Ok(tool_manager::scan_tools().await)
}

/// Get current model info for a specific tool
#[tauri::command]
pub async fn get_tool_model_info(tool_id: String) -> Result<Option<ModelInfo>, String> {
    Ok(tool_config_manager::get_tool_model_info(&tool_id).await)
}

/// Apply a model configuration to a tool
#[tauri::command]
pub async fn apply_model_to_tool(tool_id: String, model_info: ModelInfo) -> Result<ApplyResult, String> {
    // Decrypt API key if it's encrypted (frontend stores encrypted keys)
    let mut info = model_info;
    if let Some(ref encrypted_key) = info.api_key {
        let decrypted = crate::services::model_manager::decrypt_key_for_use(encrypted_key);
        if !decrypted.is_empty() {
            info.api_key = Some(decrypted);
        }
    }
    Ok(tool_config_manager::apply_model_to_tool(&tool_id, info).await)
}

/// Launch a built-in tool (game/utility) in a new WebView window
#[tauri::command]
pub async fn launch_game(
    app_handle: tauri::AppHandle,
    tool_id: String,
    launch_file: String,
    model_config: Option<serde_json::Value>,
) -> Result<serde_json::Value, String> {
    use tauri::Manager;

    let window_label = format!("tool-{}", tool_id);

    // If window already exists, just focus it
    if let Some(existing) = app_handle.get_webview_window(&window_label) {
        let _ = existing.show();
        let _ = existing.set_focus();
        return Ok(serde_json::json!({ "success": true }));
    }

    // Determine window size based on tool
    let (width, height, title) = match tool_id.as_str() {
        "reversi" => (860.0, 680.0, "Reversi"),
        "translator" => (800.0, 560.0, "AI Translate"),
        _ => (800.0, 600.0, "Tool"),
    };

    // Build path relative to frontend assets (public/tools/)
    // Use tool_id as filename �?actual files are tools/{tool_id}.html
    let app_path = format!("tools/{}.html", tool_id);

    // Prepare initialization script to inject model config BEFORE page scripts run
    let init_script = if let Some(config) = model_config {
        format!("window.__MODEL_CONFIG__ = {};", config.to_string())
    } else {
        String::new()
    };

    // Create new WebView window using App URL (served by Vite in dev, bundled in production)
    let mut builder = tauri::WebviewWindowBuilder::new(
        &app_handle,
        &window_label,
        tauri::WebviewUrl::App(app_path.into()),
    )
    .title(title)
    .inner_size(width, height)
    .resizable(true)
    .decorations(false)
    .center();

    // initialization_script runs BEFORE any page JavaScript
    if !init_script.is_empty() {
        builder = builder.initialization_script(&init_script);
    }

    let _window = builder
        .build()
        .map_err(|e| format!("Failed to create window: {}", e))?;

    log::info!("[ToolCommands] Launched {} in new window", tool_id);
    Ok(serde_json::json!({ "success": true }))
}

/// List installed skills from a tool's skills directory
/// Note: frontend passes the skills directory path as `tool_id` parameter
#[tauri::command]
pub async fn get_tool_installed_skills(tool_id: String) -> Result<Vec<serde_json::Value>, String> {
    let skills_dir = std::path::Path::new(&tool_id);
    if !skills_dir.exists() || !skills_dir.is_dir() {
        return Ok(vec![]);
    }

    let mut skills = Vec::new();
    let entries = std::fs::read_dir(skills_dir)
        .map_err(|e| format!("Failed to read skills dir: {}", e))?;

    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        if !path.is_dir() { continue; }

        let dir_name = entry.file_name().to_string_lossy().to_string();
        if dir_name.starts_with('.') { continue; }

        // Try to read skill name from package.json or .prompt file
        let display_name = read_skill_name(&path).unwrap_or_else(|| dir_name.clone());

        skills.push(serde_json::json!({
            "id": dir_name,
            "name": display_name,
            "path": path.to_string_lossy().to_string(),
        }));
    }

    skills.sort_by(|a, b| {
        let na = a["name"].as_str().unwrap_or("");
        let nb = b["name"].as_str().unwrap_or("");
        na.to_lowercase().cmp(&nb.to_lowercase())
    });

    Ok(skills)
}

/// Read skill display name from package.json or .md/.prompt file
/// Falls back to directory name if only generic files exist (e.g. SKILL.md)
fn read_skill_name(skill_dir: &std::path::Path) -> Option<String> {
    // Try package.json
    let pkg = skill_dir.join("package.json");
    if pkg.exists() {
        if let Ok(content) = std::fs::read_to_string(&pkg) {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
                if let Some(name) = json.get("name").and_then(|v| v.as_str()) {
                    if !name.is_empty() {
                        return Some(name.to_string());
                    }
                }
            }
        }
    }
    // Try .prompt/.md file �?skip generic names like SKILL.md
    if let Ok(entries) = std::fs::read_dir(skill_dir) {
        for entry in entries.filter_map(|e| e.ok()) {
            let fname = entry.file_name().to_string_lossy().to_string();
            if fname.ends_with(".md") || fname.ends_with(".prompt") {
                let stem = fname.trim_end_matches(".md").trim_end_matches(".prompt");
                // Skip generic names �?use directory name instead
                if !stem.eq_ignore_ascii_case("skill")
                    && !stem.eq_ignore_ascii_case("readme")
                    && !stem.eq_ignore_ascii_case("index")
                {
                    return Some(stem.to_string());
                }
            }
        }
    }
    None
}

/// Open a folder in the system file manager
#[tauri::command]
pub async fn open_folder(path: String) -> Result<(), String> {
    let p = std::path::Path::new(&path);
    if !p.exists() {
        return Err(format!("Path does not exist: {}", path));
    }

    // Canonicalize to get proper OS path separators (backslashes on Windows)
    let canonical = p.canonicalize()
        .map_err(|e| format!("Failed to resolve path: {}", e))?;
    let resolved = canonical.to_string_lossy().to_string();
    // Strip Windows UNC prefix \\?\ that canonicalize adds
    #[cfg(target_os = "windows")]
    let resolved = resolved.strip_prefix(r"\\?\").unwrap_or(&resolved).to_string();

    log::info!("[OpenFolder] Opening: {}", resolved);

    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(&resolved)
            .spawn()
            .map_err(|e| format!("Failed to open folder: {}", e))?;
    }

    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(&resolved)
            .spawn()
            .map_err(|e| format!("Failed to open folder: {}", e))?;
    }

    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(&resolved)
            .spawn()
            .map_err(|e| format!("Failed to open folder: {}", e))?;
    }

    Ok(())
}
