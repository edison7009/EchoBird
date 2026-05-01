// Tauri Commands for tool operations — exposed to frontend via invoke()

use crate::models::tool::DetectedTool;
use crate::services::tool_config_manager::{self, ApplyResult, ModelInfo};
use crate::services::tool_manager;

// ── LLM Proxy (bypass browser CORS for tool windows) ──

/// Proxy an OpenAI-compatible chat request from a tool window (no CORS involved).
/// Called by reversi.html and other tool pages via window.__TAURI__.core.invoke().
#[tauri::command]
pub async fn llm_proxy_chat(
    url: String,
    api_key: String,
    body: serde_json::Value,
    anthropic_version: Option<String>,
) -> Result<serde_json::Value, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .map_err(|e| format!("HTTP client error: {}", e))?;

    let mut req = client.post(&url)
        .header("Content-Type", "application/json");

    if let Some(av) = &anthropic_version {
        // Anthropic protocol
        req = req
            .header("x-api-key", &api_key)
            .header("anthropic-version", av);
    } else {
        // OpenAI-compatible protocol
        req = req.header("Authorization", format!("Bearer {}", api_key));
    }

    let resp = req
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?;

    let status = resp.status().as_u16();
    let json = resp
        .json::<serde_json::Value>()
        .await
        .map_err(|e| format!("Failed to read response: {}", e))?;

    if status >= 400 {
        return Err(format!("API {} — {}", status,
            json.get("error").and_then(|e| e.get("message")).and_then(|m| m.as_str()).unwrap_or("unknown")));
    }

    Ok(json)
}

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

/// Restore a tool to its official defaults by deleting its config file. The
/// tool will regenerate a default config (pointing at its vendor endpoint) on
/// next launch.
#[tauri::command]
pub async fn restore_tool_to_official(tool_id: String) -> Result<ApplyResult, String> {
    Ok(tool_config_manager::restore_tool_to_official(&tool_id).await)
}

/// Launch a built-in tool (game/utility) in a new WebView window
#[tauri::command]
pub async fn launch_game(
    app_handle: tauri::AppHandle,
    tool_id: String,
    _launch_file: String,
    model_config: Option<serde_json::Value>,
) -> Result<serde_json::Value, String> {
    #[cfg(not(target_os = "android"))]
    {
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

        let app_path = format!("tools/{}.html", tool_id);

        let init_script = {
            // Read current app locale from settings (falls back to empty = browser default)
            let locale = crate::commands::settings_commands::get_settings()
                .locale
                .unwrap_or_default();

            let mut script = format!("window.__APP_LOCALE__ = {:?};", locale);

            if let Some(mut config) = model_config {
                // Decrypt API key if it's encrypted (frontend stores encrypted keys as enc:v1:...)
                // Without this, the tool window receives an encrypted key and gets 401 from all APIs.
                if let Some(api_key_val) = config.get("apiKey").and_then(|v| v.as_str()) {
                    let decrypted = crate::services::model_manager::decrypt_key_for_use(api_key_val);
                    if !decrypted.is_empty() {
                        config["apiKey"] = serde_json::Value::String(decrypted);
                    }
                }
                script.push_str(&format!("\nwindow.__MODEL_CONFIG__ = {};", config.to_string()));
            }
            script
        };


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

        if !init_script.is_empty() {
            builder = builder.initialization_script(&init_script);
        }

        let _window = builder
            .build()
            .map_err(|e| format!("Failed to create window: {}", e))?;

        log::info!("[ToolCommands] Launched {} in new window", tool_id);
        Ok(serde_json::json!({ "success": true }))
    }
    #[cfg(target_os = "android")]
    {
        let _ = (app_handle, tool_id, _launch_file, model_config);
        Err("Not available on mobile".to_string())
    }
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

    #[cfg(target_os = "android")]
    {
        let _ = path;
        return Err("Not available on mobile".to_string());
    }

    #[cfg(not(target_os = "android"))]
    Ok(())
}
