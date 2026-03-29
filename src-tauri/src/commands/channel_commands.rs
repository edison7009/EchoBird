// Channel commands — bridge process management + channel persistence
//
// Local channel (id=1) uses Bridge binary (bridge/src/main.rs → echobird-bridge)
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
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        
        // wmic covers Node-based CLI tools where the process is node.exe
        let mut wmic_cmd = Command::new("wmic");
        wmic_cmd.args(["process", "where", "commandline like '%openclaw%gateway%'", "get", "processid", "/format:value"]);
        wmic_cmd.creation_flags(CREATE_NO_WINDOW);
        
        if let Ok(out) = wmic_cmd.output()
        {
            let text = String::from_utf8_lossy(&out.stdout);
            if text.lines().any(|l| l.trim_start().starts_with("ProcessId=") && l.trim() != "ProcessId=") {
                log::info!("[Bridge] OpenClaw gateway detected via process list (wmic)");
                return true;
            }
        }
        
        // Fallback: tasklist for native binary installs
        let mut tasklist_cmd = Command::new("tasklist");
        tasklist_cmd.args(["/FO", "CSV", "/NH"]);
        tasklist_cmd.creation_flags(CREATE_NO_WINDOW);
        
        if let Ok(out) = tasklist_cmd.output() {
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
    plugin_id: String,
}

static BRIDGE_PROCESS: Mutex<Option<BridgeProcess>> = Mutex::new(None);

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

    // Launch OpenClaw Gateway (only for openclaw plugin)
    // Skip if gateway is already running (check port 18789)
    if plugin_id == "openclaw" {
        // Pre-flight: OpenClaw requires Node.js >= 22.14.0
        match Command::new("node").arg("--version").output() {
            Ok(out) => {
                let ver_raw = String::from_utf8_lossy(&out.stdout);
                let ver = ver_raw.trim().trim_start_matches('v');
                let parts: Vec<u64> = ver.split('.').map(|s| s.parse().unwrap_or(0)).collect();
                let major = parts.first().copied().unwrap_or(0);
                let minor = parts.get(1).copied().unwrap_or(0);
                if major < 22 || (major == 22 && minor < 14) {
                    return Err(format!(
                        "OpenClaw requires Node.js >= 22.14.0 — detected v{}. Please upgrade at https://nodejs.org",
                        ver
                    ));
                }
                log::info!("[Bridge] Node.js pre-flight OK: v{}", ver);
            }
            Err(_) => {
                return Err(
                    "OpenClaw requires Node.js >= 22.14.0, but Node.js was not found in PATH. \
                     Please install it from https://nodejs.org".to_string()
                );
            }
        }
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
                cmd.stdin(Stdio::null())
                   .stdout(Stdio::null())
                   .stderr(Stdio::null());
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

            // Wait for Gateway to be ready — poll port 18789 instead of fixed sleep.
            // Node.js Gateway can take 1-3s normally, longer on slow machines or first install.
            // Heartbeat keeps the session alive — we poll up to 30s before giving up.
            {
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
                let mut ready = false;
                while std::time::Instant::now() < deadline {
                    if is_port_in_use(18789) {
                        ready = true;
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(300));
                }
                if ready {
                    log::info!("[Bridge] OpenClaw Gateway is ready on port 18789");
                } else {
                    log::warn!("[Bridge] OpenClaw Gateway did not become ready within 30s — proceeding anyway");
                }
            }
        }
    }
    } // end: only openclaw launches Gateway

    // Pass --config to Bridge so it loads the correct agent config
    let plugin_json_path = crate::services::plugin_manager::get_plugin_json_path(plugin);

    // Spawn bridge process with piped stdin/stdout
    let mut cmd = Command::new(&bridge_path);
    if let Some(ref config_path) = plugin_json_path {
        cmd.arg("--config").arg(config_path.to_string_lossy().as_ref());
        log::info!("[Bridge] Using config: {:?}", config_path);
    }
    // Redirect Bridge stderr to log file for diagnosis
    let stderr_file = std::fs::File::create(std::env::temp_dir().join("echobird_bridge_debug.log"))
        .map(Stdio::from)
        .unwrap_or(Stdio::null());
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(stderr_file);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = cmd.spawn()
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
        plugin_id: plugin_id.to_string(),
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
    use crate::services::plugin_manager::{scan_plugins, get_bridge_path};

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

    // Destination path: use central bridge/ directory
    let dest_dir = crate::services::plugin_manager::bridge_dir();
    std::fs::create_dir_all(&dest_dir)
        .map_err(|e| format!("Failed to create bridge dir: {}", e))?;
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
    let _protocol = plugin.map(|p| p.protocol.as_str()).unwrap_or("stdio-json");

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
                        // Bridge running — check if it's the same agent
                        if bp.plugin_id == pid {
                            return Ok(BridgeStartResult {
                                status: "connected".to_string(),
                                error: None,
                                agent_name: bp.agent_name.clone(),
                            });
                        } else {
                            // Different agent — kill old bridge and restart
                            log::info!("[Bridge] Agent switch: {} -> {}, restarting", bp.plugin_id, pid);
                            let _ = bp.child.kill();
                            let _ = bp.child.wait();
                            *guard = None;
                        }
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

