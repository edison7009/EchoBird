use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use serde::{Deserialize, Serialize};
use async_ssh2_tokio::client::{Client, AuthMethod, ServerCheckMethod};

// ── Types ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SSHServer {
    pub id: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    #[serde(default)]
    pub password: String, // Stored encrypted (enc:v1:...)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alias: Option<String>, // User-defined display name
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SSHConnectResult {
    pub success: bool,
    pub message: String,
}

// Connection pool: maps server ID → connected client
pub type SSHPool = Arc<Mutex<HashMap<String, Client>>>;

pub fn create_ssh_pool() -> SSHPool {
    Arc::new(Mutex::new(HashMap::new()))
}

// ── Persistence (config file) ──

fn ssh_config_path() -> std::path::PathBuf {
    crate::utils::platform::echobird_dir()
        .join("config")
        .join("ssh_servers.json")
}

fn ensure_config_dir() {
    let dir = crate::utils::platform::echobird_dir().join("config");
    let _ = std::fs::create_dir_all(&dir);
}

pub fn read_servers_from_disk() -> Vec<SSHServer> {
    let path = ssh_config_path();
    if !path.exists() {
        return Vec::new();
    }
    match std::fs::read_to_string(&path) {
        Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
        Err(e) => {
            log::error!("[SSH] Failed to read ssh_servers.json: {}", e);
            Vec::new()
        }
    }
}

fn write_servers_to_disk(servers: &[SSHServer]) {
    ensure_config_dir();
    let content = serde_json::to_string_pretty(servers).unwrap_or_default();
    if let Err(e) = std::fs::write(ssh_config_path(), content) {
        log::error!("[SSH] Failed to write ssh_servers.json: {}", e);
    }
}

// ── Persistence Commands ──

/// Load all saved SSH servers (passwords remain encrypted, not exposed)
#[tauri::command]
pub async fn load_ssh_servers() -> Result<Vec<SSHServer>, String> {
    let servers = read_servers_from_disk();
    log::info!("[SSH] Loaded {} saved servers", servers.len());
    Ok(servers)
}

/// Save (add or update) an SSH server with encrypted password
#[tauri::command]
pub async fn save_ssh_server(
    id: String,
    host: String,
    port: u16,
    username: String,
    password: String,
    alias: Option<String>,
) -> Result<SSHServer, String> {
    let mut servers = read_servers_from_disk();

    // On upsert: preserve existing alias if none provided
    let existing_alias = servers.iter().find(|s| s.id == id).and_then(|s| s.alias.clone());

    let server = SSHServer {
        id: id.clone(),
        host,
        port,
        username,
        password,
        alias: alias.filter(|a| !a.is_empty()).or(existing_alias),
    };

    // Upsert: replace if exists, add if new
    if let Some(idx) = servers.iter().position(|s| s.id == id) {
        servers[idx] = server.clone();
    } else {
        servers.push(server.clone());
    }

    write_servers_to_disk(&servers);
    log::info!("[SSH] Saved server: {}", id);
    Ok(server)
}

/// Remove a saved SSH server
#[tauri::command]
pub async fn remove_ssh_server(id: String) -> Result<bool, String> {
    let mut servers = read_servers_from_disk();
    let before = servers.len();
    servers.retain(|s| s.id != id);
    if servers.len() == before {
        return Ok(false);
    }
    write_servers_to_disk(&servers);
    log::info!("[SSH] Removed server: {}", id);
    Ok(true)
}

/// Decrypt an encrypted SSH password (for lock toggle UI)
#[tauri::command]
pub async fn decrypt_ssh_password(encrypted: String) -> Result<String, String> {
    use crate::services::model_manager;
    Ok(model_manager::decrypt_key_for_use(&encrypted))
}

/// Encrypt a plaintext SSH password (for lock toggle UI)
#[tauri::command]
pub async fn encrypt_ssh_password(plaintext: String) -> Result<String, String> {
    use crate::services::model_manager;
    if plaintext.is_empty() || plaintext.starts_with("enc:v1:") {
        return Ok(plaintext);
    }
    Ok(model_manager::encrypt_key_for_storage(&plaintext))
}

// ── Connection Commands ──

