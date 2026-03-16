// Channel commands — bridge process management + channel persistence
//
// Local channel (id=1) uses Bridge binary (plugins/openclaw/bridge-win.exe)
// as a persistent subprocess. Communication via stdin/stdout JSON (Echobird Bridge Protocol).
// Bridge binary is auto-downloaded if not present (macOS/Linux installs).

use crate::utils::platform::echobird_dir;
use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::Mutex;

/// Check if a port is already in use (TCP connect check).
fn is_port_in_use(port: u16) -> bool {
    let addr: std::net::SocketAddr = format!("127.0.0.1:{}", port).parse().unwrap();
    if std::net::TcpStream::connect_timeout(&addr, std::time::Duration::from_millis(500)).is_ok() {
        return true;
    }
    // Also try localhost (resolves to ::1 on some systems)
    let addr6: std::net::SocketAddr = format!("[::1]:{}", port).parse().unwrap();
    if std::net::TcpStream::connect_timeout(&addr6, std::time::Duration::from_millis(300)).is_ok() {
        return true;
    }
    false
}

/// Check if the OpenClaw gateway is already running — port check + process name fallback.
/// Handles cases where gateway listens on IPv6-only or a different loopback interface.
fn is_openclaw_gateway_running() -> bool {
    // Primary: fast port check on both IPv4 and IPv6 loopback
    if is_port_in_use(18789) {
        return true;
    }
    // Retry once after a short wait (gateway may still be binding)
    std::thread::sleep(std::time::Duration::from_millis(800));
    if is_port_in_use(18789) {
        return true;
    }
    // Secondary: process name check — catches cases where port detection fails
    // (e.g. OpenClaw listening on a non-loopback interface, or IPv6-only)
    #[cfg(target_os = "windows")]
    {
        // wmic covers Node-based CLI tools where the process is node.exe
        if let Ok(out) = Command::new("wmic")
            .args(["process", "where", "commandline like '%openclaw%gateway%'", "get", "processid", "/format:value"])
            .output()
        {
            let text = String::from_utf8_lossy(&out.stdout);
            if text.lines().any(|l| l.trim_start().starts_with("ProcessId=") && l.trim() != "ProcessId=") {
                log::info!("[Bridge] OpenClaw gateway detected via process list (wmic)");
                return true;
            }
        }
        // Fallback: tasklist for native binary installs
        if let Ok(out) = Command::new("tasklist").args(["/FO", "CSV", "/NH"]).output() {
            let text = String::from_utf8_lossy(&out.stdout).to_lowercase();
            if text.contains("openclaw") {
                log::info!("[Bridge] OpenClaw process detected via tasklist");
                return true;
            }
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        if let Ok(out) = Command::new("pgrep").args(["-f", "openclaw.*gateway"]).output() {
            if out.status.success() && !out.stdout.is_empty() {
                log::info!("[Bridge] OpenClaw gateway detected via pgrep");
                return true;
            }
        }
    }
    false
}


// ── Channel Config (persistence) ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelConfig {
    pub id: i32,
    pub name: String,
    pub protocol: String,
    pub address: String,
}

// ── Bridge Process (persistent subprocess for stdio-json protocol) ──

struct BridgeProcess {
    child: Child,
    stdin: ChildStdin,
    reader: BufReader<ChildStdout>,
    session_id: Option<String>,
    agent_name: Option<String>,
}

static BRIDGE_PROCESS: Mutex<Option<BridgeProcess>> = Mutex::new(None);

// ── Oneshot State (for cli-oneshot protocol — no persistent subprocess) ──

#[derive(Debug, Clone)]
#[allow(dead_code)]
struct OneshotState {
    plugin_id: String,
    plugin_name: String,
    cli_command: String,
    cli_args: Vec<String>,
    resume_args: Option<Vec<String>>,
    session_arg: Option<String>,
    system_prompt_arg: Option<String>,
    system_prompt_when: Option<String>,  // "new-session" | "always"
    session_id: Option<String>,
    prompt_injected: bool,  // track if system_prompt was already sent (for "new-session" mode)
}

static ONESHOT_STATE: Mutex<Option<OneshotState>> = Mutex::new(None);

