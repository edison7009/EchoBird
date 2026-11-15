//! Built-in utility windows share the selected model and application locale.
use tauri::Manager;

#[tauri::command]
pub async fn launch_game(
    app_handle: tauri::AppHandle,
    tool_id: String,
    launch_file: String,
    model_config: Option<serde_json::Value>,
) -> Result<serde_json::Value, String> {
    let (width, height, title) = match tool_id.as_str() {
        "reversi" => (860.0, 680.0, "Reversi"),
        "translator" => (800.0, 560.0, "AI Translate"),
        _ => return Err("Unknown built-in tool".into()),
    };
    if launch_file != "game.html" {
        return Err("Invalid built-in launch file".into());
    }
    let label = format!("tool-{tool_id}");
    let locale = crate::commands::settings_commands::get_settings()
        .locale
        .unwrap_or_default();
    let mut script = format!(
        "window.__APP_LOCALE__ = {};",
        serde_json::to_string(&locale).map_err(|e| e.to_string())?
    );
    if let Some(mut configuration) = model_config {
        if let Some(key) = configuration.get("apiKey").and_then(|v| v.as_str()) {
            configuration["apiKey"] =
                crate::services::model_manager::decrypt_key_for_use(key).into();
        }
        script.push_str(&format!("window.__MODEL_CONFIG__ = {configuration};"));
    }
    if let Some(existing) = app_handle.get_webview_window(&label) {
        existing
            .eval(format!(
                "{script} if (window.__MODEL_CONFIG__) MODEL = window.__MODEL_CONFIG__;"
            ))
            .map_err(|e| e.to_string())?;
        existing.show().map_err(|e| e.to_string())?;
        existing.set_focus().map_err(|e| e.to_string())?;
        return Ok(serde_json::json!({"success":true}));
    }
    tauri::WebviewWindowBuilder::new(
        &app_handle,
        label,
        tauri::WebviewUrl::App(format!("tools/{tool_id}.html").into()),
    )
    .title(title)
    .inner_size(width, height)
    .resizable(true)
    .decorations(false)
    .center()
    .initialization_script(script)
    .build()
    .map_err(|e| e.to_string())?;
    Ok(serde_json::json!({"success":true}))
}
