// Codex Responses↔Chat proxy — the full stack that replaces the
// v4.6.x Node launcher (tools/codex/lib/*.cjs, removed in v5.0).
//
// Architecture
//
// Tauri's main process spawns a background tokio task at startup that
// binds 127.0.0.1:CODEX_PROXY_PORT (53682) and serves POST /v1/responses
// (and /responses) by translating Codex's Responses API request body
// into OpenAI Chat Completions format, forwarding to the user's chosen
// upstream provider, then translating the response back to Responses
// API SSE events.
//
// Why the port from Node:
//
//   1. End users don't need Node.js installed locally — everything
//      ships compiled inside the Tauri binary.
//   2. The translation dictionary (the trickiest part of integrating
//      Codex with non-OpenAI providers) compiles to machine code
//      instead of shipping as readable .cjs, raising the reverse-
//      engineering bar significantly.
//   3. Tokio + axum + reqwest end-to-end means SSE forwarding has
//      less buffering than the Node version (no shim layer between
//      sockets).
//
// Module layout
//
//   mod.rs                ← this file: public entry, task spawn
//   server.rs             ← axum router + /v1/responses handler
//   protocol_converter.rs ← Responses input items → Chat messages
//   stream_handler.rs     ← Chat SSE ↔ Responses SSE state machine
//   session_store.rs      ← response_id history + reasoning cache
//   content_mapper.rs     ← text/image multimodal content parts
//   config_manager.rs     ← ~/.codex/config.toml + ~/.echobird/codex.json
//   onboarding_bypass.rs  ← ~/.codex/.codex-global-state.json patch
//   codex_binary.rs       ← Codex CLI + Desktop binary path discovery

mod codex_binary;
mod config_manager;
mod content_mapper;
mod onboarding_bypass;
mod protocol_converter;
mod server;
mod session_store;
mod stream_handler;

// Re-export the Codex spawn helpers so `process_manager.rs` can call
// into them directly. These cover everything the old `node codex-
// launcher.cjs` invocation used to do (config self-heal, onboarding
// patch, binary discovery) — entirely in-process now.
pub use codex_binary::{
    resolve_codex_cli_binary, resolve_codex_cli_shim, resolve_desktop_binary,
    resolve_desktop_launch_uri,
};
pub use config_manager::{default_codex_dir, ensure_canonical_config, CODEX_CONFIG_FILENAME};
pub use onboarding_bypass::bypass_onboarding;

#[cfg(test)]
pub use protocol_converter::responses_to_chat;
#[cfg(test)]
pub use session_store::SessionStore;
#[cfg(test)]
pub use stream_handler::{
    chat_error_to_responses_error, chat_to_responses_non_stream, chat_usage_to_responses_usage,
    SseEvent, StreamState,
};

/// Fixed proxy port. `tool_config_manager` imports this for the
/// canonical `~/.codex/config.toml` template, so there's exactly one
/// source of truth.
pub const CODEX_PROXY_PORT: u16 = 53682;

/// Spawn the proxy as a background task on Tauri's async runtime.
/// Called from Tauri's setup() (which is sync), returns immediately.
/// On bind failure we log and continue — EchoBird's other features
/// keep working even if the proxy port is taken by another EchoBird
/// instance still running.
pub fn spawn_proxy_task() {
    tauri::async_runtime::spawn(async move {
        match server::run(CODEX_PROXY_PORT).await {
            Ok(()) => log::info!("[CodexProxy] server task exited cleanly"),
            Err(e) => log::error!("[CodexProxy] server task failed: {e}"),
        }
    });
}
