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
use std::process::Command;

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
    },
    #[serde(rename = "resume")]
    Resume {
        message: String,
        session_id: String,
        model: Option<String>,
    },
    #[serde(rename = "status")]
    Status {},
    #[serde(rename = "abort")]
    Abort {
        session_id: Option<String>,
    },
    #[serde(rename = "ping")]
    Ping {},
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
}

// ── Config ──

struct BridgeConfig {
    command: String,
    args: Vec<String>,
    resume_args: Vec<String>,
    session_arg: Option<String>,
    model_arg: Option<String>,
    system_prompt_arg: Option<String>,
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
        } => {
            execute_chat(config, &message, session_id.as_deref(), model.as_deref(), system_prompt.as_deref(), false);
        }
        InboundMessage::Resume {
            message,
            session_id,
            model,
        } => {
            execute_chat(config, &message, Some(&session_id), model.as_deref(), None, true);
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
    }
}

fn execute_chat(
    config: &BridgeConfig,
    message: &str,
    session_id: Option<&str>,
    model: Option<&str>,
    _system_prompt: Option<&str>,
    is_resume: bool,
) {
    // Build command args
    let mut args: Vec<String> = if is_resume {
        // Resume: use resume_args, replace {sessionId}
        let sid = session_id.unwrap_or("unknown");
        config.resume_args.iter()
            .map(|a| a.replace("{sessionId}", sid))
            .collect()
    } else {
        // New chat: use standard args
        let mut a = config.args.clone();
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
        Command::new("cmd.exe").args(&cmd_args).output()
    } else {
        Command::new(&config.command).args(&args).output()
    };

    match result
    {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();

            if !stderr.is_empty() {
                eprintln!("[bridge] stderr: {}", stderr);
            }

            // Parse OpenClaw agent --json output
            let (text, new_session_id) = parse_openclaw_output(&stdout);

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

/// Parse OpenClaw `agent --json` output
///
/// Supports two formats:
/// 1. Wrapped: { "result": { "payloads": [...], "meta": { "agentMeta": { "sessionId": "..." } } } }
/// 2. Direct:  { "payloads": [...], "meta": { "agentMeta": { "sessionId": "..." } } }
fn parse_openclaw_output(stdout: &str) -> (String, Option<String>) {
    // Find JSON in stdout (skip non-JSON lines like [Echobird] injection logs)
    let json_str = find_json_object(stdout);
    let json_str = match json_str {
        Some(s) => s,
        None => return (stdout.to_string(), None),
    };

    match serde_json::from_str::<serde_json::Value>(&json_str) {
        Ok(json) => {
            // Try both: result.payloads (wrapped) and top-level payloads (direct)
            let payloads = json.get("result")
                .and_then(|r| r.get("payloads"))
                .or_else(|| json.get("payloads"));

            let text = payloads
                .and_then(|p| p.as_array())
                .and_then(|arr| {
                    let texts: Vec<&str> = arr.iter()
                        .filter_map(|p| p.get("text").and_then(|t| t.as_str()))
                        .collect();
                    if texts.is_empty() { None } else { Some(texts.join("\n")) }
                })
                .unwrap_or_else(|| {
                    // Fallback: try result.text or top-level text
                    json.get("result")
                        .and_then(|r| r.get("text"))
                        .or_else(|| json.get("text"))
                        .and_then(|v| v.as_str())
                        .unwrap_or(stdout)
                        .to_string()
                });

            // Extract session ID: try both nesting levels
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

/// Load config: CLI args (--command) > plugin.json > defaults
fn load_config() -> BridgeConfig {
    // 1. Check CLI args: --command "zeroclaw agent --json"
    let args: Vec<String> = std::env::args().collect();
    for i in 0..args.len() {
        if args[i] == "--command" {
            if let Some(cmd_str) = args.get(i + 1) {
                let parts: Vec<&str> = cmd_str.split_whitespace().collect();
                if !parts.is_empty() {
                    let command = parts[0].to_string();
                    let cmd_args: Vec<String> = parts[1..].iter().map(|s| s.to_string()).collect();
                    // Build a config with the provided command + args + --message at the end
                    let mut chat_args = cmd_args.clone();
                    if !chat_args.contains(&"--message".to_string()) {
                        chat_args.push("--message".to_string());
                    }
                    let mut resume_args = cmd_args;
                    resume_args.push("--session-id".to_string());
                    resume_args.push("{sessionId}".to_string());
                    resume_args.push("--message".to_string());
                    eprintln!("[bridge] Using CLI config: {}", cmd_str);
                    return BridgeConfig {
                        command,
                        args: chat_args,
                        resume_args,
                        session_arg: Some("--session-id".to_string()),
                        model_arg: None,
                        system_prompt_arg: None,
                    };
                }
            }
        }
    }

    // 2. Check plugin.json in same directory
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()));

    if let Some(dir) = exe_dir {
        let plugin_json = dir.join("plugin.json");
        if let Ok(content) = std::fs::read_to_string(&plugin_json) {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
                if let Some(cli) = json.get("cli") {
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

                    eprintln!("[bridge] Loaded config from {:?}", plugin_json);
                    return config;
                }
            }
        }
    }

    // 3. Default: openclaw
    eprintln!("[bridge] Using default config (openclaw agent)");
    BridgeConfig::default()
}
