// Local LLM Server �?mirrors old ipc/localModelHandlers.ts
// Manages llama-server: find binary, start/stop, status, model scanning
// Includes Unified Proxy: /v1/* passthrough + /anthropic/* format conversion

use std::path::PathBuf;
use std::process::Command;
#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::sync::watch;

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

/// Server logs
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

/// Local LLM Server Manager
pub struct LocalLlmServer {
    info: LocalServerInfo,
    logs: Vec<String>,
    child_pid: Option<u32>,
    proxy_shutdown: Option<watch::Sender<bool>>,
}

const MAX_LOGS: usize = 1000;

impl LocalLlmServer {
    pub fn new() -> Self {
        Self {
            info: LocalServerInfo::default(),
            logs: Vec::new(),
            child_pid: None,
            proxy_shutdown: None,
        }
    }

    /// Find llama-server executable
    pub fn find_llama_server() -> Option<PathBuf> {
        let exe_name = if cfg!(windows) { "llama-server.exe" } else { "llama-server" };

        // Search order:
        // 1. Next to current exe (bundled in resources)
        if let Ok(exe_path) = std::env::current_exe() {
            if let Some(dir) = exe_path.parent() {
                let candidate = dir.join(exe_name);
                if candidate.exists() {
                    return Some(candidate);
                }
                // resources/ subdirectory
                let candidate = dir.join("resources").join(exe_name);
                if candidate.exists() {
                    return Some(candidate);
                }
            }
        }

        // 2. ~/.echobird/llama-server/bin/ (downloaded by SETUP ENGINE)
        let llama_bin_dir = crate::utils::platform::echobird_dir()
            .join("llama-server").join("bin");
        if llama_bin_dir.exists() {
            // Direct match
            let direct = llama_bin_dir.join(exe_name);
            if direct.exists() {
                return Some(direct);
            }
            // Search subdirectories (up to 2 levels deep for tar.gz nested extraction)
            if let Ok(entries) = std::fs::read_dir(&llama_bin_dir) {
                for entry in entries.flatten() {
                    if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                        let candidate = entry.path().join(exe_name);
                        if candidate.exists() {
                            return Some(candidate);
                        }
                        // Search one more level (e.g. llama-b7981-bin-ubuntu-x64/llama-b7981/)
                        if let Ok(sub_entries) = std::fs::read_dir(entry.path()) {
                            for sub_entry in sub_entries.flatten() {
                                if sub_entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                                    let candidate = sub_entry.path().join(exe_name);
                                    if candidate.exists() {
                                        return Some(candidate);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // 3. ~/.echobird/bin/
        let echobird_bin = crate::utils::platform::echobird_dir().join("bin").join(exe_name);
        if echobird_bin.exists() {
            return Some(echobird_bin);
        }

        // 4. System PATH
        if let Ok(path) = which::which(exe_name) {
            return Some(path);
        }

        None
    }

    /// Start LLM runtime with model
    pub async fn start(
        &mut self,
        model_path: &str,
        port: u16,
        gpu_layers: Option<i32>,
        context_size: Option<u32>,
        runtime: &str,
    ) -> Result<(), String> {
        if self.info.running {
            return Err("Server already running".to_string());
        }

        // Derive model name from path
        let model_name = std::path::Path::new(model_path)
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "Unknown Model".to_string());

        // Generate random API key for this session
        let api_key = generate_session_api_key();

        let (child, needs_proxy) = match runtime {
            "vllm" => {
                // vLLM: python3 -m vllm.entrypoints.openai.api_server
                self.add_log(&format!("Starting vLLM on port {} with model: {}", port, model_name));
                let mut args = vec![
                    "-m".to_string(), "vllm.entrypoints.openai.api_server".to_string(),
                    "--model".to_string(), model_path.to_string(),
                    "--port".to_string(), port.to_string(),
                    "--host".to_string(), "127.0.0.1".to_string(),
                    "--api-key".to_string(), api_key.clone(),
                ];
                if let Some(ctx) = context_size {
                    args.push("--max-model-len".to_string());
                    args.push(ctx.to_string());
                }
                let c = Command::new("python3")
                    .args(&args)
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::piped())
                    .spawn()
                    .map_err(|e| format!("Failed to spawn vLLM: {}", e))?;
                (c, false)
            }
            "sglang" => {
                // SGLang: python3 -m sglang.launch_server
                self.add_log(&format!("Starting SGLang on port {} with model: {}", port, model_name));
                let mut args = vec![
                    "-m".to_string(), "sglang.launch_server".to_string(),
                    "--model-path".to_string(), model_path.to_string(),
                    "--port".to_string(), port.to_string(),
                    "--host".to_string(), "127.0.0.1".to_string(),
                    "--api-key".to_string(), api_key.clone(),
                ];
                if let Some(ctx) = context_size {
                    args.push("--context-length".to_string());
                    args.push(ctx.to_string());
                }
                let c = Command::new("python3")
                    .args(&args)
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::piped())
                    .spawn()
                    .map_err(|e| format!("Failed to spawn SGLang: {}", e))?;
                (c, false)
            }
            _ => {
                // llama-server (default): needs unified proxy
                let exe = Self::find_llama_server()
                    .ok_or_else(|| "llama-server not found".to_string())?;
                let internal_port = port + 100;
                self.add_log(&format!("Starting llama-server on port {} with model: {}", port, model_name));
                self.add_log(&format!("Internal port: {}, Proxy port: {}", internal_port, port));
                let mut args = vec![
                    "-m".to_string(), model_path.to_string(),
                    "--port".to_string(), internal_port.to_string(),
                    "--host".to_string(), "127.0.0.1".to_string(),
                    "--api-key".to_string(), api_key.clone(),
                ];
                if let Some(layers) = gpu_layers {
                    args.push("-ngl".to_string());
                    args.push(layers.to_string());
                }
                if let Some(ctx) = context_size {
                    args.push("-c".to_string());
                    args.push(ctx.to_string());
                }
                log::info!("[LocalLLM] Starting: {:?} (api_key={})", exe, &api_key[..12]);
                let c = Command::new(&exe)
                    .args(&args)
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::piped())
                    .spawn()
                    .map_err(|e| format!("Failed to spawn llama-server: {}", e))?;
                (c, true)
            }
        };

        let pid = child.id();
        self.child_pid = Some(pid);
        self.info = LocalServerInfo {
            running: true,
            port,
            model_name,
            pid: Some(pid),
            api_key,
            runtime: runtime.to_string(),
        };

        log::info!("[LocalLLM] {} started with PID: {}", runtime, pid);
        self.add_log(&format!("{} started (PID: {})", runtime, pid));

        // Only llama-server needs Unified Proxy
        if needs_proxy {
            let (shutdown_tx, shutdown_rx) = watch::channel(false);
            self.proxy_shutdown = Some(shutdown_tx);
            let proxy_port = port;
            let target_port = port + 100;
            tokio::spawn(async move {
                if let Err(e) = run_unified_proxy(proxy_port, target_port, shutdown_rx).await {
                    log::error!("[LocalLLM] Proxy error: {}", e);
                }
            });
            self.add_log("Unified Proxy started:".to_string().as_str());
            self.add_log(&format!("  OpenAI:    http://127.0.0.1:{}/v1", port));
            self.add_log(&format!("  Anthropic: http://127.0.0.1:{}/anthropic", port));
        } else {
            self.add_log(&format!("OpenAI API: http://127.0.0.1:{}/v1", port));
            self.add_log("(native OpenAI endpoint, no proxy needed)");
        }

        Ok(())
    }

    /// Stop the server
    pub async fn stop(&mut self) -> Result<(), String> {
        if !self.info.running {
            return Err("Server not running".to_string());
        }

        if let Some(pid) = self.child_pid {
            log::info!("[LocalLLM] Stopping server (PID: {})", pid);

            #[cfg(windows)]
            {
                let _ = Command::new("taskkill")
                    .args(["/pid", &pid.to_string(), "/T", "/F"])
                    .output();
            }

            #[cfg(not(windows))]
            {
                unsafe { libc::kill(pid as i32, libc::SIGTERM); }
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                unsafe { libc::kill(pid as i32, libc::SIGKILL); }
            }
        }

        // Stop proxy
        if let Some(tx) = self.proxy_shutdown.take() {
            let _ = tx.send(true);
            log::info!("[LocalLLM] Proxy shutdown signal sent");
        }

        self.add_log("Server stopped");
        self.info = LocalServerInfo::default();
        self.child_pid = None;

        Ok(())
    }

    /// Get server info
    pub fn get_info(&self) -> LocalServerInfo {
        self.info.clone()
    }

    /// Get server logs
    pub fn get_logs(&self) -> Vec<String> {
        self.logs.clone()
    }

    /// Add a log entry
    fn add_log(&mut self, msg: &str) {
        let timestamp = chrono::Local::now().format("%H:%M:%S").to_string();
        self.logs.push(format!("[{}] {}", timestamp, msg));
        if self.logs.len() > MAX_LOGS {
            self.logs.drain(0..self.logs.len() - MAX_LOGS);
        }
    }
}

/// Generate a random session API key (eb-sk-{hex})
fn generate_session_api_key() -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    use std::time::SystemTime;

    let mut hasher = DefaultHasher::new();
    SystemTime::now().duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
        .hash(&mut hasher);
    std::process::id().hash(&mut hasher);
    let h1 = hasher.finish();

    let mut hasher2 = DefaultHasher::new();
    h1.hash(&mut hasher2);
    (h1 ^ 0xdeadbeef).hash(&mut hasher2);
    let h2 = hasher2.finish();

    format!("eb-sk-{:016x}{:016x}", h1, h2)
}

// ─── Model settings persistence ───

fn settings_path() -> PathBuf {
    crate::utils::platform::echobird_dir()
        .join("config")
        .join("local-model-settings.json")
}

pub fn load_model_settings() -> ModelSettings {
    let path = settings_path();
    if path.exists() {
        if let Ok(content) = std::fs::read_to_string(&path) {
            if let Ok(settings) = serde_json::from_str(&content) {
                return settings;
            }
        }
    }
    ModelSettings::default()
}

pub fn save_model_settings(settings: &ModelSettings) {
    let path = settings_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let content = serde_json::to_string_pretty(settings).unwrap_or_default();
    let _ = std::fs::write(path, content);
}

/// Get model directories
pub fn get_models_dirs() -> Vec<String> {
    let settings = load_model_settings();
    if !settings.models_dirs.is_empty() {
        return settings.models_dirs;
    }
    // Default: ~/Models
    let default_dir = dirs::home_dir()
        .unwrap_or_default()
        .join("Models")
        .to_string_lossy()
        .to_string();
    vec![default_dir]
}

/// Get download directory
pub fn get_download_dir() -> String {
    let settings = load_model_settings();
    if let Some(dir) = settings.download_dir {
        return dir;
    }
    // Default: ~/Models
    dirs::home_dir()
        .unwrap_or_default()
        .join("Models")
        .to_string_lossy()
        .to_string()
}

/// Set download directory
pub fn set_download_dir(dir: &str) {
    let mut settings = load_model_settings();
    settings.download_dir = Some(dir.to_string());
    save_model_settings(&settings);
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
}

/// Get system information: OS, architecture, and GPU details
pub fn get_system_info() -> SystemInfo {
    let os = std::env::consts::OS.to_string();
    let arch = std::env::consts::ARCH.to_string();
    let gpu = detect_gpu();
    let has_gpu = gpu.is_some();
    SystemInfo {
        os,
        arch,
        gpu_name: gpu.as_ref().map(|g| g.gpu_name.clone()),
        gpu_vram_gb: gpu.as_ref().map(|g| g.gpu_vram_gb),
        has_gpu,
    }
}

/// Detect GPU and persist to settings
pub fn detect_gpu() -> Option<GpuInfo> {
    let info = detect_gpu_system();
    if let Some(ref gpu) = info {
        let mut settings = load_model_settings();
        settings.gpu_name = Some(gpu.gpu_name.clone());
        settings.gpu_vram_gb = Some(gpu.gpu_vram_gb);
        save_model_settings(&settings);
    }
    info
}

/// Get cached GPU info from settings (no re-detection)
pub fn get_gpu_info() -> Option<GpuInfo> {
    let settings = load_model_settings();
    match (settings.gpu_name, settings.gpu_vram_gb) {
        (Some(name), Some(vram)) if !name.is_empty() => Some(GpuInfo {
            gpu_name: name,
            gpu_vram_gb: vram,
        }),
        _ => None,
    }
}

#[cfg(windows)]
fn detect_gpu_system() -> Option<GpuInfo> {
    // 1. Try nvidia-smi first (NVIDIA driver native, accurate VRAM)
    if let Some(info) = detect_gpu_nvidia_smi() {
        return Some(info);
    }

    // 2. Fallback to wmic (for non-NVIDIA GPUs)
    detect_gpu_wmic()
}

#[cfg(windows)]
fn detect_gpu_nvidia_smi() -> Option<GpuInfo> {
    let output = Command::new("nvidia-smi")
        .args(["--query-gpu=name,memory.total", "--format=csv,noheader,nounits"])
        .creation_flags(0x08000000) // CREATE_NO_WINDOW
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    log::info!("[GPU] nvidia-smi output: {}", stdout.trim());

    // Output format: "NVIDIA GeForce RTX 4060, 8192"
    let first_line = stdout.lines().next()?.trim().to_string();
    let parts: Vec<&str> = first_line.split(',').map(|s| s.trim()).collect();
    if parts.len() >= 2 {
        let vram_mb: f64 = parts[1].parse().unwrap_or(0.0);
        if vram_mb > 0.0 {
            let vram_gb = (vram_mb / 1024.0 * 10.0).round() / 10.0;
            let short_name = shorten_gpu_name(parts[0]);
            log::info!("[GPU] nvidia-smi detected: {} ({:.1} GB VRAM)", short_name, vram_gb);
            return Some(GpuInfo {
                gpu_name: short_name,
                gpu_vram_gb: vram_gb,
            });
        }
    }
    None
}

#[cfg(windows)]
fn detect_gpu_wmic() -> Option<GpuInfo> {
    let output = Command::new("wmic")
        .args(["path", "win32_VideoController", "get", "Name,AdapterRAM", "/format:csv"])
        .creation_flags(0x08000000) // CREATE_NO_WINDOW
        .output()
        .ok()?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    log::info!("[GPU] wmic output: {}", stdout.trim());

    let mut best_name = String::new();
    let mut best_vram: u64 = 0;

    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with("Node") {
            continue;
        }
        let parts: Vec<&str> = line.split(',').collect();
        if parts.len() >= 3 {
            let adapter_ram: u64 = parts[1].trim().parse().unwrap_or(0);
            let name = parts[2].trim().to_string();
            if adapter_ram > best_vram && !name.is_empty() {
                best_vram = adapter_ram;
                best_name = name;
            }
        }
    }

