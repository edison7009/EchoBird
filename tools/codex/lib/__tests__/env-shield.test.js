// Unit tests for shieldOpenAIEnvVars
// Verifies the launcher overrides any inherited base-URL env vars so a
// downstream Codex process can never fall back to the real vendor URL
// even if it bypasses config.toml.

const { shieldOpenAIEnvVars } = require("../env-shield.cjs");

describe("shieldOpenAIEnvVars", () => {
    let savedBaseUrl;
    let savedApiBase;

    beforeEach(() => {
        // Snapshot whatever the test runner inherited so we can restore it.
        savedBaseUrl = process.env.OPENAI_BASE_URL;
        savedApiBase = process.env.OPENAI_API_BASE;
    });

    afterEach(() => {
        if (savedBaseUrl === undefined) delete process.env.OPENAI_BASE_URL;
        else process.env.OPENAI_BASE_URL = savedBaseUrl;
        if (savedApiBase === undefined) delete process.env.OPENAI_API_BASE;
        else process.env.OPENAI_API_BASE = savedApiBase;
    });

    test("overrides OPENAI_BASE_URL with the proxy URL", () => {
        process.env.OPENAI_BASE_URL = "https://api.xiaomimimo.com/v1";

        shieldOpenAIEnvVars("http://127.0.0.1:54321/v1");

        expect(process.env.OPENAI_BASE_URL).toBe("http://127.0.0.1:54321/v1");
    });

    test("also overrides legacy OPENAI_API_BASE convention", () => {
        process.env.OPENAI_API_BASE = "https://api.deepseek.com";

        shieldOpenAIEnvVars("http://127.0.0.1:9999/v1");

        expect(process.env.OPENAI_API_BASE).toBe("http://127.0.0.1:9999/v1");
    });

    test("sets the env vars when they were not previously defined", () => {
        delete process.env.OPENAI_BASE_URL;
        delete process.env.OPENAI_API_BASE;

        shieldOpenAIEnvVars("http://127.0.0.1:8080/v1");

        expect(process.env.OPENAI_BASE_URL).toBe("http://127.0.0.1:8080/v1");
        expect(process.env.OPENAI_API_BASE).toBe("http://127.0.0.1:8080/v1");
    });

    test("is a no-op when called with empty / null proxy URL", () => {
        process.env.OPENAI_BASE_URL = "https://api.openai.com/v1";

        shieldOpenAIEnvVars("");
        expect(process.env.OPENAI_BASE_URL).toBe("https://api.openai.com/v1");

        shieldOpenAIEnvVars(null);
        expect(process.env.OPENAI_BASE_URL).toBe("https://api.openai.com/v1");

        shieldOpenAIEnvVars(undefined);
        expect(process.env.OPENAI_BASE_URL).toBe("https://api.openai.com/v1");
    });

    test("regression: the real vendor URL never survives the call", () => {
        // The exact scenario from issue #36 — process_manager pre-seeded
        // the env var with the third-party URL; we must clobber it before
        // launching Codex.
        process.env.OPENAI_BASE_URL = "https://token-plan-cn.xiaomimimo.com/v1";

        shieldOpenAIEnvVars("http://127.0.0.1:55000/v1");

        expect(process.env.OPENAI_BASE_URL).not.toContain("xiaomimimo");
        expect(process.env.OPENAI_BASE_URL.startsWith("http://127.0.0.1:")).toBe(true);
    });
});
