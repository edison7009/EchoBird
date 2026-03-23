// Process Manager �?mirrors old processManager.ts
// Manages tool processes: start/stop CLI & GUI tools, monitor PIDs

use std::collections::HashMap;
use std::process::Command;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Process info for a running tool
#[derive(Debug, Clone)]
struct ProcessInfo {
    pid: u32,
}

/// Cooldown tracker (prevents rapid restarts)
struct CooldownSet {
    tools: HashMap<String, tokio::time::Instant>,
}

impl CooldownSet {
    fn new() -> Self {
        Self { tools: HashMap::new() }
    }

    fn is_cooling(&self, tool_id: &str) -> bool {
        if let Some(ts) = self.tools.get(tool_id) {
            ts.elapsed() < std::time::Duration::from_secs(3)
        } else {
            false
        }
    }

    fn mark(&mut self, tool_id: &str) {
        self.tools.insert(tool_id.to_string(), tokio::time::Instant::now());
    }
}

/// Global process manager state
pub struct ProcessManager {
    processes: HashMap<String, ProcessInfo>,
    cooldown: CooldownSet,
}

impl ProcessManager {
    pub fn new() -> Self {
        Self {
            processes: HashMap::new(),
            cooldown: CooldownSet::new(),
        }
    }

    /// Start a tool process �?mirrors old Electron processManager.startTool logic
    pub async fn start_tool(&mut self, tool_id: &str, start_command: Option<&str>) -> Result<(), String> {
        if self.cooldown.is_cooling(tool_id) {
            return Err("Please wait before launching again".to_string());
        }
        self.cooldown.mark(tool_id);

        // Claude Code: ensure onboarding is marked as completed
        if tool_id == "claudecode" {
            Self::ensure_claude_onboarding();
        }

        log::info!(
            "[ProcessManager] start_tool called: tool_id={}, start_command={:?}, \
             get_tool_start_command={:?}, get_tool_command={:?}, \
             get_tool_exe_path={:?}, is_vscode_extension={}",
            tool_id,
            start_command,
            crate::services::tool_manager::get_tool_start_command(tool_id),
            crate::services::tool_manager::get_tool_command(tool_id),
            crate::services::tool_manager::get_tool_exe_path(tool_id),
            crate::services::tool_manager::is_vscode_extension(tool_id),
        );

        // Priority 0: Codex launcher — MUST be checked before startCommand
        // because paths.json provides startCommand="codex" which would bypass the proxy
        if tool_id == "codex" {
            if let Some(launcher) = Self::find_codex_launcher() {
                log::info!("[ProcessManager] Found codex-launcher.cjs, launching via proxy: {:?}", launcher);
                return self.start_codex_launcher(tool_id, &launcher);
            }
        }

        // Priority 1: If explicit command is given from frontend, use it
        if let Some(cmd) = start_command {
            log::info!("[ProcessManager] Starting tool: {} with explicit command: {}", tool_id, cmd);
            return self.start_cli_tool(tool_id, cmd);
        }

        // Priority 2: CLI tools with startCommand in paths.json (e.g. "openclaw gateway")
        if let Some(command) = crate::services::tool_manager::get_tool_start_command(tool_id) {
            // Extract base command (first word) for existence check
            let base_cmd = command.split_whitespace().next().unwrap_or(&command);
            if crate::utils::platform::command_exists(base_cmd).await {
                log::info!("[ProcessManager] Starting CLI tool: {} with startCommand: {}", tool_id, command);
                return self.start_cli_tool(tool_id, &command);
            } else {
                log::warn!("[ProcessManager] startCommand base '{}' for tool '{}' not found in PATH, skipping", base_cmd, tool_id);
            }
        }

        // Priority 3: GUI executable found (for desktop apps like CodeBuddy)
        if crate::services::tool_manager::get_tool_exe_path(tool_id).is_some() {
            log::info!("[ProcessManager] Found GUI exe for {}, launching as desktop app", tool_id);
            return self.start_gui_tool(tool_id).await;
        }

        // Priority 4: VS Code extension tools (Cline, Roo Code, Continue) �?launch VS Code
        if crate::services::tool_manager::is_vscode_extension(tool_id) {
            log::info!("[ProcessManager] Tool {} is a VS Code extension, launching VS Code", tool_id);
            return self.launch_vscode(tool_id).await;
        }

        // Priority 5: Fall back to CLI command from paths.json "command" field
        if let Some(command) = crate::services::tool_manager::get_tool_command(tool_id) {
            if crate::utils::platform::command_exists(&command).await {
                log::info!("[ProcessManager] Falling back to CLI command for {}: {}", tool_id, command);
                return self.start_cli_tool(tool_id, &command);
            } else {
                log::warn!("[ProcessManager] CLI command '{}' for tool '{}' not found in PATH", command, tool_id);
            }
        }

        Err(format!("No executable or command found for tool '{}'. The tool may be installed but not in PATH.", tool_id))
    }

