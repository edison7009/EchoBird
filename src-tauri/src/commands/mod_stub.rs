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

/// Pop the main webview's devtools panel for the user-facing
/// "问题反馈 / Feedback" page. Devtools is enabled in production via the
/// `devtools` feature on the tauri crate (Cargo.toml) — without it this
/// is a no-op on release builds.
#[tauri::command]
pub async fn open_devtools(app: tauri::AppHandle) {
    #[cfg(not(target_os = "android"))]
    {
        use tauri::Manager;
        if let Some(main) = app.get_webview_window("main") {
            main.open_devtools();
        }
    }
    #[cfg(target_os = "android")]
    {
        let _ = app;
    }
}
