// Tauri Commands for process management and local LLM server

use crate::services::local_llm::{
    self, GgufFile, GpuInfo, HfModelEntry, LocalServerInfo, SystemInfo,
};
use crate::services::process_manager;

// ─── Process Manager ───

#[tauri::command]
#[allow(deprecated)]
pub async fn start_tool(
    app_handle: tauri::AppHandle,
    tool_id: String,
    start_command: Option<String>,
    cwd: Option<String>,
) -> Result<(), String> {
    process_manager::start_tool(&tool_id, start_command.as_deref(), cwd.as_deref()).await?;
    if tool_id == "claudescience" {
        use tauri_plugin_shell::ShellExt;
        let url = process_manager::claude_science_url().await?;
        app_handle
            .shell()
            .open(url.as_str(), None)
            .map_err(|e| format!("Failed to open Claude Science: {e}"))?;
    }
    Ok(())
}

// ─── CLI launch settings ───

#[tauri::command]
pub async fn get_cli_launch_settings(
) -> std::collections::HashMap<String, crate::services::cli_launch::CliLaunchSettings> {
    crate::services::cli_launch::load()
}

#[tauri::command]
pub async fn save_cli_launch_settings(
    settings: std::collections::HashMap<String, crate::services::cli_launch::CliLaunchSettings>,
) -> Result<(), String> {
    crate::services::cli_launch::save(&settings)
}

#[tauri::command]
pub async fn get_cli_launch_options(
) -> Result<crate::services::cli_launch::CliLaunchOptions, String> {
    tauri::async_runtime::spawn_blocking(crate::services::cli_launch::options)
        .await
        .map_err(|e| format!("Terminal detection failed: {e}"))
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
    local_llm::start_server(&model_path, port, gpu_layers, context_size, rt, app_handle).await
}

#[tauri::command]
pub async fn stop_llm_server() -> Result<(), String> {
    local_llm::stop_server().await
}

#[tauri::command]
pub async fn get_llm_server_info() -> LocalServerInfo {
    local_llm::get_server_info().await
}

#[tauri::command]
pub async fn get_llm_server_logs() -> Vec<String> {
    local_llm::get_server_logs().await
}

// ─── Local LLM: custom launch command (advanced / bring-your-own engine) ───
// Powers the gear dialog on the local-LLM page. A stored custom command makes
// start_llm_server spawn the user's own exe + args verbatim (e.g. an AMD
// user's Vulkan llama-server); clearing it reverts to the auto default.

#[tauri::command]
pub async fn get_llm_default_command(
    model_path: String,
    port: u16,
    gpu_layers: Option<i32>,
    context_size: Option<u32>,
) -> Result<local_llm::CustomCommand, String> {
    let gpu_count = local_llm::gpu::detect_nvidia_gpu_count();
    let (exe, args) = local_llm::server::build_llama_default_command(
        &model_path,
        port + 100,
        gpu_layers,
        context_size,
        gpu_count,
    )?;
    Ok(local_llm::CustomCommand { exe, args })
}

#[tauri::command]
pub async fn get_llm_custom_command() -> Option<local_llm::CustomCommand> {
    local_llm::custom_command::get()
}

#[tauri::command]
pub async fn set_llm_custom_command(exe: String, args: Vec<String>) -> Result<(), String> {
    local_llm::custom_command::set(local_llm::CustomCommand { exe, args })
}

#[tauri::command]
pub async fn clear_llm_custom_command() -> Result<(), String> {
    local_llm::custom_command::clear()
}

// ─── In-app self-update (Windows): download installer + launch + exit ───

#[tauri::command]
pub async fn download_and_install_update(
    app_handle: tauri::AppHandle,
    version: String,
) -> Result<(), String> {
    crate::services::self_update::download_and_install(app_handle, version).await
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
pub async fn scan_gguf_files(dir: String) -> Result<Vec<GgufFile>, String> {
    tauri::async_runtime::spawn_blocking(move || local_llm::scan_gguf_files(&dir, 5))
        .await
        .map_err(|e| format!("Model scan task join failed: {e}"))
}

#[tauri::command]
pub async fn scan_hf_models(dir: String) -> Result<Vec<HfModelEntry>, String> {
    tauri::async_runtime::spawn_blocking(move || local_llm::scan_hf_models(&dir, 5))
        .await
        .map_err(|e| format!("Model scan task join failed: {e}"))
}

#[tauri::command]
pub async fn add_models_dir() -> Result<Vec<String>, String> {
    #[cfg(not(target_os = "android"))]
    {
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
            None => Ok(local_llm::get_models_dirs()),
        }
    }
    #[cfg(target_os = "android")]
    Err("Not available on mobile".to_string())
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
    #[cfg(not(target_os = "android"))]
    {
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
            None => Ok(local_llm::get_download_dir()),
        }
    }
    #[cfg(target_os = "android")]
    Err("Not available on mobile".to_string())
}