    /// Find codex-launcher.cjs (dual-spoofing proxy)
    fn find_codex_launcher() -> Option<std::path::PathBuf> {
        let tools_dir = crate::services::tool_manager::get_tools_dir();
        let launcher = tools_dir.join("codex").join("codex-launcher.cjs");
        if launcher.exists() {
            return Some(launcher);
        }
        None
    }
    /// Start Codex via launcher script — bypasses start_cli_tool's split_whitespace
    /// to avoid quoting issues with the file path argument.
    /// On Windows we run via `cmd /C node <launcher>` so that cmd.exe acts as the
    /// console host, ensuring codex.exe (a Rust TUI) gets a proper TTY on stdin.
    fn start_codex_launcher(&mut self, tool_id: &str, launcher: &std::path::Path) -> Result<(), String> {
        let home = dirs::home_dir().unwrap_or_default();

        // Read echobird config for env vars
        let config_path = home.join(".echobird").join("codex.json");
        let mut api_key_env: Option<String> = None;
        let mut base_url_env: Option<String> = None;
        if config_path.exists() {
            if let Ok(content) = std::fs::read_to_string(&config_path) {
                if let Ok(config) = serde_json::from_str::<serde_json::Value>(&content) {
                    api_key_env = config.get("apiKey").and_then(|v| v.as_str()).map(|s| s.to_string());
                    base_url_env = config.get("baseUrl").and_then(|v| v.as_str()).map(|s| s.to_string());
                }
            }
        }

        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NEW_PROCESS_GROUP: u32 = 0x00000200;
            const CREATE_NEW_CONSOLE: u32 = 0x00000010;

            // Strip the Windows extended-length path prefix "\\?\" if present.
            // Rust/Tauri may produce paths like \\?\D:\build-output\...\codex-launcher.cjs
            // in debug builds. cmd.exe cannot handle the \\?\ prefix, so we strip it
            // to get a plain DOS path like D:\build-output\...\codex-launcher.cjs.
            let launcher_str = launcher.to_string_lossy();
            let launcher_clean = launcher_str
                .strip_prefix(r"\\?\")
                .unwrap_or(&launcher_str);

            // Pass launcher path as a separate argument — do NOT embed it in a
            // quoted string. Embedding causes node.exe to receive a path with
            // literal quote characters, making module resolution fail.
            // cmd.exe /C node <path> works correctly with separate args.
            let mut cmd = Command::new("cmd");
            cmd.args(["/C", "node", launcher_clean]);
            cmd.current_dir(&home);
            if let Some(ref key) = api_key_env { cmd.env("OPENAI_API_KEY", key); }
            if let Some(ref url) = base_url_env { cmd.env("OPENAI_BASE_URL", url); }
            cmd.creation_flags(CREATE_NEW_PROCESS_GROUP | CREATE_NEW_CONSOLE);

            log::info!("[ProcessManager] Starting Codex via: cmd /C node {}", launcher_clean);

            return match cmd.spawn() {
                Ok(child) => {
                    let pid = child.id();
                    log::info!("[ProcessManager] Codex launcher started via cmd /C with PID: {}", pid);
                    self.processes.insert(tool_id.to_string(), ProcessInfo { pid });
                    Ok(())
                }
                Err(e) => Err(format!("Failed to launch Codex: {}", e)),
            };
        }

        #[cfg(not(windows))]
        {
            let mut cmd = Command::new("node");
            cmd.arg(launcher);
            cmd.current_dir(&home);
            if let Some(ref key) = api_key_env { cmd.env("OPENAI_API_KEY", key); }
            if let Some(ref url) = base_url_env { cmd.env("OPENAI_BASE_URL", url); }

            match cmd.spawn() {
                Ok(child) => {
                    let pid = child.id();
                    log::info!("[ProcessManager] Codex launcher started with PID: {}", pid);
                    self.processes.insert(tool_id.to_string(), ProcessInfo { pid });
                    Ok(())
                }
                Err(e) => Err(format!("Failed to launch Codex via node: {}", e)),
            }
        }
    }

