// Unit tests for responsesToChat function
// These tests verify the Responses API → Chat Completions translation logic

const { createSessionStore } = require("../session-store.cjs");
const { responsesToChat } = require("../protocol-converter.cjs");
const { valueToChatContent, mapContentPart } = require("../content-mapper.cjs");

describe("responsesToChat", () => {
  let store;

  beforeEach(() => {
    store = createSessionStore();
  });

  describe("simple text input", () => {
    test("converts string input to user message", () => {
      const body = {
        model: "deepseek-chat",
        input: "Hello, how are you?",
        stream: true,
      };

      const result = responsesToChat(body, store);

      expect(result.model).toBe("deepseek-chat");
      expect(result.stream).toBe(true);
      expect(result.messages).toEqual([
        { role: "user", content: "Hello, how are you?" },
      ]);
    });

    test("prepends instructions as system message", () => {
      const body = {
        model: "deepseek-chat",
        instructions: "You are a helpful assistant.",
        input: "What is 2+2?",
        stream: true,
      };

      const result = responsesToChat(body, store);

      expect(result.messages).toEqual([
        { role: "system", content: "You are a helpful assistant." },
        { role: "user", content: "What is 2+2?" },
      ]);
    });

    test("does not duplicate system message if already present", () => {
      store.saveHistory("resp_abc", [
        { role: "system", content: "You are a helpful assistant." },
        { role: "user", content: "Hello" },
      ]);

      const body = {
        model: "deepseek-chat",
        previous_response_id: "resp_abc",
        instructions: "You are a helpful assistant.",
        input: "Tell me more",
        stream: true,
      };

      const result = responsesToChat(body, store);

      expect(result.messages.filter(m => m.role === "system")).toHaveLength(1);
    });
  });

  describe("message items", () => {
    test("converts message items to chat messages", () => {
      const body = {
        model: "deepseek-chat",
        input: [
          { type: "message", role: "user", content: "Hello" },
          { type: "message", role: "assistant", content: "Hi there!" },
          { type: "message", role: "user", content: "How are you?" },
        ],
        stream: true,
      };

      const result = responsesToChat(body, store);

      expect(result.messages).toEqual([
        { role: "user", content: "Hello" },
        { role: "assistant", content: "Hi there!" },
        { role: "user", content: "How are you?" },
      ]);
    });

    test("converts developer role to system", () => {
      const body = {
        model: "deepseek-chat",
        input: [
          { type: "message", role: "developer", content: "System prompt" },
          { type: "message", role: "user", content: "Hello" },
        ],
        stream: true,
      };

      const result = responsesToChat(body, store);

      expect(result.messages[0]).toEqual({ role: "system", content: "System prompt" });
    });

    test("merges consecutive same-role messages", () => {
      const body = {
        model: "deepseek-chat",
        input: [
          { type: "message", role: "user", content: "First" },
          { type: "message", role: "user", content: "Second" },
          { type: "message", role: "user", content: "Third" },
        ],
        stream: true,
      };

      const result = responsesToChat(body, store);

      expect(result.messages).toHaveLength(1);
      expect(result.messages[0].content).toBe("First\n\nSecond\n\nThird");
    });
  });

  describe("function calls", () => {
    test("groups consecutive function_calls into one assistant message", () => {
      const body = {
        model: "deepseek-chat",
        input: [
          {
            type: "function_call",
            call_id: "call_1",
            name: "get_weather",
            arguments: '{"location":"Tokyo"}',
          },
          {
            type: "function_call",
            call_id: "call_2",
            name: "get_time",
            arguments: '{"timezone":"Asia/Tokyo"}',
          },
        ],
        stream: true,
      };

      const result = responsesToChat(body, store);

      expect(result.messages).toHaveLength(1);
      expect(result.messages[0]).toMatchObject({
        role: "assistant",
        content: null,
        tool_calls: [
          {
            id: "call_1",
            type: "function",
            function: { name: "get_weather", arguments: '{"location":"Tokyo"}' },
          },
          {
            id: "call_2",
            type: "function",
            function: { name: "get_time", arguments: '{"timezone":"Asia/Tokyo"}' },
          },
        ],
      });
    });

    test("converts function_call_output to tool message", () => {
      const body = {
        model: "deepseek-chat",
        input: [
          {
            type: "function_call_output",
            call_id: "call_123",
            output: '{"temperature":20,"condition":"sunny"}',
          },
        ],
        stream: true,
      };

      const result = responsesToChat(body, store);

      expect(result.messages).toEqual([
        {
          role: "tool",
          tool_call_id: "call_123",
          content: '{"temperature":20,"condition":"sunny"}',
        },
      ]);
    });

    test("attaches reasoning_content to function calls", () => {
      store.storeReasoning("call_123", "Let me check the weather...");

      const body = {
        model: "deepseek-chat",
        input: [
          {
            type: "function_call",
            call_id: "call_123",
            name: "get_weather",
            arguments: '{"location":"Tokyo"}',
          },
        ],
        stream: true,
      };

      const result = responsesToChat(body, store);

      expect(result.messages[0].reasoning_content).toBe("Let me check the weather...");
    });
  });

  describe("local_shell_call", () => {
    test("converts local_shell_call to assistant tool_call", () => {
      const body = {
        model: "deepseek-chat",
        input: [
          {
            type: "local_shell_call",
            call_id: "call_shell_1",
            action: { command: "ls -la" },
          },
        ],
        stream: true,
      };

      const result = responsesToChat(body, store);

      expect(result.messages[0]).toMatchObject({
        role: "assistant",
        content: null,
        tool_calls: [
          {
            id: "call_shell_1",
            type: "function",
            function: {
              name: "local_shell",
              arguments: '{"command":"ls -la"}',
            },
          },
        ],
      });
    });

    test("converts local_shell_call_output to tool message", () => {
      const body = {
        model: "deepseek-chat",
        input: [
          {
            type: "local_shell_call_output",
            call_id: "call_shell_1",
            output: "total 0\ndrwxr-xr-x  2 user user 4096",
          },
        ],
        stream: true,
      };

      const result = responsesToChat(body, store);

      expect(result.messages).toEqual([
        {
          role: "tool",
          tool_call_id: "call_shell_1",
          content: "total 0\ndrwxr-xr-x  2 user user 4096",
        },
      ]);
    });
  });

  describe("reasoning items", () => {
    test("drops standalone reasoning items", () => {
      const body = {
        model: "deepseek-chat",
        input: [
          { type: "message", role: "user", content: "What is 2+2?" },
          { type: "reasoning", content: "Let me think..." },
          { type: "message", role: "assistant", content: "The answer is 4." },
        ],
        stream: true,
      };

      const result = responsesToChat(body, store);

      expect(result.messages).toHaveLength(2);
      expect(result.messages.some(m => m.type === "reasoning")).toBe(false);
    });
  });

  describe("multimodal content", () => {
    test("converts input_text parts to text parts", () => {
      const content = [
        { type: "input_text", text: "What's in this image?" },
      ];

      const result = valueToChatContent(content);

      expect(result).toBe("What's in this image?");
    });

    test("converts input_image to image_url with wrapped URL", () => {
      const part = {
        type: "input_image",
        image_url: "data:image/png;base64,abc123",
      };

      const result = mapContentPart(part);

      expect(result).toEqual({
        type: "image_url",
        image_url: { url: "data:image/png;base64,abc123" },
      });
    });

    test("preserves multimodal array when non-text parts present", () => {
      const content = [
        { type: "input_text", text: "What's in this image?" },
        { type: "input_image", image_url: "data:image/png;base64,abc123" },
      ];

      const result = valueToChatContent(content);

      expect(Array.isArray(result)).toBe(true);
      expect(result).toHaveLength(2);
      expect(result[0]).toEqual({ type: "text", text: "What's in this image?" });
      expect(result[1]).toEqual({
        type: "image_url",
        image_url: { url: "data:image/png;base64,abc123" },
      });
    });
  });

  describe("tools conversion", () => {
    test("converts function tools to Chat Completions format", () => {
      const body = {
        model: "deepseek-chat",
        input: "What's the weather?",
        tools: [
          {
            type: "function",
            name: "get_weather",
            description: "Get current weather",
            parameters: {
              type: "object",
              properties: { location: { type: "string" } },
              required: ["location"],
            },
          },
        ],
        tool_choice: "auto",
        stream: true,
      };

      const result = responsesToChat(body, store);

      expect(result.tools).toHaveLength(1);
      expect(result.tools[0]).toEqual({
        type: "function",
        function: {
          name: "get_weather",
          description: "Get current weather",
          parameters: {
            type: "object",
            properties: { location: { type: "string" } },
            required: ["location"],
          },
        },
      });
      expect(result.tool_choice).toBe("auto");
    });

    test("drops non-function tools", () => {
      const body = {
        model: "deepseek-chat",
        input: "Hello",
        tools: [
          { type: "function", name: "get_weather" },
          { type: "local_shell" },
          { type: "web_search" },
        ],
        stream: true,
      };

      const result = responsesToChat(body, store);

      expect(result.tools).toHaveLength(1);
      expect(result.tools[0].function.name).toBe("get_weather");
    });

    test("flattens namespace tools", () => {
      const body = {
        model: "deepseek-chat",
        input: "Hello",
        tools: [
          {
            type: "namespace",
            name: "utils",
            tools: [
              { type: "function", name: "get_time" },
              { type: "function", name: "get_date" },
            ],
          },
        ],
        stream: true,
      };

      const result = responsesToChat(body, store);

      expect(result.tools).toHaveLength(2);
      expect(result.tools[0].function.name).toBe("get_time");
      expect(result.tools[1].function.name).toBe("get_date");
    });
  });

  describe("MiniMax special handling", () => {
    test("merges system messages into first user message", () => {
      const body = {
        model: "minimax-abab6.5",
        instructions: "You are helpful.",
        input: "Hello",
        stream: true,
      };

      const result = responsesToChat(body, store);

      expect(result.messages).toHaveLength(1);
      expect(result.messages[0].role).toBe("user");
      expect(result.messages[0].content).toContain("[System Instructions]");
      expect(result.messages[0].content).toContain("You are helpful.");
      expect(result.messages[0].content).toContain("Hello");
    });

    test("adds fallback user message if no input provided", () => {
      const body = {
        model: "minimax-abab6.5",
        stream: true,
      };

      const result = responsesToChat(body, store);

      expect(result.messages).toHaveLength(1);
      expect(result.messages[0].role).toBe("user");
      expect(result.messages[0].content).toBe("Hello");
    });
  });

  describe("previous_response_id", () => {
    test("replays history from previous response", () => {
      store.saveHistory("resp_abc", [
        { role: "user", content: "Hello" },
        { role: "assistant", content: "Hi!" },
      ]);

      const body = {
        model: "deepseek-chat",
        previous_response_id: "resp_abc",
        input: "How are you?",
        stream: true,
      };

      const result = responsesToChat(body, store);

      expect(result.messages).toHaveLength(3);
      expect(result.messages[0].content).toBe("Hello");
      expect(result.messages[1].content).toBe("Hi!");
      expect(result.messages[2].content).toBe("How are you?");
    });

    test("handles missing previous_response_id gracefully", () => {
      const body = {
        model: "deepseek-chat",
        previous_response_id: "resp_nonexistent",
        input: "Hello",
        stream: true,
      };

      const result = responsesToChat(body, store);

      expect(result.messages).toEqual([
        { role: "user", content: "Hello" },
      ]);
    });
  });

  describe("parameter mapping", () => {
    test("maps max_output_tokens to max_tokens", () => {
      const body = {
        model: "deepseek-chat",
        input: "Hello",
        max_output_tokens: 1000,
        stream: true,
      };

      const result = responsesToChat(body, store);

      expect(result.max_tokens).toBe(1000);
      expect(result.max_output_tokens).toBeUndefined();
    });

    test("maps stop_sequences to stop", () => {
      const body = {
        model: "deepseek-chat",
        input: "Hello",
        stop_sequences: ["END", "STOP"],
        stream: true,
      };

      const result = responsesToChat(body, store);

      expect(result.stop).toEqual(["END", "STOP"]);
      expect(result.stop_sequences).toBeUndefined();
    });

    test("preserves temperature", () => {
      const body = {
        model: "deepseek-chat",
        input: "Hello",
        temperature: 0.7,
        stream: true,
      };

      const result = responsesToChat(body, store);

      expect(result.temperature).toBe(0.7);
    });
  });
});
