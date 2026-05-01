// Agent detection — reuses tool_manager::scan_tools() for consistent detection
// Role catalog is now loaded from CDN in the frontend (tauri.ts → echobird.ai/roles/)

use serde::{Deserialize, Serialize};
use crate::services::tool_manager;

// ── Local Agent Detection ──

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentStatus {
    pub id: String,
    pub name: String,
    pub installed: bool,
    pub running: bool,
    pub path: Option<String>,
}

/// Agent CLI tool IDs detected on the local machine.
/// These must match the tool directory names under tools/ (e.g. tools/claudecode/).
const AGENT_TOOL_IDS: &[&str] = &[
    "openclaw",
    "claudecode",
    "zeroclaw",
    "nanobot",
    "picoclaw",
    "hermes",
];

/// Detect which Agent CLI tools are installed on this machine.
/// Reuses scan_tools() from tool_manager for consistent detection logic
/// (requireConfigFile, platform paths, config dir checks, etc.).
#[tauri::command]
pub async fn detect_local_agents() -> Vec<AgentStatus> {
    let detected = tool_manager::scan_tools().await;
    // We already have a get_running_tools endpoint that checks process existence
    let running_tools = crate::services::process_manager::get_running_tools().await;

    AGENT_TOOL_IDS.iter().map(|&agent_id| {
        let tool = detected.iter().find(|t| t.id == agent_id);
        AgentStatus {
            id: agent_id.to_string(),
            name: tool.map(|t| t.name.clone()).unwrap_or_else(|| agent_id.to_string()),
            installed: tool.map(|t| t.installed).unwrap_or(false),
            running: running_tools.contains(&agent_id.to_string()),
            path: tool.and_then(|t| t.detected_path.clone()),
        }
    }).collect()
}
