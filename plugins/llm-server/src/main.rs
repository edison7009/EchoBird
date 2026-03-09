// Echobird Remote LLM Server — standalone HTTP management API
// Copied from src-tauri/src/services/local_llm.rs, adapted for standalone operation
//
// Provides HTTP API for remote LLM management:
//   GET  /api/status           — Server status
//   GET  /api/gpu              — GPU detection
//   GET  /api/models?dir=...   — Scan GGUF files
//   GET  /api/dirs             — Get model directories
//   GET  /api/logs             — Server logs
//   GET  /api/engine/status    — Engine (llama-server) installed?
//   POST /api/start            — Start llama-server
//   POST /api/stop             — Stop llama-server
//   POST /api/download         — Download model from HuggingFace (mirror speed test)
//   GET  /api/download/status  — Download progress
//   POST /api/download/cancel  — Cancel download
//
// Also runs Unified Proxy for LLM inference:
//   /v1/*        → OpenAI passthrough
//   /anthropic/* → Anthropic→OpenAI format conversion

use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::Mutex;
use tokio::sync::watch;
use futures_util::StreamExt;

// ============================================================
// Data Structures (from local_llm.rs lines 16-63)
// ============================================================

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ServerInfo {
    running: bool,
    port: u16,
    model_name: String,
    pid: Option<u32>,
    api_key: String,
    runtime: String,
}

