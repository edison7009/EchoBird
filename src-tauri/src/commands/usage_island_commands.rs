use crate::services::usage_island;
use tauri::{AppHandle, WebviewWindow};

// Async keeps these off the main thread: building a window there deadlocks on Windows.
#[tauri::command]
pub async fn usage_island_set_visible(
    app: AppHandle,
    window: WebviewWindow,
    visible: bool,
) -> Result<(), String> {
    if window.label() != "main" {
        return Err("Usage island can only be toggled from the main window".into());
    }
    usage_island::set_visible(&app, visible)
}

#[tauri::command]
pub async fn usage_island_resize(
    window: WebviewWindow,
    width: f64,
    height: f64,
    centered: bool,
) -> Result<(), String> {
    if window.label() != usage_island::WINDOW_LABEL {
        return Err("Only the usage island can resize itself".into());
    }
    if !width.is_finite() || !height.is_finite() || width < 1.0 || height < 1.0 {
        return Err("Invalid usage island size".into());
    }
    usage_island::resize(&window, width, height, centered)
}