/// Auto-connect to an SSH server by loading saved credentials from disk.
/// Used by agent_tools when a server_id isn't in the pool.
pub async fn auto_connect_ssh(pool: &SSHPool, server_id: &str) -> Result<(), String> {
    use crate::services::model_manager;

    // Check if already connected AND connection is still alive
    {
        let connections = pool.lock().await;
        if let Some(client) = connections.get(server_id) {
            // Health check: try a quick command to verify connection is alive
            match tokio::time::timeout(
                std::time::Duration::from_secs(3),
                client.execute("echo ok")
            ).await {
                Ok(Ok(_)) => return Ok(()),  // Connection is alive
                _ => {
                    log::warn!("[SSH] Stale connection detected for '{}', will reconnect", server_id);
                }
            }
        } else {
            // No connection in pool at all
        }
    }
    // Remove stale connection before reconnecting
    {
        let mut connections = pool.lock().await;
        connections.remove(server_id);
    }

    // Load credentials from disk
    let servers = read_servers_from_disk();
    let server = servers.iter().find(|s| s.id == server_id)
        .ok_or_else(|| format!("SSH server '{}' not found in saved servers", server_id))?;

    let plain_password = model_manager::decrypt_key_for_use(&server.password);
    let auth = AuthMethod::with_password(&plain_password);
    let check = ServerCheckMethod::NoCheck;

    log::info!("[SSH] Auto-connecting to {}@{}:{}", server.username, server.host, server.port);

    match Client::connect((server.host.as_str(), server.port), server.username.as_str(), auth, check).await {
        Ok(client) => {
            let mut connections = pool.lock().await;
            connections.insert(server_id.to_string(), client);
            log::info!("[SSH] Auto-connected: {}", server_id);
            Ok(())
        }
        Err(e) => {
            Err(format!("SSH auto-connect failed for '{}': {}", server_id, e))
        }
    }
}

/// Test SSH connection (connect, run uname, disconnect)
#[tauri::command]
pub async fn ssh_test_connection(
    host: String,
    port: u16,
    username: String,
    password: String,
) -> Result<SSHConnectResult, String> {
    log::info!("SSH test connection to {}@{}:{}", username, host, port);

    // Auto-decrypt password if encrypted
    let actual_password = if password.starts_with("enc:v1:") {
        crate::services::model_manager::decrypt_key_for_use(&password)
    } else {
        password
    };

    let auth = AuthMethod::with_password(&actual_password);
    let check = ServerCheckMethod::NoCheck;

    let start = std::time::Instant::now();

    match Client::connect((host.as_str(), port), username.as_str(), auth, check).await {
        Ok(client) => {
            // Try running uname to verify command execution works
            match client.execute("uname -a").await {
                Ok(result) => {
                    let elapsed = start.elapsed().as_millis();
                    Ok(SSHConnectResult {
                        success: true,
                        message: format!("OK ({}ms) — {}", elapsed, result.stdout.trim()),
                    })
                }
                Err(e) => {
                    Ok(SSHConnectResult {
                        success: false,
                        message: format!("Connected but command failed: {}", e),
                    })
                }
            }
        }
        Err(e) => {
            // Sanitize OS-localized error messages (e.g. Chinese on zh-CN Windows)
            let raw = format!("{}", e);
            let message = if raw.contains("os error 11001") {
                "Connection failed: Host not found (DNS resolution failed)".to_string()
            } else if raw.contains("os error 10060") || raw.contains("os error 10061") {
                "Connection failed: Connection timed out or refused".to_string()
            } else if raw.contains("os error 10013") {
                "Connection failed: Permission denied by firewall".to_string()
            } else if raw.contains("os error") {
                // Strip localized text, keep only "os error XXXX"
                let clean = if let Some(pos) = raw.find("os error") {
                    let end = raw[pos..].find(')').map(|i| pos + i + 1).unwrap_or(raw.len());
                    format!("Connection failed: {}", &raw[pos..end])
                } else {
                    format!("Connection failed: {}", raw)
                };
                clean
            } else {
                format!("Connection failed: {}", raw)
            };
            Ok(SSHConnectResult {
                success: false,
                message,
            })
        }
    }
}

/// Execute an SSH command that tolerates missing exit codes.
/// Some commands (especially piped commands) don't always send an exit status
/// through the SSH channel. The standard `client.execute()` discards all collected
/// stdout/stderr and returns an error in that case. This helper uses `execute_io`
/// with `default_exit_code: Some(0)` to avoid that.
pub async fn execute_tolerant(
    client: &async_ssh2_tokio::Client,
    command: &str,
) -> Result<async_ssh2_tokio::client::CommandExecutedResult, String> {
    let (stdout_tx, mut stdout_rx) = tokio::sync::mpsc::channel::<Vec<u8>>(64);
    let (stderr_tx, mut stderr_rx) = tokio::sync::mpsc::channel::<Vec<u8>>(64);

    let exec_future = client.execute_io(
        command,
        stdout_tx,
        Some(stderr_tx),
        None,   // no stdin
        false,  // no pty
        Some(0), // default exit code if none received
    );
    tokio::pin!(exec_future);

    let mut stdout_buf = Vec::new();
    let mut stderr_buf = Vec::new();
    let mut exit_code: u32 = 0;
    let mut exec_done = false;

    loop {
        tokio::select! {
            result = &mut exec_future, if !exec_done => {
                match result {
                    Ok(code) => { exit_code = code; exec_done = true; },
                    Err(e) => return Err(format!("SSH exec error: {}", e)),
                }
            },
            data = stdout_rx.recv() => {
                match data {
                    Some(d) => stdout_buf.extend_from_slice(&d),
                    None => if exec_done { break; }, // channel closed + exec done = all done
                }
            },
            data = stderr_rx.recv() => {
                match data {
                    Some(d) => stderr_buf.extend_from_slice(&d),
                    None => {}, // stderr channel closed, keep going for stdout
                }
            },
        }
    }
    // Final drain of stderr
    while let Ok(data) = stderr_rx.try_recv() { stderr_buf.extend_from_slice(&data); }

    Ok(async_ssh2_tokio::client::CommandExecutedResult {
        stdout: String::from_utf8_lossy(&stdout_buf).to_string(),
        stderr: String::from_utf8_lossy(&stderr_buf).to_string(),
        exit_status: exit_code,
    })
}

