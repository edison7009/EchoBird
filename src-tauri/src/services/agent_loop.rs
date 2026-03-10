// Agent Loop — the core ReAct loop (Reason → Act → Observe → Repeat)
// Messages are streamed to the frontend via Tauri events.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use tauri::{AppHandle, Emitter};

use crate::commands::ssh_commands::SSHPool;
use super::llm_client::*;
use super::agent_tools;

// ── Constants ──

const MAX_TOOL_LOOPS: usize = 25; // Prevent infinite execution
// Byte-based context limit: keep recent messages whose total size fits within this budget.
// 30-message count limit was not safe when tool results are large (e.g. 8KB each × 30 = 240KB+).
const MAX_CONTEXT_BYTES: usize = 150_000; // ~150KB total payload to LLM

// ── Types emitted to frontend ──

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum AgentEvent {
    #[serde(rename = "text_delta")]
    TextDelta { text: String },
    #[serde(rename = "thinking")]
    Thinking { text: String },
    #[serde(rename = "tool_call_start")]
    ToolCallStart { id: String, name: String },
    #[serde(rename = "tool_call_args")]
    ToolCallArgs { id: String, args: String },
    #[serde(rename = "tool_result")]
    ToolResult { id: String, output: String, success: bool },
    #[serde(rename = "done")]
    Done { },
    #[serde(rename = "error")]
    Error { message: String },
    #[serde(rename = "state")]
    StateChange { state: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRequest {
    pub message: String,
    pub model_id: String,
    pub base_url: String,        // OpenAI-compatible URL
    pub api_key: String,
    pub model_name: String,
    pub provider: String,        // initial preferred protocol: "openai" or "anthropic"
    /// Optional Anthropic-compatible URL. When present the agent always tries Anthropic
    /// first and falls back to OpenAI (`base_url`) on 400 / tool-unsupported errors.
    pub anthropic_url: Option<String>,
    pub proxy_url: Option<String>,
    pub server_ids: Vec<String>,  // selected SSH servers
    pub skills: Vec<String>,      // skill descriptions
    /// UI locale (e.g. "zh-Hans", "en", "ja"). Used to hint the agent's response language.
    pub locale: Option<String>,
}

// ── Session State (kept in memory for continuous operation) ──

pub struct AgentSession {
    pub id: String,
    pub messages: Vec<Message>,
    pub running: bool,
    pub cancel_token: CancellationToken,
}

impl AgentSession {
    pub fn new() -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            messages: Vec::new(),
            running: false,
            cancel_token: CancellationToken::new(),
        }
    }

    /// Cancel current operation and create a fresh token for next run
    pub fn cancel(&mut self) {
        self.cancel_token.cancel();
        self.running = false;
    }

    /// Prepare for a new run
    pub fn prepare_run(&mut self) {
        if self.cancel_token.is_cancelled() {
            self.cancel_token = CancellationToken::new();
        }
        self.running = true;
    }
}

// Per-server session map (keyed by server_id: "local" or SSH server id)
pub type SharedSessionMap = Arc<Mutex<std::collections::HashMap<String, AgentSession>>>;

pub fn create_session_map() -> SharedSessionMap {
    Arc::new(Mutex::new(std::collections::HashMap::new()))
}

// ── Session Persistence ──

fn sessions_dir() -> std::path::PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join(".echobird")
        .join("config")
        .join("agent_sessions")
}

fn session_file(server_key: &str) -> std::path::PathBuf {
    // Sanitize server_key for filename
    let safe_key = server_key.replace(['/', '\\', ':', '*', '?', '"', '<', '>', '|'], "_");
    sessions_dir().join(format!("{}.json", safe_key))
}

/// Save a session's messages to disk
pub fn save_session_to_disk(server_key: &str, messages: &[Message]) {
    if let Err(e) = std::fs::create_dir_all(sessions_dir()) {
        log::error!("[AgentSession] Failed to create sessions dir: {}", e);
        return;
    }
    let path = session_file(server_key);
    match serde_json::to_string_pretty(messages) {
        Ok(json) => {
            if let Err(e) = std::fs::write(&path, json) {
                log::error!("[AgentSession] Failed to write session {}: {}", server_key, e);
            }
        }
        Err(e) => log::error!("[AgentSession] Failed to serialize session {}: {}", server_key, e),
    }
}

