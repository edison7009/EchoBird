//! The single native authority for window close and app shutdown.
use crate::{commands::settings_commands, hide_main_window, kill_stale_llama_server, services};
pub(crate) fn on_event(app_handle: &tauri::AppHandle, event: tauri::RunEvent) {
    match event {
        tauri::RunEvent::WindowEvent {
            label,
            event: tauri::WindowEvent::CloseRequested { api, .. },
            ..
        } if label == "main" => {
            // Check user settings for close behavior
            let settings = settings_commands::get_settings();
            let close_to_tray = settings.close_to_tray.unwrap_or(false);

            if close_to_tray {
                // Prevent the window from closing and hide it instead.
                // On macOS this also drops the dock icon (Accessory
                // policy) so the app lives only in the menu bar until
                // the user re-opens it from the tray.
                api.prevent_close();
                hide_main_window(app_handle);
            }
            // Otherwise, let it close normally
        }
        // Hidden helper windows (background plugins, voice tools) would
        // otherwise keep the process alive with a tray icon that has no
        // window left to restore.
        tauri::RunEvent::WindowEvent {
            label,
            event: tauri::WindowEvent::Destroyed,
            ..
        } if label == "main" => {
            app_handle.exit(0);
        }
        tauri::RunEvent::Exit => {
            services::smart_router::statistics::shutdown();
            services::voice_input::shutdown(app_handle);
            // Clean up llama-server (our spawned local LLM). The
            // codex launcher we don't touch — if it's still running
            // when EchoBird exits, that's fine; the user keeps
            // working in Codex, and any new launcher we spawn next
            // session will share the same FIXED proxy port.
            kill_stale_llama_server();
            log::info!("[App] Exit: cleaned up llama-server");
        }
        _ => {}
    }
}
