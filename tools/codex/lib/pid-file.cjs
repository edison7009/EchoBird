const fs = require("fs");
const path = require("path");
const os = require("os");

// PID file lives next to the relay config under ~/.echobird/. Used by the
// Tauri startup-cleanup path to identify and kill our own orphaned
// launcher processes WITHOUT relying on name-pattern matches like
// `pkill -f codex-launcher.cjs` (which would kill another EchoBird
// instance's launcher and ride roughshod over multi-instance setups).
//
// File contents:
//   { "pid": 12345, "startedAt": "2026-05-14T07:55:00Z", "version": "4.6.2" }
//
// Lifecycle:
//   - Launcher writes the file on startup (after the proxy binds successfully).
//   - Launcher deletes the file in its exit callback.
//   - Tauri startup reads the file: if the PID is alive, kill it; then delete.
//
// We do NOT use this for cross-process locking — the launcher is allowed
// to start while another launcher exists (e.g. two EchoBird instances).
// Tauri startup cleanup runs once per EchoBird launch and only acts on
// PIDs *its own* prior session wrote (since each EchoBird process is the
// one writing/clearing this file).
const RELAY_DIR = process.env.ECHOBIRD_RELAY_DIR || path.join(os.homedir(), ".echobird");
const PID_FILE = path.join(RELAY_DIR, "codex-launcher.pid");

function writePidFile(pid, version) {
    try {
        fs.mkdirSync(RELAY_DIR, { recursive: true });
        const payload = JSON.stringify({
            pid,
            startedAt: new Date().toISOString(),
            version: version || "unknown",
        });
        // Atomic: write to .tmp, then rename. Avoids half-written PID
        // file racing against Tauri's startup read.
        const tmp = `${PID_FILE}.tmp`;
        fs.writeFileSync(tmp, payload, "utf-8");
        fs.renameSync(tmp, PID_FILE);
        return true;
    } catch {
        return false;
    }
}

function deletePidFile() {
    try { fs.unlinkSync(PID_FILE); } catch { /* missing is fine */ }
}

function readPidFile() {
    try {
        const raw = fs.readFileSync(PID_FILE, "utf-8");
        const obj = JSON.parse(raw);
        if (typeof obj.pid === "number" && obj.pid > 0) return obj;
        return null;
    } catch {
        return null;
    }
}

module.exports = { writePidFile, deletePidFile, readPidFile, PID_FILE };