/// Stop the Bridge subprocess
#[tauri::command]
pub async fn bridge_stop() -> Result<(), String> {
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
fn bridge_chat_sync(message: String, session_id: Option<String>, system_prompt: Option<String>, role_name: Option<String>) -> Result<BridgeChatResult, String> {
    log::info!("[BridgeChat] message={}, session_id={:?}, role_name={:?}, system_prompt len: {:?}",
        safe_truncate(&message, 50), session_id, role_name, system_prompt.as_ref().map(|s| s.len()));

    // Auto-start bridge if not running
    {
        let guard = BRIDGE_PROCESS.lock().map_err(|e| format!("Lock error: {}", e))?;
        let needs_start = guard.is_none();
        drop(guard);

        if needs_start {
            // Bridge should already be started by frontend's bridge_start() call.
            // This is a safety fallback — log a warning so we can detect if this ever triggers.
            log::warn!("[BridgeChat] Bridge not running — frontend should have started it. Falling back to openclaw.");
            let start_result = start_bridge_internal("openclaw")?;
            if start_result.status != "connected" {
                return Err(format!("Failed to start bridge: {:?}", start_result.error));
            }
        }
    }

    // Send message and read response
    let mut guard = BRIDGE_PROCESS.lock().map_err(|e| format!("Lock error: {}", e))?;
    let bp = guard.as_mut().ok_or_else(|| "Bridge not running".to_string())?;

    // Extract plugin CLI config to check if sessionMode == "always"
    let plugins = crate::services::plugin_manager::scan_plugins();
    let plugin_config = plugins.iter().find(|p| p.id == bp.plugin_id);
    let session_mode_always = plugin_config
        .and_then(|p| p.cli.as_ref())
        .and_then(|cli| cli.session_mode.as_deref())
        == Some("always");

    // Use stored session_id if caller doesn't provide one
    let mut effective_sid = session_id.or_else(|| bp.session_id.clone());

    // Auto-generate UUID for new sessions if plugin requires it
    if effective_sid.is_none() && session_mode_always {
        effective_sid = Some(uuid::Uuid::new_v4().to_string());
    }

    // Build JSON input for bridge protocol
    // If we have a session_id from a previous message, use "resume" to continue the session
    // (Bridge translates "resume" → --resume {sessionId} for Claude Code)
    let mut input_json = if let Some(ref sid) = effective_sid {
        serde_json::json!({
            "type": "resume",
            "message": message,
            "session_id": sid
        })
    } else {
        serde_json::json!({
            "type": "chat",
            "message": message
        })
    };
    
    if let Some(ref rn) = role_name {
        input_json["agent_name"] = serde_json::json!(rn);
    }
    if let Some(ref sp) = system_prompt {
        input_json["system_prompt"] = serde_json::json!(sp);
    }

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
                // EOF — bridge process died, attempt auto-recovery
                log::error!("[BridgeChat] Bridge process EOF — attempting auto-recovery");
                let plugin_id = bp.plugin_id.clone();
                *guard = None; // Clear dead process
                drop(guard);   // Release lock before restart

                // Restart Bridge
                log::info!("[BridgeChat] Restarting bridge (plugin: {})", plugin_id);
                let restart = start_bridge_internal(&plugin_id);
                match restart {
                    Ok(ref r) if r.status == "connected" => {
                        log::info!("[BridgeChat] Bridge restarted, retrying message...");
                        // Retry: re-acquire lock and resend message
                        let mut guard2 = BRIDGE_PROCESS.lock().map_err(|e| format!("Lock error: {}", e))?;
                        let bp2 = guard2.as_mut().ok_or("Bridge not running after restart")?;
                        let mut retry_json = serde_json::json!({"type": "chat", "message": message.clone()});
                        if let Some(ref sp) = system_prompt {
                            retry_json["system_prompt"] = serde_json::json!(sp);
                        }
                        let retry_str = serde_json::to_string(&retry_json).unwrap_or_default();
                        let _ = writeln!(bp2.stdin, "{}", retry_str);
                        let _ = bp2.stdin.flush();

                        // Read retry response
                        loop {
                            let mut retry_line = String::new();
                            match bp2.reader.read_line(&mut retry_line) {
                                Ok(0) => return Err("Bridge crashed again after restart".to_string()),
                                Ok(_) => {
                                    let t = retry_line.trim();
                                    if t.is_empty() { continue; }
                                    if let Ok(j) = serde_json::from_str::<serde_json::Value>(t) {
                                        match j.get("type").and_then(|v| v.as_str()) {
                                            Some("text") => {
                                                if let Some(txt) = j.get("text").and_then(|v| v.as_str()) {
                                                    response_text.push_str(txt);
                                                }
                                                if let Some(sid) = j.get("session_id").and_then(|v| v.as_str()) {
                                                    new_session_id = Some(sid.to_string());
                                                }
                                            }
                                            Some("done") => {
                                                if let Some(sid) = j.get("session_id").and_then(|v| v.as_str()) {
                                                    new_session_id = Some(sid.to_string());
                                                }
                                                break;
                                            }
                                            Some("error") => {
                                                let msg = j.get("message").and_then(|v| v.as_str()).unwrap_or("Unknown error");
                                                return Err(format!("Bridge error after restart: {}", msg));
                                            }
                                            _ => continue,
                                        }
                                    }
                                }
                                Err(e) => return Err(format!("Read error after restart: {}", e)),
                            }
                        }
                        // Store session after retry
                        if let Some(ref sid) = new_session_id { bp2.session_id = Some(sid.clone()); }
                        return Ok(BridgeChatResult { text: response_text, session_id: new_session_id, model, tokens, duration_ms });
                    }
                    _ => return Err("Bridge crashed and restart failed".to_string()),
                }
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
    role_name: Option<String>,
) -> Result<BridgeChatResult, String> {

    // stdio-json: existing persistent subprocess chat
    tokio::task::spawn_blocking(move || {
        bridge_chat_sync(message, session_id, system_prompt, role_name)
    }).await.map_err(|e| format!("Task error: {}", e))?
}

/// Set role on local bridge subprocess (stdio-json protocol)
fn bridge_set_role_sync(agent_id: String, role_id: String, url: String) -> Result<serde_json::Value, String> {
    log::info!("[BridgeSetRoleLocal] agent={}, role={}, url={}", agent_id, role_id, url);

    // Auto-start bridge if not running
    {
        let guard = BRIDGE_PROCESS.lock().map_err(|e| format!("Lock error: {}", e))?;
        let needs_start = guard.is_none();
        drop(guard);

        if needs_start {
            // Use the agent_id from the role being set — not hardcoded openclaw
            log::info!("[BridgeSetRoleLocal] Bridge not running, auto-starting for agent: {}", agent_id);
            let start_result = start_bridge_internal(&agent_id)?;
            if start_result.status != "connected" {
                return Err(format!("Failed to start bridge: {:?}", start_result.error));
            }
        }
    }

    let mut guard = BRIDGE_PROCESS.lock().map_err(|e| format!("Lock error: {}", e))?;
    let bp = guard.as_mut().ok_or_else(|| "Bridge not running".to_string())?;

    let input_json = serde_json::json!({
        "type": "set_role",
        "agent_id": agent_id,
        "role_id": role_id,
        "url": url
    });
    let input_str = serde_json::to_string(&input_json)
        .map_err(|e| format!("JSON error: {}", e))?;

    writeln!(bp.stdin, "{}", input_str)
        .map_err(|e| format!("Failed to write to bridge stdin: {}", e))?;
    bp.stdin.flush()
        .map_err(|e| format!("Failed to flush bridge stdin: {}", e))?;

    log::info!("[BridgeSetRoleLocal] Sent: {}", safe_truncate(&input_str, 100));

    // Read response: look for role_set or error
    loop {
        let mut line = String::new();
        match bp.reader.read_line(&mut line) {
            Ok(0) => return Err("Bridge EOF during set_role".to_string()),
            Ok(_) => {
                let trimmed = line.trim();
                if trimmed.is_empty() { continue; }
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(trimmed) {
                    match json.get("type").and_then(|v| v.as_str()) {
                        Some("role_set") => {
                            log::info!("[BridgeSetRoleLocal] SUCCESS: role_set received");
                            // Clear stored session so next chat starts fresh
                            // (SOUL.md is only read at session start)
                            bp.session_id = None;
                            return Ok(json);
                        }
                        Some("error") => {
                            let msg = json.get("message").and_then(|v| v.as_str()).unwrap_or("Unknown error");
                            log::error!("[BridgeSetRoleLocal] ERROR from bridge: {}", msg);
                            return Err(format!("Bridge error: {}", msg));
                        }
                        Some("done") => return Ok(json),
                        _ => continue,
                    }
                }
            }
            Err(e) => return Err(format!("Read error: {}", e)),
        }
    }
}

/// Tauri command: set role on local bridge
#[tauri::command]
pub async fn bridge_set_role_local(
    agent_id: String,
    role_id: String,
    url: String,
) -> Result<serde_json::Value, String> {
    // Empty role_id = clear role (return to default mode)
    if role_id.is_empty() {
        {
            // Send clear_role to Bridge subprocess
            if let Ok(mut guard) = BRIDGE_PROCESS.lock() {
                if let Some(ref mut bp) = *guard {
                    let clear_json = serde_json::json!({
                        "type": "clear_role",
                        "agent_id": agent_id
                    });
                    let _ = writeln!(bp.stdin, "{}", clear_json.to_string());
                    let _ = bp.stdin.flush();
                    // Clear stored session so next chat starts fresh
                    bp.session_id = None;
                    log::info!("[BridgeSetRoleLocal] Sent clear_role to Bridge for {}", agent_id);
                }
            }
        }

        return Ok(serde_json::json!({
            "type": "role_cleared",
            "agent_id": agent_id
        }));
    }

    tokio::task::spawn_blocking(move || {
        bridge_set_role_sync(agent_id, role_id, url)
    }).await.map_err(|e| format!("Task error: {}", e))?
}


// ── Auto-Deploy Bridge to Remote ──

/// Cache of server IDs that have been verified to have Bridge installed this session.
/// Avoids re-checking on every message.
static REMOTE_BRIDGE_VERIFIED: std::sync::LazyLock<std::sync::Mutex<std::collections::HashSet<String>>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashSet::new()));