    if best_name.is_empty() {
        return None;
    }

    // Note: AdapterRAM from wmic is a 32-bit field, may be capped at ~4GB
    let vram_gb = best_vram as f64 / (1024.0 * 1024.0 * 1024.0);
    let vram_gb = (vram_gb * 10.0).round() / 10.0;
    let short_name = shorten_gpu_name(&best_name);

    log::info!("[GPU] wmic detected: {} ({:.1} GB VRAM)", short_name, vram_gb);

    Some(GpuInfo {
        gpu_name: short_name,
        gpu_vram_gb: vram_gb,
    })
}

#[cfg(not(windows))]
fn detect_gpu_system() -> Option<GpuInfo> {
    // Try all GPU vendors in order -- first match wins
    None
        // International
        .or_else(detect_gpu_nvidia_smi_unix)   // NVIDIA (GeForce/Tesla/H100/H200/A100...)
        .or_else(detect_gpu_rocm)              // AMD ROCm / Hygon DCU
        .or_else(detect_gpu_intel_xpu)         // Intel Arc / Flex / Gaudi
        .or_else(detect_gpu_apple)             // Apple Silicon
        // Chinese domestic
        .or_else(detect_gpu_mthreads)          // Moore Threads 摩尔线程
        .or_else(detect_gpu_iluvatar)          // Iluvatar CoreX 天数智芯
        .or_else(detect_gpu_cambricon)         // Cambricon 寒武纪
        .or_else(detect_gpu_biren)             // Biren 壁仞科技
        .or_else(detect_gpu_kunlunxin)         // KunlunXin 昆仑芯
}

/// Shorten verbose GPU names for display
fn shorten_gpu_name(name: &str) -> String {
    name
        // International brands
        .replace("NVIDIA GeForce ", "")
        .replace("NVIDIA RTX ", "RTX ")
        .replace("NVIDIA Tesla ", "Tesla ")
        .replace("NVIDIA ", "")
        .replace("AMD Radeon RX ", "RX ")
        .replace("AMD Radeon PRO ", "Radeon PRO ")
        .replace("AMD Radeon ", "")
        .replace("Intel(R) Arc\u{2122} ", "Arc ")
        .replace("Intel(R) Data Center GPU ", "Intel DC-GPU ")
        .replace("Intel(R) ", "Intel ")
        .replace("Apple ", "")
        // Chinese domestic brands
        .replace("Moore Threads ", "")
        .replace("Iluvatar CoreX ", "")
        .replace("Cambricon ", "")
        .replace("Biren ", "")
        .replace("KunlunXin ", "")
        // Cleanup
        .replace("(TM)", "")
        .replace("(R)", "")
        .replace("  ", " ")
        .trim()
        .to_string()
}

// === GPU detection functions (non-Windows) ===

