// Phase 1 — axum HTTP server skeleton.
//
// Today this serves placeholder 501 Not Implemented for /v1/responses
// while we port the translator logic from .cjs over the coming days.
// The legacy Node launcher (tools/codex/lib/proxy-server.cjs) keeps
// handling real Codex traffic in v4.6.8 — we won't switch over until
// Phase 6 wires this up and Phase 7 removes the .cjs path entirely.

use axum::{
    http::StatusCode,
    response::{IntoResponse, Json},
    routing::post,
    Router,
};
use serde_json::json;
use std::net::SocketAddr;

pub async fn run(port: u16) -> Result<(), String> {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));

    let app = Router::new()
        .route("/v1/responses", post(handle_responses))
        .route("/responses", post(handle_responses));

    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(l) => l,
        Err(e) => {
            // Most common cause: the legacy Node launcher (or another
            // EchoBird instance) still holds the port. In v5.0+ this is
            // a hard error worth surfacing; for now we log + bail so
            // Tauri keeps starting.
            return Err(format!("bind 127.0.0.1:{port} failed: {e}"));
        }
    };

    log::info!("[CodexProxy] listening on 127.0.0.1:{port} (Phase 1 skeleton)");
    axum::serve(listener, app)
        .await
        .map_err(|e| format!("serve failed: {e}"))
}

// Placeholder until Phase 2/3 wire in the real translator.
async fn handle_responses() -> impl IntoResponse {
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(json!({
            "error": {
                "message": "Rust codex_proxy is under construction; the Node launcher at tools/codex/codex-launcher.cjs is still the live proxy in v4.6.x.",
                "code": "rust_proxy_phase_1"
            }
        })),
    )
}
