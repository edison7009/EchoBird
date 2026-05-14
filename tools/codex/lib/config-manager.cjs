const fs = require("fs");
const path = require("path");
const os = require("os");

// Paths are derived from $HOME by default. ECHOBIRD_CODEX_CONFIG_DIR override
// exists for smoke tests so we don't touch the user's real Codex config.
const CODEX_DIR = process.env.ECHOBIRD_CODEX_CONFIG_DIR || path.join(os.homedir(), ".codex");
const RELAY_DIR = process.env.ECHOBIRD_RELAY_DIR || path.join(os.homedir(), ".echobird");
const CODEX_CONFIG = path.join(CODEX_DIR, "config.toml");
const ECHOBIRD_CONFIG = path.join(RELAY_DIR, "codex.json");

// Stable port for the local proxy. The same port is referenced by
// CODEX_PROXY_PORT in src-tauri/src/services/tool_config_manager.rs —
// keep them in sync. config.toml's base_url is permanently
// "http://127.0.0.1:53682/v1", so the launcher never has to rewrite it
// across model switches.
const CODEX_PROXY_PORT = 53682;
const CODEX_PROXY_URL = `http://127.0.0.1:${CODEX_PROXY_PORT}/v1`;

// Canonical 13-line config.toml shape. Must match the template written
// by apply_codex in src-tauri/src/services/tool_config_manager.rs —
// keep them aligned. The launcher writes this defensively if the file
// is missing or its base_url doesn't point at our proxy port (e.g. the
// user deleted ~/.codex/ or an older build wrote something different).
const CANONICAL_CONFIG = `model_provider = "OpenAI"
model = "gpt-5.4"
review_model = "gpt-5.4"
model_reasoning_effort = "xhigh"
disable_response_storage = true
network_access = "enabled"
model_context_window = 1000000
model_auto_compact_token_limit = 900000

[model_providers.OpenAI]
name = "OpenAI"
base_url = "${CODEX_PROXY_URL}"
wire_api = "responses"
requires_openai_auth = true
`;

// Verify config.toml points Codex at our proxy. If missing or drifted,
// rewrite it to the canonical 13-line template. Idempotent: cheap when
// already correct, self-healing when not.
function ensureCanonicalConfig(logger) {
    const log = logger?.log || (() => {});
    let current;
    try {
        current = fs.readFileSync(CODEX_CONFIG, "utf-8");
    } catch {
        log(`config.toml not found at ${CODEX_CONFIG} — writing canonical template`);
        try { fs.mkdirSync(CODEX_DIR, { recursive: true }); } catch { /* ignore */ }
        fs.writeFileSync(CODEX_CONFIG, CANONICAL_CONFIG);
        return { wrote: true, reason: "missing" };
    }
    // Quick check: does the file mention our exact proxy URL? If yes,
    // it's compatible (Codex will hit our proxy). If not, replace the
    // whole file — we don't merge, we own this shape end-to-end.
    if (!current.includes(CODEX_PROXY_URL)) {
        log(`config.toml base_url does not point at ${CODEX_PROXY_URL} — rewriting to canonical`);
        fs.writeFileSync(CODEX_CONFIG, CANONICAL_CONFIG);
        return { wrote: true, reason: "drifted" };
    }
    return { wrote: false, reason: "already-canonical" };
}

function loadEchobirdConfig() {
    try { return JSON.parse(fs.readFileSync(ECHOBIRD_CONFIG, "utf-8")); }
    catch { return null; }
}

// Read the relay file fresh. Called by the proxy on EVERY incoming
// request so model switches take effect without restarting Codex or
// the launcher: EchoBird's apply_codex only rewrites this JSON, and the
// next request the proxy sees uses the new model / key / upstream URL.
// Returns null if the file is missing or malformed — caller should
// respond with a clear error to Codex.
function readEchobirdRelay() {
    return loadEchobirdConfig();
}

function isOpenAI(url) { return !!url && url.includes("api.openai.com"); }

module.exports = {
    loadEchobirdConfig,
    readEchobirdRelay,
    isOpenAI,
    ensureCanonicalConfig,
    CODEX_CONFIG,
    ECHOBIRD_CONFIG,
    CODEX_DIR,
    RELAY_DIR,
    CODEX_PROXY_PORT,
    CODEX_PROXY_URL,
    CANONICAL_CONFIG,
};