impl Default for ServerInfo {
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

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct GgufFile {
    file_name: String,
    file_path: String,
    file_size: u64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct GpuInfo {
    gpu_name: String,
    gpu_vram_gb: f64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct ModelSettings {
    #[serde(default)]
    models_dirs: Vec<String>,
    #[serde(default)]
    download_dir: Option<String>,
    #[serde(default)]
    gpu_name: Option<String>,
    #[serde(default)]
    gpu_vram_gb: Option<f64>,
}

// ============================================================
// LLM Server Manager (from local_llm.rs lines 65-309)
// ============================================================

struct LlmServer {
    info: ServerInfo,
    logs: Vec<String>,
    child: Option<std::process::Child>,
    child_pid: Option<u32>,
    proxy_shutdown: Option<watch::Sender<bool>>,
}

const MAX_LOGS: usize = 1000;

impl LlmServer {
    fn new() -> Self {
        Self {
            info: ServerInfo::default(),
            logs: Vec::new(),
            child: None,
            child_pid: None,
            proxy_shutdown: None,
        }
    }

    /// Find llama-server executable
    fn find_llama_server() -> Option<PathBuf> {
        let exe_name = if cfg!(windows) { "llama-server.exe" } else { "llama-server" };

        // 1. Next to current exe
        if let Ok(exe_path) = std::env::current_exe() {
            if let Some(dir) = exe_path.parent() {
                let candidate = dir.join(exe_name);
                if candidate.exists() {
                    return Some(candidate);
                }
            }
        }

        // 2. ~/.echobird/llama-server/bin/
        if let Some(home) = dirs::home_dir() {
            let llama_bin_dir = home.join(".echobird").join("llama-server").join("bin");
            if llama_bin_dir.exists() {
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
            let echobird_bin = home.join(".echobird").join("bin").join(exe_name);
            if echobird_bin.exists() {
                return Some(echobird_bin);
            }
        }

        // 4. System PATH
        if let Ok(output) = Command::new("which").arg(exe_name).output() {
            if output.status.success() {
                let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !path.is_empty() {
                    return Some(PathBuf::from(path));
                }
            }
        }

        None
    }

    /// Start LLM runtime with model
    async fn start(
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

        let model_name = std::path::Path::new(model_path)
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "Unknown Model".to_string());

        let api_key = generate_session_api_key();

        let (child, needs_proxy) = match runtime {
            "vllm" => {
                // vLLM: python3 -m vllm.entrypoints.openai.api_server
                // Natively exposes /v1/chat/completions, no proxy needed
                self.add_log(&format!("Starting vLLM on port {} with model: {}", port, model_name));
                let mut args = vec![
                    "-m".to_string(), "vllm.entrypoints.openai.api_server".to_string(),
                    "--model".to_string(), model_path.to_string(),
                    "--port".to_string(), port.to_string(),
                    "--host".to_string(), "0.0.0.0".to_string(),
                    "--api-key".to_string(), api_key.clone(),
                ];
                if let Some(ctx) = context_size {
                    args.push("--max-model-len".to_string());
                    args.push(ctx.to_string());
                }
                let c = Command::new("python3")
                    .args(&args)
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn()
                    .map_err(|e| format!("Failed to spawn vLLM: {}", e))?;
                (c, false)
            }
            "sglang" => {
                // SGLang: python3 -m sglang.launch_server
                // Natively exposes /v1/chat/completions, no proxy needed
                self.add_log(&format!("Starting SGLang on port {} with model: {}", port, model_name));
                let mut args = vec![
                    "-m".to_string(), "sglang.launch_server".to_string(),
                    "--model-path".to_string(), model_path.to_string(),
                    "--port".to_string(), port.to_string(),
                    "--host".to_string(), "0.0.0.0".to_string(),
                    "--api-key".to_string(), api_key.clone(),
                ];
                if let Some(ctx) = context_size {
                    args.push("--context-length".to_string());
                    args.push(ctx.to_string());
                }
                let c = Command::new("python3")
                    .args(&args)
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
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
                eprintln!("[LLM] Starting: {:?} (api_key={}...)", exe, &api_key[..12]);
                // Use Stdio::null() so the child process runs as a true background daemon.
                // Stdio::piped() would cause the child to exit when the pipe is dropped.
                let c = Command::new(&exe)
                    .args(&args)
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn()
                    .map_err(|e| format!("Failed to spawn llama-server: {}", e))?;
                (c, true)
            }
        };

        let pid = child.id();
        self.child_pid = Some(pid);
        // Store the child handle to keep the process alive (dropping Child does not kill it
        // on Unix, but closing piped stdio can cause the subprocess to exit on SIGPIPE).
        self.child = Some(child);
        self.info = ServerInfo {
            running: true,
            port,
            model_name,
            pid: Some(pid),
            api_key,
            runtime: runtime.to_string(),
        };

        self.add_log(&format!("{} started (PID: {})", runtime, pid));

        // Only llama-server needs Unified Proxy (vLLM/SGLang expose OpenAI API natively)
        if needs_proxy {
            let (shutdown_tx, shutdown_rx) = watch::channel(false);
            self.proxy_shutdown = Some(shutdown_tx);
            let proxy_port = port;
            let target_port = port + 100;
            tokio::spawn(async move {
                if let Err(e) = run_unified_proxy(proxy_port, target_port, shutdown_rx).await {
                    eprintln!("[LLM] Proxy error: {}", e);
                }
            });
            self.add_log("Unified Proxy started:");
            self.add_log(&format!("  OpenAI:    http://0.0.0.0:{}/v1", port));
            self.add_log(&format!("  Anthropic: http://0.0.0.0:{}/anthropic", port));
        } else {
            self.add_log(&format!("OpenAI API: http://0.0.0.0:{}/v1", port));
            self.add_log("(native OpenAI endpoint, no proxy needed)");
        }

        Ok(())
    }

    /// Stop the server
    async fn stop(&mut self) -> Result<(), String> {
        if !self.info.running {
            return Err("Server not running".to_string());
        }

        // Prefer killing via the Child handle for clean process management.
        if let Some(mut child) = self.child.take() {
            eprintln!("[LLM] Stopping server via child handle");
            let _ = child.kill();
            let _ = child.wait();
        } else if let Some(pid) = self.child_pid {
            // Fallback: kill by PID (legacy path, should not normally be reached)
            eprintln!("[LLM] Stopping server (PID: {})", pid);

            #[cfg(unix)]
            unsafe {
                libc::kill(pid as i32, libc::SIGTERM);
            }

            #[cfg(not(unix))]
            {
                let _ = Command::new("kill").arg(pid.to_string()).output();
            }

            tokio::time::sleep(std::time::Duration::from_secs(2)).await;

            #[cfg(unix)]
            unsafe {
                libc::kill(pid as i32, libc::SIGKILL);
            }
        }

        if let Some(tx) = self.proxy_shutdown.take() {
            let _ = tx.send(true);
        }

        self.add_log("Server stopped");
        self.info = ServerInfo::default();
        self.child_pid = None;

        Ok(())
    }

    fn get_info(&self) -> ServerInfo {
        self.info.clone()
    }

    fn get_logs(&self) -> Vec<String> {
        self.logs.clone()
    }

    fn add_log(&mut self, msg: &str) {
        let now = chrono::Local::now().format("%H:%M:%S").to_string();
        self.logs.push(format!("[{}] {}", now, msg));
        if self.logs.len() > MAX_LOGS {
            self.logs.drain(0..self.logs.len() - MAX_LOGS);
        }
    }
}

// ============================================================
// API Key Generation (from local_llm.rs lines 289-309)
// ============================================================

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

// ============================================================
// Model Settings (from local_llm.rs lines 311-374)
// ============================================================

fn settings_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join(".echobird")
        .join("config")
        .join("local-model-settings.json")
}

fn load_model_settings() -> ModelSettings {
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

fn save_model_settings(settings: &ModelSettings) {
    let path = settings_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let content = serde_json::to_string_pretty(settings).unwrap_or_default();
    let _ = std::fs::write(path, content);
}

fn get_models_dirs() -> Vec<String> {
    let settings = load_model_settings();
    if !settings.models_dirs.is_empty() {
        return settings.models_dirs;
    }
    let default_dir = dirs::home_dir()
        .unwrap_or_default()
        .join("Models")
        .to_string_lossy()
        .to_string();
    vec![default_dir]
}

// ============================================================
// GPU Detection (from local_llm.rs lines 376-515)
// ============================================================

fn detect_gpu() -> Option<GpuInfo> {
    let info = detect_gpu_nvidia_smi()
        .or_else(detect_gpu_rocm)        // AMD ROCm / Hygon DCU
        .or_else(detect_gpu_intel_xpu)   // Intel Arc / Flex / Gaudi
        .or_else(detect_gpu_mthreads)    // Moore Threads
        .or_else(detect_gpu_iluvatar)    // Iluvatar CoreX
        .or_else(detect_gpu_cambricon)   // Cambricon MLU
        .or_else(detect_gpu_biren)       // Biren BR
        .or_else(detect_gpu_kunlunxin);  // KunlunXin XPU
    if let Some(ref gpu) = info {
        let mut settings = load_model_settings();
        settings.gpu_name = Some(gpu.gpu_name.clone());
        settings.gpu_vram_gb = Some(gpu.gpu_vram_gb);
        save_model_settings(&settings);
    }
    info
}

fn get_gpu_info() -> Option<GpuInfo> {
    let settings = load_model_settings();
    match (settings.gpu_name, settings.gpu_vram_gb) {
        (Some(name), Some(vram)) if !name.is_empty() => Some(GpuInfo {
            gpu_name: name,
            gpu_vram_gb: vram,
        }),
        _ => None,
    }
}

fn detect_gpu_nvidia_smi() -> Option<GpuInfo> {
    let output = Command::new("nvidia-smi")
        .args(["--query-gpu=name,memory.total", "--format=csv,noheader,nounits"])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let first_line = stdout.lines().next()?.trim().to_string();
    let parts: Vec<&str> = first_line.split(',').map(|s| s.trim()).collect();
    if parts.len() >= 2 {
        let vram_mb: f64 = parts[1].parse().unwrap_or(0.0);
        if vram_mb > 0.0 {
            let vram_gb = (vram_mb / 1024.0 * 10.0).round() / 10.0;
            let short_name = shorten_gpu_name(parts[0]);
            return Some(GpuInfo {
                gpu_name: short_name,
                gpu_vram_gb: vram_gb,
            });
        }
    }
    None
}

/// Moore Threads GPU detection via mthreads-gmi
fn detect_gpu_mthreads() -> Option<GpuInfo> {
    // mthreads-gmi default table output (no special flags needed):
    //   ID | Name      | PCIe | %GPU Mem              | ...
    //   0  | MTT S4000 | ...  | 0%  4MiB(49152MiB)    | ...
    // Total VRAM is the number inside the trailing parentheses: (49152MiB)
    let output = Command::new("mthreads-gmi")
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Find the first data row (skip header lines that contain "ID" or "---")
    let mut gpu_name = String::from("Moore Threads MTT");
    let mut vram_gb: Option<f64> = None;

    for line in stdout.lines() {
        let trimmed = line.trim();
        // Skip header / separator lines
        if trimmed.is_empty()
            || trimmed.starts_with("ID")
            || trimmed.starts_with('+')
            || trimmed.starts_with('-')
            || trimmed.starts_with('=')
        {
            continue;
        }

        // The Name column is typically the second pipe-separated field:
        //   | 0  | MTT S4000 | ... | 0% 4MiB(49152MiB) | ...
        let cols: Vec<&str> = trimmed.split('|').map(|s| s.trim()).collect();
        if cols.len() >= 3 {
            // col[1] = ID, col[2] = Name
            let name_col = cols[1];
            // col[4] usually contains "%GPU Mem" like "0% 4MiB(49152MiB)"
            // Find the column that contains "MiB(" pattern (total VRAM in parens)
            let mem_col = cols.iter().find(|c| c.contains("MiB(") || c.contains("GiB("));

            if let Some(mem_str) = mem_col {
                // Extract the number inside the last set of parentheses: (49152MiB)
                if let Some(start) = mem_str.rfind('(') {
                    let inside = &mem_str[start + 1..];
                    let end = inside.find(')').unwrap_or(inside.len());
                    let token = inside[..end].trim();
                    // token is like "49152MiB" or "48GiB"
                    let (num_str, unit) = if token.ends_with("GiB") {
                        (&token[..token.len() - 3], "GiB")
                    } else if token.ends_with("MiB") {
                        (&token[..token.len() - 3], "MiB")
                    } else {
                        (token, "MiB") // assume MiB
                    };
                    if let Ok(v) = num_str.trim().parse::<f64>() {
                        if v > 0.0 {
                            vram_gb = Some(if unit == "GiB" {
                                (v * 10.0).round() / 10.0
                            } else {
                                (v / 1024.0 * 10.0).round() / 10.0
                            });
                        }
                    }
                }
            }

            // Extract GPU name from Name column (second data column)
            let candidate = name_col.trim();
            if !candidate.is_empty() && candidate != "Name" && candidate.to_uppercase().contains("MTT") {
                gpu_name = shorten_gpu_name(candidate);
            }

            if vram_gb.is_some() {
                break; // First GPU is enough
            }
        }
    }

    // Fallback: try mthreads-gmi -L for a simple listing like "GPU 0: MTT S4000 (UUID: ...)"
    if vram_gb.is_none() {
        let list_out = Command::new("mthreads-gmi").args(["-L"]).output().ok()?;
        if list_out.status.success() {
            let list_str = String::from_utf8_lossy(&list_out.stdout);
            // Just confirms the tool is present; can't get VRAM from -L alone
            for line in list_str.lines() {
                if line.to_uppercase().contains("MTT") {
                    if let Some(colon_pos) = line.find(':') {
                        let after = line[colon_pos + 1..].trim();
                        // Strip UUID part "(UUID: ...)"
                        let name = if let Some(paren) = after.find('(') {
                            after[..paren].trim()
                        } else {
                            after
                        };
                        if !name.is_empty() {
                            gpu_name = shorten_gpu_name(name);
                        }
                    }
                    // Can't get VRAM reliably from -L; return None so next method is tried
                    break;
                }
            }
        }
        return None; // No VRAM info found
    }

    Some(GpuInfo { gpu_name, gpu_vram_gb: vram_gb? })
}

fn shorten_gpu_name(name: &str) -> String {
    name.replace("NVIDIA GeForce ", "")
        .replace("NVIDIA RTX ", "RTX ")
        .replace("NVIDIA Tesla ", "Tesla ")
        .replace("NVIDIA ", "")
        .replace("AMD Radeon RX ", "RX ")
        .replace("AMD Radeon PRO ", "Radeon PRO ")
        .replace("AMD Radeon ", "")
        .replace("Intel(R) Arc ", "Arc ")
        .replace("Intel(R) Data Center GPU ", "Intel DC-GPU ")
        .replace("Intel(R) ", "Intel ")
        .replace("Apple ", "")
        .replace("Moore Threads ", "")
        .replace("Iluvatar CoreX ", "")
        .replace("Cambricon ", "")
        .replace("Biren ", "")
        .replace("KunlunXin ", "")
        .replace("(TM)", "")
        .replace("(R)", "")
        .replace("  ", " ")
        .trim()
        .to_string()
}

// AMD ROCm / Hygon DCU
fn detect_gpu_rocm() -> Option<GpuInfo> {
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
                return Some(GpuInfo { gpu_name: shorten_gpu_name(p[1]), gpu_vram_gb: gb });
            }
        }
    }
    None
}

