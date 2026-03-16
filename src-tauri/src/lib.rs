pub mod models;
pub mod utils;
pub mod services;
pub mod commands;

use commands::mod_stub;
use commands::tool_commands;
use commands::model_commands;
use commands::proxy_commands;
use commands::process_commands;
use commands::channel_commands;
use commands::settings_commands;
use commands::skill_commands;
use commands::ssh_commands;
use commands::agent_commands;
use commands::role_commands;

use std::sync::Mutex;
use tauri::{Emitter, Manager};
use tauri::menu::{MenuBuilder, MenuItemBuilder};
use tauri::tray::TrayIconBuilder;

/// Managed state for tray locale and server status
pub struct TrayState {
    pub locale: Mutex<String>,
    pub server_running: Mutex<bool>,
}

/// Track app start time for splash minimum duration
pub struct AppStartTime(pub std::time::Instant);

/// 7×7 pixel pattern (Echobird logo), 1=filled, 0=transparent
const PIXEL_PATTERN: [[u8; 7]; 7] = [
    [0, 1, 1, 1, 1, 1, 0],
    [1, 0, 0, 0, 0, 1, 1],
    [1, 1, 1, 1, 1, 1, 1],
    [0, 1, 1, 1, 1, 1, 0],
    [1, 1, 1, 0, 1, 1, 1],
    [1, 0, 1, 0, 1, 0, 1],
    [1, 0, 1, 0, 1, 0, 1],
];

/// Generate tray icon RGBA data from pixel pattern
/// color: "green" (#00FF9D) for offline, "yellow" (#FFD700) for online
fn create_tray_icon_rgba(color: &str) -> (Vec<u8>, u32, u32) {
    let (r, g, b) = match color {
        "yellow" => (0xFF_u8, 0xD7_u8, 0x00_u8),
        _ => (0x00_u8, 0xFF_u8, 0x9D_u8), // green
    };

    let src_size: u32 = 7;
    let scale: u32 = 4;       // 7 * 4 = 28
    let inner_size = src_size * scale; // 28
    let out_size: u32 = 32;   // standard tray icon size
    let pad = (out_size - inner_size) / 2; // 2px padding

    let mut buf = vec![0u8; (out_size * out_size * 4) as usize];

    for py in 0..out_size {
        for px in 0..out_size {
            let ix = px as i32 - pad as i32;
            let iy = py as i32 - pad as i32;
            let offset = ((py * out_size + px) * 4) as usize;

            if ix >= 0 && ix < inner_size as i32 && iy >= 0 && iy < inner_size as i32 {
                let sx = (ix as u32 / scale) as usize;
                let sy = (iy as u32 / scale) as usize;
                if PIXEL_PATTERN[sy][sx] == 1 {
                    buf[offset] = r;
                    buf[offset + 1] = g;
                    buf[offset + 2] = b;
                    buf[offset + 3] = 0xFF; // fully opaque
                    continue;
                }
            }
            buf[offset + 3] = 0x00; // fully transparent
        }
    }

    (buf, out_size, out_size)
}