#[cfg(not(windows))]
fn detect_gpu_nvidia_smi_unix() -> Option<GpuInfo> {
    let out = Command::new("nvidia-smi")
        .args(["--query-gpu=name,memory.total", "--format=csv,noheader,nounits"])
        .output().ok()?;
    if !out.status.success() { return None; }
    let s = String::from_utf8_lossy(&out.stdout);
    let line = s.lines().next()?.trim().to_string();
    let p: Vec<&str> = line.split(',').map(|x| x.trim()).collect();
    if p.len() >= 2 {
        let mb: f64 = p[1].parse().unwrap_or(0.0);
        if mb > 0.0 {
            let gb = (mb / 1024.0 * 10.0).round() / 10.0;
            log::info!("[GPU] nvidia-smi: {} ({:.1}GB)", p[0], gb);
            return Some(GpuInfo { gpu_name: shorten_gpu_name(p[0]), gpu_vram_gb: gb });
        }
    }
    None
}

#[cfg(not(windows))]
fn detect_gpu_rocm() -> Option<GpuInfo> {
    // AMD ROCm + Hygon DCU
    let out = Command::new("rocm-smi")
        .args(["--showmeminfo", "vram", "--showname", "--csv"])
        .output().ok()?;
    if !out.status.success() { return None; }
    let s = String::from_utf8_lossy(&out.stdout);
    for line in s.lines().skip(1) {
        let p: Vec<&str> = line.split(',').map(|x| x.trim()).collect();
        if p.len() >= 3 {
            let mb: f64 = p[2].parse().unwrap_or(0.0);
            if mb > 0.0 {
                let gb = (mb / 1024.0 * 10.0).round() / 10.0;
                let name = shorten_gpu_name(p[1]);
                log::info!("[GPU] rocm-smi: {} ({:.1}GB)", name, gb);
                return Some(GpuInfo { gpu_name: name, gpu_vram_gb: gb });
            }
        }
    }
    None
}

#[cfg(not(windows))]
fn detect_gpu_intel_xpu() -> Option<GpuInfo> {
    // Intel Arc / Flex / Gaudi
    let out = Command::new("xpu-smi").args(["discovery", "-j"]).output().ok()?;
    if !out.status.success() { return None; }
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).ok()?;
    let dev = json.get("device_list")?.as_array()?.first()?;
    let name = dev.get("device_name").and_then(|v| v.as_str()).unwrap_or("Intel GPU");
    let mb = dev.get("memory_physical_size").and_then(|v| v.as_f64()).unwrap_or(0.0);
    if mb > 0.0 {
        let gb = (mb / 1024.0 * 10.0).round() / 10.0;
        log::info!("[GPU] xpu-smi: {} ({:.1}GB)", name, gb);
        return Some(GpuInfo { gpu_name: shorten_gpu_name(name), gpu_vram_gb: gb });
    }
    None
}

#[cfg(target_os = "macos")]
fn detect_gpu_apple() -> Option<GpuInfo> {
    let out = Command::new("system_profiler")
        .args(["SPDisplaysDataType", "-json"]).output().ok()?;
    if !out.status.success() { return None; }
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).ok()?;
    for d in json.get("SPDisplaysDataType")?.as_array()? {
        let name = d.get("sppci_model").and_then(|v| v.as_str()).unwrap_or("");
        if name.is_empty() { continue; }
        let vraw = d.get("spdisplays_vram").and_then(|v| v.as_str()).unwrap_or("0 MB");
        let mb: f64 = vraw.split_whitespace().next()
            .and_then(|n| n.parse().ok()).unwrap_or(0.0);
        let gb = if mb >= 1024.0 { mb / 1024.0 } else { mb };
        let gb = (gb * 10.0).round() / 10.0;
        log::info!("[GPU] Apple: {} ({:.1}GB)", name, gb);
        return Some(GpuInfo { gpu_name: shorten_gpu_name(name), gpu_vram_gb: gb });
    }
    None
}

#[cfg(all(not(windows), not(target_os = "macos")))]
fn detect_gpu_apple() -> Option<GpuInfo> { None }

#[cfg(not(windows))]
fn detect_gpu_mthreads() -> Option<GpuInfo> {
    // Moore Threads (MTT S80, MTT S3000...)
    let out = Command::new("mthreads-gmi").args(["-q", "--display=MEMORY"]).output().ok()?;
    if !out.status.success() { return None; }
    let name_o = Command::new("mthreads-gmi").args(["-q", "--display=NAME"]).output().ok()?;
    let gb = parse_vram_mb_line(&String::from_utf8_lossy(&out.stdout))?;
    let name = parse_name_colon(&String::from_utf8_lossy(&name_o.stdout))
        .unwrap_or_else(|| "Moore Threads MTT".to_string());
    log::info!("[GPU] mthreads-gmi: {} ({:.1}GB)", name, gb);
    Some(GpuInfo { gpu_name: shorten_gpu_name(&name), gpu_vram_gb: gb })
}

#[cfg(not(windows))]
fn detect_gpu_iluvatar() -> Option<GpuInfo> {
    // Iluvatar CoreX (BI-V100, BI-V150...)
    let out = Command::new("ixsmi").args(["-q", "--display=MEMORY"]).output().ok()?;
    if !out.status.success() { return None; }
    let name_o = Command::new("ixsmi").args(["-q", "--display=NAME"]).output().ok()?;
    let gb = parse_vram_mb_line(&String::from_utf8_lossy(&out.stdout))?;
    let name = parse_name_colon(&String::from_utf8_lossy(&name_o.stdout))
        .unwrap_or_else(|| "Iluvatar CoreX".to_string());
    log::info!("[GPU] ixsmi: {} ({:.1}GB)", name, gb);
    Some(GpuInfo { gpu_name: shorten_gpu_name(&name), gpu_vram_gb: gb })
}

#[cfg(not(windows))]
fn detect_gpu_cambricon() -> Option<GpuInfo> {
    // Cambricon MLU (MLU370, MLU590...)
    let out = Command::new("cnmon").args(["info", "-j"]).output().ok()?;
    if !out.status.success() { return None; }
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).ok()?;
    let dev = json.get("device")?.as_array()?.first()?;
    let name = dev.get("Product Name").or_else(|| dev.get("name"))
        .and_then(|v| v.as_str()).unwrap_or("Cambricon MLU");
    let mb = dev.get("Memory Info").and_then(|m| m.get("Total"))
        .and_then(|v| v.as_f64()).unwrap_or(0.0);
    if mb > 0.0 {
        let gb = (mb / 1024.0 * 10.0).round() / 10.0;
        log::info!("[GPU] cnmon: {} ({:.1}GB)", name, gb);
        return Some(GpuInfo { gpu_name: shorten_gpu_name(name), gpu_vram_gb: gb });
    }
    None
}

#[cfg(not(windows))]
fn detect_gpu_biren() -> Option<GpuInfo> {
    // Biren (BR100, BR104...)
    let out = Command::new("brsmi").args(["-q", "--display=MEMORY"]).output().ok()?;
    if !out.status.success() { return None; }
    let name_o = Command::new("brsmi").args(["-q", "--display=NAME"]).output().ok()?;
    let gb = parse_vram_mb_line(&String::from_utf8_lossy(&out.stdout))?;
    let name = parse_name_colon(&String::from_utf8_lossy(&name_o.stdout))
        .unwrap_or_else(|| "Biren BR".to_string());
    log::info!("[GPU] brsmi: {} ({:.1}GB)", name, gb);
    Some(GpuInfo { gpu_name: shorten_gpu_name(&name), gpu_vram_gb: gb })
}

#[cfg(not(windows))]
fn detect_gpu_kunlunxin() -> Option<GpuInfo> {
    // KunlunXin XPU (K200, K300...)
    let out = Command::new("kunlunxin-smi")
        .args(["--query-xpu=name,memory.total", "--format=csv,noheader,nounits"])
        .output().ok()?;
    if !out.status.success() { return None; }
    let s = String::from_utf8_lossy(&out.stdout);
    let line = s.lines().next()?.trim().to_string();
    let p: Vec<&str> = line.split(',').map(|x| x.trim()).collect();
    if p.len() >= 2 {
        let mb: f64 = p[1].parse().unwrap_or(0.0);
        if mb > 0.0 {
            let gb = (mb / 1024.0 * 10.0).round() / 10.0;
            log::info!("[GPU] kunlunxin-smi: {} ({:.1}GB)", p[0], gb);
            return Some(GpuInfo { gpu_name: shorten_gpu_name(p[0]), gpu_vram_gb: gb });
        }
    }
    None
}

