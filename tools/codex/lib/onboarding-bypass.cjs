const fs = require("fs");
const path = require("path");

// Auto-skip Codex onboarding/login by patching .codex-global-state.json
//
// Codex checks several flags in its global state to determine if the user
// needs to go through the onboarding flow. By setting these flags, we can
// skip the login screen and go straight to the main interface.
//
// This is particularly useful for:
// - First-time Codex launches via EchoBird
// - After Codex updates that reset onboarding state
// - Users who want to bypass the welcome screens

const GLOBAL_STATE_FILE = ".codex-global-state.json";

/**
 * Patches the Codex global state file to skip onboarding/login.
 *
 * @param {string} codexDir - Path to .codex directory (usually ~/.codex)
 * @param {object} logger - Optional logger with log/warn/err methods
 * @returns {boolean} - true if patched successfully, false otherwise
 */
function bypassOnboarding(codexDir, logger) {
    const log = logger?.log || (() => {});
    const warn = logger?.warn || (() => {});
    const err = logger?.err || (() => {});

    const globalStatePath = path.join(codexDir, GLOBAL_STATE_FILE);

    // Read existing state or create new one
    let state = {};
    try {
        if (fs.existsSync(globalStatePath)) {
            const content = fs.readFileSync(globalStatePath, "utf-8");
            state = JSON.parse(content);
            log("Loaded existing .codex-global-state.json");
        } else {
            log("Creating new .codex-global-state.json");
        }
    } catch (e) {
        warn(`Failed to read global state: ${e.message}`);
        // Continue with empty state
    }

    // Ensure electron-persisted-atom-state exists
    if (!state["electron-persisted-atom-state"]) {
        state["electron-persisted-atom-state"] = {};
    }

    const atomState = state["electron-persisted-atom-state"];
    let modified = false;

    // Key flags to skip onboarding:
    // 1. electron:onboarding-override: "auto" - skip onboarding checks
    // 2. electron:onboarding-welcome-pending: false - no welcome screen
    // 3. electron:onboarding-projectless-completed: true - projectless mode done
    // 4. last_completed_onboarding: timestamp - mark as completed
    // 5. skip-full-access-confirm: true - skip permission confirmation

    const patches = {
        "electron:onboarding-override": "auto",
        "electron:onboarding-welcome-pending": false,
        "electron:onboarding-projectless-completed": true,
        "skip-full-access-confirm": true,
    };

    for (const [key, value] of Object.entries(patches)) {
        if (atomState[key] !== value) {
            atomState[key] = value;
            modified = true;
            log(`Set ${key} = ${JSON.stringify(value)}`);
        }
    }

    // Set last_completed_onboarding timestamp if not present
    if (!atomState["last_completed_onboarding"]) {
        atomState["last_completed_onboarding"] = Date.now();
        modified = true;
        log("Set last_completed_onboarding timestamp");
    }

    // Set default agent mode to full-access for local host
    if (!atomState["agent-mode-by-host-id"]) {
        atomState["agent-mode-by-host-id"] = {};
    }
    if (!atomState["agent-mode-by-host-id"]["local"]) {
        atomState["agent-mode-by-host-id"]["local"] = "full-access";
        modified = true;
        log("Set default agent mode to full-access");
    }

    // Write back if modified
    if (modified) {
        const tmp = globalStatePath + ".tmp";
        try {
            // Create backup before modifying. Backup mirrors the file as
            // it currently exists on disk — if anything goes wrong we
            // still have the pre-bypass state.
            if (fs.existsSync(globalStatePath)) {
                const backupPath = globalStatePath + ".bak";
                fs.copyFileSync(globalStatePath, backupPath);
                log(`Created backup: ${backupPath}`);
            }

            // Atomic write: write the new payload to a sibling .tmp file
            // and rename it onto the target. This guarantees the global
            // state file is either fully old or fully new — never half-
            // written — even if the launcher is SIGKILL'd mid-write.
            // Without this, a crash between writeFileSync's open() and
            // its final flush could leave a truncated JSON that prevents
            // Codex from starting at all.
            fs.writeFileSync(tmp, JSON.stringify(state, null, 2), "utf-8");
            fs.renameSync(tmp, globalStatePath);
            log("✓ Codex onboarding bypass applied successfully");
            return true;
        } catch (e) {
            err(`Failed to write global state: ${e.message}`);
            // Best-effort cleanup of the half-written tmp file.
            try { fs.unlinkSync(tmp); } catch { /* ignore */ }
            return false;
        }
    } else {
        log("Onboarding already bypassed, no changes needed");
        return true;
    }
}

/**
 * Check if onboarding has been completed.
 *
 * @param {string} codexDir - Path to .codex directory
 * @returns {boolean} - true if onboarding is complete
 */
function isOnboardingComplete(codexDir) {
    const globalStatePath = path.join(codexDir, GLOBAL_STATE_FILE);

    try {
        if (!fs.existsSync(globalStatePath)) {
            return false;
        }

        const content = fs.readFileSync(globalStatePath, "utf-8");
        const state = JSON.parse(content);
        const atomState = state["electron-persisted-atom-state"] || {};

        return (
            atomState["electron:onboarding-projectless-completed"] === true &&
            atomState["last_completed_onboarding"] != null
        );
    } catch {
        return false;
    }
}

module.exports = {
    bypassOnboarding,
    isOnboardingComplete,
    GLOBAL_STATE_FILE,
};
