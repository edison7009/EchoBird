// Agent Tools — shell execution (local + SSH), file operations
// Cross-platform: Windows (PowerShell) / macOS+Linux (sh)

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::process::{Command, Stdio};
use std::time::Duration;
use tokio::time::timeout;

use crate::commands::ssh_commands::SSHPool;

// ── Types ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub success: bool,
    pub output: String,
}

const EXEC_TIMEOUT_SECS: u64 = 60; // Default: 60s for most commands (systemctl, pkill, ls, etc.)
const EXEC_TIMEOUT_LONG_SECS: u64 = 600; // Long: 10 min for installs, builds, downloads
const MAX_OUTPUT_BYTES: usize = 8_000; // ~8KB per tool result to keep API payload manageable

/// Find the largest byte index <= max that is a UTF-8 character boundary.
/// Equivalent to String::floor_char_boundary (Rust 1.91+) but works on 1.77.2.
fn floor_char_boundary(s: &str, max: usize) -> usize {
    if max >= s.len() {
        return s.len();
    }
    let mut idx = max;
    while idx > 0 && !s.is_char_boundary(idx) {
        idx -= 1;
    }
    idx
}

/// Find the smallest byte index >= min that is a UTF-8 character boundary.
/// Equivalent to String::ceil_char_boundary (Rust 1.91+) but works on 1.77.2.
fn ceil_char_boundary(s: &str, min: usize) -> usize {
    if min >= s.len() {
        return s.len();
    }
    let mut idx = min;
    while idx < s.len() && !s.is_char_boundary(idx) {
        idx += 1;
    }
    idx
}

/// Return the appropriate timeout for a shell command.
/// Long-running operations (package installs, builds, downloads) get 600s.
/// Everything else gets 60s so users don't wait 10 min for a hung simple command.
fn get_exec_timeout(command: &str) -> u64 {
    let cmd = command.to_lowercase();
    let long_patterns = [
        "npm install",
        "npm ci",
        "npm run build",
        "cargo install",
        "cargo build",
        "pip install",
        "pip3 install",
        "apt install",
        "apt-get install",
        "apt upgrade",
        "apt-get upgrade",
        "dnf install",
        "dnf upgrade",
        "dnf update",
        "yum install",
        "yum upgrade",
        "yum update",
        "pacman -s",
        "pacman -syu",
        "pacman -syyu",
        "zypper install",
        "zypper in ",
        "zypper update",
        "zypper up",
        "apk add",
        "apk upgrade",
        "brew install",
        "curl",
        "wget",
        "tar ",
        "unzip ",
        "huggingface-cli",
        "modelscope",
        "docker pull",
        "docker build",
        "nohup",
        "install.sh",
        "install.ps1",
        "cargo check",
        "cargo test",
        "yarn install",
        "yarn build",
        "git clone",
        "dpkg -i",
        "snap install",
        "nvm install",
    ];
    if long_patterns.iter().any(|p| cmd.contains(p)) {
        EXEC_TIMEOUT_LONG_SECS
    } else {
        EXEC_TIMEOUT_SECS
    }
}

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
            name: "file_edit".into(),
            description: "Edit a file by replacing exactly one occurrence of `old_string` with `new_string`. \
                Fails if `old_string` is not found, or appears more than once (provide more context to disambiguate). \
                Prefer this over `file_write` for small changes — it preserves the rest of the file and is much cheaper on tokens. \
                Use `server_id` to target a remote SSH server.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Absolute path to the file" },
                    "old_string": { "type": "string", "description": "Exact substring to replace (must match once and only once)" },
                    "new_string": { "type": "string", "description": "Replacement string" },
                    "server_id": { "type": "string", "description": "Optional SSH server ID. Omit or use 'local' for local execution." }
                },
                "required": ["path", "old_string", "new_string"]
            }),
        },
        super::llm_client::ToolDef {
            name: "grep".into(),
            description: "Search for a pattern across files in a directory tree. \
                Returns up to 200 matching lines as `path:lineno:content`. \
                Use this instead of running `grep` via `shell_exec` — output is normalized and capped. \
                Use `server_id` for remote search.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "pattern": { "type": "string", "description": "Regex pattern (POSIX extended on Unix, .NET-flavored on Windows)" },
                    "path": { "type": "string", "description": "Directory to search (default: current working dir on local, $HOME on remote)" },
                    "case_insensitive": { "type": "boolean", "description": "Match case-insensitively (default false)" },
                    "server_id": { "type": "string", "description": "Optional SSH server ID. Omit or use 'local' for local execution." }
                },
                "required": ["pattern"]
            }),
        },
        super::llm_client::ToolDef {
            name: "glob".into(),
            description: "List files matching a glob/name pattern under a directory. \
                Returns up to 200 paths. Use this to locate config files, binaries, etc. \
                Use `server_id` for remote search.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "pattern": { "type": "string", "description": "Filename glob, e.g. '*.toml' or 'openclaw*'" },
                    "path": { "type": "string", "description": "Directory to search (default: current working dir on local, $HOME on remote)" },
                    "server_id": { "type": "string", "description": "Optional SSH server ID. Omit or use 'local' for local execution." }
                },
                "required": ["pattern"]
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
            description: "Get the saved sudo password for a REMOTE SSH server. \
                Pass the server_id and you get back the saved SSH password, then run: echo '<password>' | sudo -S <command>. \
                Do NOT call this for the local machine — there is no saved local password. \
                For LOCAL sudo, the default is hand-off: print the copy-paste install commands plus the tool's \
                homepage/docs URL in your <chat> reply and let the user run them in their own terminal. The user \
                MAY volunteer their password in chat if they prefer; if they do, use it directly via \
                `echo '<pwd>' | sudo -S` and mask it as '***' in any output. \
                Always prefer non-sudo alternatives first (nvm, pip install --user, cargo install).".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "server_id": {
                        "type": "string",
                        "description": "Server ID. Use 'local' (or omit) for the local machine, or a remote SSH server ID."
                    }
                }
            }),
        },
        super::llm_client::ToolDef {
            name: "deploy_plugin_source".into(),
            description: "Deploy a plugin to a remote server by downloading a pre-compiled binary from GitHub Releases. \
                Detects remote OS and CPU architecture, downloads the correct binary (~30 seconds), \
                makes it executable, and starts the server on the specified port. No compilation needed. Returns download and start status.".into(),
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
                    },
                    "port": {
                        "type": "integer",
                        "description": "Port for the plugin to listen on"
                    }
                },
                "required": ["server_id", "plugin_id"]
            }),
        },
        super::llm_client::ToolDef {
            name: "upload_file".into(),
            description: "Upload a local file to a remote server via SSH. \
                Transfers the file using base64 encoding through the SSH channel (no SCP/SFTP needed). \
                Supports large files (chunked transfer). The uploaded file is automatically made executable (chmod +x). \
                Use this to deploy binaries, scripts, or any file from this machine to a remote server.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "local_path": {
                        "type": "string",
                        "description": "Absolute path to the local file to upload"
                    },
                    "remote_path": {
                        "type": "string",
                        "description": "Absolute path on the remote server where the file should be placed"
                    },
                    "server_id": {
                        "type": "string",
                        "description": "SSH server ID to upload to"
                    }
                },
                "required": ["local_path", "remote_path", "server_id"]
            }),
        },
        super::llm_client::ToolDef {
            name: "download_file".into(),
            description: "Download a file from a remote server to this local machine via SSH. \
                Transfers the file using base64 encoding through the SSH channel (no SCP/SFTP needed). \
                Use this to retrieve logs, config files, crash dumps, or any file from a remote server.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "remote_path": {
                        "type": "string",
                        "description": "Absolute path to the file on the remote server"
                    },
                    "local_path": {
                        "type": "string",
                        "description": "Absolute path on this local machine where the file should be saved"
                    },
                    "server_id": {
                        "type": "string",
                        "description": "SSH server ID to download from"
                    }
                },
                "required": ["remote_path", "local_path", "server_id"]
            }),
        },
    ]
}

