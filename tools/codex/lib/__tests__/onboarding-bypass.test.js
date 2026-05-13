// Unit tests for onboarding bypass functionality

const fs = require("fs");
const path = require("path");
const os = require("os");
const { bypassOnboarding, isOnboardingComplete, GLOBAL_STATE_FILE } = require("../onboarding-bypass.cjs");

describe("onboarding-bypass", () => {
  let testDir;
  let globalStatePath;

  beforeEach(() => {
    // Create a temporary test directory
    testDir = path.join(os.tmpdir(), `codex-test-${Date.now()}`);
    fs.mkdirSync(testDir, { recursive: true });
    globalStatePath = path.join(testDir, GLOBAL_STATE_FILE);
  });

  afterEach(() => {
    // Clean up test directory
    if (fs.existsSync(testDir)) {
      fs.rmSync(testDir, { recursive: true, force: true });
    }
  });

  describe("bypassOnboarding", () => {
    test("creates new global state file if none exists", () => {
      const result = bypassOnboarding(testDir);

      expect(result).toBe(true);
      expect(fs.existsSync(globalStatePath)).toBe(true);

      const state = JSON.parse(fs.readFileSync(globalStatePath, "utf-8"));
      expect(state["electron-persisted-atom-state"]).toBeDefined();
    });

    test("sets all required onboarding flags", () => {
      bypassOnboarding(testDir);

      const state = JSON.parse(fs.readFileSync(globalStatePath, "utf-8"));
      const atomState = state["electron-persisted-atom-state"];

      expect(atomState["electron:onboarding-override"]).toBe("auto");
      expect(atomState["electron:onboarding-welcome-pending"]).toBe(false);
      expect(atomState["electron:onboarding-projectless-completed"]).toBe(true);
      expect(atomState["skip-full-access-confirm"]).toBe(true);
      expect(atomState["last_completed_onboarding"]).toBeDefined();
      expect(typeof atomState["last_completed_onboarding"]).toBe("number");
    });

    test("sets default agent mode to full-access", () => {
      bypassOnboarding(testDir);

      const state = JSON.parse(fs.readFileSync(globalStatePath, "utf-8"));
      const atomState = state["electron-persisted-atom-state"];

      expect(atomState["agent-mode-by-host-id"]).toBeDefined();
      expect(atomState["agent-mode-by-host-id"]["local"]).toBe("full-access");
    });

    test("preserves existing state when patching", () => {
      // Create initial state with some existing data
      const initialState = {
        "some-other-key": "preserved-value",
        "electron-persisted-atom-state": {
          "existing-setting": "should-remain",
        },
      };
      fs.writeFileSync(globalStatePath, JSON.stringify(initialState), "utf-8");

      bypassOnboarding(testDir);

      const state = JSON.parse(fs.readFileSync(globalStatePath, "utf-8"));
      expect(state["some-other-key"]).toBe("preserved-value");
      expect(state["electron-persisted-atom-state"]["existing-setting"]).toBe("should-remain");
    });

    test("creates backup before modifying existing file", () => {
      // Create initial state
      const initialState = { "electron-persisted-atom-state": {} };
      fs.writeFileSync(globalStatePath, JSON.stringify(initialState), "utf-8");

      bypassOnboarding(testDir);

      const backupPath = globalStatePath + ".bak";
      expect(fs.existsSync(backupPath)).toBe(true);

      const backup = JSON.parse(fs.readFileSync(backupPath, "utf-8"));
      expect(backup).toEqual(initialState);
    });

    test("returns true when no changes needed", () => {
      // First call patches the file
      const result1 = bypassOnboarding(testDir);
      expect(result1).toBe(true);

      // Second call should detect no changes needed
      const result2 = bypassOnboarding(testDir);
      expect(result2).toBe(true);
    });

    test("handles malformed JSON gracefully", () => {
      // Write invalid JSON
      fs.writeFileSync(globalStatePath, "{ invalid json", "utf-8");

      // Should create new state instead of crashing
      const result = bypassOnboarding(testDir);
      expect(result).toBe(true);

      const state = JSON.parse(fs.readFileSync(globalStatePath, "utf-8"));
      expect(state["electron-persisted-atom-state"]).toBeDefined();
    });

    test("uses custom logger if provided", () => {
      const logs = [];
      const logger = {
        log: (msg) => logs.push(msg),
        warn: (msg) => logs.push(`WARN: ${msg}`),
        err: (msg) => logs.push(`ERROR: ${msg}`),
      };

      bypassOnboarding(testDir, logger);

      expect(logs.length).toBeGreaterThan(0);
      expect(logs.some(log => log.includes("Codex onboarding bypass"))).toBe(true);
    });
  });

  describe("isOnboardingComplete", () => {
    test("returns false when global state file does not exist", () => {
      const result = isOnboardingComplete(testDir);
      expect(result).toBe(false);
    });

    test("returns false when onboarding flags are not set", () => {
      const state = {
        "electron-persisted-atom-state": {
          "some-other-setting": true,
        },
      };
      fs.writeFileSync(globalStatePath, JSON.stringify(state), "utf-8");

      const result = isOnboardingComplete(testDir);
      expect(result).toBe(false);
    });

    test("returns true when onboarding is complete", () => {
      bypassOnboarding(testDir);

      const result = isOnboardingComplete(testDir);
      expect(result).toBe(true);
    });

    test("returns false when only partial flags are set", () => {
      const state = {
        "electron-persisted-atom-state": {
          "electron:onboarding-projectless-completed": true,
          // Missing last_completed_onboarding
        },
      };
      fs.writeFileSync(globalStatePath, JSON.stringify(state), "utf-8");

      const result = isOnboardingComplete(testDir);
      expect(result).toBe(false);
    });

    test("handles malformed JSON gracefully", () => {
      fs.writeFileSync(globalStatePath, "{ invalid json", "utf-8");

      const result = isOnboardingComplete(testDir);
      expect(result).toBe(false);
    });
  });

  describe("integration", () => {
    test("bypass and check work together", () => {
      // Initially not complete
      expect(isOnboardingComplete(testDir)).toBe(false);

      // Apply bypass
      bypassOnboarding(testDir);

      // Now should be complete
      expect(isOnboardingComplete(testDir)).toBe(true);
    });

    test("multiple bypass calls are idempotent", () => {
      bypassOnboarding(testDir);
      const state1 = fs.readFileSync(globalStatePath, "utf-8");

      bypassOnboarding(testDir);
      const state2 = fs.readFileSync(globalStatePath, "utf-8");

      // State should be identical (no timestamp changes)
      expect(state1).toBe(state2);
    });
  });
});
