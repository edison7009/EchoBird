// Process Manager �?mirrors old processManager.ts
// Manages tool processes: start/stop CLI & GUI tools, monitor PIDs

use std::collections::HashMap;
use std::process::Command;
use std::sync::Arc;
use tokio::sync::Mutex;

pub(crate) struct QoderCampaignIdentity {
    pub machine_token: String,
    pub machine_type: String,
    pub machine_code: String,
    pub version: Option<String>,
}

fn qoder_identity_value(value: &serde_json::Value, key: &str) -> Result<String, String> {
    let value = value[key]
        .as_str()
        .map(str::trim)
        .ok_or("accountError.format|Qoder device identity is invalid")?;
    if value.is_empty() || value.len() > 4096 || !value.bytes().all(|b| (32..=126).contains(&b)) {
        return Err("accountError.format|Qoder device identity is invalid".into());
    }
    Ok(value.into())
}

fn parse_qoder_identity(bytes: &[u8]) -> Result<QoderCampaignIdentity, String> {
    let value: serde_json::Value = serde_json::from_slice(bytes)
        .map_err(|_| "accountError.format|Qoder device identity output is invalid")?;
    Ok(QoderCampaignIdentity {
        machine_token: qoder_identity_value(&value, "machineToken")?,
        machine_type: qoder_identity_value(&value, "machineType")?,
        machine_code: qoder_identity_value(&value, "machineCode")?,
        version: None,
    })
}

fn qoder_identity_arguments(platform: &str, tool_id: &str) -> Result<Vec<String>, String> {
    let environment = match tool_id {
        "qoder" => "3",
        "qodercn" => "0",
        _ => return Err("accountError.invalidAccount".into()),
    };
    let mut args = vec![environment.into()];
    if matches!(platform, "windows" | "macos") {
        args.push("--account-stdin".into());
    }
    Ok(args)
}

