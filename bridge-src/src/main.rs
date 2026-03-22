// Echobird Bridge — remote Agent communication via stdin/stdout JSON
//
// Runs on the remote machine, receives JSON commands from Echobird via SSH,
// invokes the Agent CLI (e.g. openclaw agent), and streams responses back as JSON lines.
//
// Protocol:
//   stdin  → {"type":"chat","message":"...","session_id":"..."}
//   stdout ← {"type":"text","text":"...","session_id":"..."}
//   stdout ← {"type":"done","session_id":"..."}

use serde::{Deserialize, Serialize};
use std::io::{self, BufRead, Write};
use std::path::PathBuf;
use std::process::Command;
use std::sync::Mutex;

// ── Global state: current active role per agent ──
static ACTIVE_ROLE: Mutex<Option<(String, String)>> = Mutex::new(None); // (agent_id, role_id)

// ── Types ──

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum InboundMessage {
    #[serde(rename = "chat")]
    Chat {
        message: String,
        session_id: Option<String>,
        model: Option<String>,
        system_prompt: Option<String>,
        agent_name: Option<String>,
    },
    #[serde(rename = "resume")]
    Resume {
        message: String,
        session_id: String,
        model: Option<String>,
        agent_name: Option<String>,
    },
    #[serde(rename = "status")]
    Status {},
    #[serde(rename = "abort")]
    Abort {
        #[allow(dead_code)]
        session_id: Option<String>,
    },
    #[serde(rename = "ping")]
    Ping {},
    #[serde(rename = "detect_agents")]
    DetectAgents {},
    #[serde(rename = "set_role")]
    SetRole {
        agent_id: String,
        role_id: String,
        url: String,
    },
    #[serde(rename = "start_agent")]
    StartAgent {
        agent_id: String,
    },
    #[serde(rename = "clear_role")]
    ClearRole {
        agent_id: String,
        role_id: String,
    },
    #[serde(rename = "set_model")]
    SetModel {
        agent_id: String,
        model_id: String,
        model_name: String,
        api_key: String,
        base_url: String,
        api_type: String,
    },
}

#[derive(Debug, Serialize)]
#[serde(tag = "type")]
enum OutboundMessage {
    #[serde(rename = "text")]
    Text {
        text: String,
        session_id: Option<String>,
    },
    #[serde(rename = "done")]
    Done {
        session_id: Option<String>,
    },
    #[serde(rename = "error")]
    Error {
        message: String,
    },
    #[serde(rename = "status")]
    Status {
        agent: String,
        version: String,
        ready: bool,
    },
    #[serde(rename = "pong")]
    Pong {},
    #[serde(rename = "agents_detected")]
    AgentsDetected {
        agents: Vec<AgentInfo>,
    },
    #[serde(rename = "role_set")]
    RoleSet {
        agent_id: String,
        role_id: String,
        installed: bool,
        path: String,
    },
    #[serde(rename = "agent_started")]
    AgentStarted {
        agent_id: String,
        success: bool,
        message: String,
    },
    #[serde(rename = "role_cleared")]
    RoleCleared {
        agent_id: String,
        role_id: String,
        success: bool,
    },
    #[serde(rename = "model_set")]
    ModelSet {
        agent_id: String,
        model_id: String,
        success: bool,
    },
}

#[derive(Debug, Clone, Serialize)]
struct AgentInfo {
    id: String,
    name: String,
    installed: bool,
    running: bool,
    path: Option<String>,
}

// ── Config ──

struct BridgeConfig {
    command: String,
    args: Vec<String>,
    resume_args: Vec<String>,
    session_arg: Option<String>,
    model_arg: Option<String>,
    system_prompt_arg: Option<String>,
    agent_arg: Option<String>,
}

impl Default for BridgeConfig {
    fn default() -> Self {
        Self {
            command: "openclaw".to_string(),
            args: vec![
                "agent".to_string(),
                "--json".to_string(),
                "--agent".to_string(),
                "main".to_string(),
                "--message".to_string(),
            ],
            resume_args: vec![
                "agent".to_string(),
                "--json".to_string(),
                "--agent".to_string(),
                "main".to_string(),
                "--session-id".to_string(),
                "{sessionId}".to_string(),
                "--message".to_string(),
            ],
            session_arg: Some("--session-id".to_string()),
            model_arg: None,
            system_prompt_arg: None,
            agent_arg: None,
        }
    }
}

// ── Main Loop ──

fn main() {
    let config = load_config();

    // Send ready status
    send(&OutboundMessage::Status {
        agent: config.command.split('/').last().unwrap_or(&config.command).to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        ready: true,
    });

    let stdin = io::stdin();
    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break, // SSH disconnected
        };

        if line.trim().is_empty() {
            continue;
        }

        match serde_json::from_str::<InboundMessage>(&line) {
            Ok(msg) => handle_message(&config, msg),
            Err(e) => {
                send(&OutboundMessage::Error {
                    message: format!("Invalid JSON: {}", e),
                });
            }
        }
    }
}

