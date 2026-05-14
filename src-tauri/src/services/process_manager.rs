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
        Self {
            tools: HashMap::new(),
        }
    }

    fn is_cooling(&self, tool_id: &str) -> bool {
        if let Some(ts) = self.tools.get(tool_id) {
            ts.elapsed() < std::time::Duration::from_secs(3)
        } else {
            false
        }
    }

    fn mark(&mut self, tool_id: &str) {
        self.tools
            .insert(tool_id.to_string(), tokio::time::Instant::now());
    }
}

/// Global process manager state
pub struct ProcessManager {
    processes: HashMap<String, ProcessInfo>,
    cooldown: CooldownSet,
}

impl Default for ProcessManager {
    fn default() -> Self {
        Self::new()
    }
}

impl ProcessManager {
    pub fn new() -> Self {
        Self {
            processes: HashMap::new(),
            cooldown: CooldownSet::new(),
        }
    }

    /// Start a tool process �?mirrors old Electron processManager.startTool logic
    pub async fn start_tool(
        &mut self,
        tool_id: &str,
        start_command: Option<&str>,
    ) -> Result<(), String> {
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

        // Priority 0: Codex launcher (dual-spoof proxy + TTY preservation).
        //
        // CLI always goes through the launcher: even without a proxy, it
        // resolves the npm-bundled Rust binary directly so the TUI gets
        // a real TTY (the codex.cmd → node codex.js → codex.exe path on
        // Windows drops stdin-is-a-terminal somewhere in cmd /d /s /c).
        //
        // Desktop only goes through the launcher when a third-party
        // (non-OpenAI) relay is configured — that's the only case where
        // the proxy is actually needed. Skipping the launcher otherwise
        // preserves Desktop's normal launchUri path (Priority 2.9), which
        // is the *only* way to start a Microsoft Store install of Codex
        // Desktop; direct-exe spawn would fail with "not found" because
        // Store packages live under \\WindowsApps\... not \\Programs\\.
        let needs_launcher = match tool_id {
            "codex" => true,
            "codexdesktop" => Self::codex_has_third_party_relay(),
            _ => false,
        };
        if needs_launcher {
            match Self::find_codex_launcher() {
                Some(launcher) => {
                    log::info!(
                        "[ProcessManager] Routing {} through dual-spoof launcher: {:?}",
                        tool_id,
                        launcher
                    );
                    return self.start_codex_launcher(tool_id, &launcher);
                }
                None => {
                    // Log the technical detail for diagnostics, show a
                    // user-friendly message in the UI without leaking the
                    // bundled file path (which users can't act on anyway).
                    log::error!(
                        "[ProcessManager] {} requires launcher but codex-launcher.cjs is missing from the bundled tools directory",
                        tool_id
                    );
                    return Err(format!(
                        "{} cannot start because a required EchoBird component is missing. \
                         Try reinstalling EchoBird from the latest release.",
                        if tool_id == "codexdesktop" {
                            "Codex Desktop"
                        } else {
                            "Codex CLI"
                        }
                    ));
                }
            }
        }

        // Priority 1: If explicit command is given from frontend, use it
        if let Some(cmd) = start_command {
            log::info!(
                "[ProcessManager] Starting tool: {} with explicit command: {}",
                tool_id,
                cmd
            );
            return self.start_cli_tool(tool_id, cmd);
        }

        // Priority 2: CLI tools with startCommand in paths.json (e.g. "openclaw gateway")
        if let Some(command) = crate::services::tool_manager::get_tool_start_command(tool_id) {
            // Extract base command (first word) for existence check
            let base_cmd = command.split_whitespace().next().unwrap_or(&command);
            if crate::utils::platform::command_exists(base_cmd).await {
                log::info!(
                    "[ProcessManager] Starting CLI tool: {} with startCommand: {}",
                    tool_id,
                    command
                );
                return self.start_cli_tool(tool_id, &command);
            } else {
                log::warn!("[ProcessManager] startCommand base '{}' for tool '{}' not found in PATH, skipping", base_cmd, tool_id);
            }
        }

        // Priority 2.9: MSIX / Store app — use shell:AppsFolder\<AUMID>
        if let Some(uri) = crate::services::tool_manager::get_tool_launch_uri(tool_id) {
            log::info!(
                "[ProcessManager] Launching MSIX/Store app for {}: {}",
                tool_id,
                uri
            );
            return self.start_shell_uri(tool_id, &uri);
        }

        // Priority 3: GUI executable found (for desktop apps like CodeBuddy)
        if crate::services::tool_manager::get_tool_exe_path(tool_id).is_some() {
            log::info!(
                "[ProcessManager] Found GUI exe for {}, launching as desktop app",
                tool_id
            );
            return self.start_gui_tool(tool_id).await;
        }

        // Priority 4: VS Code extension tools — launch VS Code
        if crate::services::tool_manager::is_vscode_extension(tool_id) {
            log::info!(
                "[ProcessManager] Tool {} is a VS Code extension, launching VS Code",
                tool_id
            );
            return self.launch_vscode(tool_id).await;
        }

        // Priority 5: Fall back to CLI command from paths.json "command" field
        if let Some(command) = crate::services::tool_manager::get_tool_command(tool_id) {
            if crate::utils::platform::command_exists(&command).await {
                log::info!(
                    "[ProcessManager] Falling back to CLI command for {}: {}",
                    tool_id,
                    command
                );
                return self.start_cli_tool(tool_id, &command);
            } else {
                log::warn!(
                    "[ProcessManager] CLI command '{}' for tool '{}' not found in PATH",
                    command,
                    tool_id
                );
            }
        }

        Err(format!("No executable or command found for tool '{}'. The tool may be installed but not in PATH.", tool_id))
    }

