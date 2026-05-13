// Local LLM server lifecycle management
// Handles: start, stop, find binary, stdout/stderr piped reading

#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;
use tauri::Emitter;
use tokio::sync::OnceCell;
use tokio::sync::{watch, Mutex};

use super::proxy::run_unified_proxy;
use super::types::LocalServerInfo;

const MAX_LOGS: usize = 1000;

/// Local LLM Server Manager
pub struct LocalLlmServer {
    pub(super) info: LocalServerInfo,
    pub(super) logs: Vec<String>,
    pub(super) child_pid: Option<u32>,
    pub(super) proxy_shutdown: Option<watch::Sender<bool>>,
}

impl Default for LocalLlmServer {
    fn default() -> Self {
        Self::new()
    }
}

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
        let exe_name = if cfg!(windows) {
            "llama-server.exe"
        } else {
            "llama-server"
        };

        // 1. Next to current exe
        if let Ok(exe_path) = std::env::current_exe() {
            if let Some(dir) = exe_path.parent() {
                let candidate = dir.join(exe_name);
                if candidate.exists() {
                    return Some(candidate);
                }
                let candidate = dir.join("resources").join(exe_name);
                if candidate.exists() {
                    return Some(candidate);
                }
            }
        }

        // 2. ~/.echobird/llama-server/bin/
        let llama_bin_dir = crate::utils::platform::echobird_dir()
            .join("llama-server")
            .join("bin");
        if llama_bin_dir.exists() {
            let direct = llama_bin_dir.join(exe_name);
            if direct.exists() {
                return Some(direct);
            }
            if let Ok(entries) = std::fs::read_dir(&llama_bin_dir) {
                for entry in entries.flatten() {
                    if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                        let candidate = entry.path().join(exe_name);
                        if candidate.exists() {
                            return Some(candidate);
                        }
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
        let echobird_bin = crate::utils::platform::echobird_dir()
            .join("bin")
            .join(exe_name);
        if echobird_bin.exists() {
            return Some(echobird_bin);
        }

        // 4. System PATH (desktop only)
        #[cfg(not(target_os = "android"))]
        if let Ok(path) = which::which(exe_name) {
            return Some(path);
        }

        None
    }

    /// Start LLM runtime with model.
    /// `app_handle` is used to emit stdout/stderr lines to the frontend.
    pub async fn start(
        &mut self,
        model_path: &str,
        port: u16,
        gpu_layers: Option<i32>,
        context_size: Option<u32>,
        runtime: &str,
        app_handle: tauri::AppHandle,
    ) -> Result<(), String> {
        if self.info.running {
            return Err("Server already running".to_string());
        }

        let model_name = std::path::Path::new(model_path)
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "Unknown Model".to_string());

        // Pre-flight cleanup: kill any leftover llama-server processes from
        // previous sessions that may be occupying the port. This prevents the
        // "port already in use" and stale-api-key 401 issues.
        self.add_log("Cleaning up any stale llama-server processes...");
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            let _ = Command::new("taskkill")
                .args(["/F", "/IM", "llama-server.exe", "/T"])
                .creation_flags(0x08000000)
                .output();
        }
        #[cfg(not(windows))]
        {
            let _ = Command::new("pkill").args(["-f", "llama-server"]).output();
        }
        // Brief pause to let the OS release ports
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;

        let (child, needs_proxy) = match runtime {
            "vllm" => {
                self.add_log(&format!(
                    "Starting vLLM on port {} with model: {}",
                    port, model_name
                ));
                let mut args = vec![
                    "-m".to_string(),
                    "vllm.entrypoints.openai.api_server".to_string(),
                    "--model".to_string(),
                    model_path.to_string(),
                    "--port".to_string(),
                    port.to_string(),
                    "--host".to_string(),
                    "127.0.0.1".to_string(),
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
                self.add_log(&format!(
                    "Starting SGLang on port {} with model: {}",
                    port, model_name
                ));
                let mut args = vec![
                    "-m".to_string(),
                    "sglang.launch_server".to_string(),
                    "--model-path".to_string(),
                    model_path.to_string(),
                    "--port".to_string(),
                    port.to_string(),
                    "--host".to_string(),
                    "127.0.0.1".to_string(),
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
                self.add_log(&format!(
                    "Starting llama-server on port {} with model: {}",
                    port, model_name
                ));
                self.add_log(&format!(
                    "Internal port: {}, Proxy port: {}",
                    internal_port, port
                ));
                // No --api-key for local LLM: the server is localhost-only,
                // so authentication is unnecessary and causes 401 errors when
                // clients use a key from a previous session.
                let mut args = vec![
                    "-m".to_string(),
                    model_path.to_string(),
                    "--port".to_string(),
                    internal_port.to_string(),
                    "--host".to_string(),
                    "127.0.0.1".to_string(),
                ];
                if let Some(layers) = gpu_layers {
                    args.push("-ngl".to_string());
                    args.push(layers.to_string());
                }
                if let Some(ctx) = context_size {
                    args.push("-c".to_string());
                    args.push(ctx.to_string());
                }
                log::info!("[LocalLLM] Starting: {:?}", exe);

                #[cfg(windows)]
                let c = Command::new(&exe)
                    .args(&args)
                    .creation_flags(0x08000000) // CREATE_NO_WINDOW — keep UI clean
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::piped())
                    .spawn()
                    .map_err(|e| format!("Failed to spawn llama-server: {}", e))?;

                #[cfg(not(windows))]
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
            api_key: String::new(), // No auth for local LLM
            runtime: runtime.to_string(),
        };

        log::info!("[LocalLLM] {} started with PID: {}", runtime, pid);
        self.add_log(&format!("{} started (PID: {})", runtime, pid));

        // Bug 3 Fix: drain stdout + stderr so OS pipe buffer never fills up,
        // and emit each line to the frontend via "local-llm-stdout" event.
        spawn_output_reader(child, pid, app_handle.clone());

        if needs_proxy {
            let (shutdown_tx, shutdown_rx) = watch::channel(false);
            self.proxy_shutdown = Some(shutdown_tx);
            let proxy_port = port;
            let target_port = port + 100;
            let proxy_app = app_handle.clone();
            tokio::spawn(async move {
                // Small delay to let llama-server spin up its port
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                match run_unified_proxy(proxy_port, target_port, shutdown_rx, proxy_app.clone())
                    .await
                {
                    Ok(()) => {
                        log::info!("[LocalLLM] Proxy stopped cleanly");
                    }
                    Err(e) => {
                        log::error!("[LocalLLM] Proxy error: {}", e);
                        let _ = proxy_app.emit(
                            "local-llm-stdout",
                            format!("[ERROR] Proxy failed to start: {}", e),
                        );
                    }
                }
            });
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
                    .creation_flags(0x08000000)
                    .output();
            }

            #[cfg(not(windows))]
            {
                unsafe {
                    libc::kill(pid as i32, libc::SIGTERM);
                }
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                unsafe {
                    libc::kill(pid as i32, libc::SIGKILL);
                }
            }
        }

        if let Some(tx) = self.proxy_shutdown.take() {
            let _ = tx.send(true);
            log::info!("[LocalLLM] Proxy shutdown signal sent");
        }

        self.add_log("Server stopped");
        self.info = LocalServerInfo::default();
        self.child_pid = None;

        Ok(())
    }

    pub fn get_info(&self) -> LocalServerInfo {
        self.info.clone()
    }
    pub fn get_logs(&self) -> Vec<String> {
        self.logs.clone()
    }

    pub(super) fn add_log(&mut self, msg: &str) {
        let timestamp = chrono::Local::now().format("%H:%M:%S").to_string();
        self.logs.push(format!("[{}] {}", timestamp, msg));
        if self.logs.len() > MAX_LOGS {
            self.logs.drain(0..self.logs.len() - MAX_LOGS);
        }
    }
}

