// Agent Tools — shell execution (local + SSH), file operations
// Cross-platform: Windows (PowerShell) / macOS+Linux (sh)

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::process::Command;
use std::time::Duration;
use tokio::time::timeout;

use crate::commands::ssh_commands::SSHPool;

// ── Types ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub success: bool,
    pub output: String,
}

const EXEC_TIMEOUT_SECS: u64 = 600; // 10 min — needed for Rust install + cargo build on remote
const MAX_OUTPUT_BYTES: usize = 32_000; // ~32KB cap to avoid flooding LLM context

// ── Tool Definitions (sent to LLM) ──

pub fn get_tool_definitions() -> Vec<super::llm_client::ToolDef> {
    vec![
        super::llm_client::ToolDef {
            name: "shell_exec".into(),
            description: "Execute a shell command on the local machine or a remote SSH server. \
                Use server_id to target a specific SSH server, or omit for local execution.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "The shell command to execute"
                    },
                    "server_id": {
                        "type": "string",
                        "description": "Optional SSH server ID. Omit or use 'local' for local execution."
                    }
                },
                "required": ["command"]
            }),
        },
        super::llm_client::ToolDef {
            name: "file_read".into(),
            description: "Read the contents of a file. Use server_id for remote files via SSH.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Absolute path to the file"
                    },
                    "server_id": {
                        "type": "string",
                        "description": "Optional SSH server ID"
                    }
                },
                "required": ["path"]
            }),
        },
        super::llm_client::ToolDef {
            name: "file_write".into(),
            description: "Write content to a file (creates or overwrites). Use server_id for remote files.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Absolute path to the file"
                    },
                    "content": {
                        "type": "string",
                        "description": "Content to write"
                    },
                    "server_id": {
                        "type": "string",
                        "description": "Optional SSH server ID"
                    }
                },
                "required": ["path", "content"]
            }),
        },
        super::llm_client::ToolDef {
            name: "deploy_bridge".into(),
            description: "Deploy the Echobird bridge binary to a remote SSH server. \
                This uploads the bridge program and makes it executable. \
                The bridge enables direct communication with the Agent (e.g. OpenClaw) on the remote machine.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "server_id": {
                        "type": "string",
                        "description": "SSH server ID to deploy to"
                    },
                    "plugin_id": {
                        "type": "string",
                        "description": "Plugin ID (e.g. 'openclaw')"
                    }
                },
                "required": ["server_id", "plugin_id"]
            }),
        },
        super::llm_client::ToolDef {
            name: "bridge_chat".into(),
            description: "Send a message to a remote Agent through the deployed bridge. \
                The bridge must be deployed first using deploy_bridge. \
                Returns the Agent's response text.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "server_id": {
                        "type": "string",
                        "description": "SSH server ID where bridge is deployed"
                    },
                    "message": {
                        "type": "string",
                        "description": "Message to send to the remote Agent"
                    },
                    "session_id": {
                        "type": "string",
                        "description": "Optional session ID to resume a previous conversation"
                    },
                    "plugin_id": {
                        "type": "string",
                        "description": "Agent plugin ID (e.g. 'openclaw', 'zeroclaw'). Defaults to 'openclaw' if not specified."
                    }
                },
                "required": ["server_id", "message"]
            }),
        },
        super::llm_client::ToolDef {
            name: "web_fetch".into(),
            description: "Fetch the content of a web page by URL. Returns the page text (HTML stripped). \
                Use this to read documentation, check npm packages, or look up installation guides. \
                Maximum response is 8000 characters.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "url": {
                        "type": "string",
                        "description": "The URL to fetch (must be https://)"
                    }
                },
                "required": ["url"]
            }),
        },
        super::llm_client::ToolDef {
            name: "get_sudo_password".into(),
            description: "Get the saved SSH/sudo password for a remote server. Use this when you need to run sudo commands. Pipe it: echo '<password>' | sudo -S <command>".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "server_id": {
                        "type": "string",
                        "description": "The server ID to get the password for"
                    }
                },
                "required": ["server_id"]
            }),
        },
        super::llm_client::ToolDef {
            name: "deploy_plugin_source".into(),
            description: "Deploy a plugin to a remote server by downloading a pre-compiled binary from GitHub Releases. \
                Detects remote OS and CPU architecture, downloads the correct binary (~30 seconds), \
                makes it executable, and starts the server on the specified port. \
                Use this for llm-server or any plugin. No Rust installation or compilation needed. Returns download and start status.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "server_id": {
                        "type": "string",
                        "description": "SSH server ID to deploy to"
                    },
                    "plugin_id": {
                        "type": "string",
                        "description": "Plugin ID (e.g. 'llm-server', 'openclaw')"
                    },
                    "port": {
                        "type": "integer",
                        "description": "Port for the plugin to listen on (default 8090 for llm-server)"
                    }
                },
                "required": ["server_id", "plugin_id"]
            }),
        },
        super::llm_client::ToolDef {
            name: "configure_openclaw".into(),
            description: "Configure OpenClaw on a remote server with model API credentials. \
                Writes the correct openclaw.json config file automatically. \
                Use this instead of manually writing config files with file_write.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "server_id": {
                        "type": "string",
                        "description": "SSH server ID"
                    },
                    "base_url": {
                        "type": "string",
                        "description": "Model API base URL (e.g. https://api.minimaxi.com/v1)"
                    },
                    "api_key": {
                        "type": "string",
                        "description": "API key for the model provider"
                    },
                    "model_id": {
                        "type": "string",
                        "description": "Model ID (e.g. MiniMax-M2.5, gpt-4o, claude-sonnet-4-20250514)"
                    },
                    "provider_name": {
                        "type": "string",
                        "description": "Optional provider name (e.g. minimax, openai). Auto-detected from base_url if omitted."
                    }
                },
                "required": ["server_id", "base_url", "api_key", "model_id"]
            }),
        },
    ]
}

