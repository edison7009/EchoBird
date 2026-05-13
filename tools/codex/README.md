# Codex CLI Integration

This directory contains the **Codex Launcher** — a dual-spoofing proxy that bridges Codex's Responses API to third-party Chat-only endpoints.

## The Problem

**Codex v0.107+** only emits `POST /v1/responses` (Anthropic's Responses API format).

**Most LLM providers** (DeepSeek, Moonshot, Qwen, OpenRouter, etc.) only accept `POST /v1/chat/completions` (OpenAI's Chat Completions format).

There is **no config-file path** that bridges this gap — Codex cannot be configured to speak Chat Completions natively.

## The Solution

The **Codex Launcher** (`codex-launcher.cjs`) runs a local HTTP proxy server that:

1. **Translates Responses → Chat Completions** (outbound)
2. **Translates Chat Completions SSE stream → Responses-shaped SSE** (inbound)
3. **Rewrites `~/.codex/config.toml`** to point Codex at the proxy (`http://127.0.0.1:<random-port>`)
4. **Restores the original `base_url`** in `config.toml` on exit

This allows Codex to work with **any Chat Completions-compatible provider** without modifying Codex itself.

## Architecture

```
┌─────────────┐                  ┌──────────────────┐                  ┌─────────────────┐
│             │  Responses API   │                  │  Chat Completions│                 │
│  Codex CLI  │ ───────────────> │  Codex Launcher  │ ──────────────>  │  LLM Provider   │
│             │                  │  (127.0.0.1:*)   │                  │  (DeepSeek/etc) │
│             │ <─────────────── │                  │ <──────────────  │                 │
└─────────────┘  Responses SSE   └──────────────────┘  Chat SSE        └─────────────────┘
```

### Key Components

1. **Protocol Translation**
   - `responsesToChat()`: Converts Responses API items to Chat Completions messages
   - `chatToResponses()`: Converts Chat Completions SSE stream to Responses SSE format

2. **Config Management**
   - Reads `~/.echobird/codex.json` for target provider config
   - Injects proxy URL into `~/.codex/config.toml`
   - Restores original config on exit (SIGINT, SIGTERM, process exit)

3. **Session Store**
   - Stores conversation history and reasoning content
   - Enables multi-turn conversations with tool calls
   - Persists across requests within a session

4. **Logging**
   - All output mirrored to `~/.echobird/codex-launcher.log`
   - Timestamped entries for debugging
   - Quiet mode for desktop integration

## Usage

### From EchoBird Desktop

The launcher is automatically invoked when you:
1. Configure a Chat Completions provider in Model Nexus
2. Click "Apply to Codex" in the Codex integration panel
3. Launch Codex CLI from EchoBird

### Manual Launch (CLI)

```bash
# Set up your provider config
cat > ~/.echobird/codex.json <<EOF
{
  "baseURL": "https://api.deepseek.com/v1",
  "apiKey": "sk-...",
  "model": "deepseek-chat"
}
EOF

# Launch the proxy
node tools/codex/codex-launcher.cjs

# In another terminal, run Codex
codex
```

### Environment Variables

- `ECHOBIRD_CODEX_CONFIG_DIR`: Override Codex config directory (default: `~/.codex`)
- `ECHOBIRD_RELAY_DIR`: Override EchoBird relay directory (default: `~/.echobird`)
- `ECHOBIRD_LAUNCHER_QUIET=1`: Suppress console output (errors still shown)

## Protocol Translation Details

### Responses API → Chat Completions

The Responses API uses a heterogeneous array of items:

```javascript
{
  "items": [
    { "type": "message", "role": "user", "content": "Hello" },
    { "type": "message", "role": "assistant", "content": "Hi!" },
    { "type": "function_call", "name": "search", "arguments": "{...}" },
    { "type": "function_call_output", "call_id": "...", "output": "{...}" }
  ]
}
```

Chat Completions uses a simpler messages array:

```javascript
{
  "messages": [
    { "role": "user", "content": "Hello" },
    { "role": "assistant", "content": "Hi!" },
    { "role": "assistant", "tool_calls": [{...}] },
    { "role": "tool", "tool_call_id": "...", "content": "{...}" }
  ]
}
```

**Critical**: All item types must be translated faithfully. Dropping `function_call` or `function_call_output` items produces an empty messages array on tool-call follow-up turns, causing 400 errors.

### Chat Completions SSE → Responses SSE

Chat Completions streams deltas:

```
data: {"choices":[{"delta":{"content":"Hello"}}]}
```

Responses API expects:

```
data: {"type":"content_block_delta","delta":{"type":"text_delta","text":"Hello"}}
```

The launcher translates each SSE event in real-time, preserving streaming behavior.

## History

- **v4.0.2** (2026-05-06): Initial release, broken by missing `function_call` translation
- **v4.0.3** (2026-05-11): Fixed by translating ALL item types + SSE flush improvements
- **v4.5.x** (2026-05-13): Refactored logging, added session store, improved error handling

## Known Limitations

1. **OpenAI API**: Codex has native OpenAI support, so the launcher is bypassed for `api.openai.com`
2. **Streaming only**: Non-streaming responses are not supported (Codex always streams)
3. **Single session**: Each launcher instance handles one Codex session
4. **No authentication**: The proxy runs on `127.0.0.1` with no auth (local-only)

## Troubleshooting

### Codex returns "请求失败,请重试" (Request failed, please retry)

**Cause**: The launcher is not running, or the config injection failed.

**Fix**:
1. Check if the launcher is running: `ps aux | grep codex-launcher`
2. Check the log: `cat ~/.echobird/codex-launcher.log`
3. Verify config: `cat ~/.codex/config.toml` (should have `base_url = "http://127.0.0.1:..."`)

### Tool calls fail after the first turn

**Cause**: `function_call` or `function_call_output` items are being dropped.

**Fix**: This was fixed in v4.0.3. Update to the latest version.

### Streaming is slow or choppy

**Cause**: Nagle's algorithm is buffering SSE events.

**Fix**: This was fixed in v4.0.3 with `socket.setNoDelay(true)` and `res.flushHeaders()`.

### Provider returns 401 Unauthorized

**Cause**: API key is missing or invalid in `~/.echobird/codex.json`.

**Fix**: Verify your API key:
```bash
cat ~/.echobird/codex.json
```

## Related Files

- `codex-launcher.cjs`: Main launcher script (1100+ lines)
- `codex-provider-sync/`: Background sync tool for provider switching
- `../../src-tauri/src/commands/tool_commands.rs`: Rust integration for desktop app

## Future Work

- **Module extraction**: Break `codex-launcher.cjs` into smaller modules (~200 lines each)
- **Unit tests**: Add tests for protocol translation functions
- **Multi-session**: Support multiple concurrent Codex sessions
- **Config validation**: Validate provider config before starting proxy

## Contributing

See [CONTRIBUTING.md](../../CONTRIBUTING.md) for development guidelines.

When modifying the launcher:
1. Test with multiple providers (DeepSeek, Moonshot, Qwen)
2. Test tool calls (multi-turn conversations)
3. Test streaming behavior (no buffering)
4. Check logs for errors: `tail -f ~/.echobird/codex-launcher.log`