/// Load a session's messages from disk
pub fn load_session_from_disk(server_key: &str) -> Vec<Message> {
    let path = session_file(server_key);
    if !path.exists() {
        return Vec::new();
    }
    match std::fs::read_to_string(&path) {
        Ok(json) => serde_json::from_str(&json).unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

/// Clear a session's persisted file from disk
pub fn clear_session_from_disk(server_key: &str) {
    let path = session_file(server_key);
    if path.exists() {
        if let Err(e) = std::fs::remove_file(&path) {
            log::error!("[AgentSession] Failed to delete session file {}: {}", server_key, e);
        } else {
            log::info!("[AgentSession] Session file deleted for {}", server_key);
        }
    }
}

// ── Main Agent Loop ──

pub async fn run_agent(
    app: AppHandle,
    request: AgentRequest,
    session_map: SharedSessionMap,
    ssh_pool: SSHPool,
) -> Result<(), String> {
    // Derive server key from request
    let server_key = request.server_ids.first()
        .cloned()
        .unwrap_or_else(|| "local".to_string());

    // 1. Build LLM clients — triple fallback strategy:
    //   a) Anthropic client (when anthropic_url is set)
    //   b) On 400 / tool-unsupported → downgrade to OpenAI client
    let decrypted_key = super::model_manager::decrypt_key_for_use(&request.api_key);

    // Primary client: Anthropic if URL is available, otherwise OpenAI
    let (mut active_provider, mut client) = if let Some(ref anth_url) = request.anthropic_url {
        let cfg = LlmConfig {
            provider: LlmProvider::Anthropic,
            base_url: anth_url.clone(),
            api_key: decrypted_key.clone(),
            model: request.model_name.clone(),
            proxy_url: request.proxy_url.clone(),
        };
        (LlmProvider::Anthropic, LlmClient::new(cfg)?)
    } else {
        let cfg = LlmConfig {
            provider: LlmProvider::OpenAI,
            base_url: request.base_url.clone(),
            api_key: decrypted_key.clone(),
            model: request.model_name.clone(),
            proxy_url: request.proxy_url.clone(),
        };
        (LlmProvider::OpenAI, LlmClient::new(cfg)?)
    };

    // Fallback OpenAI client — built only when needed
    let openai_fallback: Option<LlmClient> = if request.anthropic_url.is_some() && !request.base_url.is_empty() {
        let cfg = LlmConfig {
            provider: LlmProvider::OpenAI,
            base_url: request.base_url.clone(),
            api_key: decrypted_key.clone(),
            model: request.model_name.clone(),
            proxy_url: request.proxy_url.clone(),
        };
        LlmClient::new(cfg).ok()
    } else {
        None
    };
    let mut protocol_downgraded = false;
    let tools = agent_tools::get_tool_definitions();

    // 2. Build system prompt
    let system_prompt = build_system_prompt(&request, &ssh_pool).await;

    // 3. Add user message to per-server session history
    let cancel_token = {
        let mut map = session_map.lock().await;
        let sess = map.entry(server_key.clone()).or_insert_with(|| {
            let mut s = AgentSession::new();
            s.messages = load_session_from_disk(&server_key);
            s
        });
        sess.prepare_run();
        sess.messages.push(Message {
            role: "user".into(),
            content: MessageContent::Text(request.message.clone()),
        });
        sess.cancel_token.clone()
    };

    emit_event(&app, AgentEvent::StateChange { state: "processing".into() });

    // 4. ReAct loop
    let mut loop_count = 0;
    let mut sse_retry_count = 0;
    const MAX_SSE_RETRIES: u32 = 3;

    loop {
        loop_count += 1;
        if loop_count > MAX_TOOL_LOOPS {
            emit_event(&app, AgentEvent::Error {
                message: format!("Reached maximum tool call limit ({})", MAX_TOOL_LOOPS),
            });
            break;
        }

        // Check cancellation
        if cancel_token.is_cancelled() {
            log::info!("[AgentLoop] Cancelled by user");
            emit_event(&app, AgentEvent::Error { message: "Cancelled by user".into() });
            break;
        }

        // Get current messages (byte-budget truncation: keep most-recent messages within 150KB)
        let messages = {
            let map = session_map.lock().await;
            let all = map.get(&server_key).map(|s| s.messages.clone()).unwrap_or_default();
            // Walk backwards accumulating until budget is exceeded, then reverse
            let mut budget = MAX_CONTEXT_BYTES;
            let mut kept: Vec<_> = all.iter().rev().take_while(|m| {
                let sz = serde_json::to_string(m).map(|s| s.len()).unwrap_or(256);
                if sz > budget { return false; }
                budget -= sz;
                true
            }).cloned().collect();
            kept.reverse();
            if kept.len() < all.len() {
                log::info!("[AgentLoop] Context trimmed: {} → {} messages (byte budget {}KB)",
                    all.len(), kept.len(), MAX_CONTEXT_BYTES / 1024);
            }
            kept
        };

        // Call LLM
        log::info!("[AgentLoop] Loop {}: calling LLM with {} messages (SSE retry: {}/{})", loop_count, messages.len(), sse_retry_count, MAX_SSE_RETRIES);

        let mut rx = match client.chat_stream(&messages, &tools, &system_prompt).await {
            Ok(rx) => rx,
            Err(ref e) if is_llm_server_down(e) => {
                // Fatal: LLM server is unreachable — no point retrying
                let msg = format_server_down_error(e);
                log::error!("[AgentLoop] LLM server is down, aborting: {}", e);
                emit_event(&app, AgentEvent::Error { message: msg });
                break;
            }
            Err(e) => {
                if sse_retry_count < MAX_SSE_RETRIES {
                    sse_retry_count += 1;
                    log::warn!("[AgentLoop] chat_stream failed, retrying ({}/{}): {}", sse_retry_count, MAX_SSE_RETRIES, e);
                    emit_event(&app, AgentEvent::TextDelta {
                        text: format!("\n\n⚠️ Connection error, retrying ({}/{})...\n\n", sse_retry_count, MAX_SSE_RETRIES),
                    });
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                    loop_count -= 1; // Don't count retries toward tool loop limit
                    continue;
                }
                emit_event(&app, AgentEvent::Error { message: e });
                break;
            }
        };

        // Collect the response
        let mut text_accumulator = String::new();
        let mut tool_calls: Vec<ToolCall> = Vec::new();
        let mut tool_args_map: std::collections::HashMap<String, String> = std::collections::HashMap::new();
        let mut stop_reason = String::new();
        let mut had_error = false;
        let mut sse_error_msg = String::new();
        let mut just_downgraded = false;
        let mut received_any_token = false;
        let mut wait_warnings = 0u32;
        const FIRST_TOKEN_TIMEOUT_SECS: u64 = 60;
        const INTER_TOKEN_TIMEOUT_SECS: u64 = 120;
        const MAX_WAIT_WARNINGS: u32 = 2;

        loop {
            // Per-event timeout — longer after first token (thinking models can be slow)
            let timeout_secs = if received_any_token { INTER_TOKEN_TIMEOUT_SECS } else { FIRST_TOKEN_TIMEOUT_SECS };
            // Race: receive next LLM token OR user cancellation
            let recv_result = tokio::select! {
                result = tokio::time::timeout(
                    std::time::Duration::from_secs(timeout_secs),
                    rx.recv()
                ) => result,
                _ = cancel_token.cancelled() => {
                    log::info!("[AgentLoop] Cancelled during LLM stream");
                    emit_event(&app, AgentEvent::Error { message: "Cancelled by user".into() });
                    had_error = true;
                    break;
                }
            };

            match recv_result {
                // Timeout: no token received within the window
                Err(_elapsed) => {
                    wait_warnings += 1;
                    if wait_warnings <= MAX_WAIT_WARNINGS {
                        let hint = format!(
                            "\n\u{23f3} Still waiting for model response... ({}/{})\n\
                             If the local LLM is not responding, try restarting it.\n",
                            wait_warnings, MAX_WAIT_WARNINGS
                        );
                        log::warn!("[AgentLoop] No token after {}s (warning {}/{})", timeout_secs, wait_warnings, MAX_WAIT_WARNINGS);
                        emit_event(&app, AgentEvent::TextDelta { text: hint });
                        continue; // Keep waiting
                    }
                    // Max warnings reached — abort
                    let timeout_msg = if received_any_token {
                        format!("\u{26a0}\u{fe0f} Model stopped responding (no data for {}s).\nThe local LLM may have crashed. Please restart it.", INTER_TOKEN_TIMEOUT_SECS)
                    } else {
                        format!("\u{26a0}\u{fe0f} LLM did not respond within {}s.\nThe model may be overloaded or crashed. Please restart the LLM server.", FIRST_TOKEN_TIMEOUT_SECS * (MAX_WAIT_WARNINGS as u64 + 1))
                    };
                    log::error!("[AgentLoop] LLM response timed out");
                    emit_event(&app, AgentEvent::Error { message: timeout_msg });
                    had_error = true;
                    sse_error_msg = String::new(); // Non-retryable
                    break;
                }

                // Channel closed without Done event
                Ok(None) => break,

                // Got an event — process it
                Ok(Some(event)) => match event {
                    LlmEvent::TextDelta(text) => {
                        received_any_token = true;
                        wait_warnings = 0;
                        text_accumulator.push_str(&text);
                        emit_event(&app, AgentEvent::TextDelta { text });
                    }
                    LlmEvent::Thinking(text) => {
                        received_any_token = true;
                        emit_event(&app, AgentEvent::Thinking { text });
                    }
                    LlmEvent::ToolCallStart { id, name } => {
                        received_any_token = true;
                        emit_event(&app, AgentEvent::ToolCallStart { id: id.clone(), name: name.clone() });
                        emit_event(&app, AgentEvent::StateChange { state: "tool_calling".into() });
                        tool_args_map.insert(id.clone(), String::new());
                        tool_calls.push(ToolCall { id, name, arguments: String::new() });
                    }
                    LlmEvent::ToolCallDelta { id, args_chunk } => {
                        if let Some(args) = tool_args_map.get_mut(&id) {
                            args.push_str(&args_chunk);
                        }
                        emit_event(&app, AgentEvent::ToolCallArgs { id, args: args_chunk });
                    }
                    LlmEvent::ToolCallEnd { id } => {
                        if let Some(final_args) = tool_args_map.get(&id) {
                            if let Some(tc) = tool_calls.iter_mut().find(|t| t.id == id) {
                                tc.arguments = final_args.clone();
                            }
                        }
                    }
                    LlmEvent::Done { stop_reason: reason } => {
                        stop_reason = reason;
                        break;
                    }
                    LlmEvent::Error(e) => {
                        // Fatal: LLM server is down — abort immediately without retry
                        if is_llm_server_down(&e) {
                            let msg = format_server_down_error(&e);
                            log::error!("[AgentLoop] LLM server went down during stream: {}", e);
                            emit_event(&app, AgentEvent::Error { message: msg });
                            had_error = true;
                            sse_error_msg = String::new(); // Prevent retry
                            break;
                        }
                        // Protocol downgrade conditions:
                        //   1. 400 / Bad Request — Anthropic tool calling not supported
                        //   2. SSE connection error — endpoint unreachable or not Anthropic-compatible
                        let should_downgrade = !protocol_downgraded
                            && active_provider == LlmProvider::Anthropic
                            && (e.contains("400")
                                || e.contains("Bad Request")
                                || e.contains("SSE error")
                                || e.contains("error sending request"));

                        if should_downgrade {
                            if let Some(ref fallback) = openai_fallback {
                                log::warn!("[AgentLoop] Anthropic failed ({}), downgrading to OpenAI fallback", e);
                                client = fallback.clone();
                                active_provider = LlmProvider::OpenAI;
                                protocol_downgraded = true;
                                had_error = false;
                                just_downgraded = true;
                                loop_count -= 1;
                                break;
                            }
                            let user_msg = format!("{}\n\n\u{26a0}\u{fe0f} This model does not support the Anthropic protocol. Please configure an OpenAI-compatible URL in Model Nexus.", e);
                            emit_event(&app, AgentEvent::Error { message: user_msg });
                            had_error = true;
                            break;
                        }
                        // Retryable SSE errors (stream ended, decode error, timeout)
                        sse_error_msg = e;
                        had_error = true;
                        break;
                    }
                },
            }
        }

        if had_error {
            if !sse_error_msg.is_empty() && sse_retry_count < MAX_SSE_RETRIES {
                // SSE stream error — retry
                sse_retry_count += 1;
                log::warn!("[AgentLoop] SSE stream error, retrying ({}/{}): {}", sse_retry_count, MAX_SSE_RETRIES, sse_error_msg);
                emit_event(&app, AgentEvent::TextDelta {
                    text: format!("\n\n⚠️ Connection error, retrying ({}/{})...\n", sse_retry_count, MAX_SSE_RETRIES),
                });
                // If we had partial text but no tool calls, save it to avoid losing progress
                if !text_accumulator.is_empty() && tool_calls.is_empty() {
                    let mut map = session_map.lock().await;
                    let sess = map.entry(server_key.clone()).or_insert_with(AgentSession::new);
                    sess.messages.push(Message {
                        role: "assistant".into(),
                        content: MessageContent::Text(text_accumulator.clone()),
                    });
                }
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                loop_count -= 1; // Don't count retries toward tool loop limit
                continue;
            }
            // Max retries exceeded or non-retryable error
            if !sse_error_msg.is_empty() {
                let hint = format_connection_error_hint(&sse_error_msg);
                emit_event(&app, AgentEvent::Error {
                    message: format!("⚠️ Failed to connect to AI model after {} retries.\n\n💡 {}", MAX_SSE_RETRIES, hint),
                });
            }
            // Remove the user message that caused the error from history
            let mut map = session_map.lock().await;
            if let Some(sess) = map.get_mut(&server_key) {
                if let Some(last) = sess.messages.last() {
                    if last.role == "user" {
                        sess.messages.pop();
                    }
                }
            }
            break;
        }

        // If we just downgraded, retry the outer loop with the new OpenAI client
        // without saving an empty response or breaking on "no tool calls"
        if just_downgraded {
            sse_retry_count = 0;
            continue;
        }

        // Reset SSE retry counter on successful stream completion
        sse_retry_count = 0;

        // 5. Store assistant response in history
        {
            let mut map = session_map.lock().await;
            let sess = map.entry(server_key.clone()).or_insert_with(AgentSession::new);
            if tool_calls.is_empty() {
                // Pure text response
                sess.messages.push(Message {
                    role: "assistant".into(),
                    content: MessageContent::Text(text_accumulator.clone()),
                });
            } else {
                // Response with tool calls
                let mut blocks: Vec<ContentBlock> = Vec::new();
                if !text_accumulator.is_empty() {
                    blocks.push(ContentBlock::Text { text: text_accumulator.clone() });
                }
                for tc in &tool_calls {
                    let input: Value = serde_json::from_str(&tc.arguments).unwrap_or(Value::Object(Default::default()));
                    blocks.push(ContentBlock::ToolUse {
                        id: tc.id.clone(),
                        name: tc.name.clone(),
                        input,
                    });
                }
                sess.messages.push(Message {
                    role: "assistant".into(),
                    content: MessageContent::Blocks(blocks),
                });
            }
        }

        // 6. If no tool calls, we're done
        if tool_calls.is_empty() {
            log::info!("[AgentLoop] LLM finished with no tool calls (reason: {})", stop_reason);
            break;
        }

        // 7. Execute tool calls and feed results back
        log::info!("[AgentLoop] Executing {} tool calls", tool_calls.len());
        emit_event(&app, AgentEvent::StateChange { state: "executing".into() });

        // Track how many tools were saved, so we can complete the rest on cancel
        let mut completed_count = 0usize;

        for tc in &tool_calls {
            // Check cancellation before each tool
            if cancel_token.is_cancelled() {
                log::info!("[AgentLoop] Cancelled before tool: {}", tc.name);
                break;
            }

            log::info!("[AgentLoop] Executing tool: {} ({})", tc.name, tc.id);

            // Race tool execution against cancel token
            let (result, was_cancelled) = tokio::select! {
                r = agent_tools::execute_tool(&tc.name, &tc.arguments, &ssh_pool, &request.server_ids) => (r, false),
                _ = cancel_token.cancelled() => {
                    log::info!("[AgentLoop] Tool cancelled by user: {}", tc.name);
                    emit_event(&app, AgentEvent::Error { message: "Cancelled by user".into() });
                    (agent_tools::ToolResult { output: "Cancelled by user".to_string(), success: false }, true)
                }
            };

            // Always emit ToolResult to frontend
            emit_event(&app, AgentEvent::ToolResult {
                id: tc.id.clone(),
                output: result.output.clone(),
                success: result.success,
            });

            // Always save tool result to message history — even for cancelled tools.
            // This keeps the conversation structure valid (every tool_call must have a tool_result).
            {
                let mut map = session_map.lock().await;
                let sess = map.entry(server_key.clone()).or_insert_with(AgentSession::new);
                match active_provider {
                    LlmProvider::OpenAI => {
                        sess.messages.push(Message {
                            role: "tool".into(),
                            content: MessageContent::Blocks(vec![
                                ContentBlock::ToolResult {
                                    tool_use_id: tc.id.clone(),
                                    content: result.output,
                                }
                            ]),
                        });
                    }
                    LlmProvider::Anthropic => {
                        sess.messages.push(Message {
                            role: "user".into(),
                            content: MessageContent::Blocks(vec![
                                ContentBlock::ToolResult {
                                    tool_use_id: tc.id.clone(),
                                    content: result.output,
                                }
                            ]),
                        });
                    }
                }
            }
            completed_count += 1;

            if was_cancelled {
                break;
            }
        }

        // If cancelled mid-loop, add "Cancelled" results for any remaining (unexecuted) tool calls.
        // This ensures the conversation history is always valid: every tool_call has a tool_result.
        if cancel_token.is_cancelled() {
            if completed_count < tool_calls.len() {
                let mut map = session_map.lock().await;
                let sess = map.entry(server_key.clone()).or_insert_with(AgentSession::new);
                for tc in tool_calls.iter().skip(completed_count) {
                    let cancelled_result = ContentBlock::ToolResult {
                        tool_use_id: tc.id.clone(),
                        content: "Cancelled by user".to_string(),
                    };
                    match active_provider {
                        LlmProvider::OpenAI => {
                            sess.messages.push(Message {
                                role: "tool".into(),
                                content: MessageContent::Blocks(vec![cancelled_result]),
                            });
                        }
                        LlmProvider::Anthropic => {
                            sess.messages.push(Message {
                                role: "user".into(),
                                content: MessageContent::Blocks(vec![cancelled_result]),
                            });
                        }
                    }
                }
            }
            break;
        }

        // Continue loop — feed tool results back to LLM
        emit_event(&app, AgentEvent::StateChange { state: "processing".into() });
    }

    // 8. Done — save session to disk
    {
        let mut map = session_map.lock().await;
        if let Some(sess) = map.get_mut(&server_key) {
            sess.running = false;
            save_session_to_disk(&server_key, &sess.messages);
        }
    }
    emit_event(&app, AgentEvent::Done {});
    emit_event(&app, AgentEvent::StateChange { state: "idle".into() });

    Ok(())
}

// ── Helpers ──

fn emit_event(app: &AppHandle, event: AgentEvent) {
    if let Err(e) = app.emit("agent_event", &event) {
        log::error!("[AgentLoop] Failed to emit event: {}", e);
    }
}

const REMOTE_PROMPT_URL: &str = "https://echobird.ai/api/mother/system_prompt.md";

/// Fetch the latest system prompt from the remote server.
/// Falls back to a basic version if the network is unavailable.
async fn fetch_remote_prompt() -> String {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(3))
        .build()
        .unwrap_or_default();

    match client.get(REMOTE_PROMPT_URL).send().await {
        Ok(resp) if resp.status().is_success() => {
            if let Ok(text) = resp.text().await {
                if !text.trim().is_empty() {
                    log::info!("[AgentLoop] Remote prompt loaded ({} bytes)", text.len());
                    return text;
                }
            }
        }
        Ok(resp) => log::warn!("[AgentLoop] Remote prompt HTTP {}", resp.status()),
        Err(e) => log::warn!("[AgentLoop] Remote prompt fetch failed: {}", e),
    }

    // Fallback: basic instructions when offline
    log::info!("[AgentLoop] Using fallback prompt");
    String::from(
        "## Echobird Product Knowledge\n\
        After installing any agent, ALWAYS guide users through these steps:\n\
        1. **Model Nexus** — add your AI model API key here first\n\
        2. **App Manager** — find the agent and assign a model to it\n\
        3. **Channels** — chat with the agent\n\n\
        NEVER tell users to set environment variables manually.\n\
        NEVER fabricate configuration steps — use `web_fetch` to read official docs first.\n\
        OpenClaw official docs: https://docs.openclaw.ai/\n\
        OpenClaw npm package: `openclaw` (NOT `@anthropic-ai/claude-code`)\n\
        Install command: `npm install -g openclaw@latest`\n"
    )
}

async fn build_system_prompt(request: &AgentRequest, ssh_pool: &SSHPool) -> String {
    // Prepend locale hint so the agent responds in the user's preferred language
    let locale_hint = if let Some(ref locale) = request.locale {
        // Map locale code to readable language name for clarity
        let lang_name = match locale.as_str() {
            "zh" | "zh-Hans" => "Simplified Chinese (简体中文)",
            "zh-Hant" => "Traditional Chinese (繁體中文)",
            "ja" => "Japanese (日本語)",
            "ko" => "Korean (한국어)",
            "de" => "German (Deutsch)",
            "fr" => "French (Français)",
            "es" => "Spanish (Español)",
            "pt" => "Portuguese (Português)",
            "ru" => "Russian (Русский)",
            "ar" => "Arabic (العربية)",
            "en" | _ => "English",
        };
        format!("## User Language\nThe user's interface is set to **{}**. Respond in this language by default unless the user writes in a different language.\n\n", lang_name)
    } else {
        String::new()
    };

    let mut prompt = locale_hint;
    prompt.push_str(
        "You are Mother Agent, the built-in deployment assistant of Echobird \
        (known as \u{767e}\u{7075}\u{9e1f} in Simplified Chinese, \u{767e}\u{9748}\u{9ce5} in Traditional Chinese). \
        Your purpose is to help users deploy AI agents on local machines or remote servers via SSH.\n\n\
        ## Your Mission\n\
        You specialize in one-click deployment of AI agents such as:\n\
        - OpenClaw (open-source AI agent, npm package name: `openclaw`, NOT `@anthropic-ai/claude-code`)\n\
        - ZeroClaw (lightweight AI agent)\n\
        - Other open-source AI agents that can run autonomously\n\n\
        You ONLY deploy AI agents. You do NOT deploy IDEs, editors, or local tools.\n\n\
        Users should NEVER have to manually fiddle with installation steps. \
        You handle EVERYTHING automatically: detect the OS, install prerequisites \
        (Node.js, Git, Rust, Python, Docker, etc.), download the target agent, \
        configure it, and verify it works. The user just tells you WHAT agent to deploy \
        and WHERE, and you deliver a working result.\n\n\
        ## Rules\n\
        - Work autonomously. Do NOT ask the user unnecessary questions.\n\
        - Detect the OS and package manager first, then proceed.\n\
        - Always verify each step succeeded before moving to the next.\n\
        - If a command fails, diagnose and try alternative approaches automatically.\n\
        - For destructive operations, explain briefly before executing.\n\
        - Keep responses concise. Only show output when it reveals useful info.\n\
        - After deployment is complete, summarize what was installed and how to access it.\n\
        - **Windows targets**: When the user wants to install an AI agent (e.g. OpenClaw) on Windows, \
you MUST first ask the user to choose:\n\
          - **Option A: Install in WSL2** (Recommended) - better compatibility, native Linux environment.\n\
          - **Option B: Install directly on Windows** - simpler but may have compatibility issues.\n\
        Briefly explain that WSL2 is recommended because most AI agents are designed for Linux/macOS. \
Wait for the user's choice before proceeding. Do NOT install WSL2 automatically - \
many users are beginners and just want to try things out quickly.\n\
        - **CRITICAL SAFETY RULES** (NEVER violate these):\n\
          - NEVER delete, remove, or modify the `~/.echobird/` directory or anything inside it. \
            This directory contains Echobird's configuration, models, and user data.\n\
          - NEVER kill or stop the Echobird process (echobird.exe / Echobird).\n\
          - When uninstalling agents (e.g. OpenClaw), ONLY remove the agent itself \
            (e.g. `npm uninstall -g openclaw`). Do NOT touch Echobird's files.\n\
          - NEVER run commands that delete user home directories or broad recursive deletions.\n\n"
    );

    // Fetch remote prompt (product knowledge + deployment workflows)
    let remote_prompt = fetch_remote_prompt().await;
    prompt.push_str(&remote_prompt);
    prompt.push_str("\n\n");

    // Local platform info
    prompt.push_str("## Local Machine\n");
    prompt.push_str(&agent_tools::get_local_platform_info());
    prompt.push_str("\n\n");

    // SSH servers info
    if !request.server_ids.is_empty() {
        let has_remote = request.server_ids.iter().any(|s| s != "local");
        if has_remote {
            prompt.push_str("## ACTIVE TARGET SERVER (CRITICAL)\n");
            prompt.push_str("The user has selected a REMOTE server as their target. \
                You MUST execute ALL shell_exec, file_read, and file_write calls \
                with the server_id shown below. NEVER omit server_id — \
                omitting it will run commands on the LOCAL machine instead of the remote server, \
                which is WRONG and defeats the user's intent.\n\n");
            let connections = ssh_pool.lock().await;
            for sid in &request.server_ids {
                if sid == "local" { continue; }
                let status = if connections.contains_key(sid) { "connected" } else { "not connected" };
                prompt.push_str(&format!(">>> TARGET: server_id='{}' ({}) <<<\n", sid, status));
                prompt.push_str(&format!("Every tool call MUST include: \"server_id\": \"{}\"\n\n", sid));
            }
        }
    }

    // Skills
    if !request.skills.is_empty() {
        prompt.push_str("## Active Skills\n");
        for skill in &request.skills {
            prompt.push_str(&format!("- {}\n", skill));
        }
        prompt.push_str("\n");
    }

    // Agent Plugins
    let plugins_info = agent_tools::get_plugins_info();
    if !plugins_info.is_empty() {
        prompt.push_str(&plugins_info);
        prompt.push_str("\n\n");
    }

    prompt
}


// -- Connection Error Hints --

/// Translate a raw SSE/HTTP error into a short, user-friendly hint.
fn format_connection_error_hint(raw: &str) -> &'static str {
    if raw.contains("405") || raw.contains("Method Not Allowed") {
        "Please check your Base URL in Model Nexus (e.g. remove any /chat/completions suffix)."
    } else if raw.contains("401") || raw.contains("nauthorized") {
        "API Key rejected. Please verify your API Key in Model Nexus."
    } else if raw.contains("403") || raw.contains("orbidden") {
        "Access denied. Please check your API Key permissions in Model Nexus."
    } else if raw.contains("timeout") || raw.contains("timed out") {
        "Request timed out. Check your network or the model provider's status."
    } else if raw.contains("dns") || raw.contains("resolve") || raw.contains("No such host") {
        "Cannot reach model provider. Check your network or API URL."
    } else {
        "Please verify your model configuration in Model Nexus — check the API URL, API Key, and ensure your token quota is sufficient."
    }
}

// -- LLM Server Down Detection --

/// Returns true if the error indicates the LLM server is completely unreachable.
/// These are fatal connection errors -- no point retrying.
fn is_llm_server_down(err: &str) -> bool {
    let lower = err.to_lowercase();
    lower.contains("connection refused")
        || lower.contains("os error 111")    // Linux: connection refused
        || lower.contains("os error 61")     // macOS: connection refused
        || lower.contains("os error 10061")  // Windows: connection refused
        || lower.contains("no connection could be made")
        || lower.contains("failed to connect")
        || lower.contains("tcp connect error")
}

/// Build a clear, user-friendly message when the LLM server is detected as down.
fn format_server_down_error(err: &str) -> String {
    log::error!("[AgentLoop] Local LLM server is offline: {}", err);
    "⚠️ Local LLM server is offline (connection refused).\n     Please restart the local LLM server and try again.\n\n     In the sidebar: stop the LLM server, then start it again.".to_string()
}