// ── Execution ──

pub async fn execute_tool(
    name: &str,
    args_json: &str,
    ssh_pool: &SSHPool,
) -> ToolResult {
    let args: Value = match serde_json::from_str(args_json) {
        Ok(v) => v,
        Err(e) => return ToolResult {
            success: false,
            output: format!("Invalid tool arguments: {}", e),
        },
    };

    match name {
        "shell_exec" => {
            let command = args["command"].as_str().unwrap_or("");
            let server_id = args["server_id"].as_str().unwrap_or("local");
            if command.is_empty() {
                return ToolResult { success: false, output: "Empty command".into() };
            }
            exec_shell(command, server_id, ssh_pool).await
        }
        "file_read" => {
            let path = args["path"].as_str().unwrap_or("");
            let server_id = args["server_id"].as_str().unwrap_or("local");
            if path.is_empty() {
                return ToolResult { success: false, output: "Empty path".into() };
            }
            exec_file_read(path, server_id, ssh_pool).await
        }
        "file_write" => {
            let path = args["path"].as_str().unwrap_or("");
            let content = args["content"].as_str().unwrap_or("");
            let server_id = args["server_id"].as_str().unwrap_or("local");
            if path.is_empty() {
                return ToolResult { success: false, output: "Empty path".into() };
            }
            exec_file_write(path, content, server_id, ssh_pool).await
        }
        "deploy_bridge" => {
            let server_id = args["server_id"].as_str().unwrap_or("");
            let plugin_id = args["plugin_id"].as_str().unwrap_or("openclaw");
            if server_id.is_empty() {
                return ToolResult { success: false, output: "server_id is required".into() };
            }
            exec_deploy_bridge(server_id, plugin_id, ssh_pool).await
        }
        "bridge_chat" => {
            let server_id = args["server_id"].as_str().unwrap_or("");
            let message = args["message"].as_str().unwrap_or("");
            let session_id = args["session_id"].as_str();
            let plugin_id = args["plugin_id"].as_str().unwrap_or("openclaw");
            if server_id.is_empty() || message.is_empty() {
                return ToolResult { success: false, output: "server_id and message are required".into() };
            }
            exec_bridge_chat(server_id, message, session_id, plugin_id, ssh_pool).await
        }
        "web_fetch" => {
            let url = args["url"].as_str().unwrap_or("");
            if url.is_empty() {
                return ToolResult { success: false, output: "URL is required".into() };
            }
            exec_web_fetch(url).await
        }
        "get_sudo_password" => {
            let server_id = args["server_id"].as_str().unwrap_or("");
            if server_id.is_empty() {
                return ToolResult { success: false, output: "server_id is required".into() };
            }
            exec_get_sudo_password(server_id)
        }
        "deploy_plugin_source" => {
            let server_id = args["server_id"].as_str().unwrap_or("");
            let plugin_id = args["plugin_id"].as_str().unwrap_or("");
            let port = args["port"].as_u64().unwrap_or(8090) as u16;
            if server_id.is_empty() || plugin_id.is_empty() {
                return ToolResult { success: false, output: "server_id and plugin_id are required".into() };
            }
            exec_deploy_plugin_source(server_id, plugin_id, port, ssh_pool).await
        }
        "configure_openclaw" => {
            let server_id = args["server_id"].as_str().unwrap_or("");
            let base_url = args["base_url"].as_str().unwrap_or("");
            let api_key = args["api_key"].as_str().unwrap_or("");
            let model_id = args["model_id"].as_str().unwrap_or("");
            let provider_name = args["provider_name"].as_str().unwrap_or("");
            if server_id.is_empty() || base_url.is_empty() || api_key.is_empty() || model_id.is_empty() {
                return ToolResult { success: false, output: "server_id, base_url, api_key, and model_id are all required".into() };
            }
            exec_configure_openclaw(server_id, base_url, api_key, model_id, provider_name, ssh_pool).await
        }
        _ => ToolResult { success: false, output: format!("Unknown tool: {}", name) },
    }
}

// ── Shell Execution ──

async fn exec_shell(command: &str, server_id: &str, ssh_pool: &SSHPool) -> ToolResult {
    if server_id == "local" || server_id.is_empty() {
        exec_local_shell(command).await
    } else {
        exec_ssh_shell(command, server_id, ssh_pool).await
    }
}