fn handle_message(config: &BridgeConfig, msg: InboundMessage) {
    match msg {
        InboundMessage::Chat {
            message,
            session_id,
            model,
            system_prompt,
            agent_name,
        } => {
            execute_chat(config, &message, session_id.as_deref(), model.as_deref(), system_prompt.as_deref(), agent_name.as_deref(), false);
        }
        InboundMessage::Resume {
            message,
            session_id,
            model,
            agent_name,
        } => {
            execute_chat(config, &message, Some(&session_id), model.as_deref(), None, agent_name.as_deref(), true);
        }
        InboundMessage::Status {} => {
            let version = detect_agent(&config.command);
            send(&OutboundMessage::Status {
                agent: config.command.split('/').last().unwrap_or(&config.command).to_string(),
                version,
                ready: true,
            });
        }
        InboundMessage::Abort { .. } => {
            send(&OutboundMessage::Error {
                message: "Abort received (current process will complete)".to_string(),
            });
        }
        InboundMessage::Ping {} => {
            send(&OutboundMessage::Pong {});
        }
        InboundMessage::DetectAgents {} => {
            handle_detect_agents();
        }
        InboundMessage::SetRole { agent_id, role_id, url } => {
            handle_set_role(&agent_id, &role_id, &url);
        }
        InboundMessage::StartAgent { agent_id } => {
            handle_start_agent(&agent_id);
        }
        InboundMessage::ClearRole { agent_id, role_id } => {
            handle_clear_role(&agent_id, &role_id);
        }
        InboundMessage::SetModel { agent_id, model_id, model_name, api_key, base_url, api_type } => {
            handle_set_model(&agent_id, &model_id, &model_name, &api_key, &base_url, &api_type);
        }
    }
}

fn execute_chat(
    config: &BridgeConfig,
    message: &str,
    session_id: Option<&str>,
    model: Option<&str>,
    _system_prompt: Option<&str>,
    agent_name: Option<&str>,
    is_resume: bool,
) {
    // Determine effective agent_name: explicit parameter > stored from set_role
    let stored_role = ACTIVE_ROLE.lock().ok().and_then(|g| g.clone());
    let effective_agent = agent_name
        .map(String::from)
        .or_else(|| stored_role.clone().map(|(_, role_id)| role_id));
    // Build command args
    let mut args: Vec<String> = if is_resume {
        // Resume: use resume_args, replace {sessionId}
        let sid = session_id.unwrap_or("unknown");
        config.resume_args.iter()
            .map(|a| {
                let a = a.replace("{sessionId}", sid);
                if a == "main" { "main".to_string() } else { a }
            })
            .collect()
    } else {
        let mut a: Vec<String> = config.args.iter().map(|arg| arg.clone()).collect();
        // Insert session ID BEFORE --message (which must be last, followed by the actual message text)
        if let (Some(sid), Some(session_arg)) = (session_id, &config.session_arg) {
            // --message is the last element in args; insert --session-id before it
            let insert_pos = if a.last().map(|s| s.as_str()) == Some("--message") {
                a.len() - 1
            } else {
                a.len()
            };
            a.insert(insert_pos, sid.to_string());
            a.insert(insert_pos, session_arg.clone());
        }
        a
    };

    // Add model if specified (openclaw uses provider/model format)
    if let (Some(m), Some(model_arg)) = (model, &config.model_arg) {
        args.push(model_arg.clone());
        args.push(m.to_string());
    }

    // Add agent name if specified (claude code uses --agent)
    // Claude Code reads ~/.claude/agents/{name}.md automatically (upstream agency-agents pattern)
    if let (Some(ref name), Some(agent_arg)) = (&effective_agent, &config.agent_arg) {
        args.push(agent_arg.clone());
        args.push(name.to_string());
    }

    // Add message as last arg (--message is already in args list).
    // On Windows, cmd.exe treats a real newline as a command separator and truncates
    // the argument at the first newline. Replace real newlines with the two-character
    // literal \n so the full message is passed intact. The agent (openclaw) interprets \n.
    #[cfg(target_os = "windows")]
    let message_arg = message.replace("\r\n", "\\n").replace('\n', "\\n");
    #[cfg(not(target_os = "windows"))]
    let message_arg = message.to_string();
    args.push(message_arg);

    // Execute the CLI
    eprintln!("[bridge] Executing: {} {}", config.command, args.join(" "));

    // On Windows, .cmd scripts must be run through cmd.exe.
    // Resolve the full .cmd path first, then pass each arg separately so that
    // Rust's Windows argv encoding (CreateProcess) correctly quotes args that
    // contain newlines or special characters — avoiding cmd.exe shell truncation.
    let result = if cfg!(target_os = "windows") {
        let resolved = resolve_command(&config.command);
        // Pass /c + full-path-to-.cmd as two separate args, then all message args.
        // Rust's Command on Windows calls CreateProcess and quotes each element
        // individually, so newlines in message are preserved correctly.
        let mut cmd_args = vec!["/c".to_string(), resolved];
        cmd_args.extend(args.iter().cloned());
        Command::new("cmd.exe").args(&cmd_args)
            .env("NO_COLOR", "1")  // Disable ANSI colors (https://no-color.org/)
            .output()
    } else {
        Command::new(&config.command).args(&args)
            .env("NO_COLOR", "1")  // Disable ANSI colors (https://no-color.org/)
            .output()
    };

    match result
    {
        Ok(output) => {
            let raw_stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let stdout = strip_ansi(&raw_stdout);
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();

            if !stderr.is_empty() {
                eprintln!("[bridge] stderr: {}", strip_ansi(&stderr));
            }

            // Parse agent JSON output (supports OpenClaw + Claude Code formats)
            let (text, new_session_id) = parse_agent_output(&stdout);

            send(&OutboundMessage::Text {
                text,
                session_id: new_session_id.clone().or_else(|| session_id.map(String::from)),
            });

            send(&OutboundMessage::Done {
                session_id: new_session_id.or_else(|| session_id.map(String::from)),
            });
        }
        Err(e) => {
            send(&OutboundMessage::Error {
                message: format!("Failed to execute {}: {}", config.command, e),
            });
        }
    }
}

