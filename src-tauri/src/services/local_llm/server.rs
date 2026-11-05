// Local LLM server lifecycle management
// Handles: start, stop, find binary, stdout/stderr piped reading

use std::path::PathBuf;
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

        // Pre-flight cleanup: kill the leftover llama-server we recorded
        // last time (if any). Uses the PID file so we only ever kill OUR
        // own server — user-launched llama-server instances (different
        // project, different port, etc.) are left alone.
        if let Some(stale_pid) = super::pid_file::read_pid_file() {
            self.add_log(&format!(
                "Cleaning up stale llama-server (pid={})...",
                stale_pid
            ));
            #[cfg(windows)]
            {
                let _ = crate::utils::process::command("taskkill")
                    .args(["/F", "/PID", &stale_pid.to_string()])
                    .output();
            }
            #[cfg(not(windows))]
            {
                unsafe {
                    libc::kill(stale_pid as i32, libc::SIGKILL);
                }
            }
            super::pid_file::delete_pid_file();
            // Brief pause to let the OS release ports
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        }

        // Zero-UI multi-GPU: detect once at spawn time, inject runtime-
        // specific tensor-parallel args when ≥2 NVIDIA GPUs are present.
        // Single-GPU / AMD-only / no-GPU users see no change in behaviour
        // (count == 0 or 1 short-circuits all the multi-GPU branches).
        let gpu_count = super::gpu::detect_nvidia_gpu_count();
        let multi_gpu = gpu_count >= 2;
        if multi_gpu {
            self.add_log(&format!(
                "Detected {} NVIDIA GPU(s) — enabling tensor parallelism across all of them",
                gpu_count
            ));
        }

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
                append_multi_gpu_args(&mut args, "vllm", gpu_count);
                if multi_gpu {
                    self.add_log(&format!(
                        "  vLLM tensor parallelism: --tensor-parallel-size {}",
                        gpu_count
                    ));
                }
                let c = crate::utils::process::command("python3")
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
                append_multi_gpu_args(&mut args, "sglang", gpu_count);
                if multi_gpu {
                    self.add_log(&format!("  SGLang tensor parallelism: --tp {}", gpu_count));
                }
                let c = crate::utils::process::command("python3")
                    .args(&args)
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::piped())
                    .spawn()
                    .map_err(|e| format!("Failed to spawn SGLang: {}", e))?;
                (c, false)
            }
            _ => {
                // llama-server (default): needs unified proxy
                let internal_port = port + 100;
                self.add_log(&format!(
                    "Starting llama-server on port {} with model: {}",
                    port, model_name
                ));
                self.add_log(&format!(
                    "Internal port: {}, Proxy port: {}",
                    internal_port, port
                ));
                // Custom override: a model with a stored custom command
                // launches the user's own executable + args verbatim (e.g. an
                // AMD user's Vulkan-compiled llama-server). No stored command
                // falls back to the auto-computed default below.
                let (exe, args) = match super::custom_command::get() {
                    Some(custom) => {
                        self.add_log("Using your custom launch command");
                        // Force the EchoBird-managed args (model from the UI
                        // selection, host + proxy-internal port) onto the user's
                        // command so the server always loads the selected model
                        // and listens where our proxy connects — the user owns
                        // the executable + flags, never the model/host/port.
                        let args =
                            ensure_managed_llama_args(custom.args, model_path, internal_port);
                        (PathBuf::from(custom.exe), args)
                    }
                    None => {
                        let (e, a) = build_llama_default_command(
                            model_path,
                            internal_port,
                            gpu_layers,
                            context_size,
                            gpu_count,
                        )?;
                        if multi_gpu {
                            let ratios = vec!["1"; gpu_count].join(",");
                            self.add_log(&format!(
                                "  llama-server tensor parallelism: --tensor-split {} --main-gpu 0",
                                ratios
                            ));
                        }
                        (PathBuf::from(e), a)
                    }
                };
                log::info!("[LocalLLM] Starting: {:?}", exe);

                let c = crate::utils::process::command(&exe)
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

        // Record PID so Tauri's exit-cleanup can kill OUR server by PID,
        // and the next startup's pre-flight cleanup knows what to reap.
        super::pid_file::write_pid_file(pid, runtime);

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
                let _ = crate::utils::process::command("taskkill")
                    .args(["/pid", &pid.to_string(), "/T", "/F"])
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

        // Drop the PID file — we exited cleanly, no orphan to reap.
        super::pid_file::delete_pid_file();

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
    if crate::commands::settings_commands::get_settings().local_llm_enabled == Some(false) {
        return Err("Local models are disabled in Settings".into());
    }
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

/// Only the child/proxy owned by this manager is stopped; external servers
/// are never searched for or terminated when the optional page is disabled.
pub async fn stop_server_if_running() -> Result<(), String> {
    let server = get_server().await;
    let mut server = server.lock().await;
    if server.get_info().running {
        server.stop().await?;
        update_server_info_cache(&server.get_info());
    }
    Ok(())
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

/// Build the default llama-server `(exe, args)` for a model — the exact
/// command `start()` would auto-run when no custom override is set. Shared by
/// the launch path and the "show default command" tauri command so the gear
/// dialog prefills precisely what we'd otherwise run. (No `--api-key`: the
/// server is localhost-only, so a stale key would only cause 401s.)
pub fn build_llama_default_command(
    model_path: &str,
    internal_port: u16,
    gpu_layers: Option<i32>,
    context_size: Option<u32>,
    gpu_count: usize,
) -> Result<(String, Vec<String>), String> {
    let exe =
        LocalLlmServer::find_llama_server().ok_or_else(|| "llama-server not found".to_string())?;
    let args = build_llama_default_args(
        model_path,
        internal_port,
        gpu_layers,
        context_size,
        gpu_count,
    );
    Ok((exe.to_string_lossy().to_string(), args))
}

fn build_llama_default_args(
    model_path: &str,
    internal_port: u16,
    gpu_layers: Option<i32>,
    context_size: Option<u32>,
    gpu_count: usize,
) -> Vec<String> {
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
    let basename = model_path.rsplit(['/', '\\']).next().unwrap_or(model_path);
    if ["MiniCPM5-2B-Q4_K_M.gguf", "MiniCPM5-2B-Q8_0.gguf"]
        .iter()
        .any(|name| basename.eq_ignore_ascii_case(name))
    {
        // Official GGUFs omit min_p; the model card recommends 0 to avoid repetition.
        args.push("--min-p".to_string());
        args.push("0".to_string());
    }
    if [
        "Qwable-9B-Claude-Fable-5-Q4_K_M.gguf",
        "Qwable-9B-Claude-Fable-5-Q6_K.gguf",
        "Qwable-9B-Claude-Fable-5-Q8_0.gguf",
    ]
    .iter()
    .any(|name| basename.eq_ignore_ascii_case(name))
    {
        // Publisher recommends a small repetition penalty to avoid reasoning loops.
        args.push("--min-p".to_string());
        args.push("0".to_string());
        args.push("--repeat-penalty".to_string());
        args.push("1.05".to_string());
    }
    append_multi_gpu_args(&mut args, "llama-server", gpu_count);
    args
}

/// Force the EchoBird-managed args (`-m` model, `--host`, `--port`) onto a custom
/// command's arg list. The custom command is a GLOBAL bring-your-own override
/// (exe + flags); the model comes from the UI selection and the host + proxy
/// port are ours, so we set them here regardless of what the user saved —
/// guaranteeing the server loads the selected model and our proxy can reach it.
fn ensure_managed_llama_args(
    mut args: Vec<String>,
    model_path: &str,
    internal_port: u16,
) -> Vec<String> {
    set_managed_arg(&mut args, &["-m", "--model"], model_path);
    set_managed_arg(&mut args, &["--host"], "127.0.0.1");
    set_managed_arg(&mut args, &["--port"], &internal_port.to_string());
    args
}

/// Set a flag's value in an arg list: if any alias in `flags` is present, replace
/// the element after it (or push the value if the flag is trailing); otherwise
/// append the canonical flag (`flags[0]`) + value.
fn set_managed_arg(args: &mut Vec<String>, flags: &[&str], value: &str) {
    if let Some(i) = args.iter().position(|a| flags.contains(&a.as_str())) {
        if i + 1 < args.len() {
            args[i + 1] = value.to_string();
        } else {
            args.push(value.to_string());
        }
    } else {
        args.push(flags[0].to_string());
        args.push(value.to_string());
    }
}

/// Append runtime-specific tensor-parallel args when ≥ 2 GPUs are
/// available. Pure function so the per-runtime arg shape (llama.cpp's
/// `--tensor-split <ratios>` vs vLLM's `--tensor-parallel-size N` vs
/// SGLang's `--tp N`) is testable without spawning a real process.
///
/// `runtime` is the same string the spawn-site `match` consumes
/// ("vllm" / "sglang" / anything else → llama-server). `gpu_count` is
/// the return value from `super::gpu::detect_nvidia_gpu_count()` — a
/// `0` or `1` makes this a no-op so single-GPU users keep their
/// existing single-GPU behaviour.
fn append_multi_gpu_args(args: &mut Vec<String>, runtime: &str, gpu_count: usize) {
    if gpu_count < 2 {
        return;
    }
    match runtime {
        "vllm" => {
            // vLLM: --tensor-parallel-size N. Must divide num_heads —
            // vLLM itself rejects misconfigurations, no need to pre-validate.
            args.push("--tensor-parallel-size".to_string());
            args.push(gpu_count.to_string());
        }
        "sglang" => {
            // SGLang: --tp N. Same divisibility constraint as vLLM,
            // SGLang enforces it on its own.
            args.push("--tp".to_string());
            args.push(gpu_count.to_string());
        }
        _ /* llama-server */ => {
            // llama.cpp: --tensor-split takes comma-separated weights
            // (not fractions — sum doesn't need to be 1). "1,1" gives
            // 50/50 split across 2 GPUs, "1,1,1,1" gives even split
            // across 4. --main-gpu pins non-distributable ops to GPU 0.
            let ratios = vec!["1"; gpu_count].join(",");
            args.push("--tensor-split".to_string());
            args.push(ratios);
            args.push("--main-gpu".to_string());
            args.push("0".to_string());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn disabling_an_idle_local_engine_is_idempotent_without_process_discovery() {
        assert!(!get_server_info().await.running);
        stop_server_if_running().await.unwrap();
        stop_server_if_running().await.unwrap();
        let server = get_server().await;
        let server = server.lock().await;
        assert!(server.child_pid.is_none());
        assert!(!server.info.running);
        assert!(server.proxy_shutdown.is_none());
    }

    #[test]
    fn minicpm5_2b_default_args_disable_min_p_for_official_files() {
        for model_path in [
            "MiniCPM5-2B-Q4_K_M.gguf",
            "/models/MINICPM5-2B-Q8_0.GGUF",
            r"D:\models\minicpm5-2b-q4_k_m.gguf",
        ] {
            assert_eq!(
                build_llama_default_args(model_path, 8180, Some(99), Some(8192), 2),
                [
                    "-m",
                    model_path,
                    "--port",
                    "8180",
                    "--host",
                    "127.0.0.1",
                    "-ngl",
                    "99",
                    "-c",
                    "8192",
                    "--min-p",
                    "0",
                    "--tensor-split",
                    "1,1",
                    "--main-gpu",
                    "0",
                ]
                .map(String::from)
                .to_vec(),
                "model path: {model_path}"
            );
        }
    }

    #[test]
    fn qwable_default_args_follow_publisher_sampling() {
        for model_path in [
            "Qwable-9B-Claude-Fable-5-Q4_K_M.gguf",
            "/models/QWABLE-9B-CLAUDE-FABLE-5-Q6_K.GGUF",
            r"D:\models\qwable-9b-claude-fable-5-q8_0.gguf",
        ] {
            assert_eq!(
                build_llama_default_args(model_path, 8180, None, None, 1),
                [
                    "-m",
                    model_path,
                    "--port",
                    "8180",
                    "--host",
                    "127.0.0.1",
                    "--min-p",
                    "0",
                    "--repeat-penalty",
                    "1.05",
                ]
                .map(String::from)
                .to_vec(),
                "model path: {model_path}"
            );
        }
    }

    #[test]
    fn other_models_and_matching_directories_keep_default_sampling() {
        for model_path in [
            "MiniCPM5-1B-Q4_K_M.gguf",
            "MiniCPM5-2B-DSpark-Q4_K_M.gguf",
            "MiniCPM5-2B-Distill-Q4_K_M.gguf",
            "MiniCPM5-20B-Q4_K_M.gguf",
            "MiniCPM5-2B-Q4_K_M.gguf.downloading",
            "/models/MiniCPM5-2B-Q4_K_M.gguf/other.gguf",
            r"D:\models\MiniCPM5-2B-Q8_0.gguf\other.gguf",
            "Qwable-9B-Claude-Fable-5-Q5_K_M.gguf",
            "Qwable-9B-Claude-Fable-5-Distill-Q4_K_M.gguf",
            "Qwable-9B-Claude-Fable-5-Q4_K_M.gguf.downloading",
            "/models/Qwable-9B-Claude-Fable-5-Q6_K.gguf/other.gguf",
            r"D:\models\Qwable-9B-Claude-Fable-5-Q8_0.gguf\other.gguf",
            "LFM2.5-2.6B-Q4_K_M.gguf",
        ] {
            assert_eq!(
                build_llama_default_args(model_path, 8180, None, None, 1),
                ["-m", model_path, "--port", "8180", "--host", "127.0.0.1"]
                    .map(String::from)
                    .to_vec(),
                "model path: {model_path}"
            );
        }
    }

    #[test]
    fn model_specific_sampling_keeps_custom_commands() {
        for model_path in [
            "MiniCPM5-2B-Q4_K_M.gguf",
            "Qwable-9B-Claude-Fable-5-Q4_K_M.gguf",
        ] {
            for custom in [
                vec![
                    "--min-p".into(),
                    "0.2".into(),
                    "--repeat-penalty".into(),
                    "1.2".into(),
                ],
                Vec::new(),
            ] {
                let mut expected = custom.clone();
                expected.extend(
                    ["-m", model_path, "--host", "127.0.0.1", "--port", "8180"].map(String::from),
                );
                assert_eq!(
                    ensure_managed_llama_args(custom, model_path, 8180),
                    expected
                );
            }
        }
    }

    #[test]
    fn managed_args_replace_when_present_and_append_when_missing() {
        // Present → replaced in place; the user's other flags are preserved.
        let custom = vec![
            "-m".to_string(),
            "OLD.gguf".to_string(),
            "--host".to_string(),
            "0.0.0.0".to_string(),
            "--port".to_string(),
            "9999".to_string(),
            "-ngl".to_string(),
            "99".to_string(),
        ];
        assert_eq!(
            ensure_managed_llama_args(custom, "NEW.gguf", 8080),
            [
                "-m",
                "NEW.gguf",
                "--host",
                "127.0.0.1",
                "--port",
                "8080",
                "-ngl",
                "99"
            ]
            .map(String::from)
            .to_vec()
        );

        // Missing → appended after the user's flags.
        let custom = vec!["--device".to_string(), "Vulkan0".to_string()];
        assert_eq!(
            ensure_managed_llama_args(custom, "M.gguf", 8080),
            [
                "--device",
                "Vulkan0",
                "-m",
                "M.gguf",
                "--host",
                "127.0.0.1",
                "--port",
                "8080"
            ]
            .map(String::from)
            .to_vec()
        );

        // `--model` alias recognized (replaced, no duplicate `-m`).
        let custom = vec!["--model".to_string(), "OLD.gguf".to_string()];
        assert_eq!(
            ensure_managed_llama_args(custom, "NEW.gguf", 8080),
            [
                "--model",
                "NEW.gguf",
                "--host",
                "127.0.0.1",
                "--port",
                "8080"
            ]
            .map(String::from)
            .to_vec()
        );
    }

    #[test]
    fn no_args_appended_for_zero_or_one_gpu() {
        // GPU detection failed / not installed.
        let mut a: Vec<String> = vec![];
        append_multi_gpu_args(&mut a, "llama-server", 0);
        assert!(a.is_empty(), "got: {:?}", a);

        // Single-card user.
        let mut a: Vec<String> = vec![];
        append_multi_gpu_args(&mut a, "llama-server", 1);
        assert!(a.is_empty(), "got: {:?}", a);

        let mut a: Vec<String> = vec![];
        append_multi_gpu_args(&mut a, "vllm", 1);
        assert!(a.is_empty(), "got: {:?}", a);

        let mut a: Vec<String> = vec![];
        append_multi_gpu_args(&mut a, "sglang", 1);
        assert!(a.is_empty(), "got: {:?}", a);
    }

    #[test]
    fn llama_server_gets_tensor_split_and_main_gpu() {
        let mut a: Vec<String> = vec!["-m".into(), "model.gguf".into()];
        append_multi_gpu_args(&mut a, "llama-server", 2);
        assert_eq!(
            a,
            vec![
                "-m".to_string(),
                "model.gguf".to_string(),
                "--tensor-split".to_string(),
                "1,1".to_string(),
                "--main-gpu".to_string(),
                "0".to_string(),
            ]
        );

        // 4-GPU box: "1,1,1,1".
        let mut a: Vec<String> = vec![];
        append_multi_gpu_args(&mut a, "llama-server", 4);
        assert_eq!(
            a,
            vec![
                "--tensor-split".to_string(),
                "1,1,1,1".to_string(),
                "--main-gpu".to_string(),
                "0".to_string(),
            ]
        );
    }

    #[test]
    fn vllm_gets_tensor_parallel_size() {
        let mut a: Vec<String> = vec![];
        append_multi_gpu_args(&mut a, "vllm", 2);
        assert_eq!(
            a,
            vec!["--tensor-parallel-size".to_string(), "2".to_string()]
        );

        let mut a: Vec<String> = vec![];
        append_multi_gpu_args(&mut a, "vllm", 8);
        assert_eq!(
            a,
            vec!["--tensor-parallel-size".to_string(), "8".to_string()]
        );
    }

    #[test]
    fn sglang_gets_tp() {
        let mut a: Vec<String> = vec![];
        append_multi_gpu_args(&mut a, "sglang", 2);
        assert_eq!(a, vec!["--tp".to_string(), "2".to_string()]);

        let mut a: Vec<String> = vec![];
        append_multi_gpu_args(&mut a, "sglang", 4);
        assert_eq!(a, vec!["--tp".to_string(), "4".to_string()]);
    }

    #[test]
    fn unknown_runtime_falls_through_to_llama_args() {
        // Defensive: any unknown runtime string lands in the wildcard
        // branch — which is the llama-server arg shape. Mirrors the
        // spawn-site match's `_` arm.
        let mut a: Vec<String> = vec![];
        append_multi_gpu_args(&mut a, "ollama-future", 2);
        assert_eq!(
            a,
            vec![
                "--tensor-split".to_string(),
                "1,1".to_string(),
                "--main-gpu".to_string(),
                "0".to_string(),
            ]
        );
    }
}
