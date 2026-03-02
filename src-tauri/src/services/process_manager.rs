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
    #[cfg(windows)]
    is_windows: bool,
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

    /// Start a CLI tool via terminal
    fn start_cli_tool(&mut self, tool_id: &str, command: &str) -> Result<(), String> {
        let home = dirs::home_dir().unwrap_or_default();
        let echobird_dir = dirs::home_dir().unwrap_or_default().join(".echobird");
        let config_path = echobird_dir.join(format!("{}.json", tool_id));


        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;

            // Windows creation flags
            const CREATE_NEW_PROCESS_GROUP: u32 = 0x00000200;
            const CREATE_NO_WINDOW: u32 = 0x08000000;

            // Parse command into parts
            let parts: Vec<&str> = command.split_whitespace().collect();
            if parts.is_empty() {
                return Err("Empty command".to_string());
            }

            let mut cmd = Command::new(parts[0]);
            if parts.len() > 1 {
                cmd.args(&parts[1..]);
            }
            cmd.current_dir(&home);

            // Set env vars directly on the Command
            if config_path.exists() {
                if let Ok(content) = std::fs::read_to_string(&config_path) {
                    if let Ok(config) = serde_json::from_str::<serde_json::Value>(&content) {
                        if let Some(api_key) = config.get("apiKey").and_then(|v| v.as_str()) {
                            cmd.env("OPENAI_API_KEY", api_key);
                        }
                        if let Some(base_url) = config.get("baseUrl").and_then(|v| v.as_str()) {
                            cmd.env("OPENAI_BASE_URL", base_url);
                        }
                    }
                }
            }

            // CREATE_NO_WINDOW: hide the console window
            // CREATE_NEW_PROCESS_GROUP: independent process group
            cmd.creation_flags(CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW);

            match cmd.spawn() {
                Ok(child) => {
                    let pid = child.id();
                    log::info!("[ProcessManager] Tool {} started with PID: {}", tool_id, pid);
                    self.processes.insert(tool_id.to_string(), ProcessInfo {
                        pid,
                        is_windows: true,
                    });
                    Ok(())
                }
                Err(e) => Err(format!("Spawn error: {}", e)),
            }
        }


        #[cfg(not(windows))]
        {
            let parts: Vec<&str> = command.split_whitespace().collect();
            if parts.is_empty() {
                return Err("Empty command".to_string());
            }

            let child = Command::new(parts[0])
                .args(&parts[1..])
                .current_dir(&home)
                .spawn()
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

            // On Windows, `code` is actually `code.cmd` in PATH
            let output = Command::new("cmd")
                .args(["/c", "code"])
                .creation_flags(CREATE_NEW_PROCESS_GROUP)
                .spawn()
                .map_err(|e| format!("Failed to launch VS Code: {}. Is VS Code installed and in PATH?", e))?;

            let pid = output.id();
            log::info!("[ProcessManager] VS Code launched for {} with PID: {}", tool_id, pid);
            self.processes.insert(tool_id.to_string(), ProcessInfo {
                pid,
                is_windows: true,
            });
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

            let ps_cmd = format!(
                "$process = Start-Process '{}' -PassThru; Write-Output $process.Id",
                exe_path
            );

            let output = Command::new("powershell")
                .args(["-Command", &ps_cmd])
                .creation_flags(CREATE_NEW_PROCESS_GROUP)
                .output()
                .map_err(|e| format!("PowerShell error: {}", e))?;

            let pid_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if let Ok(pid) = pid_str.parse::<u32>() {
                log::info!("[ProcessManager] GUI tool {} started with PID: {}", tool_id, pid);
                self.processes.insert(tool_id.to_string(), ProcessInfo {
                    pid,
                    is_windows: true,
                });
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
    }

    /// Ensure Claude Code onboarding is marked as completed in ~/.claude.json
    fn ensure_claude_onboarding() {
        let claude_json = dirs::home_dir().unwrap_or_default().join(".claude.json");
        let mut config: serde_json::Value = if claude_json.exists() {
            std::fs::read_to_string(&claude_json)
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_else(|| serde_json::json!({}))
        } else {
            serde_json::json!({})
        };

        if config.get("hasCompletedOnboarding").and_then(|v| v.as_bool()) != Some(true) {
            config["hasCompletedOnboarding"] = serde_json::json!(true);
            if let Ok(content) = serde_json::to_string_pretty(&config) {
                let _ = std::fs::write(&claude_json, content);
                log::info!("[ProcessManager] Set hasCompletedOnboarding=true in {:?}", claude_json);
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
            // Windows: taskkill /T /F to kill process tree
            let output = Command::new("taskkill")
                .args(["/pid", &info.pid.to_string(), "/T", "/F"])
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
                    let _ = Command::new("taskkill")
                        .args(["/pid", &info.pid.to_string(), "/T", "/F"])
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
            let output = Command::new("tasklist")
                .args(["/FI", &format!("PID eq {}", info.pid), "/FO", "CSV", "/NH"])
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
