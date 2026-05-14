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

    const { apiKey, baseUrl, modelId, providerId, displayModel } = config;
    const envKey = config.envKey || "OPENAI_API_KEY";
    log(`relay: baseUrl=${baseUrl} model=${modelId || "(default)"} displayModel=${displayModel || "(same)"} provider=${providerId || "(none)"} envKey=${envKey}`);

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
    const { port, server } = await startProxy(baseUrl, apiKey, modelId, displayModel, sessionStore, logger);
    const localUrl = `http://127.0.0.1:${port}/v1`;

    const rewriteResult = rewriteBaseUrl(providerId, baseUrl, localUrl, logger);
    if (!rewriteResult.ok) {
        err("config.toml base_url was NOT rewritten — Codex will bypass the proxy and hit the upstream directly.");
        err(`Check ${CODEX_CONFIG} — expected to find a base_url line we could replace.`);
    }

    if (apiKey) process.env[envKey] = apiKey;

    launchCodex(mode, __dirname, (code) => {
        if (rewriteResult.ok) rewriteBaseUrl(providerId, localUrl, baseUrl, logger);
        server.close();
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
        startProxy: (baseUrl, apiKey, sessions, logger) => startProxy(baseUrl, apiKey, null, null, sessions, logger),
        rewriteBaseUrl,
        valueToChatContent,
        mapContentPart,
        sessionStore,
        CODEX_CONFIG,
        ECHOBIRD_CONFIG,
    };
}