    /// Start a CLI tool via terminal
    fn start_cli_tool(&mut self, tool_id: &str, command: &str) -> Result<(), String> {
        let home = dirs::home_dir().unwrap_or_default();
        let echobird_dir = dirs::home_dir().unwrap_or_default().join(".echobird");
        let config_path = echobird_dir.join(format!("{}.json", tool_id));

        // Read echobird config for env vars and model info
        let mut api_key_env: Option<String> = None;
        let mut base_url_env: Option<String> = None;
        let mut model_id: Option<String> = None;
        if config_path.exists() {
            if let Ok(content) = std::fs::read_to_string(&config_path) {
                if let Ok(config) = serde_json::from_str::<serde_json::Value>(&content) {
                    api_key_env = config.get("apiKey").and_then(|v| v.as_str()).map(|s| s.to_string());
                    base_url_env = config.get("baseUrl").and_then(|v| v.as_str()).map(|s| s.to_string());
                    model_id = config.get("modelId").and_then(|v| v.as_str()).map(|s| s.to_string());
                }
            }
        }

        // Build the final command with tool-specific args
        let mut full_command = command.to_string();

        // OpenCode: append --model echobird/{modelId} to force model selection
        if tool_id == "opencode" {
            if let Some(ref mid) = model_id {
                full_command = format!("{} --model echobird/{}", command, mid);
            }
        }


        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;

            const CREATE_NEW_PROCESS_GROUP: u32 = 0x00000200;
            const CREATE_NEW_CONSOLE: u32 = 0x00000010;

            // Use cmd.exe /C to run the command — this handles .cmd/.bat scripts
            // (npm-installed tools like openclaw.cmd, codex.cmd, opencode.cmd)
            let mut cmd = Command::new("cmd");
            cmd.args(["/C", &full_command]);
            cmd.current_dir(&home);

            // Set env vars directly on the Command — they inherit properly
            if let Some(ref key) = api_key_env {
                cmd.env("OPENAI_API_KEY", key);
            }
            if let Some(ref url) = base_url_env {
                cmd.env("OPENAI_BASE_URL", url);
            }

            // CREATE_NEW_CONSOLE: visible terminal window for TUI tools
            // CREATE_NEW_PROCESS_GROUP: independent process
            cmd.creation_flags(CREATE_NEW_PROCESS_GROUP | CREATE_NEW_CONSOLE);

            match cmd.spawn() {
                Ok(child) => {
                    let pid = child.id();
                    log::info!("[ProcessManager] Tool {} started with PID: {}", tool_id, pid);
                    self.processes.insert(tool_id.to_string(), ProcessInfo { pid });
                    Ok(())
                }
                Err(e) => Err(format!("Spawn error: {}", e)),
            }
        }


