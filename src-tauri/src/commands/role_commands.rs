// Agent detection — local agent CLI availability checks
// Role catalog is now loaded from CDN in the frontend (tauri.ts → echobird.ai/roles/)

use serde::{Deserialize, Serialize};

// ── Local Agent Detection ──

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentStatus {
    pub id: String,
    pub name: String,
    pub installed: bool,
    pub path: Option<String>,
}

/// Detect which Agent CLI tools are installed on this machine
#[tauri::command]
pub fn detect_local_agents() -> Vec<AgentStatus> {
    let agents = [
        ("claudecode", "Claude Code", "claude"),
        ("opencode",   "OpenCode",    "opencode"),
        ("openclaw",   "OpenClaw",    "openclaw"),
        ("zeroclaw",   "ZeroClaw",    "zeroclaw"),
        ("nanobot",    "NanoBot",     "nanobot"),
        ("picoclaw",   "PicoClaw",    "picoclaw"),
        ("openfang",   "OpenFang",    "openfang"),
        ("hermes",     "Hermes Agent","hermes"),
    ];

    agents.iter().map(|&(id, name, cmd)| {
        let (installed, path) = check_command_installed(cmd);
        AgentStatus {
            id: id.to_string(),
            name: name.to_string(),
            installed,
            path,
        }
    }).collect()
}

fn check_command_installed(cmd: &str) -> (bool, Option<String>) {
    let result = {
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            std::process::Command::new("where.exe")
                .arg(cmd)
                .creation_flags(0x08000000) // CREATE_NO_WINDOW
                .output()
        }
        #[cfg(not(target_os = "windows"))]
        {
            std::process::Command::new("which").arg(cmd).output()
        }
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