async fn exec_local_shell(command: &str) -> ToolResult {
    log::info!("[AgentTools] Local exec: {}", &command[..command.len().min(200)]);

    // Safety check: block commands that could damage Echobird or user data
    let cmd_lower = command.to_lowercase();
    let blocked_patterns = [
        ".echobird",         // Echobird config directory
        "echobird.exe",      // Echobird process
        "stop-process",      // PowerShell kill
        "taskkill",          // Windows kill all
        "format c:",         // Format drive
        "rd /s /q c:\\",     // Delete system drive
        "rm -rf /",          // Linux nuke
    ];
    for pattern in &blocked_patterns {
        if cmd_lower.contains(pattern) {
            log::warn!("[AgentTools] BLOCKED dangerous command: {}", &command[..command.len().min(200)]);
            return ToolResult {
                success: false,
                output: format!("Command blocked: contains '{}'. This operation could damage Echobird or user data.", pattern),
            };
        }
    }
    let cmd = command.to_string();
    let result = timeout(
        Duration::from_secs(EXEC_TIMEOUT_SECS),
        tokio::task::spawn_blocking(move || {
            #[cfg(target_os = "windows")]
            let output = {
                use std::os::windows::process::CommandExt;
                const CREATE_NO_WINDOW: u32 = 0x08000000;
                Command::new("powershell")
                    .args([
                        "-NoProfile",
                        "-NonInteractive",
                        "-Command",
                        &format!(
                            "[Console]::InputEncoding = [Console]::OutputEncoding = [System.Text.Encoding]::UTF8; {}",
                            cmd
                        ),
                    ])
                    .env("PYTHONIOENCODING", "utf-8")
                    .creation_flags(CREATE_NO_WINDOW)
                    .output()
            };

            #[cfg(not(target_os = "windows"))]
            let output = Command::new("sh")
                .args(["-c", &cmd])
                .output();

            output
        }),
    ).await;

    match result {
        Ok(Ok(Ok(output))) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            let mut combined = String::new();
            if !stdout.is_empty() {
                combined.push_str(&stdout);
            }
            if !stderr.is_empty() {
                if !combined.is_empty() { combined.push_str("\n--- stderr ---\n"); }
                combined.push_str(&stderr);
            }
            // Truncate if too long
            if combined.len() > MAX_OUTPUT_BYTES {
                combined.truncate(MAX_OUTPUT_BYTES);
                combined.push_str("\n... [output truncated]");
            }
            ToolResult {
                success: output.status.success(),
                output: if combined.is_empty() {
                    format!("Command completed (exit code: {})", output.status.code().unwrap_or(-1))
                } else {
                    combined
                },
            }
        }
        Ok(Ok(Err(e))) => ToolResult {
            success: false,
            output: format!("Failed to execute command: {}", e),
        },
        Ok(Err(e)) => ToolResult {
            success: false,
            output: format!("Task join error: {}", e),
        },
        Err(_) => ToolResult {
            success: false,
            output: format!("Command timed out after {}s", EXEC_TIMEOUT_SECS),
        },
    }
}

async fn exec_ssh_shell(command: &str, server_id: &str, ssh_pool: &SSHPool) -> ToolResult {
    log::info!("[AgentTools] SSH exec on {}: {}", server_id, &command[..command.len().min(200)]);

    // Auto-connect if not in pool
    if let Err(e) = crate::commands::ssh_commands::auto_connect_ssh(ssh_pool, server_id).await {
        return ToolResult {
            success: false,
            output: format!("SSH auto-connect failed: {}", e),
        };
    }

    let connections = ssh_pool.lock().await;
    let client = match connections.get(server_id) {
        Some(c) => c,
        None => return ToolResult {
            success: false,
            output: format!("SSH server '{}' not connected after auto-connect attempt.", server_id),
        },
    };

    // Timeout to prevent hanging forever on remote commands
    match timeout(Duration::from_secs(EXEC_TIMEOUT_SECS), client.execute(command)).await {
        Ok(Ok(result)) => {
            let mut output = result.stdout;
            if !result.stderr.is_empty() {
                if !output.is_empty() { output.push_str("\n--- stderr ---\n"); }
                output.push_str(&result.stderr);
            }
            if output.len() > MAX_OUTPUT_BYTES {
                output.truncate(MAX_OUTPUT_BYTES);
                output.push_str("\n... [output truncated]");
            }
            ToolResult {
                success: result.exit_status == 0,
                output: if output.is_empty() {
                    format!("Command completed (exit code: {})", result.exit_status)
                } else {
                    output
                },
            }
        }
        Ok(Err(e)) => ToolResult {
            success: false,
            output: format!("SSH command failed: {}", e),
        },
        Err(_) => ToolResult {
            success: false,
            output: format!("SSH command timed out after {}s", EXEC_TIMEOUT_SECS),
        },
    }
}

