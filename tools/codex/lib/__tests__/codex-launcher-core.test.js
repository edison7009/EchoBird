// Sanity tests for codex-launcher-core exports.
// The launch logic itself depends on real OS process management and
// can't be fully unit-tested without a process mock framework. These
// tests just verify the module surface is intact.

const core = require("../codex-launcher-core.cjs");

describe("codex-launcher-core exports", () => {
    test("exports launchCodex", () => {
        expect(typeof core.launchCodex).toBe("function");
    });

    test("exports waitForCodexProcessLifecycle", () => {
        expect(typeof core.waitForCodexProcessLifecycle).toBe("function");
    });
});
