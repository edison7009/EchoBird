use crate::services::usage_island;
use crate::services::usage_island_tooltip;
use tauri::{AppHandle, WebviewWindow};

#[tauri::command]
pub async fn usage_island_tooltip_ready(
    app: AppHandle,
    window: WebviewWindow,
    revision: u64,
) -> Result<(), String> {
    if window.label() != usage_island_tooltip::WINDOW_LABEL {
        return Err("Only the usage tooltip can confirm its snapshot".into());
    }
    usage_island_tooltip::ready(&app, revision)
}

#[tauri::command]
pub async fn usage_island_tooltip(
    app: AppHandle,
    window: WebviewWindow,
    tooltip: Option<usage_island_tooltip::Request>,
) -> Result<(), String> {
    if window.label() != usage_island::WINDOW_LABEL {
        return Err("Only the usage island can show its tooltip".into());
    }
    usage_island_tooltip::show(&app, &window, tooltip)
}

#[tauri::command]
pub fn usage_island_tooltip_state(
    window: WebviewWindow,
) -> Result<Option<usage_island_tooltip::Snapshot>, String> {
    if window.label() != usage_island_tooltip::WINDOW_LABEL {
        return Err("Only the usage tooltip can read its snapshot".into());
    }
    Ok(usage_island_tooltip::snapshot())
}

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
    anchor_x: f64,
    anchor_y: f64,
) -> Result<(), String> {
    if window.label() != usage_island::WINDOW_LABEL {
        return Err("Only the usage island can resize itself".into());
    }
    if !width.is_finite() || !height.is_finite() || width < 1.0 || height < 1.0 {
        return Err("Invalid usage island size".into());
    }
    usage_island::resize(
        &window,
        width,
        height,
        (anchor_x.clamp(0.0, 1.0), anchor_y.clamp(0.0, 1.0)),
    )
}

#[tauri::command]
pub async fn usage_island_set_hit_area(
    window: WebviewWindow,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    radius: Option<f64>,
) -> Result<(), String> {
    if window.label() != usage_island::WINDOW_LABEL {
        return Err("Only the usage island can set its hit area".into());
    }
    let radius = radius.unwrap_or(width.min(height) / 2.0);
    if ![x, y, width, height, radius]
        .iter()
        .all(|value| value.is_finite())
        || width <= 0.0
        || height <= 0.0
        || radius < 0.0
    {
        return Err("Invalid usage island hit area".into());
    }
    usage_island::set_hit_area(
        &window,
        usage_island::HitArea {
            x,
            y,
            width,
            height,
            radius,
        },
    );
    Ok(())
}
