---
description: Hermes Agent Bridge integration — config, model switching, role injection, output cleaning, pitfalls
---

# Hermes Agent — Bridge Integration Guide

> Last verified: 2026-03-22 | Hermes Agent | Bridge v3.2.2

## Configuration

### plugin.json (`plugins/hermes/plugin.json`)
```json
{
    "id": "hermes",
    "name": "Hermes Agent",
    "protocol": "cli-oneshot",
    "cli": {
        "command": "hermes",
        "detectCommand": "hermes --version",
        "args": ["chat", "-Q", "-q"],
        "messageMode": "last-arg"
    }
}
```

> [!CAUTION]
> `-Q` (uppercase) = quiet/programmatic mode (suppress banner, spinner, tool previews).
> `-q` (lowercase) = query text input.
> **Both are needed**: `hermes chat -Q -q "message"`. Missing `-Q` outputs full ASCII art banner.

### Platform support
- **Linux / macOS / WSL2 only** — does NOT support native Windows
- Set `"win32": []` in paths.json
- App Manager shows "AI Auto-Install" button on Windows

---

## Model Switching

### Config Rules (confirmed from Hermes source cli.py, 2026-03-23)

```python
# Model comes from: CLI arg or config.yaml model.default (single source of truth).
# LLM_MODEL/OPENAI_MODEL env vars are NOT checked — config.yaml is authoritative.
# Default fallback: anthropic/claude-opus-4.6
```

### Config Precedence
1. **CLI argument** `--model X` (highest)
2. **config.yaml** `model.default` key (nested YAML, NOT flat `model:`)
3. **Hardcoded default** `anthropic/claude-opus-4.6` (lowest)

> [!WARNING]
> `.env LLM_MODEL` and `HERMES_MODEL` are **NOT read by Hermes** despite being documented.
> Hermes source explicitly skips env var model detection.

### Correct approach: custom endpoint
```bash
# 1. Set model in config.yaml (the ONLY place Hermes reads model from)
hermes config set model.default {model_id}

# 2. Set API credentials in .env (sed directly, hermes config set routes wrong)
sed -i 's|^OPENAI_API_KEY=.*|OPENAI_API_KEY={api_key}|' ~/.hermes/.env
sed -i 's|^OPENAI_BASE_URL=.*|OPENAI_BASE_URL={base_url}|' ~/.hermes/.env

# 3. Clear ANTHROPIC_API_KEY (prevents auto-detection override)
sed -i 's|^ANTHROPIC_API_KEY=.*|ANTHROPIC_API_KEY=|' ~/.hermes/.env
```

> [!CAUTION]
> **DO NOT use `openai/` prefix on model names.** Hermes uses OpenAI SDK directly
> and passes model name as-is to the endpoint. `openai/MiniMax-M2.7` → API returns
> "unknown model" error.

> [!CAUTION]
> **`hermes config set OPENAI_BASE_URL`** writes to config.yaml, but Hermes reads
> OPENAI_BASE_URL from **.env only**. Always sed .env directly.

### What DOESN'T work (tried and failed)

| Command | Problem |
|---------|---------|
| `hermes config set model X` | Writes flat `model: X` to config.yaml — Hermes ignores (needs `model.default`) |
| `hermes config set LLM_MODEL X` | Writes `LLM_MODEL:` to config.yaml — Hermes doesn't read this key |
| `.env LLM_MODEL=X` | Hermes source explicitly skips env var model detection |
| `openai/model-name` prefix | Sent as-is to API, causing "unknown model" errors |

Bridge `handle_set_model("hermes")` uses: `hermes config set model.default` + sed `.env`.

---

## Role Injection

### File path
```
~/.hermes/SOUL.md   (always overwritten by Bridge set_role)
```

### Key behavior: Auto-read per chat
Unlike OpenClaw (reads SOUL.md only at session start), **Hermes reads `~/.hermes/SOUL.md` on every `hermes chat` invocation**. No session reset needed on role change.

### Message Prepending (backup)
Hermes is also in the Message Prepending list as reinforcement for weaker models:
```
[Context: Please follow these guidelines when responding]
(role content)

[User Query]
(user message)
```

---

## Output Cleaning

### Banner suppression
The `-Q` flag is the primary solution — one flag suppresses banner, spinner, and tool previews. This was discovered by running `hermes chat --help`.

> **Lesson**: ALWAYS run `--help` first before writing complex regex filters.

### Log line filtering
`is_agent_log_line()` catches lines starting with `Hermes` or `hermes`.

### Session ID footer
With `-Q`, Hermes appends `session_id: xxx` as the last line. Backend strips this and captures it for session management.

### ANSI cleanup
General `strip_ansi()` removes any remaining escape codes.

---

## Pitfalls & Solutions

### 1. No native Windows support
Hermes is Python-based and does not build/install natively on Windows. Set empty paths for win32 in paths.json. Remote-only on Windows.

### 2. `-Q` vs `-q` case sensitivity
`-Q` = quiet mode (programmatic), `-q` = query text. Using wrong case either suppresses input or shows full banner.

### 3. SOUL.md auto-read per chat
No session reset needed after role change — each `hermes chat` invocation reads SOUL.md fresh. This is different from OpenClaw.

### 4. `session_id:` footer in output
With `-Q`, Hermes appends `session_id: xxx` as last line. Must strip this before displaying response and capture for session continuity.

### 5. Remote uses Bridge too (unified architecture)
Both local and remote channels pipe JSON into Bridge. Remote: `echo JSON | ~/echobird/echobird-bridge --command '{agent_cli}'` via SSH. The Bridge binary on the remote server handles all agent invocation — no direct SSH CLI calls.

### 6. Version detection: strip ANSI + v-prefix
Tools like Hermes may output colored version strings. `get_version()` in `platform.rs` must strip ANSI codes and handle `v`-prefixed versions.

---

## File Index

| File | Purpose |
|------|---------|
| `plugins/hermes/plugin.json` | CLI config, detection, args |
| `tools/hermes/paths.json` | Detection paths (empty win32) |
| `bridge-src/src/main.rs` | Bridge protocol: set_model (CLI commands), set_role |