async fn read_qoder_identity(
    mut command: tokio::process::Command,
    account: Option<&str>,
) -> Result<QoderCampaignIdentity, String> {
    use std::process::Stdio;
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
    command
        .kill_on_drop(true)
        .stdin(if account.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = command
        .spawn()
        .map_err(|_| "accountError.failed|Qoder device identity helper could not start")?;
    let operation = async {
        if let Some(account) = account {
            let body = format!("{}\n", serde_json::json!({"account":account}));
            let mut stdin = child.stdin.take().ok_or("accountError.failed")?;
            stdin
                .write_all(body.as_bytes())
                .await
                .map_err(|_| "accountError.failed")?;
            stdin.shutdown().await.map_err(|_| "accountError.failed")?;
        }
        let stdout = child.stdout.take().ok_or("accountError.failed")?;
        let mut stdout = BufReader::new(stdout).take(1_048_577);
        let mut line = Vec::new();
        stdout
            .read_until(b'\n', &mut line)
            .await
            .map_err(|_| "accountError.failed")?;
        if line.len() > 1_048_576 {
            return Err("accountError.format|Qoder device identity output is too large".into());
        }
        parse_qoder_identity(&line)
    };
    tokio::time::timeout(std::time::Duration::from_secs(25), operation)
        .await
        .map_err(|_| "accountError.network|Qoder device identity helper timed out".to_string())?
}

pub(crate) async fn qoder_campaign_identity(
    tool_id: &str,
    account_uid: &str,
) -> Result<QoderCampaignIdentity, String> {
    if account_uid.trim().is_empty() || account_uid.len() > 512 {
        return Err("accountError.invalidAccount".into());
    }
    let runtime = super::tool_manager::get_qoder_campaign_runtime_path(tool_id)?;
    let platform = std::env::consts::OS;
    let mut command = crate::utils::process::async_command(&runtime.path);
    command.args(qoder_identity_arguments(platform, tool_id)?);
    let mut identity = read_qoder_identity(
        command,
        matches!(platform, "windows" | "macos").then_some(account_uid),
    )
    .await?;
    identity.version = runtime.version;
    Ok(identity)
}

#[cfg(any(not(windows), test))]
fn executable_process_pattern(path: &str) -> String {
    let escaped: String = path
        .chars()
        .flat_map(|c| {
            if "\\.^$|?*+()[]{}".contains(c) {
                vec!['\\', c]
            } else {
                vec![c]
            }
        })
        .collect();
    format!("^{escaped}($| )")
}

/// Process info for a running tool
#[derive(Debug, Clone)]
struct ProcessInfo {
    pid: u32,
}

impl ProcessInfo {
    fn new(pid: u32) -> Self {
        Self { pid }
    }
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
        cwd: Option<&str>,
    ) -> Result<(), String> {
        self.launch_tool(tool_id, start_command, cwd, true).await
    }

    async fn launch_tool(
        &mut self,
        tool_id: &str,
        start_command: Option<&str>,
        cwd: Option<&str>,
        restart: bool,
    ) -> Result<(), String> {
        if self.cooldown.is_cooling(tool_id) {
            return Err("Please wait before launching again".to_string());
        }
        self.cooldown.mark(tool_id);

        // Claude Code: ensure onboarding is marked as completed
        if tool_id == "claudecode" {
            Self::ensure_claude_onboarding();
        }
        if tool_id == "minimaxdesktop"
            && crate::services::tool_manager::get_tool_exe_path(tool_id).is_none()
        {
            return Err("MiniMax Desktop is not installed".into());
        }

        // Desktop apps load provider config at startup, so switching the model
        // while the app is open silently fails. Restore "launch = kill +
        // restart": terminate any running instance (including one the USER
        // opened, matched by image name) first, so the spawn below is a fresh
        // process with the current config. CLI tools re-read config per
        // invocation and are never touched. (A lone multi-open request once
        // removed this; the silent-switch failures it caused hit far more
        // users — so kill is the default, no toggle.)
        // Claude Science keeps work in a daemon. Reopening its browser must
        // leave that daemon alone unless its model or login actually changed.
        if restart
            && crate::services::tool_manager::is_managed_desktop_tool(tool_id)
            && (tool_id != "claudescience"
                || crate::services::tool_config_manager::claude_science_restart_file().exists())
            && self.kill_desktop_instances(tool_id)
        {
            // Let the OS release the app's single-instance lock so the relaunch
            // starts a fresh process instead of focusing the dying instance.
            tokio::time::sleep(std::time::Duration::from_millis(800)).await;
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

        // Merge for both official and third-party launches, after desktop
        // shutdown and before choosing the native/Store/CLI launch route.
        if tool_id == "claudedesktop" && restart {
            crate::services::plugin_accounts::prepare_history("claudedesktop", false).await?;
        }
        if matches!(tool_id, "codex" | "chatgptdesktop") {
            if let Some(codex_dir) = crate::services::codex_runtime::default_codex_dir() {
                crate::services::codex_session_merge::merge_codex_history(&codex_dir);
            }
        }

        // Provider-specific preparation does not choose a separate launcher.
        if tool_id == "codex"
            || (tool_id == "chatgptdesktop" && Self::codex_has_third_party_provider())
        {
            if let Some(dir) = crate::services::codex_runtime::default_codex_dir() {
                if let Err(error) = crate::services::codex_runtime::bypass_onboarding(&dir) {
                    log::warn!("[ProcessManager] bypass_onboarding failed (non-fatal): {error}");
                }
            }
        }

        if tool_id == "claudescience" {
            self.start_claude_science().await?;
            let path = crate::services::tool_config_manager::claude_science_restart_file();
            if path.exists() {
                std::fs::remove_file(path)
                    .map_err(|e| format!("Failed to clear Claude Science restart state: {e}"))?;
            }
            return Ok(());
        }

        // A normal launch honors the CLI launch settings; opening a client for
        // login keeps its plain default.
        let launch = if restart {
            crate::services::cli_launch::resolve(tool_id, start_command, cwd)?
        } else {
            crate::services::cli_launch::ResolvedLaunch {
                command: start_command.map(str::to_string),
                cwd: cwd.map(str::to_string),
                terminal: crate::services::cli_launch::AUTO_TERMINAL.to_string(),
            }
        };
        let start_command = launch.command.as_deref();
        let cwd = launch.cwd.as_deref();
        let terminal = launch.terminal.as_str();

        // Priority 1: If explicit command is given from frontend, use it
        if let Some(cmd) = start_command {
            log::info!(
                "[ProcessManager] Starting tool: {} with explicit command: {}",
                tool_id,
                cmd
            );
            return self.start_cli_tool(tool_id, cmd, cwd, terminal);
        }

        // Priority 2: CLI tools with startCommand in paths.json (e.g. "openclaw gateway")
        if let Some(command) = crate::services::tool_manager::get_tool_start_command(tool_id) {
            // Extract base command (first word) for existence check
            let base_cmd = command.split_whitespace().next().unwrap_or(&command);
            // A curl/scoop/choco install can drop the binary in a dir that is not
            // on EchoBird's process PATH (the installing shell's setx only affects
            // future processes). The command_exists gate alone would reject it here
            // before start_cli_tool's PATH augmentation can help. Allow the launch
            // whenever paths.json still detects the binary on disk — start_cli_tool
            // prepends its dir to PATH, so `cmd /C <command>` resolves.
            if crate::utils::platform::command_exists(base_cmd).await
                || crate::services::tool_manager::get_tool_declared_exe_path(tool_id).is_some()
            {
                log::info!(
                    "[ProcessManager] Starting CLI tool: {} with startCommand: {}",
                    tool_id,
                    command
                );
                return self.start_cli_tool(tool_id, &command, cwd, terminal);
            } else {
                log::warn!("[ProcessManager] startCommand base '{}' for tool '{}' not found in PATH, skipping", base_cmd, tool_id);
            }
        }

        // Priority 2.9: MSIX / Store app — Windows-only. shell:AppsFolder\<AUMID>
        // is UWP-specific; on macOS/Linux this would shadow the platform-correct
        // GUI exe path below (paths.darwin / paths.linux). claudedesktop and
        // chatgptdesktop ship with both an AUMID and a valid .app / Linux path.
        #[cfg(windows)]
        if let Some(uri) = crate::services::tool_manager::get_tool_launch_uri(tool_id) {
            // Prefer a directly-launchable exe on disk over the Store AUMID.
            // winget installs Claude Desktop as a Squirrel build at
            // %LOCALAPPDATA%\AnthropicClaude\Claude.exe, whose AUMID
            // (shell:AppsFolder\Claude_pzs8sxrjxfjjc!Claude) does NOT resolve —
            // explorer.exe then pops a stray folder instead of the app. A true
            // MSIX/Store install has no exe at the declared paths (it lives
            // under WindowsApps), so this returns None and we use the AUMID as
            // before — no regression for Store installs.
            if crate::services::tool_manager::get_tool_declared_exe_path(tool_id).is_none() {
                log::info!(
                    "[ProcessManager] Launching MSIX/Store app for {}: {}",
                    tool_id,
                    uri
                );
                return self.start_shell_uri(tool_id, &uri);
            }
            log::info!(
                "[ProcessManager] {} has a directly-launchable exe on disk; preferring it over the Store AUMID",
                tool_id
            );
            // fall through to Priority 3 (GUI exe)
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
            // Same non-PATH install allowance as Priority 2: if paths.json still
            // detects the binary on disk, proceed — start_cli_tool prepends its
            // dir to PATH so the bare command resolves.
            if crate::utils::platform::command_exists(&command).await
                || crate::services::tool_manager::get_tool_declared_exe_path(tool_id).is_some()
            {
                log::info!(
                    "[ProcessManager] Falling back to CLI command for {}: {}",
                    tool_id,
                    command
                );
                return self.start_cli_tool(tool_id, &command, cwd, terminal);
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

    /// True iff ~/.codex/config.toml points at a non-OpenAI endpoint.
    /// Third-party configurations need the onboarding preparation before the
    /// shared desktop launch path; official OpenAI keeps its native onboarding.
    fn codex_has_third_party_provider() -> bool {
        let config_path = match crate::services::codex_runtime::default_codex_dir() {
            Some(dir) => dir.join("config.toml"),
            None => {
                log::warn!("[codex_has_third_party_provider] No Codex config directory found");
                return false;
            }
        };

        log::info!(
            "[codex_has_third_party_provider] Checking config at: {:?}",
            config_path
        );

        let content = match std::fs::read_to_string(&config_path) {
            Ok(c) => c,
            Err(e) => {
                log::warn!(
                    "[codex_has_third_party_provider] Failed to read config: {}",
                    e
                );
                return false;
            }
        };

        let provider =
            crate::services::tool_config_manager::toml_read_top(&content, "model_provider");
        let base_url = crate::services::tool_config_manager::toml_read_table_value(
            &content,
            &format!("model_providers.{provider}"),
            "base_url",
        );
        let is_third_party = is_third_party_codex_base_url(&base_url);

        log::info!(
            "[codex_has_third_party_provider] baseUrl='{}', is_third_party={}",
            base_url,
            is_third_party
        );

        is_third_party
    }

    async fn start_claude_science(&mut self) -> Result<(), String> {
        let executable = crate::services::tool_manager::get_tool_exe_path("claudescience")
            .ok_or("Claude Science is not installed")?;
        let mut command = crate::utils::process::async_command(&executable);
        command.args(["serve", "--detached", "--no-browser"]);
        if dirs::home_dir().is_some_and(|home| home.join(".echobird/claudescience.json").exists()) {
            command.env(
                "ANTHROPIC_BASE_URL",
                format!(
                    "http://127.0.0.1:{}/claudescience",
                    crate::services::anthropic_proxy::port()
                ),
            );
        } else {
            command.env_remove("ANTHROPIC_BASE_URL");
        }
        let status = tokio::time::timeout(std::time::Duration::from_secs(20), command.status())
            .await
            .map_err(|_| "Claude Science took too long to start")?
            .map_err(|e| format!("Failed to start Claude Science: {e}"))?;
        if !status.success() {
            return Err(format!(
                "Claude Science could not start (exit code {status})"
            ));
        }
        let output = crate::utils::process::async_command(&executable)
            .arg("status")
            .output()
            .await
            .map_err(|e| format!("Failed to read Claude Science status: {e}"))?;
        if let Ok(value) = serde_json::from_slice::<serde_json::Value>(&output.stdout) {
            if let Some(pid) = value["pid"]
                .as_u64()
                .and_then(|pid| u32::try_from(pid).ok())
            {
                self.processes
                    .insert("claudescience".into(), ProcessInfo::new(pid));
            }
        }
        Ok(())
    }

    /// Start a CLI tool via terminal
    fn start_cli_tool(
        &mut self,
        tool_id: &str,
        command: &str,
        cwd: Option<&str>,
        terminal: &str,
    ) -> Result<(), String> {
        if terminal == crate::services::cli_launch::COFFEE_TERMINAL {
            let pid = crate::services::cli_launch::start_in_coffee(tool_id, cwd)?;
            self.processes
                .insert(tool_id.to_string(), ProcessInfo::new(pid));
            return Ok(());
        }
        if tool_id == "cursorcli" && command != "cursor-agent" {
            return Err(
                "Cursor CLI must launch through cursor-agent, not an ambiguous agent command"
                    .into(),
            );
        }
        let home = dirs::home_dir().unwrap_or_default();
        // Working directory for the spawned terminal: the folder the user
        // picked in the frontend launch dialog (threaded through start_tool),
        // validated to still exist on disk — fall back to home if absent so a
        // stale/deleted path never fails the spawn. Coffee-CLI-style launchpad
        // picker, minus the per-tool persistence (we re-prompt every launch).
        let work_dir = cwd
            .map(std::path::PathBuf::from)
            .filter(|p| p.is_dir())
            .unwrap_or_else(|| home.clone());
        let echobird_dir = dirs::home_dir().unwrap_or_default().join(".echobird");
        let config_path = echobird_dir.join(format!("{}.json", tool_id));

        // Read echobird config for env vars
        let mut api_key_env: Option<String> = None;
        let mut base_url_env: Option<String> = None;
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
                    custom_api_key_env_name = config
                        .get("envKey")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                }
            }
        }

        // Codex carries its upstream in ~/.codex/config.toml + auth.json.
        // Muse keeps the real upstream in EchoBird and reads only its local URL.
        // Configuration and launch are separate concerns, so do not inject
        // duplicate OPENAI_* environment variables.
        if matches!(tool_id, "codex" | "musecode") {
            api_key_env = None;
            base_url_env = None;
            custom_api_key_env_name = None;
        }

        // Build the final command with tool-specific args
        let literal_executable = (tool_id == "copilot")
            .then(|| {
                copilot_literal_launch_path(
                    command,
                    crate::services::tool_manager::get_tool_exe_path(tool_id),
                )
            })
            .flatten()
            .or_else(|| {
                if tool_id == "cursorcli" {
                    return crate::services::tool_manager::get_tool_exe_path(tool_id)
                        .filter(|path| command == "cursor-agent" || command == path);
                }
                if tool_id == "devincli" {
                    return devin_literal_launch_path(
                        command,
                        crate::services::tool_manager::get_tool_exe_path(tool_id),
                    );
                }
                if tool_id != "minimaxcode" {
                    return None;
                }
                minimax_literal_launch_path(
                    command,
                    crate::services::tool_manager::get_tool_exe_path(tool_id),
                )
            });
        if tool_id == "cursorcli" && literal_executable.is_none() {
            return Err("Cursor CLI executable not found".into());
        }
        let mut full_command = command.to_string();
        #[cfg(not(windows))]
        if let Some(executable) = &literal_executable {
            full_command = shell_single_quote(executable);
        }

        // OpenCode reads the selected model from its native config; the V2
        // default command does not accept --model. MiMo / Kilo still use the
        // flag to override project config and recent-model history.
        let model_id = match tool_id {
            "mimocode" => crate::services::tool_config_manager::mimocode_echobird_model(),
            "kilo" => crate::services::tool_config_manager::kilo_echobird_model(),
            _ => None,
        };
        if tool_id == "kilo" {
            #[cfg(not(windows))]
            if let Some(argument) = kilo_model_arguments(model_id.as_deref()).get(1) {
                full_command = format!("{} --model {}", command, shell_single_quote(argument));
            }
        }

        if tool_id == "mimocode" {
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
            let shell = crate::services::cli_launch::windows_shell(terminal)?;
            let independent_terminal =
                shell.is_some() || (literal_executable.is_none() && tool_id != "kilo");
            let mut cmd = if let Some(shell) = &shell {
                let mut line = full_command.clone();
                let mut native_env = None;
                if let Some(executable) = &literal_executable {
                    line = shell.program_line(executable);
                } else if tool_id == "kilo" {
                    let runtime = crate::services::tool_manager::get_kilo_runtime_path()
                        .ok_or("Kilo CLI executable not found")?;
                    let arguments = kilo_model_arguments(model_id.as_deref())
                        .iter()
                        .map(|argument| shell.quote(argument))
                        .collect::<Vec<_>>();
                    line = std::iter::once(shell.program_line(&runtime))
                        .chain(arguments)
                        .collect::<Vec<_>>()
                        .join(" ");
                    native_env = Some(kilo_native_command(&runtime, model_id.as_deref()));
                }
                let mut launcher = windows_shell_terminal(shell, &line, &work_dir);
                if let Some(native) = &native_env {
                    for (key, value) in native.get_envs() {
                        if let Some(value) = value {
                            launcher.env(key, value);
                        }
                    }
                }
                launcher
            } else if let Some(executable) = &literal_executable {
                copilot_native_command(executable)
            } else if tool_id == "kilo" {
                let runtime = crate::services::tool_manager::get_kilo_runtime_path()
                    .ok_or("Kilo CLI executable not found")?;
                kilo_native_command(&runtime, model_id.as_deref())
            } else {
                windows_cli_terminal(&full_command, &work_dir)
            };
            cmd.current_dir(&work_dir);

            // Augment PATH with the detected tool's bin dir so a bare
            // `cmd /C <command>` resolves tools installed outside EchoBird's
            // process PATH. The gap: Kimi Code's curl installer drops kimi.exe
            // in ~/.kimi-code/bin and only prepends that dir to the *installing*
            // shell's PATH (setx updates future processes, but an already-running
            // EchoBird keeps the stale env) — detection still finds it via the
            // hardcoded paths.json entry, but `cmd /C kimi` can't, so the spawned
            // console flashes "not recognized" and exits. npm-installed tools
            // (claude/codex/mimo in %APPDATA%\npm) are already in PATH, so this
            // is a no-op for them; it only fills the curl/binary-install gap.
            if let Some(exe) = crate::services::tool_manager::get_tool_declared_exe_path(tool_id) {
                if let Some(bin) = std::path::Path::new(&exe).parent() {
                    let augmented = match std::env::var_os("PATH") {
                        Some(existing) => {
                            let mut joined = std::ffi::OsString::new();
                            joined.push(bin.as_os_str());
                            joined.push(";");
                            joined.push(existing);
                            joined
                        }
                        None => bin.as_os_str().to_os_string(),
                    };
                    cmd.env("PATH", augmented);
                    log::info!(
                        "[ProcessManager] Augmented PATH with detected bin for {}: {}",
                        tool_id,
                        bin.display()
                    );
                }
            }

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

            let pid = if independent_terminal {
                let tool_name = match tool_id {
                    "musecode" => "Muse Code",
                    "claudecode" => "Claude Code",
                    "omp" => "Oh My Pi",
                    "opencode" => "OpenCode",
                    _ => tool_id,
                };
                let output = cmd.output().map_err(|e| format!("Spawn error: {e}"))?;
                if !output.status.success() {
                    return Err(format!(
                        "Failed to launch {tool_name}: {}",
                        String::from_utf8_lossy(&output.stderr).trim()
                    ));
                }
                String::from_utf8_lossy(&output.stdout)
                    .trim()
                    .parse::<u32>()
                    .map_err(|e| format!("Failed to read {tool_name} terminal PID: {e}"))?
            } else {
                // Visible terminal window, independent of the parent process.
                cmd.creation_flags(CREATE_NEW_PROCESS_GROUP | CREATE_NEW_CONSOLE);
                cmd.spawn().map_err(|e| format!("Spawn error: {e}"))?.id()
            };
            log::info!(
                "[ProcessManager] Tool {} started with PID: {}",
                tool_id,
                pid
            );
            self.processes
                .insert(tool_id.to_string(), ProcessInfo::new(pid));
            Ok(())
        }

        // Non-Windows: TUI tools (claude/codex/opencode etc.) need a real
        // TTY, so we can't spawn the binary as a child of the Tauri GUI —
        // we launch it inside a terminal emulator (Linux) / Terminal.app
        // (macOS). All platform detail lives in spawn_in_unix_terminal so
        // the Codex native path can share the exact same mechanism.
        #[cfg(not(windows))]
        {
            let full_command = if tool_id == "musecode" {
                let executable = crate::services::tool_manager::get_tool_declared_exe_path(tool_id)
                    .map(std::path::PathBuf::from);
                musecode_unix_command(
                    &full_command,
                    crate::services::tool_config_manager::musecode::managed_config_home()
                        .as_deref(),
                    executable.as_deref(),
                )
            } else {
                full_command
            };
            let mut env_pairs: Vec<(String, String)> = Vec::new();
            if let Some(ref key) = api_key_env {
                env_pairs.push(("OPENAI_API_KEY".to_string(), key.clone()));
                if let Some(ref env_name) = custom_api_key_env_name {
                    env_pairs.push((env_name.clone(), key.clone()));
                }
            }
            if let Some(ref url) = base_url_env {
                env_pairs.push(("OPENAI_BASE_URL".to_string(), url.clone()));
            }

            let pid = Self::spawn_in_unix_terminal(
                tool_id,
                &work_dir,
                &full_command,
                &env_pairs,
                terminal,
            )?;
            log::info!(
                "[ProcessManager] Tool {} started in terminal with PID: {}",
                tool_id,
                pid
            );
            self.processes
                .insert(tool_id.to_string(), ProcessInfo::new(pid));
            Ok(())
        }
    }

    /// Launch `full_command` inside a fresh OS terminal so a TUI gets a
    /// real TTY, returning the spawned PID. This is the single place that
    /// knows how to "open a terminal" on each Unix platform; both
    /// `start_cli_tool` and the Codex CLI native path go through it so their
    /// Linux/macOS behavior can't drift apart. `env_pairs` are exported into
    /// the launched shell (empty for tools like Codex that carry their
    /// upstream config out-of-band).
    #[cfg(not(windows))]
    fn spawn_in_unix_terminal(
        tool_id: &str,
        cwd: &std::path::Path,
        full_command: &str,
        env_pairs: &[(String, String)],
        terminal: &str,
    ) -> Result<u32, String> {
        // `tool_id` only names the macOS launch script; reference it on the
        // other targets so they don't warn about an unused parameter.
        #[cfg(not(target_os = "macos"))]
        let _ = tool_id;
        #[cfg(all(unix, not(target_os = "linux"), not(target_os = "macos")))]
        let _ = terminal;

        // Linux: find a terminal emulator and run the command through the
        // user's login+interactive shell inside it. `-l -i` sources
        // .bashrc/.zshrc where npm/bun/cargo PATH entries live; separate
        // flags (not `-ilc`) keep fish-shell happy.
        #[cfg(target_os = "linux")]
        {
            let term = find_terminal_emulator(terminal).ok_or_else(|| {
                if terminal != crate::services::cli_launch::AUTO_TERMINAL {
                    return format!("Terminal not found: {terminal}");
                }
                "No terminal emulator found. Please install one of: \
                 gnome-terminal, konsole, xfce4-terminal, alacritty, \
                 kitty, wezterm, foot, tilix, or xterm."
                    .to_string()
            })?;

            let mut cmd = Command::new(&term.binary);
            cmd.args(term.prefix_args.iter().copied());
            let user_shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".into());
            // The login+interactive shell sources the user's PATH (nvm/bun/
            // cargo), but the pause wrapper is POSIX-sh syntax that non-POSIX
            // shells (fish) can't parse. Have the login shell set up the
            // environment and then `exec /bin/sh` run the wrapper, so it works
            // regardless of $SHELL.
            let wrapped = wrap_with_pause_on_quick_or_error(full_command);
            let via_posix = format!("exec /bin/sh -c {}", shell_single_quote(&wrapped));
            cmd.arg(&user_shell)
                .arg("-l")
                .arg("-i")
                .arg("-c")
                .arg(&via_posix);
            cmd.current_dir(cwd);
            for (k, v) in env_pairs {
                cmd.env(k, v);
            }

            let child = cmd.spawn().map_err(|e| {
                format!(
                    "Failed to launch terminal '{}': {}",
                    term.binary.display(),
                    e
                )
            })?;
            Ok(child.id())
        }

        // macOS: Terminal.app is system-bundled, so no detection needed.
        // Bake env exports + a re-exec into the user's login shell into a
        // cached script and hand it to `open -a Terminal`. Env can't ride on
        // `open` itself because Terminal spawns a fresh login shell. Caveat:
        // `open` exits immediately, so the captured PID belongs to `open`,
        // not the tool — is_tool_running() will report it exited shortly
        // after launch, acceptable since users close the tab themselves.
        #[cfg(target_os = "macos")]
        {
            use std::os::unix::fs::PermissionsExt;

            let cache_dir = dirs::cache_dir()
                .unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join(".cache"))
                .join("echobird");
            std::fs::create_dir_all(&cache_dir)
                .map_err(|e| format!("Failed to create cache dir: {}", e))?;

            let script_path = cache_dir.join(format!("launch-{}.sh", tool_id));

            let mut script = String::from("#!/bin/bash\n");
            for (k, v) in env_pairs {
                script.push_str(&format!("export {}={}\n", k, shell_single_quote(v)));
            }
            script.push_str(&format!(
                "cd {}\n",
                shell_single_quote(&cwd.to_string_lossy())
            ));
            // Same fish-compat reasoning as the Linux branch: the login shell
            // sets up PATH, then `exec /bin/sh` runs the POSIX pause wrapper.
            let via_posix = format!(
                "exec /bin/sh -c {}",
                shell_single_quote(&wrap_with_pause_on_quick_or_error(full_command))
            );
            script.push_str(&format!(
                "exec \"${{SHELL:-/bin/bash}}\" -l -i -c {}\n",
                shell_single_quote(&via_posix)
            ));

            std::fs::write(&script_path, &script)
                .map_err(|e| format!("Failed to write launch script: {}", e))?;

            let mut perms = std::fs::metadata(&script_path)
                .map_err(|e| format!("metadata: {}", e))?
                .permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&script_path, perms).map_err(|e| format!("chmod: {}", e))?;

            let script_file = script_path.to_string_lossy();
            let run_script = format!("exec sh {}", shell_single_quote(&script_file));
            let mut opener = match terminal {
                crate::services::cli_launch::AUTO_TERMINAL => {
                    let mut opener = Command::new("open");
                    opener.args(["-a", "Terminal"]).arg(&script_path);
                    opener
                }
                "iterm2" => {
                    // Shell quoting adds backslashes, so AppleScript escaping comes last.
                    let shell_line = format!("sh -c {}", shell_single_quote(&run_script));
                    let quoted = shell_line.replace('\\', "\\\\").replace('"', "\\\"");
                    let mut opener = Command::new("osascript");
                    opener.args([
                        "-e",
                        "tell application \"iTerm\" to activate",
                        "-e",
                        &format!(
                            "tell application \"iTerm\" to create window with default profile command \"{quoted}\""
                        ),
                    ]);
                    opener
                }
                app => {
                    let (name, flags) = crate::services::cli_launch::macos_terminal_app(app)
                        .ok_or_else(|| format!("Terminal not found: {app}"))?;
                    let mut opener = Command::new("open");
                    opener
                        .args(["-na", name, "--args"])
                        .args(flags.iter().copied())
                        .args(["sh", "-c", &run_script]);
                    opener
                }
            };
            let child = opener
                .spawn()
                .map_err(|e| format!("Failed to launch terminal: {}", e))?;
            Ok(child.id())
        }

        // Other unix (*BSD, etc.): no terminal-emulator probe, so fall back
        // to the historical raw spawn. Realistically unreached in prod.
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
            cmd.current_dir(cwd);
            for (k, v) in env_pairs {
                cmd.env(k, v);
            }
            let child = cmd.spawn().map_err(|e| format!("Spawn error: {}", e))?;
            Ok(child.id())
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
                .insert(tool_id.to_string(), ProcessInfo::new(pid));
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
                .insert(tool_id.to_string(), ProcessInfo::new(pid));
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
                .insert(tool_id.to_string(), ProcessInfo::new(pid));
            Ok(())
        }

        #[cfg(target_os = "android")]
        {
            let _ = tool_id;
            return Err("Not available on mobile".to_string());
        }
    }

    /// Launch an MSIX/Store app via shell:AppsFolder URI (Windows only).
    #[cfg(windows)]
    fn start_shell_uri(&mut self, tool_id: &str, uri: &str) -> Result<(), String> {
        let result = crate::utils::process::command("explorer.exe")
            .arg(uri)
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
                    .insert(tool_id.to_string(), ProcessInfo::new(pid));
                Ok(())
            }
            Err(e) => Err(format!("Failed to launch via shell URI: {}", e)),
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

            // Launch from the executable's own directory, like a desktop
            // shortcut does. Most GUIs (incl. Electron apps such as Hermes
            // Desktop's win-unpacked build) resolve resources relative to the
            // exe, but some rely on the working directory, so match shortcut
            // behaviour to be safe.
            let work_dir = std::path::Path::new(&exe_path)
                .parent()
                .map(|p| p.to_string_lossy().to_string());
            // PowerShell single-quoted strings escape a literal apostrophe by
            // doubling it (''). Escape before interpolating so a path with an
            // apostrophe (e.g. a Windows username like O'Brien) can't break the
            // quoting or inject commands.
            let exe_q = exe_path.replace('\'', "''");
            let ps_cmd = match &work_dir {
                Some(dir) => format!(
                    "$process = Start-Process '{}' -WorkingDirectory '{}' -PassThru; Write-Output $process.Id",
                    exe_q,
                    dir.replace('\'', "''")
                ),
                None => format!(
                    "$process = Start-Process '{}' -PassThru; Write-Output $process.Id",
                    exe_q
                ),
            };

            let mut command = crate::utils::process::command("powershell");
            command
                .args(["-Command", &ps_cmd])
                .creation_flags(CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW);

            // OpenScience currently builds runtime-run filenames long enough
            // to exceed Win32's legacy path limit under its default managed
            // data-root link. A short, per-user direct root keeps prompt
            // publishing below that limit without sharing state across Windows
            // accounts; the Electron sidecar inherits this variable.
            if tool_id == "openscience" {
                let data_dir = openscience_windows_data_dir();
                std::fs::create_dir_all(&data_dir).map_err(|e| {
                    format!(
                        "Failed to create OpenScience data directory {}: {}",
                        data_dir.display(),
                        e
                    )
                })?;
                command.env("OPENSCIENCE_DATA_DIR", &data_dir);
            }

            let output = command
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
                    .insert(tool_id.to_string(), ProcessInfo::new(pid));
                return Ok(());
            }
            Err(format!("Failed to launch GUI tool: {}", pid_str))
        }

        #[cfg(target_os = "macos")]
        {
            // exe_path is the inner Mach-O binary (…/Foo.app/Contents/MacOS/Foo).
            // Handing that straight to `open` makes LaunchServices treat the
            // executable as a *document* and route it to the user's default
            // text editor (Sublime Text, etc.) instead of launching the app —
            // so for a bundled app we must `open` the .app BUNDLE root. A bare
            // (non-bundle) binary has no .app ancestor; `open`-ing it would hit
            // the same document trap, so spawn it directly like the Linux path.
            let child = match exe_path.rfind(".app/") {
                Some(i) => Command::new("open")
                    .arg(&exe_path[..i + ".app".len()])
                    .spawn(),
                None => Command::new(&exe_path).spawn(),
            }
            .map_err(|e| format!("Failed to launch {}: {}", tool_id, e))?;

            let pid = child.id();
            log::info!(
                "[ProcessManager] GUI tool {} started ({}), PID: {}",
                tool_id,
                exe_path,
                pid
            );
            self.processes
                .insert(tool_id.to_string(), ProcessInfo::new(pid));
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
                .insert(tool_id.to_string(), ProcessInfo::new(pid));
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
            // Windows: taskkill /T /F to kill process tree
            let output = crate::utils::process::command("taskkill")
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

    /// Kill every running instance of a desktop tool — by process image name or path
    /// (so it also catches an instance the USER launched, which we have no
    /// tracked PID for) and by dropping our own tracked PID. Returns true if
    /// anything was killed. Desktop-only: callers gate on
    /// `tool_manager::is_desktop_tool`. Restores "launch = kill + restart" so
    /// switching the model actually takes effect — a running desktop app loaded
    /// the OLD provider config at startup and won't pick up the change.
    fn kill_desktop_instances(&mut self, tool_id: &str) -> bool {
        // Drop our tracked PID so is_tool_running stays honest; the image-name
        // kill below terminates that process anyway.
        self.processes.remove(tool_id);

        #[cfg(windows)]
        if tool_id == "claudedesktop" {
            let executable = crate::services::tool_manager::get_tool_exe_path(tool_id);
            let launch_uri = crate::services::tool_manager::get_tool_launch_uri(tool_id);
            // Use the existing PID/path revalidation instead of killing every
            // Claude.exe image: the standalone Claude Code CLI has the same name.
            return qoder_process_snapshot().is_ok_and(|processes| {
                let desktop = processes
                    .into_iter()
                    .filter(|process| {
                        claude_desktop_windows_process(
                            &process.path,
                            executable.as_deref(),
                            launch_uri.as_deref(),
                        )
                    })
                    .collect::<Vec<_>>();
                stop_qoder_account_instances(&desktop)
            });
        }

        if matches!(tool_id, "qoder" | "qodercn" | "qwenwork" | "qwenworkcn") {
            let Some(path) = crate::services::tool_manager::get_tool_exe_path(tool_id) else {
                return false;
            };
            return qoder_process_snapshot()
                .and_then(|processes| {
                    qoder_account_instances(tool_id, std::path::Path::new(&path), processes)
                })
                .is_ok_and(|processes| stop_qoder_account_instances(&processes));
        }

        #[cfg(not(windows))]
        if matches!(tool_id, "copilotdesktop" | "kilodesktop") {
            let Some(path) = crate::services::tool_manager::get_tool_exe_path(tool_id) else {
                return false;
            };
            #[cfg(target_os = "linux")]
            if tool_id == "copilotdesktop"
                && std::path::Path::new(&path)
                    .extension()
                    .is_some_and(|extension| extension == "AppImage")
            {
                return crate::services::copilot_accounts::desktop::verified_appimage_pid(
                    std::path::Path::new(&path),
                )
                .is_some_and(|pid| unsafe { libc::kill(pid as i32, libc::SIGTERM) == 0 });
            }
            let Ok(path) = std::fs::canonicalize(path) else {
                return false;
            };
            return crate::utils::process::command("pkill")
                .args(["-f", &executable_process_pattern(&path.to_string_lossy())])
                .output()
                .is_ok_and(|output| output.status.success());
        }

        // CodeArts and Devin share image names with standalone CLIs; MiniMax
        // editions and macOS bundles also need the resolved executable path.
        if matches!(tool_id, "minimaxdesktop" | "codearts" | "dumate" | "devin")
            || (cfg!(target_os = "macos")
                && matches!(tool_id, "workbuddy" | "workbuddyai" | "kirocrew"))
        {
            let Some(path) = crate::services::tool_manager::get_tool_exe_path(tool_id) else {
                return false;
            };
            #[cfg(target_os = "macos")]
            let path = if matches!(tool_id, "workbuddy" | "workbuddyai" | "kirocrew") {
                // Also handle bundle executable symlinks, including legacy Electron links.
                let Ok(path) = std::fs::canonicalize(path) else {
                    return false;
                };
                path.to_string_lossy().into_owned()
            } else {
                path
            };
            #[cfg(windows)]
            {
                return crate::utils::process::command("powershell")
                    .args(["-NoProfile", "-NonInteractive", "-Command", "$stopped=$false; Get-CimInstance Win32_Process | Where-Object { $_.ExecutablePath -eq $env:ECHOBIRD_DESKTOP_EXE } | ForEach-Object { taskkill /PID $_.ProcessId /T /F; if($LASTEXITCODE -eq 0){$stopped=$true} }; if($stopped){exit 0}else{exit 1}"])
                    .env("ECHOBIRD_DESKTOP_EXE", path)
                    .output().is_ok_and(|out|out.status.success());
            }
            #[cfg(not(windows))]
            {
                return crate::utils::process::command("pkill")
                    .args(["-f", &executable_process_pattern(&path)])
                    .output()
                    .is_ok_and(|out| out.status.success());
            }
        }

        let names = crate::services::tool_manager::get_tool_process_names(tool_id);
        let mut killed = false;
        for name in &names {
            #[cfg(windows)]
            {
                // /T also reaps the Electron helper-process tree.
                if let Ok(out) = crate::utils::process::command("taskkill")
                    .args(["/IM", name.as_str(), "/F", "/T"])
                    .output()
                {
                    killed |= out.status.success();
                }
            }
            #[cfg(not(windows))]
            {
                // macOS/Linux: match the exact process (binary) name.
                if let Ok(out) = crate::utils::process::command("pkill")
                    .args(["-x", name.as_str()])
                    .output()
                {
                    killed |= out.status.success();
                }
            }
        }
        if killed {
            log::info!(
                "[ProcessManager] killed running instances of {tool_id} ({names:?}) for kill+restart"
            );
        }
        killed
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
                    let _ = crate::utils::process::command("taskkill")
                        .args(["/pid", &info.pid.to_string(), "/T", "/F"])
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
            let output = crate::utils::process::command("tasklist")
                .args(["/FI", &format!("PID eq {}", info.pid), "/FO", "CSV", "/NH"])
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

fn is_third_party_codex_base_url(base_url: &str) -> bool {
    !base_url.is_empty()
        && !crate::services::codex_catalog::url_matches_domain(base_url, "api.openai.com")
}

#[cfg(windows)]
fn openscience_windows_data_dir() -> std::path::PathBuf {
    let home = dirs::home_dir().unwrap_or_default();
    openscience_windows_data_dir_for(&home)
}

#[cfg(windows)]
fn openscience_windows_data_dir_for(home: &std::path::Path) -> std::path::PathBuf {
    use sha2::{Digest, Sha256};

    let digest = Sha256::digest(home.to_string_lossy().to_lowercase().as_bytes());
    let user_key = hex::encode(&digest[..5]);
    home.ancestors()
        .last()
        .unwrap_or(home)
        .join("OS")
        .join(user_key)
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

#[cfg(windows)]
fn windows_console_launcher() -> Command {
    let powershell = std::env::var_os("SystemRoot")
        .map(|root| {
            std::path::PathBuf::from(root).join("System32/WindowsPowerShell/v1.0/powershell.exe")
        })
        .unwrap_or_else(|| "powershell.exe".into());
    crate::utils::process::command(powershell)
}

#[cfg(windows)]
fn windows_cli_terminal(command: &str, cwd: &std::path::Path) -> Command {
    // CREATE_NEW_CONSOLE alone still inherits a development client's log pipes.
    // Start-Process gives interactive TUIs their own console handles.
    let mut launcher = windows_console_launcher();
    launcher
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "$ErrorActionPreference='Stop'; $child=Start-Process -FilePath $env:ECHOBIRD_CLI_CMD_EXE -ArgumentList ('/D /S /C \"' + $env:ECHOBIRD_CLI_COMMAND + '\"') -WorkingDirectory $env:ECHOBIRD_CLI_CWD -WindowStyle Normal -PassThru; Write-Output $child.Id",
        ])
        .env("ECHOBIRD_CLI_CMD_EXE", resolve_cmd_exe())
        .env("ECHOBIRD_CLI_COMMAND", command)
        .env("ECHOBIRD_CLI_CWD", cwd);
    launcher
}

/// Same detached console as `windows_cli_terminal`, hosted by the chosen shell.
/// The shell stays open after the client exits, as its own window would.
#[cfg(windows)]
fn windows_shell_terminal(
    shell: &crate::services::cli_launch::WindowsShell,
    command: &str,
    cwd: &std::path::Path,
) -> Command {
    let mut launcher = windows_console_launcher();
    launcher
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "$ErrorActionPreference='Stop'; $child=Start-Process -FilePath $env:ECHOBIRD_CLI_SHELL -ArgumentList $env:ECHOBIRD_CLI_ARGUMENTS -WorkingDirectory $env:ECHOBIRD_CLI_CWD -WindowStyle Normal -PassThru; Write-Output $child.Id",
        ])
        .env("ECHOBIRD_CLI_SHELL", shell.program())
        .env("ECHOBIRD_CLI_ARGUMENTS", shell.arguments(command))
        .env("ECHOBIRD_CLI_COMMAND", command)
        .env("ECHOBIRD_CLI_CWD", cwd);
    launcher
}

