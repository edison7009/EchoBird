// Unit tests for sessionStore
// These tests verify the session history and reasoning storage logic

const { createSessionStore } = require("../session-store.cjs");

describe("sessionStore", () => {
  let store;

  beforeEach(() => {
    store = createSessionStore();
  });

  describe("reasoning storage", () => {
    test("stores and retrieves reasoning by call_id", () => {
      store.storeReasoning("call_123", "Let me think about this...");

      const result = store.getReasoning("call_123");

      expect(result).toBe("Let me think about this...");
    });

    test("returns null for non-existent call_id", () => {
      const result = store.getReasoning("call_nonexistent");

      expect(result).toBeNull();
    });

    test("does not store reasoning with empty call_id", () => {
      store.storeReasoning("", "Some reasoning");

      const result = store.getReasoning("");

      expect(result).toBeNull();
    });

    test("does not store reasoning with empty text", () => {
      store.storeReasoning("call_123", "");

      const result = store.getReasoning("call_123");

      expect(result).toBeNull();
    });

    test("overwrites existing reasoning for same call_id", () => {
      store.storeReasoning("call_123", "First reasoning");
      store.storeReasoning("call_123", "Second reasoning");

      const result = store.getReasoning("call_123");

      expect(result).toBe("Second reasoning");
    });
  });

  describe("turn reasoning storage", () => {
    test("stores and retrieves reasoning by content fingerprint", () => {
      const content = "The answer is 42.";
      store.storeTurnReasoning(content, "Let me calculate...");

      const result = store.getTurnReasoning(content);

      expect(result).toBe("Let me calculate...");
    });

    test("handles array content with text parts", () => {
      const content = [
        { type: "text", text: "Hello " },
        { type: "text", text: "world" },
      ];
      store.storeTurnReasoning(content, "Greeting reasoning");

      const result = store.getTurnReasoning(content);

      expect(result).toBe("Greeting reasoning");
    });

    test("returns null for empty content", () => {
      const result = store.getTurnReasoning("");

      expect(result).toBeNull();
    });

    test("returns null for non-existent content", () => {
      const result = store.getTurnReasoning("Never stored this");

      expect(result).toBeNull();
    });

    test("uses content fingerprint for lookup", () => {
      const content1 = "Same text";
      const content2 = "Same text";
      store.storeTurnReasoning(content1, "Reasoning for this text");

      const result = store.getTurnReasoning(content2);

      expect(result).toBe("Reasoning for this text");
    });

    test("different content produces different fingerprints", () => {
      store.storeTurnReasoning("Content A", "Reasoning A");
      store.storeTurnReasoning("Content B", "Reasoning B");

      expect(store.getTurnReasoning("Content A")).toBe("Reasoning A");
      expect(store.getTurnReasoning("Content B")).toBe("Reasoning B");
    });
  });

  describe("history storage", () => {
    test("stores and retrieves message history", () => {
      const messages = [
        { role: "user", content: "Hello" },
        { role: "assistant", content: "Hi there!" },
      ];
      store.saveHistory("resp_abc", messages);

      const result = store.getHistory("resp_abc");

      expect(result).toEqual(messages);
    });

    test("returns empty array for non-existent response_id", () => {
      const result = store.getHistory("resp_nonexistent");

      expect(result).toEqual([]);
    });

    test("does not store history with empty response_id", () => {
      const messages = [{ role: "user", content: "Hello" }];
      store.saveHistory("", messages);

      const result = store.getHistory("");

      expect(result).toEqual([]);
    });

    test("does not store non-array messages", () => {
      store.saveHistory("resp_abc", "not an array");

      const result = store.getHistory("resp_abc");

      expect(result).toEqual([]);
    });

    test("overwrites existing history for same response_id", () => {
      const messages1 = [{ role: "user", content: "First" }];
      const messages2 = [{ role: "user", content: "Second" }];

      store.saveHistory("resp_abc", messages1);
      store.saveHistory("resp_abc", messages2);

      const result = store.getHistory("resp_abc");

      expect(result).toEqual(messages2);
    });

    test("stores complex message structures", () => {
      const messages = [
        { role: "user", content: "What's the weather?" },
        {
          role: "assistant",
          content: null,
          tool_calls: [
            {
              id: "call_123",
              type: "function",
              function: { name: "get_weather", arguments: '{"location":"Tokyo"}' },
            },
          ],
          reasoning_content: "I need to check the weather API",
        },
        {
          role: "tool",
          tool_call_id: "call_123",
          content: '{"temperature":20,"condition":"sunny"}',
        },
        { role: "assistant", content: "It's 20°C and sunny in Tokyo." },
      ];

      store.saveHistory("resp_complex", messages);

      const result = store.getHistory("resp_complex");

      expect(result).toEqual(messages);
    });
  });

  describe("response ID generation", () => {
    test("generates response IDs with correct prefix", () => {
      const id = store.newResponseId();

      expect(id).toMatch(/^resp_[a-z0-9]+$/);
    });

    test("generates unique IDs", () => {
      const id1 = store.newResponseId();
      const id2 = store.newResponseId();

      expect(id1).not.toBe(id2);
    });

    test("generates IDs of reasonable length", () => {
      const id = store.newResponseId();

      expect(id.length).toBeGreaterThan(10);
      expect(id.length).toBeLessThan(25);
    });
  });

  describe("integration scenarios", () => {
    test("handles multi-turn conversation with reasoning", () => {
      // Turn 1: User asks, assistant responds with tool call
      const turn1Messages = [
        { role: "user", content: "What's 2+2?" },
        {
          role: "assistant",
          content: null,
          tool_calls: [
            {
              id: "call_calc",
              type: "function",
              function: { name: "calculate", arguments: '{"expr":"2+2"}' },
            },
          ],
          reasoning_content: "I'll use the calculator",
        },
      ];
      store.storeReasoning("call_calc", "I'll use the calculator");
      store.saveHistory("resp_turn1", turn1Messages);

      // Turn 2: Tool result, assistant final answer
      const turn2Messages = [
        ...turn1Messages,
        { role: "tool", tool_call_id: "call_calc", content: "4" },
        { role: "assistant", content: "The answer is 4." },
      ];
      store.storeTurnReasoning("The answer is 4.", "Simple arithmetic");
      store.saveHistory("resp_turn2", turn2Messages);

      // Verify we can retrieve everything
      expect(store.getReasoning("call_calc")).toBe("I'll use the calculator");
      expect(store.getTurnReasoning("The answer is 4.")).toBe("Simple arithmetic");
      expect(store.getHistory("resp_turn1")).toHaveLength(2);
      expect(store.getHistory("resp_turn2")).toHaveLength(4);
    });

    test("handles conversation continuation via previous_response_id", () => {
      // Initial conversation
      const initialMessages = [
        { role: "user", content: "Hello" },
        { role: "assistant", content: "Hi! How can I help?" },
      ];
      store.saveHistory("resp_001", initialMessages);

      // Continuation (simulated by retrieving and extending)
      const previousMessages = store.getHistory("resp_001");
      const continuedMessages = [
        ...previousMessages,
        { role: "user", content: "Tell me a joke" },
        { role: "assistant", content: "Why did the chicken cross the road?" },
      ];
      store.saveHistory("resp_002", continuedMessages);

      const result = store.getHistory("resp_002");

      expect(result).toHaveLength(4);
      expect(result[0].content).toBe("Hello");
      expect(result[3].content).toBe("Why did the chicken cross the road?");
    });
  });
});