/// Parse agent CLI JSON output
///
/// Supports multiple formats:
/// 1. OpenClaw wrapped: { "result": { "payloads": [...], "meta": { "agentMeta": { "sessionId": "..." } } } }
/// 2. OpenClaw direct:  { "payloads": [...], "meta": { "agentMeta": { "sessionId": "..." } } }
/// 3. Claude Code:      { "result": "text...", "session_id": "..." }
/// 4. Claude Code rich:  { "result": { "content": [{"type":"text","text":"..."}] }, "session_id": "..." }
fn parse_agent_output(stdout: &str) -> (String, Option<String>) {
    // Find JSON in stdout (skip non-JSON lines like [Echobird] injection logs)
    let json_str = find_json_object(stdout);
    let json_str = match json_str {
        Some(s) => s,
        None => return (stdout.to_string(), None),
    };

    match serde_json::from_str::<serde_json::Value>(&json_str) {
        Ok(json) => {
            // ── Try OpenClaw format first: result.payloads or top-level payloads ──
            let payloads = json.get("result")
                .and_then(|r| r.get("payloads"))
                .or_else(|| json.get("payloads"));

            if let Some(payloads) = payloads {
                let text = payloads
                    .as_array()
                    .and_then(|arr| {
                        let texts: Vec<&str> = arr.iter()
                            .filter_map(|p| p.get("text").and_then(|t| t.as_str()))
                            .collect();
                        if texts.is_empty() { None } else { Some(texts.join("\n")) }
                    })
                    .unwrap_or_else(|| stdout.to_string());

                // Extract OpenClaw session ID
                let session_id = json.get("result")
                    .and_then(|r| r.get("meta"))
                    .or_else(|| json.get("meta"))
                    .and_then(|m| m.get("agentMeta"))
                    .and_then(|am| am.get("sessionId"))
                    .and_then(|v| v.as_str())
                    .map(String::from);

                // Check for error status
                let status = json.get("status").and_then(|s| s.as_str()).unwrap_or("ok");
                if status != "ok" {
                    let error_msg = json.get("error")
                        .and_then(|e| e.as_str())
                        .unwrap_or("Agent returned error status");
                    return (format!("Error: {}", error_msg), session_id);
                }

                return (text, session_id);
            }

            // ── Try Claude Code format: result as string or result.content[] ──
            let session_id = json.get("session_id")
                .and_then(|v| v.as_str())
                .map(String::from);

            let text = if let Some(result) = json.get("result") {
                if let Some(s) = result.as_str() {
                    // Simple string result
                    s.to_string()
                } else if let Some(content) = result.get("content").and_then(|c| c.as_array()) {
                    // Rich content: [{"type":"text","text":"..."}]
                    let texts: Vec<&str> = content.iter()
                        .filter_map(|c| c.get("text").and_then(|t| t.as_str()))
                        .collect();
                    if texts.is_empty() {
                        result.to_string()
                    } else {
                        texts.join("\n")
                    }
                } else {
                    // Fallback: try result.text
                    result.get("text")
                        .and_then(|v| v.as_str())
                        .unwrap_or(stdout)
                        .to_string()
                }
            } else {
                // No result field — raw fallback
                json.get("text")
                    .and_then(|v| v.as_str())
                    .unwrap_or(stdout)
                    .to_string()
            };

            // Check for error
            if let Some(true) = json.get("is_error").and_then(|v| v.as_bool()) {
                return (format!("Error: {}", text), session_id);
            }

            (text, session_id)
        }
        Err(_) => {
            // Fallback: raw text
            (stdout.to_string(), None)
        }
    }
}

/// Find the first JSON object in the output (skip non-JSON prefix lines)
fn find_json_object(input: &str) -> Option<String> {
    // Find the first '{' that starts a JSON object
    if let Some(start) = input.find('{') {
        // Find the matching closing '}'
        let rest = &input[start..];
        let mut depth = 0;
        for (i, ch) in rest.char_indices() {
            match ch {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(rest[..=i].to_string());
                    }
                }
                _ => {}
            }
        }
    }
    None
}

// ── Agent Detection ──

/// Agents we know how to detect
const KNOWN_AGENTS: &[(&str, &str, &str)] = &[
    // (id, display_name, command_name)
    ("claudecode", "Claude Code", "claude"),
    ("openclaw",   "OpenClaw",    "openclaw"),
    ("zeroclaw",   "ZeroClaw",    "zeroclaw"),
    ("nanobot",    "NanoBot",     "nanobot"),
    ("picoclaw",   "PicoClaw",    "picoclaw"),
    ("hermes",     "Hermes Agent","hermes"),
];

