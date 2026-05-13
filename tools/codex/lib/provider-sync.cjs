const fs = require("fs");
const path = require("path");
const { spawn } = require("child_process");

// Provider history sync (pre-launch)
//
// Codex tags every conversation with the active `model_provider`. When the
// user switches providers via EchoBird's apply_codex, prior conversations
// stay tagged with the OLD provider and Codex Desktop / `/resume` hide them.
// The vendored codex-provider-sync CLI rewrites that metadata to the new
// provider so historical chats stay visible across switches.
//
// We run it HERE — in the launcher, BEFORE spawning Codex — because:
//   1. Codex isn't running yet, so it doesn't hold state_5.sqlite's WAL lock.
//      provider-sync's exclusive directory lock acquires cleanly.
//   2. The retag finishes before Codex starts reading session metadata.
//
// (apply_codex previously fired sync fire-and-forget too, but that path is
// racy because the user is often still using Codex when they apply a new
// model. The lock fails silently and the user sees no merged history. The
// launcher pre-step is the reliable path.)
//
// Bounded with a 10s timeout — sync is usually <2s; if it hangs we'd
// rather launch Codex with stale tags than make the user wait forever.

async function runProviderSync(providerId, launcherDir, logger) {
    const log = logger?.log || (() => {});
    const warn = logger?.warn || (() => {});

    if (!providerId) return;
    // The vendored CLI lives as a SIBLING of this launcher (both under
    // tools/codex/). In dev mode Tauri mirrors tools/ to <target>/_up_/tools/
    // — but only at startup, so a freshly-vendored subdir may not be in
    // the mirror yet. Try several candidate paths so dev workflows that
    // skip a full restart still work:
    //   1. <launcher_dir>/codex-provider-sync (production / synced dev)
    //   2. <launcher_dir>/../../codex/codex-provider-sync (defensive)
    //   3. ECHOBIRD_PROVIDER_SYNC_CLI env override (escape hatch)
    const candidates = [
        process.env.ECHOBIRD_PROVIDER_SYNC_CLI,
        path.join(launcherDir, "codex-provider-sync", "src", "cli.js"),
        path.join(launcherDir, "..", "codex", "codex-provider-sync", "src", "cli.js"),
    ].filter(Boolean);
    const cliJs = candidates.find(p => { try { return fs.existsSync(p); } catch { return false; } });
    if (!cliJs) {
        warn(`provider-sync CLI not found in any of: ${candidates.join(" | ")} — skipping history retag`);
        return;
    }
    log(`provider-sync: retag historical sessions to provider=${providerId}`);
    return new Promise((resolve) => {
        const child = spawn("node", [cliJs, "sync", "--provider", providerId, "--keep", "5"], {
            stdio: ["ignore", "pipe", "pipe"],
            env: process.env,
            windowsHide: true,
        });
        let outBuf = "";
        let errBuf = "";
        child.stdout.on("data", c => outBuf += c.toString());
        child.stderr.on("data", c => errBuf += c.toString());

        const timer = setTimeout(() => {
            if (!child.killed) {
                child.kill();
                warn(`provider-sync exceeded 10s timeout, killed`);
            }
        }, 10_000);

        child.on("close", (code) => {
            clearTimeout(timer);
            if (code === 0) {
                const tail = outBuf.trim().split("\n").slice(-5).join(" | ");
                log(`provider-sync OK: ${tail || "(no stdout)"}`);
            } else {
                warn(`provider-sync exited code=${code}; stderr=${errBuf.slice(0, 400).trim()}`);
            }
            resolve();
        });
        child.on("error", e => {
            clearTimeout(timer);
            warn(`provider-sync spawn error: ${e.message}`);
            resolve();
        });
    });
}

module.exports = { runProviderSync };