        #[cfg(not(windows))]
        {
            let parts: Vec<&str> = full_command.split_whitespace().collect();
            if parts.is_empty() {
                return Err("Empty command".to_string());
            }

            let mut cmd = Command::new(parts[0]);
            if parts.len() > 1 {
                cmd.args(&parts[1..]);
            }
            cmd.current_dir(&home);
            if let Some(ref key) = api_key_env {
                cmd.env("OPENAI_API_KEY", key);
            }
            if let Some(ref url) = base_url_env {
                cmd.env("OPENAI_BASE_URL", url);
            }

            let child = cmd.spawn()
                .map_err(|e| format!("Spawn error: {}", e))?;

            let pid = child.id();
            log::info!("[ProcessManager] Tool {} started with PID: {}", tool_id, pid);
            self.processes.insert(tool_id.to_string(), ProcessInfo { pid });
            Ok(())
        }
    }

    /// Launch VS Code for extension-based tools (Cline, Roo Code, Continue, etc.)
    async fn launch_vscode(&mut self, tool_id: &str) -> Result<(), String> {
        log::info!("[ProcessManager] Launching VS Code for extension tool: {}", tool_id);

        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NEW_PROCESS_GROUP: u32 = 0x00000200;
            const CREATE_NO_WINDOW: u32 = 0x08000000;

            // On Windows, `code` is actually `code.cmd` in PATH
            let output = Command::new("cmd")
                .args(["/c", "code"])
                .creation_flags(CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW)
                .spawn()
                .map_err(|e| format!("Failed to launch VS Code: {}. Is VS Code installed and in PATH?", e))?;

            let pid = output.id();
            log::info!("[ProcessManager] VS Code launched for {} with PID: {}", tool_id, pid);
            self.processes.insert(tool_id.to_string(), ProcessInfo { pid });
            return Ok(());
        }

        #[cfg(target_os = "macos")]
        {
            let child = Command::new("open")
                .args(["-a", "Visual Studio Code"])
                .spawn()
                .map_err(|e| format!("Failed to launch VS Code: {}. Is VS Code installed?", e))?;

            let pid = child.id();
            log::info!("[ProcessManager] VS Code launched for {} with PID: {}", tool_id, pid);
            self.processes.insert(tool_id.to_string(), ProcessInfo { pid });
            return Ok(());
        }

        #[cfg(target_os = "linux")]
        {
            let child = Command::new("code")
                .spawn()
                .map_err(|e| format!("Failed to launch VS Code: {}. Is VS Code installed and in PATH?", e))?;

            let pid = child.id();
            log::info!("[ProcessManager] VS Code launched for {} with PID: {}", tool_id, pid);
            self.processes.insert(tool_id.to_string(), ProcessInfo { pid });
            return Ok(());
        }

        #[cfg(target_os = "android")]
        {
            let _ = tool_id;
            return Err("Not available on mobile".to_string());
        }
    }

    /// Start a GUI tool by opening its executable
    async fn start_gui_tool(&mut self, tool_id: &str) -> Result<(), String> {
        // Look up the executable path from tool definitions
        let exe_path = crate::services::tool_manager::get_tool_exe_path(tool_id)
            .ok_or_else(|| format!("No executable path found for tool '{}'", tool_id))?;

        log::info!("[ProcessManager] Starting GUI tool: {} at {}", tool_id, exe_path);

        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NEW_PROCESS_GROUP: u32 = 0x00000200;
            const CREATE_NO_WINDOW: u32 = 0x08000000;

            let ps_cmd = format!(
                "$process = Start-Process '{}' -PassThru; Write-Output $process.Id",
                exe_path
            );

            let output = Command::new("powershell")
                .args(["-Command", &ps_cmd])
                .creation_flags(CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW)
                .output()
                .map_err(|e| format!("PowerShell error: {}", e))?;

            let pid_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if let Ok(pid) = pid_str.parse::<u32>() {
                log::info!("[ProcessManager] GUI tool {} started with PID: {}", tool_id, pid);
                self.processes.insert(tool_id.to_string(), ProcessInfo { pid });
                return Ok(());
            }
            Err(format!("Failed to launch GUI tool: {}", pid_str))
        }

        #[cfg(target_os = "macos")]
        {
            let child = Command::new("open")
                .arg(&exe_path)
                .spawn()
                .map_err(|e| format!("open error: {}", e))?;

            let pid = child.id();
            log::info!("[ProcessManager] GUI tool {} started with PID: {}", tool_id, pid);
            self.processes.insert(tool_id.to_string(), ProcessInfo { pid });
            Ok(())
        }

        #[cfg(target_os = "linux")]
        {
            let child = Command::new(&exe_path)
                .spawn()
                .map_err(|e| format!("Spawn error: {}", e))?;

            let pid = child.id();
            log::info!("[ProcessManager] GUI tool {} started with PID: {}", tool_id, pid);
            self.processes.insert(tool_id.to_string(), ProcessInfo { pid });
            Ok(())
        }

        #[cfg(target_os = "android")]
        {
            let _ = (tool_id, exe_path);
            Err("Not available on mobile".to_string())
        }
    }

    /// Ensure Claude Code onboarding is marked as completed in ~/.claude.json
    /// and settings.json has allowedTools for non-interactive use
    fn ensure_claude_onboarding() {
        // ~/.claude.json: skip onboarding (only if missing)
        let claude_json = dirs::home_dir().unwrap_or_default().join(".claude.json");
        if !claude_json.exists() {
            let config = serde_json::json!({ "hasCompletedOnboarding": true });
            if let Ok(content) = serde_json::to_string_pretty(&config) {
                let _ = std::fs::write(&claude_json, content);
                log::info!("[ProcessManager] Created {:?} (onboarding skip)", claude_json);
            }
        }

        // ~/.claude/settings.json: ensure allowedTools exist (only if missing)
        let claude_dir = dirs::home_dir().unwrap_or_default().join(".claude");
        let _ = std::fs::create_dir_all(&claude_dir);
        let settings_path = claude_dir.join("settings.json");
        if !settings_path.exists() {
            let settings = serde_json::json!({
                "allowedTools": ["Edit","Write","Bash","Read","MultiEdit","Glob","Grep","LS","TodoRead","TodoWrite","WebFetch","NotebookRead","NotebookEdit"]
            });
            if let Ok(content) = serde_json::to_string_pretty(&settings) {
                let _ = std::fs::write(&settings_path, content);
                log::info!("[ProcessManager] Created {:?} (allowedTools)", settings_path);
            }
        }
    }

    /// Stop a running tool by PID
    pub async fn stop_tool(&mut self, tool_id: &str) -> Result<(), String> {
        let info = self.processes.remove(tool_id)
            .ok_or_else(|| "Tool is not running".to_string())?;

        log::info!("[ProcessManager] Stopping tool: {} (PID: {})", tool_id, info.pid);

        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x08000000;
            // Windows: taskkill /T /F to kill process tree
            let output = Command::new("taskkill")
                .args(["/pid", &info.pid.to_string(), "/T", "/F"])
                .creation_flags(CREATE_NO_WINDOW)
                .output()
                .map_err(|e| format!("taskkill error: {}", e))?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                if !stderr.contains("not found") {
                    log::error!("[ProcessManager] taskkill stderr: {}", stderr);
                }
            }
        }

        #[cfg(not(windows))]
        {
            // Unix: SIGKILL
            unsafe {
                libc::kill(info.pid as i32, libc::SIGKILL);
            }
        }

        Ok(())
    }

    /// Get list of running tool IDs
    pub fn get_running_tools(&self) -> Vec<String> {
        self.processes.keys().cloned().collect()
    }

    /// Check if a tool is running
    pub fn is_tool_running(&self, tool_id: &str) -> bool {
        self.processes.contains_key(tool_id)
    }

    /// Stop all tools (called on app quit)
    pub fn stop_all(&mut self) {
        log::info!("[ProcessManager] Stopping all tools...");
        let tool_ids: Vec<String> = self.processes.keys().cloned().collect();

        for tool_id in &tool_ids {
            if let Some(info) = self.processes.remove(tool_id) {
                #[cfg(windows)]
                {
                    use std::os::windows::process::CommandExt;
                    const CREATE_NO_WINDOW: u32 = 0x08000000;
                    let _ = Command::new("taskkill")
                        .args(["/pid", &info.pid.to_string(), "/T", "/F"])
                        .creation_flags(CREATE_NO_WINDOW)
                        .output();
                }
                #[cfg(not(windows))]
                {
                    unsafe { libc::kill(info.pid as i32, libc::SIGKILL); }
                }
            }
        }
    }

    /// Monitor running processes (check if PIDs still exist)
    #[cfg(windows)]
    pub async fn check_processes(&mut self) -> Vec<String> {
        let mut exited = Vec::new();

        for (tool_id, info) in &self.processes {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x08000000;
            let output = Command::new("tasklist")
                .args(["/FI", &format!("PID eq {}", info.pid), "/FO", "CSV", "/NH"])
                .creation_flags(CREATE_NO_WINDOW)
                .output();

            match output {
                Ok(out) => {
                    let stdout = String::from_utf8_lossy(&out.stdout);
                    if stdout.contains("No tasks") || stdout.trim().is_empty() {
                        log::info!("[ProcessManager] Tool {} (PID: {}) exited externally", tool_id, info.pid);
                        exited.push(tool_id.clone());
                    }
                }
                Err(_) => {
                    exited.push(tool_id.clone());
                }
            }
        }

        for id in &exited {
            self.processes.remove(id);
        }

        exited
    }

    #[cfg(not(windows))]
    pub async fn check_processes(&mut self) -> Vec<String> {
        let mut exited = Vec::new();

        for (tool_id, info) in &self.processes {
            let alive = unsafe { libc::kill(info.pid as i32, 0) == 0 };
            if !alive {
                log::info!("[ProcessManager] Tool {} (PID: {}) exited externally", tool_id, info.pid);
                exited.push(tool_id.clone());
            }
        }

        for id in &exited {
            self.processes.remove(id);
        }

        exited
    }
}