/// Spawn std blocking threads to drain stdout + stderr from a child process,
/// emitting each line to the frontend via "local-llm-stdout" event.
///
/// This is the Bug 3 fix: without draining the pipe, the OS buffer fills up
/// (~64 KB) and the child process blocks on write(), causing it to hang.
///
/// Crash detection: when the child exits unexpectedly, we update the server
/// state to running=false and emit a warning to the frontend STDOUT, so the
/// status indicator switches from green to stopped without requiring a restart.
fn spawn_output_reader(mut child: std::process::Child, pid: u32, app_handle: tauri::AppHandle) {
    use std::io::{BufRead, BufReader};

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    // Stdout reader thread
    if let Some(out) = stdout {
        let app = app_handle.clone();
        std::thread::spawn(move || {
            let reader = BufReader::new(out);
            for line in reader.lines() {
                match line {
                    Ok(l) => {
                        log::debug!("[llama-server stdout] {}", l);
                        let _ = app.emit("local-llm-stdout", &l);
                        // Write into server.logs so frontend polling picks it up
                        if let Ok(handle) = tokio::runtime::Handle::try_current() {
                            let line_clone = l.clone();
                            handle.spawn(async move {
                                let server = get_server().await;
                                let mut srv = server.lock().await;
                                srv.add_log(&line_clone);
                            });
                        }
                    }
                    Err(_) => break,
                }
            }
            log::info!("[LocalLLM] stdout reader finished for PID {}", pid);
        });
    }

    // Stderr reader thread
    if let Some(err) = stderr {
        let app = app_handle.clone();
        std::thread::spawn(move || {
            let reader = BufReader::new(err);
            for line in reader.lines() {
                match line {
                    Ok(l) => {
                        log::debug!("[llama-server stderr] {}", l);
                        let _ = app.emit("local-llm-stdout", &l);
                        // Write into server.logs so frontend polling picks it up
                        if let Ok(handle) = tokio::runtime::Handle::try_current() {
                            let line_clone = l.clone();
                            handle.spawn(async move {
                                let server = get_server().await;
                                let mut srv = server.lock().await;
                                srv.add_log(&line_clone);
                            });
                        }
                    }
                    Err(_) => break,
                }
            }
            log::info!("[LocalLLM] stderr reader finished for PID {}", pid);
        });
    }

    // Crash watcher: reap the child and detect unexpected exits
    let app = app_handle.clone();
    std::thread::spawn(move || {
        let exit_status = child.wait();
        log::info!("[LocalLLM] PID {} exited: {:?}", pid, exit_status);

        // Detect crash (non-zero exit or wait error)
        let crashed = match &exit_status {
            Ok(status) => !status.success(),
            Err(_) => true,
        };

        if crashed {
            log::error!(
                "[LocalLLM] PID {} crashed unexpectedly: {:?}",
                pid,
                exit_status
            );
            let _ = app.emit(
                "local-llm-stdout",
                "\u{26a0}\u{fe0f} LLM server process crashed! Status updated to Stopped.",
            );
            let _ = app.emit(
                "local-llm-stdout",
                "   Click START to restart the LLM server.",
            );
        } else {
            log::info!("[LocalLLM] PID {} exited cleanly", pid);
        }

        // Update the global server state — use tokio runtime that's already running
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                let server = get_server().await;
                let mut srv = server.lock().await;
                // Only reset if we're still tracking this PID (not already stopped manually)
                if srv.child_pid == Some(pid) {
                    srv.info.running = false;
                    srv.child_pid = None;
                    if crashed {
                        srv.add_log(
                            "\u{26a0}\u{fe0f} Process crashed — server stopped unexpectedly",
                        );
                    }
                    update_server_info_cache(&srv.info);
                    log::info!("[LocalLLM] Server state reset after PID {} exit", pid);
                }
            });
        } else {
            log::error!("[LocalLLM] No tokio runtime available in crash watcher thread");
        }
    });
}