fn handle_detect_agents() {
    let mut agents = Vec::new();

    for &(id, name, cmd) in KNOWN_AGENTS {
        let (installed, path) = check_installed(cmd);
        let running = if installed { check_running(cmd) } else { false };

        agents.push(AgentInfo {
            id: id.to_string(),
            name: name.to_string(),
            installed,
            running,
            path,
        });
    }

    eprintln!("[bridge] Detected agents: {:?}", agents.iter().map(|a| (&a.id, a.installed, a.running)).collect::<Vec<_>>());
    send(&OutboundMessage::AgentsDetected { agents });
}

/// Check if a command is installed — returns (installed, optional_path)
fn check_installed(cmd: &str) -> (bool, Option<String>) {
    let result = if cfg!(target_os = "windows") {
        Command::new("where.exe").arg(cmd).output()
    } else {
        Command::new("which").arg(cmd).output()
    };

    match result {
        Ok(output) if output.status.success() => {
            let path = String::from_utf8_lossy(&output.stdout)
                .lines().next().unwrap_or("").trim().to_string();
            if path.is_empty() { (false, None) } else { (true, Some(path)) }
        }
        _ => (false, None),
    }
}

/// Check if a process is currently running
fn check_running(cmd: &str) -> bool {
    let result = if cfg!(target_os = "windows") {
        // tasklist /FI "IMAGENAME eq claude.exe" /NH
        let exe_name = format!("{}.exe", cmd);
        Command::new("tasklist")
            .args(["/FI", &format!("IMAGENAME eq {}", exe_name), "/NH"])
            .output()
    } else {
        // pgrep -x <cmd>
        Command::new("pgrep").args(["-x", cmd]).output()
    };

    match result {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            if cfg!(target_os = "windows") {
                // tasklist returns "INFO: No tasks are running..." if not found
                !stdout.contains("INFO:") && stdout.contains(cmd)
            } else {
                // pgrep returns exit code 0 if found
                output.status.success()
            }
        }
        Err(_) => false,
    }
}

// ── Role Installation (URL-based download) ──

/// Extract `name` from YAML frontmatter (--- delimited block at top of .md file)
fn extract_yaml_name(content: &str) -> Option<String> {
    let trimmed = content.trim_start();
    if !trimmed.starts_with("---") { return None; }
    let after_first = &trimmed[3..];
    let end = after_first.find("---")?;
    let frontmatter = &after_first[..end];
    for line in frontmatter.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("name:") {
            let name = rest.trim().trim_matches('"').trim_matches('\'').trim();
            if !name.is_empty() {
                return Some(name.to_string());
            }
        }
    }
    None
}

fn handle_set_role(agent_id: &str, role_id: &str, url: &str) {
    // Determine target path based on agent
    let home = home_dir();
    let target = match agent_id {
        "claudecode" => home.join(".claude").join("agents").join(format!("{}.md", role_id)),
        "openclaw"   => home.join(".openclaw").join("workspace").join("SOUL.md"),
        "zeroclaw"   => home.join(".zeroclaw").join("workspace").join("skills").join(role_id).join("SKILL.md"),
        "nanobot"    => home.join(".nanobot").join("workspace").join("AGENTS.md"),
        "picoclaw"   => home.join(".picoclaw").join("workspace").join("AGENT.md"),
        "hermes"     => home.join(".hermes").join("SOUL.md"),
        _ => {
            send(&OutboundMessage::Error {
                message: format!("Unknown agent: {}", agent_id),
            });
            return;
        }
    };

    // Idempotent: skip if already installed (only for agents with per-role file paths)
    // OpenClaw always overwrites SOUL.md in main workspace, so never skip
    if agent_id != "openclaw" && agent_id != "nanobot" && agent_id != "picoclaw" && agent_id != "hermes" && target.exists() {
        eprintln!("[bridge] Role {} already installed for {} at {:?}", role_id, agent_id, target);
        // For Claude Code: extract YAML name from existing file for --agent
        let effective_role_id = if agent_id == "claudecode" {
            std::fs::read_to_string(&target).ok()
                .and_then(|content| extract_yaml_name(&content))
                .unwrap_or_else(|| role_id.to_string())
        } else {
            role_id.to_string()
        };
        // Store as active role for execute_chat --agent
        if let Ok(mut guard) = ACTIVE_ROLE.lock() {
            *guard = Some((agent_id.to_string(), effective_role_id));
        }
        send(&OutboundMessage::RoleSet {
            agent_id: agent_id.to_string(),
            role_id: role_id.to_string(),
            installed: true,
            path: target.to_string_lossy().to_string(),
        });
        return;
    }

    // Download role file from URL
    eprintln!("[bridge] Downloading role from: {}", url);
    let body = match ureq::get(url).call() {
        Ok(resp) => match resp.into_string() {
            Ok(s) => s,
            Err(e) => {
                send(&OutboundMessage::Error {
                    message: format!("Failed to read response body: {}", e),
                });
                return;
            }
        },
        Err(e) => {
            send(&OutboundMessage::Error {
                message: format!("Failed to download role from {}: {}", url, e),
            });
            return;
        }
    };

    // Create parent directories
    if let Some(parent) = target.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            send(&OutboundMessage::Error {
                message: format!("Failed to create directory {:?}: {}", parent, e),
            });
            return;
        }
    }

    // Write downloaded content to file
    match std::fs::write(&target, &body) {
        Ok(_) => {
            eprintln!("[bridge] Role {} installed for {} at {:?} ({} bytes)", role_id, agent_id, target, body.len());
            // For Claude Code: extract YAML frontmatter `name` field for --agent flag
            // Claude Code matches agents by YAML name (e.g. "叙事设计师"), not filename (e.g. "narrative-designer")
            let effective_role_id = if agent_id == "claudecode" {
                extract_yaml_name(&body).unwrap_or_else(|| role_id.to_string())
            } else {
                role_id.to_string()
            };
            // Store as active role for execute_chat --agent
            if let Ok(mut guard) = ACTIVE_ROLE.lock() {
                *guard = Some((agent_id.to_string(), effective_role_id));
            }
            send(&OutboundMessage::RoleSet {
                agent_id: agent_id.to_string(),
                role_id: role_id.to_string(),
                installed: true,
                path: target.to_string_lossy().to_string(),
            });
        }
        Err(e) => {
            send(&OutboundMessage::Error {
                message: format!("Failed to write role file: {}", e),
            });
        }
    }
}

