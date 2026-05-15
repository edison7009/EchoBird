// Window lifecycle commands

/// App ready — show the main window. Called by the frontend (App.tsx) after
/// React has mounted and scanTools() has resolved, so the WebView has already
/// painted the inline #boot-splash from index.html.
#[tauri::command]
pub async fn app_ready(app: tauri::AppHandle) {
    #[cfg(not(target_os = "android"))]
    {
        use tauri::Manager;
        if let Some(main) = app.get_webview_window("main") {
            // Re-center right before show(): on Linux (GNOME/Wayland), the
            // initial `center: true` is dropped because the compositor ignores
            // client positioning until the window is mapped.
            let _ = main.center();
            let _ = main.show();
            let _ = main.set_focus();
        }
    }
    #[cfg(target_os = "android")]
    {
        let _ = app;
    }
}

/// Read the last `lines` lines from EchoBird's log file. Used by the
/// Feedback page's "copy log tail" button so users can paste recent
/// backend logs straight into a GitHub issue.
///
/// Resolves the log file via Tauri's path API to match the
/// `tauri_plugin_log::TargetKind::LogDir` configuration in `lib.rs`
/// (`<app_log_dir>/echobird.log`).
///
/// Returns the lines as a single newline-joined string (or an empty
/// string when the log file doesn't exist yet — fresh installs before
/// any log has been written).
#[tauri::command]
pub async fn read_log_tail(app: tauri::AppHandle, lines: usize) -> Result<String, String> {
    use tauri::Manager;

    let log_dir = app
        .path()
        .app_log_dir()
        .map_err(|e| format!("Could not resolve app log dir: {e}"))?;
    let log_path = log_dir.join("echobird.log");

    if !log_path.exists() {
        return Ok(String::new());
    }

    let contents = std::fs::read_to_string(&log_path)
        .map_err(|e| format!("Failed to read log file ({}): {e}", log_path.display()))?;

    // Tail the last `lines` non-empty lines. Cheap split — log file
    // size is bounded by tauri-plugin-log's rotation; we don't need
    // mmap or reverse seek for the volumes we see in practice.
    let tail: Vec<&str> = contents
        .lines()
        .filter(|l| !l.trim().is_empty())
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .take(lines)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();

    Ok(tail.join("\n"))
}