// ── Execution ──

pub async fn execute_tool(
    name: &str,
    args_json: &str,
    ssh_pool: &SSHPool,
    session_server_ids: &[String],
) -> ToolResult {
    let args: Value = match serde_json::from_str(args_json) {
        Ok(v) => v,
        Err(e) => {
            return ToolResult {
                success: false,
                output: format!("Invalid tool arguments: {}", e),
            }
        }
    };

    // Determine effective server_id: if model omitted it (or said "local") but the
    // session targets a remote server, auto-redirect to that server.
    // This prevents the model from accidentally running commands on the local machine.
    let effective_remote: Option<&str> = session_server_ids
        .iter()
        .find(|s| s.as_str() != "local" && !s.is_empty())
        .map(|s| s.as_str());

    // Returns owned String to avoid lifetime issues with the closure.
    let resolve_server_id = |raw: Option<&str>| -> String {
        let sid = raw.unwrap_or("local");
        if sid == "local" || sid.is_empty() {
            if let Some(remote) = effective_remote {
                log::warn!(
                    "[AgentTools] GUARD: model omitted server_id (got '{}'), \
                     auto-redirecting to session target '{}'",
                    sid,
                    remote
                );
                return remote.to_string();
            }
        }
        sid.to_string()
    };

    match name {
        "shell_exec" => {
            let command = args["command"].as_str().unwrap_or("");
            let raw_sid = args["server_id"].as_str();
            if command.is_empty() {
                return ToolResult {
                    success: false,
                    output: "Empty command".into(),
                };
            }
            let server_id = resolve_server_id(raw_sid);
            exec_shell(command, &server_id, ssh_pool).await
        }
        "file_read" => {
            let path = args["path"].as_str().unwrap_or("");
            let raw_sid = args["server_id"].as_str();
            if path.is_empty() {
                return ToolResult {
                    success: false,
                    output: "Empty path".into(),
                };
            }
            let server_id = resolve_server_id(raw_sid);
            exec_file_read(path, &server_id, ssh_pool).await
        }
        "file_write" => {
            let path = args["path"].as_str().unwrap_or("");
            let content = args["content"].as_str().unwrap_or("");
            let raw_sid = args["server_id"].as_str();
            if path.is_empty() {
                return ToolResult {
                    success: false,
                    output: "Empty path".into(),
                };
            }
            let server_id = resolve_server_id(raw_sid);
            exec_file_write(path, content, &server_id, ssh_pool).await
        }
        "file_edit" => {
            let path = args["path"].as_str().unwrap_or("");
            let old_string = args["old_string"].as_str().unwrap_or("");
            let new_string = args["new_string"].as_str().unwrap_or("");
            let raw_sid = args["server_id"].as_str();
            if path.is_empty() {
                return ToolResult {
                    success: false,
                    output: "Empty path".into(),
                };
            }
            if old_string.is_empty() {
                return ToolResult {
                    success: false,
                    output: "old_string cannot be empty".into(),
                };
            }
            let server_id = resolve_server_id(raw_sid);
            exec_file_edit(path, old_string, new_string, &server_id, ssh_pool).await
        }
        "grep" => {
            let pattern = args["pattern"].as_str().unwrap_or("");
            let path = args["path"].as_str();
            let ci = args["case_insensitive"].as_bool().unwrap_or(false);
            let raw_sid = args["server_id"].as_str();
            if pattern.is_empty() {
                return ToolResult {
                    success: false,
                    output: "pattern is required".into(),
                };
            }
            let server_id = resolve_server_id(raw_sid);
            exec_grep(pattern, path, ci, &server_id, ssh_pool).await
        }
        "glob" => {
            let pattern = args["pattern"].as_str().unwrap_or("");
            let path = args["path"].as_str();
            let raw_sid = args["server_id"].as_str();
            if pattern.is_empty() {
                return ToolResult {
                    success: false,
                    output: "pattern is required".into(),
                };
            }
            let server_id = resolve_server_id(raw_sid);
            exec_glob(pattern, path, &server_id, ssh_pool).await
        }
        "web_fetch" => {
            let url = args["url"].as_str().unwrap_or("");
            if url.is_empty() {
                return ToolResult {
                    success: false,
                    output: "URL is required".into(),
                };
            }
            exec_web_fetch(url).await
        }
        "get_sudo_password" => {
            let raw_sid = args["server_id"].as_str();
            let server_id = resolve_server_id(raw_sid);
            exec_get_sudo_password(&server_id)
        }
        "deploy_plugin_source" => {
            let server_id = args["server_id"].as_str().unwrap_or("");
            let plugin_id = args["plugin_id"].as_str().unwrap_or("");
            let port = args["port"].as_u64().unwrap_or(8090) as u16;
            if server_id.is_empty() || plugin_id.is_empty() {
                return ToolResult {
                    success: false,
                    output: "server_id and plugin_id are required".into(),
                };
            }
            exec_deploy_plugin_source(server_id, plugin_id, port, ssh_pool).await
        }
        "upload_file" => {
            let local_path = args["local_path"].as_str().unwrap_or("");
            let remote_path = args["remote_path"].as_str().unwrap_or("");
            let server_id = args["server_id"].as_str().unwrap_or("");
            if local_path.is_empty() || remote_path.is_empty() || server_id.is_empty() {
                return ToolResult {
                    success: false,
                    output: "local_path, remote_path, and server_id are all required".into(),
                };
            }
            exec_upload_file(local_path, remote_path, server_id, ssh_pool).await
        }
        "download_file" => {
            let remote_path = args["remote_path"].as_str().unwrap_or("");
            let local_path = args["local_path"].as_str().unwrap_or("");
            let server_id = args["server_id"].as_str().unwrap_or("");
            if remote_path.is_empty() || local_path.is_empty() || server_id.is_empty() {
                return ToolResult {
                    success: false,
                    output: "remote_path, local_path, and server_id are all required".into(),
                };
            }
            exec_download_file(remote_path, local_path, server_id, ssh_pool).await
        }
        _ => ToolResult {
            success: false,
            output: format!("Unknown tool: {}", name),
        },
    }
}