/// Get localized tray string
fn tray_t(locale: &str, key: &str) -> String {
    match (locale, key) {
        // English
        ("en", "show") => "Show EchoBird".into(),
        ("en", "server") => "LOCAL SERVER".into(),
        ("en", "on") => "ON".into(),
        ("en", "off") => "OFF".into(),
        ("en", "quit") => "Quit".into(),
        ("en", "tooltip") => "Local Server".into(),
        // Simplified Chinese
        ("zh-Hans", "show") => "\u{663E}\u{793A} EchoBird".into(),
        ("zh-Hans", "server") => "\u{672C}\u{5730}\u{670D}\u{52A1}\u{5668}".into(),
        ("zh-Hans", "on") => "\u{5F00}\u{542F}".into(),
        ("zh-Hans", "off") => "\u{5173}\u{95ED}".into(),
        ("zh-Hans", "quit") => "\u{9000}\u{51FA}".into(),
        ("zh-Hans", "tooltip") => "\u{672C}\u{5730}\u{670D}\u{52A1}\u{5668}".into(),
        // Traditional Chinese
        ("zh-Hant", "show") => "\u{986F}\u{793A} EchoBird".into(),
        ("zh-Hant", "server") => "\u{672C}\u{4F3A}\u{670D}\u{5668}".into(),
        ("zh-Hant", "on") => "\u{958B}\u{555F}".into(),
        ("zh-Hant", "off") => "\u{95DC}\u{9589}".into(),
        ("zh-Hant", "quit") => "\u{7D50}\u{675F}".into(),
        ("zh-Hant", "tooltip") => "\u{672C}\u{4F3A}\u{670D}\u{5668}".into(),
        // Japanese
        ("ja", "show") => "EchoBird \u{3092}\u{8868}\u{793A}".into(),
        ("ja", "server") => "\u{30ED}\u{30FC}\u{30AB}\u{30EB}\u{30B5}\u{30FC}\u{30D0}\u{30FC}".into(),
        ("ja", "on") => "\u{30AA}\u{30F3}".into(),
        ("ja", "off") => "\u{30AA}\u{30D5}".into(),
        ("ja", "quit") => "\u{7D42}\u{4E86}".into(),
        ("ja", "tooltip") => "\u{30ED}\u{30FC}\u{30AB}\u{30EB}\u{30B5}\u{30FC}\u{30D0}\u{30FC}".into(),
        // Korean
        ("ko", "show") => "EchoBird \u{D45C}\u{C2DC}".into(),
        ("ko", "server") => "\u{B85C}\u{CEEC} \u{C11C}\u{BC84}".into(),
        ("ko", "on") => "\u{CF1C}\u{AE30}".into(),
        ("ko", "off") => "\u{B044}\u{AE30}".into(),
        ("ko", "quit") => "\u{C885}\u{B8CC}".into(),
        ("ko", "tooltip") => "\u{B85C}\u{CEEC} \u{C11C}\u{BC84}".into(),
        // Fallback to English
        (_, key) => tray_t("en", key),
    }
}

/// Resolve locale to one of 5 supported tray locales
fn resolve_tray_locale(locale: &str) -> &'static str {
    if locale.starts_with("zh") {
        if locale.contains("Hans") || locale.contains("CN") || locale.contains("SG") {
            "zh-Hans"
        } else if locale.contains("Hant") || locale.contains("TW") || locale.contains("HK") {
            "zh-Hant"
        } else {
            "zh-Hans"
        }
    } else if locale.starts_with("ja") {
        "ja"
    } else if locale.starts_with("ko") {
        "ko"
    } else {
        "en"
    }
}