    /// Locate the bundled codex-launcher.cjs under tools/codex/.
    fn find_codex_launcher() -> Option<std::path::PathBuf> {
        let launcher = crate::services::tool_manager::find_tools_dir()?
            .join("codex")
            .join("codex-launcher.cjs");
        if launcher.exists() {
            Some(launcher)
        } else {
            None
        }
    }

    /// True iff ~/.echobird/codex.json points at a non-OpenAI endpoint.
    /// Used to decide whether Codex Desktop needs to route through the
    /// dual-spoof launcher (third-party endpoints only) or can take the
    /// normal launchUri / GUI-exe path.
    fn codex_has_third_party_relay() -> bool {
        let relay_path = match dirs::home_dir() {
            Some(h) => h.join(".echobird").join("codex.json"),
            None => {
                log::warn!("[codex_has_third_party_relay] No home directory found");
                return false;
            }
        };

        log::info!(
            "[codex_has_third_party_relay] Checking relay config at: {:?}",
            relay_path
        );

        if !relay_path.exists() {
            log::warn!("[codex_has_third_party_relay] Relay config file does not exist");
            return false;
        }

        let content = match std::fs::read_to_string(&relay_path) {
            Ok(c) => c,
            Err(e) => {
                log::error!(
                    "[codex_has_third_party_relay] Failed to read relay config: {}",
                    e
                );
                return false;
            }
        };

        let cfg: serde_json::Value = match serde_json::from_str(&content) {
            Ok(v) => v,
            Err(e) => {
                log::error!(
                    "[codex_has_third_party_relay] Failed to parse relay config JSON: {}",
                    e
                );
                return false;
            }
        };

        let base_url = cfg.get("baseUrl").and_then(|v| v.as_str()).unwrap_or("");
        let is_third_party = !base_url.is_empty() && !base_url.contains("api.openai.com");

        log::info!(
            "[codex_has_third_party_relay] baseUrl='{}', is_third_party={}",
            base_url,
            is_third_party
        );

        is_third_party
    }

