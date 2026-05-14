// Sanity tests for codex-launcher-core exports.
// The kill / launch logic itself depends on real OS process management
// and can't be fully unit-tested without a process mock framework. These
// tests just verify the module surface is intact and the kill helper
// doesn't throw when invoked on a machine with no running Codex.

const core = require("../codex-launcher-core.cjs");

describe("codex-launcher-core exports", () => {
    test("exports launchCodex", () => {
        expect(typeof core.launchCodex).toBe("function");
    });

    test("exports waitForCodexProcessLifecycle", () => {
        expect(typeof core.waitForCodexProcessLifecycle).toBe("function");
    });

    test("exports killExistingCodexDesktop", () => {
        expect(typeof core.killExistingCodexDesktop).toBe("function");
    });
});

describe("killExistingCodexDesktop", () => {
    test("does not throw when called with no Codex Desktop running", () => {
        // CI runners and developer machines almost never have Codex Desktop
        // installed/running. The function must handle that gracefully.
        const logger = { log: () => {}, warn: () => {}, err: () => {} };
        expect(() => core.killExistingCodexDesktop(logger)).not.toThrow();
    });

    test("works without a logger argument", () => {
        // Defensive: caller may pass undefined/null logger.
        expect(() => core.killExistingCodexDesktop()).not.toThrow();
        expect(() => core.killExistingCodexDesktop(null)).not.toThrow();
    });

    test("returns a boolean", () => {
        const result = core.killExistingCodexDesktop({ log: () => {}, warn: () => {} });
        expect(typeof result).toBe("boolean");
    });
});