/// Execute a long-running SSH command with line-by-line streaming and idle timeout.
///
/// Unlike `execute_tolerant` (which waits for the full result), this function reads
/// SSH stdout incrementally. For every complete line received, `on_line` is called.
/// If no output is received for `idle_timeout`, returns an error — distinguishing a
/// truly stuck connection from a working-but-slow agent that emits heartbeat lines.
///
/// Used to keep long-running SSH commands alive when they emit heartbeat-style
/// progress lines but no exit code yet.
pub async fn execute_with_heartbeat<F>(
    client: &async_ssh2_tokio::Client,
    command: &str,
    idle_timeout: std::time::Duration,
    mut on_line: F,
) -> Result<async_ssh2_tokio::client::CommandExecutedResult, String>
where
    F: FnMut(&str) + Send + Sync,
{
    let (stdout_tx, mut stdout_rx) = tokio::sync::mpsc::channel::<Vec<u8>>(64);
    let (stderr_tx, mut stderr_rx) = tokio::sync::mpsc::channel::<Vec<u8>>(64);

    let exec_future = client.execute_io(
        command,
        stdout_tx,
        Some(stderr_tx),
        None,    // no stdin
        false,   // no pty
        Some(0), // default exit code if none received
    );
    tokio::pin!(exec_future);

    let mut stdout_buf: Vec<u8> = Vec::new();
    let mut stderr_buf: Vec<u8> = Vec::new();
    let mut line_buf: Vec<u8> = Vec::new(); // accumulates bytes until '\n'
    let mut exit_code: u32 = 0;
    let mut exec_done = false;
    let mut last_output = tokio::time::Instant::now();

    loop {
        // Check idle timeout: no output for idle_timeout seconds = truly stuck
        let elapsed_since_last = last_output.elapsed();
        if elapsed_since_last >= idle_timeout {
            return Err(format!(
                "Agent idle timeout ({}s with no output). The agent may have crashed or lost connection.",
                idle_timeout.as_secs()
            ));
        }
        let remaining_idle = idle_timeout - elapsed_since_last;

        tokio::select! {
            result = &mut exec_future, if !exec_done => {
                match result {
                    Ok(code) => { exit_code = code; exec_done = true; }
                    Err(e) => return Err(format!("SSH exec error: {}", e)),
                }
            },
            data = stdout_rx.recv() => {
                match data {
                    Some(d) => {
                        last_output = tokio::time::Instant::now();
                        // Process byte by byte to detect line boundaries
                        for byte in &d {
                            if *byte == b'\n' {
                                // Got a complete line — call callback with trimmed content
                                let line = String::from_utf8_lossy(&line_buf);
                                let trimmed = line.trim();
                                if !trimmed.is_empty() {
                                    on_line(trimmed);
                                }
                                stdout_buf.extend_from_slice(&line_buf);
                                stdout_buf.push(b'\n');
                                line_buf.clear();
                            } else {
                                line_buf.push(*byte);
                            }
                        }
                    }
                    None => {
                        // stdout channel closed
                        if !line_buf.is_empty() {
                            // Flush remaining partial line
                            let line = String::from_utf8_lossy(&line_buf);
                            let trimmed = line.trim();
                            if !trimmed.is_empty() {
                                on_line(trimmed);
                            }
                            stdout_buf.extend_from_slice(&line_buf);
                            line_buf.clear();
                        }
                        if exec_done { break; }
                    }
                }
            },
            data = stderr_rx.recv() => {
                if let Some(d) = data { stderr_buf.extend_from_slice(&d); }
            },
            // Recheck idle timeout after waiting
            _ = tokio::time::sleep(remaining_idle) => {
                // Loop will check idle_timeout at top of next iteration
            },
        }
    }
    // Final drain of stderr
    while let Ok(data) = stderr_rx.try_recv() { stderr_buf.extend_from_slice(&data); }

    Ok(async_ssh2_tokio::client::CommandExecutedResult {
        stdout: String::from_utf8_lossy(&stdout_buf).to_string(),
        stderr: String::from_utf8_lossy(&stderr_buf).to_string(),
        exit_status: exit_code,
    })
}

