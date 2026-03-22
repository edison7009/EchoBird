---
description: PicoClaw Bridge integration — config, model switching, role injection, output cleaning, pitfalls
---

# PicoClaw — Bridge Integration Guide

> Last verified: 2026-03-22 | PicoClaw v0.2.3 | Bridge v3.2.2

## Configuration

### plugin.json (`plugins/picoclaw/plugin.json`)
```json
{
    "id": "picoclaw",
    "name": "PicoClaw",
    "protocol": "cli-oneshot",
    "cli": {
        "command": "picoclaw",
        "detectCommand": "picoclaw version",
        "args": ["agent", "-m"],
        "messageMode": "last-arg"
    }
}
```

### config.json (`~/.picoclaw/config.json`)
```json
{
  "agents": { "defaults": { "model": "MiniMax-M2.7" } },
  "model_list": [{
    "model_name": "MiniMax-M2.7",
    "model": "openai/MiniMax-M2.7",
    "api_key": "sk-...",
    "api_base": "https://api.minimaxi.com/v1"
  }]
}
```

> [!CAUTION]
> `agents.defaults.model` uses **bare model name** (e.g. `MiniMax-M2.7`).
> `model_list[].model` uses **vendor/model** format (e.g. `openai/MiniMax-M2.7`).
> These MUST NOT match — `defaults.model` is the lookup key into `model_list[].model_name`.

---

## Model Switching

Bridge `handle_set_model("picoclaw")` writes `~/.picoclaw/config.json`.

### Critical: Vendor prefix must be `openai` or `anthropic`
PicoClaw's Go client only recognizes built-in protocol engines. Custom vendor names (e.g. `minimaxi/MiniMax-M2.7`) crash with `unknown protocol "minimaxi"`. Bridge forces all non-Anthropic models to use `openai/` prefix.

---

## Role Injection

### Method: Native AGENT.md file (NOT Message Prepending)

> [!IMPORTANT]
> PicoClaw has its own identity system (`AGENT.md`, `AGENTS.md`, `IDENTITY.md`, `SOUL.md`).
> **Message Prepending does NOT work** — PicoClaw's internal safety layer rejects user-message-level identity overrides as prompt injection attacks.

### File path
```
~/.picoclaw/workspace/AGENT.md   (always overwritten by Bridge set_role)
```

PicoClaw reads this file automatically via mtime tracking. After Bridge writes AGENT.md:
1. Bridge resets `bridgeSessionId` → forces new session
2. Next `picoclaw agent -m "..."` picks up the new AGENT.md content

### Flow
```
Channel page: user selects role
  → bridgeSetRoleLocal("picoclaw", roleId, url)
  → Bridge handle_set_role("picoclaw")
  → Downloads role markdown from CDN
  → Writes to ~/.picoclaw/workspace/AGENT.md
  → Session reset → next chat reads new role
```

---

## Output Cleaning

PicoClaw (Go binary) has the dirtiest output of all agents:

### 1. ASCII Art Banner
Every invocation prints a massive `██████╗` box-drawing logo. Filtered by `is_agent_log_line()` using block character whitelist (`█`, `╗`, `║`, `╔`, `╚`, `═`, `╝`).

### 2. Go Structured Logs
Format: `21:48:48 INF agent registry.go:38 ...`. Filtered by detecting `HH:MM:SS INF/WRN/ERR` pattern.

### 3. Go Key=Value Lines
Lines like `channel=cli chat_id=direct sender_id=cron`. Filtered by `is_agent_log_line()` key=value pattern detection.

### 4. Lobster Emoji Delimiter 🦞
PicoClaw outputs the response twice — once in log metadata, once clean after `🦞`. Bridge uses `rfind("🦞")` to extract only the clean response.

### 5. `<think>` Blocks
Models like MiniMax output reasoning in `<think>...</think>` tags. Bridge strips these with `strip_think_tags()`.

### 6. ANSI Color Codes
Stripped by `strip_ansi()` state machine + `NO_COLOR=1` env var.

---

## Pitfalls & Solutions

### 1. `model_list` array, NOT `providers` object
PicoClaw uses `model_list: [{model_name, model, api_key, api_base}]`. The deprecated `providers` object parses but `gateway` fails with "model not found in model_list".

### 2. Custom endpoint crashes with `unknown protocol`
PicoClaw's Go client only recognizes `openai/`, `anthropic/`, etc. Custom vendor prefixes (e.g. `minimaxi/`) cause fatal crash. Bridge forces `openai/` or `anthropic/` prefix based on model/URL heuristics.

### 3. Windows PATH not updated after install
Mother Agent installs to `AppData\Local\Programs\PicoClaw` but PATH may not be updated in the Bridge process. `resolve_command()` has hardcoded fallback paths for PicoClaw on Windows.

### 4. Role download handled by Bridge generically
Bridge's `handle_set_role()` uses a catch-all branch for unknown agents: `format!(".{}", agent_id)`. It writes AGENT.md to `~/.{agent_id}/workspace/AGENT.md` when the workspace directory exists. No special handling in the Rust backend needed.

### 5. `defaults.model` vs `model_list[].model` format mismatch
`defaults.model` = bare name (`MiniMax-M2.7`), `model_list[].model` = vendor-prefixed (`openai/MiniMax-M2.7`). Using the vendor-prefixed name in defaults causes "model not found in model_list" crash.

---

## CLI Reference

```bash
picoclaw version                          # show version
picoclaw agent                            # interactive mode
picoclaw agent -m "Hello"                # single message
picoclaw agent --model gpt-4o -m "Hi"    # specify model
picoclaw onboard                          # initial setup
picoclaw skills list                      # list installed skills
picoclaw skills install-builtin           # install builtin skills
picoclaw gateway                          # start gateway server
```

---

## File Index

| File | Purpose |
|------|---------|
| `plugins/picoclaw/plugin.json` | CLI config, detection, args |
| `tools/picoclaw/paths.json` | Detection paths (incl. AppData fallback) |
| `docs/api/tools/install/picoclaw.json` | Mother Agent install docs |
| `bridge-src/src/main.rs` | Bridge protocol + resolve_command fallback |
