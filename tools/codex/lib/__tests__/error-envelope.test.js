// Unit tests for chatErrorToResponsesError
// Verifies that upstream /chat/completions error responses are translated
// into the /responses error envelope Codex expects to render.

const { chatErrorToResponsesError } = require("../stream-handler.cjs");
const { createSessionStore } = require("../session-store.cjs");

describe("chatErrorToResponsesError", () => {
    let store;

    beforeEach(() => {
        store = createSessionStore();
    });

    test("translates OpenAI-shape error envelope", () => {
        const upstreamBody = JSON.stringify({
            error: {
                message: "Invalid API key",
                code: "invalid_api_key",
                type: "authentication_error",
            },
        });

        const env = chatErrorToResponsesError(401, upstreamBody, store);

        expect(env.status).toBe("failed");
        expect(env.error.message).toBe("Invalid API key");
        expect(env.error.code).toBe("invalid_api_key");
        expect(env.output).toEqual([]);
        expect(env.id).toMatch(/^resp_/);
    });

    test("falls back to upstream status when body is empty", () => {
        const env = chatErrorToResponsesError(500, "", store);

        expect(env.status).toBe("failed");
        expect(env.error.message).toBe("Upstream returned 500");
        expect(env.error.code).toBe("upstream_500");
    });

    test("uses .type when .code is missing", () => {
        const upstreamBody = JSON.stringify({
            error: { message: "Rate limited", type: "rate_limit_exceeded" },
        });

        const env = chatErrorToResponsesError(429, upstreamBody, store);

        expect(env.error.code).toBe("rate_limit_exceeded");
        expect(env.error.message).toBe("Rate limited");
    });

    test("handles flat error shape (no nested error object)", () => {
        const upstreamBody = JSON.stringify({
            message: "Service unavailable",
            code: "service_down",
        });

        const env = chatErrorToResponsesError(503, upstreamBody, store);

        expect(env.error.message).toBe("Service unavailable");
        expect(env.error.code).toBe("service_down");
    });

    test("handles .detail field (some providers use this instead of .message)", () => {
        const upstreamBody = JSON.stringify({
            detail: "Model not found in your subscription",
        });

        const env = chatErrorToResponsesError(404, upstreamBody, store);

        expect(env.error.message).toBe("Model not found in your subscription");
    });

    test("surfaces raw text when body is not JSON (truncated to 500 chars)", () => {
        const longText = "x".repeat(1000);
        const env = chatErrorToResponsesError(500, longText, store);

        expect(env.error.message.length).toBeLessThanOrEqual(500);
        expect(env.error.message).toBe("x".repeat(500));
    });

    test("works without a sessions store (generates fallback id)", () => {
        const env = chatErrorToResponsesError(500, "", null);

        expect(env.id).toMatch(/^resp_err_/);
        expect(env.status).toBe("failed");
    });
});