fn exec_get_sudo_password(server_id: &str) -> ToolResult {
    use crate::commands::ssh_commands::read_servers_from_disk;
    use crate::services::model_manager;

    let servers = read_servers_from_disk();
    match servers.iter().find(|s| s.id == server_id) {
        Some(server) => {
            let plain = model_manager::decrypt_key_for_use(&server.password);
            if plain.is_empty() {
                ToolResult { success: false, output: "No password saved for this server.".into() }
            } else {
                ToolResult { success: true, output: plain }
            }
        }
        None => ToolResult {
            success: false,
            output: format!("Server '{}' not found in saved servers.", server_id),
        },
    }
}

// ── File Operations ──

async fn exec_file_read(path: &str, server_id: &str, ssh_pool: &SSHPool) -> ToolResult {
    if server_id == "local" || server_id.is_empty() {
        match tokio::fs::read_to_string(path).await {
            Ok(mut content) => {
                if content.len() > MAX_OUTPUT_BYTES {
                    content.truncate(MAX_OUTPUT_BYTES);
                    content.push_str("\n... [file truncated]");
                }
                ToolResult { success: true, output: content }
            }
            Err(e) => ToolResult { success: false, output: format!("Failed to read file: {}", e) },
        }
    } else {
        // Read via SSH
        exec_ssh_shell(&format!("cat {}", shell_escape(path)), server_id, ssh_pool).await
    }
}

async fn exec_file_write(path: &str, content: &str, server_id: &str, ssh_pool: &SSHPool) -> ToolResult {
    if server_id == "local" || server_id.is_empty() {
        // Ensure parent directory exists
        if let Some(parent) = std::path::Path::new(path).parent() {
            if let Err(e) = tokio::fs::create_dir_all(parent).await {
                return ToolResult { success: false, output: format!("Failed to create directory: {}", e) };
            }
        }
        match tokio::fs::write(path, content).await {
            Ok(_) => ToolResult { success: true, output: format!("Written {} bytes to {}", content.len(), path) },
            Err(e) => ToolResult { success: false, output: format!("Failed to write file: {}", e) },
        }
    } else {
        // Write via SSH (using heredoc), chunked for large files
        let escaped_content = content.replace('\\', "\\\\").replace('$', "\\$");
        const CHUNK_THRESHOLD: usize = 16_000; // ~16KB

        if escaped_content.len() <= CHUNK_THRESHOLD {
            // Small file: single heredoc
            let cmd = format!("mkdir -p \"$(dirname {})\" && cat > {} << 'ECHOBIRD_EOF'\n{}\nECHOBIRD_EOF", shell_escape(path), shell_escape(path), escaped_content);
            exec_ssh_shell(&cmd, server_id, ssh_pool).await
        } else {
            // Large file: split into line-aligned chunks
            let lines: Vec<&str> = escaped_content.lines().collect();
            let mut chunks: Vec<String> = Vec::new();
            let mut current = String::new();

            for line in &lines {
                if current.len() + line.len() + 1 > CHUNK_THRESHOLD && !current.is_empty() {
                    chunks.push(current);
                    current = String::new();
                }
                if !current.is_empty() {
                    current.push('\n');
                }
                current.push_str(line);
            }
            if !current.is_empty() {
                chunks.push(current);
            }

            // Ensure parent dir exists
            let mkdir_cmd = format!("mkdir -p \"$(dirname {})\"", shell_escape(path));
            let _ = exec_ssh_shell(&mkdir_cmd, server_id, ssh_pool).await;

            for (i, chunk) in chunks.iter().enumerate() {
                let redirect = if i == 0 { ">" } else { ">>" };
                let cmd = format!("cat {} {} << 'ECHOBIRD_EOF'\n{}\nECHOBIRD_EOF", redirect, shell_escape(path), chunk);
                let result = exec_ssh_shell(&cmd, server_id, ssh_pool).await;
                if !result.success {
                    return ToolResult {
                        success: false,
                        output: format!("Failed writing chunk {}/{}: {}", i + 1, chunks.len(), result.output),
                    };
                }
            }

            ToolResult {
                success: true,
                output: format!("Written {} bytes to {} ({} chunks)", content.len(), path, chunks.len()),
            }
        }
    }
}

fn shell_escape(s: &str) -> String {
    // Simple quoting: not shell_escape for now, just wrap in quotes
    format!("\"{}\"", s)
}

/// Fetch the latest published plugin version from the version API.
/// Falls back to the compile-time version if the network call fails.
async fn fetch_latest_plugin_version() -> String {
    let compile_ver = format!("v{}", env!("CARGO_PKG_VERSION"));
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(8))
        .build()
    {
        Ok(c) => c,
        Err(_) => return compile_ver,
    };
    match client
        .get("https://echobird.ai/api/version/index.json")
        .header("User-Agent", "Echobird-MotherAgent/1.0")
        .send()
        .await
    {
        Ok(resp) if resp.status().is_success() => {
            if let Ok(json) = resp.json::<serde_json::Value>().await {
                if let Some(v) = json.get("version").and_then(|v| v.as_str()) {
                    let ver = format!("v{}", v);
                    log::info!("[AgentTools] Latest plugin version from API: {}", ver);
                    return ver;
                }
            }
            compile_ver
        }
        _ => compile_ver,
    }
}

