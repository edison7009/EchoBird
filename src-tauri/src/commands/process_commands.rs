// Tauri Commands for process management and local LLM server

use crate::services::process_manager;
use crate::services::local_llm::{self, LocalServerInfo, ModelSettings, GgufFile, GpuInfo, HfModelEntry};

// ─── Process Manager ───

#[tauri::command]
pub async fn start_tool(tool_id: String, start_command: Option<String>) -> Result<(), String> {
    process_manager::start_tool(&tool_id, start_command.as_deref()).await
}

#[tauri::command]
pub async fn stop_tool(tool_id: String) -> Result<(), String> {
    process_manager::stop_tool(&tool_id).await
}

#[tauri::command]
pub async fn get_running_tools() -> Vec<String> {
    process_manager::get_running_tools().await
}

#[tauri::command]
pub async fn is_tool_running(tool_id: String) -> bool {
    process_manager::is_tool_running(&tool_id).await
}

// ─── Local LLM Server ───

#[tauri::command]
pub async fn start_llm_server(
    app_handle: tauri::AppHandle,
    model_path: String,
    port: u16,
    gpu_layers: Option<i32>,
    context_size: Option<u32>,
    runtime: Option<String>,
) -> Result<(), String> {
    let rt = runtime.as_deref().unwrap_or("llama-server");
    let result = local_llm::start_server(&model_path, port, gpu_layers, context_size, rt).await;
    if result.is_ok() {
        use tauri::Manager;
        let state = app_handle.state::<crate::TrayState>();
        *state.server_running.lock().unwrap() = true;
        crate::rebuild_tray_menu(&app_handle);
    }
    result
}

#[tauri::command]
pub async fn stop_llm_server(app_handle: tauri::AppHandle) -> Result<(), String> {
    let result = local_llm::stop_server().await;
    if result.is_ok() {
        use tauri::Manager;
        let state = app_handle.state::<crate::TrayState>();
        *state.server_running.lock().unwrap() = false;
        crate::rebuild_tray_menu(&app_handle);
    }
    result
}

#[tauri::command]
pub async fn get_llm_server_info() -> LocalServerInfo {
    local_llm::get_server_info().await
}

#[tauri::command]
pub async fn get_llm_server_logs() -> Vec<String> {
    local_llm::get_server_logs().await
}

#[tauri::command]
pub fn find_llama_server() -> Option<String> {
    local_llm::LocalLlmServer::find_llama_server()
        .map(|p| p.to_string_lossy().to_string())
}

#[tauri::command]
pub fn get_models_dirs() -> Vec<String> {
    local_llm::get_models_dirs()
}

#[tauri::command]
pub fn get_download_dir() -> String {
    local_llm::get_download_dir()
}

#[tauri::command]
pub fn load_model_settings() -> ModelSettings {
    local_llm::load_model_settings()
}

#[tauri::command]
pub fn save_model_settings(settings: ModelSettings) {
    local_llm::save_model_settings(&settings);
}

#[tauri::command]
pub fn scan_gguf_files(dir: String) -> Vec<GgufFile> {
    local_llm::scan_gguf_files(&dir, 5)
}

#[tauri::command]
pub fn scan_hf_models(dir: String) -> Vec<HfModelEntry> {
    local_llm::scan_hf_models(&dir, 5)
}

#[tauri::command]
pub async fn add_models_dir() -> Result<Vec<String>, String> {
    // Open native folder picker
    let folder = rfd::AsyncFileDialog::new()
        .set_title("Select Models Directory")
        .pick_folder()
        .await;

    match folder {
        Some(handle) => {
            let path = handle.path().to_string_lossy().to_string();
            let mut settings = local_llm::load_model_settings();
            if !settings.models_dirs.contains(&path) {
                settings.models_dirs.push(path);
                local_llm::save_model_settings(&settings);
            }
            Ok(settings.models_dirs)
        }
        None => Ok(local_llm::get_models_dirs()), // User cancelled
    }
}

#[tauri::command]
pub fn remove_models_dir(dir: String) -> Vec<String> {
    let mut settings = local_llm::load_model_settings();
    settings.models_dirs.retain(|d| d != &dir);
    local_llm::save_model_settings(&settings);
    local_llm::get_models_dirs()
}

#[tauri::command]
pub fn detect_gpu() -> Option<GpuInfo> {
    local_llm::detect_gpu()
}

#[tauri::command]
pub fn get_gpu_info() -> Option<GpuInfo> {
    local_llm::get_gpu_info()
}

#[tauri::command]
pub async fn set_download_dir() -> Result<String, String> {
    let folder = rfd::AsyncFileDialog::new()
        .set_title("Select Download Directory")
        .pick_folder()
        .await;

    match folder {
        Some(handle) => {
            let path = handle.path().to_string_lossy().to_string();
            local_llm::set_download_dir(&path);
            Ok(path)
        }
        None => Ok(local_llm::get_download_dir()), // User cancelled
    }
}

#[tauri::command]
pub async fn get_store_models() -> Vec<serde_json::Value> {
    local_llm::fetch_store_models().await
}

#[tauri::command]
pub async fn download_model(app_handle: tauri::AppHandle, repo: String, file_name: String) -> Result<String, String> {
    local_llm::download_model(app_handle, repo, file_name).await
}

#[tauri::command]
pub fn pause_download() {
    local_llm::pause_download();
}

#[tauri::command]
pub fn cancel_download(app_handle: tauri::AppHandle, file_name: Option<String>) {
    local_llm::cancel_download(&app_handle, file_name);
}

#[tauri::command]
pub async fn download_llama_server(app_handle: tauri::AppHandle) -> Result<String, String> {
    local_llm::download_llama_server(app_handle).await
}
