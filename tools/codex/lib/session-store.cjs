// SessionStore (in-memory)
//
// Ported from MetaFARS/codex-relay's session.rs. Three maps, all
// process-local (the launcher lives only as long as the Codex session
// it spawned, so no need for persistence):
//
//   responseHistory  : response_id  → ChatMessage[]
//       Codex uses `previous_response_id` to continue a conversation;
//       we replay the stored messages so each Chat Completions call is
//       self-contained even when Codex doesn't redundantly send the
//       full input.
//
//   reasoning        : call_id      → reasoning_content (string)
//       For thinking models (DeepSeek-V4-*, Kimi-K2.6, etc.), the
//       upstream returns reasoning_content alongside tool_calls. We
//       save it keyed by the call_id so when Codex replays the same
//       function_call in a subsequent request, we can attach the
//       saved reasoning_content to the assistant message — providers
//       require this round-trip or the next turn degrades.
//
//   turnReasoning    : fingerprint(content) → reasoning_content
//       For pure-text assistant turns (no tool_calls), there's no
//       call_id to key on. We hash the assistant content and use that
//       as the lookup key, so Codex replaying the assistant turn as a
//       message item still recovers the reasoning_content.
//
// Hash uses Node's built-in `crypto` via a tiny stable string hash —
// no external deps. Collisions are not a correctness issue (just
// missed lookups), so a 64-bit FNV-1a is plenty.

function createSessionStore() {
    const responseHistory = new Map();
    const reasoning = new Map();
    const turnReasoning = new Map();

    // 64-bit FNV-1a, returned as hex string for Map keying.
    const fnv1a = (s) => {
        let h1 = 0xcbf29ce4 >>> 0;
        let h2 = 0x84222325 >>> 0;
        for (let i = 0; i < s.length; i++) {
            const c = s.charCodeAt(i);
            h1 ^= c & 0xff;
            h2 ^= (c >>> 8) & 0xff;
            // Multiply by FNV prime 0x100000001b3 (mod 2^64), split into halves
            h1 = Math.imul(h1, 0x1b3) >>> 0;
            h2 = Math.imul(h2, 0x1b3) >>> 0;
        }
        return h1.toString(16).padStart(8, "0") + h2.toString(16).padStart(8, "0");
    };

    const contentToString = (content) => {
        if (typeof content === "string") return content;
        if (Array.isArray(content)) {
            return content
                .map(p => (p && typeof p.text === "string") ? p.text : "")
                .join("");
        }
        return "";
    };

    return {
        storeReasoning(callId, text) {
            if (callId && text) reasoning.set(callId, text);
        },
        getReasoning(callId) {
            return reasoning.get(callId) || null;
        },
        storeTurnReasoning(assistantContent, text) {
            const key = contentToString(assistantContent);
            if (key && text) turnReasoning.set(fnv1a(key), text);
        },
        getTurnReasoning(assistantContent) {
            const key = contentToString(assistantContent);
            if (!key) return null;
            return turnReasoning.get(fnv1a(key)) || null;
        },
        saveHistory(responseId, messages) {
            if (responseId && Array.isArray(messages)) {
                responseHistory.set(responseId, messages);
            }
        },
        getHistory(responseId) {
            return responseHistory.get(responseId) || [];
        },
        newResponseId() {
            return "resp_" + Math.random().toString(36).slice(2, 14);
        },
    };
}

module.exports = { createSessionStore };
