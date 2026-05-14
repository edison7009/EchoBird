#!/usr/bin/env node
// Codex Launcher — local proxy + dual-spoofing for third-party providers.
//
// Two responsibilities, both small:
//
//   1. Local proxy + protocol/model deception. The proxy listens on the
//      fixed port CODEX_PROXY_PORT (53682), translates Codex's
//      /v1/responses to upstream /chat/completions, and rewrites the
//      model id from the canonical display ("gpt-5.4") to the user's
//      real model. It reads ~/.echobird/codex.json on every request, so
//      switching models in EchoBird takes effect without restarting
//      Codex or the launcher.
//
//   2. config.toml verification. base_url is permanently
//      "http://127.0.0.1:53682/v1", written once by apply_codex. The
//      launcher just verifies that's still the case — if not, rewrites
//      the canonical 13-line template. No per-launch base_url
//      modification, no kill-and-restart of Codex Desktop, no history
//      retag (model_provider stays "OpenAI", sessions stay in one bucket).
//
// Anything else the codebase used to do (kill_codex_if_running,
// run_codex_provider_sync, killExistingCodexDesktop, rewriteBaseUrl
// tiered cleanup, exit-time URL restore) was driven by the old
// hash-based provider id scheme, which the dual-deception design
// retired. See memory/project_model_switching_vs_proxy.md for the
// architectural split.

const { log, warn, err } = require("./lib/logger.cjs");
const { createSessionStore } = require("./lib/session-store.cjs");
const {
    loadEchobirdConfig,
    ensureCanonicalConfig,
    CODEX_DIR,
    CODEX_PROXY_PORT,
    CODEX_PROXY_URL,
    CODEX_CONFIG,
    ECHOBIRD_CONFIG,
} = require("./lib/config-manager.cjs");
const { startProxy } = require("./lib/proxy-server.cjs");
const { launchCodex } = require("./lib/codex-launcher-core.cjs");
const { resolveDesktopBinary, resolveDesktopLaunchUri } = require("./lib/binary-resolver.cjs");
const { responsesToChat } = require("./lib/protocol-converter.cjs");
const { chatToResponsesNonStream } = require("./lib/stream-handler.cjs");
const { valueToChatContent, mapContentPart } = require("./lib/content-mapper.cjs");
const { bypassOnboarding } = require("./lib/onboarding-bypass.cjs");

async function main() {
    const mode = (process.env.ECHOBIRD_CODEX_LAUNCH_MODE || "cli").toLowerCase();
    const logger = { log, warn, err };
    log(`──── launcher start, mode=${mode}, pid=${process.pid} ────`);

    if (mode === "desktop" && !resolveDesktopBinary() && !resolveDesktopLaunchUri(__dirname)) {
        err("Codex Desktop not installed (no Codex.exe at the standard paths");
        err("and no Store launchUri available either).");
        err("Install Codex Desktop from https://openai.com/codex or the Microsoft Store.");
        process.exit(1);
    }

    const config = loadEchobirdConfig();
    if (!config) {
        log(`No relay at ${ECHOBIRD_CONFIG} — launching Codex ${mode} directly (official mode)`);
        launchCodex(mode, __dirname, null, logger);
        return;
    }

    log(`relay: baseUrl=${config.baseUrl} actualModel=${config.actualModel || "(none)"}`);

    // The one EXTRA feature: skip Codex's first-run OAuth onboarding so
    // our api-key auth path works. Everything else (proxy + config check)
    // is core, not an "extra".
    try {
        bypassOnboarding(CODEX_DIR, logger);
    } catch (e) {
        warn(`Onboarding bypass failed (non-fatal): ${e.message}`);
    }

    // Verify config.toml points at our proxy port. Self-heals if missing
    // or drifted.
    const ensured = ensureCanonicalConfig(logger);
    if (ensured.wrote) {
        log(`config.toml ${ensured.reason === "missing" ? "created" : "repaired"}`);
    }

    // Start the proxy on the FIXED port. If another launcher already
    // owns it, share — its proxy serves new clients fine.
    const sessionStore = createSessionStore();
    let server = null;
    try {
        const proxy = await startProxy(sessionStore, logger);
        server = proxy.server;
    } catch (e) {
        if (e.code === "EADDRINUSE") {
            log(`Proxy port ${CODEX_PROXY_PORT} already held by another launcher — sharing it`);
        } else {
            err(`Proxy bind failed: ${e.message}`);
            process.exit(1);
        }
    }

    launchCodex(mode, __dirname, (code) => {
        if (server) server.close();
        process.exit(code);
    }, logger);
}

// Run main() when invoked as a script; export translation helpers so
// tests can exercise them in isolation without spawning Codex.
if (require.main === module) {
    main().catch(e => { err(`Fatal: ${e.stack || e}`); process.exit(1); });
} else {
    const sessionStore = createSessionStore();
    module.exports = {
        responsesToChat,
        chatToResponsesNonStream,
        startProxy: (sessions, logger) => startProxy(sessions, logger),
        ensureCanonicalConfig,
        valueToChatContent,
        mapContentPart,
        sessionStore,
        CODEX_CONFIG,
        ECHOBIRD_CONFIG,
        CODEX_PROXY_PORT,
        CODEX_PROXY_URL,
    };
}