    /// Start Codex (CLI or Desktop) via the dual-spoof launcher.
    ///
    /// We invoke `node codex-launcher.cjs` rather than calling node with the
    /// path glued into a single argv string, so cmd.exe's quoting doesn't
    /// mangle paths containing spaces. The launcher takes care of spawning
    /// the actual Codex binary (npm Rust CLI vs. standalone Desktop exe)
    /// based on the ECHOBIRD_CODEX_LAUNCH_MODE env var we set here.
    fn start_codex_launcher(
        &mut self,
        tool_id: &str,
        launcher: &std::path::Path,
    ) -> Result<(), String> {
        let home = dirs::home_dir().unwrap_or_default();

        // Pull api_key / base_url out of ~/.echobird/codex.json so we can
        // pre-seed env vars. The launcher itself also reads this file, so
        // the env pre-seed is mainly a backstop for the OpenAI-direct case
        // where the launcher skips the proxy.
        let relay_path = home.join(".echobird").join("codex.json");
        let mut api_key: Option<String> = None;
        let mut base_url: Option<String> = None;
        let mut env_key: Option<String> = None;
        if relay_path.exists() {
            if let Ok(content) = std::fs::read_to_string(&relay_path) {
                if let Ok(cfg) = serde_json::from_str::<serde_json::Value>(&content) {
                    api_key = cfg.get("apiKey").and_then(|v| v.as_str()).map(String::from);
                    base_url = cfg
                        .get("baseUrl")
                        .and_then(|v| v.as_str())
                        .map(String::from);
                    env_key = cfg.get("envKey").and_then(|v| v.as_str()).map(String::from);
                }
            }
        }

        // Tell the launcher which Codex binary to spawn. CLI mode picks up
        // the npm-bundled Rust binary via resolveCodexBinary(); desktop
        // mode looks at tools/codexdesktop/paths.json's exe locations.
        let launch_mode = if tool_id == "codexdesktop" {
            "desktop"
        } else {
            "cli"
        };

        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NEW_PROCESS_GROUP: u32 = 0x00000200;
            const CREATE_NEW_CONSOLE: u32 = 0x00000010;
            const CREATE_NO_WINDOW: u32 = 0x08000000;

            // Strip the Windows extended-length path prefix "\\?\" if
            // present — cmd.exe rejects it, even though Rust hands them
            // out in debug builds.
            let launcher_str = launcher.to_string_lossy();
            let launcher_clean = launcher_str.strip_prefix(r"\\?\").unwrap_or(&launcher_str);

            let mut cmd = Command::new("cmd");
            cmd.args(["/C", "node", launcher_clean]);
            cmd.current_dir(&home);
            cmd.env("ECHOBIRD_CODEX_LAUNCH_MODE", launch_mode);
            // Suppress launcher console output in CLI mode (logs still go to file)
            cmd.env("ECHOBIRD_LAUNCHER_QUIET", "1");
            if let Some(ref key) = api_key {
                cmd.env("OPENAI_API_KEY", key);
                if let Some(ref ek) = env_key {
                    cmd.env(ek, key);
                }
            }
            if let Some(ref url) = base_url {
                cmd.env("OPENAI_BASE_URL", url);
            }

            // CLI needs a visible terminal — Codex CLI's TUI renders in
            // it via stdio:inherit. Desktop is a GUI app, so hide the
            // launcher console entirely (otherwise users see a black
            // cmd window flash open with bootstrap logs in it).
            let flags = if launch_mode == "desktop" {
                CREATE_NO_WINDOW
            } else {
                CREATE_NEW_PROCESS_GROUP | CREATE_NEW_CONSOLE
            };
            cmd.creation_flags(flags);

            log::info!(
                "[ProcessManager] Codex launcher ({}): cmd /C node {}",
                launch_mode,
                launcher_clean
            );
            match cmd.spawn() {
                Ok(child) => {
                    let pid = child.id();
                    log::info!("[ProcessManager] Codex launcher PID: {}", pid);
                    self.processes
                        .insert(tool_id.to_string(), ProcessInfo { pid });
                    Ok(())
                }
                Err(e) => Err(format!("Failed to launch Codex launcher: {}", e)),
            }
        }

        #[cfg(not(windows))]
        {
            let mut cmd = Command::new("node");
            cmd.arg(launcher);
            cmd.current_dir(&home);
            cmd.env("ECHOBIRD_CODEX_LAUNCH_MODE", launch_mode);
            // Suppress launcher console output in CLI mode (logs still go to file)
            cmd.env("ECHOBIRD_LAUNCHER_QUIET", "1");
            if let Some(ref key) = api_key {
                cmd.env("OPENAI_API_KEY", key);
                if let Some(ref ek) = env_key {
                    cmd.env(ek, key);
                }
            }
            if let Some(ref url) = base_url {
                cmd.env("OPENAI_BASE_URL", url);
            }

            match cmd.spawn() {
                Ok(child) => {
                    let pid = child.id();
                    log::info!(
                        "[ProcessManager] Codex launcher PID ({}): {}",
                        launch_mode,
                        pid
                    );
                    self.processes
                        .insert(tool_id.to_string(), ProcessInfo { pid });
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
        let mut custom_api_key_env_name: Option<String> = None;
        if config_path.exists() {
            if let Ok(content) = std::fs::read_to_string(&config_path) {
                if let Ok(config) = serde_json::from_str::<serde_json::Value>(&content) {
                    api_key_env = config
                        .get("apiKey")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    base_url_env = config
                        .get("baseUrl")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    model_id = config
                        .get("modelId")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    custom_api_key_env_name = config
                        .get("envKey")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
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
            // (npm-installed tools like openclaw.cmd, codex.cmd, opencode.cmd).
            // Resolve cmd.exe via %COMSPEC% / %SystemRoot%\System32 instead of
            // relying on PATH — some users have System32 stripped from PATH.
            let mut cmd = Command::new(resolve_cmd_exe());
            cmd.args(["/C", &full_command]);
            cmd.current_dir(&home);

            // Set env vars directly on the Command — they inherit properly
            if let Some(ref key) = api_key_env {
                cmd.env("OPENAI_API_KEY", key);
                if let Some(ref env_name) = custom_api_key_env_name {
                    cmd.env(env_name, key);
                }
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
                    log::info!(
                        "[ProcessManager] Tool {} started with PID: {}",
                        tool_id,
                        pid
                    );
                    self.processes
                        .insert(tool_id.to_string(), ProcessInfo { pid });
                    Ok(())
                }
                Err(e) => Err(format!("Spawn error: {}", e)),
            }
        }

        // Linux: TUI tools (claude/codex/opencode etc.) need a real TTY, so we
        // can't just spawn the binary as a child of the Tauri GUI. Find a
        // terminal emulator and run the command through `bash -lc` inside it,
        // mirroring what CREATE_NEW_CONSOLE gives us on Windows.
        #[cfg(target_os = "linux")]
        {
            let term = find_terminal_emulator().ok_or_else(|| {
                "No terminal emulator found. Please install one of: \
                                gnome-terminal, konsole, xfce4-terminal, alacritty, \
                                kitty, wezterm, foot, tilix, or xterm."
                    .to_string()
            })?;

            let mut cmd = Command::new(&term.binary);
            cmd.args(term.prefix_args.iter().copied());
            // Use the user's login shell in login+interactive mode. `-i` is
            // critical: it sources .bashrc / .zshrc, where npm/bun/cargo PATH
            // entries actually live. `bash -lc` alone misses .bashrc and
            // doesn't read .zshrc at all, which is why pi/hermes (installed
            // via npm) flashed "command not found" while system-PATH tools
            // like claude worked. Separate flags (not `-ilc`) for fish-shell
            // compatibility.
            let user_shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".into());
            let wrapped = wrap_with_pause_on_quick_or_error(&full_command);
            cmd.arg(&user_shell)
                .arg("-l")
                .arg("-i")
                .arg("-c")
                .arg(&wrapped);
            cmd.current_dir(&home);

            if let Some(ref key) = api_key_env {
                cmd.env("OPENAI_API_KEY", key);
                if let Some(ref env_name) = custom_api_key_env_name {
                    cmd.env(env_name, key);
                }
            }
            if let Some(ref url) = base_url_env {
                cmd.env("OPENAI_BASE_URL", url);
            }

            let child = cmd.spawn().map_err(|e| {
                format!(
                    "Failed to launch terminal '{}': {}",
                    term.binary.display(),
                    e
                )
            })?;

            let pid = child.id();
            log::info!(
                "[ProcessManager] Tool {} started in {} with PID: {}",
                tool_id,
                term.binary.display(),
                pid
            );
            self.processes
                .insert(tool_id.to_string(), ProcessInfo { pid });
            Ok(())
        }

        // macOS: same TTY problem as Linux. The fix is simpler though —
        // Terminal.app is system-bundled so no detection is needed. We bake
        // env exports into a cached shell script and hand it to `open -a
        // Terminal`. Env can't ride along on `open` itself because Terminal
        // launches a fresh login shell, so the script approach is the clean
        // way to propagate API keys.
        //
        // Caveat: `open` exits immediately after handing off to Terminal,
        // so the PID we capture belongs to `open`, not the running tool.
        // is_tool_running() will report the tool as exited shortly after
        // launch — acceptable for now since users close the Terminal tab
        // themselves anyway.
        #[cfg(target_os = "macos")]
        {
            use std::os::unix::fs::PermissionsExt;

            let cache_dir = dirs::cache_dir()
                .unwrap_or_else(|| home.join(".cache"))
                .join("echobird");
            std::fs::create_dir_all(&cache_dir)
                .map_err(|e| format!("Failed to create cache dir: {}", e))?;

            let script_path = cache_dir.join(format!("launch-{}.sh", tool_id));

            // Single-quote wrap; escape internal singles as '\''. Bullet-proof
            // for bash, and keeps API keys / URLs safe from shell metachars.
            fn shq(s: &str) -> String {
                format!("'{}'", s.replace('\'', "'\\''"))
            }

            let mut script = String::from("#!/bin/bash\n");
            if let Some(ref key) = api_key_env {
                script.push_str(&format!("export OPENAI_API_KEY={}\n", shq(key)));
                if let Some(ref env_name) = custom_api_key_env_name {
                    script.push_str(&format!("export {}={}\n", env_name, shq(key)));
                }
            }
            if let Some(ref url) = base_url_env {
                script.push_str(&format!("export OPENAI_BASE_URL={}\n", shq(url)));
            }
            script.push_str(&format!("cd {}\n", shq(&home.to_string_lossy())));
            // Re-exec into the user's actual login shell in interactive mode
            // so PATH entries from .zshrc / .bashrc (npm/bun bin dirs etc.)
            // are picked up — same reason as the Linux branch. The wrapped
            // command itself contains no single quotes, so single-quote
            // wrapping it is safe; if that ever changes, shq() handles
            // escaping. Pause-on-quick-exit logic is inside the wrapped cmd.
            script.push_str(&format!(
                "exec \"${{SHELL:-/bin/bash}}\" -l -i -c {}\n",
                shq(&wrap_with_pause_on_quick_or_error(&full_command))
            ));

            std::fs::write(&script_path, &script)
                .map_err(|e| format!("Failed to write launch script: {}", e))?;

            let mut perms = std::fs::metadata(&script_path)
                .map_err(|e| format!("metadata: {}", e))?
                .permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&script_path, perms).map_err(|e| format!("chmod: {}", e))?;

            let child = Command::new("open")
                .args(["-a", "Terminal"])
                .arg(&script_path)
                .spawn()
                .map_err(|e| format!("Failed to launch Terminal.app: {}", e))?;

            let pid = child.id();
            log::info!(
                "[ProcessManager] Tool {} launched in Terminal.app via {}, open PID: {}",
                tool_id,
                script_path.display(),
                pid
            );
            self.processes
                .insert(tool_id.to_string(), ProcessInfo { pid });
            Ok(())
        }

        // Other unix (*BSD, etc.): keep the historical raw-spawn behavior as
        // a safety net. Realistically nothing hits this branch in production.
        #[cfg(all(unix, not(target_os = "linux"), not(target_os = "macos")))]
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
                if let Some(ref env_name) = custom_api_key_env_name {
                    cmd.env(env_name, key);
                }
            }
            if let Some(ref url) = base_url_env {
                cmd.env("OPENAI_BASE_URL", url);
            }

            let child = cmd.spawn().map_err(|e| format!("Spawn error: {}", e))?;

            let pid = child.id();
            log::info!(
                "[ProcessManager] Tool {} started with PID: {}",
                tool_id,
                pid
            );
            self.processes
                .insert(tool_id.to_string(), ProcessInfo { pid });
            Ok(())
        }
    }

    /// Launch VS Code for extension-based tools
    async fn launch_vscode(&mut self, tool_id: &str) -> Result<(), String> {
        log::info!(
            "[ProcessManager] Launching VS Code for extension tool: {}",
            tool_id
        );

        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NEW_PROCESS_GROUP: u32 = 0x00000200;
            const CREATE_NO_WINDOW: u32 = 0x08000000;

            // On Windows, `code` is actually `code.cmd` in PATH. Resolve
            // cmd.exe directly so we don't rely on PATH containing System32.
            let output = Command::new(resolve_cmd_exe())
                .args(["/c", "code"])
                .creation_flags(CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW)
                .spawn()
                .map_err(|e| {
                    format!(
                        "Failed to launch VS Code: {}. Is VS Code installed and in PATH?",
                        e
                    )
                })?;

            let pid = output.id();
            log::info!(
                "[ProcessManager] VS Code launched for {} with PID: {}",
                tool_id,
                pid
            );
            self.processes
                .insert(tool_id.to_string(), ProcessInfo { pid });
            Ok(())
        }

        #[cfg(target_os = "macos")]
        {
            let child = Command::new("open")
                .args(["-a", "Visual Studio Code"])
                .spawn()
                .map_err(|e| format!("Failed to launch VS Code: {}. Is VS Code installed?", e))?;

            let pid = child.id();
            log::info!(
                "[ProcessManager] VS Code launched for {} with PID: {}",
                tool_id,
                pid
            );
            self.processes
                .insert(tool_id.to_string(), ProcessInfo { pid });
            Ok(())
        }

        #[cfg(target_os = "linux")]
        {
            let child = Command::new("code").spawn().map_err(|e| {
                format!(
                    "Failed to launch VS Code: {}. Is VS Code installed and in PATH?",
                    e
                )
            })?;

            let pid = child.id();
            log::info!(
                "[ProcessManager] VS Code launched for {} with PID: {}",
                tool_id,
                pid
            );
            self.processes
                .insert(tool_id.to_string(), ProcessInfo { pid });
            Ok(())
        }

        #[cfg(target_os = "android")]
        {
            let _ = tool_id;
            return Err("Not available on mobile".to_string());
        }
    }

