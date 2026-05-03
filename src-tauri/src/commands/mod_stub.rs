// Stub commands for initial validation

use crate::models::config::AppLogEntry;
use crate::services::logger;

/// Get application logs
#[tauri::command]
pub fn get_app_logs() -> Vec<AppLogEntry> {
    logger::get_logs()
}

/// Clear application logs
#[tauri::command]
pub fn clear_app_logs() {
    logger::clear_logs();
}

/// Health check — returns app version
#[tauri::command]
pub fn health_check() -> String {
    format!("Echobird v{}", env!("CARGO_PKG_VERSION"))
}

/// App ready — close splash screen and show main window
/// Ensures splash stays visible for at least 1.5 seconds
#[tauri::command]
pub async fn app_ready(app: tauri::AppHandle) {
    #[cfg(not(target_os = "android"))]
    {
        use tauri::Manager;

        // Minimum splash display time (1.5 seconds from app start)
        let state = app.state::<crate::AppStartTime>();
        let elapsed = state.0.elapsed();
        let min_splash = std::time::Duration::from_millis(1500);
        if elapsed < min_splash {
            tokio::time::sleep(min_splash - elapsed).await;
        }

        // Close splash window
        if let Some(splash) = app.get_webview_window("splash") {
            let _ = splash.close();
        }
        // Show and focus main window
        if let Some(main) = app.get_webview_window("main") {
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