// Single-quote-wrap a string for safe inclusion in a POSIX shell command
// line. Embedded single quotes become '\''. Used to quote binary paths and
// env values that get spliced into `shell -c` strings (macOS launch script
// and the Codex CLI native path).
#[cfg(any(not(windows), test))]
fn shell_single_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

#[cfg(any(not(windows), test))]
fn musecode_unix_command(
    command: &str,
    config_home: Option<&std::path::Path>,
    executable: Option<&std::path::Path>,
) -> String {
    let command = match (command, executable) {
        ("muse", Some(path)) => shell_single_quote(&path.to_string_lossy()),
        _ => command.to_string(),
    };
    match config_home {
        Some(home) => format!(
            "XDG_CONFIG_HOME={} {command}",
            shell_single_quote(&home.to_string_lossy())
        ),
        None => command,
    }
}

fn copilot_literal_launch_path(command: &str, executable: Option<String>) -> Option<String> {
    executable.filter(|path| command == "copilot" || command == path)
}

fn minimax_literal_launch_path(command: &str, executable: Option<String>) -> Option<String> {
    executable.filter(|path| command == "mcode" || command == path)
}

fn devin_literal_launch_path(command: &str, executable: Option<String>) -> Option<String> {
    executable.filter(|path| command == "devin" || command == path)
}