// Intel Arc / Flex / Gaudi
fn detect_gpu_intel_xpu() -> Option<GpuInfo> {
    let out = Command::new("xpu-smi").args(["discovery", "-j"]).output().ok()?;
    if !out.status.success() { return None; }
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).ok()?;
    let dev = json.get("device_list")?.as_array()?.first()?;
    let name = dev.get("device_name").and_then(|v| v.as_str()).unwrap_or("Intel GPU");
    let mb = dev.get("memory_physical_size").and_then(|v| v.as_f64()).unwrap_or(0.0);
    if mb > 0.0 {
        let gb = (mb / 1024.0 * 10.0).round() / 10.0;
        return Some(GpuInfo { gpu_name: shorten_gpu_name(name), gpu_vram_gb: gb });
    }
    None
}

// Iluvatar CoreX
fn detect_gpu_iluvatar() -> Option<GpuInfo> {
    let out = Command::new("ixsmi").args(["-q", "--display=MEMORY"]).output().ok()?;
    if !out.status.success() { return None; }
    let name_o = Command::new("ixsmi").args(["-q", "--display=NAME"]).output().ok()?;
    let gb = parse_vram_mb_line(&String::from_utf8_lossy(&out.stdout))?;
    let name = parse_name_colon(&String::from_utf8_lossy(&name_o.stdout))
        .unwrap_or_else(|| "Iluvatar CoreX".to_string());
    Some(GpuInfo { gpu_name: shorten_gpu_name(&name), gpu_vram_gb: gb })
}

// Cambricon MLU
fn detect_gpu_cambricon() -> Option<GpuInfo> {
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
        return Some(GpuInfo { gpu_name: shorten_gpu_name(name), gpu_vram_gb: gb });
    }
    None
}

// Biren BR
fn detect_gpu_biren() -> Option<GpuInfo> {
    let out = Command::new("brsmi").args(["-q", "--display=MEMORY"]).output().ok()?;
    if !out.status.success() { return None; }
    let name_o = Command::new("brsmi").args(["-q", "--display=NAME"]).output().ok()?;
    let gb = parse_vram_mb_line(&String::from_utf8_lossy(&out.stdout))?;
    let name = parse_name_colon(&String::from_utf8_lossy(&name_o.stdout))
        .unwrap_or_else(|| "Biren BR".to_string());
    Some(GpuInfo { gpu_name: shorten_gpu_name(&name), gpu_vram_gb: gb })
}

// KunlunXin XPU
fn detect_gpu_kunlunxin() -> Option<GpuInfo> {
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
            return Some(GpuInfo { gpu_name: shorten_gpu_name(p[0]), gpu_vram_gb: gb });
        }
    }
    None
}

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


// ============================================================
// Model Scanning (from local_llm.rs lines 517-552)
// ============================================================

fn scan_gguf_files(dir: &str, max_depth: u32) -> Vec<GgufFile> {
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

// ============================================================
// HuggingFace Model Scanning (for vLLM / SGLang)
// ============================================================

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct HfModelEntry {
    model_name: String,
    model_path: String,
    total_size: u64,
}

fn scan_hf_models(dir: &str, max_depth: u32) -> Vec<HfModelEntry> {
    let mut results = Vec::new();
    scan_hf_recursive(std::path::Path::new(dir), max_depth, &mut results);
    results
}

fn scan_hf_recursive(dir: &std::path::Path, depth: u32, results: &mut Vec<HfModelEntry>) {
    if depth == 0 || !dir.is_dir() {
        return;
    }

    let config_path = dir.join("config.json");
    if config_path.exists() {
        let model_name = std::fs::read_to_string(&config_path)
            .ok()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .and_then(|v| {
                v.get("_name_or_path")
                    .and_then(|n| n.as_str().map(String::from))
                    .or_else(|| v.get("model_type").and_then(|n| n.as_str().map(String::from)))
            })
            .unwrap_or_else(|| {
                dir.file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default()
            });

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
        return;
    }

    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            if entry.path().is_dir() {
                scan_hf_recursive(&entry.path(), depth - 1, results);
            }
        }
    }
}


/// Check if a Python package is installed, return version if found
fn check_python_package(package: &str) -> Option<String> {
    let output = Command::new("python3")
        .args(["-c", &format!("import {}; print({}.__version__)", package, package)])
        .output();
    match output {
        Ok(o) if o.status.success() => {
            Some(String::from_utf8_lossy(&o.stdout).trim().to_string())
        }
        _ => None,
    }
}

fn get_engine_status() -> serde_json::Value {
    // llama-server: binary search
    let llama = LlmServer::find_llama_server();

    // vLLM: Python package
    let vllm_ver = check_python_package("vllm");

    // SGLang: Python package
    let sglang_ver = check_python_package("sglang");

    // System info for UI (architecture + GPU)
    let arch = detect_runtime_arch();
    let gpu = get_gpu_info().or_else(detect_gpu);
    let has_nvidia = gpu.as_ref().map(|g: &GpuInfo| {
        let name = g.gpu_name.to_lowercase();
        name.contains("nvidia") || name.contains("rtx") || name.contains("gtx") || name.contains("quadro")
    }).unwrap_or(false);
    let has_amd = gpu.as_ref().map(|g: &GpuInfo| {
        g.gpu_name.to_lowercase().contains("amd") || g.gpu_name.to_lowercase().contains("radeon")
    }).unwrap_or(false);

    serde_json::json!({
        "systemInfo": {
            "os": std::env::consts::OS,
            "arch": arch,
            "hasNvidiaGpu": has_nvidia,
            "hasAmdGpu": has_amd,
            "gpuName": gpu.as_ref().map(|g| &g.gpu_name),
            "gpuVramGb": gpu.map(|g| g.gpu_vram_gb),
        },
        "llama-server": {
            "installed": llama.is_some(),
            "path": llama.map(|p| p.to_string_lossy().to_string()),
            "installCmd": {
                "linux": "Download from https://github.com/ggml-org/llama.cpp/releases",
                "windows": "Download from https://github.com/ggml-org/llama.cpp/releases",
            },
        },
        "vllm": {
            "installed": vllm_ver.is_some(),
            "version": vllm_ver,
            "linuxOnly": true,
            "requiresGpu": true,
            "installCmd": "pip3 install vllm",
        },
        "sglang": {
            "installed": sglang_ver.is_some(),
            "version": sglang_ver,
            "linuxOnly": true,
            "requiresGpu": true,
            "installCmd": "pip3 install sglang[all]",
        },
    })
}

// ============================================================
// Model Download (ported from local_llm.rs)
// ============================================================

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct DownloadProgress {
    file_name: String,
    progress: u32,
    downloaded: u64,
    total: u64,
    status: String, // "idle" | "speed_test" | "downloading" | "completed" | "error" | "cancelled"
}

impl Default for DownloadProgress {
    fn default() -> Self {
        Self {
            file_name: String::new(),
            progress: 0,
            downloaded: 0,
            total: 0,
            status: "idle".to_string(),
        }
    }
}

type SharedDownload = Arc<Mutex<DownloadProgress>>;

/// Global download abort flag
static DOWNLOAD_ABORT: AtomicBool = AtomicBool::new(false);

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
    eprintln!(
        "[Download] Speed test {}: {:.0} KB/s ({:.0} KB in {:.1}s)",
        source.name,
        speed / 1024.0,
        bytes as f64 / 1024.0,
        elapsed
    );
    (source.name.clone(), speed)
}

/// Download model file with speed test + resume (runs in background tokio task)
async fn download_model_task(
    state: SharedDownload,
    repo: String,
    file_name: String,
) {
    // Download directory = first models dir
    let download_dir = get_models_dirs().first().cloned().unwrap_or_else(|| {
        dirs::home_dir()
            .unwrap_or_default()
            .join("Models")
            .to_string_lossy()
            .to_string()
    });
    let save_path = PathBuf::from(&download_dir).join(&file_name);
    let temp_path = PathBuf::from(&download_dir).join(format!("{}.downloading", file_name));

    // Reset abort flag
    DOWNLOAD_ABORT.store(false, Ordering::SeqCst);

    // Ensure download dir exists
    let _ = std::fs::create_dir_all(&download_dir);

    let sources = build_download_sources(&repo, &file_name);

    // Update state: speed_test
    {
        let mut dl = state.lock().await;
        dl.file_name = file_name.clone();
        dl.status = "speed_test".to_string();
        dl.progress = 0;
        dl.downloaded = 0;
        dl.total = 0;
    }

    // Speed test all sources in parallel
    eprintln!("[Download] Speed testing {} sources for {}...", sources.len(), file_name);
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
        let mut dl = state.lock().await;
        dl.status = "error".to_string();
        eprintln!("[Download] All sources unreachable for {}", file_name);
        return;
    }

    eprintln!("[Download] Fastest source: {} ({:.0} KB/s)", sorted[0].0, sorted[0].1 / 1024.0);

    // Try each source in speed order
    for (source_name, _) in &sorted {
        if DOWNLOAD_ABORT.load(Ordering::SeqCst) {
            let mut dl = state.lock().await;
            dl.status = "cancelled".to_string();
            return;
        }

        let source = sources.iter().find(|s| &s.name == source_name).unwrap();
        match try_download_source(&state, source, &save_path, &temp_path, &file_name).await {
            Ok(_) => {
                return; // Success
            }
            Err(e) => {
                eprintln!("[Download] {} failed: {}", source_name, e);
                if DOWNLOAD_ABORT.load(Ordering::SeqCst) {
                    let mut dl = state.lock().await;
                    dl.status = "cancelled".to_string();
                    return;
                }
            }
        }
    }

    // All sources failed
    let mut dl = state.lock().await;
    dl.status = "error".to_string();
    eprintln!("[Download] All sources failed for {}", file_name);
}