/// Truncate a string at a safe UTF-8 char boundary
fn safe_truncate(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    // Walk backwards from max_bytes to find a char boundary
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

// ── Channel Persistence Commands ──

/// Get saved channels from channels.json
#[tauri::command]
pub fn get_channels() -> Vec<ChannelConfig> {
    let path = echobird_dir().join("channels.json");
    if !path.exists() {
        return Vec::new();
    }
    match std::fs::read_to_string(&path) {
        Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
        Err(e) => {
            log::warn!("[Channels] Failed to read channels.json: {}", e);
            Vec::new()
        }
    }
}

/// Save channels to channels.json
#[tauri::command]
pub fn save_channels(channels: Vec<ChannelConfig>) -> Result<(), String> {
    let dir = echobird_dir();
    if !dir.exists() {
        std::fs::create_dir_all(&dir)
            .map_err(|e| format!("Failed to create config dir: {}", e))?;
    }
    let path = dir.join("channels.json");
    let content = serde_json::to_string_pretty(&channels)
        .map_err(|e| format!("Failed to serialize channels: {}", e))?;
    std::fs::write(&path, content)
        .map_err(|e| format!("Failed to write channels.json: {}", e))?;
    Ok(())
}

// ── Bridge Lifecycle Commands ──

/// Internal: start bridge (blocking — call from spawn_blocking)
fn start_bridge_internal(plugin_id: &str) -> Result<BridgeStartResult, String> {
    // Find bridge binary via plugin manager
    let plugins = crate::services::plugin_manager::scan_plugins();

    let exe_path = std::env::current_exe().unwrap_or_default();
    log::info!("[Bridge] exe path: {:?}, found {} plugins, target: {}", exe_path, plugins.len(), plugin_id);

    let plugin = plugins.iter().find(|p| p.id == plugin_id)
        .ok_or_else(|| format!(
            "Plugin '{}' not found. Exe: {:?}, scanned {} plugins: [{}]",
            plugin_id,
            exe_path,
            plugins.len(),
            plugins.iter().map(|p| p.id.as_str()).collect::<Vec<_>>().join(", ")
        ))?;

    let bridge_path = crate::services::plugin_manager::get_bridge_path(plugin)
        .ok_or_else(|| "Bridge binary not found for current platform.".to_string())?;

    log::info!("[Bridge] Starting bridge: {:?}", bridge_path);

    // Launch OpenClaw Gateway in a visible terminal window (like App Manager's LAUNCH APP)
    // Skip if gateway is already running (check port 18789)
    if let Some(cli) = &plugin.cli {
        let gateway_command = format!("{} gateway --allow-unconfigured", cli.command);

        // Check if gateway is already running — port check + process name fallback
        let already_running = is_openclaw_gateway_running();

        if already_running {
            log::info!("[Bridge] OpenClaw Gateway already running on port 18789, skipping launch");
        } else {
            log::info!("[Bridge] Launching OpenClaw Gateway: {}", gateway_command);

            #[cfg(target_os = "windows")]
            {
                use std::os::windows::process::CommandExt;
                const CREATE_NEW_PROCESS_GROUP: u32 = 0x00000200;
                const CREATE_NO_WINDOW: u32 = 0x08000000; // Silent background — no visible console
                let mut cmd = Command::new("cmd");
                cmd.args(["/C", &gateway_command]);
                cmd.creation_flags(CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW);
                match cmd.spawn() {
                    Ok(_) => log::info!("[Bridge] OpenClaw Gateway launched silently (Windows)"),
                    Err(e) => log::warn!("[Bridge] Could not launch Gateway: {}", e),
                }
            }

            #[cfg(target_os = "macos")]
            {
                // Open Terminal.app with the gateway command
                let script = format!(
                    "tell application \"Terminal\" to do script \"{}\"",
                    gateway_command
                );
                match Command::new("osascript").args(["-e", &script]).spawn() {
                    Ok(_) => log::info!("[Bridge] OpenClaw Gateway launched (macOS Terminal)"),
                    Err(e) => log::warn!("[Bridge] Could not launch Gateway: {}", e),
                }
            }

            #[cfg(target_os = "linux")]
            {
                // Try common terminal emulators
                let launched = [
                    ("gnome-terminal", vec!["--", "sh", "-c", &gateway_command]),
                    ("konsole", vec!["-e", "sh", "-c", &gateway_command]),
                    ("xfce4-terminal", vec!["-e", &gateway_command]),
                    ("xterm", vec!["-e", &gateway_command]),
                ].iter().any(|(term, args)| {
                    Command::new(term).args(args).spawn().is_ok()
                });

                if launched {
                    log::info!("[Bridge] OpenClaw Gateway launched (Linux terminal)");
                } else {
                    // Fallback: run in background without terminal
                    log::warn!("[Bridge] No terminal emulator found, running Gateway in background");
                    let parts: Vec<&str> = gateway_command.split_whitespace().collect();
                    if !parts.is_empty() {
                        let mut cmd = Command::new(parts[0]);
                        if parts.len() > 1 { cmd.args(&parts[1..]); }
                        let _ = cmd.spawn();
                    }
                }
            }

            // Brief pause to let gateway start
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
    }

    // Spawn bridge process with piped stdin/stdout
    let mut child = Command::new(&bridge_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to spawn bridge: {}", e))?;

    let stdin = child.stdin.take()
        .ok_or_else(|| "Failed to capture bridge stdin".to_string())?;
    let stdout = child.stdout.take()
        .ok_or_else(|| "Failed to capture bridge stdout".to_string())?;

    let mut reader = BufReader::new(stdout);

    // Read the initial status message from bridge
    // Bridge sends: {"type":"status","agent":"openclaw","version":"...","ready":true}
    let mut status_line = String::new();
    match reader.read_line(&mut status_line) {
        Ok(0) => {
            let _ = child.kill();
            return Err("Bridge process exited immediately".to_string());
        }
        Ok(_) => {
            log::info!("[Bridge] Status: {}", status_line.trim());
            // Verify it's a valid status message
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(status_line.trim()) {
                let ready = json.get("ready").and_then(|v| v.as_bool()).unwrap_or(false);
                if !ready {
                    let _ = child.kill();
                    return Err("Bridge reported not ready".to_string());
                }
            }
        }
        Err(e) => {
            let _ = child.kill();
            return Err(format!("Failed to read bridge status: {}", e));
        }
    }

    // Store the persistent process
    let agent_name = Some(plugin.name.clone());
    let bp = BridgeProcess {
        child,
        stdin,
        reader,
        session_id: None,
        agent_name: agent_name.clone(),
    };

    let mut guard = BRIDGE_PROCESS.lock().map_err(|e| format!("Lock error: {}", e))?;
    *guard = Some(bp);

    log::info!("[Bridge] Started successfully");
    Ok(BridgeStartResult {
        status: "connected".to_string(),
        error: None,
        agent_name: Some(plugin.name.clone()),
    })
}

/// Returns the platform+arch specific bridge binary filename for this machine.
fn local_bridge_binary_name() -> &'static str {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "aarch64") => "bridge-linux-aarch64",
        ("linux", _)         => "bridge-linux-x86_64",
        ("macos", "aarch64") => "bridge-darwin-aarch64",
        ("macos", _)         => "bridge-darwin-x86_64",
        _                    => "bridge-win.exe",
    }
}

/// Auto-download the bridge binary if not already present.
/// Tries dl.echobird.ai (Cloudflare, GFW-friendly) then GitHub Releases as fallback.
/// This is mandatory — bridge is required for Channels to work.
async fn download_bridge_if_missing(plugin_id: &str) -> Result<(), String> {
    use crate::services::plugin_manager::{plugins_dir, scan_plugins, get_bridge_path};

    // Check if already present
    let plugins = scan_plugins();
    if let Some(plugin) = plugins.iter().find(|p| p.id == plugin_id) {
        if get_bridge_path(plugin).is_some() {
            return Ok(()); // Already present
        }
    }

    let binary_name = local_bridge_binary_name();
    log::info!("[Bridge] Bridge binary '{}' not found — auto-downloading...", binary_name);

    // Fetch latest version from version API
    let version = {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(8))
            .build()
            .unwrap_or_default();
        match client
            .get("https://echobird.ai/api/version/index.json")
            .header("User-Agent", "Echobird/1.0")
            .send()
            .await
        {
            Ok(r) if r.status().is_success() => {
                if let Ok(json) = r.json::<serde_json::Value>().await {
                    json.get("version")
                        .and_then(|v| v.as_str())
                        .map(|v| format!("v{}", v))
                        .unwrap_or_else(|| format!("v{}", env!("CARGO_PKG_VERSION")))
                } else {
                    format!("v{}", env!("CARGO_PKG_VERSION"))
                }
            }
            _ => format!("v{}", env!("CARGO_PKG_VERSION")),
        }
    };

    // Destination path
    let dest_dir = plugins_dir().join(plugin_id);
    std::fs::create_dir_all(&dest_dir)
        .map_err(|e| format!("Failed to create plugin dir: {}", e))?;
    let dest_path = dest_dir.join(binary_name);

    // Try primary (Cloudflare) then GitHub fallback
    let primary_url  = format!("https://dl.echobird.ai/releases/{}/{}", version, binary_name);
    let fallback_url = format!("https://github.com/edison7009/Echobird-MotherAgent/releases/download/{}/{}", version, binary_name);

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .map_err(|e| format!("HTTP client error: {}", e))?;

    let mut last_error = String::new();
    for url in &[primary_url.as_str(), fallback_url.as_str()] {
        log::info!("[Bridge] Downloading from: {}", url);
        match client.get(*url).send().await {
            Ok(resp) if resp.status().is_success() => {
                match resp.bytes().await {
                    Ok(bytes) => {
                        std::fs::write(&dest_path, &bytes)
                            .map_err(|e| format!("Failed to write bridge binary: {}", e))?;
                        log::info!("[Bridge] Downloaded {} bytes → {:?}", bytes.len(), dest_path);
                        break;
                    }
                    Err(e) => { last_error = format!("Read error: {}", e); }
                }
            }
            Ok(resp) => { last_error = format!("HTTP {}: {}", resp.status(), url); }
            Err(e)   => { last_error = format!("Request error: {}", e); }
        }
    }

    if !dest_path.exists() {
        return Err(format!("Bridge download failed (tried {} and {}). Last error: {}",
            primary_url, fallback_url, last_error));
    }

    // Set executable permissions on Unix
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&dest_path, std::fs::Permissions::from_mode(0o755));
    }

    // Create a copy/symlink using the name expected by plugin.json (e.g. "bridge-darwin")
    // so get_bridge_path() can find it without arch suffix.
    let plugins = scan_plugins();
    if let Some(plugin) = plugins.iter().find(|p| p.id == plugin_id) {
        if let Some(bridge) = &plugin.bridge {
            let expected = if cfg!(target_os = "linux") { bridge.linux.as_deref() }
                else if cfg!(target_os = "macos")       { bridge.darwin.as_deref() }
                else                                     { bridge.win32.as_deref() };
            if let Some(name) = expected {
                let expected_path = dest_dir.join(name);
                if !expected_path.exists() {
                    #[cfg(unix)]
                    { let _ = std::os::unix::fs::symlink(&dest_path, &expected_path); }
                    #[cfg(windows)]
                    { let _ = std::fs::copy(&dest_path, &expected_path); }
                }
            }
        }
    }

    log::info!("[Bridge] Bridge binary ready: {:?}", dest_path);
    Ok(())
}