// ── Agent Startup ──

fn handle_start_agent(agent_id: &str) {
    let cmd = match agent_id {
        "claudecode" => "claude",
        "openclaw"   => "openclaw",
        "zeroclaw"   => "zeroclaw",
        "nanobot"    => "nanobot",
        "picoclaw"   => "picoclaw",
        "hermes"     => "hermes",
        _ => {
            send(&OutboundMessage::AgentStarted {
                agent_id: agent_id.to_string(),
                success: false,
                message: format!("Unknown agent: {}", agent_id),
            });
            return;
        }
    };

    // Check if already running
    if check_running(cmd) {
        send(&OutboundMessage::AgentStarted {
            agent_id: agent_id.to_string(),
            success: true,
            message: "Already running".to_string(),
        });
        return;
    }

    // Check if installed
    let (installed, _) = check_installed(cmd);
    if !installed {
        send(&OutboundMessage::AgentStarted {
            agent_id: agent_id.to_string(),
            success: false,
            message: format!("{} is not installed on this machine", cmd),
        });
        return;
    }

    // Start the agent (detached background process)
    let result = if cfg!(target_os = "windows") {
        Command::new("cmd.exe")
            .args(["/c", "start", "/b", cmd])
            .spawn()
    } else {
        Command::new(cmd)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
    };

    match result {
        Ok(_) => {
            eprintln!("[bridge] Started agent: {}", cmd);
            send(&OutboundMessage::AgentStarted {
                agent_id: agent_id.to_string(),
                success: true,
                message: format!("{} started successfully", cmd),
            });
        }
        Err(e) => {
            send(&OutboundMessage::AgentStarted {
                agent_id: agent_id.to_string(),
                success: false,
                message: format!("Failed to start {}: {}", cmd, e),
            });
        }
    }
}

// ── Role Clearing ──

fn handle_clear_role(agent_id: &str, role_id: &str) {
    let home = home_dir();
    let target = match agent_id {
        "claudecode" => home.join(".claude").join("agents").join(format!("{}.md", role_id)),
        "openclaw"   => home.join(".openclaw").join("workspace").join("SOUL.md"),
        "zeroclaw"   => home.join(".zeroclaw").join("workspace").join("skills").join(role_id),
        "nanobot"    => home.join(".nanobot").join("workspace").join("AGENTS.md"),
        "picoclaw"   => home.join(".picoclaw").join("workspace").join("AGENT.md"),
        "hermes"     => home.join(".hermes").join("SOUL.md"),
        _ => {
            send(&OutboundMessage::Error {
                message: format!("Unknown agent: {}", agent_id),
            });
            return;
        }
    };

    if !target.exists() {
        // Already cleared
        send(&OutboundMessage::RoleCleared {
            agent_id: agent_id.to_string(),
            role_id: role_id.to_string(),
            success: true,
        });
        return;
    }

    // Delete: file or directory (OpenClaw/ZeroClaw use subdirectories)
    let result = if target.is_dir() {
        std::fs::remove_dir_all(&target)
    } else {
        std::fs::remove_file(&target)
    };

    match result {
        Ok(_) => {
            eprintln!("[bridge] Role {} cleared for {}", role_id, agent_id);
            // Clear active role state
            if let Ok(mut guard) = ACTIVE_ROLE.lock() {
                *guard = None;
            }
            send(&OutboundMessage::RoleCleared {
                agent_id: agent_id.to_string(),
                role_id: role_id.to_string(),
                success: true,
            });
        }
        Err(e) => {
            send(&OutboundMessage::Error {
                message: format!("Failed to clear role: {}", e),
            });
        }
    }
}

// ── Model Config (SINGLE SOURCE OF TRUTH) ──

