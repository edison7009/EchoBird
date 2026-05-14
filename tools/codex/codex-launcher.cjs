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

const path = require("path");
const { log, warn, err } = require("./lib/logger.cjs");
const { createSessionStore } = require("./lib/session-store.cjs");
const {
    loadEchobirdConfig,
    ensureCanonicalConfig,
    CODEX_DIR,
    CODEX_PROXY_PORT,
    CODEX_PROXY_URL,
} = require("./lib/config-manager.cjs");
const { startProxy } = require("./lib/proxy-server.cjs");
const { launchCodex } = require("./lib/codex-launcher-core.cjs");
const { resolveDesktopBinary, resolveDesktopLaunchUri } = require("./lib/binary-resolver.cjs");
const { responsesToChat } = require("./lib/protocol-converter.cjs");
const { chatToResponsesNonStream } = require("./lib/stream-handler.cjs");
const { valueToChatContent, mapContentPart } = require("./lib/content-mapper.cjs");
const { bypassOnboarding } = require("./lib/onboarding-bypass.cjs");
const { CODEX_CONFIG, ECHOBIRD_CONFIG } = require("./lib/config-manager.cjs");
const { writePidFile, updatePidFile, deletePidFile } = require("./lib/pid-file.cjs");
const { shieldOpenAIEnvVars } = require("./lib/env-shield.cjs");

// Read launcher version from package.json so the PID file's `version`
// field is accurate without hard-coding it here.
function getLauncherVersion() {
    try {
        const pkg = require(path.join(__dirname, "..", "..", "package.json"));
        return pkg.version || "unknown";
    } catch { return "unknown"; }
}

async function main() {
    const mode = (process.env.ECHOBIRD_CODEX_LAUNCH_MODE || "cli").toLowerCase();
    const logger = { log, warn, err };
    log(`──── launcher start, mode=${mode}, pid=${process.pid} ────`);

    // Desktop mode requires either a direct Codex.exe / Codex.app path
    // OR a Store launchUri (UWP). Abort early if neither is available.
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

    log(`relay: baseUrl=${config.baseUrl} actualModel=${config.actualModel || "(none)"} provider=${config.providerId || "(none)"}`);

    // Third-party providers need the onboarding bypass so Codex Desktop
    // doesn't try to run its OAuth sign-in flow against our 127.0.0.1
    // proxy on first launch.
    try {
        bypassOnboarding(CODEX_DIR, logger);
    } catch (e) {
        warn(`Onboarding bypass failed (non-fatal): ${e.message}`);
    }

    // Verify config.toml has the canonical shape pointing at our proxy.
    // Self-healing: writes the 13-line template if missing or drifted.
    // apply_codex (Rust) writes the same template on every model switch,
    // so in steady state this is a no-op file read.
    const ensured = ensureCanonicalConfig(logger);
    if (ensured.wrote) {
        log(`config.toml ${ensured.reason === "missing" ? "created" : "repaired"}`);
    }

    // Start the proxy on the FIXED port. If another launcher already
    // owns 53682 (e.g. user clicked "Open Codex" while Codex CLI is
    // running in another terminal), share it — that launcher's proxy
    // serves all clients fine. We still spawn Codex so the user's click
    // does something visible.
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

    // Defense-in-depth env vars. config.toml's base_url is already our
    // proxy, but if Codex ever falls back to OPENAI_BASE_URL we want it
    // to land on the same address rather than leaking to vendor URLs.
    if (config.apiKey) process.env.OPENAI_API_KEY = config.apiKey;
    shieldOpenAIEnvVars(CODEX_PROXY_URL);

    // Write PID file only when WE own the proxy port — that's the
    // resource Tauri's startup-cleanup needs to reclaim if EchoBird
    // died abnormally. The shared-proxy fallback above leaves PID
    // management to the launcher that actually bound 53682.
    if (server) {
        writePidFile(process.pid, getLauncherVersion());
        process.on("exit", () => { deletePidFile(); });
    }

    launchCodex(mode, __dirname, (code) => {
        if (server) {
            server.close();
            deletePidFile();
        }
        process.exit(code);
    }, logger, (codexPid) => {
        if (server) {
            updatePidFile({ codexPid });
            log(`Codex child spawned, pid=${codexPid}, recorded in PID file`);
        }
    });
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