#[tauri::command]
pub async fn get_store_models() -> Vec<serde_json::Value> {
    local_llm::fetch_store_models().await
}

/// Right-panel Providers + Relays list for the Model Center page.
/// Remote-first, disk-cached, with `null` returned when both fail so
/// the frontend falls back to its bundled `src/data/modelDirectory.json`.
#[tauri::command]
pub async fn get_model_directory() -> serde_json::Value {
    crate::services::model_directory::fetch_model_directory().await
}

/// Remote-updatable free-model catalog. Returns `null` when the
/// remote and cache are unavailable.
#[tauri::command]
pub async fn get_free_model_directory() -> serde_json::Value {
    crate::services::free_model_directory::fetch_free_model_directory().await
}

#[tauri::command]
pub fn get_cached_free_model_directory() -> serde_json::Value {
    crate::services::free_model_directory::load_cached_free_model_directory()
}

#[tauri::command]
pub async fn download_model(
    app_handle: tauri::AppHandle,
    repo: String,
    files: Vec<String>,
) -> Result<String, String> {
    local_llm::download_model(app_handle, repo, files).await
}

#[tauri::command]
pub fn pause_download() {
    local_llm::pause_download();
}

#[tauri::command]
pub fn cancel_download(app_handle: tauri::AppHandle, files: Option<Vec<String>>) {
    local_llm::cancel_download(&app_handle, files);
}

#[tauri::command]
pub fn get_system_info() -> SystemInfo {
    local_llm::get_system_info()
}

#[tauri::command]
pub async fn get_local_engine_status(runtime: Option<String>) -> serde_json::Value {
    local_llm::get_local_engine_status(runtime.as_deref()).await
}

#[tauri::command]
pub async fn install_local_engine(
    app_handle: tauri::AppHandle,
    runtime: String,
    version: Option<String>,
    cuda_version: Option<String>,
) -> Result<(), String> {
    local_llm::install_local_engine(app_handle, runtime, version, cuda_version).await
}

#[tauri::command]
pub async fn list_engine_release_options(
    runtime: String,
    top_n: Option<usize>,
) -> Result<Vec<crate::services::local_llm::model_store::LlamaReleaseOption>, String> {
    match runtime.as_str() {
        "llama-server" => {
            let n = top_n.unwrap_or(10);
            Ok(crate::services::local_llm::model_store::fetch_llama_release_options(n).await)
        }
        // vllm / sglang have their own version conventions and we don't
        // ship a picker for them yet — return empty so the frontend
        // degrades to its auto-latest install path.
        _ => Ok(Vec::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn background_model_scans_preserve_results_and_read_new_files_on_rescan() {
        let root =
            std::env::temp_dir().join(format!("echobird-model-scan-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let hf = root.join("hf-model");
        std::fs::create_dir(&hf).unwrap();
        std::fs::write(root.join("first.gguf"), b"GGUF").unwrap();
        std::fs::write(hf.join("config.json"), r#"{"model_type":"fixture"}"#).unwrap();
        std::fs::write(hf.join("weights.safetensors"), b"weights").unwrap();
        let dir = root.to_string_lossy().into_owned();
        let (gguf, models) =
            tokio::join!(scan_gguf_files(dir.clone()), scan_hf_models(dir.clone()));
        let gguf = gguf.unwrap();
        assert_eq!(gguf.len(), 1);
        assert_eq!(gguf[0].file_name, "first.gguf");
        assert_eq!(gguf[0].file_size, 4);
        let models = models.unwrap();
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].model_name, "fixture");
        assert_eq!(models[0].total_size, 7);
        std::fs::write(root.join("second.gguf"), b"new").unwrap();
        assert_eq!(scan_gguf_files(dir).await.unwrap().len(), 2);
        let missing = root.join("missing").to_string_lossy().into_owned();
        assert!(scan_gguf_files(missing.clone()).await.unwrap().is_empty());
        assert!(scan_hf_models(missing).await.unwrap().is_empty());
        for file in [
            "first.gguf",
            "second.gguf",
            "hf-model/config.json",
            "hf-model/weights.safetensors",
        ] {
            std::fs::remove_file(root.join(file)).unwrap();
        }
        std::fs::remove_dir(hf).unwrap();
        std::fs::remove_dir(root).unwrap();
    }
}
