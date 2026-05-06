// Stub commands for initial validation

/// Health check — returns app version
#[tauri::command]
pub fn health_check() -> String {
    format!("Echobird v{}", env!("CARGO_PKG_VERSION"))
}

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
    { let _ = app; }
}

/// Quit app — fully exit the application
#[tauri::command]
pub fn quit_app(app: tauri::AppHandle) {
    log::info!("[App] Quit requested from frontend");
    app.exit(0);
}