fn handle_set_model(agent_id: &str, model_id: &str, model_name: &str, api_key: &str, base_url: &str, api_type: &str) {
    let home = home_dir();
    eprintln!("[bridge] set_model: agent={}, model={}, type={}", agent_id, model_id, api_type);

    let result = match agent_id {
        "hermes" => {
            // Hermes: use CLI `hermes config set` commands
            let cmds = vec![
                vec!["hermes", "config", "set", "model", model_id],
                vec!["hermes", "config", "set", "OPENAI_API_KEY", api_key],
                vec!["hermes", "config", "set", "OPENAI_BASE_URL", base_url],
            ];
            let mut last_err = None;
            for cmd in &cmds {
                match Command::new(cmd[0]).args(&cmd[1..]).output() {
                    Ok(o) if o.status.success() => {},
                    Ok(o) => { last_err = Some(String::from_utf8_lossy(&o.stderr).to_string()); },
                    Err(e) => { last_err = Some(e.to_string()); },
                }
            }
            match last_err {
                None => Ok(()),
                Some(e) => Err(format!("hermes config set failed: {}", e)),
            }
        }

        "openclaw" => {
            // OpenClaw: write fresh ~/.openclaw/openclaw.json
            let base = base_url.trim_end_matches('/');
            let provider_tag = {
                let without_protocol = base
                    .strip_prefix("https://").or_else(|| base.strip_prefix("http://"))
                    .unwrap_or(base);
                let host = without_protocol.split('/').next().unwrap_or("");
                let host = host.split(':').next().unwrap_or(host);
                if host == "localhost" || host.starts_with("127.") || host.starts_with("192.168.") {
                    "local".to_string()
                } else {
                    let parts: Vec<&str> = host.split('.').collect();
                    if parts.len() >= 2 { parts[parts.len() - 2].to_string() } else { host.to_string() }
                }
            };
            let eb_provider = format!("eb_{}", provider_tag);
            let is_anthropic = api_type == "anthropic"
                || model_id.to_lowercase().contains("claude")
                || base_url.to_lowercase().contains("anthropic");
            let oc_api_type = if is_anthropic { "anthropic-messages" } else { "openai-completions" };

            let oc_config = serde_json::json!({
                "models": {
                    "mode": "merge",
                    "providers": {
                        &eb_provider: {
                            "baseUrl": base,
                            "apiKey": api_key,
                            "api": oc_api_type,
                            "models": [{
                                "id": model_id,
                                "name": model_name,
                                "contextWindow": 128000,
                                "maxTokens": 8192,
                                "input": ["text"],
                                "reasoning": false,
                                "cost": { "input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0 }
                            }]
                        }
                    }
                },
                "agents": {
                    "defaults": {
                        "model": { "primary": format!("{}/{}", eb_provider, model_id) }
                    }
                }
            });

            // Preserve gateway token from existing config
            let oc_dir = home.join(".openclaw");
            let oc_path = oc_dir.join("openclaw.json");
            let gateway = if oc_path.exists() {
                std::fs::read_to_string(&oc_path).ok()
                    .and_then(|c| serde_json::from_str::<serde_json::Value>(&c).ok())
                    .and_then(|v| v.get("gateway").cloned())
            } else { None };

            let mut config = oc_config;
            if let Some(gw) = gateway { config["gateway"] = gw; }

            write_config_file(&oc_dir, "openclaw.json",
                &serde_json::to_string_pretty(&config).unwrap_or_default())
        }

        "zeroclaw" => {
            // ZeroClaw v2026+: top-level keys (no [providers] table!)
            // Official config: default_provider, default_model, api_key
            let base = base_url.trim_end_matches('/');
            let provider_value = if base.contains("openrouter.ai") {
                "openrouter"
            } else if base.contains("anthropic.com") {
                "anthropic"
            } else if base.contains("openai.com") {
                "openai"
            } else {
                // custom provider handled below
                ""
            };
            let toml_content = if provider_value.is_empty() {
                let url = if base.ends_with("/v1") { base.to_string() } else { format!("{}/v1", base) };
                format!("default_provider = \"custom:{}\"\ndefault_model = \"{}\"\ndefault_temperature = 0.7\napi_key = \"{}\"", url, model_id, api_key)
            } else {
                format!("default_provider = \"{}\"\ndefault_model = \"{}\"\ndefault_temperature = 0.7\napi_key = \"{}\"", provider_value, model_id, api_key)
            };

            let zc_dir = home.join(".zeroclaw");
            let result = write_config_file(&zc_dir, "config.toml", &toml_content);

            // Also set env vars as fallback (ZeroClaw checks both config.toml and env)
            std::env::set_var("OPENROUTER_API_KEY", api_key);
            std::env::set_var("OPENAI_API_KEY", api_key);

            result
        }

        "nanobot" => {
            // NanoBot: providers.custom format
            let api_base = ensure_v1_suffix(base_url);
            let config = serde_json::json!({
                "agents": { "defaults": { "model": model_id } },
                "providers": { "custom": { "apiBase": api_base, "apiKey": api_key } }
            });
            write_config_file(&home.join(".nanobot"), "config.json",
                &serde_json::to_string_pretty(&config).unwrap_or_default())
        }

        "picoclaw" => {
            // PicoClaw: model_list array format
            let api_base = ensure_v1_suffix(base_url);
            let vendor = base_url
                .find("api.")
                .and_then(|start| {
                    let after = &base_url[start + 4..];
                    after.find('.').map(|end| &after[..end])
                })
                .unwrap_or("custom");
            let vendor_model = format!("{}/{}", vendor, model_id);
            let config = serde_json::json!({
                "agents": { "defaults": { "model": model_id } },
                "model_list": [{ "model_name": model_id, "model": vendor_model, "api_key": api_key, "api_base": api_base }]
            });
            write_config_file(&home.join(".picoclaw"), "config.json",
                &serde_json::to_string_pretty(&config).unwrap_or_default())
        }


        "claudecode" => {
            // Claude Code: settings.json + onboarding skip
            let claude_dir = home.join(".claude");

            // ~/.claude.json: onboarding skip (only if missing)
            let claude_json = home.join(".claude.json");
            if !claude_json.exists() {
                let onboarding = serde_json::json!({ "hasCompletedOnboarding": true });
                let _ = std::fs::write(&claude_json, serde_json::to_string(&onboarding).unwrap_or_default());
            }

            // ~/.claude/settings.json: env vars + allowed tools
            let settings = serde_json::json!({
                "env": {
                    "ANTHROPIC_BASE_URL": base_url,
                    "ANTHROPIC_AUTH_TOKEN": api_key,
                    "API_TIMEOUT_MS": "3000000",
                    "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC": 1,
                    "ANTHROPIC_MODEL": model_id,
                    "ANTHROPIC_SMALL_FAST_MODEL": model_id,
                    "ANTHROPIC_DEFAULT_SONNET_MODEL": model_id,
                    "ANTHROPIC_DEFAULT_OPUS_MODEL": model_id,
                    "ANTHROPIC_DEFAULT_HAIKU_MODEL": model_id
                },
                "allowedTools": ["Edit","Write","Bash","Read","MultiEdit","Glob","Grep","LS","TodoRead","TodoWrite","WebFetch","NotebookRead","NotebookEdit"]
            });
            write_config_file(&claude_dir, "settings.json",
                &serde_json::to_string_pretty(&settings).unwrap_or_default())
        }

        _ => {
            send(&OutboundMessage::Error {
                message: format!("Unknown agent for set_model: {}", agent_id),
            });
            return;
        }
    };

    // Also write Echobird relay JSON for read-back (all agents)
    let eb_dir = home.join(".echobird");
    let relay = serde_json::json!({
        "apiKey": api_key, "modelId": model_id,
        "modelName": model_name, "baseUrl": base_url, "protocol": api_type,
    });
    let _ = write_config_file(&eb_dir, &format!("{}.json", agent_id),
        &serde_json::to_string_pretty(&relay).unwrap_or_default());

    match result {
        Ok(()) => {
            eprintln!("[bridge] Model set: agent={}, model={}", agent_id, model_id);
            send(&OutboundMessage::ModelSet {
                agent_id: agent_id.to_string(),
                model_id: model_id.to_string(),
                success: true,
            });
        }
        Err(e) => {
            send(&OutboundMessage::Error {
                message: format!("Failed to set model for {}: {}", agent_id, e),
            });
        }
    }
}

/// Write a config file to dir/filename, creating parent directories as needed.
fn write_config_file(dir: &std::path::Path, filename: &str, content: &str) -> Result<(), String> {
    std::fs::create_dir_all(dir)
        .map_err(|e| format!("Failed to create dir {:?}: {}", dir, e))?;
    std::fs::write(dir.join(filename), content)
        .map_err(|e| format!("Failed to write {}/{}: {}", dir.display(), filename, e))
}

/// Ensure URL ends with /v1
fn ensure_v1_suffix(url: &str) -> String {
    let base = url.trim_end_matches('/');
    if base.ends_with("/v1") { base.to_string() } else { format!("{}/v1", base) }
}

// ── Helpers ──

/// Get user home directory (cross-platform)
fn home_dir() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        std::env::var("USERPROFILE")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("C:\\Users\\default"))
    }
    #[cfg(not(target_os = "windows"))]
    {
        std::env::var("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("/root"))
    }
}

