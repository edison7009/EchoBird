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

const EXEC_TIMEOUT_SECS: u64 = 120;
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
            if server_id.is_empty() || message.is_empty() {
                return ToolResult { success: false, output: "server_id and message are required".into() };
            }
            exec_bridge_chat(server_id, message, session_id, ssh_pool).await
        }
        "web_fetch" => {
            let url = args["url"].as_str().unwrap_or("");
            if url.is_empty() {
                return ToolResult { success: false, output: "URL is required".into() };
            }
            exec_web_fetch(url).await
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

    match client.execute(command).await {
        Ok(result) => {
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
        Err(e) => ToolResult {
            success: false,
            output: format!("SSH command failed: {}", e),
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
        // Write via SSH (using heredoc)
        let escaped_content = content.replace('\\', "\\\\").replace('$', "\\$");
        let cmd = format!("cat > {} << 'ECHOBIRD_EOF'\n{}\nECHOBIRD_EOF", shell_escape(path), escaped_content);
        exec_ssh_shell(&cmd, server_id, ssh_pool).await
    }
}

fn shell_escape(s: &str) -> String {
    // Simple quoting: not shell_escape for now, just wrap in quotes
    format!("\"{}\"", s)
}

// ── Bridge Operations ──

async fn exec_deploy_bridge(server_id: &str, plugin_id: &str, ssh_pool: &SSHPool) -> ToolResult {
    log::info!("[AgentTools] Deploying bridge '{}' to server '{}'", plugin_id, server_id);

    // Find the plugin
    let plugins = crate::services::plugin_manager::scan_plugins();
    let plugin = match plugins.iter().find(|p| p.id == plugin_id) {
        Some(p) => p,
        None => return ToolResult {
            success: false,
            output: format!("Plugin '{}' not found. Available: {}",
                plugin_id,
                plugins.iter().map(|p| p.id.as_str()).collect::<Vec<_>>().join(", ")),
        },
    };

    // Detect remote OS
    let remote_os = exec_ssh_shell("uname -s 2>/dev/null || echo windows", server_id, ssh_pool).await;
    let os_name = remote_os.output.trim().to_lowercase();

    let bridge = match &plugin.bridge {
        Some(b) => b,
        None => return ToolResult {
            success: false,
            output: format!("Plugin '{}' has no bridge binaries configured", plugin_id),
        },
    };

    let bridge_filename = if os_name.contains("linux") {
        bridge.linux.as_deref()
    } else if os_name.contains("darwin") {
        bridge.darwin.as_deref()
    } else {
        bridge.win32.as_deref()
    };

    let bridge_filename = match bridge_filename {
        Some(f) => f,
        None => return ToolResult {
            success: false,
            output: format!("No bridge binary for remote OS '{}'", os_name),
        },
    };

    // Get local bridge binary path
    let local_path = match crate::services::plugin_manager::get_bridge_path(plugin) {
        Some(p) => p,
        None => return ToolResult {
            success: false,
            output: format!("Bridge binary '{}' not found locally. Build it first.", bridge_filename),
        },
    };

    // Read and encode
    let file_data = match std::fs::read(&local_path) {
        Ok(d) => d,
        Err(e) => return ToolResult {
            success: false,
            output: format!("Failed to read bridge binary: {}", e),
        },
    };

    let encoded = {
        use base64::Engine;
        base64::engine::general_purpose::STANDARD.encode(&file_data)
    };

    // Create directory + upload + make executable
    let _ = exec_ssh_shell("mkdir -p ~/echobird", server_id, ssh_pool).await;

    let remote_path = format!("~/echobird/{}", bridge_filename);
    let upload_cmd = format!("printf '%s' '{}' | base64 -d > {} && chmod +x {}",
        encoded, remote_path, remote_path);
    let result = exec_ssh_shell(&upload_cmd, server_id, ssh_pool).await;

    if result.success {
        ToolResult {
            success: true,
            output: format!("Bridge deployed: {}:{} ({} bytes)", server_id, remote_path, file_data.len()),
        }
    } else {
        ToolResult {
            success: false,
            output: format!("Deploy failed: {}", result.output),
        }
    }
}

async fn exec_bridge_chat(
    server_id: &str,
    message: &str,
    session_id: Option<&str>,
    ssh_pool: &SSHPool,
) -> ToolResult {
    log::info!("[AgentTools] Bridge chat on {}: {}", server_id, &message[..message.len().min(100)]);

    // Build JSON input
    let input_json = if let Some(sid) = session_id {
        json!({ "type": "resume", "message": message, "session_id": sid })
    } else {
        json!({ "type": "chat", "message": message })
    };

    let input_str = serde_json::to_string(&input_json).unwrap_or_default();
    // Escape for shell single quotes
    let escaped = input_str.replace('\'', "'\\''");

    // Pipe JSON into bridge via SSH
    let cmd = format!("echo '{}' | ~/echobird/echobird-bridge 2>/dev/null", escaped);
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