// Windows AMD ROCm
#[cfg(windows)]
fn detect_gpu_rocm() -> Option<GpuInfo> {
    let out = Command::new("rocm-smi")
        .args(["--showmeminfo", "vram", "--showname", "--csv"])
        .creation_flags(0x08000000)
        .output().ok()?;
    if !out.status.success() { return None; }
    let s = String::from_utf8_lossy(&out.stdout);
    for line in s.lines().skip(1) {
        let p: Vec<&str> = line.split(',').map(|x| x.trim()).collect();
        if p.len() >= 3 {
            let mb: f64 = p[2].parse().unwrap_or(0.0);
            if mb > 0.0 {
                let gb = (mb / 1024.0 * 10.0).round() / 10.0;
                return Some(GpuInfo { gpu_name: shorten_gpu_name(p[1]), gpu_vram_gb: gb });
            }
        }
    }
    None
}

/// Parse VRAM total from lines like "Total Memory: 24576 MiB"
fn parse_vram_mb_line(text: &str) -> Option<f64> {
    for line in text.lines() {
        let lower = line.to_lowercase();
        if lower.contains("total") && (lower.contains("mib") || lower.contains("mb")) {
            if let Some(n) = line.split_whitespace().find(|s| s.parse::<f64>().is_ok()) {
                let mb: f64 = n.parse().ok()?;
                if mb > 0.0 { return Some((mb / 1024.0 * 10.0).round() / 10.0); }
            }
        }
    }
    None
}

/// Parse "Product Name : MTT S80" -> "MTT S80"
fn parse_name_colon(text: &str) -> Option<String> {
    text.lines()
        .find(|l| {
            let lower = l.to_lowercase();
            lower.contains("product name") || lower.contains("device name")
        })
        .and_then(|l| l.split(':').nth(1))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Scan for GGUF files in model directories
pub fn scan_gguf_files(dir: &str, max_depth: u32) -> Vec<GgufFile> {
    let mut results = Vec::new();
    scan_gguf_recursive(std::path::Path::new(dir), max_depth, &mut results);
    results
}

fn scan_gguf_recursive(dir: &std::path::Path, depth: u32, results: &mut Vec<GgufFile>) {
    if depth == 0 || !dir.is_dir() {
        return;
    }

    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                if let Some(ext) = path.extension() {
                    if ext.to_string_lossy().to_lowercase() == "gguf" {
                        let file_size = std::fs::metadata(&path)
                            .map(|m| m.len())
                            .unwrap_or(0);
                        results.push(GgufFile {
                            file_name: path.file_name()
                                .map(|n| n.to_string_lossy().to_string())
                                .unwrap_or_default(),
                            file_path: path.to_string_lossy().to_string(),
                            file_size,
                        });
                    }
                }
            } else if path.is_dir() {
                scan_gguf_recursive(&path, depth - 1, results);
            }
        }
    }
}

/// HuggingFace model directory entry (for vLLM / SGLang)
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HfModelEntry {
    pub model_name: String,
    pub model_path: String,
    pub total_size: u64,
}

/// Scan for HuggingFace model directories (folders containing config.json)
pub fn scan_hf_models(dir: &str, max_depth: u32) -> Vec<HfModelEntry> {
    let mut results = Vec::new();
    scan_hf_recursive(std::path::Path::new(dir), max_depth, &mut results);
    results
}

fn scan_hf_recursive(dir: &std::path::Path, depth: u32, results: &mut Vec<HfModelEntry>) {
    if depth == 0 || !dir.is_dir() {
        return;
    }

    // Check if this directory contains config.json (HuggingFace model)
    let config_path = dir.join("config.json");
    if config_path.exists() {
        // Read model name from config.json if possible
        let model_name = std::fs::read_to_string(&config_path)
            .ok()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .and_then(|v| {
                // Try _name_or_path first, then model_type
                v.get("_name_or_path")
                    .and_then(|n| n.as_str().map(String::from))
                    .or_else(|| v.get("model_type").and_then(|n| n.as_str().map(String::from)))
            })
            .unwrap_or_else(|| {
                dir.file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default()
            });

        // Calculate total size of model files (safetensors, bin, pt)
        let total_size = std::fs::read_dir(dir)
            .map(|entries| {
                entries.flatten()
                    .filter(|e| e.path().is_file())
                    .filter(|e| {
                        e.path().extension()
                            .map(|ext| {
                                let ext = ext.to_string_lossy().to_lowercase();
                                ext == "safetensors" || ext == "bin" || ext == "pt"
                            })
                            .unwrap_or(false)
                    })
                    .map(|e| e.metadata().map(|m| m.len()).unwrap_or(0))
                    .sum()
            })
            .unwrap_or(0);

        results.push(HfModelEntry {
            model_name,
            model_path: dir.to_string_lossy().to_string(),
            total_size,
        });
        return; // Don't recurse deeper inside a model directory
    }

    // Recurse into subdirectories
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            if entry.path().is_dir() {
                scan_hf_recursive(&entry.path(), depth - 1, results);
            }
        }
    }
}


use tokio::sync::OnceCell;

static LOCAL_LLM: OnceCell<Arc<Mutex<LocalLlmServer>>> = OnceCell::const_new();

/// Sync cache of server info �?updated on start/stop, readable without async
static SERVER_INFO_CACHE: std::sync::Mutex<Option<LocalServerInfo>> = std::sync::Mutex::new(None);

/// Get server info synchronously (for use in sync contexts like get_models)
pub fn get_server_info_sync() -> LocalServerInfo {
    SERVER_INFO_CACHE
        .lock()
        .unwrap()
        .clone()
        .unwrap_or_default()
}

fn update_server_info_cache(info: &LocalServerInfo) {
    *SERVER_INFO_CACHE.lock().unwrap() = Some(info.clone());
}

async fn get_server() -> Arc<Mutex<LocalLlmServer>> {
    LOCAL_LLM
        .get_or_init(|| async { Arc::new(Mutex::new(LocalLlmServer::new())) })
        .await
        .clone()
}

pub async fn start_server(
    model_path: &str,
    port: u16,
    gpu_layers: Option<i32>,
    context_size: Option<u32>,
    runtime: &str,
) -> Result<(), String> {
    let server = get_server().await;
    let mut server = server.lock().await;
    let result = server.start(model_path, port, gpu_layers, context_size, runtime).await;
    if result.is_ok() {
        update_server_info_cache(&server.get_info());
    }
    result
}

pub async fn stop_server() -> Result<(), String> {
    let server = get_server().await;
    let mut server = server.lock().await;
    let result = server.stop().await;
    if result.is_ok() {
        update_server_info_cache(&server.get_info());
    }
    result
}

pub async fn get_server_info() -> LocalServerInfo {
    let server = get_server().await;
    let server = server.lock().await;
    server.get_info()
}

pub async fn get_server_logs() -> Vec<String> {
    let server = get_server().await;
    let server = server.lock().await;
    server.get_logs()
}

// ─── Model Store: Fetch + Download Engine ───

use std::sync::atomic::{AtomicBool, Ordering};
use futures_util::StreamExt;
use tauri::Emitter;

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

/// Global download abort flag
static DOWNLOAD_ABORT: AtomicBool = AtomicBool::new(false);
static DOWNLOAD_PAUSED: AtomicBool = AtomicBool::new(false);
/// Currently downloading file name
static DOWNLOAD_FILE: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

/// Fetch store models: remote �?cache �?built-in fallback
pub async fn fetch_store_models() -> Vec<serde_json::Value> {
    let remote_url = "https://echobird.ai/api/store/models.json";
    let cache_dir = dirs::home_dir()
        .unwrap_or_default()
        .join(".echobird")
        .join("cache");
    let cache_path = cache_dir.join("store-models.json");

    // 1. Try remote
    if let Ok(resp) = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(8))
        .build()
        .unwrap_or_default()
        .get(remote_url)
        .header("User-Agent", "Echobird/1.1")
        .send()
        .await
    {
        if resp.status().is_success() {
            if let Ok(text) = resp.text().await {
                if let Ok(models) = serde_json::from_str::<Vec<serde_json::Value>>(&text) {
                    if !models.is_empty() {
                        // Save to cache
                        let _ = std::fs::create_dir_all(&cache_dir);
                        let _ = std::fs::write(&cache_path, &text);
                        log::info!("[ModelStore] Loaded {} models from remote", models.len());
                        return models;
                    }
                }
            }
        }
    }

    // 2. Try cache
    if let Ok(text) = std::fs::read_to_string(&cache_path) {
        if let Ok(models) = serde_json::from_str::<Vec<serde_json::Value>>(&text) {
            if !models.is_empty() {
                log::info!("[ModelStore] Loaded {} models from cache", models.len());
                return models;
            }
        }
    }

    // 3. Built-in fallback: empty (frontend has its own static JSON)
    log::warn!("[ModelStore] No models available from remote or cache");
    vec![]
}

/// Download source
#[derive(Debug, Clone)]
struct DownloadSource {
    name: String,
    url: String,
}