    /// Start a GUI tool by opening its executable
    /// Launch an MSIX/Store app via shell:AppsFolder URI (Windows only).
    /// On non-Windows hosts there are no Store apps, so this is a no-op error.
    fn start_shell_uri(&mut self, tool_id: &str, uri: &str) -> Result<(), String> {
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x08000000;
            let result = Command::new("explorer.exe")
                .arg(uri)
                .creation_flags(CREATE_NO_WINDOW)
                .spawn();
            match result {
                Ok(child) => {
                    let pid = child.id();
                    log::info!(
                        "[ProcessManager] Launched {} via shell URI, PID: {}",
                        tool_id,
                        pid
                    );
                    self.processes
                        .insert(tool_id.to_string(), ProcessInfo { pid });
                    Ok(())
                }
                Err(e) => Err(format!("Failed to launch via shell URI: {}", e)),
            }
        }
        #[cfg(not(windows))]
        {
            let _ = (tool_id, uri);
            Err("Shell-URI launch is Windows-only".to_string())
        }
    }

    async fn start_gui_tool(&mut self, tool_id: &str) -> Result<(), String> {
        // Look up the executable path from tool definitions
        let exe_path = crate::services::tool_manager::get_tool_exe_path(tool_id)
            .ok_or_else(|| format!("No executable path found for tool '{}'", tool_id))?;

        log::info!(
            "[ProcessManager] Starting GUI tool: {} at {}",
            tool_id,
            exe_path
        );

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
                log::info!(
                    "[ProcessManager] GUI tool {} started with PID: {}",
                    tool_id,
                    pid
                );
                self.processes
                    .insert(tool_id.to_string(), ProcessInfo { pid });
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
            log::info!(
                "[ProcessManager] GUI tool {} started with PID: {}",
                tool_id,
                pid
            );
            self.processes
                .insert(tool_id.to_string(), ProcessInfo { pid });
            Ok(())
        }

        #[cfg(target_os = "linux")]
        {
            let child = Command::new(&exe_path)
                .spawn()
                .map_err(|e| format!("Spawn error: {}", e))?;

            let pid = child.id();
            log::info!(
                "[ProcessManager] GUI tool {} started with PID: {}",
                tool_id,
                pid
            );
            self.processes
                .insert(tool_id.to_string(), ProcessInfo { pid });
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
                log::info!(
                    "[ProcessManager] Created {:?} (onboarding skip)",
                    claude_json
                );
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
                log::info!(
                    "[ProcessManager] Created {:?} (allowedTools)",
                    settings_path
                );
            }
        }
    }

    /// Stop a running tool by PID
    pub async fn stop_tool(&mut self, tool_id: &str) -> Result<(), String> {
        let info = self
            .processes
            .remove(tool_id)
            .ok_or_else(|| "Tool is not running".to_string())?;

        log::info!(
            "[ProcessManager] Stopping tool: {} (PID: {})",
            tool_id,
            info.pid
        );

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
                    unsafe {
                        libc::kill(info.pid as i32, libc::SIGKILL);
                    }
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
                        log::info!(
                            "[ProcessManager] Tool {} (PID: {}) exited externally",
                            tool_id,
                            info.pid
                        );
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
                log::info!(
                    "[ProcessManager] Tool {} (PID: {}) exited externally",
                    tool_id,
                    info.pid
                );
                exited.push(tool_id.clone());
            }
        }

        for id in &exited {
            self.processes.remove(id);
        }

        exited
    }
}