// ── Bridge Operations ──

async fn exec_deploy_bridge(server_id: &str, plugin_id: &str, ssh_pool: &SSHPool) -> ToolResult {
    log::info!("[AgentTools] Deploying bridge '{}' to server '{}' via GitHub Release download", plugin_id, server_id);

    // Detect remote OS + architecture
    let os_result = exec_ssh_shell("uname -s 2>/dev/null || echo windows", server_id, ssh_pool).await;
    let arch_result = exec_ssh_shell("uname -m 2>/dev/null || echo x86_64", server_id, ssh_pool).await;
    let os_name = os_result.output.trim().to_lowercase();
    let arch = arch_result.output.trim().to_lowercase();

    // Map OS + arch to bridge binary filename
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

    log::info!("[AgentTools] Remote: os={}, arch={}, binary={}", os_name, arch, bridge_filename);

    // Fetch latest version dynamically from version API (falls back to compile-time version)
    let version = fetch_latest_plugin_version().await;
    // Primary: Cloudflare proxy dl.echobird.ai (bypasses China GFW)
    let download_url = format!("https://dl.echobird.ai/releases/{}/{}", version, bridge_filename);
    // Fallback 1: GitHub versioned
    let github_url = format!("https://github.com/edison7009/Echobird-MotherAgent/releases/download/{}/{}", version, bridge_filename);

    log::info!("[AgentTools] Downloading bridge from: {}", download_url);

    let deploy_cmd = format!(
        "mkdir -p ~/echobird && curl -fSL --connect-timeout 30 --max-time 120 -o ~/echobird/{} '{}' && chmod +x ~/echobird/{} && ln -sf ~/echobird/{} ~/echobird/echobird-bridge",
        bridge_filename, download_url, bridge_filename, bridge_filename
    );
    let result = exec_ssh_shell(&deploy_cmd, server_id, ssh_pool).await;

    if result.success {
        // Verify the download
        let verify_cmd = format!("ls -la ~/echobird/{} && file ~/echobird/{}", bridge_filename, bridge_filename);
        let verify = exec_ssh_shell(&verify_cmd, server_id, ssh_pool).await;

        ToolResult {
            success: true,
            output: format!("Bridge deployed via download: ~/echobird/{}\n{}", bridge_filename, verify.output),
        }
    } else {
        // Fallback 1: GitHub latest (avoids in-progress CI builds)
        let latest_url = format!("https://github.com/edison7009/Echobird-MotherAgent/releases/latest/download/{}", bridge_filename);
        let fallback_cmd1 = format!(
            "curl -fSL --connect-timeout 30 --max-time 120 -o ~/echobird/{} '{}' && chmod +x ~/echobird/{} && ln -sf ~/echobird/{} ~/echobird/echobird-bridge",
            bridge_filename, latest_url, bridge_filename, bridge_filename
        );
        let fallback1 = exec_ssh_shell(&fallback_cmd1, server_id, ssh_pool).await;

        if fallback1.success {
            ToolResult {
                success: true,
                output: format!("Bridge deployed via GitHub latest: ~/echobird/{}", bridge_filename),
            }
        } else {
            // Fallback 2: GitHub versioned
            let fallback_cmd2 = format!(
                "curl -fSL --connect-timeout 30 --max-time 120 -o ~/echobird/{} '{}' && chmod +x ~/echobird/{} && ln -sf ~/echobird/{} ~/echobird/echobird-bridge",
                bridge_filename, github_url, bridge_filename, bridge_filename
            );
            let fallback2 = exec_ssh_shell(&fallback_cmd2, server_id, ssh_pool).await;

            if fallback2.success {
                ToolResult {
                    success: true,
                    output: format!("Bridge deployed via GitHub versioned: ~/echobird/{}", bridge_filename),
                }
            } else {
                ToolResult {
                    success: false,
                    output: format!("Failed to download bridge binary '{}'. Tried:\n1. {}\n2. {}\n3. {}\nError: {}",
                        bridge_filename, download_url, latest_url, github_url, fallback2.output),
                }
            }
        }
    }
}