/// Try downloading from a single source with resume support
async fn try_download_source(
    state: &SharedDownload,
    source: &DownloadSource,
    save_path: &PathBuf,
    temp_path: &PathBuf,
    file_name: &str,
) -> Result<(), String> {
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
        eprintln!("[Download] [{}] Resume mode, {} bytes already downloaded", source.name, start_byte);
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
        0u64
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
    let mut last_update = std::time::Instant::now();

    // Update state: downloading
    {
        let mut dl = state.lock().await;
        dl.status = "downloading".to_string();
        dl.downloaded = downloaded;
        dl.total = total_size;
        dl.progress = if total_size > 0 { ((downloaded as f64 / total_size as f64) * 100.0) as u32 } else { 0 };
    }

    while let Some(chunk) = stream.next().await {
        // Check abort
        if DOWNLOAD_ABORT.load(Ordering::SeqCst) {
            return Err("Download cancelled".to_string());
        }

        let data = chunk.map_err(|e| format!("[{}] Stream error: {}", source.name, e))?;
        file.write_all(&data).map_err(|e| format!("Write error: {}", e))?;
        downloaded += data.len() as u64;

        // Update state (~4 times per second)
        if last_update.elapsed() >= std::time::Duration::from_millis(250) {
            let progress = if total_size > 0 {
                ((downloaded as f64 / total_size as f64) * 100.0) as u32
            } else {
                0
            };
            let mut dl = state.lock().await;
            dl.progress = progress;
            dl.downloaded = downloaded;
            dl.total = total_size;
            last_update = std::time::Instant::now();
        }
    }

    // Rename temp file to final
    if save_path.exists() {
        let _ = std::fs::remove_file(save_path);
    }
    std::fs::rename(temp_path, save_path)
        .map_err(|e| format!("Rename error: {}", e))?;

    // Update state: completed
    {
        let mut dl = state.lock().await;
        dl.status = "completed".to_string();
        dl.progress = 100;
        dl.downloaded = total_size;
        dl.total = total_size;
    }

    eprintln!("[Download] [{}] Complete: {}", source.name, file_name);
    Ok(())
}

// ============================================================
// Engine Download: llama-server binary installer
// (ported from local_llm.rs)
// ============================================================

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

/// Detect runtime CPU architecture via `uname -m`.
/// This is more reliable than std::env::consts::ARCH which reflects the
/// compile-time target, not the actual host — important when the plugin binary
/// is compiled for x86-64 but deployed on an ARM64 server.
fn detect_runtime_arch() -> String {
    if let Ok(out) = Command::new("uname").arg("-m").output() {
        let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !s.is_empty() {
            return s;
        }
    }
    // Fallback: compile-time arch
    std::env::consts::ARCH.to_string()
}

/// Get platform-specific download file names (detects remote server's own OS)
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
            // Use runtime uname -m so ARM64 servers get the correct binary
            // even if the plugin binary itself was compiled for x86-64
            let arch = detect_runtime_arch();
            if arch == "aarch64" || arch == "arm64" || arch.starts_with("arm") {
                vec![format!("llama-{}-bin-ubuntu-arm64.tar.gz", LLAMA_VERSION)]
            } else {
                vec![format!("llama-{}-bin-ubuntu-x64.tar.gz", LLAMA_VERSION)]
            }
        }
    }
}

/// Get llama-server install directory
fn llama_install_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join(".echobird")
        .join("llama-server")
}

/// Test engine mirror speed (5 seconds)
async fn test_engine_mirror_speed(url: String, name: String) -> (String, String, f64) {
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
    eprintln!("[Engine] Speed test {}: {:.0} KB/s ({} KB in {:.1}s)",
        name, speed / 1024.0, bytes / 1024, elapsed);
    (name, url, speed)
}

/// Download a single engine file with progress
async fn download_engine_file(
    state: &SharedDownload,
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
    let mut last_update = std::time::Instant::now();

    while let Some(chunk) = stream.next().await {
        if DOWNLOAD_ABORT.load(Ordering::SeqCst) {
            return Err("Download cancelled".to_string());
        }

        let data = chunk.map_err(|e| format!("Stream error: {}", e))?;
        file.write_all(&data).map_err(|e| format!("Write error: {}", e))?;
        downloaded += data.len() as u64;

        if last_update.elapsed() >= std::time::Duration::from_millis(250) {
            let file_progress = if content_length > 0 {
                (downloaded as f64 / content_length as f64) * 100.0
            } else { 0.0 };
            let overall = ((completed_files as f64 + file_progress / 100.0) / total_files as f64 * 100.0) as u32;

            let mut dl = state.lock().await;
            dl.progress = overall;
            dl.downloaded = downloaded;
            dl.total = content_length;
            last_update = std::time::Instant::now();
        }
    }

    Ok(())
}

/// Install vLLM or SGLang via pip (background task)
async fn install_python_runtime_task(state: SharedDownload, runtime: String) {
    let pkg = if runtime == "vllm" { "vllm" } else { "sglang[all]" };
    let display = if runtime == "vllm" { "vllm" } else { "sglang" };

    {
        let mut dl = state.lock().await;
        dl.file_name = display.to_string();
        dl.status = "installing".to_string();
        dl.progress = 0;
        dl.downloaded = 0;
        dl.total = 0;
    }

    eprintln!("[PipInstall] Starting: pip3 install --upgrade {}", pkg);

    // Try 1: official PyPI (Fastly CDN, works globally)
    let result = tokio::task::spawn_blocking({
        let pkg = pkg.to_string();
        move || {
            Command::new("pip3")
                .args(["install", "--upgrade", &pkg])
                .output()
        }
    }).await;

    let success = match result {
        Ok(Ok(output)) => {
            eprintln!("[PipInstall] stdout: {}", String::from_utf8_lossy(&output.stdout));
            eprintln!("[PipInstall] stderr: {}", String::from_utf8_lossy(&output.stderr));
            output.status.success()
        }
        _ => false,
    };

    if success {
        let mut dl = state.lock().await;
        dl.status = "completed".to_string();
        dl.progress = 100;
        eprintln!("[PipInstall] {} installed successfully", display);
        return;
    }

    eprintln!("[PipInstall] PyPI failed, retrying via Tsinghua mirror...");

    // Try 2: Tsinghua mirror (confirmed 200 OK, reliable fallback)
    {
        let mut dl = state.lock().await;
        dl.status = "installing".to_string();
        dl.progress = 5;
    }

    let result2 = tokio::task::spawn_blocking({
        let pkg = pkg.to_string();
        move || {
            Command::new("pip3")
                .args([
                    "install", "--upgrade", &pkg,
                    "-i", "https://pypi.tuna.tsinghua.edu.cn/simple",
                    "--trusted-host", "pypi.tuna.tsinghua.edu.cn",
                ])
                .output()
        }
    }).await;

    let mut dl = state.lock().await;
    match result2 {
        Ok(Ok(output)) => {
            eprintln!("[PipInstall] mirror stdout: {}", String::from_utf8_lossy(&output.stdout));
            eprintln!("[PipInstall] mirror stderr: {}", String::from_utf8_lossy(&output.stderr));
            if output.status.success() {
                dl.status = "completed".to_string();
                dl.progress = 100;
                eprintln!("[PipInstall] {} installed via Tsinghua mirror", display);
            } else {
                dl.status = "error".to_string();
                eprintln!("[PipInstall] All sources failed for {}", display);
            }
        }
        _ => {
            dl.status = "error".to_string();
            eprintln!("[PipInstall] Failed to spawn pip3 for {}", display);
        }
    }
}


/// Install engine (background task) — runtime-aware