/// Ensure the remote server has an up-to-date Bridge binary.
/// If missing or outdated, upload from local bundle via base64 SSH transfer.
async fn ensure_remote_bridge(
    pool: &crate::commands::ssh_commands::SSHPool,
    server_id: &str,
) -> Result<(), String> {
    // Skip if already verified this session
    if let Ok(cache) = REMOTE_BRIDGE_VERIFIED.lock() {
        if cache.contains(server_id) {
            return Ok(());
        }
    }

    log::info!("[Bridge] Checking remote bridge on server: {}", server_id);

    let connections = pool.lock().await;
    let client = connections.get(server_id)
        .ok_or_else(|| format!("SSH not connected: {}", server_id))?;

    // Check if bridge exists and get version
    // IMPORTANT: Old Bridge (pre-3.1.5) doesn't support --version and enters main loop,
    // so we must use timeout + pipe empty stdin to prevent hanging.
    // Use execute_tolerant (not client.execute) because piped commands may not send exit status.
    let check_result = match tokio::time::timeout(
        std::time::Duration::from_secs(10),
        crate::commands::ssh_commands::execute_tolerant(client, "echo '' | timeout 3 ~/echobird/echobird-bridge --version 2>/dev/null || echo 'NOT_INSTALLED'"),
    ).await {
        Ok(Ok(r)) => r,
        Ok(Err(e)) => return Err(format!("SSH check failed: {}", e)),
        Err(_) => {
            log::warn!("[Bridge] Version check timed out, assuming bridge is OK");
            // If version check hangs, bridge likely exists — cache and proceed
            if let Ok(mut cache) = REMOTE_BRIDGE_VERIFIED.lock() {
                cache.insert(server_id.to_string());
            }
            drop(connections);
            return Ok(());
        }
    };

    let remote_version = check_result.stdout.trim().to_string();
    let local_version = env!("CARGO_PKG_VERSION");

    if !remote_version.contains("NOT_INSTALLED") && remote_version.contains(local_version) {
        // Bridge exists and version matches — cache and return
        log::info!("[Bridge] Remote bridge OK (version: {})", remote_version);
        if let Ok(mut cache) = REMOTE_BRIDGE_VERIFIED.lock() {
            cache.insert(server_id.to_string());
        }
        return Ok(());
    }

    log::info!("[Bridge] Remote bridge needs deploy (remote: '{}', local: '{}')", remote_version, local_version);

    // Detect remote OS + architecture
    let os_result = client.execute("uname -s 2>/dev/null || echo windows")
        .await.map_err(|e| format!("Failed to detect remote OS: {}", e))?;
    let arch_result = client.execute("uname -m 2>/dev/null || echo x86_64")
        .await.map_err(|e| format!("Failed to detect remote arch: {}", e))?;

    let os_name = os_result.stdout.trim().to_lowercase();
    let arch = arch_result.stdout.trim().to_lowercase();

    // Map to local bridge binary filename
    let bridge_filename = if os_name.contains("linux") {
        if arch.contains("aarch64") || arch.contains("arm64") {
            "bridge-linux-aarch64"
        } else {
            "bridge-linux-x86_64"
        }
    } else if os_name.contains("darwin") {
        if arch.contains("arm64") || arch.contains("aarch64") {
            "bridge-darwin-aarch64"
        } else {
            "bridge-darwin-x86_64"
        }
    } else {
        "bridge-win.exe"
    };

    let remote_binary = format!("~/echobird/{}", bridge_filename);

    // ── Strategy 1: SSH base64 upload (most reliable — connection already established) ──

    // Find local bridge binary path from bundle
    let local_path = {
        let plugins = crate::services::plugin_manager::plugins_dir();
        let bridge_dir = plugins.parent().unwrap_or(&plugins).join("bridge");
        let path = bridge_dir.join(bridge_filename);
        if path.exists() { Some(path) } else { None }
    };

    if let Some(ref local_path) = local_path {
        log::info!("[Bridge] Uploading {} via SSH", bridge_filename);

        let file_data = std::fs::read(local_path)
            .map_err(|e| format!("Failed to read local bridge binary: {}", e))?;

        let encoded = {
            use base64::Engine;
            base64::engine::general_purpose::STANDARD.encode(&file_data)
        };

        // Release connection lock before upload (re-acquire after)
        drop(connections);

        let connections = pool.lock().await;
        let client = connections.get(server_id)
            .ok_or_else(|| format!("SSH not connected: {}", server_id))?;

        let _ = client.execute("mkdir -p ~/echobird").await;

        // Upload via base64 chunks
        let chunk_size = 65536;
        let chunks: Vec<&str> = encoded.as_bytes()
            .chunks(chunk_size)
            .map(|c| std::str::from_utf8(c).unwrap_or(""))
            .collect();

        let mut upload_ok = true;
        if chunks.len() == 1 {
            let cmd = format!("echo '{}' | base64 -d > {}", encoded, remote_binary);
            if let Err(e) = client.execute(&cmd).await {
                log::warn!("[Bridge] SSH upload failed: {}", e);
                upload_ok = false;
            }
        } else {
            let first_cmd = format!("printf '%s' '{}' > {}.b64", chunks[0], remote_binary);
            if let Err(e) = client.execute(&first_cmd).await {
                log::warn!("[Bridge] SSH upload chunk 0 failed: {}", e);
                upload_ok = false;
            } else {
                for (i, chunk) in chunks[1..].iter().enumerate() {
                    let cmd = format!("printf '%s' '{}' >> {}.b64", chunk, remote_binary);
                    if let Err(e) = client.execute(&cmd).await {
                        log::warn!("[Bridge] SSH upload chunk {} failed: {}", i + 1, e);
                        upload_ok = false;
                        break;
                    }
                }
                if upload_ok {
                    let decode_cmd = format!("base64 -d {}.b64 > {} && rm {}.b64",
                        remote_binary, remote_binary, remote_binary);
                    if let Err(e) = client.execute(&decode_cmd).await {
                        log::warn!("[Bridge] SSH upload decode failed: {}", e);
                        upload_ok = false;
                    }
                }
            }
        }

        if upload_ok {
            let chmod_cmd = format!("chmod +x {} && ln -sf {} ~/echobird/echobird-bridge",
                remote_binary, remote_binary);
            let _ = client.execute(&chmod_cmd).await;

            log::info!("[Bridge] Remote bridge deployed via SSH: {} ({} bytes)", bridge_filename, file_data.len());
            if let Ok(mut cache) = REMOTE_BRIDGE_VERIFIED.lock() {
                cache.insert(server_id.to_string());
            }
            return Ok(());
        }

        log::info!("[Bridge] SSH upload failed, falling back to CDN download");
    } else {
        log::info!("[Bridge] Local binary not found, trying CDN download");
    }

    // ── Strategy 2: CDN download (fallback — requires server internet access) ──
    let cdn_url = format!(
        "https://dl.echobird.ai/releases/v{}/{}",
        local_version, bridge_filename
    );

    log::info!("[Bridge] Trying CDN download: {}", cdn_url);

    // Re-acquire connection if needed
    let connections = pool.lock().await;
    let client = connections.get(server_id)
        .ok_or_else(|| format!("SSH not connected: {}", server_id))?;

    let cdn_cmd = format!(
        "mkdir -p ~/echobird && (wget -q --timeout=15 -O {remote} '{url}' 2>/dev/null || curl -fsSL --max-time 15 -o {remote} '{url}' 2>/dev/null) && chmod +x {remote} && ln -sf {remote} ~/echobird/echobird-bridge && echo 'CDN_OK'",
        remote = remote_binary,
        url = cdn_url,
    );
    let cdn_result = match tokio::time::timeout(
        std::time::Duration::from_secs(20),
        client.execute(&cdn_cmd),
    ).await {
        Ok(r) => r,
        Err(_) => {
            log::warn!("[Bridge] CDN download timed out (20s)");
            Err(async_ssh2_tokio::Error::from(std::io::Error::new(std::io::ErrorKind::TimedOut, "CDN timeout")))
        }
    };

    if let Ok(res) = &cdn_result {
        if res.stdout.trim().contains("CDN_OK") {
            log::info!("[Bridge] Remote bridge deployed via CDN: {}", bridge_filename);
            if let Ok(mut cache) = REMOTE_BRIDGE_VERIFIED.lock() {
                cache.insert(server_id.to_string());
            }
            return Ok(());
        }
    }

    Err(format!("Failed to deploy bridge to remote server (both SSH upload and CDN download failed)"))
}


