pub mod commands;
pub mod models;
pub mod services;
pub mod utils;

use commands::mod_stub;
use commands::model_commands;
use commands::process_commands;
use commands::settings_commands;
use commands::tool_commands;

use commands::agent_commands;
use commands::bundled_commands;
use commands::secret_commands;
use commands::ssh_commands;

use std::sync::Mutex;
use tauri::menu::{MenuBuilder, MenuItemBuilder};
use tauri::tray::TrayIconBuilder;
use tauri::Manager;

/// Managed state for tray locale
pub struct TrayState {
    pub locale: Mutex<String>,
}

/// Load tray icon from the bundled tray-icon.png
fn load_tray_icon() -> tauri::image::Image<'static> {
    let icon_bytes = include_bytes!("../icons/tray-icon.png");
    let img = image::load_from_memory(icon_bytes).expect("Failed to decode tray-icon.png");
    let rgba = img.to_rgba8();
    let (width, height) = rgba.dimensions();
    tauri::image::Image::new_owned(rgba.into_raw(), width, height)
}

/// Get localized tray string
fn tray_t(locale: &str, key: &str) -> String {
    match (locale, key) {
        // English
        ("en", "show") => "Show EchoBird".into(),
        ("en", "quit") => "Quit".into(),
        // Simplified Chinese
        ("zh-Hans", "show") => "显示 EchoBird".into(),
        ("zh-Hans", "quit") => "退出".into(),
        // Fallback to English
        (_, key) => tray_t("en", key),
    }
}