/// Start the Bridge binary as a persistent subprocess
#[tauri::command]
pub async fn bridge_start(plugin_id: Option<String>) -> Result<BridgeStartResult, String> {
    let pid = plugin_id.unwrap_or_else(|| "openclaw".to_string());

    // Look up plugin config to determine protocol
    let plugins = crate::services::plugin_manager::scan_plugins();
    let plugin = plugins.iter().find(|p| p.id == pid);
    let protocol = plugin.map(|p| p.protocol.as_str()).unwrap_or("stdio-json");

    if protocol == "cli-oneshot" {
        // CLI-oneshot: no persistent subprocess — just verify the CLI exists
        return start_oneshot(&pid, &plugins).await;
    }

    // stdio-json: persistent subprocess (existing logic)
    // Always ensure bridge binary is present before starting
    if let Err(e) = download_bridge_if_missing(&pid).await {
        log::warn!("[Bridge] Auto-download failed: {}", e);
    }

    tokio::task::spawn_blocking(move || {
        // Check if already running
        {
            let mut guard = BRIDGE_PROCESS.lock().map_err(|e| format!("Lock error: {}", e))?;
            if let Some(ref mut bp) = *guard {
                match bp.child.try_wait() {
                    Ok(None) => {
                        return Ok(BridgeStartResult {
                            status: "connected".to_string(),
                            error: None,
                            agent_name: bp.agent_name.clone(),
                        });
                    }
                    _ => {
                        log::info!("[Bridge] Previous process exited, restarting...");
                        *guard = None;
                    }
                }
            }
        }

        start_bridge_internal(&pid)
    }).await.map_err(|e| format!("Task error: {}", e))?
}