// ─── Global singleton + public async API ───

static LOCAL_LLM: OnceCell<Arc<Mutex<LocalLlmServer>>> = OnceCell::const_new();

static SERVER_INFO_CACHE: std::sync::Mutex<Option<LocalServerInfo>> = std::sync::Mutex::new(None);

/// Get server info synchronously (for use in sync contexts like get_models)
pub fn get_server_info_sync() -> LocalServerInfo {
    SERVER_INFO_CACHE
        .lock()
        .unwrap()
        .clone()
        .unwrap_or_default()
}

pub(super) fn update_server_info_cache(info: &LocalServerInfo) {
    *SERVER_INFO_CACHE.lock().unwrap() = Some(info.clone());
}

async fn get_server() -> Arc<Mutex<LocalLlmServer>> {
    LOCAL_LLM
        .get_or_init(|| async { Arc::new(Mutex::new(LocalLlmServer::new())) })
        .await
        .clone()
}

/// Public alias for sibling modules (e.g. proxy) that need to write into server.logs
pub(super) async fn get_server_arc() -> Arc<Mutex<LocalLlmServer>> {
    get_server().await
}

pub async fn start_server(
    model_path: &str,
    port: u16,
    gpu_layers: Option<i32>,
    context_size: Option<u32>,
    runtime: &str,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    let server = get_server().await;
    let mut server = server.lock().await;
    let result = server
        .start(
            model_path,
            port,
            gpu_layers,
            context_size,
            runtime,
            app_handle,
        )
        .await;
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