/// Public Tauri command: deploy/update Bridge binary on a remote server.
/// Used by "Test Connection" flow to ensure Bridge is ready before showing success.
#[tauri::command]
pub async fn bridge_ensure_remote(
    pool: tauri::State<'_, crate::commands::ssh_commands::SSHPool>,
    server_id: String,
) -> Result<String, String> {
    let pool = pool.inner().clone();
    crate::commands::ssh_commands::auto_connect_ssh(&pool, &server_id).await
        .map_err(|e| format!("SSH connection failed: {}", e))?;
    ensure_remote_bridge(&pool, &server_id).await?;
    Ok("Bridge ready".to_string())
}


/// Tauri command: chat with remote Agent via SSH → echobird-bridge
#[tauri::command]
pub async fn bridge_chat_remote(
    pool: tauri::State<'_, crate::commands::ssh_commands::SSHPool>,
    server_id: String,
    message: String,
    session_id: Option<String>,
    plugin_id: Option<String>,
    role_id: Option<String>,
) -> Result<BridgeChatResult, String> {
    let pool = pool.inner().clone();
    let plugin = plugin_id.unwrap_or_else(|| "openclaw".to_string());

    log::info!("[BridgeChatRemote] server={}, plugin={}, msg={}",
        server_id, plugin, safe_truncate(&message, 50));

    // Auto-connect SSH if needed
    crate::commands::ssh_commands::auto_connect_ssh(&pool, &server_id).await
        .map_err(|e| format!("SSH connection failed: {}", e))?;

    // Auto-deploy Bridge binary if missing or outdated (skip on mobile — SSH impl incompatible)
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    ensure_remote_bridge(&pool, &server_id).await
        .map_err(|e| format!("Bridge auto-deploy failed: {}", e))?;

    // Map plugin_id to agent CLI command (dynamic from plugin.json)
    let plugins = crate::services::plugin_manager::scan_plugins();
    let plugin_config = plugins.iter().find(|p| p.id == plugin);
    let _protocol = plugin_config.map(|p| p.protocol.as_str()).unwrap_or("stdio-json");

    let session_mode_always = plugin_config
        .and_then(|p| p.cli.as_ref())
        .and_then(|cli| cli.session_mode.as_deref())
        == Some("always");

    let effective_session_id = if session_id.is_none() && session_mode_always {
        Some(uuid::Uuid::new_v4().to_string())
    } else {
        session_id.clone()
    };

    let agent_command = if let Some(p) = plugin_config {
        if let Some(ref cli) = p.cli {
            let mut parts = vec![cli.command.clone()];
            // When resuming with session_id, use resumeArgs if available
            if let (Some(ref sid), Some(ref resume_args)) = (&effective_session_id, &cli.resume_args) {
                let resolved: Vec<String> = resume_args.iter()
                    .map(|a| a.replace("{sessionId}", sid))
                    .collect();
                parts.extend(resolved);
            } else {
                parts.extend(cli.args.iter().cloned());
            }
            parts.join(" ")
        } else {
            format!("{} agent --json", plugin)
        }
    } else {
        format!("{} agent --json", plugin)
    };

    // Determine if we need --agent-arg for role injection
    let agent_arg_flag = plugin_config
        .and_then(|p| p.cli.as_ref())
        .and_then(|cli| cli.agent_arg.as_ref())
        .cloned();

    // All protocols: pipe JSON into Bridge (Bridge handles output parsing for all agents)
    // NOTE: Do NOT pass session_id in JSON here. When session_id is present,
    // we already embed resumeArgs (e.g. --resume {uuid}) into the --command string above.
    // If we also pass session_id in JSON, Bridge's execute_chat enters is_resume=true and
    // overrides our correct --resume with its hardcoded --session-id — breaking Claude Code.
    let mut input_map = serde_json::json!({ "type": "chat", "message": message });
    if let Some(ref rid) = role_id {
        if agent_arg_flag.is_some() {
            input_map["agent_name"] = serde_json::json!(rid);
        }
    }
    let input_str = serde_json::to_string(&input_map)
        .map_err(|e| format!("JSON error: {}", e))?;
    let escaped = input_str.replace('\'', "'\\''");
    let bridge_extra = if let Some(ref aa) = agent_arg_flag {
        format!(" --agent-arg '{}'", aa)
    } else {
        String::new()
    };
    let cmd = format!(
        "export PATH=\"$HOME/.npm-global/bin:$HOME/.local/bin:$HOME/.cargo/bin:$PATH\" && echo '{}' | ~/echobird/echobird-bridge --command '{}'{}",
        escaped, agent_command, bridge_extra
    );

    let connections = pool.lock().await;
    let client = connections.get(&server_id)
        .ok_or_else(|| format!("SSH not connected: {}", server_id))?;

    // Heartbeat-aware streaming execution:
    // - bridge binary emits {"type":"working","elapsed_secs":N} every 20s while agent runs
    // - On each line received, idle timer resets (90s idle = truly dead connection)
    // - No more hard 5-min timeout: the agent can run as long as it needs
    let mut response_text = String::new();
    let mut new_session_id: Option<String> = None;
    let mut parsed_bridge_json = false;
    let mut bridge_error: Option<String> = None;

    let idle_timeout = std::time::Duration::from_secs(90);
    let result = crate::commands::ssh_commands::execute_with_heartbeat(
        client,
        &cmd,
        idle_timeout,
        |line| {
            // Called for each output line from the bridge subprocess.
            // Parse JSON and collect text/done/error; silently drop working heartbeats.
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(line) {
                if let Some(msg_type) = json.get("type").and_then(|v| v.as_str()) {
                    match msg_type {
                        "working" => {
                            // Heartbeat from bridge — just log it, idle timer already reset
                            let elapsed = json.get("elapsed_secs").and_then(|v| v.as_u64()).unwrap_or(0);
                            log::info!("[BridgeChatRemote] agent working ({}s elapsed)", elapsed);
                        }
                        "text" => {
                            parsed_bridge_json = true;
                            if let Some(text) = json.get("text").and_then(|v| v.as_str()) {
                                response_text.push_str(text);
                            }
                            if let Some(sid) = json.get("session_id").and_then(|v| v.as_str()) {
                                new_session_id = Some(sid.to_string());
                            }
                        }
                        "done" => {
                            parsed_bridge_json = true;
                            if let Some(sid) = json.get("session_id").and_then(|v| v.as_str()) {
                                new_session_id = Some(sid.to_string());
                            }
                        }
                        "error" => {
                            parsed_bridge_json = true;
                            let msg = json.get("message").and_then(|v| v.as_str()).unwrap_or("Unknown error");
                            bridge_error = Some(format!("Bridge error: {}", msg));
                        }
                        _ => {}
                    }
                }
            }
        },
    ).await;

    drop(connections); // Release lock

    // Propagate idle-timeout or SSH errors
    let result = match result {
        Ok(r) => r,
        Err(e) => return Err(e),
    };

    // Propagate any bridge protocol error seen in streaming
    if let Some(err) = bridge_error {
        return Err(err);
    }

    if result.exit_status != 0 && result.stdout.is_empty() {
        return Err(format!("Bridge execution failed (exit {}): {}", result.exit_status, result.stderr));
    }

    // Only fallback to raw stdout when Bridge returned NO valid protocol JSON
    // (i.e., Bridge completely failed to run — not when agent returned empty text)
    if response_text.is_empty() && !parsed_bridge_json && !result.stdout.is_empty() {
        response_text = result.stdout.clone();
    }

    Ok(BridgeChatResult {
        text: response_text,
        session_id: new_session_id.or(effective_session_id),
        model: None,
        tokens: None,
        duration_ms: None,
    })
}