/// Start a cli-oneshot agent: verify CLI exists, store config in ONESHOT_STATE
async fn start_oneshot(
    plugin_id: &str,
    plugins: &[crate::services::plugin_manager::PluginConfig],
) -> Result<BridgeStartResult, String> {
    let plugin = plugins.iter().find(|p| p.id == plugin_id)
        .ok_or_else(|| format!("Plugin '{}' not found", plugin_id))?;

    let cli = plugin.cli.as_ref()
        .ok_or_else(|| format!("Plugin '{}' has no CLI config", plugin_id))?;

    // Check if CLI command exists
    let detect_cmd = cli.detect_command.as_deref().unwrap_or(&cli.command);
    let parts: Vec<&str> = detect_cmd.split_whitespace().collect();
    let detect_result = tokio::task::spawn_blocking({
        let cmd = parts[0].to_string();
        let args: Vec<String> = parts[1..].iter().map(|s| s.to_string()).collect();
        move || Command::new(&cmd).args(&args).output()
    }).await.map_err(|e| format!("Task error: {}", e))?;

    match detect_result {
        Ok(output) if output.status.success() => {
            let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
            log::info!("[Bridge] CLI-oneshot '{}' detected: {}", cli.command, version);
        }
        Ok(output) => {
            log::warn!("[Bridge] CLI '{}' exited with {}: {}",
                cli.command, output.status,
                String::from_utf8_lossy(&output.stderr).trim());
            // Don't fail — CLI might still work for chat
        }
        Err(e) => {
            return Err(format!("CLI '{}' not found: {}. Please install it first.", cli.command, e));
        }
    }

    // Store oneshot state
    let state = OneshotState {
        plugin_id: plugin_id.to_string(),
        plugin_name: plugin.name.clone(),
        cli_command: cli.command.clone(),
        cli_args: cli.args.clone(),
        resume_args: cli.resume_args.clone(),
        session_arg: cli.session_arg.clone(),
        system_prompt_arg: cli.system_prompt_arg.clone(),
        system_prompt_when: cli.system_prompt_when.clone(),
        session_id: None,
        prompt_injected: false,
    };

    let mut guard = ONESHOT_STATE.lock().map_err(|e| format!("Lock error: {}", e))?;
    *guard = Some(state);

    Ok(BridgeStartResult {
        status: "connected".to_string(),
        error: None,
        agent_name: Some(plugin.name.clone()),
    })
}

