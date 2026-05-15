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

/// Read the last `lines` lines from EchoBird's log files. Used by the
/// Feedback page's "copy log tail" button so users can paste recent
/// backend logs straight into a GitHub issue.
///
/// Resolves files via Tauri's `app_log_dir` to match the
/// `tauri_plugin_log::TargetKind::LogDir` configuration in `lib.rs`.
/// Reads `echobird.log` plus any rotated siblings (`echobird*.log`,
/// `echobird.log.*`) so a rotation that lands between the user's
/// operation and the copy click doesn't lose the head of the window —
/// previously the user's first action could disappear from the second
/// copy when the file rotated under us. Files are walked newest-mtime
/// first; we stop as soon as we've gathered enough lines.
///
/// Returns the lines as a single newline-joined string (or an empty
/// string when no log file exists yet — fresh installs before any log
/// has been written).
#[tauri::command]
pub async fn read_log_tail(app: tauri::AppHandle, lines: usize) -> Result<String, String> {
    use std::time::SystemTime;
    use tauri::Manager;

    let log_dir = app
        .path()
        .app_log_dir()
        .map_err(|e| format!("Could not resolve app log dir: {e}"))?;

    if !log_dir.exists() {
        return Ok(String::new());
    }

    // Enumerate every echobird*.log* file in the dir, capture mtime.
    // Rotated files come from tauri-plugin-log in shapes like
    // `echobird.log.1`, `echobird_2026-05-15.log`, etc. — be liberal.
    let mut files: Vec<(std::path::PathBuf, SystemTime)> = std::fs::read_dir(&log_dir)
        .map_err(|e| format!("Failed to read log dir ({}): {e}", log_dir.display()))?
        .filter_map(|e| e.ok())
        .filter(|e| {
            let name = e.file_name().to_string_lossy().to_lowercase();
            name.starts_with("echobird") && name.contains(".log")
        })
        .filter_map(|e| {
            let m = e.metadata().ok()?;
            Some((e.path(), m.modified().ok()?))
        })
        .collect();

    if files.is_empty() {
        return Ok(String::new());
    }

    // Newest first. We pull tail-lines off each file in reverse order
    // until we've satisfied `lines`.
    files.sort_by(|a, b| b.1.cmp(&a.1));

    let mut collected_rev: Vec<String> = Vec::with_capacity(lines);
    for (path, _) in &files {
        if collected_rev.len() >= lines {
            break;
        }
        let contents = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let need = lines - collected_rev.len();
        let from_file: Vec<&str> = contents
            .lines()
            .rev()
            .filter(|l| !l.trim().is_empty())
            .take(need)
            .collect();
        for line in from_file {
            collected_rev.push(line.to_string());
        }
    }

    // collected_rev is newest-first; flip back to chronological order.
    collected_rev.reverse();
    Ok(collected_rev.join("\n"))
}