async fn exec_configure_openclaw(
    server_id: &str,
    base_url: &str,
    api_key: &str,
    model_id: &str,
    provider_name: &str,
    ssh_pool: &SSHPool,
) -> ToolResult {
    log::info!("[AgentTools] Configuring OpenClaw on server '{}' with model '{}'", server_id, model_id);

    // Auto-detect provider name from base URL if not provided
    let provider = if !provider_name.is_empty() {
        provider_name.to_string()
    } else {
        let url_lower = base_url.to_lowercase();
        if url_lower.contains("openai") { "openai".to_string() }
        else if url_lower.contains("anthropic") { "anthropic".to_string() }
        else if url_lower.contains("minimax") { "minimax".to_string() }
        else if url_lower.contains("deepseek") { "deepseek".to_string() }
        else if url_lower.contains("google") || url_lower.contains("gemini") { "google".to_string() }
        else if url_lower.contains("groq") { "groq".to_string() }
        else if url_lower.contains("mistral") { "mistral".to_string() }
        else if url_lower.contains("together") { "together".to_string() }
        else if url_lower.contains("localhost") || url_lower.contains("127.0.0.1") { "local".to_string() }
        else { "custom".to_string() }
    };

    // Determine API type based on URL
    let api_type = if base_url.to_lowercase().contains("anthropic") {
        "anthropic"
    } else {
        "openai-completions"
    };

    // Build the config JSON
    let config = json!({
        "models": {
            "providers": {
                &provider: {
                    "baseUrl": base_url,
                    "apiKey": api_key,
                    "api": api_type,
                    "auth": "api-key",
                    "authHeader": true,
                    "models": [{
                        "id": model_id,
                        "name": model_id,
                        "contextWindow": 128000,
                        "maxTokens": 8192,
                        "cost": { "input": 0, "output": 0 }
                    }]
                }
            }
        },
        "agents": {
            "defaults": {
                "model": {
                    "primary": format!("{}/{}", provider, model_id)
                }
            }
        }
    });

    let config_str = serde_json::to_string_pretty(&config).unwrap_or_default();

    // Create directory and write config via SSH
    let mkdir_cmd = "mkdir -p ~/.openclaw";
    let mkdir_result = exec_ssh_shell(mkdir_cmd, server_id, ssh_pool).await;
    if !mkdir_result.success {
        return ToolResult {
            success: false,
            output: format!("Failed to create ~/.openclaw directory: {}", mkdir_result.output),
        };
    }

    // Write config file using heredoc (reliable for multi-line JSON)
    let write_cmd = format!(
        "cat > ~/.openclaw/openclaw.json << 'ECHOBIRD_EOF'\n{}\nECHOBIRD_EOF",
        config_str
    );
    let write_result = exec_ssh_shell(&write_cmd, server_id, ssh_pool).await;
    if !write_result.success {
        return ToolResult {
            success: false,
            output: format!("Failed to write openclaw.json: {}", write_result.output),
        };
    }

    // Verify the file was written correctly
    let verify_cmd = "cat ~/.openclaw/openclaw.json | head -5";
    let verify_result = exec_ssh_shell(verify_cmd, server_id, ssh_pool).await;

    ToolResult {
        success: true,
        output: format!(
            "OpenClaw configured successfully!\n\
            Provider: {}\n\
            Model: {}/{}\n\
            Base URL: {}\n\
            Config file: ~/.openclaw/openclaw.json\n\
            Verify: {}",
            provider, provider, model_id, base_url, verify_result.output.trim()
        ),
    }
}

