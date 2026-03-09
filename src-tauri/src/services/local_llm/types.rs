// Types shared across all local_llm submodules

/// Local server status
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalServerInfo {
    pub running: bool,
    pub port: u16,
    pub model_name: String,
    pub pid: Option<u32>,
    pub api_key: String,
    pub runtime: String,
}

impl Default for LocalServerInfo {
    fn default() -> Self {
        Self {
            running: false,
            port: 0,
            model_name: String::new(),
            pid: None,
            api_key: String::new(),
            runtime: "llama-server".to_string(),
        }
    }
}

/// Server logs container
#[derive(Debug, Clone, serde::Serialize)]
pub struct ServerLogs {
    pub logs: Vec<String>,
}

/// Model settings (persisted to ~/.echobird/config/local-model-settings.json)
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ModelSettings {
    #[serde(default)]
    pub models_dirs: Vec<String>,
    #[serde(default)]
    pub download_dir: Option<String>,
    #[serde(default)]
    pub gpu_name: Option<String>,
    #[serde(default)]
    pub gpu_vram_gb: Option<f64>,
}

/// GGUF file entry
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GgufFile {
    pub file_name: String,
    pub file_path: String,
    pub file_size: u64,
}

/// HuggingFace model directory entry (for vLLM / SGLang)
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HfModelEntry {
    pub model_name: String,
    pub model_path: String,
    pub total_size: u64,
}

/// GPU detection result
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GpuInfo {
    pub gpu_name: String,
    pub gpu_vram_gb: f64,
}

/// System information for engine setup UI
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemInfo {
    pub os: String,
    pub arch: String,
    pub gpu_name: Option<String>,
    pub gpu_vram_gb: Option<f64>,
    pub has_gpu: bool,
    pub has_nvidia_gpu: bool,
    pub has_amd_gpu: bool,
}

/// Download progress event payload
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadProgress {
    pub file_name: String,
    pub progress: u32,
    pub downloaded: u64,
    pub total: u64,
    pub status: String, // "downloading" | "completed" | "error" | "cancelled" | "paused" | "speed_test"
}
