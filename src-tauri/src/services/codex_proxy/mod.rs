// Codex Responses↔Chat proxy — Rust port of tools/codex/lib/*.cjs.
//
// Phase 2-5 status note: the translator submodules (`protocol_converter`,
// `content_mapper`, `session_store`, eventually `stream_handler`) compile
// in isolation and are exercised by their own unit tests, but the live
// HTTP handler in `server.rs` is still the Phase 1 placeholder — it
// returns 501 NotImplemented and doesn't yet call the translator. That
// means everything in the submodules is "dead code" from the linker's
// point of view until Phase 6 wires the handler. We blanket-allow
// dead_code module-wide while the rewrite is mid-flight; the allow gets
// removed in Phase 7 when the whole pipeline is integrated.
#![allow(dead_code)]

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
// Why we ported from Node:
//
//   1. End users don't need Node.js installed locally — everything ships
//      compiled inside the Tauri binary.
//   2. The translation dictionary (the trickiest part of integrating
//      Codex with non-OpenAI providers) gets compiled to machine code
//      instead of shipping as readable .cjs files, raising the bar for
//      reverse engineering significantly.
//   3. Tokio + axum + reqwest end-to-end means SSE forwarding has less
//      buffering than the Node version (no shim layer between sockets).
//
// Module layout (work in progress, Phase 1 = server skeleton only):
//
//   mod.rs                ← this file: public entry, task spawn
//   server.rs             ← axum router + handler entry point
//   protocol_converter.rs ← Phase 2: Responses input items → Chat messages
//   stream_handler.rs     ← Phase 3: Chat SSE ↔ Responses SSE
//   session_store.rs      ← Phase 4: response_id history + reasoning cache
//   content_mapper.rs     ← Phase 5: text/image multimodal parts
//   onboarding_bypass.rs  ← Phase 5: ~/.codex/.codex-global-state.json patch

mod config_manager;
mod content_mapper;
mod onboarding_bypass;
mod protocol_converter;
mod server;
mod session_store;
mod stream_handler;

#[cfg(test)]
pub use protocol_converter::responses_to_chat;
#[cfg(test)]
pub use session_store::SessionStore;
#[cfg(test)]
pub use stream_handler::{
    chat_error_to_responses_error, chat_to_responses_non_stream, chat_usage_to_responses_usage,
    SseEvent, StreamState,
};

/// Fixed port. Kept in sync with `CODEX_PROXY_PORT` in
/// `tool_config_manager.rs` and `config-manager.cjs` (still used by the
/// legacy .cjs launcher until Phase 7 cleanup).
pub const CODEX_PROXY_PORT: u16 = 53682;

/// Spawn the proxy as a background task on Tauri's async runtime.
/// Called from Tauri's setup() (which is sync), returns immediately.
/// On bind failure we log and continue — EchoBird's other features
/// keep working even if the proxy port is taken by another EchoBird
/// instance or a leftover Node launcher.
pub fn spawn_proxy_task() {
    tauri::async_runtime::spawn(async move {
        match server::run(CODEX_PROXY_PORT).await {
            Ok(()) => log::info!("[CodexProxy] server task exited cleanly"),
            Err(e) => log::error!("[CodexProxy] server task failed: {e}"),
        }
    });
}