async fn install_engine_task(state: SharedDownload, runtime: String) {
    // ── vLLM / SGLang: pip install ──
    if runtime == "vllm" || runtime == "sglang" {
        install_python_runtime_task(state, runtime).await;
        return;
    }

    // ── llama-server: binary download ──
    let file_names = get_llama_platform_files();
    let bin_dir = llama_install_dir().join("bin");
    let temp_dir = llama_install_dir().join("temp");
    let mirrors = llama_download_mirrors();

    // Reset abort flag
    DOWNLOAD_ABORT.store(false, Ordering::SeqCst);

    let _ = std::fs::create_dir_all(&bin_dir);
    let _ = std::fs::create_dir_all(&temp_dir);

    // Update state: speed_test
    {
        let mut dl = state.lock().await;
        dl.file_name = "llama-server".to_string();
        dl.status = "speed_test".to_string();
        dl.progress = 0;
        dl.downloaded = 0;
        dl.total = 0;
    }

    let total_files = file_names.len();
    let mut completed_files = 0u32;

    for file_name in &file_names {
        if DOWNLOAD_ABORT.load(Ordering::SeqCst) {
            let _ = std::fs::remove_dir_all(&temp_dir);
            let _ = std::fs::remove_dir_all(&bin_dir);
            let mut dl = state.lock().await;
            dl.status = "cancelled".to_string();
            return;
        }

        let temp_file = temp_dir.join(file_name);

        // Speed test all mirrors in parallel
        eprintln!("[Engine] Speed testing {} mirrors for {}...", mirrors.len(), file_name);
        let mut speed_futures = Vec::new();
        for (i, mirror) in mirrors.iter().enumerate() {
            let url = format!("{}/{}", mirror, file_name);
            let name = if i == 0 {
                "GitHub".to_string()
            } else {
                // Extract host from mirror URL (e.g., "https://ghfast.top/..." -> "ghfast.top")
                let host = mirror.trim_start_matches("https://")
                    .trim_start_matches("http://")
                    .split('/')
                    .next()
                    .unwrap_or("unknown");
                host.to_string()
            };
            speed_futures.push(test_engine_mirror_speed(url, name));
        }
        let speed_results = futures_util::future::join_all(speed_futures).await;

        // Sort by speed descending
        let mut sorted: Vec<_> = speed_results
            .into_iter()
            .filter(|(_, _, speed)| *speed > 0.0)
            .collect();
        sorted.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));

        if sorted.is_empty() {
            let mut dl = state.lock().await;
            dl.status = "error".to_string();
            let _ = std::fs::remove_dir_all(&temp_dir);
            eprintln!("[Engine] All mirrors unreachable for {}", file_name);
            return;
        }

        eprintln!("[Engine] Fastest: {} ({:.0} KB/s)", sorted[0].0, sorted[0].2 / 1024.0);

        // Update state: downloading
        {
            let mut dl = state.lock().await;
            dl.status = "downloading".to_string();
        }

        // Try each mirror in speed order
        let mut download_ok = false;
        for (mirror_name, mirror_url, _) in &sorted {
            if DOWNLOAD_ABORT.load(Ordering::SeqCst) {
                break;
            }
            eprintln!("[Engine] Downloading via {}: {}", mirror_name, mirror_url);
            match download_engine_file(&state, mirror_url, &temp_file, completed_files, total_files as u32).await {
                Ok(_) => {
                    download_ok = true;
                    break;
                }
                Err(e) => {
                    eprintln!("[Engine] {} failed: {}", mirror_name, e);
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
            let mut dl = state.lock().await;
            dl.status = "cancelled".to_string();
            return;
        }

        if !download_ok {
            let _ = std::fs::remove_dir_all(&temp_dir);
            let mut dl = state.lock().await;
            dl.status = "error".to_string();
            eprintln!("[Engine] All mirrors failed for {}", file_name);
            return;
        }

        // Extract
        eprintln!("[Engine] Extracting: {}", file_name);
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
                    .status();
                if status.map(|s: std::process::ExitStatus| !s.success()).unwrap_or(true) {
                    eprintln!("[Engine] Expand-Archive failed for {}", file_name);
                }
            }
            #[cfg(not(windows))]
            {
                // Fallback: unzip command
                let _ = Command::new("unzip")
                    .args(["-o", &temp_file.to_string_lossy(), "-d", &extract_dir.to_string_lossy()])
                    .status();
            }
        } else {
            // macOS/Linux: tar -xzf
            let _ = Command::new("tar")
                .args(["-xzf", &temp_file.to_string_lossy(), "-C", &extract_dir.to_string_lossy()])
                .status();
        }

        // Clean temp file
        let _ = std::fs::remove_file(&temp_file);
        completed_files += 1;
    }

    // Unix: set executable permission
    #[cfg(not(windows))]
    {
        if let Some(exe_path) = LlmServer::find_llama_server() {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&exe_path, std::fs::Permissions::from_mode(0o755));
            eprintln!("[Engine] Set executable permission: {}", exe_path.display());
        }
    }

    // Clean temp dir
    let _ = std::fs::remove_dir_all(&temp_dir);

    // Update state: completed
    {
        let mut dl = state.lock().await;
        dl.status = "completed".to_string();
        dl.progress = 100;
        dl.file_name = "llama-server".to_string();
    }

    let install_path = LlmServer::find_llama_server()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();
    eprintln!("[Engine] Installation complete: {}", install_path);
}

// ============================================================
// HTTP API Server (replaces Tauri commands)
// ============================================================

type SharedServer = Arc<Mutex<LlmServer>>;

fn json_response(status: u16, body: &serde_json::Value) -> tiny_http::Response<std::io::Cursor<Vec<u8>>> {
    let body_str = serde_json::to_string(body).unwrap_or_default();
    let data = body_str.into_bytes();
    let len = data.len();
    let cursor = std::io::Cursor::new(data);

    let status_line = match status {
        200 => tiny_http::StatusCode(200),
        400 => tiny_http::StatusCode(400),
        500 => tiny_http::StatusCode(500),
        _ => tiny_http::StatusCode(status),
    };

    tiny_http::Response::new(
        status_line,
        vec![
            tiny_http::Header::from_bytes("Content-Type", "application/json").unwrap(),
            tiny_http::Header::from_bytes("Access-Control-Allow-Origin", "*").unwrap(),
            tiny_http::Header::from_bytes("Access-Control-Allow-Methods", "GET, POST, OPTIONS").unwrap(),
            tiny_http::Header::from_bytes("Access-Control-Allow-Headers", "*").unwrap(),
        ],
        cursor,
        Some(len),
        None,
    )
}