/// Rebuild tray menu dynamically (call when locale changes)
pub fn rebuild_tray_menu(app: &tauri::AppHandle) {
    let state = app.state::<TrayState>();
    let locale = state.locale.lock().unwrap().clone();
    let version = env!("CARGO_PKG_VERSION");

    // Get tray icon by ID
    let Some(tray) = app.tray_by_id("main-tray") else {
        log::warn!("[Tray] Cannot find tray icon 'main-tray'");
        return;
    };

    // Build menu items
    let app_name = "EchoBird";
    let version_item = MenuItemBuilder::with_id("version", format!("{} v{}", app_name, version))
        .enabled(false)
        .build(app)
        .unwrap();
    let show_item = MenuItemBuilder::with_id("show", tray_t(&locale, "show"))
        .build(app)
        .unwrap();
    let quit_item = MenuItemBuilder::with_id("quit", tray_t(&locale, "quit"))
        .build(app)
        .unwrap();

    // Build menu
    let menu = MenuBuilder::new(app)
        .item(&version_item)
        .separator()
        .item(&show_item)
        .separator()
        .item(&quit_item)
        .build()
        .unwrap();

    let _ = tray.set_menu(Some(menu));

    // Update icon
    let tray_icon = load_tray_icon();
    let _ = tray.set_icon(Some(tray_icon));

    // Update tooltip
    let tooltip = format!("{} v{}", app_name, version);
    let _ = tray.set_tooltip(Some(&tooltip));

    log::info!("[Tray] Menu rebuilt: locale={}", locale);
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // Note: window-state plugin is temporarily disabled because it intercepts
        // CloseRequested events, preventing our "minimize to tray" feature from working.
        // We'll need to manually save/restore window state if needed.
        // .plugin(tauri_plugin_window_state::Builder::default().build())
        .manage(TrayState {
            locale: Mutex::new("en".into()),
        })
        .manage(ssh_commands::create_ssh_pool())
        .manage(services::agent_loop::create_session_map())
        .setup(|app| {
            // Initialize resource_dir for correct tools/ path resolution on all platforms
            // (especially Linux where exe is at /usr/bin but tools are at /usr/lib/com.echobird.ai/)
            if let Ok(res_dir) = app.path().resource_dir() {
                services::tool_manager::init_resource_dir(res_dir);
            } else {
                log::warn!("[Setup] Could not resolve resource_dir");
            }

            // Enable file logging in all builds for diagnostics
            app.handle().plugin(
                tauri_plugin_log::Builder::default()
                    .level(log::LevelFilter::Info)
                    .targets([
                        // Log to file
                        tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::LogDir {
                            file_name: Some("echobird".to_string()),
                        }),
                        // Also log to stdout in dev mode
                        tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::Stdout),
                    ])
                    .build(),
            )?;

            // Register shell plugin (open external URLs, folders)
            app.handle().plugin(tauri_plugin_shell::init())?;

            // ─── System Tray ───
            let tray_icon = load_tray_icon();

            // Load user's locale from settings
            let user_locale = settings_commands::get_settings()
                .locale
                .unwrap_or_else(|| "en".to_string());

            // Build initial tray menu with user's locale
            let version = env!("CARGO_PKG_VERSION");
            let version_item =
                MenuItemBuilder::with_id("version", format!("EchoBird v{}", version))
                    .enabled(false)
                    .build(app)?;
            let show_item =
                MenuItemBuilder::with_id("show", tray_t(&user_locale, "show")).build(app)?;
            let quit_item =
                MenuItemBuilder::with_id("quit", tray_t(&user_locale, "quit")).build(app)?;
            let tray_menu = MenuBuilder::new(app)
                .item(&version_item)
                .separator()
                .item(&show_item)
                .separator()
                .item(&quit_item)
                .build()?;

            // Update TrayState with user's locale
            let state = app.state::<TrayState>();
            *state.locale.lock().unwrap() = user_locale;

            TrayIconBuilder::with_id("main-tray")
                .icon(tray_icon)
                .menu(&tray_menu)
                .show_menu_on_left_click(false)
                .tooltip(format!("EchoBird v{}", version))
                .on_menu_event(move |app_handle, event| match event.id().as_ref() {
                    "show" => {
                        if let Some(window) = app_handle.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.unminimize();
                            let _ = window.set_focus();
                        }
                    }
                    "quit" => {
                        app_handle.exit(0);
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    // Left click: toggle window visibility (only on button release)
                    if let tauri::tray::TrayIconEvent::Click {
                        button: tauri::tray::MouseButton::Left,
                        button_state: tauri::tray::MouseButtonState::Up,
                        ..
                    } = event
                    {
                        let app_handle = tray.app_handle();
                        if let Some(window) = app_handle.get_webview_window("main") {
                            if window.is_visible().unwrap_or(false) {
                                let _ = window.hide();
                            } else {
                                let _ = window.show();
                                let _ = window.unminimize();
                                let _ = window.set_focus();
                            }
                        }
                    }
                    // Right click: show menu (handled automatically by Tauri)
                })
                .build(app)?;

            // Windows 11: disable shadow and force square corners on borderless window.
            // Without this, DWM adds a drop-shadow and rounds corners by default,
            // creating visible gaps between the system border and the app content.
            #[cfg(target_os = "windows")]
            {
                if let Some(win) = app.get_webview_window("main") {
                    let _ = win.set_shadow(false);

                    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
                    use windows::Win32::Foundation::HWND;
                    use windows::Win32::Graphics::Dwm::{
                        DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND,
                    };

                    if let Ok(handle) = win.window_handle() {
                        if let RawWindowHandle::Win32(win32_handle) = handle.as_ref() {
                            let hwnd = HWND(win32_handle.hwnd.get() as _);
                            let pref: i32 = DWMWCP_DONOTROUND.0;
                            unsafe {
                                let _ = DwmSetWindowAttribute(
                                    hwnd,
                                    DWMWA_WINDOW_CORNER_PREFERENCE,
                                    &pref as *const _ as *const _,
                                    std::mem::size_of_val(&pref) as u32,
                                );
                            }
                        }
                    }
                }
            }

            // macOS: ensure cursor events are enabled to prevent hit-test failures.
            // Some users reported buttons becoming unresponsive (only scrolling worked)
            // when decorations=false + transparent=true + window-state restoration
            // caused the window to lose proper event routing.
            #[cfg(target_os = "macos")]
            {
                if let Some(win) = app.get_webview_window("main") {
                    let _ = win.set_ignore_cursor_events(false);
                    log::info!("[macOS] Explicitly enabled cursor events for hit-test");
                }
            }

            // Note: Window close interception is now handled in the frontend (App.tsx)
            // using getCurrentWindow().onCloseRequested() API, which is the recommended
            // approach in Tauri 2.0 for cross-platform compatibility.

            // Safety fallback: show main window after 1s even if appReady() never fires.
            // Uses std::thread to avoid tokio runtime dependency in sync setup().
            #[cfg(not(target_os = "android"))]
            {
                let fallback_handle = app.handle().clone();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(1000));
                    if let Some(win) = fallback_handle.get_webview_window("main") {
                        if !win.is_visible().unwrap_or(true) {
                            log::warn!(
                                "[Safety] appReady() not called after 1s — showing main window"
                            );
                            let _ = win.center();
                            let _ = win.show();
                            let _ = win.set_focus();
                        }
                    }
                });
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            mod_stub::app_ready,
            tool_commands::scan_tools,
            tool_commands::apply_model_to_tool,
            tool_commands::restore_tool_to_official,
            tool_commands::launch_game,
            tool_commands::open_folder,
            model_commands::get_models,
            model_commands::add_model,
            model_commands::delete_model,
            model_commands::update_model,
            model_commands::test_model,
            model_commands::ping_model,
            model_commands::is_key_destroyed,
            process_commands::start_tool,
            process_commands::start_llm_server,
            process_commands::stop_llm_server,
            process_commands::get_llm_server_info,
            process_commands::get_llm_server_logs,
            process_commands::get_models_dirs,
            process_commands::get_download_dir,
            process_commands::scan_gguf_files,
            process_commands::scan_hf_models,
            process_commands::add_models_dir,
            process_commands::remove_models_dir,
            process_commands::detect_gpu,
            process_commands::get_gpu_info,
            process_commands::set_download_dir,
            process_commands::get_store_models,
            process_commands::download_model,
            process_commands::pause_download,
            process_commands::cancel_download,
            process_commands::get_system_info,
            process_commands::get_local_engine_status,
            process_commands::install_local_engine,
            settings_commands::get_settings,
            settings_commands::save_settings,
            ssh_commands::ssh_test_connection,
            ssh_commands::load_ssh_servers,
            ssh_commands::save_ssh_server,
            ssh_commands::remove_ssh_server,
            secret_commands::decrypt_secret,
            secret_commands::encrypt_secret,
            agent_commands::agent_send_message,
            agent_commands::agent_abort,
            agent_commands::agent_reset,
            bundled_commands::get_mother_hints,
            bundled_commands::get_install_index,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app_handle, event| {
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
                        // Prevent the window from closing and hide it instead
                        api.prevent_close();

                        if let Some(window) = app_handle.get_webview_window("main") {
                            let _ = window.hide();
                        }
                    }
                    // Otherwise, let it close normally
                }
                tauri::RunEvent::Exit => {
                    // Clean up all spawned processes on app exit to prevent zombie processes.
                    // ProcessManager uses a global singleton, so we can't access it here.
                    // Instead, we kill processes by name/pattern.

                    #[cfg(target_os = "windows")]
                    {
                        use std::os::windows::process::CommandExt;
                        const CREATE_NO_WINDOW: u32 = 0x08000000;

                        // 1. Kill codex-launcher node processes
                        // The launcher spawns as "node.exe codex-launcher.cjs" and runs a local proxy.
                        // We need to kill it to stop the proxy server.
                        let _ = std::process::Command::new("wmic")
                            .args([
                                "process",
                                "where",
                                "CommandLine like '%codex-launcher.cjs%'",
                                "delete",
                            ])
                            .creation_flags(CREATE_NO_WINDOW)
                            .stdout(std::process::Stdio::null())
                            .stderr(std::process::Stdio::null())
                            .spawn();

                        // 2. Kill Codex processes (both Desktop and CLI)
                        let _ = std::process::Command::new("taskkill")
                            .args(["/F", "/IM", "Codex.exe", "/T"])
                            .creation_flags(CREATE_NO_WINDOW)
                            .stdout(std::process::Stdio::null())
                            .stderr(std::process::Stdio::null())
                            .spawn();

                        let _ = std::process::Command::new("taskkill")
                            .args(["/F", "/IM", "codex.exe", "/T"])
                            .creation_flags(CREATE_NO_WINDOW)
                            .stdout(std::process::Stdio::null())
                            .stderr(std::process::Stdio::null())
                            .spawn();

                        // 3. Kill llama-server processes
                        let _ = std::process::Command::new("taskkill")
                            .args(["/F", "/IM", "llama-server.exe", "/T"])
                            .creation_flags(CREATE_NO_WINDOW)
                            .stdout(std::process::Stdio::null())
                            .stderr(std::process::Stdio::null())
                            .spawn();
                    }

                    #[cfg(target_os = "macos")]
                    {
                        // 1. Kill codex-launcher node processes
                        let _ = std::process::Command::new("pkill")
                            .args(["-f", "codex-launcher.cjs"])
                            .stdout(std::process::Stdio::null())
                            .stderr(std::process::Stdio::null())
                            .spawn();

                        // 2. Kill Codex processes
                        let _ = std::process::Command::new("pkill")
                            .args(["-f", "Codex.app/Contents/MacOS/Codex"])
                            .stdout(std::process::Stdio::null())
                            .stderr(std::process::Stdio::null())
                            .spawn();

                        let _ = std::process::Command::new("pkill")
                            .args(["-f", "@openai/codex.*vendor.*codex"])
                            .stdout(std::process::Stdio::null())
                            .stderr(std::process::Stdio::null())
                            .spawn();

                        // 3. Kill llama-server processes
                        let _ = std::process::Command::new("pkill")
                            .args(["-f", "llama-server"])
                            .stdout(std::process::Stdio::null())
                            .stderr(std::process::Stdio::null())
                            .spawn();
                    }

                    #[cfg(target_os = "linux")]
                    {
                        // 1. Kill codex-launcher node processes
                        let _ = std::process::Command::new("pkill")
                            .args(["-f", "codex-launcher.cjs"])
                            .stdout(std::process::Stdio::null())
                            .stderr(std::process::Stdio::null())
                            .spawn();

                        // 2. Kill Codex CLI processes
                        let _ = std::process::Command::new("pkill")
                            .args(["-f", "@openai/codex.*vendor.*codex"])
                            .stdout(std::process::Stdio::null())
                            .stderr(std::process::Stdio::null())
                            .spawn();

                        // 3. Kill llama-server processes
                        let _ = std::process::Command::new("pkill")
                            .args(["-f", "llama-server"])
                            .stdout(std::process::Stdio::null())
                            .stderr(std::process::Stdio::null())
                            .spawn();
                    }

                    log::info!(
                        "[App] Exit: killed codex-launcher, Codex, and llama-server processes"
                    );
                }
                _ => {}
            }
        });
}