// ── Remote Bridge CLI: Detect Agents ──

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteAgentInfo {
    pub id: String,
    pub name: String,
    pub installed: bool,
    pub running: bool,
    pub path: Option<String>,
}

#[tauri::command]
pub async fn bridge_detect_agents_remote(
    pool: tauri::State<'_, crate::commands::ssh_commands::SSHPool>,
    server_id: String,
) -> Result<Vec<RemoteAgentInfo>, String> {
    let pool = pool.inner().clone();

    log::info!("[BridgeDetectAgents] server={}", server_id);

    crate::commands::ssh_commands::auto_connect_ssh(&pool, &server_id).await
        .map_err(|e| format!("SSH connection failed: {}", e))?;

    // Auto-deploy Bridge binary if missing or outdated (skip on mobile)
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    ensure_remote_bridge(&pool, &server_id).await
        .map_err(|e| format!("Bridge auto-deploy failed: {}", e))?;

    let input_json = serde_json::json!({ "type": "detect_agents" });
    let input_str = serde_json::to_string(&input_json).map_err(|e| format!("JSON error: {}", e))?;
    let escaped = input_str.replace('\'', "'\\''");

    let cmd = format!(
        "export PATH=\"$HOME/.npm-global/bin:$HOME/.local/bin:$HOME/.cargo/bin:$PATH\" && echo '{}' | ~/echobird/echobird-bridge 2>/dev/null",
        escaped
    );

    let connections = pool.lock().await;
    let client = connections.get(&server_id)
        .ok_or_else(|| format!("SSH not connected: {}", server_id))?;

    let result = match tokio::time::timeout(
        std::time::Duration::from_secs(15),
        crate::commands::ssh_commands::execute_tolerant(client, &cmd)
    ).await {
        Ok(Ok(r)) => r,
        Ok(Err(e)) => return Err(format!("SSH exec failed: {}", e)),
        Err(_) => return Err("Agent detection timed out".to_string()),
    };

    drop(connections);

    log::info!("[BridgeDetectAgents] stdout={}", result.stdout);
    log::info!("[BridgeDetectAgents] stderr={}", result.stderr);
    log::info!("[BridgeDetectAgents] exit_status={}", result.exit_status);

    // Parse Bridge CLI JSON output: look for {"type":"agents_detected","agents":[...]}
    for line in result.stdout.lines() {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(line) {
            if json.get("type").and_then(|v| v.as_str()) == Some("agents_detected") {
                if let Some(agents) = json.get("agents") {
                    let agents: Vec<RemoteAgentInfo> = serde_json::from_value(agents.clone())
                        .unwrap_or_default();
                    log::info!("[BridgeDetectAgents] detected {} agents", agents.len());
                    return Ok(agents);
                }
            }
        }
    }

    Err(format!("No agent detection response from Bridge CLI. stdout={}, stderr={}", result.stdout, result.stderr))
}