async fn handle_request(
    server: SharedServer,
    download_state: SharedDownload,
    method: &str,
    path: &str,
    body: &str,
) -> (u16, serde_json::Value) {
    match (method, path) {
        ("GET", "/api/status") => {
            let srv = server.lock().await;
            (200, serde_json::to_value(srv.get_info()).unwrap_or_default())
        }

        ("GET", "/api/gpu") => {
            // Try cached first, then detect
            let info = get_gpu_info().or_else(|| detect_gpu());
            match info {
                Some(gpu) => (200, serde_json::to_value(gpu).unwrap_or_default()),
                None => (200, serde_json::json!(null)),
            }
        }

        ("GET", path) if path.starts_with("/api/models") => {
            // Parse ?dir= query param
            let dir = path.split("dir=").nth(1)
                .map(|d| urlencoding::decode(d).unwrap_or_default().to_string())
                .unwrap_or_else(|| {
                    get_models_dirs().first().cloned().unwrap_or_default()
                });
            let files = scan_gguf_files(&dir, 3);
            (200, serde_json::to_value(files).unwrap_or_default())
        }

        ("GET", path) if path.starts_with("/api/hf-models") => {
            let dir = path.split("dir=").nth(1)
                .map(|d| urlencoding::decode(d).unwrap_or_default().to_string())
                .unwrap_or_else(|| {
                    get_models_dirs().first().cloned().unwrap_or_default()
                });
            let models = scan_hf_models(&dir, 3);
            (200, serde_json::to_value(models).unwrap_or_default())
        }

        ("GET", "/api/dirs") => {
            let dirs = get_models_dirs();
            (200, serde_json::to_value(dirs).unwrap_or_default())
        }

        ("GET", "/api/logs") => {
            let srv = server.lock().await;
            (200, serde_json::to_value(srv.get_logs()).unwrap_or_default())
        }

        ("GET", "/api/engine/status") => {
            (200, get_engine_status())
        }

        ("POST", "/api/engine/install") => {
            // Check if already downloading
            {
                let dl = download_state.lock().await;
                if dl.status == "downloading" || dl.status == "speed_test" || dl.status == "installing" {
                    return (409, serde_json::json!({
                        "error": "A download is already in progress",
                        "fileName": dl.file_name
                    }));
                }
            }

            // Spawn background engine install task
            let dl_state = download_state.clone();
            let body_json: serde_json::Value = serde_json::from_str(body)
                .unwrap_or_else(|_| serde_json::json!({}));
            let rt = body_json.get("runtime")
                .and_then(|v| v.as_str())
                .unwrap_or("llama-server")
                .to_string();
            tokio::spawn(async move {
                install_engine_task(dl_state, rt).await;
            });

            (200, serde_json::json!({
                "ok": true,
                "status": "started",
                "platform": std::env::consts::OS,
                "files": get_llama_platform_files()
            }))
        }

        ("POST", "/api/start") => {
            let req: serde_json::Value = serde_json::from_str(body)
                .unwrap_or_else(|_| serde_json::json!({}));

            let model_path = req.get("modelPath")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let port = req.get("port")
                .and_then(|v| v.as_u64())
                .unwrap_or(11434) as u16;
            let gpu_layers = req.get("gpuLayers")
                .and_then(|v| v.as_i64())
                .map(|v| v as i32);
            let context_size = req.get("contextSize")
                .and_then(|v| v.as_u64())
                .map(|v| v as u32);
            let runtime = req.get("runtime")
                .and_then(|v| v.as_str())
                .unwrap_or("llama-server");

            if model_path.is_empty() {
                return (400, serde_json::json!({"error": "modelPath is required"}));
            }

            let mut srv = server.lock().await;
            match srv.start(model_path, port, gpu_layers, context_size, runtime).await {
                Ok(()) => (200, serde_json::to_value(srv.get_info()).unwrap_or_default()),
                Err(e) => (500, serde_json::json!({"error": e})),
            }
        }

        ("POST", "/api/stop") => {
            let mut srv = server.lock().await;
            match srv.stop().await {
                Ok(()) => (200, serde_json::json!({"ok": true})),
                Err(e) => (500, serde_json::json!({"error": e})),
            }
        }

        // ── Model Download API ──

        ("POST", "/api/download") => {
            let req: serde_json::Value = serde_json::from_str(body)
                .unwrap_or_else(|_| serde_json::json!({}));

            let repo = req.get("repo")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let file_name = req.get("fileName")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            if repo.is_empty() || file_name.is_empty() {
                return (400, serde_json::json!({"error": "repo and fileName are required"}));
            }

            // Check if already downloading
            {
                let dl = download_state.lock().await;
                if dl.status == "downloading" || dl.status == "speed_test" {
                    return (409, serde_json::json!({
                        "error": "A download is already in progress",
                        "fileName": dl.file_name
                    }));
                }
            }

            // Spawn background download task
            let dl_state = download_state.clone();
            tokio::spawn(async move {
                download_model_task(dl_state, repo, file_name).await;
            });

            (200, serde_json::json!({"ok": true, "status": "started"}))
        }

        ("GET", "/api/download/status") => {
            let dl = download_state.lock().await;
            (200, serde_json::to_value(&*dl).unwrap_or_default())
        }

        ("POST", "/api/download/cancel") => {
            DOWNLOAD_ABORT.store(true, Ordering::SeqCst);

            // Also try to delete temp file
            let file_name = {
                let dl = download_state.lock().await;
                dl.file_name.clone()
            };
            if !file_name.is_empty() {
                let download_dir = get_models_dirs().first().cloned().unwrap_or_default();
                let temp_path = PathBuf::from(&download_dir).join(format!("{}.downloading", file_name));
                if temp_path.exists() {
                    let _ = std::fs::remove_file(&temp_path);
                    eprintln!("[Download] Cleaned temp file: {}", temp_path.display());
                }
            }

            // Update state
            {
                let mut dl = download_state.lock().await;
                dl.status = "cancelled".to_string();
            }

            (200, serde_json::json!({"ok": true}))
        }

        _ => {
            (404, serde_json::json!({"error": "Not found"}))
        }
    }
}

// ============================================================
// Unified Proxy (from local_llm.rs lines 1342-1737)
// ============================================================

async fn run_unified_proxy(
    listen_port: u16,
    target_port: u16,
    mut shutdown_rx: watch::Receiver<bool>,
) -> Result<(), String> {
    use tokio::net::TcpListener;

    let addr = format!("0.0.0.0:{}", listen_port);
    let listener = TcpListener::bind(&addr).await
        .map_err(|e| format!("Failed to bind proxy on {}: {}", addr, e))?;

    eprintln!("[Proxy] Unified proxy listening on port {}", listen_port);

    loop {
        tokio::select! {
            _ = shutdown_rx.changed() => {
                if *shutdown_rx.borrow() {
                    eprintln!("[Proxy] Shutdown received, stopping proxy");
                    break;
                }
            }
            accept = listener.accept() => {
                match accept {
                    Ok((stream, _peer)) => {
                        let tp = target_port;
                        tokio::spawn(async move {
                            if let Err(e) = handle_proxy_connection(stream, tp).await {
                                eprintln!("[Proxy] Connection error: {}", e);
                            }
                        });
                    }
                    Err(e) => {
                        eprintln!("[Proxy] Accept error: {}", e);
                    }
                }
            }
        }
    }

    Ok(())
}

/// Read the complete HTTP request (headers + full body) from a TCP stream.
/// Bug 1 fix: a single read() call is NOT guaranteed to receive the entire body,
/// especially for large Anthropic requests. We must read until Content-Length bytes.
async fn read_full_http_request(stream: &mut tokio::net::TcpStream) -> Result<Vec<u8>, String> {
    use tokio::io::AsyncReadExt;

    let mut buf = Vec::with_capacity(65536);
    let mut tmp = [0u8; 8192];

    // Read until we have headers (double CRLF)
    let header_end = loop {
        let n = stream.read(&mut tmp).await.map_err(|e| format!("Read error: {}", e))?;
        if n == 0 { break buf.len(); }
        buf.extend_from_slice(&tmp[..n]);
        if let Some(pos) = find_header_end(&buf) {
            break pos;
        }
    };

    // Parse Content-Length from headers
    let header_str = String::from_utf8_lossy(&buf[..header_end]);
    let content_length: usize = header_str.lines()
        .find(|l| l.to_lowercase().starts_with("content-length:"))
        .and_then(|l| l.splitn(2, ':').nth(1))
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(0);

    // Read remaining body bytes
    let body_start = header_end + 4; // skip \r\n\r\n
    let body_received = if buf.len() > body_start { buf.len() - body_start } else { 0 };
    let remaining = content_length.saturating_sub(body_received);

    if remaining > 0 {
        let mut more = vec![0u8; remaining];
        let mut got = 0;
        while got < remaining {
            let n = stream.read(&mut more[got..]).await.map_err(|e| format!("Body read: {}", e))?;
            if n == 0 { break; }
            got += n;
        }
        buf.extend_from_slice(&more[..got]);
    }

    Ok(buf)
}

fn find_header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n")
}

/// Extract API key from request headers (supports x-api-key and Authorization: Bearer)
/// Bug 2 fix: forward the client's API key to llama-server internal port.
fn extract_api_key_from_request(raw: &[u8]) -> Option<String> {
    let header_str = String::from_utf8_lossy(raw);
    for line in header_str.lines() {
        let lower = line.to_lowercase();
        if lower.starts_with("x-api-key:") {
            let key = line["x-api-key:".len()..].trim().to_string();
            if !key.is_empty() { return Some(key); }
        } else if lower.starts_with("authorization:") {
            let val = line["authorization:".len()..].trim();
            if let Some(token) = val.strip_prefix("Bearer ").or_else(|| val.strip_prefix("bearer ")) {
                let key = token.trim().to_string();
                if !key.is_empty() { return Some(key); }
            }
        }
    }
    None
}

async fn handle_proxy_connection(
    mut stream: tokio::net::TcpStream,
    target_port: u16,
) -> Result<(), String> {
    use tokio::io::AsyncWriteExt;

    // Bug 1 fix: read the complete HTTP request including full body
    let raw = read_full_http_request(&mut stream).await?;
    if raw.is_empty() { return Ok(()); }

    let raw_str = String::from_utf8_lossy(&raw);
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
        handle_anthropic_proxy(&mut stream, &raw, target_port).await
    } else {
        handle_passthrough(&mut stream, &raw, target_port).await
    }
}

async fn handle_passthrough(
    stream: &mut tokio::net::TcpStream,
    raw_request: &[u8],
    target_port: u16,
) -> Result<(), String> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpStream;

    let mut target = TcpStream::connect(format!("127.0.0.1:{}", target_port)).await
        .map_err(|e| format!("Connect to llama-server failed: {}", e))?;

    target.write_all(raw_request).await
        .map_err(|e| format!("Write to target: {}", e))?;

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