#[cfg(windows)]
pub(crate) fn copilot_native_command(executable: &str) -> Command {
    use std::os::windows::process::CommandExt;
    if std::path::Path::new(executable)
        .extension()
        .is_some_and(|extension| {
            extension.eq_ignore_ascii_case("cmd") || extension.eq_ignore_ascii_case("bat")
        })
    {
        let mut command = crate::utils::process::command(resolve_cmd_exe());
        command.args(["/d", "/v:off", "/s", "/c"]);
        command.env("ECHOBIRD_COPILOT_NATIVE_WRAPPER", executable);
        command.raw_arg("\"\"%ECHOBIRD_COPILOT_NATIVE_WRAPPER%\"\"");
        command
    } else {
        crate::utils::process::command(executable)
    }
}

fn kilo_model_arguments(model_id: Option<&str>) -> Vec<String> {
    model_id.map_or_else(Vec::new, |id| {
        vec!["--model".into(), format!("echobird/{id}")]
    })
}

#[cfg(any(windows, test))]
fn kilo_native_command(runtime: &str, model_id: Option<&str>) -> Command {
    let mut command = Command::new(runtime);
    command.args(kilo_model_arguments(model_id));
    let resources = std::path::Path::new(runtime)
        .parent()
        .map(|parent| parent.join("tree-sitter"));
    if std::env::var_os("KILO_TREE_SITTER_WASM_DIR").is_none() {
        if let Some(resources) = resources.filter(|dir| dir.join("tree-sitter.wasm").is_file()) {
            command.env("KILO_TREE_SITTER_WASM_DIR", resources);
        }
    }
    command
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
fn find_terminal_emulator(preferred: &str) -> Option<TerminalLauncher> {
    const CANDIDATES: &[(&str, &[&str])] = &[
        ("x-terminal-emulator", &["-e"]),
        ("gnome-terminal", &["--"]),
        ("konsole", &["-e"]),
        // xfce4-terminal/tilix accept argv after `-x`; `-e` wants a single string.
        ("xfce4-terminal", &["-x"]),
        ("tilix", &["-x"]),
        ("alacritty", &["-e"]),
        ("kitty", &[]),
        ("ghostty", &["-e"]),
        ("wezterm", &["start", "--"]),
        ("foot", &[]),
        ("xterm", &["-e"]),
    ];

    let auto = preferred == crate::services::cli_launch::AUTO_TERMINAL;
    for &(name, prefix_args) in CANDIDATES {
        if !auto && name != preferred {
            continue;
        }
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
pub async fn start_tool(
    tool_id: &str,
    start_command: Option<&str>,
    cwd: Option<&str>,
) -> Result<(), String> {
    let mgr = get_manager().await;
    let mut mgr = mgr.lock().await;
    mgr.start_tool(tool_id, start_command, cwd).await
}

pub async fn claude_science_url() -> Result<url::Url, String> {
    let executable = crate::services::tool_manager::get_tool_exe_path("claudescience")
        .ok_or("Claude Science is not installed")?;
    for _ in 0..15 {
        let output = crate::utils::process::async_command(&executable)
            .arg("url")
            .output()
            .await
            .map_err(|e| format!("Failed to open Claude Science: {e}"))?;
        if output.status.success() {
            let raw = String::from_utf8_lossy(&output.stdout);
            let url = url::Url::parse(raw.trim())
                .map_err(|_| "Claude Science returned an invalid local URL")?;
            if url.scheme() != "http" || !matches!(url.host_str(), Some("127.0.0.1" | "localhost"))
            {
                return Err("Claude Science returned a non-local URL".into());
            }
            return Ok(url);
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
    Err("Claude Science did not provide a login link".into())
}

/// Open the native login surface without terminating an existing login or task.
pub async fn open_tool_for_login(tool_id: &str) -> Result<(), String> {
    let mgr = get_manager().await;
    let mut mgr = mgr.lock().await;
    mgr.launch_tool(tool_id, None, None, false).await
}

/// A temporary native client used only by an explicit configuration operation.
/// It does not enter the normal launcher cooldown or restart an existing client.
pub(crate) struct OwnedDesktopConfigProcess {
    child: std::process::Child,
    stopped: bool,
}

#[cfg(windows)]
pub(crate) fn owned_windows_process_times(
    child: &impl std::os::windows::io::AsRawHandle,
) -> Result<(u64, u64), String> {
    use windows::Win32::Foundation::FILETIME;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetProcessTimes(
            process: std::os::windows::io::RawHandle,
            created: *mut FILETIME,
            exited: *mut FILETIME,
            kernel: *mut FILETIME,
            user: *mut FILETIME,
        ) -> i32;
    }
    let mut created = FILETIME::default();
    let mut exited = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    // This is the original Child handle, including after exit, not a PID lookup.
    if unsafe {
        GetProcessTimes(
            child.as_raw_handle(),
            &mut created,
            &mut exited,
            &mut kernel,
            &mut user,
        )
    } == 0
    {
        return Err("accountError.closeClient".into());
    }
    let micros = |time: FILETIME| {
        ((u64::from(time.dwHighDateTime) << 32) | u64::from(time.dwLowDateTime)) / 10
    };
    Ok((micros(created), micros(exited)))
}

#[cfg(windows)]
pub(crate) fn stop_owned_windows_tree(pid: u32, created: u64, exited: u64) -> Result<(), String> {
    let script = r#"
$ErrorActionPreference='Stop'
function Created($entry){return [long][decimal]::Truncate(([decimal]($entry.CreationDate.ToFileTimeUtc()))/10)}
$all=@(Get-CimInstance Win32_Process -ErrorAction Stop)
$root=$all | Where-Object { $_.ProcessId -eq [uint32]$env:ECHOBIRD_OWNED_CONFIG_PID }
if($root -and (Created $root) -ne [long]$env:ECHOBIRD_OWNED_CONFIG_CREATED){exit 2}
$queue=[Collections.Generic.Queue[object]]::new()
$queue.Enqueue(@{Pid=[uint32]$env:ECHOBIRD_OWNED_CONFIG_PID;Created=[long]$env:ECHOBIRD_OWNED_CONFIG_CREATED;Exited=[long]$env:ECHOBIRD_OWNED_CONFIG_EXITED})
$owned=@()
while($queue.Count -gt 0){
  $parent=$queue.Dequeue()
  foreach($entry in $all | Where-Object { $_.ParentProcessId -eq $parent.Pid }){
    $birth=Created $entry
    if($birth -lt $parent.Created -or ($parent.Exited -gt 0 -and $birth -gt $parent.Exited)){continue}
    $identity=@{Pid=$entry.ProcessId;Created=$birth;Exited=0}
    $owned+=$identity
    $queue.Enqueue($identity)
  }
}
if($root){$owned+=@{Pid=$root.ProcessId;Created=(Created $root)}}
$ErrorActionPreference='Continue'
if($root){& taskkill.exe /PID $root.ProcessId /T /F >$null 2>&1}
$survivors=@(Get-CimInstance Win32_Process -ErrorAction Stop)
foreach($entry in $owned){
  $current=$survivors | Where-Object { $_.ProcessId -eq $entry.Pid }
  if($current -and (Created $current) -eq $entry.Created){& taskkill.exe /PID $entry.Pid /T /F >$null 2>&1}
}
$ErrorActionPreference='Stop'
$deadline=[DateTime]::UtcNow.AddSeconds(3)
do{
  $remaining=$false
  $current=@(Get-CimInstance Win32_Process -ErrorAction Stop)
  foreach($entry in $owned){
    $candidate=$current | Where-Object { $_.ProcessId -eq $entry.Pid }
    if($candidate -and (Created $candidate) -eq $entry.Created){$remaining=$true}
  }
  if(-not $remaining){exit 0}
  Start-Sleep -Milliseconds 100
}while([DateTime]::UtcNow -lt $deadline)
exit 1
"#;
    let status = crate::utils::process::command("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .env("ECHOBIRD_OWNED_CONFIG_PID", pid.to_string())
        .env("ECHOBIRD_OWNED_CONFIG_CREATED", created.to_string())
        .env("ECHOBIRD_OWNED_CONFIG_EXITED", exited.to_string())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map_err(|_| "accountError.closeClient")?;
    if status.success() {
        Ok(())
    } else {
        Err("accountError.closeClient".into())
    }
}

impl OwnedDesktopConfigProcess {
    pub(crate) fn id(&self) -> u32 {
        self.child.id()
    }

    pub(crate) fn stop(&mut self) -> Result<(), String> {
        if self.stopped {
            return Ok(());
        }
        #[cfg(windows)]
        {
            // taskkill /T can terminate the parent while a native SDK child
            // survives. Capture only this owned tree before killing it, and
            // retain creation times so a reused descendant PID is never targeted.
            let (created, ended) = owned_windows_process_times(&self.child)?;
            stop_owned_windows_tree(self.id(), created, ended)?;
        }
        #[cfg(unix)]
        {
            // Keep the leader unreaped until its owned helpers are stopped: an
            // exited leader can leave helpers alive, and its PID must not be reused.
            if unsafe { libc::kill(-(self.id() as i32), libc::SIGKILL) } != 0
                && std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
            {
                return Err("accountError.closeClient".into());
            }
        }
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        loop {
            if self
                .child
                .try_wait()
                .map_err(|_| "accountError.closeClient")?
                .is_some()
            {
                self.stopped = true;
                return Ok(());
            }
            if std::time::Instant::now() >= deadline {
                return Err("accountError.closeClient".into());
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }
}

impl Drop for OwnedDesktopConfigProcess {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

pub(crate) fn start_desktop_for_config(tool_id: &str) -> Result<OwnedDesktopConfigProcess, String> {
    start_desktop_with_config_args(tool_id, &[])
}

fn start_desktop_with_config_args(
    tool_id: &str,
    arguments: &[String],
) -> Result<OwnedDesktopConfigProcess, String> {
    let executable =
        super::tool_manager::get_tool_exe_path(tool_id).ok_or("accountError.initializeClient")?;
    let mut command = crate::utils::process::command(&executable);
    command.args(arguments);
    if let Some(directory) = std::path::Path::new(&executable).parent() {
        command.current_dir(directory);
    }
    command
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let child = command
        .spawn()
        .map_err(|_| "accountError.initializeClient")?;
    Ok(OwnedDesktopConfigProcess {
        child,
        stopped: false,
    })
}

#[derive(serde::Deserialize)]
struct QoderAccountProcess {
    pid: u32,
    path: std::path::PathBuf,
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn qoder_process_path(pid: u32) -> Option<std::path::PathBuf> {
    #[cfg(target_os = "linux")]
    {
        std::fs::read_link(format!("/proc/{pid}/exe")).ok()
    }
    #[cfg(target_os = "macos")]
    {
        use std::os::unix::ffi::OsStringExt;
        let mut buffer = [0u8; 4096];
        if unsafe { libc::proc_pidpath(pid as i32, buffer.as_mut_ptr().cast(), 4096) } <= 0 {
            return None;
        }
        let path = std::ffi::CStr::from_bytes_until_nul(&buffer).ok()?;
        Some(std::ffi::OsString::from_vec(path.to_bytes().to_vec()).into())
    }
}

fn qoder_process_snapshot() -> Result<Vec<QoderAccountProcess>, String> {
    #[cfg(windows)]
    {
        let output = crate::utils::process::command("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-Command", "$ErrorActionPreference='Stop'; [Console]::OutputEncoding=[Text.UTF8Encoding]::new($false); $rows=@(Get-CimInstance Win32_Process -ErrorAction Stop | Where-Object { $_.ExecutablePath } | ForEach-Object { @{pid=$_.ProcessId;path=$_.ExecutablePath} }); ConvertTo-Json -InputObject $rows -Compress"])
            .output().map_err(|_| "accountError.closeClient")?;
        if !output.status.success() {
            return Err("accountError.closeClient".into());
        }
        serde_json::from_slice(&output.stdout).map_err(|_| "accountError.closeClient".into())
    }
    #[cfg(target_os = "macos")]
    {
        let output = crate::utils::process::command("/bin/ps")
            .args(["-A", "-o", "pid="])
            .output()
            .map_err(|_| "accountError.closeClient")?;
        if !output.status.success() {
            return Err("accountError.closeClient".into());
        }
        Ok(String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter_map(|line| line.trim().parse().ok())
            .filter_map(|pid| qoder_process_path(pid).map(|path| QoderAccountProcess { pid, path }))
            .collect())
    }
    #[cfg(target_os = "linux")]
    {
        Ok(std::fs::read_dir("/proc")
            .map_err(|_| "accountError.closeClient")?
            .flatten()
            .filter_map(|entry| entry.file_name().to_str()?.parse().ok())
            .filter_map(|pid| qoder_process_path(pid).map(|path| QoderAccountProcess { pid, path }))
            .collect())
    }
    #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
    {
        Err("accountError.unavailable".into())
    }
}

fn qoder_account_instances(
    tool_id: &str,
    selected: &std::path::Path,
    processes: Vec<QoderAccountProcess>,
) -> Result<Vec<QoderAccountProcess>, String> {
    if matches!(tool_id, "qwenwork" | "qwenworkcn") {
        let package = if tool_id == "qwenwork" {
            "qwen-work"
        } else {
            "qwen-work-cn"
        };
        let matches = |path: &std::path::Path| {
            crate::services::tool_manager::packaged_desktop_metadata(path)
                .is_some_and(|value| value["name"] == package)
        };
        if !matches(selected) {
            return Err("accountError.invalidAccount".into());
        }
        return Ok(processes
            .into_iter()
            .filter(|process| matches(&process.path))
            .collect());
    }
    use crate::services::qoder_accounts::Edition;
    let edition = match tool_id {
        "qoder" => Edition::Qoder,
        "qodercn" => Edition::QoderCn,
        _ => return Err("accountError.invalidAccount".into()),
    };
    let identity = crate::services::qoder_native::desktop::identity(selected)
        .filter(|(selected_edition, _)| *selected_edition == edition)
        .ok_or("accountError.invalidAccount")?;
    let mut names = crate::services::tool_manager::get_tool_process_names(tool_id);
    if let Some(name) = selected.file_name().and_then(|name| name.to_str()) {
        names.push(name.into());
    }
    // Each verified edition/product tuple has one fixed native account directory,
    // shared by user/system installations. App and IDE tuples use different directories.
    Ok(processes
        .into_iter()
        .filter(|process| {
            process.path.file_name().is_some_and(|name| {
                names
                    .iter()
                    .any(|candidate| name.eq_ignore_ascii_case(candidate))
            }) && crate::services::qoder_native::desktop::identity(&process.path) == Some(identity)
        })
        .collect())
}

fn stop_qoder_account_instances(processes: &[QoderAccountProcess]) -> bool {
    let mut stopped = false;
    for process in processes {
        #[cfg(windows)]
        {
            stopped |= crate::utils::process::command("powershell.exe")
                .args(["-NoProfile", "-NonInteractive", "-Command", "$ErrorActionPreference='Stop'; $entry=Get-CimInstance Win32_Process -Filter ('ProcessId = '+$env:ECHOBIRD_ACCOUNT_PID) -ErrorAction Stop; if($entry -and $entry.ExecutablePath -eq $env:ECHOBIRD_ACCOUNT_EXE){ & taskkill.exe /PID $entry.ProcessId /T /F >$null 2>&1; exit $LASTEXITCODE }; exit 1"])
                .env("ECHOBIRD_ACCOUNT_PID", process.pid.to_string())
                .env("ECHOBIRD_ACCOUNT_EXE", &process.path)
                .status().is_ok_and(|status| status.success());
        }
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        {
            if qoder_process_path(process.pid).as_ref() == Some(&process.path) {
                stopped |= unsafe { libc::kill(process.pid as i32, libc::SIGTERM) == 0 };
            }
        }
    }
    stopped
}

#[cfg(any(windows, test))]
fn claude_desktop_windows_process(
    path: &std::path::Path,
    executable: Option<&str>,
    launch_uri: Option<&str>,
) -> bool {
    let normalized = |value: &str| value.replace('/', "\\").to_ascii_lowercase();
    let path = normalized(&path.to_string_lossy());
    if let Some(executable) = executable.filter(|value| !value.is_empty()).map(normalized) {
        if path == executable {
            return true;
        }
        // Squirrel's launcher lives beside app-<version>/Claude.exe.
        if let Some((root, "claude.exe")) = executable.rsplit_once('\\') {
            if path
                .strip_prefix(&format!("{root}\\"))
                .and_then(|relative| relative.split_once('\\'))
                .is_some_and(|(directory, file)| {
                    directory
                        .strip_prefix("app-")
                        .is_some_and(|version| version.starts_with(|c: char| c.is_ascii_digit()))
                        && file == "claude.exe"
                })
            {
                return true;
            }
        }
    }
    let Some(family) = launch_uri
        .and_then(|uri| uri.strip_prefix("shell:AppsFolder\\"))
        .and_then(|uri| uri.split('!').next())
    else {
        return false;
    };
    let Some((name, publisher)) = family.rsplit_once('_') else {
        return false;
    };
    let Some((_, package_path)) = path.split_once("\\windowsapps\\") else {
        return false;
    };
    let Some((package, relative)) = package_path.split_once('\\') else {
        return false;
    };
    package.starts_with(&format!("{}_", name.to_ascii_lowercase()))
        && package.ends_with(&format!("_{}", publisher.to_ascii_lowercase()))
        && matches!(relative, "claude.exe" | "app\\claude.exe")
}

/// Account storage must not be replaced while its owning desktop app is alive.
pub async fn desktop_tool_is_running(tool_id: &str) -> Result<bool, String> {
    let path = super::tool_manager::get_tool_exe_path(tool_id).unwrap_or_default();
    #[cfg(windows)]
    if tool_id == "claudedesktop" {
        let launch_uri = super::tool_manager::get_tool_launch_uri(tool_id);
        return tokio::task::spawn_blocking(move || {
            qoder_process_snapshot().map(|processes| {
                processes.iter().any(|process| {
                    claude_desktop_windows_process(
                        &process.path,
                        Some(&path),
                        launch_uri.as_deref(),
                    )
                })
            })
        })
        .await
        .map_err(|_| "accountError.closeClient")?;
    }
    if matches!(tool_id, "qoder" | "qodercn" | "qwenwork" | "qwenworkcn") {
        let tool_id = tool_id.to_owned();
        return tokio::task::spawn_blocking(move || {
            let processes = qoder_process_snapshot()?;
            qoder_account_instances(&tool_id, std::path::Path::new(&path), processes)
                .map(|processes| !processes.is_empty())
        })
        .await
        .map_err(|_| "accountError.closeClient")?;
    }
    #[cfg(not(windows))]
    if path.is_empty() {
        return Err("accountError.unavailable".into());
    }
    #[cfg(windows)]
    {
        let script = "$ErrorActionPreference='Stop'; try { $root=$env:ECHOBIRD_DESKTOP_ROOT; $family=$env:ECHOBIRD_DESKTOP_FAMILY; $hits=Get-CimInstance Win32_Process -ErrorAction Stop | Where-Object { ($_.Name -eq $env:ECHOBIRD_DESKTOP_NAME -or ($env:ECHOBIRD_KIRO_CLI_RUNTIME -eq '1' -and $_.Name -match '^chat-cli-[0-9.]+\\.exe$')) -and $_.ExecutablePath -and (($root -and $_.ExecutablePath.StartsWith($root,[StringComparison]::OrdinalIgnoreCase)) -or ($family -and $_.ExecutablePath -match ('\\\\WindowsApps\\\\'+[regex]::Escape($family)+'_[^\\\\]+\\\\'))) }; if($hits){exit 0}else{exit 1} } catch { exit 2 }";
        let root = std::path::Path::new(&path)
            .parent()
            .map(|p| format!("{}\\", p.display()))
            .unwrap_or_default();
        let family = super::tool_manager::get_tool_launch_uri(tool_id)
            .and_then(|uri| {
                uri.strip_prefix("shell:AppsFolder\\")
                    .and_then(|s| s.split_once('_'))
                    .map(|(s, _)| s.to_owned())
            })
            .unwrap_or_default();
        let process_names = super::tool_manager::get_tool_process_names(tool_id);
        let status = crate::utils::process::async_command("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", script])
            .env(
                "ECHOBIRD_DESKTOP_NAME",
                std::path::Path::new(&path)
                    .file_name()
                    .and_then(|s| s.to_str())
                    .or_else(|| process_names.first().map(String::as_str))
                    .ok_or("accountError.unavailable")?,
            )
            .env("ECHOBIRD_DESKTOP_ROOT", root)
            .env("ECHOBIRD_DESKTOP_FAMILY", family)
            .env(
                "ECHOBIRD_KIRO_CLI_RUNTIME",
                if tool_id == "kirocli" { "1" } else { "0" },
            )
            .status()
            .await
            .map_err(|_| "accountError.closeClient")?;
        match status.code() {
            Some(0) => Ok(true),
            Some(1) => Ok(false),
            _ => Err("accountError.closeClient".into()),
        }
    }
    #[cfg(target_os = "macos")]
    {
        let output = crate::utils::process::async_command("/bin/ps")
            .args(["-A", "-o", "command="])
            .output()
            .await
            .map_err(|_| "accountError.closeClient")?;
        if !output.status.success() {
            return Err("accountError.closeClient".into());
        }
        Ok(String::from_utf8_lossy(&output.stdout)
            .lines()
            .any(|line| line.trim_start().starts_with(&path)))
    }
    #[cfg(target_os = "linux")]
    {
        if tool_id == "copilotdesktop"
            && std::path::Path::new(&path)
                .extension()
                .is_some_and(|extension| extension == "AppImage")
        {
            return Ok(
                crate::services::copilot_accounts::desktop::verified_appimage_pid(
                    std::path::Path::new(&path),
                )
                .is_some(),
            );
        }
        let executable = std::fs::canonicalize(&path).map_err(|_| "accountError.closeClient")?;
        for entry in std::fs::read_dir("/proc")
            .map_err(|_| "accountError.closeClient")?
            .flatten()
        {
            if entry
                .file_name()
                .to_str()
                .is_some_and(|s| s.bytes().all(|c| c.is_ascii_digit()))
                && std::fs::read_link(entry.path().join("exe")).is_ok_and(|p| {
                    p == executable
                        || (tool_id == "kirocli" && p == executable.with_file_name("kiro-cli-chat"))
                        || (tool_id == "kirocrew"
                            && super::tool_manager::get_tool_process_names(tool_id)
                                .iter()
                                .any(|name| {
                                    p.file_name().is_some_and(|value| value == name.as_str())
                                })
                            && std::fs::read(entry.path().join("environ")).is_ok_and(
                                |environment| appimage_matches(&environment, &executable),
                            ))
                })
            {
                return Ok(true);
            }
        }
        Ok(false)
    }
    #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
    {
        let _ = path;
        Err("accountError.unavailable".into())
    }
}

#[cfg(target_os = "linux")]
fn appimage_matches(environment: &[u8], executable: &std::path::Path) -> bool {
    use std::os::unix::ffi::OsStrExt;
    environment
        .split(|byte| *byte == 0)
        .filter_map(|entry| entry.strip_prefix(b"APPIMAGE="))
        .any(|path| {
            std::fs::canonicalize(std::ffi::OsStr::from_bytes(path))
                .is_ok_and(|path| path == executable)
        })
}

/// Release native desktop storage before an explicit model or account change. Uses the same
/// process scope as launch/restart; it never targets standalone CLI instances.
pub(crate) async fn stop_desktop_for_config(tool_id: &str) {
    if !crate::services::tool_manager::is_managed_desktop_tool(tool_id) {
        return;
    }
    stop_desktop_instances(tool_id).await;
}

/// Explicit account changes also own storage in no-model-config desktop clients.
/// Ordinary launch behavior remains governed by is_managed_desktop_tool.
pub(crate) async fn stop_desktop_for_account(tool_id: &str) -> Result<(), String> {
    stop_desktop_instances(tool_id).await;
    if desktop_tool_is_running(tool_id)
        .await
        .map_err(|_| "accountError.stopClient")?
    {
        return Err("accountError.stopClient".into());
    }
    Ok(())
}

async fn stop_desktop_instances(tool_id: &str) {
    let mgr = get_manager().await;
    if mgr.lock().await.kill_desktop_instances(tool_id) {
        tokio::time::sleep(std::time::Duration::from_millis(800)).await;
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn cursor_cli_rejects_other_product_commands_before_reading_configuration_or_spawning() {
        let mut manager = super::ProcessManager::new();
        for command in ["agent", "agent update", "grok", "Cursor.exe"] {
            assert!(manager
                .start_cli_tool(
                    "cursorcli",
                    command,
                    None,
                    crate::services::cli_launch::AUTO_TERMINAL
                )
                .is_err());
        }
        assert!(manager.processes.is_empty());
    }
    #[test]
    fn claude_desktop_shutdown_excludes_standalone_cli_and_supports_store_and_squirrel() {
        use super::claude_desktop_windows_process;
        let executable = Some(r"C:\Users\fixture\AppData\Local\AnthropicClaude\Claude.exe");
        let uri = Some(r"shell:AppsFolder\Claude_pzs8sxrjxfjjc!Claude");
        for (path, expected) in [
            (
                r"c:/Users/fixture/AppData/Local/AnthropicClaude/claude.exe",
                true,
            ),
            (
                r"C:\Users\fixture\AppData\Local\AnthropicClaude\app-1.2.3\Claude.exe",
                true,
            ),
            (r"C:\Users\fixture\.local\bin\claude.exe", false),
            (
                r"C:\Users\fixture\AppData\Local\AnthropicClaude\resources\claude.exe",
                false,
            ),
            (
                r"C:\Users\fixture\AppData\Local\AnthropicClaude\app-1.2.3\resources\claude.exe",
                false,
            ),
            (r"C:\Other\Claude.exe", false),
            (
                r"C:\Program Files\WindowsApps\Claude_1.2.3.0_x64__pzs8sxrjxfjjc\app\Claude.exe",
                true,
            ),
            (
                r"D:\WindowsApps\Claude_1.2.3.0_x64__pzs8sxrjxfjjc\Claude.exe",
                true,
            ),
            (
                r"C:\Program Files\WindowsApps\Claude_1.2.3.0_x64__other\app\Claude.exe",
                false,
            ),
            (
                r"C:\Program Files\WindowsApps\Claude_1.2.3.0_x64__pzs8sxrjxfjjc\resources\claude.exe",
                false,
            ),
        ] {
            assert_eq!(
                claude_desktop_windows_process(std::path::Path::new(path), executable, uri),
                expected,
                "{path}"
            );
        }
        assert!(!claude_desktop_windows_process(
            std::path::Path::new(r"C:\Other\Claude.exe"),
            None,
            None
        ));
        assert!(claude_desktop_windows_process(
            std::path::Path::new(
                r"C:\Program Files\WindowsApps\Claude_1.2.3.0_x64__pzs8sxrjxfjjc\app\Claude.exe"
            ),
            None,
            uri
        ));
        assert!(!claude_desktop_windows_process(
            std::path::Path::new(
                r"C:\Program Files\WindowsApps\Claude_1.2.3.0_x64__pzs8sxrjxfjjc\resources\claude.exe"
            ),
            None,
            uri
        ));
    }

    #[test]
    fn devin_cli_launch_uses_resolved_binary_and_preserves_custom_commands() {
        let path = "C:/Program Files/Devin/resources/app/extensions/windsurf/devin/bin/devin.exe";
        assert_eq!(
            super::devin_literal_launch_path("devin", Some(path.into())).as_deref(),
            Some(path)
        );
        assert!(super::devin_literal_launch_path("devin --custom", Some(path.into())).is_none());
        assert!(super::devin_literal_launch_path("devin", None).is_none());
    }
    fn qoder_process_fixture(
        root: &std::path::Path,
        name: &str,
        edition: &str,
        app: bool,
    ) -> std::path::PathBuf {
        let directory = root.join(name);
        let executable = if cfg!(target_os = "macos") {
            directory.join("Qoder.app/Contents/MacOS/Qoder")
        } else if cfg!(windows) {
            directory.join("Qoder.exe")
        } else {
            directory.join("qoder")
        };
        let resources = if cfg!(target_os = "macos") {
            directory.join("Qoder.app/Contents/Resources")
        } else {
            directory.join("resources")
        };
        std::fs::create_dir_all(executable.parent().unwrap()).unwrap();
        std::fs::create_dir_all(&resources).unwrap();
        let product = if app {
            if edition == "qoder" {
                serde_json::json!({"productId":"qoder","appId":"com.qoder.app","dataDirectoryName":"com.qoder.app.stable","protocolScheme":"qoder"})
            } else {
                serde_json::json!({"productId":"qoder-cn","appId":"com.qodercn.app","dataDirectoryName":"com.qodercn.app.stable","protocolScheme":"qoder-cn"})
            }
        } else if edition == "qoder" {
            serde_json::json!({"nameShort":"Qoder","dataFolderName":".qoder","urlProtocol":"qoder"})
        } else {
            serde_json::json!({"nameShort":"QoderCN","dataFolderName":".qoder-cn","urlProtocol":"qoder-cn"})
        };
        std::fs::write(resources.join("product.json"), product.to_string()).unwrap();
        #[cfg(windows)]
        let source = super::resolve_cmd_exe();
        #[cfg(unix)]
        let source = which::which("sleep").unwrap();
        std::fs::copy(source, &executable).unwrap();
        executable
    }

    #[test]
    fn shared_account_processes_include_the_second_install_and_exclude_other_region_and_ide() {
        let root =
            std::env::temp_dir().join(format!("echobird-account-process-{}", uuid::Uuid::new_v4()));
        for (tool_id, opposite) in [("qoder", "qodercn"), ("qodercn", "qoder")] {
            for app in [true, false] {
                let selected = qoder_process_fixture(&root, "user", tool_id, app);
                let other_install = qoder_process_fixture(&root, "system", tool_id, app);
                let other_region = qoder_process_fixture(&root, "other-region", opposite, app);
                let other_product = qoder_process_fixture(&root, "other-product", tool_id, !app);
                let processes = [other_install, other_region, other_product]
                    .into_iter()
                    .enumerate()
                    .map(|(index, path)| super::QoderAccountProcess {
                        pid: index as u32 + 1,
                        path,
                    })
                    .collect();
                let matches =
                    super::qoder_account_instances(tool_id, &selected, processes).unwrap();
                assert_eq!(matches.len(), 1);
                assert_eq!(matches[0].pid, 1);
                assert!(super::qoder_account_instances(opposite, &selected, vec![]).is_err());
            }
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn two_install_running_fixture_stops_only_the_verified_shared_account_owner() {
        fn worker(executable: &std::path::Path) -> super::OwnedDesktopConfigProcess {
            let mut command = crate::utils::process::command(executable);
            #[cfg(windows)]
            command.args(["/c", "ping -n 60 127.0.0.1 >nul"]);
            #[cfg(unix)]
            {
                use std::os::unix::process::CommandExt;
                command.arg("60").process_group(0);
            }
            super::OwnedDesktopConfigProcess {
                child: command.spawn().unwrap(),
                stopped: false,
            }
        }
        let root =
            std::env::temp_dir().join(format!("echobird-account-running-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        #[cfg(unix)]
        let root = std::fs::canonicalize(root).unwrap();
        let selected = qoder_process_fixture(&root, "user", "qoder", true);
        let mut owner = worker(&qoder_process_fixture(&root, "system", "qoder", true));
        let mut region_control = worker(&qoder_process_fixture(&root, "cn", "qodercn", true));
        let mut ide_control = worker(&qoder_process_fixture(&root, "ide", "qoder", false));
        let snapshot = super::qoder_process_snapshot().unwrap();
        for child in [&owner, &region_control, &ide_control] {
            assert!(snapshot.iter().any(|process| process.pid == child.id()));
        }
        // Restrict fixture input before matching so this test cannot target user clients.
        let fixture = snapshot
            .into_iter()
            .filter(|process| process.path.starts_with(&root))
            .collect();
        let matches = super::qoder_account_instances("qoder", &selected, fixture).unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].pid, owner.id());
        assert!(super::stop_qoder_account_instances(&matches));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while owner.child.try_wait().unwrap().is_none() {
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        assert!(region_control.child.try_wait().unwrap().is_none());
        assert!(ide_control.child.try_wait().unwrap().is_none());
        owner.stopped = true;
        region_control.stop().unwrap();
        ide_control.stop().unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn qoder_device_identity_requires_native_values_and_keeps_regions_separate() {
        let valid = serde_json::json!({"machineToken":"sdk-token","machineType":"sdk-type","machineCode":"sdk-code","other":"ignored"});
        let identity = super::parse_qoder_identity(valid.to_string().as_bytes()).unwrap();
        assert_eq!(identity.machine_token, "sdk-token");
        assert_eq!(identity.machine_type, "sdk-type");
        assert_eq!(identity.machine_code, "sdk-code");
        for field in ["machineToken", "machineType", "machineCode"] {
            for value in [
                serde_json::json!(null),
                serde_json::json!(""),
                serde_json::json!("token\ninvalid"),
                serde_json::json!("x".repeat(4097)),
            ] {
                let mut invalid = valid.clone();
                invalid[field] = value;
                assert!(super::parse_qoder_identity(invalid.to_string().as_bytes()).is_err());
            }
        }
        for platform in ["windows", "macos", "linux"] {
            for (tool, environment) in [("qoder", "3"), ("qodercn", "0")] {
                let args = super::qoder_identity_arguments(platform, tool).unwrap();
                assert_eq!(args[0], environment);
                assert_eq!(args.len(), if platform == "linux" { 1 } else { 2 });
                if platform != "linux" {
                    assert_eq!(args[1], "--account-stdin");
                }
            }
        }
        assert!(super::qoder_identity_arguments("windows", "unknown").is_err());
    }

    #[tokio::test]
    async fn qoder_device_identity_reads_only_the_first_bounded_line_from_an_owned_helper() {
        #[cfg(windows)]
        let command = {
            let mut command = crate::utils::process::async_command("powershell.exe");
            command.args(["-NoProfile","-NonInteractive","-Command", "$taskInput = [Console]::In.ReadLine() | ConvertFrom-Json; if ($taskInput.account -ne 'fixture-user') { exit 7 }; [Console]::Out.WriteLine('{\"machineToken\":\"fixture-token\",\"machineType\":\"fixture-type\",\"machineCode\":\"fixture-code\"}'); Start-Sleep -Seconds 30"]);
            command
        };
        #[cfg(not(windows))]
        let command = {
            let mut command = crate::utils::process::async_command("/bin/sh");
            command.args(["-c", "read input; case \"$input\" in *fixture-user*) ;; *) exit 7;; esac; printf '%s\\n' '{\"machineToken\":\"fixture-token\",\"machineType\":\"fixture-type\",\"machineCode\":\"fixture-code\"}'; exec sleep 30"]);
            command
        };
        // A cold PowerShell start alone can take this long on a busy machine.
        let identity = tokio::time::timeout(
            std::time::Duration::from_secs(60),
            super::read_qoder_identity(command, Some("fixture-user")),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(identity.machine_token, "fixture-token");
        assert_eq!(identity.machine_type, "fixture-type");
        assert_eq!(identity.machine_code, "fixture-code");
    }
    #[test]
    fn minimax_launch_uses_verified_cli_and_preserves_custom_commands() {
        let path = "/Users/alice/Tool Dir/it's/mcode";
        for command in ["mcode", path] {
            assert_eq!(
                super::minimax_literal_launch_path(command, Some(path.into())).as_deref(),
                Some(path)
            );
        }
        assert!(super::minimax_literal_launch_path("mcode --custom", Some(path.into())).is_none());
        assert!(super::minimax_literal_launch_path("mcode", None).is_none());
        assert_eq!(
            super::shell_single_quote(path),
            "'/Users/alice/Tool Dir/it'\\''s/mcode'"
        );
    }
    #[test]
    fn copilot_literal_launch_changes_only_default_or_exact_native_commands() {
        let executable = "/Users/alice/Tool Dir/it's/copilot";
        for command in ["copilot", executable] {
            let resolved = super::copilot_literal_launch_path(command, Some(executable.into()));
            assert_eq!(resolved.as_deref(), Some(executable));
            assert_eq!(
                super::shell_single_quote(resolved.as_deref().unwrap()),
                "'/Users/alice/Tool Dir/it'\\''s/copilot'"
            );
        }
        for custom in [
            "copilot --model local",
            "env CUSTOM=true copilot",
            "other-tool",
        ] {
            assert!(super::copilot_literal_launch_path(custom, Some(executable.into())).is_none());
        }
        assert!(super::copilot_literal_launch_path("copilot", None).is_none());
    }

    #[cfg(windows)]
    #[test]
    fn copilot_windows_native_launch_keeps_the_resolved_path_literal() {
        let native = r"C:\Tool Dir & fixture\copilot.exe";
        let command = super::copilot_native_command(native);
        assert_eq!(command.get_program(), std::ffi::OsStr::new(native));
        assert!(command.get_args().next().is_none());
        let wrapper = r"C:\Tool Dir & fixture\copilot.cmd";
        assert_eq!(
            super::copilot_native_command(wrapper)
                .get_args()
                .collect::<Vec<_>>(),
            [
                std::ffi::OsStr::new("/d"),
                std::ffi::OsStr::new("/v:off"),
                std::ffi::OsStr::new("/s"),
                std::ffi::OsStr::new("/c"),
                std::ffi::OsStr::new("\"\"%ECHOBIRD_COPILOT_NATIVE_WRAPPER%\"\"")
            ]
        );
        assert!(super::copilot_native_command(wrapper)
            .get_envs()
            .any(|(name, value)| name == "ECHOBIRD_COPILOT_NATIVE_WRAPPER"
                && value == Some(std::ffi::OsStr::new(wrapper))));
    }

    #[cfg(windows)]
    #[test]
    fn copilot_windows_wrapper_executes_an_owned_path_with_spaces_and_ampersand() {
        let root = std::env::temp_dir().join(format!(
            "copilot wrapper & %ECHOBIRD_COPILOT_WRAPPER_PROBE% ! fixture {}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir(&root).unwrap();
        let wrapper = root.join("copilot.cmd");
        std::fs::write(
            &wrapper,
            b"@echo off\r\necho echobird-copilot-wrapper-ok %*\r\n",
        )
        .unwrap();
        let results = [
            Vec::new(),
            vec!["login", "--web-flow"],
            vec![
                "--headless",
                "--stdio",
                "--no-auto-update",
                "--no-auto-login",
            ],
        ]
        .map(|arguments| {
            let output = super::copilot_native_command(&wrapper.to_string_lossy())
                .args(&arguments)
                .env("ECHOBIRD_COPILOT_WRAPPER_PROBE", "expanded-would-break")
                .output();
            (arguments, output)
        });
        std::fs::remove_file(wrapper).unwrap();
        std::fs::remove_dir(root).unwrap();
        for (arguments, output) in results {
            let output = output.unwrap();
            assert!(output.status.success());
            assert_eq!(
                String::from_utf8(output.stdout).unwrap().trim(),
                format!("echobird-copilot-wrapper-ok {}", arguments.join(" ")).trim()
            );
        }
    }

    #[test]
    fn new_desktop_restart_patterns_match_the_native_executable_and_exclude_other_apps() {
        for path in [
            "/Applications/GitHub Copilot.app/Contents/MacOS/github",
            "/Applications/Kilo.app/Contents/MacOS/kilo-desktop",
            "/home/alice/Apps/Kilo (custom).app/Contents/MacOS/native-kilo",
            "/opt/kilo-desktop/kilo-desktop",
            "/Applications/CodeArts.app/Contents/MacOS/CodeArts",
        ] {
            let pattern = regex::Regex::new(&super::executable_process_pattern(path)).unwrap();
            assert!(pattern.is_match(path));
            assert!(pattern.is_match(&format!("{path} --native-argument")));
            assert!(!pattern.is_match(&format!("{path}-other")));
            assert!(!pattern.is_match(&format!("/other{path}")));
            assert!(!pattern.is_match("/Applications/Other.app/Contents/MacOS/github"));
        }
    }

    #[cfg(windows)]
    async fn owned_visible_window_status(
        owned: &super::OwnedDesktopConfigProcess,
        executable: &str,
    ) -> Result<std::process::ExitStatus, String> {
        let (created, _) = super::owned_windows_process_times(&owned.child)?;
        let script = r#"
$ErrorActionPreference='Stop'
try {
  Add-Type 'using System; using System.Runtime.InteropServices; public static class KiloProbeWindow { private delegate bool Callback(IntPtr h, IntPtr p); [DllImport("user32.dll", SetLastError=true)] private static extern bool EnumWindows(Callback c, IntPtr p); [DllImport("user32.dll")] private static extern bool IsWindowVisible(IntPtr h); [DllImport("user32.dll")] private static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid); public static bool Visible(uint process) { bool found=false; Callback callback=(window,unused)=>{uint owner; GetWindowThreadProcessId(window,out owner); if(owner==process && IsWindowVisible(window)){found=true;} return true;}; if(!EnumWindows(callback,IntPtr.Zero)){throw new InvalidOperationException();} return found; } }'
  $expectedExe=[IO.Path]::GetFullPath($env:ECHOBIRD_KILO_GUI_EXE)
  $deadline=[DateTime]::UtcNow.AddSeconds(20)
  do {
    $process=Get-CimInstance Win32_Process -Filter ('ProcessId='+$env:ECHOBIRD_KILO_GUI_PID) -ErrorAction Stop
    if(-not $process){exit 2}
    $created=[long][decimal]::Truncate(([decimal]($process.CreationDate.ToFileTimeUtc()))/10)
    if($created -ne [long]$env:ECHOBIRD_KILO_GUI_CREATED -or -not $process.ExecutablePath -or -not $process.ExecutablePath.Equals($expectedExe,[StringComparison]::OrdinalIgnoreCase)){exit 3}
    if([KiloProbeWindow]::Visible([uint32]$env:ECHOBIRD_KILO_GUI_PID)){exit 0}
    Start-Sleep -Milliseconds 200
  }while([DateTime]::UtcNow -lt $deadline)
  exit 1
} catch {exit 4}
"#;
        crate::utils::process::async_command("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-Command", script])
            .env("ECHOBIRD_KILO_GUI_PID", owned.id().to_string())
            .env("ECHOBIRD_KILO_GUI_CREATED", created.to_string())
            .env("ECHOBIRD_KILO_GUI_EXE", executable)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .await
            .map_err(|_| "owned GUI detector could not start".into())
    }

    #[cfg(windows)]
    #[tokio::test]
    #[ignore = "explicit isolated Kilo Desktop GUI launch; ECHOBIRD_LIVE_KILO_GUI=1 and closed clients required"]
    async fn live_isolated_kilo_desktop_gui_launch_probe() -> Result<(), String> {
        use sha2::{Digest, Sha256};
        use std::{fs, path::PathBuf, process::Stdio};

        if std::env::var("ECHOBIRD_LIVE_KILO_GUI").as_deref() != Ok("1") {
            return Err("explicit Kilo GUI probe flag is required".into());
        }
        let status = crate::utils::process::async_command("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-Command",
                "$ErrorActionPreference='Stop'; try { if(Get-CimInstance Win32_Process -ErrorAction Stop | Where-Object { $_.Name -in @('kilo.exe','kilo-desktop.exe') }){exit 1}else{exit 0} } catch {exit 2}"])
            .status().await.map_err(|_| "Kilo process inventory failed")?;
        if status.code() != Some(0) {
            return Err("Kilo clients must be closed before the GUI probe".into());
        }
        let executable = crate::services::tool_manager::get_tool_exe_path("kilodesktop")
            .ok_or("Kilo Desktop is not installed")?;
        let home = dirs::home_dir().ok_or("home is unavailable")?;
        let xdg = |name: &str, fallback: PathBuf| {
            std::env::var_os(name)
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
                .unwrap_or(fallback)
        };
        let cli = xdg("XDG_CONFIG_HOME", home.join(".config")).join("kilo");
        let desktop = dirs::config_dir()
            .ok_or("desktop data is unavailable")?
            .join("Kilo Desktop");
        let mut paths = vec![
            xdg("XDG_DATA_HOME", home.join(".local/share")).join("kilo/auth.json"),
            desktop.join("plugins/kilo-ui/model-preferences.json"),
            home.join(".echobird/kilo-accounts.json"),
            home.join(".echobird/kilo.json"),
        ];
        for directory in [cli, desktop.join("plugins/kilo-server/runtime/config/kilo")] {
            paths.extend(
                ["config.json", "kilo.json", "kilo.jsonc"].map(|name| directory.join(name)),
            );
        }
        let hashes = || -> Result<Vec<Option<Vec<u8>>>, String> {
            paths
                .iter()
                .map(|path| match fs::read(path) {
                    Ok(bytes) => Ok(Some(Sha256::digest(bytes).to_vec())),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
                    Err(_) => Err("GUI probe snapshot failed".into()),
                })
                .collect()
        };
        let before = hashes()?;
        let temporary =
            std::env::temp_dir().join(format!("echobird-kilo-gui-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&temporary).map_err(|_| "GUI probe temporary directory failed")?;
        let cleanup_temporary = || -> Result<(), String> {
            let resolved =
                fs::canonicalize(&temporary).map_err(|_| "GUI temporary path resolution failed")?;
            let parent = fs::canonicalize(std::env::temp_dir())
                .map_err(|_| "GUI temporary parent resolution failed")?;
            if resolved.parent() != Some(parent.as_path()) {
                return Err("GUI cleanup directory is outside its temporary parent".into());
            }
            fs::remove_dir_all(&resolved).map_err(|_| "GUI temporary cleanup failed".into())
        };
        let mut command = crate::utils::process::command(&executable);
        command
            .arg(format!(
                "--user-data-dir={}",
                temporary.join("desktop").display()
            ))
            .current_dir(
                std::path::Path::new(&executable)
                    .parent()
                    .ok_or("Kilo executable directory missing")?,
            )
            .env("XDG_CONFIG_HOME", temporary.join("config"))
            .env("XDG_DATA_HOME", temporary.join("data"))
            .env("XDG_CACHE_HOME", temporary.join("cache"))
            .env("XDG_STATE_HOME", temporary.join("state"))
            .env("KILO_AUTH_CONTENT", "{}")
            .env("KILO_CONFIG_CONTENT", "{}")
            // Official packaged-e2e switch also isolates the external Anaconda keyring.
            .env("ANACONDA_SKIP_KEYRING", "1")
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let control_executable = PathBuf::from(
            std::env::var_os("SystemRoot").ok_or("GUI control system directory missing")?,
        )
        .join("System32/WindowsPowerShell/v1.0/powershell.exe");
        let control_child = crate::utils::process::command(&control_executable)
            .args([
                "-STA",
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "$ErrorActionPreference='Stop'; Add-Type -AssemblyName System.Windows.Forms; $form=New-Object System.Windows.Forms.Form; $form.Text='EchoBird window detector'; $form.Width=240; $form.Height=100; $timer=New-Object System.Windows.Forms.Timer; $timer.Interval=60000; $timer.Add_Tick({$form.Close()}); $timer.Start(); try{[void]$form.ShowDialog()}finally{$timer.Dispose();$form.Dispose()}",
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| "GUI cleanup positive control failed")?;
        let mut control = super::OwnedDesktopConfigProcess {
            child: control_child,
            stopped: false,
        };
        let positive =
            owned_visible_window_status(&control, &control_executable.to_string_lossy()).await;
        if !positive.as_ref().is_ok_and(|status| status.success()) {
            control.stop()?;
            cleanup_temporary()?;
            let positive = positive?;
            return Err(format!(
                "owned Form detector positive control failed: exitCode={:?}",
                positive.code()
            ));
        }
        eprintln!("provider=Kilo stage=gui_detector_positive_control status=ok");
        let child = command
            .spawn()
            .map_err(|_| "isolated Kilo GUI spawn failed")?;
        let mut owned = super::OwnedDesktopConfigProcess {
            child,
            stopped: false,
        };
        let window = owned_visible_window_status(&owned, &executable).await;
        let root_exit = owned
            .child
            .try_wait()
            .map_err(|_| "owned GUI process state failed")?
            .and_then(|status| status.code());
        let stopped = owned.stop();
        let control_untouched = control
            .child
            .try_wait()
            .map_err(|_| "GUI control state failed")?
            .is_none();
        control.stop()?;
        let after = hashes()?;
        if stopped.is_err() {
            return Err("owned Kilo GUI cleanup failed".into());
        }
        cleanup_temporary()?;
        if before != after {
            return Err("isolated GUI launch changed official native files".into());
        }
        if !control_untouched {
            return Err("owned GUI cleanup affected the unrelated process positive control".into());
        }
        let window = window?;
        if !window.success() {
            return Err(format!(
                "owned Kilo GUI visible-window verification failed: detectorExitCode={:?} rootExitCode={root_exit:?} (1=timeout, 2=rootGone, 3=identityMismatch, 4=instrumentError)",
                window.code()
            ));
        }
        eprintln!("provider=Kilo stage=gui_owned_window status=ok");
        eprintln!("provider=Kilo stage=gui_owned_cleanup status=ok unrelatedProcessUnchanged=true");
        eprintln!("provider=Kilo stage=gui_preservation status=ok officialFilesHashUnchanged=true");
        Ok(())
    }

    #[cfg(any(windows, unix))]
    #[test]
    fn temporary_config_process_cleanup_preserves_an_unowned_process() {
        fn worker() -> std::process::Child {
            #[cfg(windows)]
            let mut command = crate::utils::process::command("powershell.exe");
            #[cfg(windows)]
            command.args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "Start-Sleep -Seconds 60",
            ]);
            #[cfg(unix)]
            let mut command = crate::utils::process::command("sleep");
            #[cfg(unix)]
            {
                use std::os::unix::process::CommandExt;
                command.arg("60").process_group(0);
            }
            command.spawn().unwrap()
        }
        let mut owned = super::OwnedDesktopConfigProcess {
            child: worker(),
            stopped: false,
        };
        let mut unowned = super::OwnedDesktopConfigProcess {
            child: worker(),
            stopped: false,
        };
        assert!(owned.child.try_wait().unwrap().is_none());
        assert!(unowned.child.try_wait().unwrap().is_none());
        owned.stop().unwrap();
        assert!(owned.child.try_wait().unwrap().is_some());
        assert!(unowned.child.try_wait().unwrap().is_none());
        unowned.stop().unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn temporary_cleanup_stops_owned_group_after_the_leader_exits() {
        use std::io::BufRead;
        use std::os::unix::process::CommandExt;

        let child = crate::utils::process::command("/bin/sh")
            .args(["-c", "sleep 60 & printf '%s\\n' \"$!\"; exit 0"])
            .process_group(0)
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let mut owned = super::OwnedDesktopConfigProcess {
            child,
            stopped: false,
        };
        let mut line = String::new();
        std::io::BufReader::new(owned.child.stdout.take().unwrap())
            .read_line(&mut line)
            .unwrap();
        let descendant: i32 = line.trim().parse().unwrap();
        let mut status = unsafe { std::mem::zeroed::<libc::siginfo_t>() };
        // Observe exit without freeing the leader's PID before group cleanup.
        assert_eq!(
            unsafe {
                libc::waitid(
                    libc::P_PID,
                    owned.id(),
                    &mut status,
                    libc::WEXITED | libc::WNOWAIT,
                )
            },
            0
        );
        assert_eq!(unsafe { libc::kill(descendant, 0) }, 0);
        let other = crate::utils::process::command("sleep")
            .arg("60")
            .process_group(0)
            .spawn()
            .unwrap();
        let mut unowned = super::OwnedDesktopConfigProcess {
            child: other,
            stopped: false,
        };

        owned.stop().unwrap();
        assert!(owned.child.try_wait().unwrap().is_some());
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        loop {
            let output = crate::utils::process::command("/bin/ps")
                .args(["-o", "stat=", "-p", &descendant.to_string()])
                .output()
                .unwrap();
            let state = String::from_utf8_lossy(&output.stdout);
            if state.trim().is_empty() || state.trim_start().starts_with('Z') {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "owned helper survived"
            );
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(unowned.child.try_wait().unwrap().is_none());
        unowned.stop().unwrap();
        owned.stop().unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn temporary_cleanup_rejects_a_different_root_creation_identity() {
        let child = crate::utils::process::command("powershell.exe")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "Start-Sleep -Seconds 60",
            ])
            .spawn()
            .unwrap();
        let mut owned = super::OwnedDesktopConfigProcess {
            child,
            stopped: false,
        };
        let (created, exited) = super::owned_windows_process_times(&owned.child).unwrap();
        assert!(super::stop_owned_windows_tree(owned.id(), created + 1_000_000, exited).is_err());
        assert!(owned.child.try_wait().unwrap().is_none());
        owned.stop().unwrap();
        owned.stop().unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn temporary_cleanup_reaps_owned_descendants_after_the_parent_exits() {
        use std::io::BufRead;
        let child = crate::utils::process::command("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-Command", "$worker=Start-Process powershell.exe -ArgumentList '-NoProfile','-NonInteractive','-Command','Start-Sleep -Seconds 60' -WindowStyle Hidden -PassThru; Write-Output $worker.Id"])
            .stdout(std::process::Stdio::piped())
            .spawn().unwrap();
        let mut owned = super::OwnedDesktopConfigProcess {
            child,
            stopped: false,
        };
        let mut line = String::new();
        std::io::BufReader::new(owned.child.stdout.take().unwrap())
            .read_line(&mut line)
            .unwrap();
        let descendant: u32 = line.trim().parse().unwrap();
        owned.child.wait().unwrap();
        let other = crate::utils::process::command("powershell.exe")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "Start-Sleep -Seconds 60",
            ])
            .spawn()
            .unwrap();
        let mut unowned = super::OwnedDesktopConfigProcess {
            child: other,
            stopped: false,
        };
        owned.stop().unwrap();
        let result = crate::utils::process::command("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-Command", "if(Get-CimInstance Win32_Process -Filter ('ProcessId = '+$env:ECHOBIRD_FIXTURE_PID)){exit 1}else{exit 0}"])
            .env("ECHOBIRD_FIXTURE_PID", descendant.to_string()).status().unwrap();
        assert!(result.success());
        assert!(unowned.child.try_wait().unwrap().is_none());
        unowned.stop().unwrap();
    }

    use super::is_third_party_codex_base_url;

    #[test]
    fn kilo_models_are_individual_arguments_and_official_selection_has_no_override() {
        let runtime = if cfg!(windows) {
            r"C:\Kilo CLI\kilo.exe"
        } else {
            "/tmp/Kilo CLI/kilo"
        };
        for model in [
            "normal",
            "vendor/model",
            "space model",
            "'quoted'\"model",
            "&echo marker",
            "%PATH%",
            "$(echo marker)",
        ] {
            let command = super::kilo_native_command(runtime, Some(model));
            assert_eq!(command.get_program(), std::ffi::OsStr::new(runtime));
            assert_eq!(
                command.get_args().collect::<Vec<_>>(),
                vec![
                    std::ffi::OsStr::new("--model"),
                    std::ffi::OsStr::new(&format!("echobird/{model}"))
                ]
            );
        }
        assert_eq!(
            super::shell_single_quote("echobird/space & %PATH% $(echo marker)"),
            "'echobird/space & %PATH% $(echo marker)'"
        );
        assert_eq!(
            super::shell_single_quote("echobird/a'b"),
            "'echobird/a'\\''b'"
        );
        assert!(super::kilo_native_command(runtime, None)
            .get_args()
            .next()
            .is_none());
    }

    #[cfg(unix)]
    #[test]
    fn kilo_posix_shell_preserves_a_single_literal_model_argument() {
        for model in [
            "vendor/model",
            "space 'quote' \"double\" & %PATH% $(echo marker)",
        ] {
            let argument = format!("echobird/{model}");
            let output = std::process::Command::new("/bin/sh")
                .args([
                    "-c",
                    &format!("printf '%s' {}", super::shell_single_quote(&argument)),
                ])
                .output()
                .unwrap();
            assert!(output.status.success());
            assert_eq!(String::from_utf8(output.stdout).unwrap(), argument);
        }
    }

    #[cfg(windows)]
    use super::openscience_windows_data_dir_for;

    /// Opens one real console per installed shell, so it runs on demand only:
    /// `cargo test windows_shell_terminal -- --ignored`.
    #[cfg(windows)]
    #[test]
    #[ignore]
    fn windows_shell_terminal_runs_commands_in_each_shell() {
        use std::time::{Duration, Instant};
        let dir = std::env::temp_dir().join(format!("echobird-shells-{}", uuid::Uuid::new_v4()));
        let cwd = dir.join("项目 folder & O'Brien");
        std::fs::create_dir_all(&cwd).unwrap();
        let powershell = "Set-Content -LiteralPath shell.txt -Value $PWD.Path; exit";
        let cases = [
            ("pwsh", powershell),
            ("powershell", powershell),
            ("cmd", "cd > shell.txt & exit"),
            ("git-bash", "cygpath -w \"$PWD\" > shell.txt; exit"),
        ];
        for (terminal, command) in cases {
            let Ok(Some(shell)) = crate::services::cli_launch::windows_shell(terminal) else {
                eprintln!("{terminal} is not installed; skipped");
                continue;
            };
            let output = super::windows_shell_terminal(&shell, command, &cwd)
                .output()
                .unwrap();
            assert!(output.status.success(), "{terminal}: {output:?}");
            let marker = cwd.join("shell.txt");
            let deadline = Instant::now() + Duration::from_secs(20);
            while !marker.exists() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(100));
            }
            std::thread::sleep(Duration::from_millis(300));
            let bytes = std::fs::read(&marker).unwrap_or_else(|_| panic!("{terminal} did not run"));
            let text = String::from_utf8_lossy(&bytes);
            assert!(!text.is_empty(), "{terminal} wrote nothing");
            std::fs::remove_file(marker).unwrap();
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn windows_cli_terminal_has_console_handles_with_a_piped_parent() {
        use std::os::windows::process::CommandExt;
        use std::time::{Duration, Instant};

        let dir =
            std::env::temp_dir().join(format!("echobird-cli-launch-{}", uuid::Uuid::new_v4()));
        let cwd = dir.join("项目 folder & O'Brien");
        std::fs::create_dir_all(&cwd).unwrap();
        let result = dir.join("result.json");
        std::fs::write(
            dir.join("probe.ps1"),
            r#"param([string]$Label)
Add-Type 'using System; using System.Runtime.InteropServices; public class ConsoleProbe { [DllImport("kernel32.dll")] public static extern IntPtr GetStdHandle(int id); [DllImport("kernel32.dll")] public static extern bool GetConsoleMode(IntPtr handle, out uint mode); }'
$handles = @(-10,-11,-12) | ForEach-Object { $mode=0; [ConsoleProbe]::GetConsoleMode([ConsoleProbe]::GetStdHandle($_),[ref]$mode) }
$parentPid = (Get-CimInstance Win32_Process -Filter "ProcessId=$PID").ParentProcessId
$result = @{ handles=$handles; cwd=$PWD.ProviderPath; label=$Label; marker=$env:ECHOBIRD_LAUNCH_FIXTURE; parentPid=$parentPid } | ConvertTo-Json -Compress
[IO.File]::WriteAllText($env:ECHOBIRD_LAUNCH_RESULT,$result)
"#,
        )
        .unwrap();
        std::fs::write(
            dir.join("probe.cmd"),
            "@echo off\r\n\"%SystemRoot%\\System32\\WindowsPowerShell\\v1.0\\powershell.exe\" -NoProfile -ExecutionPolicy Bypass -File \"%~dp0probe.ps1\" %*\r\n",
        )
        .unwrap();
        let marker = "argument space & O'Brien";
        let mut handles = Vec::new();
        for (tool_id, command, independent, label) in [
            ("fixture-other", "probe".to_string(), false, ""),
            ("musecode", format!("probe \"{marker}\""), true, marker),
            (
                "musecode",
                format!("\"{}\" \"{marker}\"", dir.join("probe.cmd").display()),
                true,
                marker,
            ),
            ("claudecode", "probe".to_string(), true, ""),
            ("claudecode", format!("probe \"{marker}\""), true, marker),
            (
                "claudecode",
                format!("\"{}\" \"{marker}\"", dir.join("probe.cmd").display()),
                true,
                marker,
            ),
            ("omp", "probe".to_string(), true, ""),
            ("opencode", format!("probe \"{marker}\""), true, marker),
            (
                "fixture-future-cli",
                format!("\"{}\" \"{marker}\"", dir.join("probe.cmd").display()),
                true,
                marker,
            ),
        ] {
            let mut launcher = if independent {
                super::windows_cli_terminal(&command, &cwd)
            } else {
                let mut launcher = std::process::Command::new(super::resolve_cmd_exe());
                launcher.args(["/C", &command]).creation_flags(0x00000210);
                launcher
            };
            let mut path = dir.as_os_str().to_os_string();
            path.push(";");
            path.push(std::env::var_os("PATH").unwrap_or_default());
            let output = launcher
                .current_dir(&cwd)
                .env("PATH", path)
                .env("ECHOBIRD_LAUNCH_FIXTURE", "preserved")
                .env("ECHOBIRD_LAUNCH_RESULT", &result)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let child_pid = if independent {
                let pid = String::from_utf8_lossy(&output.stdout)
                    .trim()
                    .parse::<u32>()
                    .unwrap();
                let waited = crate::utils::process::command(launcher.get_program())
                    .args([
                        "-NoProfile",
                        "-NonInteractive",
                        "-Command",
                        "Get-Process -Id $env:ECHOBIRD_LAUNCH_PID -ErrorAction SilentlyContinue | Wait-Process -Timeout 10 -ErrorAction Stop",
                    ])
                    .env("ECHOBIRD_LAUNCH_PID", pid.to_string())
                    .output()
                    .unwrap();
                assert!(waited.status.success());
                Some(pid)
            } else {
                None
            };
            let deadline = Instant::now() + Duration::from_secs(10);
            let value: serde_json::Value = loop {
                if let Ok(value) = std::fs::read(&result)
                    .and_then(|bytes| serde_json::from_slice(&bytes).map_err(std::io::Error::other))
                {
                    break value;
                }
                assert!(Instant::now() < deadline, "console probe did not finish");
                std::thread::sleep(Duration::from_millis(50));
            };
            std::fs::remove_file(&result).unwrap();
            handles.push((
                tool_id,
                value["handles"].clone(),
                serde_json::json!([independent, independent, independent]),
            ));
            if let Some(pid) = child_pid {
                assert_eq!(value["parentPid"].as_u64(), Some(u64::from(pid)));
            }
            assert_eq!(
                std::fs::canonicalize(std::path::Path::new(value["cwd"].as_str().unwrap()))
                    .unwrap(),
                std::fs::canonicalize(&cwd).unwrap()
            );
            assert_eq!(value["label"], label);
            assert_eq!(value["marker"], "preserved");
        }
        let resolved = std::fs::canonicalize(&dir).unwrap();
        let temporary = std::fs::canonicalize(std::env::temp_dir()).unwrap();
        assert_eq!(resolved.parent(), Some(temporary.as_path()));
        std::fs::remove_dir_all(&resolved).unwrap();
        for (tool_id, actual, expected) in handles {
            assert_eq!(actual, expected, "{tool_id} terminal console handles");
        }
    }

    #[test]
    fn muse_managed_unix_command_pins_the_selected_config_after_login_shell_setup() {
        let selected = std::path::Path::new("/selected config/it's mine");
        assert_eq!(
            super::musecode_unix_command("muse", Some(selected), None),
            "XDG_CONFIG_HOME='/selected config/it'\\''s mine' muse"
        );
        assert_eq!(super::musecode_unix_command("muse", None, None), "muse");
        let executable = std::path::Path::new("/home/user's tools/.local/bin/muse");
        assert_eq!(
            super::musecode_unix_command("muse", Some(selected), Some(executable)),
            "XDG_CONFIG_HOME='/selected config/it'\\''s mine' '/home/user'\\''s tools/.local/bin/muse'"
        );
        assert_eq!(
            super::musecode_unix_command("muse", None, Some(executable)),
            "'/home/user'\\''s tools/.local/bin/muse'"
        );
        assert_eq!(
            super::musecode_unix_command("/custom/muse --model custom", None, Some(executable)),
            "/custom/muse --model custom"
        );
    }

    #[cfg(unix)]
    #[test]
    fn muse_unix_launch_uses_detected_executable_outside_path() {
        use std::os::unix::fs::PermissionsExt;

        let root = std::fs::canonicalize(std::env::temp_dir())
            .unwrap()
            .join(format!("echobird-muse-launch-{}", uuid::Uuid::new_v4()));
        let bin = root.join("user's tools");
        let cwd = root.join("项目 folder");
        let config = root.join("selected config");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::create_dir_all(&cwd).unwrap();
        let executable = bin.join("muse");
        std::fs::write(
            &executable,
            "#!/bin/sh\nprintf '%s\\n%s\\n' \"$PWD\" \"$XDG_CONFIG_HOME\"\n",
        )
        .unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        let output = std::process::Command::new("/bin/sh")
            .args([
                "-c",
                &super::musecode_unix_command("muse", Some(&config), Some(&executable)),
            ])
            .env("PATH", "/usr/bin:/bin")
            .env("XDG_CONFIG_HOME", "/other/config")
            .current_dir(&cwd)
            .output()
            .unwrap();
        assert!(output.status.success());
        assert_eq!(
            String::from_utf8(output.stdout).unwrap(),
            format!("{}\n{}\n", cwd.display(), config.display())
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn workbuddy_process_matching_is_scoped_to_the_resolved_executable() {
        for path in [
            "/Applications/WorkBuddy.app/Contents/MacOS/Electron",
            "/Applications/WorkBuddy AI.app/Contents/MacOS/Electron",
            "/Users/alice/Apps/WorkBuddy (test)+[1].app/Contents/MacOS/Electron",
        ] {
            let pattern = regex::Regex::new(&super::executable_process_pattern(path)).unwrap();
            assert!(pattern.is_match(path));
            assert!(pattern.is_match(&format!("{path} --remote-debugging-port=9222")));
            for other in [
                "Electron",
                "/Applications/Other.app/Contents/MacOS/Electron",
                "/Applications/WorkBuddy.app/Contents/MacOS/Electron",
                "/Applications/WorkBuddy AI.app/Contents/MacOS/Electron",
            ] {
                if other != path {
                    assert!(
                        !pattern.is_match(other),
                        "matched unrelated client: {other}"
                    );
                }
            }
            assert!(!pattern.is_match(&format!("{path}-other")));
            assert!(!pattern.is_match(&format!("/other{path}")));
            assert!(!pattern.is_match(&path.replace(".app", "Xapp")));
        }
    }

    #[test]
    fn codex_provider_classification_uses_domain_boundaries() {
        assert!(!is_third_party_codex_base_url(""));
        assert!(!is_third_party_codex_base_url("https://api.openai.com/v1"));
        assert!(is_third_party_codex_base_url(
            "https://api.openai.com.example/v1"
        ));
        assert!(is_third_party_codex_base_url("https://api.deepseek.com/v1"));
    }

    #[cfg(windows)]
    #[test]
    fn openscience_uses_short_per_user_drive_root_on_windows() {
        let alice = openscience_windows_data_dir_for(std::path::Path::new(r"C:\Users\Alice"));
        let bob = openscience_windows_data_dir_for(std::path::Path::new(r"C:\Users\Bob"));

        assert!(alice.starts_with(r"C:\OS"));
        assert!(bob.starts_with(r"C:\OS"));
        assert_ne!(alice, bob);
        assert_eq!(alice.file_name().unwrap().to_string_lossy().len(), 10);
    }

    #[test]
    fn qwenwork_process_matching_covers_shared_profile_installs_and_isolates_the_other_edition() {
        let root = std::env::temp_dir().join(format!("qwenwork-process-{}", uuid::Uuid::new_v4()));
        let mut paths = vec![];
        for (install, package) in [
            ("user", "qwen-work-cn"),
            ("system", "qwen-work-cn"),
            ("global", "qwen-work"),
            ("unrelated", "other-app"),
        ] {
            let base = root.join(install);
            let (exe, resources) = if cfg!(target_os = "macos") {
                (
                    base.join("Contents/MacOS/Client"),
                    base.join("Contents/Resources"),
                )
            } else {
                (base.join("Client.exe"), base.join("resources"))
            };
            std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
            std::fs::write(&exe, b"fixture").unwrap();
            std::fs::create_dir_all(&resources).unwrap();
            let content =
                serde_json::to_vec(&serde_json::json!({"name":package,"version":"1.0.0"})).unwrap();
            let header = serde_json::to_vec(
                &serde_json::json!({"files":{"package.json":{"offset":"0","size":content.len()}}}),
            )
            .unwrap();
            let size = (header.len() + 11) & !3;
            let mut bytes = vec![0u8; 8 + size];
            bytes[0..4].copy_from_slice(&4u32.to_le_bytes());
            bytes[4..8].copy_from_slice(&(size as u32).to_le_bytes());
            bytes[12..16].copy_from_slice(&(header.len() as u32).to_le_bytes());
            bytes[16..16 + header.len()].copy_from_slice(&header);
            bytes.extend(content);
            std::fs::write(resources.join("app.asar"), bytes).unwrap();
            paths.push(exe);
        }
        let snapshot = paths
            .iter()
            .enumerate()
            .map(|(i, path)| super::QoderAccountProcess {
                pid: i as u32 + 1,
                path: path.clone(),
            })
            .collect();
        let selected = super::qoder_account_instances("qwenworkcn", &paths[0], snapshot).unwrap();
        assert_eq!(
            selected.iter().map(|p| p.pid).collect::<Vec<_>>(),
            vec![1, 2]
        );
        assert!(super::qoder_account_instances("qwenwork", &paths[0], vec![]).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}