// ── Remote Bridge CLI: Set Role ──

#[tauri::command]
pub async fn bridge_set_role_remote(
    pool: tauri::State<'_, crate::commands::ssh_commands::SSHPool>,
    server_id: String,
    agent_id: String,
    role_id: String,
    url: String,
) -> Result<serde_json::Value, String> {
    let pool = pool.inner().clone();

    log::info!("[BridgeSetRole] server={}, agent={}, role={}, url={}", server_id, agent_id, role_id, url);

    crate::commands::ssh_commands::auto_connect_ssh(&pool, &server_id).await
        .map_err(|e| format!("SSH connection failed: {}", e))?;

    // Auto-deploy Bridge binary if missing or outdated (skip on mobile)
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    ensure_remote_bridge(&pool, &server_id).await
        .map_err(|e| format!("Bridge auto-deploy failed: {}", e))?;

    let input_json = serde_json::json!({
        "type": "set_role",
        "agent_id": agent_id,
        "role_id": role_id,
        "url": url
    });
    let input_str = serde_json::to_string(&input_json).map_err(|e| format!("JSON error: {}", e))?;
    let escaped = input_str.replace('\'', "'\\''");

    let cmd = format!(
        "export PATH=\"$HOME/.npm-global/bin:$HOME/.local/bin:$HOME/.cargo/bin:$PATH\" && echo '{}' | ~/echobird/echobird-bridge 2>/dev/null",
        escaped
    );

    let connections = pool.lock().await;
    let client = connections.get(&server_id)
        .ok_or_else(|| format!("SSH not connected: {}", server_id))?;

    let result = match tokio::time::timeout(
        std::time::Duration::from_secs(60),
        crate::commands::ssh_commands::execute_tolerant(client, &cmd)
    ).await {
        Ok(Ok(r)) => r,
        Ok(Err(e)) => return Err(format!("SSH exec failed: {}", e)),
        Err(_) => return Err("Set role timed out".to_string()),
    };

    drop(connections);

    // Parse response: look for role_set or error
    for line in result.stdout.lines() {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(line) {
            match json.get("type").and_then(|v| v.as_str()) {
                Some("role_set") => return Ok(json),
                Some("error") => {
                    let msg = json.get("message").and_then(|v| v.as_str()).unwrap_or("Unknown error");
                    return Err(format!("Bridge error: {}", msg));
                }
                _ => {}
            }
        }
    }

    Err("No response from Bridge CLI for set_role".to_string())
}

// ── Remote Bridge CLI: Clear Role ──

#[tauri::command]
pub async fn bridge_clear_role_remote(
    pool: tauri::State<'_, crate::commands::ssh_commands::SSHPool>,
    server_id: String,
    agent_id: String,
    role_id: String,
) -> Result<serde_json::Value, String> {
    let pool = pool.inner().clone();

    log::info!("[BridgeClearRole] server={}, agent={}, role={}", server_id, agent_id, role_id);

    crate::commands::ssh_commands::auto_connect_ssh(&pool, &server_id).await
        .map_err(|e| format!("SSH connection failed: {}", e))?;

    let input_json = serde_json::json!({
        "type": "clear_role",
        "agent_id": agent_id,
        "role_id": role_id
    });
    let input_str = serde_json::to_string(&input_json).map_err(|e| format!("JSON error: {}", e))?;
    let escaped = input_str.replace('\'', "'\\''");

    let cmd = format!(
        "export PATH=\"$HOME/.npm-global/bin:$HOME/.local/bin:$HOME/.cargo/bin:$PATH\" && echo '{}' | ~/echobird/echobird-bridge 2>/dev/null",
        escaped
    );

    let connections = pool.lock().await;
    let client = connections.get(&server_id)
        .ok_or_else(|| format!("SSH not connected: {}", server_id))?;

    let result = match tokio::time::timeout(
        std::time::Duration::from_secs(15),
        crate::commands::ssh_commands::execute_tolerant(client, &cmd)
    ).await {
        Ok(Ok(r)) => r,
        Ok(Err(e)) => return Err(format!("SSH exec failed: {}", e)),
        Err(_) => return Err("Clear role timed out".to_string()),
    };

    drop(connections);

    for line in result.stdout.lines() {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(line) {
            match json.get("type").and_then(|v| v.as_str()) {
                Some("role_cleared") => return Ok(json),
                Some("error") => {
                    let msg = json.get("message").and_then(|v| v.as_str()).unwrap_or("Unknown error");
                    return Err(format!("Bridge error: {}", msg));
                }
                _ => {}
            }
        }
    }

    Err("No response from Bridge CLI for clear_role".to_string())
}

// ── Remote Bridge CLI: Start Agent ──

#[tauri::command]
pub async fn bridge_start_agent_remote(
    pool: tauri::State<'_, crate::commands::ssh_commands::SSHPool>,
    server_id: String,
    agent_id: String,
) -> Result<serde_json::Value, String> {
    let pool = pool.inner().clone();

    log::info!("[BridgeStartAgent] server={}, agent={}", server_id, agent_id);

    crate::commands::ssh_commands::auto_connect_ssh(&pool, &server_id).await
        .map_err(|e| format!("SSH connection failed: {}", e))?;

    let input_json = serde_json::json!({
        "type": "start_agent",
        "agent_id": agent_id
    });
    let input_str = serde_json::to_string(&input_json).map_err(|e| format!("JSON error: {}", e))?;
    let escaped = input_str.replace('\'', "'\\''");

    let cmd = format!(
        "export PATH=\"$HOME/.npm-global/bin:$HOME/.local/bin:$HOME/.cargo/bin:$PATH\" && echo '{}' | ~/echobird/echobird-bridge 2>/dev/null",
        escaped
    );

    let connections = pool.lock().await;
    let client = connections.get(&server_id)
        .ok_or_else(|| format!("SSH not connected: {}", server_id))?;

    let result = match tokio::time::timeout(
        std::time::Duration::from_secs(15),
        crate::commands::ssh_commands::execute_tolerant(client, &cmd)
    ).await {
        Ok(Ok(r)) => r,
        Ok(Err(e)) => return Err(format!("SSH exec failed: {}", e)),
        Err(_) => return Err("Start agent timed out".to_string()),
    };

    drop(connections);

    for line in result.stdout.lines() {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(line) {
            match json.get("type").and_then(|v| v.as_str()) {
                Some("agent_started") => return Ok(json),
                Some("error") => {
                    let msg = json.get("message").and_then(|v| v.as_str()).unwrap_or("Unknown error");
                    return Err(format!("Bridge error: {}", msg));
                }
                _ => {}
            }
        }
    }

    Err("No response from Bridge CLI for start_agent".to_string())
}

// ── Remote Bridge CLI: Stop Agent ──