// bridge_roles_dir() removed — roles are now downloaded from URL, not copied from local files

/// Strip ANSI escape codes from a string (ESC[...m color codes, cursor moves, etc.)
/// Uses a simple state machine — no regex crate needed.
fn strip_ansi(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            // ESC found — consume the escape sequence
            if chars.peek() == Some(&'[') {
                chars.next(); // consume '['
                // Consume until we hit a letter (the terminator)
                while let Some(&next) = chars.peek() {
                    chars.next();
                    if next.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
            // else: bare ESC without '[', just skip it
        } else {
            result.push(c);
        }
    }
    result
}

/// Send a JSON message to stdout (one line)
fn send(msg: &OutboundMessage) {
    if let Ok(json) = serde_json::to_string(msg) {
        let mut stdout = io::stdout().lock();
        let _ = writeln!(stdout, "{}", json);
        let _ = stdout.flush();
    }
}

/// Detect agent version
/// Resolve a command name to its full path (handles .cmd/.bat on Windows)
fn resolve_command(command: &str) -> String {
    if cfg!(target_os = "windows") {
        // Use where.exe to find the full path of .cmd/.bat scripts
        if let Ok(output) = Command::new("where.exe").arg(command).output() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            // Take the first result line (full path)
            if let Some(path) = stdout.lines().next() {
                let path = path.trim();
                if !path.is_empty() {
                    eprintln!("[bridge] Resolved '{}' -> '{}'", command, path);
                    return path.to_string();
                }
            }
        }
    }
    command.to_string()
}

