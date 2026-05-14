const fs = require("fs");
const path = require("path");
const os = require("os");
const { spawn } = require("child_process");
const { resolveCodexBinary, resolveDesktopBinary, resolveDesktopLaunchUri } = require("./binary-resolver.cjs");

// Block until either: Codex.exe appears and then disappears (normal exit),
// or we've waited the full deadline without ever seeing it. Used for the
// launchUri path where we don't own a child process.
async function waitForCodexProcessLifecycle(logger) {
    const log = logger?.log || (() => {});
    const warn = logger?.warn || (() => {});

    const isRunning = () => {
        try {
            const { execFileSync } = require("child_process");
            const out = execFileSync("tasklist", ["/FI", "IMAGENAME eq Codex.exe", "/FO", "CSV", "/NH"],
                { encoding: "utf-8", timeout: 3000, stdio: ["ignore", "pipe", "ignore"] });
            return out.toLowerCase().includes("codex.exe");
        } catch { return false; }
    };
    const sleep = ms => new Promise(r => setTimeout(r, ms));

    // Phase 1: wait up to 30s for Codex.exe to appear.
    const startupDeadline = Date.now() + 30_000;
    while (Date.now() < startupDeadline) {
        if (isRunning()) break;
        await sleep(1000);
    }
    if (!isRunning()) {
        warn("Codex Desktop process never appeared after 30s; tearing down proxy anyway.");
        return;
    }
    log("Codex Desktop process detected; watching for exit.");

    // Phase 2: poll until codex.exe disappears for 2 consecutive checks.
    let absent = 0;
    while (absent < 2) {
        await sleep(3000);
        if (isRunning()) absent = 0;
        else absent++;
    }
}

// Resolve the right binary based on launch mode, then spawn it. Both
// CLI and Desktop go through the same "wait for child to exit" path so
// the proxy lifetime matches the Codex session — when the user closes
// Codex, the launcher tears down the proxy and restores config.toml.
//
// `onSpawn(childPid)` is invoked after the Codex child is spawned (only
// for the direct-spawn paths, not the desktop-via-URI path where the
// child PID is fire-and-forget through cmd.exe). Callers use it to
// record the PID for cross-process cleanup (so Tauri can kill OUR
// Codex on exit instead of taskkill /IM-ing every Codex on the system).
function launchCodex(mode, launcherDir, onExit, logger, onSpawn) {
    const log = logger?.log || (() => {});
    const err = logger?.err || (() => {});

    let codexPath;
    let useShell = false;
    let stdio = "inherit"; // CLI needs an attached TTY; Desktop doesn't care.
    let desktopViaUri = false;

    if (mode === "desktop") {
        codexPath = resolveDesktopBinary();
        if (!codexPath) {
            // Direct exe not found — try the Store launchUri. Common on
            // Microsoft Store installs whose binary lives under a locked
            // WindowsApps directory we can't always resolve.
            const uri = resolveDesktopLaunchUri(launcherDir);
            if (uri) {
                log(`Direct Codex.exe not found; launching via Store URI: ${uri}`);
                desktopViaUri = true;
                codexPath = "cmd";
                useShell = false;
            } else {
                err("Codex Desktop not found in standard install locations.");
                err("Install Codex Desktop from https://openai.com/codex or the Microsoft Store first.");
                process.exit(1);
            }
        } else {
            log(`Launching Codex Desktop: ${codexPath}`);
        }
        // Desktop is a GUI app: detach stdio so the launcher doesn't keep
        // a console window pinned to it.
        stdio = "ignore";
    } else {
        codexPath = resolveCodexBinary();
        if (codexPath) {
            log(`Launching Codex CLI (direct binary): ${codexPath}`);
        } else {
            const codexCmd = process.platform === "win32" ? "codex.cmd" : "codex";
            if (process.platform === "win32") {
                const appdata = process.env.APPDATA || process.env.LOCALAPPDATA || "";
                if (appdata.length > 2) {
                    const candidate = path.join(appdata, "npm", codexCmd);
                    if (fs.existsSync(candidate)) codexPath = candidate;
                }
            } else {
                const candidate = "/usr/local/bin/" + codexCmd;
                if (fs.existsSync(candidate)) codexPath = candidate;
            }
            if (!codexPath) {
                try {
                    const { execFileSync } = require("child_process");
                    const findCmd = process.platform === "win32" ? "where" : "which";
                    const r = execFileSync(findCmd, [codexCmd], {
                        encoding: "utf8", timeout: 3000,
                        stdio: ["ignore", "pipe", "ignore"],
                    }).trim().split(/\r?\n/)[0].trim();
                    if (r && fs.existsSync(r)) codexPath = r;
                } catch { /* not found */ }
            }
            if (!codexPath) codexPath = codexCmd;
            useShell = true;
            log(`Rust binary not found, falling back to shim: ${codexPath}`);
        }
    }

    if (desktopViaUri) {
        // Fire-and-forget via cmd.exe: `start "" "shell:AppsFolder\..."`
        // hands the URI to the Shell which dispatches to the Store-app
        // activation pipeline. We don't get a child process back, so we
        // run a background poller that watches for Codex.exe to appear
        // and then disappear, then triggers onExit.
        const uri = resolveDesktopLaunchUri(launcherDir);
        try {
            const launcher = spawn("cmd", ["/C", "start", "", uri], {
                stdio: "ignore",
                env: process.env,
                cwd: os.homedir(),
                detached: true,
            });
            launcher.unref();
        } catch (e) {
            err(`Failed to invoke Store launch URI: ${e.message}`);
            process.exit(1);
        }
        waitForCodexProcessLifecycle(logger).then(() => {
            if (onExit) onExit(0);
            else process.exit(0);
        });
        return;
    }

    const child = spawn(codexPath, [], {
        stdio,
        env: process.env,
        cwd: os.homedir(),
        shell: useShell,
    });
    if (onSpawn && child.pid) {
        try { onSpawn(child.pid); }
        catch (e) { err(`onSpawn callback threw: ${e.message}`); }
    }
    process.on("SIGINT",  () => child.kill("SIGINT"));
    process.on("SIGTERM", () => child.kill("SIGTERM"));
    // SIGHUP fires on POSIX when the controlling terminal closes (e.g. user
    // closes the terminal tab while Codex CLI is running). Without this
    // handler the launcher dies before onExit runs, leaving config.toml
    // pointing at the proxy and the proxy port leaked.
    if (process.platform !== "win32") {
        process.on("SIGHUP", () => child.kill("SIGHUP"));
    }
    child.on("close", (code) => {
        try {
            if (onExit) onExit(code || 0);
            else process.exit(code || 0);
        } catch (e) {
            // Never let an onExit exception swallow the exit — without
            // this the process can hang holding the proxy port open.
            err(`onExit handler threw: ${e.stack || e.message}`);
            process.exit(code || 1);
        }
    });
    child.on("error", (e) => {
        err(`Failed to launch Codex: ${e.message}`);
        process.exit(1);
    });
}

module.exports = { launchCodex, waitForCodexProcessLifecycle };
