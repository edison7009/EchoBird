---
description: NanoBot Bridge integration — config, model switching, role injection, output cleaning, pitfalls
---

# NanoBot — Bridge Integration Guide

> Last verified: 2026-03-22 | NanoBot v3.2.2 | Bridge v3.2.2

## Configuration

### plugin.json (`plugins/nanobot/plugin.json`)
```json
{
    "id": "nanobot",
    "name": "NanoBot",
    "protocol": "cli-oneshot",
    "cli": {
        "command": "python -m nanobot",
        "detectCommand": "python -m nanobot --version",
        "args": ["agent", "--no-markdown", "--no-logs", "-m"],
        "messageMode": "last-arg"
    }
}
```

### config.json (`~/.nanobot/config.json`)
```json
{
  "agents": { "defaults": { "model": "minimax/MiniMax-M2.7" } },
  "providers": { "custom": { "apiBase": "https://api.minimaxi.com/v1", "apiKey": "sk-..." } }
}
```

> `providers.custom.apiBase` MUST include `/v1` suffix — Bridge runs `ensure_v1_suffix()` automatically.

---

## Model Switching

Bridge `handle_set_model("nanobot")` writes `~/.nanobot/config.json` with `providers.custom` format.

---

## Role Injection

### File path
```
~/.nanobot/workspace/AGENTS.md   (always overwritten)
```

### Why Message Prepending is also needed
NanoBot Python source (`context.py`) **hardcodes** the system prompt: `"You are nanobot, a helpful AI assistant."`. With weaker models (MiniMax etc.), this overrides the role file content. Bridge uses Message Prepending as reinforcement:
```
[Context: Please follow these guidelines when responding]
(role content from AGENTS.md)

[User Query]
(user message)
```

---

## Output Cleaning

- `--no-markdown` and `--no-logs` flags suppress most framework noise
- `is_agent_log_line()` filters the `🐈 nanobot` ASCII banner prefix
- General `strip_ansi()` removes any remaining escape codes

---

## Pitfalls & Solutions

### 1. `pip` install doesn't produce `.exe` on Windows
**Problem**: `pip install nanobot` may not generate `nanobot.exe` in PATH on some Windows environments.
**Fix**: `paths.json` uses `"pythonModule": "nanobot"` — detection runs `python -m nanobot --version`. If available, executable command becomes `python -m nanobot`.

### 2. Rust `Command::new` cannot execute space-separated commands
**Problem**: `Command::new("python -m nanobot")` tries to find a binary literally named that.
**Fix**: Bridge splits `config.command.split_whitespace()` — first token is executable, rest are prepended to args.

### 3. Tauri hot-reload locks `.exe` preventing `cargo build`
**Problem**: Running `npm run dev` locks the Bridge binary; `cargo build` throws `os error 5`.
**Fix**: Bridge compiles independently in `bridge-src/` → copy to `bridge/bridge-win.exe`. Only `src-tauri` changes require dev server restart.

---

## File Index

| File | Purpose |
|------|---------|
| `plugins/nanobot/plugin.json` | CLI config, detection, args |
| `tools/nanobot/paths.json` | Detection paths, pythonModule |
| `bridge-src/src/main.rs` | Bridge protocol: set_model, set_role, chat |