async fn handle_anthropic_proxy(
    stream: &mut tokio::net::TcpStream,
    raw_request: &[u8],
    target_port: u16,
) -> Result<(), String> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let raw_str = String::from_utf8_lossy(raw_request);

    // Bug 2 fix: extract API key from client request headers
    let api_key = extract_api_key_from_request(raw_request);

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

    // Convert Anthropic request to OpenAI format
    let openai_req = anthropic_to_openai(&anthropic_req);

    eprintln!("[Proxy] Anthropic->OpenAI: {} messages, stream={}",
        openai_req.get("messages").and_then(|m| m.as_array()).map(|a| a.len()).unwrap_or(0),
        is_stream
    );

    let post_data = serde_json::to_string(&openai_req)
        .map_err(|e| format!("Serialize: {}", e))?;

    // Bug 2 fix: forward API key to llama-server internal port
    let auth_header = if let Some(ref key) = api_key {
        format!("Authorization: Bearer {}\r\n", key)
    } else {
        String::new()
    };

    // Forward to llama-server /v1/chat/completions
    let http_req = format!(
        "POST /v1/chat/completions HTTP/1.1\r\n\
         Host: 127.0.0.1:{}\r\n\
         Content-Type: application/json\r\n\
         {}Content-Length: {}\r\n\
         Connection: close\r\n\r\n{}",
        target_port, auth_header, post_data.len(), post_data
    );

    let mut target = tokio::net::TcpStream::connect(format!("127.0.0.1:{}", target_port)).await
        .map_err(|e| format!("Connect to llama-server: {}", e))?;

    target.write_all(http_req.as_bytes()).await
        .map_err(|e| format!("Write to target: {}", e))?;

    if is_stream {
        // 鈹€鈹€ TRUE STREAMING PATH 鈹€鈹€
        // Read llama-server HTTP response headers first (byte-by-byte until \r\n\r\n)
        let mut header_buf = Vec::new();
        let mut single = [0u8; 1];
        loop {
            let n = target.read(&mut single).await
                .map_err(|e| format!("Read header byte: {}", e))?;
            if n == 0 { break; }
            header_buf.push(single[0]);
            if header_buf.ends_with(b"\r\n\r\n") { break; }
            if header_buf.len() > 16384 { break; }
        }

        // Immediately send SSE response headers to the client
        let sse_headers = "HTTP/1.1 200 OK\r\n\
            Content-Type: text/event-stream\r\n\
            Cache-Control: no-cache\r\n\
            Connection: keep-alive\r\n\
            Access-Control-Allow-Origin: *\r\n\r\n";
        stream.write_all(sse_headers.as_bytes()).await
            .map_err(|e| format!("Write SSE headers: {}", e))?;

        // Emit Anthropic message_start event
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default().as_millis();
        let msg_id = format!("msg_{}", now_ms);
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
        let evt = format!("event: message_start\ndata: {}\n\n", msg_start);
        stream.write_all(evt.as_bytes()).await
            .map_err(|e| format!("Write message_start: {}", e))?;

        // Emit Anthropic content_block_start event (text block)
        let block_start = serde_json::json!({
            "type": "content_block_start",
            "index": 0,
            "content_block": {"type": "text", "text": ""}
        });
        let evt = format!("event: content_block_start\ndata: {}\n\n", block_start);
        stream.write_all(evt.as_bytes()).await
            .map_err(|e| format!("Write content_block_start: {}", e))?;
        stream.flush().await.ok();

        // Stream body: read chunks, parse SSE lines, convert and forward
        let mut partial = String::new();
        let mut chunk_buf = vec![0u8; 8192];
        let mut done = false;
        while !done {
            let n = target.read(&mut chunk_buf).await
                .map_err(|e| format!("Read SSE chunk: {}", e))?;
            if n == 0 { break; }

            partial.push_str(&String::from_utf8_lossy(&chunk_buf[..n]));

            loop {
                if let Some(nl) = partial.find('\n') {
                    let line = partial[..nl].trim_end_matches('\r').to_string();
                    partial = partial[nl + 1..].to_string();

                    if !line.starts_with("data: ") { continue; }
                    let json_str = &line[6..];
                    if json_str == "[DONE]" { done = true; break; }

                    if let Ok(chunk) = serde_json::from_str::<serde_json::Value>(json_str) {
                        let delta = chunk.get("choices")
                            .and_then(|c| c.as_array())
                            .and_then(|arr| arr.first())
                            .and_then(|c| c.get("delta"));

                        // Text content delta
                        if let Some(text) = delta.and_then(|d| d.get("content")).and_then(|c| c.as_str()) {
                            if !text.is_empty() {
                                let ev = serde_json::json!({
                                    "type": "content_block_delta",
                                    "index": 0,
                                    "delta": {"type": "text_delta", "text": text}
                                });
                                let evt = format!("event: content_block_delta\ndata: {}\n\n", ev);
                                if stream.write_all(evt.as_bytes()).await.is_err() {
                                    return Ok(());
                                }
                            }
                        }

                        // Tool call delta
                        if let Some(tool_calls) = delta.and_then(|d| d.get("tool_calls")).and_then(|t| t.as_array()) {
                            for tc in tool_calls {
                                let idx = tc.get("index").and_then(|v| v.as_u64()).unwrap_or(0);
                                if let Some(id) = tc.get("id").and_then(|v| v.as_str()) {
                                    let name = tc.get("function").and_then(|f| f.get("name")).and_then(|v| v.as_str()).unwrap_or("");
                                    if idx > 0 {
                                        let stop = serde_json::json!({"type": "content_block_stop", "index": idx - 1});
                                        let _ = stream.write_all(format!("event: content_block_stop\ndata: {}\n\n", stop).as_bytes()).await;
                                    }
                                    let start = serde_json::json!({
                                        "type": "content_block_start",
                                        "index": idx,
                                        "content_block": {"type": "tool_use", "id": id, "name": name, "input": {}}
                                    });
                                    let _ = stream.write_all(format!("event: content_block_start\ndata: {}\n\n", start).as_bytes()).await;
                                }
                                if let Some(args) = tc.get("function").and_then(|f| f.get("arguments")).and_then(|v| v.as_str()) {
                                    if !args.is_empty() {
                                        let ev = serde_json::json!({
                                            "type": "content_block_delta",
                                            "index": idx,
                                            "delta": {"type": "input_json_delta", "partial_json": args}
                                        });
                                        let _ = stream.write_all(format!("event: content_block_delta\ndata: {}\n\n", ev).as_bytes()).await;
                                    }
                                }
                            }
                        }
                    }
                } else {
                    break;
                }
            }
            stream.flush().await.ok();
        }

        // Emit Anthropic closing events
        let block_stop = format!("event: content_block_stop\ndata: {}\n\n",
            serde_json::json!({"type": "content_block_stop", "index": 0}));
        stream.write_all(block_stop.as_bytes()).await.ok();

        let msg_delta = format!("event: message_delta\ndata: {}\n\n",
            serde_json::json!({
                "type": "message_delta",
                "delta": {"stop_reason": "end_turn", "stop_sequence": null},
                "usage": {"output_tokens": 0}
            }));
        stream.write_all(msg_delta.as_bytes()).await.ok();

        let msg_stop = format!("event: message_stop\ndata: {}\n\n",
            serde_json::json!({"type": "message_stop"}));
        stream.write_all(msg_stop.as_bytes()).await.ok();
        stream.flush().await.ok();

    } else {
        // 鈹€鈹€ NON-STREAMING PATH 鈹€鈹€
        let mut resp_data = Vec::new();
        let mut buf = vec![0u8; 8192];
        loop {
            let n = target.read(&mut buf).await
                .map_err(|e| format!("Read from target: {}", e))?;
            if n == 0 { break; }
            resp_data.extend_from_slice(&buf[..n]);
        }

        let resp_str = String::from_utf8_lossy(&resp_data);
        let resp_body = if let Some(pos) = resp_str.find("\r\n\r\n") {
            &resp_str[pos + 4..]
        } else {
            &resp_str[..]
        };

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
// Format Conversion
// ============================================================

fn anthropic_to_openai(body: &serde_json::Value) -> serde_json::Value {
    let mut messages = Vec::new();

    // System message
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

    // Conversation messages 鈥?handle text, tool_use, and tool_result blocks
    if let Some(msgs) = body.get("messages").and_then(|m| m.as_array()) {
        for msg in msgs {
            let role = msg.get("role").and_then(|r| r.as_str()).unwrap_or("user");

            match msg.get("content") {
                Some(c) if c.is_string() => {
                    messages.push(serde_json::json!({"role": role, "content": c.as_str().unwrap_or("")}));
                }
                Some(c) if c.is_array() => {
                    let blocks = c.as_array().unwrap();

                    let text_parts: Vec<&str> = blocks.iter()
                        .filter(|b| b.get("type").and_then(|t| t.as_str()) == Some("text"))
                        .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
                        .collect();

                    let tool_use_blocks: Vec<&serde_json::Value> = blocks.iter()
                        .filter(|b| b.get("type").and_then(|t| t.as_str()) == Some("tool_use"))
                        .collect();

                    let tool_result_blocks: Vec<&serde_json::Value> = blocks.iter()
                        .filter(|b| b.get("type").and_then(|t| t.as_str()) == Some("tool_result"))
                        .collect();

                    if !tool_use_blocks.is_empty() {
                        let tool_calls: Vec<serde_json::Value> = tool_use_blocks.iter().enumerate().map(|(i, tu)| {
                            let id = tu.get("id").and_then(|v| v.as_str()).unwrap_or("call_0");
                            let name = tu.get("name").and_then(|v| v.as_str()).unwrap_or("");
                            let input = tu.get("input").cloned().unwrap_or(serde_json::json!({}));
                            serde_json::json!({
                                "id": id,
                                "index": i,
                                "type": "function",
                                "function": {
                                    "name": name,
                                    "arguments": serde_json::to_string(&input).unwrap_or_else(|_| "{}".to_string())
                                }
                            })
                        }).collect();
                        let text_content = text_parts.join("");
                        let mut assistant_msg = serde_json::json!({
                            "role": "assistant",
                            "tool_calls": tool_calls
                        });
                        if !text_content.is_empty() {
                            assistant_msg["content"] = serde_json::json!(text_content);
                        }
                        messages.push(assistant_msg);
                    } else if !tool_result_blocks.is_empty() {
                        for tr in &tool_result_blocks {
                            let tool_call_id = tr.get("tool_use_id").and_then(|v| v.as_str()).unwrap_or("");
                            let result_content = if let Some(s) = tr.get("content").and_then(|c| c.as_str()) {
                                s.to_string()
                            } else if let Some(arr) = tr.get("content").and_then(|c| c.as_array()) {
                                arr.iter()
                                    .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
                                    .collect::<Vec<_>>()
                                    .join("")
                            } else {
                                String::new()
                            };
                            messages.push(serde_json::json!({
                                "role": "tool",
                                "tool_call_id": tool_call_id,
                                "content": result_content
                            }));
                        }
                    } else {
                        messages.push(serde_json::json!({"role": role, "content": text_parts.join("")}));
                    }
                }
                _ => {
                    messages.push(serde_json::json!({"role": role, "content": ""}));
                }
            }
        }
    }

    // Convert Anthropic tools 鈫?OpenAI tools
    let mut req = serde_json::json!({
        "model": body.get("model").and_then(|m| m.as_str()).unwrap_or("local-model"),
        "messages": messages,
        "max_tokens": body.get("max_tokens").and_then(|v| v.as_u64()).unwrap_or(4096),
        "temperature": body.get("temperature").and_then(|v| v.as_f64()).unwrap_or(0.7),
        "stream": body.get("stream").and_then(|v| v.as_bool()).unwrap_or(false),
    });

    if let Some(tools) = body.get("tools").and_then(|t| t.as_array()) {
        if !tools.is_empty() {
            let openai_tools: Vec<serde_json::Value> = tools.iter().map(|t| {
                serde_json::json!({
                    "type": "function",
                    "function": {
                        "name": t.get("name").and_then(|v| v.as_str()).unwrap_or(""),
                        "description": t.get("description").and_then(|v| v.as_str()).unwrap_or(""),
                        "parameters": t.get("input_schema").cloned().unwrap_or(serde_json::json!({"type": "object", "properties": {}}))
                    }
                })
            }).collect();
            req["tools"] = serde_json::json!(openai_tools);
            if let Some(tc) = body.get("tool_choice") {
                req["tool_choice"] = tc.clone();
            }
        }
    }

    req
}

/// Convert OpenAI Chat Completions non-streaming response -> Anthropic Messages response
fn openai_to_anthropic(data: &serde_json::Value) -> serde_json::Value {
    let model = data.get("model").and_then(|m| m.as_str()).unwrap_or("local-model");
    let choice = data.get("choices").and_then(|c| c.as_array()).and_then(|a| a.first());
    let message = choice.and_then(|c| c.get("message"));
    let finish_reason = choice.and_then(|c| c.get("finish_reason")).and_then(|v| v.as_str()).unwrap_or("end_turn");

    let mut content_blocks: Vec<serde_json::Value> = Vec::new();

    // Text content
    if let Some(text) = message.and_then(|m| m.get("content")).and_then(|c| c.as_str()) {
        if !text.is_empty() {
            content_blocks.push(serde_json::json!({"type": "text", "text": text}));
        }
    }

    // Tool calls -> Anthropic tool_use blocks
    if let Some(tool_calls) = message.and_then(|m| m.get("tool_calls")).and_then(|t| t.as_array()) {
        for tc in tool_calls {
            let id = tc.get("id").and_then(|v| v.as_str()).unwrap_or("call_0");
            let name = tc.get("function").and_then(|f| f.get("name")).and_then(|v| v.as_str()).unwrap_or("");
            let args_str = tc.get("function").and_then(|f| f.get("arguments")).and_then(|v| v.as_str()).unwrap_or("{}");
            let input: serde_json::Value = serde_json::from_str(args_str).unwrap_or(serde_json::json!({}));
            content_blocks.push(serde_json::json!({
                "type": "tool_use",
                "id": id,
                "name": name,
                "input": input
            }));
        }
    }

    if content_blocks.is_empty() {
        content_blocks.push(serde_json::json!({"type": "text", "text": ""}));
    }

    // Map OpenAI finish_reason -> Anthropic stop_reason
    let stop_reason = match finish_reason {
        "tool_calls" => "tool_use",
        "stop" | "end_turn" => "end_turn",
        "length" => "max_tokens",
        other => other,
    };

    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default().as_millis();

    serde_json::json!({
        "id": format!("msg_{}", now_ms),
        "type": "message",
        "role": "assistant",
        "content": content_blocks,
        "model": model,
        "stop_reason": stop_reason,
        "stop_sequence": null,
        "usage": {
            "input_tokens": data.get("usage").and_then(|u| u.get("prompt_tokens")).and_then(|v| v.as_u64()).unwrap_or(0),
            "output_tokens": data.get("usage").and_then(|u| u.get("completion_tokens")).and_then(|v| v.as_u64()).unwrap_or(0),
        }
    })
}
// ============================================================
// Main Entry Point
// ============================================================

fn main() {
    let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
    let handle = rt.handle().clone();

    let api_port: u16 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(8090);

    eprintln!("========================================");
    eprintln!("  Echobird LLM Server v0.1.0");
    eprintln!("  API: http://0.0.0.0:{}", api_port);
    eprintln!("========================================");

    let server: SharedServer = Arc::new(Mutex::new(LlmServer::new()));
    let download_state: SharedDownload = Arc::new(Mutex::new(DownloadProgress::default()));

    // Detect GPU on startup
    if let Some(gpu) = detect_gpu() {
        eprintln!("[GPU] Detected: {} ({:.1} GB VRAM)", gpu.gpu_name, gpu.gpu_vram_gb);
    } else {
        eprintln!("[GPU] No GPU detected");
    }

    // Start HTTP API server (tiny_http is blocking, runs on main thread)
    let http_server = tiny_http::Server::http(format!("0.0.0.0:{}", api_port))
        .expect("Failed to start HTTP server");

    eprintln!("[API] Listening on http://0.0.0.0:{}", api_port);

    loop {
        let mut request = match http_server.recv() {
            Ok(r) => r,
            Err(e) => {
                eprintln!("[API] Receive error: {}", e);
                continue;
            }
        };

        let method = request.method().to_string();
        let url = request.url().to_string();

        // Handle CORS preflight
        if method == "OPTIONS" {
            let resp = json_response(204, &serde_json::json!(null));
            let _ = request.respond(resp);
            continue;
        }

        // Read body
        let mut body = String::new();
        if let Some(mut reader) = Some(request.as_reader()) {
            let _ = std::io::Read::read_to_string(&mut reader, &mut body);
        }

        // Extract path without query string for logging
        let path = url.split('?').next().unwrap_or(&url).to_string();

        let srv = server.clone();
        let dl = download_state.clone();
        // Main thread is NOT a tokio worker, so handle.block_on() works fine
        let (status, resp_body) = handle.block_on(handle_request(srv, dl, &method, &url, &body));

        let resp = json_response(status, &resp_body);
        if let Err(e) = request.respond(resp) {
            eprintln!("[API] Response error: {}", e);
        }

        eprintln!("[API] {} {} -> {}", method, path, status);
    }
}