// ── Shell Execution ──

pub async fn exec_shell(command: &str, server_id: &str, ssh_pool: &SSHPool) -> ToolResult {
    if server_id == "local" || server_id.is_empty() {
        exec_local_shell(command).await
    } else {
        exec_ssh_shell(command, server_id, ssh_pool).await
    }
}

async fn exec_local_shell(command: &str) -> ToolResult {
    log::info!(
        "[AgentTools] Local exec: {}",
        &command[..floor_char_boundary(command, 200)]
    );

    // Safety check: block commands that could damage Echobird or user data
    let cmd_lower = command.to_lowercase();
    let blocked_patterns = [
        ".echobird",     // Echobird config directory
        "echobird.exe",  // Echobird process
        "stop-process",  // PowerShell kill
        "taskkill",      // Windows kill all
        "format c:",     // Format drive
        "rd /s /q c:\\", // Delete system drive
        "rm -rf /",      // Linux nuke
    ];
    for pattern in &blocked_patterns {
        if cmd_lower.contains(pattern) {
            log::warn!(
                "[AgentTools] BLOCKED dangerous command: {}",
                &command[..floor_char_boundary(command, 200)]
            );
            return ToolResult {
                success: false,
                output: format!("Command blocked: contains '{}'. This operation could damage Echobird or user data.", pattern),
            };
        }
    }
    let cmd = command.to_string();
    let timeout_secs = get_exec_timeout(command);
    let result = timeout(
        Duration::from_secs(timeout_secs),
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
                    .stdin(Stdio::null())
                    .output()
            };

            // Use bash (not sh) so `source`, `[[ ]]`, arrays etc. work on Debian/Ubuntu
            // where /bin/sh is dash. Stdio::null() makes interactive prompts (sudo, ssh-keygen)
            // fail fast instead of blocking until the 60s/600s timeout.
            #[cfg(not(target_os = "windows"))]
            let output = Command::new("bash")
                .args(["-c", &cmd])
                .stdin(Stdio::null())
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
                if !combined.is_empty() {
                    combined.push_str("\n--- stderr ---\n");
                }
                combined.push_str(&stderr);
            }
            // Truncate if too long — keep the TAIL (end of output has the result)
            if combined.len() > MAX_OUTPUT_BYTES {
                let start = combined.len() - MAX_OUTPUT_BYTES;
                // Advance to next UTF-8 boundary
                let start = ceil_char_boundary(&combined, start);
                combined = format!(
                    "... [output truncated, showing last {}KB]\n{}",
                    MAX_OUTPUT_BYTES / 1024,
                    &combined[start..]
                );
            }
            ToolResult {
                success: output.status.success(),
                output: if combined.is_empty() {
                    format!(
                        "Command completed (exit code: {})",
                        output.status.code().unwrap_or(-1)
                    )
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
    log::info!(
        "[AgentTools] SSH exec on {}: {}",
        server_id,
        &command[..floor_char_boundary(command, 200)]
    );

    // Auto-connect if not in pool
    if let Err(e) = crate::commands::ssh_commands::auto_connect_ssh(ssh_pool, server_id).await {
        return ToolResult {
            success: false,
            output: format!("SSH auto-connect failed: {}", e),
        };
    }

    // Clone the client out of the lock guard before executing.
    // This releases the SSH pool mutex immediately so other operations
    // (e.g. build_system_prompt reading the pool) are not blocked
    // for the entire duration of the remote command (up to EXEC_TIMEOUT_SECS).
    let client = {
        let connections = ssh_pool.lock().await;
        match connections.get(server_id) {
            Some(c) => c.clone(),
            None => {
                return ToolResult {
                    success: false,
                    output: format!(
                        "SSH server '{}' not connected after auto-connect attempt.",
                        server_id
                    ),
                }
            }
        }
    }; // lock released here

    // Timeout to prevent hanging forever on remote commands
    let timeout_secs = get_exec_timeout(command);
    match timeout(Duration::from_secs(timeout_secs), client.execute(command)).await {
        Ok(Ok(result)) => {
            let mut output = result.stdout;
            if !result.stderr.is_empty() {
                if !output.is_empty() {
                    output.push_str("\n--- stderr ---\n");
                }
                output.push_str(&result.stderr);
            }
            // Truncate if too long — keep the TAIL (end of output has the result)
            if output.len() > MAX_OUTPUT_BYTES {
                let start = output.len() - MAX_OUTPUT_BYTES;
                let start = ceil_char_boundary(&output, start);
                output = format!(
                    "... [output truncated, showing last {}KB]\n{}",
                    MAX_OUTPUT_BYTES / 1024,
                    &output[start..]
                );
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

    // Local machine: no password is stored, AND we deliberately do not collect one via chat
    // by default. Typing a sudo password into a chat panel is bad UX, so the model should
    // hand off — print the install commands + homepage/docs URL and let the user run them
    // in their own terminal. The user MAY still volunteer their password in chat if they
    // prefer that flow; in that case the model uses it directly via `echo '<pwd>' | sudo -S`.
    // success=false so the model treats this branch as "stop polling, switch strategy".
    if server_id == "local" || server_id.is_empty() {
        return ToolResult {
            success: false,
            output: "NO_LOCAL_SUDO_PASSWORD_STORED. EchoBird does not collect local sudo passwords. \
                Do NOT call this tool again for local. Default path: \
                (1) prefer non-sudo alternatives first — nvm for Node, `pip install --user` for Python, \
                `cargo install` for Rust binaries; \
                (2) if sudo is truly unavoidable, HAND OFF to the user: in your <chat> reply give the \
                exact copy-paste commands from the tool's Embedded Install Reference (plus its homepage/docs \
                URL), and in one short sentence explain this step needs their sudo password and is faster \
                to run locally. Mention briefly that they MAY paste the password in chat if they prefer, \
                but do not press for it. \
                If the user has already volunteered their password (now or earlier in this conversation), \
                you may run `echo '<password>' | sudo -S <command>`. Mask the password as '***' in any \
                command or text echoed back to the UI, and do NOT include it in your final summary.".into(),
        };
    }

    let servers = read_servers_from_disk();
    match servers.iter().find(|s| s.id == server_id) {
        Some(server) => {
            let plain = model_manager::decrypt_key_for_use(&server.password);
            if plain.is_empty() {
                ToolResult { success: false, output: "No password saved for this server. Ask the user in <chat> for their sudo password, then run: echo '<password>' | sudo -S <command>".into() }
            } else {
                ToolResult {
                    success: true,
                    output: plain,
                }
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
                    let end = floor_char_boundary(&content, MAX_OUTPUT_BYTES);
                    content.truncate(end);
                    content.push_str("\n... [file truncated]");
                }
                ToolResult {
                    success: true,
                    output: content,
                }
            }
            Err(e) => ToolResult {
                success: false,
                output: format!("Failed to read file: {}", e),
            },
        }
    } else {
        // Read via SSH
        exec_ssh_shell(&format!("cat {}", shell_escape(path)), server_id, ssh_pool).await
    }
}

async fn exec_file_write(
    path: &str,
    content: &str,
    server_id: &str,
    ssh_pool: &SSHPool,
) -> ToolResult {
    if server_id == "local" || server_id.is_empty() {
        // Ensure parent directory exists
        if let Some(parent) = std::path::Path::new(path).parent() {
            if let Err(e) = tokio::fs::create_dir_all(parent).await {
                return ToolResult {
                    success: false,
                    output: format!("Failed to create directory: {}", e),
                };
            }
        }
        match tokio::fs::write(path, content).await {
            Ok(_) => ToolResult {
                success: true,
                output: format!("Written {} bytes to {}", content.len(), path),
            },
            Err(e) => ToolResult {
                success: false,
                output: format!("Failed to write file: {}", e),
            },
        }
    } else {
        // Write via SSH (using heredoc), chunked for large files
        let escaped_content = content.replace('\\', "\\\\").replace('$', "\\$");
        const CHUNK_THRESHOLD: usize = 16_000; // ~16KB

        if escaped_content.len() <= CHUNK_THRESHOLD {
            // Small file: single heredoc
            let cmd = format!(
                "mkdir -p \"$(dirname {})\" && cat > {} << 'ECHOBIRD_EOF'\n{}\nECHOBIRD_EOF",
                shell_escape(path),
                shell_escape(path),
                escaped_content
            );
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
                let cmd = format!(
                    "cat {} {} << 'ECHOBIRD_EOF'\n{}\nECHOBIRD_EOF",
                    redirect,
                    shell_escape(path),
                    chunk
                );
                let result = exec_ssh_shell(&cmd, server_id, ssh_pool).await;
                if !result.success {
                    return ToolResult {
                        success: false,
                        output: format!(
                            "Failed writing chunk {}/{}: {}",
                            i + 1,
                            chunks.len(),
                            result.output
                        ),
                    };
                }
            }

            ToolResult {
                success: true,
                output: format!(
                    "Written {} bytes to {} ({} chunks)",
                    content.len(),
                    path,
                    chunks.len()
                ),
            }
        }
    }
}

fn shell_escape(s: &str) -> String {
    // Simple quoting: not shell_escape for now, just wrap in quotes
    format!("\"{}\"", s)
}

/// Single-quote-escape for embedding into a shell command.
/// e.g. `it's` → `'it'\''s'` so it survives `sh -c 'grep PATTERN ...'`.
fn sq(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

// ── file_edit: string-replace edit (one occurrence only) ──

async fn exec_file_edit(
    path: &str,
    old_string: &str,
    new_string: &str,
    server_id: &str,
    ssh_pool: &SSHPool,
) -> ToolResult {
    // 1. Read current content
    let read = exec_file_read(path, server_id, ssh_pool).await;
    if !read.success {
        return ToolResult {
            success: false,
            output: format!("Cannot read file '{}': {}", path, read.output),
        };
    }
    let content = read.output;

    // 2. Refuse if read was truncated — replacing in a partial view is unsafe
    if content.contains("\n... [file truncated]")
        || content.contains("[output truncated, showing last")
    {
        return ToolResult {
            success: false,
            output: format!(
                "File '{}' is too large to edit safely (read returned a truncated view). \
                 Use shell_exec with sed/awk for large-file edits, or split the change into smaller pieces.",
                path
            ),
        };
    }

    // 3. Find occurrences
    let occurrences = content.matches(old_string).count();
    if occurrences == 0 {
        return ToolResult {
            success: false,
            output: format!(
                "old_string not found in '{}'. The file content may have changed; re-read the file and try again with the exact current substring.",
                path
            ),
        };
    }
    if occurrences > 1 {
        return ToolResult {
            success: false,
            output: format!(
                "old_string is ambiguous — found {} matches in '{}'. Add more surrounding context to old_string so it identifies exactly one location.",
                occurrences, path
            ),
        };
    }

    // 4. Replace and write back
    let updated = content.replacen(old_string, new_string, 1);
    let write = exec_file_write(path, &updated, server_id, ssh_pool).await;
    if !write.success {
        return ToolResult {
            success: false,
            output: format!("Edit-write failed: {}", write.output),
        };
    }

    let old_lines = old_string.lines().count().max(1);
    let new_lines = new_string.lines().count().max(1);
    ToolResult {
        success: true,
        output: format!(
            "Edited '{}' — replaced {} line(s) with {} line(s).",
            path, old_lines, new_lines
        ),
    }
}

// ── grep: search file tree for a pattern ──

async fn exec_grep(
    pattern: &str,
    path: Option<&str>,
    case_insensitive: bool,
    server_id: &str,
    ssh_pool: &SSHPool,
) -> ToolResult {
    let path_str = path.unwrap_or(".");

    if server_id == "local" || server_id.is_empty() {
        // Local: route by OS
        #[cfg(target_os = "windows")]
        {
            // PowerShell Select-String. -Pattern accepts regex.
            let ci_flag = if case_insensitive {
                "-CaseSensitive:$false"
            } else {
                "-CaseSensitive"
            };
            let cmd = format!(
                "Get-ChildItem -Recurse -File -Path {p} -ErrorAction SilentlyContinue | \
                 Select-String -Pattern {pat} {ci} -ErrorAction SilentlyContinue | \
                 Select-Object -First 200 | \
                 ForEach-Object {{ \"$($_.Path):$($_.LineNumber):$($_.Line)\" }}",
                p = sq(path_str),
                pat = sq(pattern),
                ci = ci_flag,
            );
            return exec_local_shell(&cmd).await;
        }
        #[cfg(not(target_os = "windows"))]
        {
            let ci = if case_insensitive { "-i" } else { "" };
            let cmd = format!(
                "grep -rEn {ci} -- {pat} {p} 2>/dev/null | head -n 200",
                ci = ci,
                pat = sq(pattern),
                p = sq(path_str),
            );
            return exec_local_shell(&cmd).await;
        }
    }

    // SSH: assume POSIX grep on the remote
    let ci = if case_insensitive { "-i" } else { "" };
    let cmd = format!(
        "grep -rEn {ci} -- {pat} {p} 2>/dev/null | head -n 200",
        ci = ci,
        pat = sq(pattern),
        p = sq(path_str),
    );
    exec_ssh_shell(&cmd, server_id, ssh_pool).await
}

// ── glob: list files matching a pattern ──

async fn exec_glob(
    pattern: &str,
    path: Option<&str>,
    server_id: &str,
    ssh_pool: &SSHPool,
) -> ToolResult {
    let path_str = path.unwrap_or(".");

    if server_id == "local" || server_id.is_empty() {
        #[cfg(target_os = "windows")]
        {
            let cmd = format!(
                "Get-ChildItem -Recurse -File -Path {p} -Filter {pat} -ErrorAction SilentlyContinue | \
                 Select-Object -First 200 -ExpandProperty FullName",
                p = sq(path_str),
                pat = sq(pattern),
            );
            return exec_local_shell(&cmd).await;
        }
        #[cfg(not(target_os = "windows"))]
        {
            let cmd = format!(
                "find {p} -type f -name {pat} 2>/dev/null | head -n 200",
                p = sq(path_str),
                pat = sq(pattern),
            );
            return exec_local_shell(&cmd).await;
        }
    }

    let cmd = format!(
        "find {p} -type f -name {pat} 2>/dev/null | head -n 200",
        p = sq(path_str),
        pat = sq(pattern),
    );
    exec_ssh_shell(&cmd, server_id, ssh_pool).await
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

async fn exec_deploy_plugin_source(
    server_id: &str,
    plugin_id: &str,
    port: u16,
    ssh_pool: &SSHPool,
) -> ToolResult {
    log::info!(
        "[AgentTools] Deploying plugin '{}' to server '{}' via GitHub Release download",
        plugin_id,
        server_id
    );

    // 1. Detect remote OS + architecture
    let os_result =
        exec_ssh_shell("uname -s 2>/dev/null || echo windows", server_id, ssh_pool).await;
    let arch_result =
        exec_ssh_shell("uname -m 2>/dev/null || echo x86_64", server_id, ssh_pool).await;
    let os_name = os_result.output.trim().to_lowercase();
    let arch = arch_result.output.trim().to_lowercase();

    // Map plugin_id to binary name pattern
    let (_binary_name, binary_filename) = if os_name.contains("linux") {
        if arch.contains("aarch64") || arch.contains("arm64") {
            (
                format!("{}-linux-aarch64", plugin_id),
                format!("{}-linux-aarch64", plugin_id),
            )
        } else {
            (
                format!("{}-linux-x86_64", plugin_id),
                format!("{}-linux-x86_64", plugin_id),
            )
        }
    } else if os_name.contains("darwin") {
        if arch.contains("arm64") || arch.contains("aarch64") {
            (
                format!("{}-darwin-aarch64", plugin_id),
                format!("{}-darwin-aarch64", plugin_id),
            )
        } else {
            (
                format!("{}-darwin-x86_64", plugin_id),
                format!("{}-darwin-x86_64", plugin_id),
            )
        }
    } else {
        (
            format!("{}-win.exe", plugin_id),
            format!("{}-win.exe", plugin_id),
        )
    };

    log::info!(
        "[AgentTools] Remote: os={}, arch={}, binary={}",
        os_name,
        arch,
        binary_filename
    );

    let mut log_output = String::new();

    // 2. Fetch latest version dynamically from version API (falls back to compile-time version)
    let version = fetch_latest_plugin_version().await;
    // Primary: Cloudflare proxy (GFW-friendly) — bare binary, no zip
    let primary_url = format!(
        "https://dl.echobird.ai/releases/{}/{}",
        version, binary_filename
    );
    // Fallback 1: GitHub versioned
    let github_url = format!(
        "https://github.com/edison7009/Echobird-MotherAgent/releases/download/{}/{}",
        version, binary_filename
    );
    // Fallback 2: GitHub latest
    let github_latest_url = format!(
        "https://github.com/edison7009/Echobird-MotherAgent/releases/latest/download/{}",
        binary_filename
    );

    log_output.push_str(&format!("[1/4] Downloading {} ...\n", binary_filename));

    // 3. Download bare binary directly and chmod +x
    let deploy_dir = "~/echobird";
    let download_cmd = |url: &str| {
        format!(
            "mkdir -p {dir} && rm -f {dir}/{bin} && \
         curl -fSL --connect-timeout 15 --max-time 90 -o {dir}/{bin} '{url}' && \
         chmod +x {dir}/{bin}",
            dir = deploy_dir,
            bin = binary_filename,
            url = url
        )
    };

    let result = exec_ssh_shell(&download_cmd(&primary_url), server_id, ssh_pool).await;
    if !result.success {
        log_output.push_str("  Cloudflare mirror failed, trying GitHub versioned...\n");
        let result2 = exec_ssh_shell(&download_cmd(&github_url), server_id, ssh_pool).await;
        if !result2.success {
            log_output.push_str("  GitHub versioned failed, trying GitHub latest...\n");
            let result3 =
                exec_ssh_shell(&download_cmd(&github_latest_url), server_id, ssh_pool).await;
            if !result3.success {
                return ToolResult {
                    success: false,
                    output: format!(
                        "Failed to download '{}'. Tried:\n1. {}\n2. {}\n3. {}\nError: {}",
                        binary_filename, primary_url, github_url, github_latest_url, result3.output
                    ),
                };
            }
        }
    }
    log_output.push_str("[2/4] Binary downloaded and ready\n");

    // 4. Stop any existing instance
    log_output.push_str("[3/4] Starting server...\n");
    let _ = exec_ssh_shell(
        &format!(
            "pkill -f '{}/{}' 2>/dev/null; sleep 1",
            deploy_dir, binary_filename
        ),
        server_id,
        ssh_pool,
    )
    .await;

    // 5. Start the server
    let start_result = exec_ssh_shell(
        &format!(
            "nohup {}/{} {} > /tmp/{}.log 2>&1 & sleep 2 && pgrep -f '{}' && echo 'STARTED_OK'",
            deploy_dir, binary_filename, port, plugin_id, binary_filename
        ),
        server_id,
        ssh_pool,
    )
    .await;

    if start_result.output.contains("STARTED_OK") {
        log_output.push_str(&format!("[4/4] Server started on port {}\n", port));

        // Quick API health check
        let health = exec_ssh_shell(
            &format!(
                "curl -s http://localhost:{}/api/status 2>&1 || echo 'API_NOT_READY'",
                port
            ),
            server_id,
            ssh_pool,
        )
        .await;
        if !health.output.contains("API_NOT_READY") {
            log_output.push_str(&format!("  API health check: {}\n", health.output.trim()));
        }

        ToolResult {
            success: true,
            output: format!(
                "{}Plugin '{}' deployed and running on port {}.",
                log_output, plugin_id, port
            ),
        }
    } else {
        let log_check = exec_ssh_shell(
            &format!("cat /tmp/{}.log 2>/dev/null | tail -10", plugin_id),
            server_id,
            ssh_pool,
        )
        .await;
        ToolResult {
            success: false,
            output: format!(
                "{}Server failed to start. Logs:\n{}",
                log_output, log_check.output
            ),
        }
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

// ── Web Fetch ──

const WEB_FETCH_MAX_CHARS: usize = 8000;
const WEB_FETCH_TIMEOUT_SECS: u64 = 30;
// Real-browser UA: many CDNs (Cloudflare, Akamai) and major sites (GitHub, npm)
// 4xx-block requests with non-browser UAs, which is why the previous "Echobird-MotherAgent/1.0"
// produced a string of unexplained "can't open the page" errors.
const WEB_FETCH_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36";

async fn exec_web_fetch(url: &str) -> ToolResult {
    // Accept http:// and https:// — internal docs and some CN mirrors are HTTP-only.
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return ToolResult {
            success: false,
            output: "URL must start with http:// or https://".into(),
        };
    }

    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(WEB_FETCH_TIMEOUT_SECS))
        .redirect(reqwest::redirect::Policy::limited(10))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            return ToolResult {
                success: false,
                output: format!("Failed to create HTTP client: {}", e),
            }
        }
    };

    let response = match client
        .get(url)
        .header("User-Agent", WEB_FETCH_USER_AGENT)
        .header(
            "Accept",
            "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
        )
        .header("Accept-Language", "en-US,en;q=0.9,zh-CN;q=0.8,zh;q=0.7")
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            // Distinguish error class so the model can decide what to do
            let kind = if e.is_timeout() {
                "timeout"
            } else if e.is_connect() {
                "connect_failed (DNS/TLS/network)"
            } else if e.is_redirect() {
                "too_many_redirects"
            } else {
                "request_error"
            };
            return ToolResult {
                success: false,
                output: format!("web_fetch failed [{}]: {}", kind, e),
            };
        }
    };

    let final_url = response.url().to_string();
    let status = response.status();
    if !status.is_success() {
        return ToolResult {
            success: false,
            output: format!(
                "HTTP {} {} (final URL: {})",
                status.as_u16(),
                status.canonical_reason().unwrap_or("Error"),
                final_url,
            ),
        };
    }

    let body = match response.text().await {
        Ok(t) => t,
        Err(e) => {
            return ToolResult {
                success: false,
                output: format!("Failed to read response: {}", e),
            }
        }
    };

    // Strip HTML tags (rough but effective for most pages)
    let text = strip_html_tags(&body);

    // Collapse whitespace
    let text: String = text.split_whitespace().collect::<Vec<&str>>().join(" ");

    // Truncate to max chars
    let truncated = if text.len() > WEB_FETCH_MAX_CHARS {
        format!(
            "{}...\n[Truncated: {} chars total]",
            &text[..WEB_FETCH_MAX_CHARS],
            text.len()
        )
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
            let window: String = lower_chars[i..i + 7].iter().collect();
            if window == "<script" {
                in_script = true;
            }
        }
        if in_script && i + 8 < lower_chars.len() {
            let window: String = lower_chars[i..i + 9].iter().collect();
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

// ── Upload File (local → remote via SSH base64) ──

async fn exec_upload_file(
    local_path: &str,
    remote_path: &str,
    server_id: &str,
    ssh_pool: &SSHPool,
) -> ToolResult {
    log::info!(
        "[AgentTools] Uploading file {} → {}:{}",
        local_path,
        server_id,
        remote_path
    );

    // Read local file
    let file_data = match std::fs::read(local_path) {
        Ok(data) => data,
        Err(e) => {
            return ToolResult {
                success: false,
                output: format!("Failed to read local file '{}': {}", local_path, e),
            }
        }
    };

    let file_size = file_data.len();
    log::info!("[AgentTools] File size: {} bytes", file_size);

    // Base64 encode
    let encoded = {
        use base64::Engine;
        base64::engine::general_purpose::STANDARD.encode(&file_data)
    };

    // Ensure remote directory exists
    let remote_dir = std::path::Path::new(remote_path)
        .parent()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| ".".to_string());
    let mkdir_result =
        exec_ssh_shell(&format!("mkdir -p {}", remote_dir), server_id, ssh_pool).await;
    if !mkdir_result.success {
        return ToolResult {
            success: false,
            output: format!(
                "Failed to create remote directory '{}': {}",
                remote_dir, mkdir_result.output
            ),
        };
    }

    // Transfer via base64 in chunks (SSH channel has size limits)
    let chunk_size = 65536; // 64KB base64 chunks
    let chunks: Vec<&str> = encoded
        .as_bytes()
        .chunks(chunk_size)
        .map(|c| std::str::from_utf8(c).unwrap_or(""))
        .collect();

    if chunks.len() == 1 {
        // Small file — single command
        let cmd = format!("echo '{}' | base64 -d > {}", encoded, remote_path);
        let result = exec_ssh_shell(&cmd, server_id, ssh_pool).await;
        if !result.success {
            return ToolResult {
                success: false,
                output: format!("Upload failed: {}", result.output),
            };
        }
    } else {
        // Large file — accumulate base64 text in chunks, then decode once
        let first_cmd = format!("printf '%s' '{}' > {}.b64", chunks[0], remote_path);
        let first_result = exec_ssh_shell(&first_cmd, server_id, ssh_pool).await;
        if !first_result.success {
            return ToolResult {
                success: false,
                output: format!("Upload chunk 0 failed: {}", first_result.output),
            };
        }

        for (i, chunk) in chunks[1..].iter().enumerate() {
            let cmd = format!("printf '%s' '{}' >> {}.b64", chunk, remote_path);
            let chunk_result = exec_ssh_shell(&cmd, server_id, ssh_pool).await;
            if !chunk_result.success {
                return ToolResult {
                    success: false,
                    output: format!("Upload chunk {} failed: {}", i + 1, chunk_result.output),
                };
            }
        }

        // Decode the concatenated base64 in one shot
        let decode_cmd = format!(
            "base64 -d {}.b64 > {} && rm {}.b64",
            remote_path, remote_path, remote_path
        );
        let decode_result = exec_ssh_shell(&decode_cmd, server_id, ssh_pool).await;
        if !decode_result.success {
            return ToolResult {
                success: false,
                output: format!("Upload decode failed: {}", decode_result.output),
            };
        }
    }

    // Make executable (useful for binaries)
    let _ = exec_ssh_shell(&format!("chmod +x {}", remote_path), server_id, ssh_pool).await;

    // Verify
    let verify = exec_ssh_shell(&format!("ls -la {}", remote_path), server_id, ssh_pool).await;

    ToolResult {
        success: true,
        output: format!(
            "Uploaded {} bytes ({} chunks) to {}:{}\n{}",
            file_size,
            chunks.len(),
            server_id,
            remote_path,
            verify.output
        ),
    }
}

// ── Download File (remote → local via SSH base64) ──

async fn exec_download_file(
    remote_path: &str,
    local_path: &str,
    server_id: &str,
    ssh_pool: &SSHPool,
) -> ToolResult {
    log::info!(
        "[AgentTools] Downloading file {}:{} → {}",
        server_id,
        remote_path,
        local_path
    );

    // Check remote file exists and get size
    let check = exec_ssh_shell(
        &format!(
            "test -f {} && stat -c %s {} 2>/dev/null || stat -f %z {} 2>/dev/null",
            remote_path, remote_path, remote_path
        ),
        server_id,
        ssh_pool,
    )
    .await;
    if !check.success {
        return ToolResult {
            success: false,
            output: format!("Remote file '{}' not found or not accessible", remote_path),
        };
    }

    // Read remote file as base64
    let b64_result = exec_ssh_shell(
        &format!(
            "base64 {} 2>/dev/null || openssl base64 -in {} 2>/dev/null",
            remote_path, remote_path
        ),
        server_id,
        ssh_pool,
    )
    .await;
    if !b64_result.success || b64_result.output.trim().is_empty() {
        return ToolResult {
            success: false,
            output: format!(
                "Failed to read remote file as base64: {}",
                b64_result.output
            ),
        };
    }

    // Decode base64
    let clean_b64: String = b64_result
        .output
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    let decoded = {
        use base64::Engine;
        match base64::engine::general_purpose::STANDARD.decode(&clean_b64) {
            Ok(data) => data,
            Err(e) => {
                return ToolResult {
                    success: false,
                    output: format!("Base64 decode failed: {}", e),
                }
            }
        }
    };

    let file_size = decoded.len();

    // Ensure local directory exists
    if let Some(parent) = std::path::Path::new(local_path).parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            return ToolResult {
                success: false,
                output: format!(
                    "Failed to create local directory '{}': {}",
                    parent.display(),
                    e
                ),
            };
        }
    }

    // Write to local file
    match std::fs::write(local_path, &decoded) {
        Ok(_) => {
            log::info!(
                "[AgentTools] Downloaded {} bytes to {}",
                file_size,
                local_path
            );
            ToolResult {
                success: true,
                output: format!(
                    "Downloaded {} bytes from {}:{} → {}",
                    file_size, server_id, remote_path, local_path
                ),
            }
        }
        Err(e) => ToolResult {
            success: false,
            output: format!("Failed to write local file '{}': {}", local_path, e),
        },
    }
}