/// Detect agent version
fn detect_agent(command: &str) -> String {
    let result = if cfg!(target_os = "windows") {
        Command::new("cmd.exe").args(["/c", command, "--version"]).output()
    } else {
        Command::new(command).arg("--version").output()
    };
    match result {
        Ok(output) => String::from_utf8_lossy(&output.stdout).trim().to_string(),
        Err(_) => "not found".to_string(),
    }
}

/// Load config: CLI args (--config / --command) > plugin.json > defaults
fn load_config() -> BridgeConfig {
    let args: Vec<String> = std::env::args().collect();

    // Handle --version flag (used by ensure_remote_bridge for version checking)
    if args.iter().any(|a| a == "--version") {
        println!("{}", env!("CARGO_PKG_VERSION"));
        std::process::exit(0);
    }

    // 1. Check CLI args: --config "/path/to/plugin.json"
    for i in 0..args.len() {
        if args[i] == "--config" {
            if let Some(config_path) = args.get(i + 1) {
                if let Ok(content) = std::fs::read_to_string(config_path) {
                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
                        if let Some(cli) = json.get("cli") {
                            let config = parse_plugin_json(cli);
                            eprintln!("[bridge] Loaded config from --config: {}", config_path);
                            return config;
                        }
                    }
                }
                eprintln!("[bridge] WARN: --config '{}' not found or invalid", config_path);
            }
        }
    }

    // 2. Check CLI args: --command "zeroclaw agent --json"
    for i in 0..args.len() {
        if args[i] == "--command" {
            if let Some(cmd_str) = args.get(i + 1) {
                let parts: Vec<&str> = cmd_str.split_whitespace().collect();
                if !parts.is_empty() {
                    let command = parts[0].to_string();
                    let cmd_args: Vec<String> = parts[1..].iter().map(|s| s.to_string()).collect();
                    // Build config from provided command + args
                    // Message is appended as last arg by execute_chat()
                    // Agents that need --message flag include it in their args (e.g. openclaw agent --json --message)
                    // Agents that use positional args (e.g. claude -p) just get message appended
                    let chat_args = cmd_args.clone();
                    let mut resume_args = cmd_args;
                    resume_args.push("--session-id".to_string());
                    resume_args.push("{sessionId}".to_string());
                    // Check for optional --agent-arg (e.g. --agent-arg "--agent")
                    let mut agent_arg: Option<String> = None;
                    for j in 0..args.len() {
                        if args[j] == "--agent-arg" {
                            if let Some(val) = args.get(j + 1) {
                                agent_arg = Some(val.to_string());
                            }
                        }
                    }
                    eprintln!("[bridge] Using CLI config: {} (agent_arg: {:?})", cmd_str, agent_arg);
                    return BridgeConfig {
                        command,
                        args: chat_args,
                        resume_args,
                        session_arg: Some("--session-id".to_string()),
                        model_arg: None,
                        system_prompt_arg: None,
                        agent_arg,
                    };
                }
            }
        }
    }


    // 3. Check plugin.json in same directory as binary
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()));

    if let Some(dir) = exe_dir {
        let plugin_json = dir.join("plugin.json");
        if let Ok(content) = std::fs::read_to_string(&plugin_json) {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
                if let Some(cli) = json.get("cli") {
                    let config = parse_plugin_json(cli);
                    eprintln!("[bridge] Loaded config from {:?}", plugin_json);
                    return config;
                }
            }
        }
    }

    // 4. Default: openclaw
    eprintln!("[bridge] Using default config (openclaw agent)");
    BridgeConfig::default()
}

/// Parse plugin.json "cli" section into BridgeConfig
fn parse_plugin_json(cli: &serde_json::Value) -> BridgeConfig {
    let mut config = BridgeConfig::default();
    if let Some(cmd) = cli.get("command").and_then(|v| v.as_str()) {
        config.command = cmd.to_string();
    }
    if let Some(args) = cli.get("args").and_then(|v| v.as_array()) {
        config.args = args.iter()
            .filter_map(|a| a.as_str().map(String::from))
            .collect();
    }
    if let Some(args) = cli.get("resumeArgs").and_then(|v| v.as_array()) {
        config.resume_args = args.iter()
            .filter_map(|a| a.as_str().map(String::from))
            .collect();
    }
    if let Some(arg) = cli.get("sessionArg").and_then(|v| v.as_str()) {
        config.session_arg = Some(arg.to_string());
    }
    if let Some(arg) = cli.get("modelArg").and_then(|v| v.as_str()) {
        config.model_arg = Some(arg.to_string());
    }
    if let Some(arg) = cli.get("systemPromptArg").and_then(|v| v.as_str()) {
        config.system_prompt_arg = Some(arg.to_string());
    }
    if let Some(arg) = cli.get("agentArg").and_then(|v| v.as_str()) {
        config.agent_arg = Some(arg.to_string());
    }
    config
}

