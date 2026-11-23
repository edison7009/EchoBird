// Window lifecycle commands

/// App ready — show the main window. Called by the frontend (App.tsx) after
/// React has mounted and scanTools() has resolved, so the WebView has already
/// painted the inline #boot-splash from index.html.
#[tauri::command]
pub async fn app_ready(app: tauri::AppHandle) {
    // Boot-autostart launches pass `--minimized` and must stay hidden in the
    // tray. The window starts visible:false (tauri.conf.json), so we keep it
    // hidden + drop to tray (macOS: Accessory policy); the user restores it
    // via the tray icon. Normal launches show as usual.
    let start_minimized = std::env::args().any(|a| a == "--minimized");
    #[cfg(not(target_os = "android"))]
    {
        use tauri::Manager;
        if start_minimized {
            crate::hide_main_window(&app);
        } else if let Some(main) = app.get_webview_window("main") {
            // Re-center right before show(): on Linux (GNOME/Wayland), the
            // initial `center: true` is dropped because the compositor ignores
            // client positioning until the window is mapped.
            let _ = main.center();
            let _ = main.show();
            let _ = main.set_focus();
            crate::window_visibility::publish(&main);
        }
    }
    #[cfg(target_os = "android")]
    {
        let _ = (app, start_minimized);
    }
}
