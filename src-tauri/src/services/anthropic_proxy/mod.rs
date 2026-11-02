// Anthropic Messages API proxy for Claude Desktop AND Claude Code 3P mode.
//
// Claude Desktop's `inferenceGatewayBaseUrl` (and Claude Code's
// `ANTHROPIC_BASE_URL`) point at this proxy. Every incoming POST is rewritten
// with the relay's real model id
// and forwarded to the user-selected upstream provider's /v1/messages.
//
// Two routes, two relay files, so the two Claude apps stay independent:
//   • POST /v1/messages            → ~/.echobird/claudedesktop.json
//   • POST /claudecode/v1/messages → ~/.echobird/claudecode.json
//
// Both sides speak the same Anthropic Messages API. The only transformation is
// model-id substitution: Claude Desktop hard-codes "claude-sonnet-4-*"
// / "claude-opus-*" / "claude-haiku-4-*" as the request model field,
// but strict upstreams (Xiaomi MiMo, etc.) reject those names and
// require their own real id (e.g. "mimo-v2.5-pro"). Smart upstreams
// (DeepSeek /anthropic, GLM /anthropic) auto-route claude-* to their
// own model so the rewrite is a no-op for them.
//
// Relay file: ~/.echobird/claudedesktop.json, written by
// tool_config_manager::apply_claudedesktop on every model switch. The
// proxy reads it fresh on every request so model switches need no
// proxy restart.

pub use messages_handler::{
    handle_messages, handle_messages_claudecode, handle_messages_claudescience,
    handle_models_claudescience,
};
pub use server::AppState;

pub(crate) mod billing_line;
mod messages_handler;
mod server;

pub const ANTHROPIC_PROXY_PORT: u16 = 53682;
static PORT: std::sync::atomic::AtomicU16 = std::sync::atomic::AtomicU16::new(ANTHROPIC_PROXY_PORT);

pub fn port() -> u16 {
    PORT.load(std::sync::atomic::Ordering::Relaxed)
}

pub fn spawn_proxy_task() {
    let listener = match super::local_proxy::bind("anthropic-proxy", ANTHROPIC_PROXY_PORT) {
        Ok(listener) => listener,
        Err(error) => {
            log::error!("[AnthropicProxy] {error}");
            return;
        }
    };
    PORT.store(
        listener.local_addr().expect("bound listener").port(),
        std::sync::atomic::Ordering::Relaxed,
    );
    tauri::async_runtime::spawn(async move {
        match server::run(listener).await {
            Ok(()) => log::info!("[AnthropicProxy] server task exited cleanly"),
            Err(e) => log::error!("[AnthropicProxy] server task failed: {e}"),
        }
    });
}
