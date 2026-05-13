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
