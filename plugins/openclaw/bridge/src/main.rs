// Echobird Bridge — remote Agent communication via stdin/stdout JSON
//
// Runs on the remote machine, receives JSON commands from Echobird via SSH,
// invokes the Agent CLI (e.g. claude), and streams responses back as JSON lines.
//
// Protocol:
//   stdin  → {"type":"chat","message":"...","session_id":"..."}
//   stdout ← {"type":"text_delta","text":"..."}
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
            command: "claude".to_string(),
            args: vec![
                "-p".to_string(),
                "--output-format".to_string(),
                "json".to_string(),
                "--dangerously-skip-permissions".to_string(),
            ],
            resume_args: vec![
                "-p".to_string(),
                "--output-format".to_string(),
                "json".to_string(),
                "--dangerously-skip-permissions".to_string(),
                "--resume".to_string(),
                "{sessionId}".to_string(),
            ],
            session_arg: Some("--session-id".to_string()),
            model_arg: Some("--model".to_string()),
            system_prompt_arg: Some("--append-system-prompt".to_string()),
        }
    }
}

// ── Main Loop ──

fn main() {
    let config = load_config();

    // Send ready status
    send(&OutboundMessage::Status {
        agent: "openclaw".to_string(),
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
            // Check if agent CLI is available
            let version = detect_agent(&config.command);
            send(&OutboundMessage::Status {
                agent: "openclaw".to_string(),
                version,
                ready: true,
            });
        }
        InboundMessage::Abort { .. } => {
            // Can't abort a running subprocess easily, just acknowledge
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
    system_prompt: Option<&str>,
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
        // Add session ID if provided
        if let (Some(sid), Some(session_arg)) = (session_id, &config.session_arg) {
            a.push(session_arg.clone());
            a.push(sid.to_string());
        }
        a
    };

    // Add model if specified
    if let (Some(m), Some(model_arg)) = (model, &config.model_arg) {
        args.push(model_arg.clone());
        args.push(m.to_string());
    }

    // Add system prompt if specified (first message only)
    if let (Some(sp), Some(sp_arg)) = (system_prompt, &config.system_prompt_arg) {
        args.push(sp_arg.clone());
        args.push(sp.to_string());
    }

    // Add message as last arg
    args.push(message.to_string());

    // Execute the CLI
    eprintln!("[bridge] Executing: {} {}", config.command, args.join(" "));

    match Command::new(&config.command)
        .args(&args)
        .output()
    {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();

            if !stderr.is_empty() {
                eprintln!("[bridge] stderr: {}", stderr);
            }

            // Try to parse JSON output (claude --output-format json)
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

/// Parse agent CLI JSON output, extracting response text and session ID
fn parse_agent_output(stdout: &str) -> (String, Option<String>) {
    // Try JSON parsing first
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(stdout) {
        // Claude Code JSON output format
        let text = json.get("result")
            .or_else(|| json.get("text"))
            .or_else(|| json.get("content"))
            .and_then(|v| v.as_str())
            .unwrap_or(stdout)
            .to_string();

        let session_id = json.get("session_id")
            .or_else(|| json.get("conversation_id"))
            .and_then(|v| v.as_str())
            .map(String::from);

        (text, session_id)
    } else {
        // Try JSONL (line-by-line)
        let mut text = String::new();
        let mut session_id = None;
        for line in stdout.lines() {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(line) {
                if let Some(t) = json.get("content").or_else(|| json.get("text")).and_then(|v| v.as_str()) {
                    text.push_str(t);
                }
                if session_id.is_none() {
                    session_id = json.get("session_id")
                        .or_else(|| json.get("thread_id"))
                        .and_then(|v| v.as_str())
                        .map(String::from);
                }
            }
        }
        if text.is_empty() {
            // Fallback: raw text
            (stdout.to_string(), None)
        } else {
            (text, session_id)
        }
    }
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
fn detect_agent(command: &str) -> String {
    match Command::new(command).arg("--version").output() {
        Ok(output) => String::from_utf8_lossy(&output.stdout).trim().to_string(),
        Err(_) => "not found".to_string(),
    }
}

/// Load config from plugin.json in same directory, or use defaults
fn load_config() -> BridgeConfig {
    // Try to load plugin.json from the same directory as the binary
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

    eprintln!("[bridge] Using default config (claude CLI)");
    BridgeConfig::default()
}
