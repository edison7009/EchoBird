// Agent Commands — Tauri IPC commands for the MotherAgent frontend

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};
use crate::commands::ssh_commands::SSHPool;
use crate::services::agent_loop::{self, AgentRequest, SharedSessionMap};
use crate::services::llm_client::MessageContent;

/// Send a message to the agent. The agent will process asynchronously
/// and emit `agent_event` events to the frontend.
#[tauri::command]
pub async fn agent_send_message(
    app: AppHandle,
    session_map: State<'_, SharedSessionMap>,
    ssh_pool: State<'_, SSHPool>,
    request: AgentRequest,
) -> Result<String, String> {
    let server_key = request.server_ids.first()
        .cloned()
        .unwrap_or_else(|| "local".to_string());

    // Check if already running on this server
    {
        let map = session_map.lock().await;
        if let Some(sess) = map.get(&server_key) {
            if sess.running {
                return Err("Agent is already processing a request".into());
            }
        }
    }

    let map_clone = session_map.inner().clone();
    let pool_clone = ssh_pool.inner().clone();

    // Get or create session ID
    let session_id = {
        let mut map = session_map.lock().await;
        let sess = map.entry(server_key.clone()).or_insert_with(|| {
            let mut s = agent_loop::AgentSession::new();
            s.messages = agent_loop::load_session_from_disk(&server_key);
            s
        });
        sess.id.clone()
    };

    // Spawn agent as background task
    let app_clone = app.clone();
    tokio::spawn(async move {
        if let Err(e) = agent_loop::run_agent(app, request, map_clone, pool_clone).await {
            log::error!("[AgentCommand] Agent error: {}", e);
            // Safety net: ensure frontend is never left stuck in isProcessing
            let _ = app_clone.emit("agent_event", serde_json::json!({
                "type": "error",
                "message": e,
            }));
            let _ = app_clone.emit("agent_event", serde_json::json!({
                "type": "done",
            }));
        }
    });

    Ok(session_id)
}

/// Abort the current agent execution
#[tauri::command]
pub async fn agent_abort(
    session_map: State<'_, SharedSessionMap>,
    server_key: String,
) -> Result<bool, String> {
    let mut map = session_map.lock().await;
    if let Some(sess) = map.get_mut(&server_key) {
        if sess.running {
            sess.cancel();
            log::info!("[AgentCommand] Agent aborted for server: {}", server_key);
            return Ok(true);
        }
    }
    Ok(false)
}

/// Reset the agent session for a specific server (clear conversation history)
#[tauri::command]
pub async fn agent_reset(
    session_map: State<'_, SharedSessionMap>,
    server_key: String,
) -> Result<String, String> {
    let mut map = session_map.lock().await;
    let sess = map.entry(server_key.clone()).or_insert_with(agent_loop::AgentSession::new);
    *sess = agent_loop::AgentSession::new();
    // Also clear persisted session file from disk
    agent_loop::clear_session_from_disk(&server_key);
    log::info!("[AgentCommand] Session reset for server {}: {}", server_key, sess.id);
    Ok(sess.id.clone())
}