// ─── Platform helpers ───

// Resolve cmd.exe via %COMSPEC% / %SystemRoot%\System32 instead of trusting
// PATH. Some environments (AV-cleaned, custom-policy, or tampered user PATH)
// drop System32, which makes bare `Command::new("cmd")` fail with "file not
// found". WezTerm / VS Code / Hyper all use the same fallback chain.
#[cfg(windows)]
fn resolve_cmd_exe() -> std::path::PathBuf {
    if let Ok(comspec) = std::env::var("COMSPEC") {
        let p = std::path::PathBuf::from(&comspec);
        if p.exists() {
            return p;
        }
    }
    if let Ok(sysroot) = std::env::var("SystemRoot") {
        let p = std::path::PathBuf::from(sysroot)
            .join("System32")
            .join("cmd.exe");
        if p.exists() {
            return p;
        }
    }
    std::path::PathBuf::from("cmd")
}

// Wrap a user command so the terminal stays open when the tool exits quickly
// or with a non-zero status. The heuristic catches two real failure modes:
//   - tool not found / immediate error  -> non-zero exit, user needs to read it
//   - tool ran < 2s and exited 0        -> probably printed `--help` and quit;
//                                          user wants to see what it printed
// A long-running TUI (e.g. claude) that the user `/quit`s normally exits 0
// after >2s, so the terminal closes cleanly — no extra Enter press needed.
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn wrap_with_pause_on_quick_or_error(cmd: &str) -> String {
    let mut s = String::new();
    s.push_str("start_ts=$(date +%s); ");
    s.push_str(cmd);
    s.push_str("; ec=$?; end_ts=$(date +%s); elapsed=$((end_ts - start_ts)); ");
    s.push_str("if [ $ec -ne 0 ] || [ $elapsed -lt 2 ]; then ");
    s.push_str("echo; echo \"[exit=$ec, ran ${elapsed}s -- press Enter to close]\"; ");
    s.push_str("read -r _; ");
    s.push_str("fi");
    s
}