/// Build download sources for a given repo + file
fn build_download_sources(repo: &str, file_name: &str) -> Vec<DownloadSource> {
    vec![
        DownloadSource {
            name: "HuggingFace".to_string(),
            url: format!("https://huggingface.co/{}/resolve/main/{}", repo, file_name),
        },
        DownloadSource {
            name: "HF-Mirror".to_string(),
            url: format!("https://hf-mirror.com/{}/resolve/main/{}", repo, file_name),
        },
        DownloadSource {
            name: "ModelScope".to_string(),
            url: format!("https://modelscope.cn/models/{}/resolve/master/{}", repo, file_name),
        },
    ]
}

/// Test download speed of a single source (5 seconds)
async fn test_source_speed(source: &DownloadSource) -> (String, f64) {
    let test_duration = std::time::Duration::from_secs(5);
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()
        .unwrap_or_default();

    let start = std::time::Instant::now();
    let mut bytes: u64 = 0;

    match client
        .get(&source.url)
        .header("User-Agent", "Echobird/1.1")
        .send()
        .await
    {
        Ok(resp) if resp.status().is_success() => {
            let mut stream = resp.bytes_stream();
            while let Some(chunk) = stream.next().await {
                if start.elapsed() >= test_duration {
                    break;
                }
                if let Ok(data) = chunk {
                    bytes += data.len() as u64;
                }
            }
        }
        _ => {}
    }

    let elapsed = start.elapsed().as_secs_f64();
    let speed = if elapsed > 0.0 { bytes as f64 / elapsed } else { 0.0 };
    log::info!(
        "[ModelStore] Speed test {}: {:.0} KB/s ({:.0} KB in {:.1}s)",
        source.name,
        speed / 1024.0,
        bytes as f64 / 1024.0,
        elapsed
    );
    (source.name.clone(), speed)
}

/// Download model file with speed test + resume + progress events
pub async fn download_model(
    app_handle: tauri::AppHandle,
    repo: String,
    file_name: String,
) -> Result<String, String> {
    let download_dir = get_download_dir();
    let save_path = PathBuf::from(&download_dir).join(&file_name);
    let temp_path = PathBuf::from(&download_dir).join(format!("{}.downloading", file_name));

    // Reset abort/pause flags
    DOWNLOAD_ABORT.store(false, Ordering::SeqCst);
    DOWNLOAD_PAUSED.store(false, Ordering::SeqCst);
    *DOWNLOAD_FILE.lock().unwrap() = Some(file_name.clone());

    // Ensure download dir exists
    let _ = std::fs::create_dir_all(&download_dir);

    let sources = build_download_sources(&repo, &file_name);

    // Emit speed test status
    let _ = app_handle.emit("download-progress", DownloadProgress {
        file_name: file_name.clone(),
        progress: 0,
        downloaded: 0,
        total: 0,
        status: "speed_test".to_string(),
    });

    // Speed test all sources in parallel
    log::info!("[ModelStore] Speed testing {} sources for {}...", sources.len(), file_name);
    let mut speed_futures = Vec::new();
    for source in &sources {
        speed_futures.push(test_source_speed(source));
    }
    let speed_results = futures_util::future::join_all(speed_futures).await;

    // Sort by speed (fastest first), filter out unreachable
    let mut sorted: Vec<_> = speed_results
        .into_iter()
        .filter(|(_, speed)| *speed > 0.0)
        .collect();
    sorted.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    if sorted.is_empty() {
        let _ = app_handle.emit("download-progress", DownloadProgress {
            file_name: file_name.clone(),
            progress: 0,
            downloaded: 0,
            total: 0,
            status: "error".to_string(),
        });
        return Err("All download sources unreachable".to_string());
    }

    log::info!("[ModelStore] Fastest source: {} ({:.0} KB/s)", sorted[0].0, sorted[0].1 / 1024.0);

    // Try each source in speed order
    for (source_name, _) in &sorted {
        let source = sources.iter().find(|s| &s.name == source_name).unwrap();
        match try_download_from_source(&app_handle, source, &save_path, &temp_path, &file_name).await {
            Ok(path) => {
                *DOWNLOAD_FILE.lock().unwrap() = None;
                return Ok(path);
            }
            Err(e) => {
                log::warn!("[ModelStore] {} failed: {}", source_name, e);
                // Check if user cancelled/paused
                if DOWNLOAD_ABORT.load(Ordering::SeqCst) {
                    *DOWNLOAD_FILE.lock().unwrap() = None;
                    return Err("Download cancelled".to_string());
                }
                if DOWNLOAD_PAUSED.load(Ordering::SeqCst) {
                    // Keep DOWNLOAD_FILE so cancel_download can find the temp file
                    return Err("Download paused".to_string());
                }
            }
        }
    }

    *DOWNLOAD_FILE.lock().unwrap() = None;
    let _ = app_handle.emit("download-progress", DownloadProgress {
        file_name: file_name.clone(),
        progress: 0,
        downloaded: 0,
        total: 0,
        status: "error".to_string(),
    });
    Err("All download sources failed".to_string())
}

/// Try downloading from a single source with resume support
async fn try_download_from_source(
    app_handle: &tauri::AppHandle,
    source: &DownloadSource,
    save_path: &PathBuf,
    temp_path: &PathBuf,
    file_name: &str,
) -> Result<String, String> {
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::limited(10))
        .build()
        .map_err(|e| e.to_string())?;

    // Check for existing partial download
    let start_byte: u64 = if temp_path.exists() {
        std::fs::metadata(temp_path).map(|m| m.len()).unwrap_or(0)
    } else {
        0
    };

    if start_byte > 0 {
        log::info!("[ModelStore] [{}] Resume mode, {} bytes already downloaded", source.name, start_byte);
    }

    let mut request = client
        .get(&source.url)
        .header("User-Agent", "Echobird/1.1");

    if start_byte > 0 {
        request = request.header("Range", format!("bytes={}-", start_byte));
    }

    let resp = request.send().await.map_err(|e| format!("[{}] {}", source.name, e))?;

    let status = resp.status();
    if status != reqwest::StatusCode::OK && status != reqwest::StatusCode::PARTIAL_CONTENT {
        return Err(format!("[{}] HTTP {}", source.name, status.as_u16()));
    }

    // If server returns 200 instead of 206, it doesn't support resume
    let actual_start = if status == reqwest::StatusCode::OK && start_byte > 0 {
        0u64 // Start from beginning
    } else {
        start_byte
    };

    let content_length = resp.content_length().unwrap_or(0);
    let total_size = actual_start + content_length;

    // Open file for writing
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .append(actual_start > 0)
        .truncate(actual_start == 0)
        .open(temp_path)
        .map_err(|e| format!("File open error: {}", e))?;

    let mut downloaded = actual_start;
    let mut stream = resp.bytes_stream();
    let mut last_emit = std::time::Instant::now();

    while let Some(chunk) = stream.next().await {
        // Check abort/pause
        if DOWNLOAD_ABORT.load(Ordering::SeqCst) {
            return Err("Download cancelled".to_string());
        }
        if DOWNLOAD_PAUSED.load(Ordering::SeqCst) {
            let _ = app_handle.emit("download-progress", DownloadProgress {
                file_name: file_name.to_string(),
                progress: 0,
                downloaded,
                total: total_size,
                status: "paused".to_string(),
            });
            return Err("Download paused".to_string());
        }

        let data = chunk.map_err(|e| format!("[{}] Stream error: {}", source.name, e))?;
        file.write_all(&data).map_err(|e| format!("Write error: {}", e))?;
        downloaded += data.len() as u64;

        // Emit progress (~4 times per second)
        if last_emit.elapsed() >= std::time::Duration::from_millis(250) {
            let progress = if total_size > 0 {
                ((downloaded as f64 / total_size as f64) * 100.0) as u32
            } else {
                0
            };
            let _ = app_handle.emit("download-progress", DownloadProgress {
                file_name: file_name.to_string(),
                progress,
                downloaded,
                total: total_size,
                status: "downloading".to_string(),
            });
            last_emit = std::time::Instant::now();
        }
    }

    // Rename temp file to final
    if save_path.exists() {
        let _ = std::fs::remove_file(save_path);
    }
    std::fs::rename(temp_path, save_path)
        .map_err(|e| format!("Rename error: {}", e))?;

    let _ = app_handle.emit("download-progress", DownloadProgress {
        file_name: file_name.to_string(),
        progress: 100,
        downloaded: total_size,
        total: total_size,
        status: "completed".to_string(),
    });

    log::info!("[ModelStore] [{}] Download complete: {}", source.name, file_name);
    Ok(save_path.to_string_lossy().to_string())
}

/// Pause current download (keeps .downloading file for resume)
pub fn pause_download() {
    DOWNLOAD_PAUSED.store(true, Ordering::SeqCst);
    log::info!("[ModelStore] Download paused");
}