#[tauri::command]
pub async fn bridge_stop_agent_remote(
    pool: tauri::State<'_, crate::commands::ssh_commands::SSHPool>,
    server_id: String,
    agent_id: String,
) -> Result<serde_json::Value, String> {
    let pool = pool.inner().clone();

    log::info!("[BridgeStopAgent] server={}, agent={}", server_id, agent_id);

    crate::commands::ssh_commands::auto_connect_ssh(&pool, &server_id).await
        .map_err(|e| format!("SSH connection failed: {}", e))?;

    let input_json = serde_json::json!({
        "type": "stop_agent",
        "agent_id": agent_id
    });
    let input_str = serde_json::to_string(&input_json).map_err(|e| format!("JSON error: {}", e))?;
    let escaped = input_str.replace('\'', "'\\''");

    let cmd = format!(
        "export PATH=\"$HOME/.npm-global/bin:$HOME/.local/bin:$HOME/.cargo/bin:$PATH\" && echo '{}' | ~/echobird/echobird-bridge 2>/dev/null",
        escaped
    );

    let connections = pool.lock().await;
    let client = connections.get(&server_id)
        .ok_or_else(|| format!("SSH not connected: {}", server_id))?;

    let result = match tokio::time::timeout(
        std::time::Duration::from_secs(15),
        crate::commands::ssh_commands::execute_tolerant(client, &cmd)
    ).await {
        Ok(Ok(r)) => r,
        Ok(Err(e)) => return Err(format!("SSH exec failed: {}", e)),
        Err(_) => return Err("Stop agent timed out".to_string()),
    };

    drop(connections);

    for line in result.stdout.lines() {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(line) {
            match json.get("type").and_then(|v| v.as_str()) {
                Some("agent_stopped") => return Ok(json),
                Some("error") => {
                    let msg = json.get("message").and_then(|v| v.as_str()).unwrap_or("Unknown error");
                    return Err(format!("Bridge error: {}", msg));
                }
                _ => {}
            }
        }
    }

    Err("No response from Bridge CLI for stop_agent".to_string())
}

// ── Remote Model Read/Write ──

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteModelResult {
    pub model_id: String,
    pub model_name: String,
}

/// Read the current Echobird-configured model for a remote agent.
/// 1. First try Echobird marker file ~/.echobird/remote_{agent_id}.json
/// 2. Fallback: try reading agent's native config
#[tauri::command]
pub async fn bridge_get_remote_model(
    pool: tauri::State<'_, crate::commands::ssh_commands::SSHPool>,
    server_id: String,
    agent_id: String,
) -> Result<Option<RemoteModelResult>, String> {
    let pool = pool.inner().clone();

    log::info!("[BridgeGetRemoteModel] server={}, agent={}", server_id, agent_id);

    crate::commands::ssh_commands::auto_connect_ssh(&pool, &server_id).await
        .map_err(|e| format!("SSH connection failed: {}", e))?;

    // OpenClaw: read ~/.openclaw/openclaw.json (same as local)
    let cmd = if agent_id == "openclaw" {
        "cat ~/.openclaw/openclaw.json 2>/dev/null || echo 'NOT_FOUND'".to_string()
    } else {
        format!("cat ~/.echobird/remote_{}.json 2>/dev/null || echo 'NOT_FOUND'", agent_id)
    };

    let connections = pool.lock().await;
    let client = connections.get(&server_id)
        .ok_or_else(|| format!("SSH not connected: {}", server_id))?;

    let result = match tokio::time::timeout(
        std::time::Duration::from_secs(10),
        crate::commands::ssh_commands::execute_tolerant(client, &cmd)
    ).await {
        Ok(Ok(r)) => r,
        Ok(Err(e)) => return Err(format!("SSH exec failed: {}", e)),
        Err(_) => return Err("Read model timed out".to_string()),
    };

    drop(connections);

    let stdout = result.stdout.trim();
    if stdout != "NOT_FOUND" && !stdout.is_empty() {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(stdout) {
            // OpenClaw: parse agents.defaults.model.primary
            if agent_id == "openclaw" {
                if let Some(primary) = json.pointer("/agents/defaults/model/primary").and_then(|v| v.as_str()) {
                    if let Some((_provider, model_id)) = primary.split_once('/') {
                        let model_name = json.pointer(&format!("/models/providers/{}", _provider))
                            .and_then(|p| p.get("models"))
                            .and_then(|m| m.as_array())
                            .and_then(|arr| arr.iter().find(|m| m.get("id").and_then(|v| v.as_str()) == Some(model_id)))
                            .and_then(|m| m.get("name").and_then(|v| v.as_str()))
                            .unwrap_or(model_id)
                            .to_string();
                        return Ok(Some(RemoteModelResult {
                            model_id: model_id.to_string(),
                            model_name,
                        }));
                    }
                }
                return Ok(None);
            }

            // Other agents: parse marker file format
            let model_id = json.get("modelId").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let model_name = json.get("modelName").and_then(|v| v.as_str()).unwrap_or("").to_string();
            if !model_id.is_empty() {
                return Ok(Some(RemoteModelResult { model_id, model_name }));
            }
        }
    }

    Ok(None)
}


/// Write model configuration to a remote agent via Bridge protocol (SSH → Bridge).
/// Bridge handles ALL config writing — no shell commands, no direct SSH file writes.
#[tauri::command]
pub async fn bridge_set_remote_model(
    pool: tauri::State<'_, crate::commands::ssh_commands::SSHPool>,
    server_id: String,
    agent_id: String,
    model_id: String,
    model_name: String,
    api_key: String,
    base_url: String,
    api_type: String,
) -> Result<serde_json::Value, String> {
    let pool = pool.inner().clone();

    log::info!("[BridgeSetRemoteModel] server={}, agent={}, model={}, type={}",
        server_id, agent_id, model_id, api_type);

    // Decrypt API key if encrypted (enc:v1:... → plaintext)
    let api_key = crate::services::model_manager::decrypt_key_for_use(&api_key);

    crate::commands::ssh_commands::auto_connect_ssh(&pool, &server_id).await
        .map_err(|e| format!("SSH connection failed: {}", e))?;

    // Auto-deploy Bridge binary if missing or outdated (desktop only)
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    ensure_remote_bridge(&pool, &server_id).await
        .map_err(|e| format!("Bridge auto-deploy failed: {}", e))?;

    // Send set_model JSON to Bridge via SSH (same pattern as set_role)
    let input_json = serde_json::json!({
        "type": "set_model",
        "agent_id": agent_id,
        "model_id": model_id,
        "model_name": model_name,
        "api_key": api_key,
        "base_url": base_url,
        "api_type": api_type
    });
    let input_str = serde_json::to_string(&input_json).map_err(|e| format!("JSON error: {}", e))?;
    let escaped = input_str.replace('\'', "'\\''");

    let cmd = format!(
        "export PATH=\"$HOME/.npm-global/bin:$HOME/.local/bin:$HOME/.cargo/bin:$PATH\" && echo '{}' | ~/echobird/echobird-bridge 2>/dev/null",
        escaped
    );

    let connections = pool.lock().await;
    let client = connections.get(&server_id)
        .ok_or_else(|| format!("SSH not connected: {}", server_id))?;

    let result = match tokio::time::timeout(
        std::time::Duration::from_secs(30),
        crate::commands::ssh_commands::execute_tolerant(client, &cmd)
    ).await {
        Ok(Ok(r)) => r,
        Ok(Err(e)) => return Err(format!("SSH exec failed: {}", e)),
        Err(_) => return Err("Set model timed out".to_string()),
    };

    drop(connections);

    // Parse response: look for model_set or error
    for line in result.stdout.lines() {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(line) {
            match json.get("type").and_then(|v| v.as_str()) {
                Some("model_set") => {
                    log::info!("[BridgeSetRemoteModel] SUCCESS: model_set received");
                    return Ok(json);
                }
                Some("error") => {
                    let msg = json.get("message").and_then(|v| v.as_str()).unwrap_or("Unknown error");
                    log::error!("[BridgeSetRemoteModel] ERROR from bridge: {}", msg);
                    return Err(format!("Bridge error: {}", msg));
                }
                _ => {}
            }
        }
    }

    Err("No response from Bridge for set_model".to_string())
}
// ── Local Model Read/Write ──────────────────────────────────────────────────