/// Stop the Bridge subprocess
#[tauri::command]
pub async fn bridge_stop() -> Result<(), String> {
    // Clear oneshot state too
    if let Ok(mut guard) = ONESHOT_STATE.lock() {
        *guard = None;
    }
    tokio::task::spawn_blocking(|| {
        let mut guard = BRIDGE_PROCESS.lock().map_err(|e| format!("Lock error: {}", e))?;
        if let Some(mut bp) = guard.take() {
            let _ = bp.child.kill();
            let _ = bp.child.wait();
            log::info!("[Bridge] Stopped");
        }
        Ok(())
    }).await.map_err(|e| format!("Task error: {}", e))?
}

/// Get current bridge status
#[tauri::command]
pub fn bridge_status() -> BridgeStatusResult {
    // Check oneshot state first
    if let Ok(guard) = ONESHOT_STATE.lock() {
        if let Some(ref state) = *guard {
            return BridgeStatusResult {
                status: "connected".to_string(),
                agent_name: Some(state.plugin_name.clone()),
            };
        }
    }

    // Check persistent bridge process
    let mut guard = match BRIDGE_PROCESS.lock() {
        Ok(g) => g,
        Err(_) => return BridgeStatusResult { status: "standby".to_string(), agent_name: None },
    };

    match guard.as_mut() {
        None => BridgeStatusResult { status: "standby".to_string(), agent_name: None },
        Some(bp) => {
            let name = bp.agent_name.clone();
            match bp.child.try_wait() {
                Ok(None) => BridgeStatusResult { status: "connected".to_string(), agent_name: name },
                _ => {
                    *guard = None;
                    BridgeStatusResult { status: "disconnected".to_string(), agent_name: name }
                }
            }
        }
    }
}

// ── Bridge Chat Command ──