/// Cancel download and delete temp file
pub fn cancel_download(app_handle: &tauri::AppHandle, target_file_name: Option<String>) {
    DOWNLOAD_ABORT.store(true, Ordering::SeqCst);

    let file_name = target_file_name
        .or_else(|| DOWNLOAD_FILE.lock().unwrap().clone());

    if let Some(name) = &file_name {
        let download_dir = get_download_dir();
        let temp_path = PathBuf::from(&download_dir).join(format!("{}.downloading", name));
        if temp_path.exists() {
            let _ = std::fs::remove_file(&temp_path);
            log::info!("[ModelStore] Cleaned temp file: {}", temp_path.display());
        }
        // Emit cancelled event so frontend UI updates (especially when cancelling a paused download)
        let _ = app_handle.emit("download-progress", DownloadProgress {
            file_name: name.clone(),
            progress: 0,
            downloaded: 0,
            total: 0,
            status: "cancelled".to_string(),
        });
    }

    // Clear the tracked download file
    *DOWNLOAD_FILE.lock().unwrap() = None;

    log::info!("[ModelStore] Download cancelled");
}

// ─── Engine Download: llama-server binary installer ───

const LLAMA_VERSION: &str = "b7981";
const LLAMA_CUDA_VER: &str = "13.1";

/// GitHub base URL for llama.cpp releases
fn llama_github_base() -> String {
    format!("https://github.com/ggml-org/llama.cpp/releases/download/{}", LLAMA_VERSION)
}

/// Download mirrors (GitHub direct + China proxies)
fn llama_download_mirrors() -> Vec<String> {
    let base = llama_github_base();
    vec![
        base.clone(),
        format!("https://ghfast.top/{}", base),
        format!("https://ghproxy.net/{}", base),
        format!("https://ghproxy.homeboyc.cn/{}", base),
        format!("https://github.ur1.fun/{}", base),
        format!("https://gh-proxy.com/{}", base),
        format!("https://mirror.ghproxy.com/{}", base),
    ]
}

/// Get platform-specific download file names
fn get_llama_platform_files() -> Vec<String> {
    match std::env::consts::OS {
        "windows" => vec![
            format!("llama-{}-bin-win-cuda-{}-x64.zip", LLAMA_VERSION, LLAMA_CUDA_VER),
            format!("cudart-llama-bin-win-cuda-{}-x64.zip", LLAMA_CUDA_VER),
        ],
        "macos" => vec![
            format!("llama-{}-bin-macos-arm64.tar.gz", LLAMA_VERSION),
        ],
        _ => {
            let arch = std::env::consts::ARCH;
            if arch == "aarch64" || arch == "arm" {
                vec![format!("llama-{}-bin-ubuntu-arm64.tar.gz", LLAMA_VERSION)]
            } else {
                vec![format!("llama-{}-bin-ubuntu-x64.tar.gz", LLAMA_VERSION)]
            }
        }
    }
}

/// Get llama-server install directory
fn llama_install_dir() -> PathBuf {
    crate::utils::platform::echobird_dir().join("llama-server")
}

/// Test download speed of a mirror for engine download (5 seconds)
async fn test_mirror_speed(url: String, name: String) -> (String, String, f64) {
    let client = match reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::limited(10))
        .timeout(std::time::Duration::from_secs(10))
        .build() {
        Ok(c) => c,
        Err(_) => return (name, url, 0.0),
    };

    let start = std::time::Instant::now();
    let mut bytes: u64 = 0;

    match client.get(&url).header("User-Agent", "Echobird/1.1").send().await {
        Ok(resp) => {
            if !resp.status().is_success() {
                return (name, url, 0.0);
            }
            let mut stream = resp.bytes_stream();
            while let Some(chunk) = stream.next().await {
                if let Ok(data) = chunk {
                    bytes += data.len() as u64;
                }
                if start.elapsed() >= std::time::Duration::from_secs(5) {
                    break;
                }
            }
        }
        Err(_) => return (name, url, 0.0),
    }

    let elapsed = start.elapsed().as_secs_f64();
    let speed = if elapsed > 0.0 { bytes as f64 / elapsed } else { 0.0 };
    log::info!("[LlamaDownloader] Speed test {}: {:.0} KB/s ({} KB in {:.1}s)",
        name, speed / 1024.0, bytes / 1024, elapsed);
    (name, url, speed)
}

/// Download and install llama-server binary
pub async fn download_llama_server(
    app_handle: tauri::AppHandle,
) -> Result<String, String> {
    let file_names = get_llama_platform_files();
    let bin_dir = llama_install_dir().join("bin");
    let temp_dir = llama_install_dir().join("temp");
    let mirrors = llama_download_mirrors();

    // Reset abort flag
    DOWNLOAD_ABORT.store(false, Ordering::SeqCst);
    DOWNLOAD_PAUSED.store(false, Ordering::SeqCst);
    *DOWNLOAD_FILE.lock().unwrap() = Some("llama-server".to_string());

    let _ = std::fs::create_dir_all(&bin_dir);
    let _ = std::fs::create_dir_all(&temp_dir);

    let total_files = file_names.len();
    let mut completed_files = 0u32;

    for file_name in &file_names {
        if DOWNLOAD_ABORT.load(Ordering::SeqCst) {
            // Clean up on cancel
            let _ = std::fs::remove_dir_all(&temp_dir);
            let _ = std::fs::remove_dir_all(&bin_dir);
            *DOWNLOAD_FILE.lock().unwrap() = None;
            return Err("Download cancelled".to_string());
        }

        let temp_file = temp_dir.join(file_name);

        // Emit speed test status
        let _ = app_handle.emit("download-progress", DownloadProgress {
            file_name: "llama-server".to_string(),
            progress: 0,
            downloaded: 0,
            total: 0,
            status: "speed_test".to_string(),
        });

        // Speed test all mirrors in parallel
        log::info!("[LlamaDownloader] Speed testing {} mirrors for {}...", mirrors.len(), file_name);
        let mut speed_futures = Vec::new();
        for (i, mirror) in mirrors.iter().enumerate() {
            let url = format!("{}/{}", mirror, file_name);
            let name = if i == 0 {
                "GitHub".to_string()
            } else {
                url::Url::parse(mirror).map(|u| u.host_str().unwrap_or("unknown").to_string()).unwrap_or_else(|_| format!("Mirror-{}", i))
            };
            speed_futures.push(test_mirror_speed(url, name));
        }
        let speed_results = futures_util::future::join_all(speed_futures).await;

        // Sort by speed descending, filter unreachable
        let mut sorted: Vec<_> = speed_results
            .into_iter()
            .filter(|(_, _, speed)| *speed > 0.0)
            .collect();
        sorted.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));

        if sorted.is_empty() {
            let _ = app_handle.emit("download-progress", DownloadProgress {
                file_name: "llama-server".to_string(),
                progress: 0, downloaded: 0, total: 0,
                status: "error".to_string(),
            });
            *DOWNLOAD_FILE.lock().unwrap() = None;
            return Err("All download mirrors unreachable".to_string());
        }

        log::info!("[LlamaDownloader] Fastest: {} ({:.0} KB/s)", sorted[0].0, sorted[0].2 / 1024.0);

        // Try each mirror in speed order
        let mut download_ok = false;
        for (mirror_name, mirror_url, _) in &sorted {
            if DOWNLOAD_ABORT.load(Ordering::SeqCst) {
                break;
            }

            log::info!("[LlamaDownloader] Downloading via {}: {}", mirror_name, mirror_url);
            match download_engine_file(&app_handle, mirror_url, &temp_file, completed_files, total_files as u32).await {
                Ok(_) => {
                    download_ok = true;
                    break;
                }
                Err(e) => {
                    log::warn!("[LlamaDownloader] {} failed: {}", mirror_name, e);
                    let _ = std::fs::remove_file(&temp_file);
                    if DOWNLOAD_ABORT.load(Ordering::SeqCst) {
                        break;
                    }
                }
            }
        }

        if DOWNLOAD_ABORT.load(Ordering::SeqCst) {
            let _ = std::fs::remove_dir_all(&temp_dir);
            let _ = std::fs::remove_dir_all(&bin_dir);
            *DOWNLOAD_FILE.lock().unwrap() = None;
            return Err("Download cancelled".to_string());
        }

        if !download_ok {
            let _ = std::fs::remove_dir_all(&temp_dir);
            let _ = app_handle.emit("download-progress", DownloadProgress {
                file_name: "llama-server".to_string(),
                progress: 0, downloaded: 0, total: 0,
                status: "error".to_string(),
            });
            *DOWNLOAD_FILE.lock().unwrap() = None;
            return Err("All download mirrors failed".to_string());
        }

        // Extract
        log::info!("[LlamaDownloader] Extracting: {}", file_name);
        let extract_name = file_name
            .replace(".zip", "")
            .replace(".tar.gz", "");
        let extract_dir = bin_dir.join(&extract_name);
        let _ = std::fs::create_dir_all(&extract_dir);

        if file_name.ends_with(".zip") {
            // Windows: PowerShell Expand-Archive
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                let status = Command::new("powershell")
                    .args(["-NoProfile", "-Command",
                        &format!("Expand-Archive -Path '{}' -DestinationPath '{}' -Force",
                            temp_file.display(), extract_dir.display())])
                    .creation_flags(0x08000000) // CREATE_NO_WINDOW
                    .status()
                    .map_err(|e| format!("Extract failed: {}", e))?;
                if !status.success() {
                    return Err(format!("PowerShell Expand-Archive failed for {}", file_name));
                }
            }
            #[cfg(not(windows))]
            {
                // Non-Windows should not produce .zip files, but handle gracefully
                return Err("ZIP extraction is only supported on Windows".to_string());
            }
        } else {
            // macOS/Linux: tar -xzf
            let status = Command::new("tar")
                .args(["-xzf", &temp_file.to_string_lossy(), "-C", &extract_dir.to_string_lossy()])
                .status()
                .map_err(|e| format!("Extract failed: {}", e))?;
            if !status.success() {
                return Err(format!("tar extraction failed for {}", file_name));
            }
        }

        // Clean temp file
        let _ = std::fs::remove_file(&temp_file);
        completed_files += 1;
    }

    // Linux/macOS: set executable permission
    #[cfg(not(windows))]
    {
        if let Some(exe_path) = LocalLlmServer::find_llama_server() {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&exe_path, std::fs::Permissions::from_mode(0o755));
            log::info!("[LlamaDownloader] Set executable permission: {}", exe_path.display());
        }
    }

    // Clean temp dir
    let _ = std::fs::remove_dir_all(&temp_dir);

    // Emit completed
    let _ = app_handle.emit("download-progress", DownloadProgress {
        file_name: "llama-server".to_string(),
        progress: 100,
        downloaded: 0,
        total: 0,
        status: "completed".to_string(),
    });

    *DOWNLOAD_FILE.lock().unwrap() = None;

    let install_path = LocalLlmServer::find_llama_server()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();
    log::info!("[LlamaDownloader] Installation complete: {}", install_path);
    Ok(install_path)
}