// ─── Global singleton ───

use tokio::sync::OnceCell;

static PROCESS_MANAGER: OnceCell<Arc<Mutex<ProcessManager>>> = OnceCell::const_new();

async fn get_manager() -> Arc<Mutex<ProcessManager>> {
    PROCESS_MANAGER
        .get_or_init(|| async { Arc::new(Mutex::new(ProcessManager::new())) })
        .await
        .clone()
}

/// Start a tool process
pub async fn start_tool(tool_id: &str, start_command: Option<&str>) -> Result<(), String> {
    let mgr = get_manager().await;
    let mut mgr = mgr.lock().await;
    mgr.start_tool(tool_id, start_command).await
}

/// Stop a tool process
pub async fn stop_tool(tool_id: &str) -> Result<(), String> {
    let mgr = get_manager().await;
    let mut mgr = mgr.lock().await;
    mgr.stop_tool(tool_id).await
}

/// Get running tool IDs
pub async fn get_running_tools() -> Vec<String> {
    let mgr = get_manager().await;
    let mgr = mgr.lock().await;
    mgr.get_running_tools()
}

/// Check if tool is running
pub async fn is_tool_running(tool_id: &str) -> bool {
    let mgr = get_manager().await;
    let mgr = mgr.lock().await;
    mgr.is_tool_running(tool_id)
}

/// Stop all tools
pub async fn stop_all_tools() {
    let mgr = get_manager().await;
    let mut mgr = mgr.lock().await;
    mgr.stop_all();
}