/// Chat with Agent via persistent Bridge subprocess (blocking — runs in spawn_blocking)
fn bridge_chat_sync(message: String, session_id: Option<String>) -> Result<BridgeChatResult, String> {
    log::info!("[BridgeChat] message={}, session_id={:?}",
        safe_truncate(&message, 50), session_id);

    // Auto-start bridge if not running
    {
        let guard = BRIDGE_PROCESS.lock().map_err(|e| format!("Lock error: {}", e))?;
        let needs_start = guard.is_none();
        drop(guard);

        if needs_start {
            log::info!("[BridgeChat] Bridge not running, auto-starting...");
            let start_result = start_bridge_internal("openclaw")?;
            if start_result.status != "connected" {
                return Err(format!("Failed to start bridge: {:?}", start_result.error));
            }
        }
    }

    // Send message and read response
    let mut guard = BRIDGE_PROCESS.lock().map_err(|e| format!("Lock error: {}", e))?;
    let bp = guard.as_mut().ok_or_else(|| "Bridge not running".to_string())?;

    // Use stored session_id if caller doesn't provide one
    let effective_sid = session_id.or_else(|| bp.session_id.clone());

    // Build JSON input for bridge protocol
    let input_json = if let Some(ref sid) = effective_sid {
        serde_json::json!({
            "type": "chat",
            "message": message,
            "session_id": sid
        })
    } else {
        serde_json::json!({
            "type": "chat",
            "message": message
        })
    };

    let input_str = serde_json::to_string(&input_json)
        .map_err(|e| format!("JSON serialization error: {}", e))?;

    // Write to bridge stdin
    writeln!(bp.stdin, "{}", input_str)
        .map_err(|e| format!("Failed to write to bridge stdin: {}", e))?;
    bp.stdin.flush()
        .map_err(|e| format!("Failed to flush bridge stdin: {}", e))?;

    log::info!("[BridgeChat] Sent to bridge: {}", safe_truncate(&input_str, 100));

    // Read response lines until "done" message
    let mut response_text = String::new();
    let mut new_session_id: Option<String> = None;
    let model: Option<String> = None;
    let tokens: Option<u64> = None;
    let duration_ms: Option<u64> = None;

    loop {
        let mut line = String::new();
        match bp.reader.read_line(&mut line) {
            Ok(0) => {
                // EOF — bridge process died
                log::error!("[BridgeChat] Bridge process EOF");
                break;
            }
            Ok(_) => {
                let trimmed = line.trim();
                if trimmed.is_empty() { continue; }

                log::info!("[BridgeChat] Bridge output: {}", safe_truncate(trimmed, 200));

                if let Ok(json) = serde_json::from_str::<serde_json::Value>(trimmed) {
                    match json.get("type").and_then(|v| v.as_str()) {
                        Some("text") => {
                            if let Some(text) = json.get("text").and_then(|v| v.as_str()) {
                                response_text.push_str(text);
                            }
                            if let Some(sid) = json.get("session_id").and_then(|v| v.as_str()) {
                                new_session_id = Some(sid.to_string());
                            }
                        }
                        Some("done") => {
                            if let Some(sid) = json.get("session_id").and_then(|v| v.as_str()) {
                                new_session_id = Some(sid.to_string());
                            }
                            break; // Done — exit read loop
                        }
                        Some("error") => {
                            let msg = json.get("message")
                                .and_then(|v| v.as_str())
                                .unwrap_or("Unknown error");
                            return Err(format!("Bridge error: {}", msg));
                        }
                        Some("pong") | Some("status") => {
                            // Ignore protocol messages
                        }
                        _ => {
                            // Unknown JSON type — skip (don't pollute response)
                            log::warn!("[BridgeChat] Skipping unknown bridge output: {}", safe_truncate(trimmed, 100));
                        }
                    }
                } else {
                    // Non-JSON line — skip (e.g. [Echobird] injection logs, _ ready)
                    log::warn!("[BridgeChat] Skipping non-JSON line: {}", safe_truncate(trimmed, 100));
                }
            }
            Err(e) => {
                log::error!("[BridgeChat] Read error: {}", e);
                return Err(format!("Failed to read bridge output: {}", e));
            }
        }
    }

    // Update stored session_id for next call
    if let Some(ref sid) = new_session_id {
        bp.session_id = Some(sid.clone());
    }

    Ok(BridgeChatResult {
        text: response_text,
        session_id: new_session_id.or(effective_sid),
        model,
        tokens,
        duration_ms,
    })
}

/// Tauri command: chat with Agent (async wrapper — dispatches by protocol)
#[tauri::command]
pub async fn bridge_chat_local(
    message: String,
    session_id: Option<String>,
    system_prompt: Option<String>,
) -> Result<BridgeChatResult, String> {
    // Check if we have an active oneshot agent
    let is_oneshot = ONESHOT_STATE.lock()
        .map(|g| g.is_some())
        .unwrap_or(false);

    if is_oneshot {
        return bridge_chat_oneshot(message, session_id, system_prompt).await;
    }

    // stdio-json: existing persistent subprocess chat
    tokio::task::spawn_blocking(move || {
        bridge_chat_sync(message, session_id)
    }).await.map_err(|e| format!("Task error: {}", e))?
}