/// Download a single engine file with progress reporting
async fn download_engine_file(
    app_handle: &tauri::AppHandle,
    url: &str,
    dest: &PathBuf,
    completed_files: u32,
    total_files: u32,
) -> Result<(), String> {
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::limited(10))
        .build()
        .map_err(|e| e.to_string())?;

    let resp = client.get(url)
        .header("User-Agent", "Echobird/1.1")
        .send().await
        .map_err(|e| e.to_string())?;

    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status().as_u16()));
    }

    let content_length = resp.content_length().unwrap_or(0);
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .create(true).write(true).truncate(true)
        .open(dest)
        .map_err(|e| format!("File open error: {}", e))?;

    let mut downloaded: u64 = 0;
    let mut stream = resp.bytes_stream();
    let mut last_emit = std::time::Instant::now();

    while let Some(chunk) = stream.next().await {
        if DOWNLOAD_ABORT.load(Ordering::SeqCst) {
            return Err("Download cancelled".to_string());
        }

        let data = chunk.map_err(|e| format!("Stream error: {}", e))?;
        file.write_all(&data).map_err(|e| format!("Write error: {}", e))?;
        downloaded += data.len() as u64;

        if last_emit.elapsed() >= std::time::Duration::from_millis(250) {
            let file_progress = if content_length > 0 {
                (downloaded as f64 / content_length as f64) * 100.0
            } else { 0.0 };
            // Overall progress across all files
            let overall = ((completed_files as f64 + file_progress / 100.0) / total_files as f64 * 100.0) as u32;

            let _ = app_handle.emit("download-progress", DownloadProgress {
                file_name: "llama-server".to_string(),
                progress: overall,
                downloaded,
                total: content_length,
                status: "downloading".to_string(),
            });
            last_emit = std::time::Instant::now();
        }
    }

    Ok(())
}

// ============================================================
// Unified Proxy Server
// ============================================================
// Mirrors the old Electron localModelHandlers.ts unified proxy:
//   /v1/*        �?Direct passthrough to llama-server (OpenAI native)
//   /anthropic/* �?Anthropic→OpenAI format conversion then forward

/// Run the unified proxy HTTP server
async fn run_unified_proxy(
    listen_port: u16,
    target_port: u16,
    mut shutdown_rx: watch::Receiver<bool>,
) -> Result<(), String> {
    use tokio::net::TcpListener;

    let addr = format!("0.0.0.0:{}", listen_port);
    let listener = TcpListener::bind(&addr).await
        .map_err(|e| format!("Failed to bind proxy on {}: {}", addr, e))?;

    log::info!("[Proxy] Unified proxy listening on port {}", listen_port);

    loop {
        tokio::select! {
            _ = shutdown_rx.changed() => {
                if *shutdown_rx.borrow() {
                    log::info!("[Proxy] Shutdown received, stopping proxy");
                    break;
                }
            }
            accept = listener.accept() => {
                match accept {
                    Ok((stream, _peer)) => {
                        let tp = target_port;
                        tokio::spawn(async move {
                            if let Err(e) = handle_proxy_connection(stream, tp).await {
                                log::warn!("[Proxy] Connection error: {}", e);
                            }
                        });
                    }
                    Err(e) => {
                        log::warn!("[Proxy] Accept error: {}", e);
                    }
                }
            }
        }
    }

    Ok(())
}

/// Handle a single proxy connection
async fn handle_proxy_connection(
    mut stream: tokio::net::TcpStream,
    target_port: u16,
) -> Result<(), String> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    // Read the HTTP request
    let mut buf = vec![0u8; 65536];
    let n = stream.read(&mut buf).await
        .map_err(|e| format!("Read error: {}", e))?;
    if n == 0 { return Ok(()); }

    let raw = &buf[..n];
    let raw_str = String::from_utf8_lossy(raw);

    // Parse request line
    let first_line = raw_str.lines().next().unwrap_or("");
    let parts: Vec<&str> = first_line.split_whitespace().collect();
    if parts.len() < 2 {
        let resp = "HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\n\r\n";
        let _ = stream.write_all(resp.as_bytes()).await;
        return Ok(());
    }

    let method = parts[0];
    let path = parts[1];

    // CORS preflight
    if method == "OPTIONS" {
        let resp = "HTTP/1.1 204 No Content\r\n\
            Access-Control-Allow-Origin: *\r\n\
            Access-Control-Allow-Methods: GET, POST, OPTIONS\r\n\
            Access-Control-Allow-Headers: *\r\n\
            Content-Length: 0\r\n\r\n";
        let _ = stream.write_all(resp.as_bytes()).await;
        return Ok(());
    }

    if path.starts_with("/anthropic") {
        // Anthropic format conversion proxy
        handle_anthropic_proxy(&mut stream, raw, target_port).await
    } else {
        // Direct passthrough (OpenAI and other paths)
        handle_passthrough(&mut stream, raw, target_port).await
    }
}

/// Direct passthrough: forward request to llama-server as-is
async fn handle_passthrough(
    stream: &mut tokio::net::TcpStream,
    raw_request: &[u8],
    target_port: u16,
) -> Result<(), String> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpStream;

    let mut target = TcpStream::connect(format!("127.0.0.1:{}", target_port)).await
        .map_err(|e| format!("Connect to llama-server failed: {}", e))?;

    // Forward request
    target.write_all(raw_request).await
        .map_err(|e| format!("Write to target: {}", e))?;

    // Pipe response back
    let mut resp_buf = vec![0u8; 8192];
    loop {
        let n = target.read(&mut resp_buf).await
            .map_err(|e| format!("Read from target: {}", e))?;
        if n == 0 { break; }
        stream.write_all(&resp_buf[..n]).await
            .map_err(|e| format!("Write to client: {}", e))?;
    }

    Ok(())
}

