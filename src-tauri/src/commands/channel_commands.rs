// Channel commands — bridge process management + channel persistence
//
// Local channel (id=1) uses Bridge binary (plugins/openclaw/bridge-win.exe)
// as a persistent subprocess. Communication via stdin/stdout JSON (Echobird Bridge Protocol).

use crate::utils::platform::echobird_dir;
use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::Mutex;

/// Check if a port is already in use, with retry for race conditions.
/// OpenClaw Gateway may register as a scheduled task and take a moment to bind.
fn is_port_in_use(port: u16) -> bool {
    let addr: std::net::SocketAddr = format!("127.0.0.1:{}", port).parse().unwrap();
    // First attempt: 500ms timeout
    if std::net::TcpStream::connect_timeout(&addr, std::time::Duration::from_millis(500)).is_ok() {
        return true;
    }
    // Second attempt: wait a bit then retry (gateway may still be binding)
    std::thread::sleep(std::time::Duration::from_millis(800));
    std::net::TcpStream::connect_timeout(&addr, std::time::Duration::from_millis(500)).is_ok()
}

// ── Channel Config (persistence) ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelConfig {
    pub id: i32,
    pub name: String,
    pub protocol: String,
    pub address: String,
}

// ── Bridge Process (persistent subprocess) ──

struct BridgeProcess {
    child: Child,
    stdin: ChildStdin,
    reader: BufReader<ChildStdout>,
    session_id: Option<String>,
    agent_name: Option<String>,
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

    // Launch OpenClaw Gateway in a visible terminal window (like App Manager's LAUNCH APP)
    // Skip if gateway is already running (check port 18789)
    if let Some(cli) = &plugin.cli {
        let gateway_command = format!("{} gateway --allow-unconfigured", cli.command);

        // Check if gateway is already running by testing its port
        // Use longer timeout + retry because the gateway may still be starting
        // (e.g. when registered as a Windows scheduled task at boot)
        let already_running = is_port_in_use(18789);

        if already_running {
            log::info!("[Bridge] OpenClaw Gateway already running on port 18789, skipping launch");
        } else {
            log::info!("[Bridge] Launching OpenClaw Gateway: {}", gateway_command);

            #[cfg(target_os = "windows")]
            {
                use std::os::windows::process::CommandExt;
                const CREATE_NEW_PROCESS_GROUP: u32 = 0x00000200;
                const CREATE_NEW_CONSOLE: u32 = 0x00000010;
                let mut cmd = Command::new("cmd");
                cmd.args(["/C", &gateway_command]);
                cmd.creation_flags(CREATE_NEW_PROCESS_GROUP | CREATE_NEW_CONSOLE);
                match cmd.spawn() {
                    Ok(_) => log::info!("[Bridge] OpenClaw Gateway launched (Windows)"),
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

/// Start the Bridge binary as a persistent subprocess
#[tauri::command]
pub async fn bridge_start(plugin_id: Option<String>) -> Result<BridgeStartResult, String> {
    let pid = plugin_id.unwrap_or_else(|| "openclaw".to_string());
    tokio::task::spawn_blocking(move || {
        // Check if already running
        {
            let mut guard = BRIDGE_PROCESS.lock().map_err(|e| format!("Lock error: {}", e))?;
            if let Some(ref mut bp) = *guard {
                match bp.child.try_wait() {
                    Ok(None) => {
                        // Still running
                        return Ok(BridgeStartResult {
                            status: "connected".to_string(),
                            error: None,
                            agent_name: bp.agent_name.clone(), // Populated agent_name
                        });
                    }
                    _ => {
                        // Process exited, clean up
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

/// Tauri command: chat with Agent (async wrapper around blocking I/O)
#[tauri::command]
pub async fn bridge_chat_local(message: String, session_id: Option<String>) -> Result<BridgeChatResult, String> {
    tokio::task::spawn_blocking(move || {
        bridge_chat_sync(message, session_id)
    }).await.map_err(|e| format!("Task error: {}", e))?
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

    // Execute via SSH: pipe JSON into bridge binary with --command
    let cmd = format!(
        "export PATH=\"$HOME/.npm-global/bin:$HOME/.local/bin:$HOME/.cargo/bin:$PATH\" && echo '{}' | ~/echobird/echobird-bridge --command '{}' 2>/dev/null",
        escaped, agent_command
    );

    let connections = pool.lock().await;
    let client = connections.get(&server_id)
        .ok_or_else(|| format!("SSH not connected: {}", server_id))?;

    let result = match tokio::time::timeout(
        std::time::Duration::from_secs(120),
        client.execute(&cmd)
    ).await {
        Ok(Ok(r)) => r,
        Ok(Err(e)) => return Err(format!("SSH exec failed: {}", e)),
        Err(_) => return Err("Bridge chat timed out (120s)".to_string()),
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
