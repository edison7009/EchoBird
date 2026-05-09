pub mod models;
pub mod utils;
pub mod services;
pub mod commands;

use commands::mod_stub;
use commands::tool_commands;
use commands::model_commands;
use commands::process_commands;
use commands::settings_commands;

use commands::ssh_commands;
use commands::agent_commands;
use commands::bundled_commands;

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // window-state must be registered on the Builder (not inside .setup()) so
        // it can restore size/position before the main window is created from
        // tauri.conf.json. Auto-saves on close, auto-restores on creation.
        .plugin(tauri_plugin_window_state::Builder::default().build())
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

            // Safety fallback: show main window after 1s even if appReady() never fires.
            // Uses std::thread to avoid tokio runtime dependency in sync setup().
            #[cfg(not(target_os = "android"))]
            {
            let fallback_handle = app.handle().clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(1000));
                if let Some(win) = fallback_handle.get_webview_window("main") {
                    if !win.is_visible().unwrap_or(true) {
                        log::warn!("[Safety] appReady() not called after 1s — showing main window");
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
            ssh_commands::decrypt_ssh_password,
            ssh_commands::encrypt_ssh_password,
            agent_commands::agent_send_message,
            agent_commands::agent_abort,
            agent_commands::agent_reset,
            bundled_commands::get_mother_hints,
            bundled_commands::get_install_index,
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

