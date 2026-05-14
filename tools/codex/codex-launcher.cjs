#!/usr/bin/env node
// Codex Launcher — Dual-spoofing proxy that bridges Codex's Responses API
// to third-party Chat-only endpoints.
//
// Codex v0.107+ only emits POST /v1/responses; DeepSeek / Moonshot / Qwen /
// OpenRouter / etc. only accept POST /v1/chat/completions. There is no
// config-file path that bridges this gap, so we run an http server on a
// random 127.0.0.1 port and rewrite ~/.codex/config.toml to point Codex at
// it. The proxy:
//   • translates Responses → Chat Completions outbound
//   • translates Chat-Completions stream → Responses-shaped SSE inbound
//   • restores the original base_url in config.toml on exit
//
// Restored 2026-05-11. The bug that killed v4.0.2 — second tool-call turn
// returning 400 "请求失败,请重试" — was caused by `responsesToChat`
// silently dropping `function_call` and `function_call_output` items, which
// emptied the messages array on every follow-up turn. That is now fixed,
// along with SSE flush (setNoDelay + flushHeaders) so deltas reach Codex
// immediately instead of being held by Nagle's algorithm.

const path = require("path");
const { log, warn, err } = require("./lib/logger.cjs");
const { createSessionStore } = require("./lib/session-store.cjs");
const { loadEchobirdConfig, isOpenAI, rewriteBaseUrl, CODEX_DIR } = require("./lib/config-manager.cjs");
const { startProxy } = require("./lib/proxy-server.cjs");
const { launchCodex } = require("./lib/codex-launcher-core.cjs");
const { runProviderSync } = require("./lib/provider-sync.cjs");
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

