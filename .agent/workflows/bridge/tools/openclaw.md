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

### Session reset required
OpenClaw reads SOUL.md only at **session start**. After `set_role`, Bridge resets `bridgeSessionId` → forces new session. Only resets when role actually changes (tracked by `lastAppliedRoleRef` with `${agentId}:${roleId}` key).

---

## Pitfalls & Solutions

### 1. SOUL.md must always overwrite
OpenClaw has a default SOUL.md. If bridge checks `if file.exists() { skip }`, user's role never deploys.

### 2. Session must reset on role change
SOUL.md is only read at session start. Mid-session changes have no effect. Clear `bridgeSessionId` after `set_role`.

### 3. Don't create sub-agents
`openclaw agents add {role_id}` creates a new agent workspace with NO model/API config. Only the `main` agent has user's configured models. Always write to main workspace SOUL.md.

### 4. Role dedup key must include agent ID
`lastAppliedRoleRef` must compare `${agentId}:${role.id}`, not just `role.id`. Otherwise switching agents with the same role skips `set_role`.

### 5. detectByConfigDir is not enough
Config directory `~/.openclaw/` may exist after uninstall. Always check both config dir AND executable presence.

---

## File Index

| File | Purpose |
|------|---------|
| `plugins/openclaw/plugin.json` | CLI config, stdio-json protocol, detection |
| `tools/openclaw/paths.json` | Detection paths, startCommand for gateway |
| `bridge-src/src/main.rs` | Bridge protocol: persistent subprocess handling |
