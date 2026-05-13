const fs = require("fs");
const path = require("path");
const os = require("os");

// Paths are derived from $HOME by default. ECHOBIRD_RELAY_DIR override
// exists for smoke tests so we don't touch the user's real config.
const RELAY_DIR = process.env.ECHOBIRD_RELAY_DIR || path.join(os.homedir(), ".echobird");
const LAUNCHER_LOG = path.join(RELAY_DIR, "codex-launcher.log");

// Production launches run inside a hidden cmd window on Windows, so
// console.log output is invisible. Mirror everything into a persistent
// log so we can ask the user to "cat ~/.echobird/codex-launcher.log"
// when something goes wrong. The launcher runs briefly per session,
// so an append-only file with timestamps stays useful even after
// the next launch.
//
// ECHOBIRD_LAUNCHER_QUIET=1 suppresses console output (CLI mode only).
// Desktop mode is always quiet since it has no visible terminal.
// Errors are always shown regardless of quiet mode.
function logLine(level, msg) {
    const ts = new Date().toISOString();
    const line = `${ts} [${level}] ${msg}\n`;
    try {
        fs.mkdirSync(path.dirname(LAUNCHER_LOG), { recursive: true });
        fs.appendFileSync(LAUNCHER_LOG, line, "utf-8");
    } catch { /* log-of-the-log is pointless */ }

    const quiet = process.env.ECHOBIRD_LAUNCHER_QUIET === "1";
    if (level === "ERROR") {
        console.error(`[Echobird] ${msg}`);
    } else if (!quiet) {
        if (level === "WARN") console.warn(`[Echobird] ${msg}`);
        else console.log(`[Echobird] ${msg}`);
    }
}

const log  = (msg) => logLine("INFO",  msg);
const warn = (msg) => logLine("WARN",  msg);
const err  = (msg) => logLine("ERROR", msg);

module.exports = { logLine, log, warn, err, LAUNCHER_LOG };
