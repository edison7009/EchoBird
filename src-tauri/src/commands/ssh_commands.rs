use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use serde::{Deserialize, Serialize};
use async_ssh2_tokio::client::{Client, AuthMethod, ServerCheckMethod};
use tauri::State;

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SSHExecResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: u32,
    pub success: bool,
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

fn read_servers_from_disk() -> Vec<SSHServer> {
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

/// Update only the display alias of an SSH server
#[tauri::command]
pub async fn update_ssh_alias(id: String, alias: String) -> Result<bool, String> {
    let mut servers = read_servers_from_disk();
    if let Some(server) = servers.iter_mut().find(|s| s.id == id) {
        server.alias = if alias.is_empty() { None } else { Some(alias) };
        write_servers_to_disk(&servers);
        log::info!("[SSH] Updated alias for server: {}", id);
        Ok(true)
    } else {
        log::warn!("[SSH] update_ssh_alias: server '{}' not found", id);
        Ok(false)
    }
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

/// Connect to an SSH server using password auth
#[tauri::command]
pub async fn ssh_connect(
    pool: State<'_, SSHPool>,
    id: String,
    host: String,
    port: u16,
    username: String,
    password: String,
) -> Result<SSHConnectResult, String> {
    use crate::services::model_manager;
    log::info!("SSH connecting to {}@{}:{}", username, host, port);

    // Auto-decrypt if stored encrypted
    let plain_password = model_manager::decrypt_key_for_use(&password);

    let auth = AuthMethod::with_password(&plain_password);
    let check = ServerCheckMethod::NoCheck;

    match Client::connect((host.as_str(), port), username.as_str(), auth, check).await {
        Ok(client) => {
            let mut connections = pool.lock().await;
            connections.insert(id.clone(), client);
            log::info!("SSH connected: {}", id);
            Ok(SSHConnectResult {
                success: true,
                message: format!("Connected to {}:{}", host, port),
            })
        }
        Err(e) => {
            log::error!("SSH connect failed: {}", e);
            Ok(SSHConnectResult {
                success: false,
                message: format!("Connection failed: {}", e),
            })
        }
    }
}

/// Execute a command on a connected SSH server
#[tauri::command]
pub async fn ssh_execute(
    pool: State<'_, SSHPool>,
    id: String,
    command: String,
) -> Result<SSHExecResult, String> {
    let connections = pool.lock().await;

    let client = connections.get(&id).ok_or_else(|| {
        format!("No connection found for server: {}", id)
    })?;

    log::info!("SSH exec on {}: {}", id, command);

    match client.execute(&command).await {
        Ok(result) => {
            Ok(SSHExecResult {
                stdout: result.stdout,
                stderr: result.stderr,
                exit_code: result.exit_status,
                success: result.exit_status == 0,
            })
        }
        Err(e) => {
            log::error!("SSH exec failed: {}", e);
            Err(format!("Command execution failed: {}", e))
        }
    }
}

/// Disconnect from an SSH server
#[tauri::command]
pub async fn ssh_disconnect(
    pool: State<'_, SSHPool>,
    id: String,
) -> Result<bool, String> {
    let mut connections = pool.lock().await;
    if connections.remove(&id).is_some() {
        log::info!("SSH disconnected: {}", id);
        Ok(true)
    } else {
        Ok(false)
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

/// Upload a file to a remote server via SSH (base64 encoded transfer)
/// Works without SCP/SFTP — uses SSH execute channel with base64
#[tauri::command]
pub async fn ssh_upload_file(
    pool: State<'_, SSHPool>,
    id: String,
    local_path: String,
    remote_path: String,
) -> Result<SSHExecResult, String> {
    log::info!("[SSH] Uploading file {} → {}:{}", local_path, id, remote_path);

    let connections = pool.lock().await;
    let client = connections.get(&id)
        .ok_or_else(|| format!("SSH server '{}' not connected", id))?;

    // Read local file
    let file_data = std::fs::read(&local_path)
        .map_err(|e| format!("Failed to read local file {}: {}", local_path, e))?;

    let encoded = {
        use base64::Engine;
        base64::engine::general_purpose::STANDARD.encode(&file_data)
    };

    // Create remote directory if needed
    let remote_dir = std::path::Path::new(&remote_path)
        .parent()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| ".".to_string());

    let mkdir_cmd = format!("mkdir -p {}", remote_dir);
    let _ = client.execute(&mkdir_cmd).await;

    // Transfer via base64 decode
    // Split into chunks if large (SSH channel has limits)
    let chunk_size = 65536; // 64KB base64 chunks
    let chunks: Vec<&str> = encoded.as_bytes()
        .chunks(chunk_size)
        .map(|c| std::str::from_utf8(c).unwrap_or(""))
        .collect();

    if chunks.len() == 1 {
        // Small file — single command
        let cmd = format!("echo '{}' | base64 -d > {}", encoded, remote_path);
        match client.execute(&cmd).await {
            Ok(result) => {
                // Make executable
                let _ = client.execute(&format!("chmod +x {}", remote_path)).await;
                log::info!("[SSH] File uploaded successfully: {} ({} bytes)", remote_path, file_data.len());
                Ok(SSHExecResult {
                    stdout: format!("Uploaded {} bytes to {}", file_data.len(), remote_path),
                    stderr: result.stderr,
                    exit_code: result.exit_status,
                    success: result.exit_status == 0,
                })
            }
            Err(e) => Err(format!("Upload failed: {}", e)),
        }
    } else {
        // Large file — accumulate base64 text, then decode once
        // First chunk overwrites
        let first_cmd = format!("printf '%s' '{}' > {}.b64", chunks[0], remote_path);
        client.execute(&first_cmd).await
            .map_err(|e| format!("Upload chunk 0 failed: {}", e))?;

        for (i, chunk) in chunks[1..].iter().enumerate() {
            let cmd = format!("printf '%s' '{}' >> {}.b64", chunk, remote_path);
            client.execute(&cmd).await
                .map_err(|e| format!("Upload chunk {} failed: {}", i + 1, e))?;
        }

        // Decode the concatenated base64 in one shot
        let decode_cmd = format!("base64 -d {}.b64 > {} && rm {}.b64 && chmod +x {}",
            remote_path, remote_path, remote_path, remote_path);
        match client.execute(&decode_cmd).await {
            Ok(result) => {
                log::info!("[SSH] Large file uploaded: {} ({} bytes, {} chunks)",
                    remote_path, file_data.len(), chunks.len());
                Ok(SSHExecResult {
                    stdout: format!("Uploaded {} bytes to {} ({} chunks)",
                        file_data.len(), remote_path, chunks.len()),
                    stderr: result.stderr,
                    exit_code: result.exit_status,
                    success: result.exit_status == 0,
                })
            }
            Err(e) => Err(format!("Upload decode failed: {}", e)),
        }
    }
}

/// Scan plugins directory and return available agent plugins
#[tauri::command]
pub fn scan_plugins() -> Vec<crate::services::plugin_manager::PluginConfig> {
    crate::services::plugin_manager::scan_plugins()
}

/// Get the bridge binary path for a specific plugin on the current platform
#[tauri::command]
pub fn get_bridge_path(plugin_id: String) -> Result<String, String> {
    let plugins = crate::services::plugin_manager::scan_plugins();
    let plugin = plugins.iter()
        .find(|p| p.id == plugin_id)
        .ok_or_else(|| format!("Plugin '{}' not found", plugin_id))?;

    crate::services::plugin_manager::get_bridge_path(plugin)
        .map(|p| p.to_string_lossy().to_string())
        .ok_or_else(|| format!("No bridge binary found for '{}' on this platform", plugin_id))
}
