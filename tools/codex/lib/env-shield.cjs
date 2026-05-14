// Env-var shield for the Codex child process.
//
// process_manager.rs sets OPENAI_BASE_URL to the *real* third-party
// vendor URL on the launcher process (it's a backstop for the
// OpenAI-direct case where there's no proxy). When the launcher then
// spawns Codex with `env: process.env`, that env var is inherited.
//
// In normal operation Codex reads config.toml's model_providers.<id>.base_url
// — which we've rewritten to 127.0.0.1 — and ignores the env var. But if a
// future Codex version, a default-provider fallback, or a user-edited
// config ever causes Codex to consult OPENAI_BASE_URL, it would see the
// raw vendor URL and bypass the proxy. We never want that to happen.
//
// Defense-in-depth: before launching Codex, point ANY env var that
// might supply a base URL at the local proxy. That way even if Codex
// reads it, it still gets routed through us.
function shieldOpenAIEnvVars(proxyUrl) {
    if (!proxyUrl) return;
    // OpenAI's current SDK convention.
    process.env.OPENAI_BASE_URL = proxyUrl;
    // Legacy/older SDK convention some clients still respect.
    process.env.OPENAI_API_BASE = proxyUrl;
}

module.exports = { shieldOpenAIEnvVars };