/// Chat via cli-oneshot protocol: invoke CLI directly per message
async fn bridge_chat_oneshot(
    message: String,
    session_id: Option<String>,
    system_prompt: Option<String>,
) -> Result<BridgeChatResult, String> {
    // Read state
    let (state_clone, effective_sid) = {
        let guard = ONESHOT_STATE.lock().map_err(|e| format!("Lock error: {}", e))?;
        let state = guard.as_ref().ok_or("No oneshot agent active")?
            .clone();
        let sid = session_id.or_else(|| state.session_id.clone());
        (state, sid)
    };

    // Build CLI args
    let mut args: Vec<String> = if effective_sid.is_some() && state_clone.resume_args.is_some() {
        // Resume existing session
        let resume = state_clone.resume_args.as_ref().unwrap();
        resume.iter().map(|a| {
            if a == "{sessionId}" {
                effective_sid.as_ref().unwrap().clone()
            } else {
                a.clone()
            }
        }).collect()
    } else {
        state_clone.cli_args.clone()
    };

    // Inject system prompt if configured
    let should_inject = match state_clone.system_prompt_when.as_deref() {
        Some("always") => true,
        Some("new-session") => !state_clone.prompt_injected,
        _ => false,
    };

    if should_inject {
        if let Some(ref prompt_arg) = state_clone.system_prompt_arg {
            if let Some(ref prompt) = system_prompt {
                args.push(prompt_arg.clone());
                args.push(prompt.clone());
                log::info!("[BridgeOneshot] Injecting system_prompt ({} chars) via {}",
                    prompt.len(), prompt_arg);
            }
        }
    }

    // Append the user message as the last argument
    args.push(message.clone());

    log::info!("[BridgeOneshot] {} {}",
        state_clone.cli_command,
        args.iter().map(|a| if a.len() > 30 { format!("{}...", &a[..30]) } else { a.clone() }).collect::<Vec<_>>().join(" "));

    // Execute CLI (blocking with timeout)
    let cmd_name = state_clone.cli_command.clone();
    let cmd_args = args.clone();
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(300), // 5 min timeout
        tokio::task::spawn_blocking(move || {
            Command::new(&cmd_name)
                .args(&cmd_args)
                .output()
        })
    ).await;

    let output = match result {
        Ok(Ok(Ok(output))) => output,
        Ok(Ok(Err(e))) => return Err(format!("Failed to run '{}': {}", state_clone.cli_command, e)),
        Ok(Err(e)) => return Err(format!("Task error: {}", e)),
        Err(_) => return Err(format!("CLI '{}' timed out after 5 minutes", state_clone.cli_command)),
    };

    let stdout_text = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr_text = String::from_utf8_lossy(&output.stderr).to_string();

    if !output.status.success() && stdout_text.is_empty() {
        return Err(format!("CLI '{}' failed (exit {}): {}",
            state_clone.cli_command, output.status, stderr_text));
    }

    log::info!("[BridgeOneshot] Response: {} chars, status: {}",
        stdout_text.len(), output.status);

    // Try to parse session_id from JSON output (some CLIs output JSON)
    let mut new_session_id: Option<String> = None;
    for line in stdout_text.lines() {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(line) {
            if let Some(sid) = json.get("session_id").and_then(|v| v.as_str()) {
                new_session_id = Some(sid.to_string());
            }
        }
    }

    // Update oneshot state with session_id and prompt_injected flag
    {
        let mut guard = ONESHOT_STATE.lock().map_err(|e| format!("Lock error: {}", e))?;
        if let Some(ref mut state) = *guard {
            if let Some(ref sid) = new_session_id {
                state.session_id = Some(sid.clone());
            }
            if should_inject && system_prompt.is_some() {
                state.prompt_injected = true;
            }
        }
    }

    // For plain-text CLIs (like claude --print), the entire stdout IS the response
    let response = if stdout_text.lines().all(|l| serde_json::from_str::<serde_json::Value>(l).is_err()) {
        stdout_text.clone()
    } else {
        // JSON output — extract text fields
        let mut text = String::new();
        for line in stdout_text.lines() {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(line) {
                if let Some(t) = json.get("text").and_then(|v| v.as_str()) {
                    text.push_str(t);
                }
            }
        }
        if text.is_empty() { stdout_text } else { text }
    };

    Ok(BridgeChatResult {
        text: response,
        session_id: new_session_id.or(effective_sid),
        model: None,
        tokens: None,
        duration_ms: None,
    })
}