/// Read the current Echobird-configured model for a local agent.
/// Reads marker file ~/.echobird/local_{agent_id}.json
#[tauri::command]
pub async fn bridge_get_local_model(
    agent_id: String,
) -> Result<Option<RemoteModelResult>, String> {
    log::info!("[BridgeGetLocalModel] agent={}", agent_id);

    let home = dirs::home_dir().ok_or("Cannot find home directory")?;

    // OpenClaw: read directly from ~/.openclaw/openclaw.json
    if agent_id == "openclaw" {
        let oc_path = home.join(".openclaw").join("openclaw.json");
        if !oc_path.exists() { return Ok(None); }
        let content = std::fs::read_to_string(&oc_path)
            .map_err(|e| format!("Failed to read openclaw.json: {}", e))?;
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
            if let Some(primary) = json.pointer("/agents/defaults/model/primary").and_then(|v| v.as_str()) {
                if let Some((_provider, model_id)) = primary.split_once('/') {
                    // Get display name from models array
                    let model_name = json.pointer(&format!("/models/providers/{}", _provider))
                        .and_then(|p| p.get("models"))
                        .and_then(|m| m.as_array())
                        .and_then(|arr| arr.iter().find(|m| m.get("id").and_then(|v| v.as_str()) == Some(model_id)))
                        .and_then(|m| m.get("name").and_then(|v| v.as_str()))
                        .unwrap_or(model_id)
                        .to_string();
                    return Ok(Some(RemoteModelResult {
                        model_id: model_id.to_string(),
                        model_name,
                    }));
                }
            }
        }
        return Ok(None);
    }

    // Other agents: read from marker file ~/.echobird/local_{agent_id}.json
    let marker_path = home.join(".echobird").join(format!("local_{}.json", agent_id));

    if !marker_path.exists() {
        return Ok(None);
    }

    let content = std::fs::read_to_string(&marker_path)
        .map_err(|e| format!("Failed to read marker file: {}", e))?;

    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
        let model_id = json.get("modelId").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let model_name = json.get("modelName").and_then(|v| v.as_str()).unwrap_or("").to_string();
        if !model_id.is_empty() {
            return Ok(Some(RemoteModelResult { model_id, model_name }));
        }
    }

    Ok(None)
}

/// Write model configuration for a local agent via Bridge protocol.
/// Bridge handles ALL config writing — same protocol as remote.
#[tauri::command]
pub async fn bridge_set_local_model(
    agent_id: String,
    model_id: String,
    model_name: String,
    api_key: String,
    base_url: String,
    api_type: String,
) -> Result<serde_json::Value, String> {
    log::info!("[BridgeSetLocalModel] agent={}, model={}, type={}", agent_id, model_id, api_type);

    // Decrypt API key if encrypted (enc:v1:... → plaintext)
    let api_key = crate::services::model_manager::decrypt_key_for_use(&api_key);

    // Send set_model JSON to local Bridge subprocess (same pattern as set_role)
    let result = tokio::task::spawn_blocking(move || {
        // Auto-start bridge if not running
        {
            let guard = BRIDGE_PROCESS.lock().map_err(|e| format!("Lock error: {}", e))?;
            let needs_start = guard.is_none();
            drop(guard);

            if needs_start {
                log::info!("[BridgeSetLocalModel] Bridge not running, auto-starting for agent: {}", agent_id);
                let start_result = start_bridge_internal(&agent_id)?;
                if start_result.status != "connected" {
                    return Err(format!("Failed to start bridge: {:?}", start_result.error));
                }
            }
        }

        let mut guard = BRIDGE_PROCESS.lock().map_err(|e| format!("Lock error: {}", e))?;
        let bp = guard.as_mut().ok_or_else(|| "Bridge not running".to_string())?;

        let input_json = serde_json::json!({
            "type": "set_model",
            "agent_id": agent_id,
            "model_id": model_id,
            "model_name": model_name,
            "api_key": api_key,
            "base_url": base_url,
            "api_type": api_type
        });
        let input_str = serde_json::to_string(&input_json)
            .map_err(|e| format!("JSON error: {}", e))?;

        writeln!(bp.stdin, "{}", input_str)
            .map_err(|e| format!("Failed to write to bridge stdin: {}", e))?;
        bp.stdin.flush()
            .map_err(|e| format!("Failed to flush bridge stdin: {}", e))?;

        log::info!("[BridgeSetLocalModel] Sent: {}", safe_truncate(&input_str, 100));

        // Read response: look for model_set or error
        loop {
            let mut line = String::new();
            match bp.reader.read_line(&mut line) {
                Ok(0) => return Err("Bridge EOF during set_model".to_string()),
                Ok(_) => {
                    let trimmed = line.trim();
                    if trimmed.is_empty() { continue; }
                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(trimmed) {
                        match json.get("type").and_then(|v| v.as_str()) {
                            Some("model_set") => {
                                log::info!("[BridgeSetLocalModel] SUCCESS: model_set received");
                                return Ok(json);
                            }
                            Some("error") => {
                                let msg = json.get("message").and_then(|v| v.as_str()).unwrap_or("Unknown error");
                                log::error!("[BridgeSetLocalModel] ERROR from bridge: {}", msg);
                                return Err(format!("Bridge error: {}", msg));
                            }
                            _ => continue,
                        }
                    }
                }
                Err(e) => return Err(format!("Read error: {}", e)),
            }
        }
    }).await.map_err(|e| format!("Task error: {}", e))?;

    result
}
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
