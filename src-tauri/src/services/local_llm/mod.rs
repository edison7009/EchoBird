// local_llm module — split from monolithic local_llm.rs into focused submodules
//
// Structure:
//   types.rs       — shared structs (LocalServerInfo, GpuInfo, etc.)
//   settings.rs    — ModelSettings persistence, GGUF/HF file scanning
//   gpu.rs         — GPU detection (NVIDIA, AMD, Intel, Apple, Chinese domestic)
//   server.rs      — LlmServer lifecycle (start/stop), stdout emit [Bug3 fix]
//   proxy.rs       — Unified HTTP proxy, Anthropic↔OpenAI conversion [Bug1+2 fix]
//   model_store.rs — Store fetch, model download (GGUF), llama-server installer

pub mod types;
pub mod settings;
pub mod gpu;
pub mod server;
pub mod proxy;
pub mod model_store;

// ─── Re-export public API ───
// All callers (tauri commands, services) use `local_llm::foo` directly.

pub use types::{
    LocalServerInfo, ServerLogs, ModelSettings, GgufFile, HfModelEntry,
    GpuInfo, SystemInfo, DownloadProgress,
};

pub use settings::{
    load_model_settings, save_model_settings,
    get_models_dirs, get_download_dir, set_download_dir,
    scan_gguf_files, scan_hf_models,
};

pub use gpu::{
    get_system_info, detect_gpu, get_gpu_info,
};

pub use server::{
    get_server_info_sync,
    start_server, stop_server, get_server_info, get_server_logs,
};

// Also expose LocalLlmServer for callers using LocalLlmServer::find_llama_server()
pub use server::LocalLlmServer;

pub use model_store::{
    fetch_store_models,
    download_model, pause_download, cancel_download,
    download_llama_server,
};
