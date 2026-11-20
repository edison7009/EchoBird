// Parasite Commands — Tauri IPC for the Mother Agent's "Connect" mode.
// Currently delegates the turn to Claude Code only; the wrapped agent
// uses its own configured model (set via App Desktop or `claude /model`).

use crate::services::parasite::{self, ParasiteEvent, ParasiteSession, SharedParasiteSessions};
use serde::Deserialize;
use tauri::{AppHandle, State};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParasiteSendRequest {
    pub agent_id: String,
    pub message: String,
    #[serde(default)]
    pub conversation_id: Option<String>,
    #[serde(default)]
    pub request_id: Option<String>,
}

/// Return the IDs of parasitable agents that respond to `--version` locally.
/// Used by the frontend to decide which entries to show in the agent picker.
#[tauri::command]
pub async fn parasite_list_installed() -> Vec<String> {
    parasite::detect_installed().await
}

/// Run one turn against the selected agent. Streams a `parasite_event` event
/// stream back to the frontend (text_delta / error / done / state).
#[tauri::command]
pub async fn parasite_send_message(
    app: AppHandle,
    sessions: State<'_, SharedParasiteSessions>,
    request: ParasiteSendRequest,
) -> Result<(), String> {
    let key = parasite::session_key(&request.agent_id, request.conversation_id.as_deref());
    // Reject re-entry while the same agent is mid-turn so the user can't
    // stack overlapping subprocesses by mashing enter.
    {
        let mut map = sessions.lock().await;
        let sess = map.entry(key.clone()).or_insert_with(|| {
            let mut sess = ParasiteSession::new();
            sess.session_id = parasite::load_session_from_disk(&key);
            sess
        });
        sess.prepare()?;
    }

    let sessions_clone = sessions.inner().clone();
    let app_clone = app.clone();
    let agent_id = request.agent_id.clone();
    let message = request.message;
    let conversation_id = request.conversation_id;
    let request_id = request.request_id;

    tokio::spawn(async move {
        if let Err(e) = parasite::send_message(
            app,
            sessions_clone.clone(),
            agent_id,
            message,
            conversation_id.clone(),
            request_id.clone(),
        )
        .await
        {
            log::error!("[ParasiteCommand] send_message error: {}", e);
            if let Some(sess) = sessions_clone.lock().await.get_mut(&key) {
                sess.running = false;
            }
            parasite::emit_event(
                &app_clone,
                conversation_id.as_deref(),
                request_id.as_deref(),
                ParasiteEvent::Error { message: e },
            );
            parasite::emit_event(
                &app_clone,
                conversation_id.as_deref(),
                request_id.as_deref(),
                ParasiteEvent::Done { summary: None },
            );
        }
    });

    Ok(())
}

/// Cancel the in-flight turn for a specific agent (kills its subprocess).
/// Returns `true` when a running turn was actually interrupted.
#[tauri::command]
pub async fn parasite_abort(
    sessions: State<'_, SharedParasiteSessions>,
    agent_id: String,
    conversation_id: Option<String>,
) -> Result<bool, String> {
    let mut map = sessions.lock().await;
    let key = parasite::session_key(&agent_id, conversation_id.as_deref());
    if let Some(sess) = map.get_mut(&key) {
        if sess.running {
            sess.abort();
            log::info!("[ParasiteCommand] aborted parasite agent: {}", agent_id);
            return Ok(true);
        }
    }
    Ok(false)
}

/// Forget the stored session ID for an agent so the next turn starts a fresh
/// conversation (Claude Code stops resuming the prior `--session-id`).
#[tauri::command]
pub async fn parasite_reset(
    sessions: State<'_, SharedParasiteSessions>,
    agent_id: String,
    conversation_id: Option<String>,
) -> Result<(), String> {
    let mut map = sessions.lock().await;
    let key = parasite::session_key(&agent_id, conversation_id.as_deref());
    if map.get(&key).is_some_and(|sess| sess.running) {
        return Err("Parasite agent is still processing a request".into());
    }
    // Keep the resume target intact if removing its persisted copy fails.
    parasite::clear_session_from_disk(&key)?;
    if let Some(sess) = map.get_mut(&key) {
        sess.reset()?;
        log::info!("[ParasiteCommand] reset parasite session: {}", agent_id);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn send_request_supports_existing_calls_and_scoped_camel_case_calls() {
        let legacy: ParasiteSendRequest = serde_json::from_value(serde_json::json!({
            "agentId": "claudecode",
            "message": "hello"
        }))
        .unwrap();
        assert!(legacy.conversation_id.is_none());
        assert!(legacy.request_id.is_none());
        let scoped: ParasiteSendRequest = serde_json::from_value(serde_json::json!({
            "agentId": "claudecode",
            "message": "hello",
            "conversationId": "conversation-a",
            "requestId": "request-a"
        }))
        .unwrap();
        assert_eq!(scoped.conversation_id.as_deref(), Some("conversation-a"));
        assert_eq!(scoped.request_id.as_deref(), Some("request-a"));
    }
}