#[cfg(target_os = "linux")]
struct TerminalLauncher {
    binary: std::path::PathBuf,
    /// Args that go between the terminal binary and the user command, e.g.
    /// `gnome-terminal --` or `xterm -e`. After these we always append
    /// `bash -lc <full_command>`.
    prefix_args: &'static [&'static str],
}

// Probe the user's system for a usable terminal emulator. Order matters:
// `x-terminal-emulator` is the Debian/Ubuntu meta-binary that already points
// at whatever the user picked, so it's the most user-respecting choice. The
// rest are fallbacks ordered roughly by ubiquity.
#[cfg(target_os = "linux")]
fn find_terminal_emulator() -> Option<TerminalLauncher> {
    const CANDIDATES: &[(&str, &[&str])] = &[
        ("x-terminal-emulator", &["-e"]),
        ("gnome-terminal", &["--"]),
        ("konsole", &["-e"]),
        // xfce4-terminal/tilix accept argv after `-x`; `-e` wants a single string.
        ("xfce4-terminal", &["-x"]),
        ("tilix", &["-x"]),
        ("alacritty", &["-e"]),
        ("kitty", &[]),
        ("wezterm", &["start", "--"]),
        ("foot", &[]),
        ("xterm", &["-e"]),
    ];

    for &(name, prefix_args) in CANDIDATES {
        if let Ok(path) = which::which(name) {
            return Some(TerminalLauncher {
                binary: path,
                prefix_args,
            });
        }
    }
    None
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