// Main entry point
async function main() {
    // ECHOBIRD_CODEX_LAUNCH_MODE is set by start_codex_launcher in
    // process_manager.rs. "cli" (default) or "desktop". Restore-to-official
    // doesn't need any special handling here — when the user resets via UI,
    // ~/.echobird/codex.json is deleted, loadEchobirdConfig() returns null,
    // and we fall straight through to a direct launch without a proxy.
    //
    // Provider-sync (history retag) is NOT run here — it moved back to
    // apply_codex in tool_config_manager.rs, which runs synchronously on
    // every model switch regardless of how the user later starts Codex
    // (our "Open" button, desktop shortcut, Start menu, etc.). Running
    // it here too would double the work.
    const mode = (process.env.ECHOBIRD_CODEX_LAUNCH_MODE || "cli").toLowerCase();
    const logger = { log, warn, err };
    log(`──── launcher start, mode=${mode}, pid=${process.pid} ────`);

    // For desktop mode we need EITHER a direct Codex.exe / Codex.app path
    // (preferred — child process tracking works) OR a Store launchUri
    // (fallback — fire-and-forget + tasklist polling). Only abort if
    // neither is available, which means Codex Desktop simply isn't
    // installed.
    if (mode === "desktop" && !resolveDesktopBinary() && !resolveDesktopLaunchUri(__dirname)) {
        err("Codex Desktop not installed (no Codex.exe at the standard");
        err("paths and no Store launchUri available either).");
        err("Install Codex Desktop from https://openai.com/codex or the Microsoft Store.");
        process.exit(1);
    }

    const config = loadEchobirdConfig();
    if (!config) {
        log(`No relay config at ${ECHOBIRD_CONFIG} — launching Codex ${mode} directly (official mode)`);
        log("Skipping onboarding bypass — user should authenticate with official Codex");
        launchCodex(mode, __dirname, null, logger);
        return;
    }

    const { apiKey, baseUrl, displayModel, actualModel, providerId } = config;
    const envKey = config.envKey || "OPENAI_API_KEY";
    log(`relay: baseUrl=${baseUrl} displayModel=${displayModel || "(none)"} actualModel=${actualModel || "(none)"} provider=${providerId || "(none)"} envKey=${envKey}`);

    // Retag historical sessions to the active provider BEFORE Codex starts —
    // this is what actually makes "switch model and still see old chats" work.
    // Awaited so the retag finishes before Codex opens state_5.sqlite.
    await runProviderSync(providerId, __dirname, logger);

    if (isOpenAI(baseUrl)) {
        log(`OpenAI endpoint detected — no proxy needed (${mode})`);
        log("Skipping onboarding bypass — user should authenticate with official OpenAI");
        if (apiKey) process.env[envKey] = apiKey;
        launchCodex(mode, __dirname, null, logger);
        return;
    }

    // Only bypass onboarding for third-party providers (DeepSeek, Moonshot, etc.)
    // Official Codex/OpenAI should use normal authentication flow.
    log(`Third-party provider detected — applying onboarding bypass`);
    try {
        bypassOnboarding(CODEX_DIR, logger);
    } catch (e) {
        warn(`Onboarding bypass failed (non-fatal): ${e.message}`);
    }

    log(`${mode} mode, third-party endpoint: ${baseUrl}`);

    const sessionStore = createSessionStore();
    const { port, server } = await startProxy(baseUrl, apiKey, actualModel, displayModel, sessionStore, logger);
    const localUrl = `http://127.0.0.1:${port}/v1`;

    const rewriteResult = rewriteBaseUrl(providerId, baseUrl, localUrl, logger);
    if (!rewriteResult.ok) {
        err("FATAL: config.toml base_url rewrite failed.");
        err(`Cannot start Codex — it would bypass the proxy and send /responses`);
        err(`requests directly to ${baseUrl}, which only supports /chat/completions.`);
        err(`Check ${CODEX_CONFIG} for unexpected format.`);
        server.close();
        process.exit(1);
    }

    if (apiKey) process.env[envKey] = apiKey;

    // Defense-in-depth: process_manager.rs pre-seeded OPENAI_BASE_URL with
    // the real vendor URL. Override it here so that even if Codex falls
    // back to the env var (it shouldn't — config.toml wins — but a future
    // version or odd config might), it still hits our proxy.
    shieldOpenAIEnvVars(localUrl);

    // Write PID file ONLY when the proxy is up — that's the resource we
    // need Tauri's startup-cleanup to reclaim if EchoBird died abnormally.
    // OpenAI-direct and no-relay-config paths don't bind a port, so they
    // skip this entirely (their orphaned launcher would be harmless).
    writePidFile(process.pid, getLauncherVersion());

    // Safety net: if the process dies before reaching launchCodex's exit
    // callback (e.g. uncaught exception during runProviderSync teardown),
    // still try to clean up the PID file. process.on("exit") is sync-only
    // — anything async (config restore, server.close) belongs in the
    // launchCodex callback below.
    process.on("exit", () => { deletePidFile(); });

    launchCodex(mode, __dirname, (code) => {
        // Deliberately do NOT restore base_url to the real third-party URL
        // here. If we did, the next time the user opens Codex Desktop or CLI
        // from outside EchoBird (Start menu, taskbar, `codex` in terminal),
        // it would read config.toml with base_url=https://api.deepseek.com
        // (or other third-party host) + wire_api="responses" and POST
        // /responses directly to the vendor, which returns 404 because
        // third parties only support /chat/completions.
        //
        // Leaving the stale 127.0.0.1:<port> in config.toml means:
        //   • Direct Codex launches fail fast with ECONNREFUSED on
        //     localhost — clearly localized to the proxy being down,
        //     instead of a misleading "DeepSeek 404".
        //   • Next launcher run, rewriteBaseUrl's Tier 0 cleanup swaps
        //     the stale port for the fresh proxy port (see
        //     config-manager.cjs Tier 0 block).
        //   • apply_codex still works correctly: it writes the real URL
        //     unconditionally, which the launcher overwrites with the
        //     proxy URL on its next start.
        //   • restore_codex_to_official removes the whole echobird_
        //     section, so it's not affected either.
        server.close();
        deletePidFile();
        process.exit(code);
    }, logger, (codexPid) => {
        // Record the spawned Codex PID so Tauri's exit-cleanup can kill
        // ONLY our Codex on shutdown — never a Codex that the user
        // launched independently from the terminal or Start menu.
        updatePidFile({ codexPid });
        log(`Codex child spawned, pid=${codexPid}, recorded in PID file`);
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
        startProxy: (baseUrl, apiKey, sessions, logger) => startProxy(baseUrl, apiKey, null, null, sessions, logger),
        rewriteBaseUrl,
        valueToChatContent,
        mapContentPart,
        sessionStore,
        CODEX_CONFIG,
        ECHOBIRD_CONFIG,
    };
}