async fn exec_deploy_plugin_source(
    server_id: &str,
    plugin_id: &str,
    port: u16,
    ssh_pool: &SSHPool,
) -> ToolResult {
    log::info!("[AgentTools] Deploying plugin '{}' to server '{}' via GitHub Release download", plugin_id, server_id);

    // 1. Detect remote OS + architecture
    let os_result = exec_ssh_shell("uname -s 2>/dev/null || echo windows", server_id, ssh_pool).await;
    let arch_result = exec_ssh_shell("uname -m 2>/dev/null || echo x86_64", server_id, ssh_pool).await;
    let os_name = os_result.output.trim().to_lowercase();
    let arch = arch_result.output.trim().to_lowercase();

    // Map plugin_id to binary name pattern
    let (_binary_name, binary_filename) = if os_name.contains("linux") {
        if arch.contains("aarch64") || arch.contains("arm64") {
            (format!("{}-linux-aarch64", plugin_id), format!("{}-linux-aarch64", plugin_id))
        } else {
            (format!("{}-linux-x86_64", plugin_id), format!("{}-linux-x86_64", plugin_id))
        }
    } else if os_name.contains("darwin") {
        if arch.contains("arm64") || arch.contains("aarch64") {
            (format!("{}-darwin-aarch64", plugin_id), format!("{}-darwin-aarch64", plugin_id))
        } else {
            (format!("{}-darwin-x86_64", plugin_id), format!("{}-darwin-x86_64", plugin_id))
        }
    } else {
        (format!("{}-win.exe", plugin_id), format!("{}-win.exe", plugin_id))
    };

    log::info!("[AgentTools] Remote: os={}, arch={}, binary={}", os_name, arch, binary_filename);

    let mut log_output = String::new();

    // 2. Fetch latest version dynamically from version API (falls back to compile-time version)
    let version = fetch_latest_plugin_version().await;
    // Primary: Cloudflare proxy (GFW-friendly)
    let cloudflare_url = format!("https://dl.echobird.ai/releases/{}/{}", version, binary_filename);
    // Fallback 1: GitHub versioned
    let github_url = format!("https://github.com/edison7009/Echobird-MotherAgent/releases/download/{}/{}", version, binary_filename);
    // Fallback 2: GitHub latest
    let github_latest_url = format!("https://github.com/edison7009/Echobird-MotherAgent/releases/latest/download/{}", binary_filename);

    log_output.push_str(&format!("[1/4] Downloading {} ...\n", binary_filename));

    // 3. Download binary — Cloudflare first, GitHub fallback
    let deploy_dir = "~/echobird";
    let download_cmd = |url: &str| format!(
        "mkdir -p {} && rm -rf {}/{} && curl -fSL --connect-timeout 15 --max-time 90 -o {}/{} '{}' && chmod +x {}/{}",
        deploy_dir, deploy_dir, binary_filename, deploy_dir, binary_filename, url, deploy_dir, binary_filename
    );

    let result = exec_ssh_shell(&download_cmd(&cloudflare_url), server_id, ssh_pool).await;
    if !result.success {
        log_output.push_str("  Cloudflare mirror failed, trying GitHub versioned...\n");
        let result2 = exec_ssh_shell(&download_cmd(&github_url), server_id, ssh_pool).await;
        if !result2.success {
            log_output.push_str("  GitHub versioned failed, trying GitHub latest...\n");
            let result3 = exec_ssh_shell(&download_cmd(&github_latest_url), server_id, ssh_pool).await;
            if !result3.success {
                return ToolResult {
                    success: false,
                    output: format!("Failed to download '{}'. Tried:\n1. {}\n2. {}\n3. {}\nError: {}", binary_filename, cloudflare_url, github_url, github_latest_url, result3.output),
                };
            }
        }
    }
    log_output.push_str("[2/4] Binary downloaded and ready\n");

    // 4. Stop any existing instance
    log_output.push_str("[3/4] Starting server...\n");
    let _ = exec_ssh_shell(
        &format!("pkill -f '{}/{}' 2>/dev/null; sleep 1", deploy_dir, binary_filename),
        server_id, ssh_pool
    ).await;

    // 5. Start the server
    let start_result = exec_ssh_shell(
        &format!(
            "nohup {}/{} {} > /tmp/{}.log 2>&1 & sleep 2 && pgrep -f '{}' && echo 'STARTED_OK'",
            deploy_dir, binary_filename, port, plugin_id, binary_filename
        ),
        server_id, ssh_pool
    ).await;

    if start_result.output.contains("STARTED_OK") {
        log_output.push_str(&format!("[4/4] Server started on port {}\n", port));

        // Quick API health check
        let health = exec_ssh_shell(
            &format!("curl -s http://localhost:{}/api/status 2>&1 || echo 'API_NOT_READY'", port),
            server_id, ssh_pool
        ).await;
        if !health.output.contains("API_NOT_READY") {
            log_output.push_str(&format!("  API health check: {}\n", health.output.trim()));
        }

        ToolResult {
            success: true,
            output: format!("{}Plugin '{}' deployed and running on port {}. \
                User should go to Channels page → Remote LLM Panel to manage models.", log_output, plugin_id, port),
        }
    } else {
        let log_check = exec_ssh_shell(
            &format!("cat /tmp/{}.log 2>/dev/null | tail -10", plugin_id),
            server_id, ssh_pool
        ).await;
        ToolResult {
            success: false,
            output: format!("{}Server failed to start. Logs:\n{}", log_output, log_check.output),
        }
    }
}

async fn exec_bridge_chat(
    server_id: &str,
    message: &str,
    session_id: Option<&str>,
    plugin_id: &str,
    ssh_pool: &SSHPool,
) -> ToolResult {
    log::info!("[AgentTools] Bridge chat on {} (plugin: {}): {}", server_id, plugin_id, &message[..message.len().min(100)]);

    // Map plugin_id to agent CLI command
    let agent_command = match plugin_id {
        "openclaw" => "openclaw agent --json --agent main",
        "zeroclaw" => "zeroclaw agent --json",
        "nanoclaw" => "nanoclaw agent --json",
        other => other, // allow custom commands
    };

    // Build JSON input
    let input_json = if let Some(sid) = session_id {
        json!({ "type": "resume", "message": message, "session_id": sid })
    } else {
        json!({ "type": "chat", "message": message })
    };

    let input_str = serde_json::to_string(&input_json).unwrap_or_default();
    // Escape for shell single quotes
    let escaped = input_str.replace('\'', "'\\''");

    // Pipe JSON into bridge via SSH (must set PATH for non-interactive SSH sessions)
    let cmd = format!("export PATH=\"$HOME/.npm-global/bin:$HOME/.local/bin:$HOME/.cargo/bin:$PATH\" && echo '{}' | ~/echobird/echobird-bridge --command '{}' 2>/dev/null", escaped, agent_command);
    let result = exec_ssh_shell(&cmd, server_id, ssh_pool).await;

    if !result.success {
        return ToolResult {
            success: false,
            output: format!("Bridge execution failed: {}", result.output),
        };
    }

    // Parse bridge JSON output
    let mut response_text = String::new();
    let mut final_session_id: Option<String> = None;
    let mut had_error = false;

    for line in result.output.lines() {
        if let Ok(json) = serde_json::from_str::<Value>(line) {
            match json.get("type").and_then(|v| v.as_str()) {
                Some("text") => {
                    if let Some(text) = json.get("text").and_then(|v| v.as_str()) {
                        response_text.push_str(text);
                    }
                    if let Some(sid) = json.get("session_id").and_then(|v| v.as_str()) {
                        final_session_id = Some(sid.to_string());
                    }
                }
                Some("done") => {
                    if let Some(sid) = json.get("session_id").and_then(|v| v.as_str()) {
                        final_session_id = Some(sid.to_string());
                    }
                }
                Some("error") => {
                    if let Some(msg) = json.get("message").and_then(|v| v.as_str()) {
                        response_text.push_str(&format!("[Error] {}\n", msg));
                        had_error = true;
                    }
                }
                _ => {}
            }
        }
    }

    if let Some(sid) = &final_session_id {
        response_text.push_str(&format!("\n[session_id: {}]", sid));
    }

    if response_text.is_empty() {
        response_text = result.output;
    }

    ToolResult {
        success: !had_error,
        output: response_text,
    }
}

