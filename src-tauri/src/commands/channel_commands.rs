// Channel persistence commands �?save/load channel configurations

use crate::utils::platform::echobird_dir;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelConfig {
    pub id: i32,
    pub name: String,
    pub protocol: String,
    pub address: String,
}

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

/// Chat with OpenClaw agent locally via CLI bridge
#[tauri::command]
pub async fn bridge_chat_local(message: String, session_id: Option<String>) -> Result<BridgeChatResult, String> {
    use std::process::Command;

    log::info!("[BridgeChat] message={}, session_id={:?}", &message[..message.len().min(50)], session_id);

    // Build args: openclaw agent --json --agent main [--session-id <id>] --message <text>
    let mut args = vec![
        "agent".to_string(),
        "--json".to_string(),
        "--agent".to_string(),
        "main".to_string(),
    ];
    if let Some(ref sid) = session_id {
        args.push("--session-id".to_string());
        args.push(sid.clone());
    }
    args.push("--message".to_string());
    args.push(message);

    // Execute: on Windows use cmd /c to resolve .cmd scripts
    let output = if cfg!(target_os = "windows") {
        let mut cmd_args = vec!["/c".to_string(), "openclaw".to_string()];
        cmd_args.extend(args);
        Command::new("cmd.exe").args(&cmd_args).output()
    } else {
        Command::new("openclaw").args(&args).output()
    };

    let output = output.map_err(|e| format!("Failed to execute openclaw: {}", e))?;
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if !stderr.is_empty() {
        log::warn!("[BridgeChat] stderr: {}", &stderr[..stderr.len().min(200)]);
    }

    // Parse OpenClaw agent --json output
    // Find the JSON object in stdout (skip [Echobird] injection lines)
    let json_str = find_json_in_output(&stdout);
    match json_str {
        Some(json_str) => {
            match serde_json::from_str::<serde_json::Value>(&json_str) {
                Ok(json) => {
                    // Extract text from result.payloads[].text
                    let text = json.get("result")
                        .and_then(|r| r.get("payloads"))
                        .and_then(|p| p.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|p| p.get("text").and_then(|t| t.as_str()))
                                .collect::<Vec<_>>()
                                .join("\n")
                        })
                        .unwrap_or_else(|| stdout.clone());

                    // Extract session ID from result.meta.agentMeta.sessionId
                    let agent_meta = json.get("result")
                        .and_then(|r| r.get("meta"))
                        .and_then(|m| m.get("agentMeta"));

                    let sid = agent_meta
                        .and_then(|am| am.get("sessionId"))
                        .and_then(|v| v.as_str())
                        .map(String::from);

                    // Extract metadata for UI display
                    let model = agent_meta
                        .and_then(|am| am.get("model"))
                        .and_then(|v| v.as_str())
                        .map(String::from);

                    let tokens = agent_meta
                        .and_then(|am| am.get("usage"))
                        .and_then(|u| u.get("total"))
                        .and_then(|v| v.as_u64());

                    let duration_ms = json.get("result")
                        .and_then(|r| r.get("meta"))
                        .and_then(|m| m.get("durationMs"))
                        .and_then(|v| v.as_u64());

                    Ok(BridgeChatResult {
                        text,
                        session_id: sid.or(session_id),
                        model,
                        tokens,
                        duration_ms,
                    })
                }
                Err(e) => {
                    log::warn!("[BridgeChat] JSON parse error: {}", e);
                    Ok(BridgeChatResult {
                        text: stdout,
                        session_id,
                        model: None,
                        tokens: None,
                        duration_ms: None,
                    })
                }
            }
        }
        None => {
            // No JSON found — return raw output or error
            if !output.status.success() {
                Err(format!("openclaw agent failed: {}", stderr))
            } else {
                Ok(BridgeChatResult {
                    text: stdout,
                    session_id,
                    model: None,
                    tokens: None,
                    duration_ms: None,
                })
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeChatResult {
    pub text: String,
    pub session_id: Option<String>,
    pub model: Option<String>,
    pub tokens: Option<u64>,
    pub duration_ms: Option<u64>,
}

/// Find the first JSON object in text output (skips non-JSON prefix lines)
fn find_json_in_output(input: &str) -> Option<String> {
    if let Some(start) = input.find('{') {
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
