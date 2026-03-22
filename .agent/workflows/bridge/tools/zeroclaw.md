---
description: ZeroClaw Bridge integration — config, model switching, role injection, output cleaning, pitfalls
---

# ZeroClaw — Bridge Integration Guide

> Last verified: 2026-03-22 | ZeroClaw v0.1.7 | Bridge v3.2.2

## Configuration

### plugin.json (`plugins/zeroclaw/plugin.json`)
```json
{
    "id": "zeroclaw",
    "name": "ZeroClaw",
    "protocol": "cli-oneshot",
    "cli": {
        "command": "zeroclaw",
        "detectCommand": "zeroclaw --version",
        "args": ["agent", "-m"],
        "messageMode": "last-arg"
    }
}
```

### config.toml (`~/.zeroclaw/config.toml`)
All 4 fields are **required** — ZeroClaw strict parser crashes without any of them:
```toml
default_provider = "openrouter"           # or "anthropic" / "openai" / "custom:https://..."
default_model = "MiniMax-M2.7"
default_temperature = 0.7
api_key = "sk-..."
```

### Supported API protocols
- **OpenAI only** (`apiProtocol: ["openai"]` in `paths.json`)
- ZeroClaw `custom:URL` provider always sends OpenAI-format requests
- Does NOT support Anthropic protocol — never pass anthropicUrl to ZeroClaw

---

## Model Switching

### Flow
```
Channel page: user selects model
  → invoke('bridge_set_local_model', { agentId, modelId, ... })
  → Bridge handle_set_model("zeroclaw")
  → Writes ~/.zeroclaw/config.toml (all 4 required fields)
  → Sets OPENROUTER_API_KEY + OPENAI_API_KEY env vars (fallback)
```

### Provider detection logic
| baseUrl contains | Provider value |
|-----------------|---------------|
| `openrouter.ai` | `openrouter` |
| `anthropic.com` | `anthropic` |
| `openai.com` | `openai` |
| anything else | `custom:{url}` (NO auto-append /v1) |

---

## Role Injection

### Flow
```
Channel page: user selects role
  → bridgeSetRoleLocal(agentId, roleId, url)
  → Bridge handle_set_role("zeroclaw")
  → Downloads role markdown from CDN url
  → Writes to: ~/.zeroclaw/workspace/skills/{roleId}/SKILL.md
```

- ZeroClaw auto-loads all SKILL.md files from `workspace/skills/` at startup
- No `--agent` CLI flag needed
- Role file uses standard markdown format (not YAML frontmatter)

### Message Prepending (backup)
ZeroClaw doesn't support `--system-prompt-file`. When a role is selected, Bridge prepends:
```
[Context: Please follow these guidelines when responding]
(role content)

[User Query]
(user message)
```

---

## Output Cleaning

Bridge `is_agent_log_line()` filters:
- ISO timestamp + log level lines (`2026-03-22T...Z INFO zeroclaw::...`)
- Bare log level prefixes (`INFO:`, `WARN:`)
- Structured key=value continuation lines (`provider=...`, `workspace=...`)

Dual ANSI protection:
- `NO_COLOR=1` env var set when spawning CLI
- `strip_ansi()` state machine as fallback

---

## Pitfalls & Solutions

### 1. "Custom API key not set" error
**Cause**: config.toml missing `api_key` field.
**Fix**: Bridge `handle_set_model` writes `api_key = "..."`. Also sets env vars as fallback.

### 2. "missing field `default_temperature`" crash
**Cause**: ZeroClaw strict TOML parser requires all 4 fields.
**Fix**: Always include `default_temperature = 0.7`.

### 3. ANSI escape codes in output
**Cause**: ZeroClaw uses Rust `tracing` with color output. No `--no-color` flag.
**Fix**: `NO_COLOR=1` env var + `strip_ansi()` fallback.

### 4. 404 error with custom provider
**Cause (1)**: Auto-appending `/v1` to custom URL breaks some endpoints.
**Fix**: Use URL as-is for `custom:` provider.

**Cause (2)**: Frontend passing `anthropicUrl` to OpenAI-only agent.
**Fix**: Only Claude Code uses anthropicUrl. All others always use baseUrl.

### 5. No response (silent failure)
**Debug steps**:
1. Check `~/.zeroclaw/config.toml` — all 4 fields present?
2. Test CLI directly: `zeroclaw agent -m "hello"`
3. Check Bridge version matches latest build
4. Check dev terminal stderr for `[bridge]` debug lines

### 6. PowerShell `Set-Content` adds BOM
**Cause**: PowerShell injects UTF-8 BOM which may break TOML parsing.
**Fix**: Use `[System.IO.File]::WriteAllText()` with `UTF8Encoding($false)`.

### 7. `startCommand` needed for Launch app
ZeroClaw's Launch app should run `zeroclaw daemon`. Without `startCommand` in paths.json, bare `zeroclaw` command shows help and exits.

---

## CLI Reference

```bash
zeroclaw agent                            # interactive mode
zeroclaw agent -m "Hello"                # single message
zeroclaw agent -p openrouter --model gpt-4o  # specify provider+model
zeroclaw gateway                          # start gateway server
zeroclaw daemon                           # full autonomous runtime
zeroclaw status                           # check status
zeroclaw doctor                           # run diagnostics
zeroclaw onboard                          # initial setup wizard
```

---

## File Index

| File | Purpose |
|------|---------|
| `tools/zeroclaw/paths.json` | Plugin metadata, detection paths, apiProtocol |
| `tools/zeroclaw/config.json` | Config file format declaration |
| `docs/api/tools/install/zeroclaw.json` | Install & CLI docs for frontend |
| `bridge-src/src/main.rs` | Bridge protocol: set_model, set_role, chat |
| `src-tauri/src/services/tool_config_manager.rs` | App Manager apply_zeroclaw |
| `src/pages/Channels.tsx` | Channel model/role selection UI logic |
