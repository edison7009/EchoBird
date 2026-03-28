---
description: OpenClaw Bridge integration — config, model switching, role injection, session management, pitfalls
---

# OpenClaw — Bridge Integration Guide

> Last verified: 2026-03-22 | Bridge v3.2.2

## Configuration

### plugin.json (`plugins/openclaw/plugin.json`)
```json
{
    "id": "openclaw",
    "name": "OpenClaw",
    "protocol": "stdio-json",
    "bridge": {
        "linux": "bridge-linux-x86_64",
        "darwin": "bridge-darwin-aarch64",
        "win32": "bridge-win.exe"
    },
    "cli": {
        "command": "openclaw",
        "detectCommand": "openclaw --version",
        "args": ["--json", "--message"],
        "resumeArgs": ["--json", "--session-id", "{sessionId}", "--message"],
        "sessionArg": "--session-id",
        "sessionMode": "always"
    }
}
```

### Key difference: stdio-json protocol
OpenClaw is the **only agent** using persistent subprocess communication. Bridge runs as a long-lived process, receiving JSON on stdin and writing JSON on stdout. All other agents use `cli-oneshot`.

### config.json (`~/.openclaw/openclaw.json`)
```json
{
  "models": {
    "mode": "merge",
    "providers": {
      "eb_openrouter": {
        "baseUrl": "https://openrouter.ai/api/v1",
        "apiKey": "sk-...",
        "api": "openai-completions",
        "models": [{ "id": "model-id", "name": "Model Name", ... }]
      }
    }
  },
  "agents": {
    "defaults": {
      "model": { "primary": "eb_openrouter/model-id" }
    }
  }
}
```

Bridge auto-generates the `eb_{provider}` tag from the API base URL hostname.

---

## Model Switching

Bridge `handle_set_model("openclaw")` writes `~/.openclaw/openclaw.json` with full provider config. Preserves existing `gateway` token if present.

### API type detection
- Anthropic models → `"api": "anthropic-messages"`
- All others → `"api": "openai-completions"`

---

## Role Injection

### File path
```
~/.openclaw/workspace/SOUL.md   (always overwritten)
```

> [!IMPORTANT]
> OpenClaw ships with a **default SOUL.md**. Bridge must ALWAYS overwrite it — never use idempotent skip.
> The `handle_set_role` skip check excludes OpenClaw: `if agent_id != "openclaw" && target.exists() { skip }`.

### Session reset required after role change
OpenClaw **caches the compiled system prompt** (SOUL.md + workspace files) in `agent:main:main` session. A new `--session-id UUID` reuses the cached prompt — it does NOT re-read SOUL.md from disk. Only the built-in `/new` reset trigger forces the gateway to re-read workspace files and recompile the system prompt.

After `set_role` writes SOUL.md, the bridge sends:
```
openclaw agent --json --agent main --message "/new"
```

This is handled inside `restart_gateway_if_needed()` in the bridge binary. The Tauri backend also clears `bp.session_id` and the frontend clears `bridgeSessionId` to ensure a fresh session ID is generated for the next chat.

> [!IMPORTANT]
> Neither gateway restart, new session UUIDs, nor `bp.session_id = None` alone will apply role changes. The `/new` message is **mandatory** to invalidate OpenClaw's prompt cache.

---

## Pitfalls & Solutions

### 1. SOUL.md must always overwrite
OpenClaw has a default SOUL.md. If bridge checks `if file.exists() { skip }`, user's role never deploys.

### 2. `/new` is mandatory after role change (prompt cache invalidation)
OpenClaw **caches the compiled system prompt** in `agent:main:main` session. Writing SOUL.md alone does nothing — the gateway keeps serving the old cached prompt. After `set_role` writes SOUL.md, the bridge MUST send `/new` to force prompt recompilation from disk.
- **Bridge action**: `restart_gateway_if_needed()` sends `openclaw agent --json --agent main --message /new`
- **Never use `--session-id`**: Random UUIDs create separate sessions that bypass the `/new` reset of `agent:main:main`

### 3. Windows `.cmd` needs `cmd.exe /c`
`openclaw` is an npm `.cmd` script on Windows. `Command::new("openclaw")` silently fails. MUST use `cmd.exe /c resolve_command("openclaw")` — same pattern as `execute_chat()`.

### 4. Don't create sub-agents
`openclaw agents add {role_id}` creates a new agent workspace with NO model/API config. Only the `main` agent has user's configured models. Always write to main workspace SOUL.md.

### 5. Role dedup key must include agent ID
`lastAppliedRoleRef` must compare `${agentId}:${role.id}`, not just `role.id`. Otherwise switching agents with the same role skips `set_role`.

### 6. detectByConfigDir is not enough
Config directory `~/.openclaw/` may exist after uninstall. Always check both config dir AND executable presence.

---

## File Index

| File | Purpose |
|------|---------|
| `plugins/openclaw/plugin.json` | CLI config, stdio-json protocol, detection (no sessionMode, no resumeArgs) |
| `tools/openclaw/paths.json` | Detection paths, startCommand for gateway |
| `src-tauri/src/commands/channel_commands.rs` | bridges local/remote chat, clears session_id after set_role |
| `bridge-src/src/main.rs` | `restart_gateway_if_needed` sends `/new` via `cmd.exe /c` on Windows |
| `bridge-src/src/main.rs` | Bridge protocol: persistent subprocess handling |