// ── Platform Info (for system prompt) ──

pub fn get_local_platform_info() -> String {
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;

    let (shell, pkg_manager) = match os {
        "windows" => ("PowerShell", "winget, choco, or scoop"),
        "macos" => ("zsh", "brew"),
        "linux" => ("bash", "apt, dnf, or pacman"),
        _ => ("sh", "unknown"),
    };

    let mut info = format!("Local machine: {} ({})", os, arch);
    info.push_str(&format!(", Shell: {}", shell));
    info.push_str(&format!(", Package manager: {}", pkg_manager));
    info
}

/// Get available plugins info for system prompt
pub fn get_plugins_info() -> String {
    let plugins = crate::services::plugin_manager::scan_plugins();
    if plugins.is_empty() {
        return String::new();
    }

    let mut info = "\n\nAvailable Agent Plugins:".to_string();
    for p in &plugins {
        info.push_str(&format!("\n  - {} (id: {})", p.name, p.id));
        if let Some(cli) = &p.cli {
            info.push_str(&format!(", CLI: {}", cli.command));
        }
        let has_bridge = crate::services::plugin_manager::get_bridge_path(p).is_some();
        info.push_str(&format!(", Bridge ready: {}", has_bridge));
    }
    info.push_str("\n\nTo connect a remote Agent:");
    info.push_str("\n  1. Use deploy_bridge to upload the bridge binary to the remote server");
    info.push_str("\n  2. Use bridge_chat to send messages through the bridge");
    info
}

// ── Web Fetch ──

const WEB_FETCH_MAX_CHARS: usize = 8000;
const WEB_FETCH_TIMEOUT_SECS: u64 = 15;

async fn exec_web_fetch(url: &str) -> ToolResult {
    // Only allow HTTPS
    if !url.starts_with("https://") {
        return ToolResult {
            success: false,
            output: "Only HTTPS URLs are allowed".into(),
        };
    }

    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(WEB_FETCH_TIMEOUT_SECS))
        .build()
    {
        Ok(c) => c,
        Err(e) => return ToolResult {
            success: false,
            output: format!("Failed to create HTTP client: {}", e),
        },
    };

    let response = match client.get(url)
        .header("User-Agent", "Echobird-MotherAgent/1.0")
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => return ToolResult {
            success: false,
            output: format!("Request failed: {}", e),
        },
    };

    let status = response.status();
    if !status.is_success() {
        return ToolResult {
            success: false,
            output: format!("HTTP {}: {}", status.as_u16(), status.canonical_reason().unwrap_or("Error")),
        };
    }

    let body = match response.text().await {
        Ok(t) => t,
        Err(e) => return ToolResult {
            success: false,
            output: format!("Failed to read response: {}", e),
        },
    };

    // Strip HTML tags (rough but effective for most pages)
    let text = strip_html_tags(&body);

    // Collapse whitespace
    let text: String = text
        .split_whitespace()
        .collect::<Vec<&str>>()
        .join(" ");

    // Truncate to max chars
    let truncated = if text.len() > WEB_FETCH_MAX_CHARS {
        format!("{}...\n[Truncated: {} chars total]", &text[..WEB_FETCH_MAX_CHARS], text.len())
    } else {
        text
    };

    ToolResult {
        success: true,
        output: truncated,
    }
}

fn strip_html_tags(html: &str) -> String {
    let mut result = String::with_capacity(html.len());
    let mut in_tag = false;
    let mut in_script = false;
    let lower = html.to_lowercase();
    let chars: Vec<char> = html.chars().collect();
    let lower_chars: Vec<char> = lower.chars().collect();

    let mut i = 0;
    while i < chars.len() {
        if !in_tag && i + 7 < lower_chars.len() {
            let window: String = lower_chars[i..i+7].iter().collect();
            if window == "<script" {
                in_script = true;
            }
        }
        if in_script && i + 8 < lower_chars.len() {
            let window: String = lower_chars[i..i+9].iter().collect();
            if window == "</script>" {
                in_script = false;
                i += 9;
                continue;
            }
        }
        if in_script {
            i += 1;
            continue;
        }
        if chars[i] == '<' {
            in_tag = true;
        } else if chars[i] == '>' {
            in_tag = false;
            result.push(' ');
        } else if !in_tag {
            result.push(chars[i]);
        }
        i += 1;
    }
    result
}