/// Rebuild tray menu dynamically (call when locale or server status changes)
pub fn rebuild_tray_menu(app: &tauri::AppHandle) {
    let state = app.state::<TrayState>();
    let locale = state.locale.lock().unwrap().clone();
    let is_online = *state.server_running.lock().unwrap();
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

    // Server status item (clickable �?opens Local Server page)
    let server_label = format!("{} [{}]",
        tray_t(&locale, "server"),
        if is_online { tray_t(&locale, "on") } else { tray_t(&locale, "off") }
    );
    let server_item = MenuItemBuilder::with_id("server_status", &server_label)
        .build(app)
        .unwrap();

    let quit_item = MenuItemBuilder::with_id("quit", tray_t(&locale, "quit"))
        .build(app)
        .unwrap();

    // Build menu using chaining
    let menu = MenuBuilder::new(app)
        .item(&version_item)
        .separator()
        .item(&show_item)
        .separator()
        .item(&server_item)
        .separator()
        .item(&quit_item)
        .build()
        .unwrap();

    let _ = tray.set_menu(Some(menu));

    // Update icon color: green=offline, yellow=online
    let color = if is_online { "yellow" } else { "green" };
    let (rgba, w, h) = create_tray_icon_rgba(color);
    let tray_icon = tauri::image::Image::new_owned(rgba, w, h);
    let _ = tray.set_icon(Some(tray_icon));

    // Update tooltip
    let tooltip = format!("{} - {} {}",
        app_name,
        tray_t(&locale, "tooltip"),
        if is_online { tray_t(&locale, "on") } else { tray_t(&locale, "off") }
    );
    let _ = tray.set_tooltip(Some(&tooltip));

    log::info!("[Tray] Menu rebuilt: locale={}, server={}", locale, if is_online { "ON" } else { "OFF" });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(TrayState {
            locale: Mutex::new("en".into()),
            server_running: Mutex::new(false),
        })
        .manage(AppStartTime(std::time::Instant::now()))
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
                        tauri_plugin_log::Target::new(
                            tauri_plugin_log::TargetKind::LogDir {
                                file_name: Some("echobird".to_string()),
                            },
                        ),
                    ])
                    .build(),
            )?;

            // Register shell plugin (open external URLs, folders)
            app.handle().plugin(tauri_plugin_shell::init())?;

            // ─── System Tray ───
            let (rgba, w, h) = create_tray_icon_rgba("green");
            let tray_icon = tauri::image::Image::new_owned(rgba, w, h);

            // Build initial tray menu (English default, server offline)
            let version = env!("CARGO_PKG_VERSION");
            let version_item = MenuItemBuilder::with_id("version", format!("EchoBird v{}", version))
                .enabled(false)
                .build(app)?;
            let show_item = MenuItemBuilder::with_id("show", tray_t("en", "show"))
                .build(app)?;
            let server_item = MenuItemBuilder::with_id("server_status",
                format!("{} [{}]", tray_t("en", "server"), tray_t("en", "off")))
                .build(app)?;
            let quit_item = MenuItemBuilder::with_id("quit", tray_t("en", "quit"))
                .build(app)?;
            let tray_menu = MenuBuilder::new(app)
                .item(&version_item)
                .separator()
                .item(&show_item)
                .separator()
                .item(&server_item)
                .separator()
                .item(&quit_item)
                .build()?;

            TrayIconBuilder::with_id("main-tray")
                .icon(tray_icon)
                .menu(&tray_menu)
                .tooltip(format!("EchoBird - {} {}", tray_t("en", "tooltip"), tray_t("en", "off")))
                .on_menu_event(move |app_handle, event| {
                    match event.id().as_ref() {
                        "show" => {
                            if let Some(window) = app_handle.get_webview_window("main") {
                                let _ = window.show();
                                let _ = window.unminimize();
                                let _ = window.set_focus();
                            }
                        }
                        "server_status" => {
                            // Open main window and navigate to Local Server page
                            if let Some(window) = app_handle.get_webview_window("main") {
                                let _ = window.show();
                                let _ = window.unminimize();
                                let _ = window.set_focus();
                                let _ = window.emit("tray:navigate", "player");
                            }
                        }
                        "quit" => {
                            app_handle.exit(0);
                        }
                        _ => {}
                    }
                })
                .on_tray_icon_event(|tray, event| {
                    // Only show/focus window on double-click (Windows)
                    // Single click must not steal focus �?it opens the context menu
                    if let tauri::tray::TrayIconEvent::DoubleClick { .. } = event {
                        let app_handle = tray.app_handle();
                        if let Some(window) = app_handle.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.unminimize();
                            let _ = window.set_focus();
                        }
                    }
                })
                .build(app)?;

            // Safety fallback: show main window after 1s even if appReady() never fires.
            // Uses std::thread to avoid tokio runtime dependency in sync setup().
            let fallback_handle = app.handle().clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(1000));
                if let Some(win) = fallback_handle.get_webview_window("main") {
                    if !win.is_visible().unwrap_or(true) {
                        log::warn!("[Safety] appReady() not called after 1s — showing main window");
                        let _ = win.show();
                        let _ = win.set_focus();
                    }
                }
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            mod_stub::health_check,
            mod_stub::get_app_logs,
            mod_stub::clear_app_logs,
            mod_stub::app_ready,
            mod_stub::set_locale,
            mod_stub::quit_app,
            tool_commands::scan_tools,
            tool_commands::get_tool_model_info,
            tool_commands::apply_model_to_tool,
            tool_commands::launch_game,
            tool_commands::open_folder,
            tool_commands::get_tool_installed_skills,
            model_commands::get_models,
            model_commands::add_model,
            model_commands::delete_model,
            model_commands::update_model,
            model_commands::test_model,
            model_commands::ping_model,
            model_commands::toggle_key_encryption,
            model_commands::is_key_destroyed,
            proxy_commands::start_proxy,
            proxy_commands::stop_proxy,
            proxy_commands::get_proxy_port,
            proxy_commands::get_proxy_rules,
            proxy_commands::save_proxy_rules,
            proxy_commands::add_proxy_host_rule,
            proxy_commands::clear_proxy_host_rules,
            proxy_commands::parse_ss_url,
            process_commands::start_tool,
            process_commands::stop_tool,
            process_commands::get_running_tools,
            process_commands::is_tool_running,
            process_commands::start_llm_server,
            process_commands::stop_llm_server,
            process_commands::get_llm_server_info,
            process_commands::get_llm_server_logs,
            process_commands::find_llama_server,
            process_commands::get_models_dirs,
            process_commands::get_download_dir,
            process_commands::load_model_settings,
            process_commands::save_model_settings,
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
            process_commands::download_llama_server,
            process_commands::get_system_info,
            process_commands::get_local_engine_status,
            process_commands::install_local_engine,
            channel_commands::get_channels,
            channel_commands::save_channels,
            channel_commands::bridge_start,
            channel_commands::bridge_stop,
            channel_commands::bridge_status,
            channel_commands::bridge_chat_local,
            channel_commands::bridge_chat_remote,
            channel_commands::bridge_detect_agents_remote,
            channel_commands::bridge_set_role_remote,
            channel_commands::bridge_clear_role_remote,
            channel_commands::channel_history_load,
            channel_commands::channel_history_save,
            channel_commands::channel_history_clear,
            settings_commands::get_settings,
            settings_commands::save_settings,
            skill_commands::load_skills_data,
            skill_commands::save_skills_data,
            skill_commands::load_skills_favorites,
            skill_commands::save_skills_favorites,
            skill_commands::fetch_skill_source,
            skill_commands::llm_quick_chat,
            skill_commands::load_skills_i18n,
            skill_commands::save_skills_i18n,
            ssh_commands::ssh_connect,
            ssh_commands::ssh_execute,
            ssh_commands::ssh_disconnect,
            ssh_commands::ssh_test_connection,
            ssh_commands::load_ssh_servers,
            ssh_commands::save_ssh_server,
            ssh_commands::remove_ssh_server,
            ssh_commands::update_ssh_alias,
            ssh_commands::decrypt_ssh_password,
            ssh_commands::encrypt_ssh_password,
            agent_commands::agent_send_message,
            agent_commands::agent_abort,
            agent_commands::agent_reset,
            agent_commands::agent_status,
            agent_commands::load_agent_history,
            ssh_commands::ssh_upload_file,
            ssh_commands::scan_plugins,
            ssh_commands::get_bridge_path,
            role_commands::scan_roles,
            role_commands::load_role_content,
            role_commands::detect_local_agents,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|_app_handle, event| {
            if let tauri::RunEvent::Exit = event {
                // Kill all llama-server processes on app exit.
                // This prevents zombie processes from lingering and blocking ports
                // on the next launch (which would cause 401 errors or port conflicts).
                #[cfg(target_os = "windows")]
                {
                    use std::os::windows::process::CommandExt;
                    let _ = std::process::Command::new("taskkill")
                        .args(["/F", "/IM", "llama-server.exe", "/T"])
                        .creation_flags(0x08000000) // CREATE_NO_WINDOW
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .spawn();
                }
                #[cfg(not(target_os = "windows"))]
                {
                    let _ = std::process::Command::new("pkill")
                        .args(["-f", "llama-server"])
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .spawn();
                }
                log::info!("[App] Exit: killed all llama-server processes");
            }
        });
}

