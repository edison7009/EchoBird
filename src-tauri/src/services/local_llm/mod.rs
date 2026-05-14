// local_llm module — split from monolithic local_llm.rs into focused submodules
//
// Structure:
//   types.rs       — shared structs (LocalServerInfo, GpuInfo, etc.)
//   settings.rs    — ModelSettings persistence, GGUF/HF file scanning
//   gpu.rs         — GPU detection (NVIDIA, AMD, Intel, Apple, Chinese domestic)
//   server.rs      — LlmServer lifecycle (start/stop), stdout emit [Bug3 fix]
//   proxy.rs       — Unified HTTP proxy, Anthropic↔OpenAI conversion [Bug1+2 fix]
//   model_store.rs — Store fetch, model download (GGUF), llama-server installer

pub mod gpu;
pub mod model_store;
pub mod pid_file;
pub mod proxy;
pub mod server;
pub mod settings;
pub mod types;

// ─── Re-export public API ───
// All callers (tauri commands, services) use `local_llm::foo` directly.

pub use types::{
    DownloadProgress, GgufFile, GpuInfo, HfModelEntry, LocalServerInfo, ModelSettings, ServerLogs,
    SystemInfo,
};

pub use settings::{
    get_download_dir, get_models_dirs, load_model_settings, save_model_settings, scan_gguf_files,
    scan_hf_models, set_download_dir,
};

pub use gpu::{detect_gpu, get_gpu_info, get_system_info};

pub use server::{
    get_server_info, get_server_info_sync, get_server_logs, start_server, stop_server,
};

// Also expose LocalLlmServer for callers using LocalLlmServer::find_llama_server()
pub use server::LocalLlmServer;

pub use model_store::{
    cancel_download, download_llama_server, download_model, fetch_store_models,
    get_local_engine_status, install_local_engine, pause_download,
};