/// Tauri command: chat with remote Agent via SSH → echobird-bridge
#[tauri::command]
pub async fn bridge_chat_remote(
    pool: tauri::State<'_, crate::commands::ssh_commands::SSHPool>,
    server_id: String,
    message: String,
    session_id: Option<String>,
    plugin_id: Option<String>,
) -> Result<BridgeChatResult, String> {
    let pool = pool.inner().clone();
    let plugin = plugin_id.unwrap_or_else(|| "openclaw".to_string());

    log::info!("[BridgeChatRemote] server={}, plugin={}, msg={}",
        server_id, plugin, safe_truncate(&message, 50));

    // Auto-connect SSH if needed
    crate::commands::ssh_commands::auto_connect_ssh(&pool, &server_id).await
        .map_err(|e| format!("SSH connection failed: {}", e))?;

    // Map plugin_id to agent CLI command
    let agent_command = match plugin.as_str() {
        "openclaw" => "openclaw agent --json --agent main",
        "zeroclaw" => "zeroclaw agent --json",
        "nanoclaw" => "nanoclaw agent --json",
        other => other,
    };

    // Build JSON input
    let input_json = if let Some(ref sid) = session_id {
        serde_json::json!({ "type": "chat", "message": message, "session_id": sid })
    } else {
        serde_json::json!({ "type": "chat", "message": message })
    };

    let input_str = serde_json::to_string(&input_json)
        .map_err(|e| format!("JSON error: {}", e))?;
    let escaped = input_str.replace('\'', "'\\''");

    // Execute via SSH: pipe JSON into bridge binary with --command.
    // First kill any stale echobird-bridge processes left over from a previous
    // timed-out session — SSH disconnect does not always SIGHUP child processes.
    // Also kill any orphaned 'openclaw agent --json' processes left by previous bridge sessions.
    // Use precise pattern to avoid killing user's intentional 'openclaw gateway' process.
    let cmd = format!(
        "pkill -f 'echobird-bridge' 2>/dev/null; pkill -f 'openclaw.*agent.*--json' 2>/dev/null; sleep 0.3; export PATH=\"$HOME/.npm-global/bin:$HOME/.local/bin:$HOME/.cargo/bin:$PATH\" && echo '{}' | ~/echobird/echobird-bridge --command '{}' 2>/dev/null",
        escaped, agent_command
    );

    let connections = pool.lock().await;
    let client = connections.get(&server_id)
        .ok_or_else(|| format!("SSH not connected: {}", server_id))?;

    // Timeout: OpenClaw currently works in blocking mode; 5 minutes covers most tasks.
    // The UI shows a "working" animation while waiting.
    let result = match tokio::time::timeout(
        std::time::Duration::from_secs(300), // 5 minutes
        client.execute(&cmd)
    ).await {
        Ok(Ok(r)) => r,
        Ok(Err(e)) => return Err(format!("SSH exec failed: {}", e)),
        Err(_) => return Err("Bridge chat timed out (5 min). The agent may still be running — try resuming the session.".to_string()),
    };

    drop(connections); // Release lock

    if result.exit_status != 0 && result.stdout.is_empty() {
        return Err(format!("Bridge execution failed (exit {}): {}", result.exit_status, result.stderr));
    }

    // Parse bridge JSON output
    let mut response_text = String::new();
    let mut new_session_id: Option<String> = None;

    for line in result.stdout.lines() {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(line) {
            match json.get("type").and_then(|v| v.as_str()) {
                Some("text") => {
                    if let Some(text) = json.get("text").and_then(|v| v.as_str()) {
                        response_text.push_str(text);
                    }
                    if let Some(sid) = json.get("session_id").and_then(|v| v.as_str()) {
                        new_session_id = Some(sid.to_string());
                    }
                }
                Some("done") => {
                    if let Some(sid) = json.get("session_id").and_then(|v| v.as_str()) {
                        new_session_id = Some(sid.to_string());
                    }
                }
                Some("error") => {
                    let msg = json.get("message").and_then(|v| v.as_str()).unwrap_or("Unknown error");
                    return Err(format!("Bridge error: {}", msg));
                }
                _ => {}
            }
        }
    }

    if response_text.is_empty() && !result.stdout.is_empty() {
        response_text = result.stdout.clone();
    }

    Ok(BridgeChatResult {
        text: response_text,
        session_id: new_session_id.or(session_id),
        model: None,
        tokens: None,
        duration_ms: None,
    })
}

// ── Result Types ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeChatResult {
    pub text: String,
    pub session_id: Option<String>,
    pub model: Option<String>,
    pub tokens: Option<u64>,
    pub duration_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BridgeStartResult {
    pub status: String,
    pub error: Option<String>,
    pub agent_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BridgeStatusResult {
    pub status: String,
    pub agent_name: Option<String>,
}

// ── Channel Chat History Commands ──────────────────────────────────────────────

use crate::services::channel_history::{self, ChannelMessage};

/// Load a paginated slice of messages for a channel.
/// offset=0 → newest batch; offset=30 → next older batch.
/// Messages returned in chronological order (oldest first within slice).
#[tauri::command]
pub async fn channel_history_load(
    channel_key: String,
    offset: usize,
    limit: usize,
) -> Result<ChannelHistoryResponse, String> {
    let messages = channel_history::load_channel_history(&channel_key, offset, limit);
    let total = channel_history::channel_history_count(&channel_key);
    Ok(ChannelHistoryResponse { messages, total })
}

/// Save the full message list for a channel (replaces existing file).
#[tauri::command]
pub async fn channel_history_save(
    channel_key: String,
    messages: Vec<ChannelMessage>,
) -> Result<(), String> {
    channel_history::save_channel_history(&channel_key, messages);
    Ok(())
}

/// Delete the channel history file.
#[tauri::command]
pub async fn channel_history_clear(channel_key: String) -> Result<(), String> {
    channel_history::clear_channel_history(&channel_key);
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelHistoryResponse {
    pub messages: Vec<ChannelMessage>,
    pub total: usize,
}