/// Anthropic proxy: convert Anthropic Messages API �?OpenAI Chat Completions API
async fn handle_anthropic_proxy(
    stream: &mut tokio::net::TcpStream,
    raw_request: &[u8],
    target_port: u16,
) -> Result<(), String> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let raw_str = String::from_utf8_lossy(raw_request);

    // Find body (after \r\n\r\n)
    let body_str = if let Some(pos) = raw_str.find("\r\n\r\n") {
        &raw_str[pos + 4..]
    } else {
        ""
    };

    // Parse Anthropic request body
    let anthropic_req: serde_json::Value = serde_json::from_str(body_str)
        .unwrap_or_else(|_| serde_json::json!({}));

    let is_stream = anthropic_req.get("stream").and_then(|v| v.as_bool()).unwrap_or(false);

    // Convert Anthropic �?OpenAI
    let openai_req = anthropic_to_openai(&anthropic_req);

    log::info!("[Proxy] Anthropic �?OpenAI: {} messages, stream={}",
        openai_req.get("messages").and_then(|m| m.as_array()).map(|a| a.len()).unwrap_or(0),
        is_stream
    );

    let post_data = serde_json::to_string(&openai_req)
        .map_err(|e| format!("Serialize: {}", e))?;

    // Forward to llama-server's /v1/chat/completions
    let http_req = format!(
        "POST /v1/chat/completions HTTP/1.1\r\n\
         Host: 127.0.0.1:{}\r\n\
         Content-Type: application/json\r\n\
         Content-Length: {}\r\n\
         Connection: close\r\n\r\n{}",
        target_port, post_data.len(), post_data
    );

    let mut target = tokio::net::TcpStream::connect(format!("127.0.0.1:{}", target_port)).await
        .map_err(|e| format!("Connect to llama-server: {}", e))?;

    target.write_all(http_req.as_bytes()).await
        .map_err(|e| format!("Write to target: {}", e))?;

    // Read full response from llama-server
    let mut resp_data = Vec::new();
    let mut buf = vec![0u8; 8192];
    loop {
        let n = target.read(&mut buf).await
            .map_err(|e| format!("Read from target: {}", e))?;
        if n == 0 { break; }
        resp_data.extend_from_slice(&buf[..n]);
    }

    let resp_str = String::from_utf8_lossy(&resp_data);

    // Find response body
    let resp_body = if let Some(pos) = resp_str.find("\r\n\r\n") {
        &resp_str[pos + 4..]
    } else {
        &resp_str[..]
    };

    if is_stream {
        // Stream: convert OpenAI SSE �?Anthropic SSE
        let anthropic_sse = openai_stream_to_anthropic_sse(resp_body);

        let headers = format!(
            "HTTP/1.1 200 OK\r\n\
             Content-Type: text/event-stream\r\n\
             Cache-Control: no-cache\r\n\
             Connection: keep-alive\r\n\
             Access-Control-Allow-Origin: *\r\n\r\n"
        );
        stream.write_all(headers.as_bytes()).await
            .map_err(|e| format!("Write headers: {}", e))?;
        stream.write_all(anthropic_sse.as_bytes()).await
            .map_err(|e| format!("Write SSE: {}", e))?;
    } else {
        // Non-stream: convert OpenAI JSON �?Anthropic JSON
        match serde_json::from_str::<serde_json::Value>(resp_body) {
            Ok(openai_data) => {
                let anthropic_resp = openai_to_anthropic(&openai_data);
                let body = serde_json::to_string(&anthropic_resp).unwrap_or_default();
                let resp = format!(
                    "HTTP/1.1 200 OK\r\n\
                     Content-Type: application/json\r\n\
                     Access-Control-Allow-Origin: *\r\n\
                     Content-Length: {}\r\n\r\n{}",
                    body.len(), body
                );
                stream.write_all(resp.as_bytes()).await
                    .map_err(|e| format!("Write response: {}", e))?;
            }
            Err(e) => {
                let err_body = serde_json::json!({
                    "type": "error",
                    "error": { "type": "api_error", "message": format!("Parse error: {}", e) }
                }).to_string();
                let resp = format!(
                    "HTTP/1.1 500 Internal Server Error\r\n\
                     Content-Type: application/json\r\n\
                     Access-Control-Allow-Origin: *\r\n\
                     Content-Length: {}\r\n\r\n{}",
                    err_body.len(), err_body
                );
                stream.write_all(resp.as_bytes()).await
                    .map_err(|e| format!("Write error: {}", e))?;
            }
        }
    }

    Ok(())
}

// ============================================================
// Format Conversion: Anthropic �?OpenAI
// ============================================================

/// Convert Anthropic Messages request �?OpenAI Chat Completions request
fn anthropic_to_openai(body: &serde_json::Value) -> serde_json::Value {
    let mut messages = Vec::new();

    // Handle system message
    if let Some(system) = body.get("system") {
        let system_text = if let Some(s) = system.as_str() {
            s.to_string()
        } else if let Some(arr) = system.as_array() {
            arr.iter()
                .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
                .collect::<Vec<_>>()
                .join("")
        } else {
            String::new()
        };
        if !system_text.is_empty() {
            messages.push(serde_json::json!({"role": "system", "content": system_text}));
        }
    }

    // Handle conversation messages
    if let Some(msgs) = body.get("messages").and_then(|m| m.as_array()) {
        for msg in msgs {
            let role = msg.get("role").and_then(|r| r.as_str()).unwrap_or("user");
            let content = if let Some(s) = msg.get("content").and_then(|c| c.as_str()) {
                s.to_string()
            } else if let Some(arr) = msg.get("content").and_then(|c| c.as_array()) {
                arr.iter()
                    .filter(|b| b.get("type").and_then(|t| t.as_str()) == Some("text"))
                    .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
                    .collect::<Vec<_>>()
                    .join("")
            } else {
                String::new()
            };
            messages.push(serde_json::json!({"role": role, "content": content}));
        }
    }

    serde_json::json!({
        "model": body.get("model").and_then(|m| m.as_str()).unwrap_or("local-model"),
        "messages": messages,
        "max_tokens": body.get("max_tokens").and_then(|v| v.as_u64()).unwrap_or(4096),
        "temperature": body.get("temperature").and_then(|v| v.as_f64()).unwrap_or(0.7),
        "top_p": body.get("top_p").and_then(|v| v.as_f64()).unwrap_or(0.9),
        "stream": body.get("stream").and_then(|v| v.as_bool()).unwrap_or(false),
    })
}

/// Convert OpenAI Chat Completions non-streaming response �?Anthropic Messages response
fn openai_to_anthropic(data: &serde_json::Value) -> serde_json::Value {
    let content_text = data.get("choices")
        .and_then(|c| c.as_array())
        .and_then(|arr| arr.first())
        .and_then(|c| c.get("message"))
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_str())
        .unwrap_or("");

    let model = data.get("model").and_then(|m| m.as_str()).unwrap_or("local-model");

    serde_json::json!({
        "id": format!("msg_{}", chrono::Utc::now().timestamp_millis()),
        "type": "message",
        "role": "assistant",
        "content": [{"type": "text", "text": content_text}],
        "model": model,
        "stop_reason": "end_turn",
        "stop_sequence": null,
        "usage": {
            "input_tokens": data.get("usage").and_then(|u| u.get("prompt_tokens")).and_then(|v| v.as_u64()).unwrap_or(0),
            "output_tokens": data.get("usage").and_then(|u| u.get("completion_tokens")).and_then(|v| v.as_u64()).unwrap_or(0),
        }
    })
}

/// Convert OpenAI SSE stream �?Anthropic SSE stream
fn openai_stream_to_anthropic_sse(sse_data: &str) -> String {
    let msg_id = format!("msg_{}", chrono::Utc::now().timestamp_millis());
    let mut output = String::new();

    // message_start
    let msg_start = serde_json::json!({
        "type": "message_start",
        "message": {
            "id": msg_id,
            "type": "message",
            "role": "assistant",
            "content": [],
            "model": "local-model",
            "stop_reason": null,
            "stop_sequence": null,
            "usage": {"input_tokens": 0, "output_tokens": 0}
        }
    });
    output.push_str(&format!("event: message_start\ndata: {}\n\n", msg_start));

    // content_block_start
    let block_start = serde_json::json!({
        "type": "content_block_start",
        "index": 0,
        "content_block": {"type": "text", "text": ""}
    });
    output.push_str(&format!("event: content_block_start\ndata: {}\n\n", block_start));

    // Parse OpenAI SSE lines and convert to Anthropic deltas
    for line in sse_data.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with("data: ") { continue; }
        let json_str = &trimmed[6..];
        if json_str == "[DONE]" { continue; }

        if let Ok(chunk) = serde_json::from_str::<serde_json::Value>(json_str) {
            if let Some(delta_content) = chunk.get("choices")
                .and_then(|c| c.as_array())
                .and_then(|arr| arr.first())
                .and_then(|c| c.get("delta"))
                .and_then(|d| d.get("content"))
                .and_then(|c| c.as_str())
            {
                if !delta_content.is_empty() {
                    let delta = serde_json::json!({
                        "type": "content_block_delta",
                        "index": 0,
                        "delta": {"type": "text_delta", "text": delta_content}
                    });
                    output.push_str(&format!("event: content_block_delta\ndata: {}\n\n", delta));
                }
            }
        }
    }

    // content_block_stop
    output.push_str(&format!("event: content_block_stop\ndata: {}\n\n",
        serde_json::json!({"type": "content_block_stop", "index": 0})));

    // message_delta
    output.push_str(&format!("event: message_delta\ndata: {}\n\n",
        serde_json::json!({
            "type": "message_delta",
            "delta": {"stop_reason": "end_turn", "stop_sequence": null},
            "usage": {"output_tokens": 0}
        })));

    // message_stop
    output.push_str(&format!("event: message_stop\ndata: {}\n\n",
        serde_json::json!({"type": "message_stop"})));

    output
}
