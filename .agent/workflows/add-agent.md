---
description: How to add a new AI agent tool to Echobird (end-to-end, all touchpoints)
---

# Add Agent Tool — Step-by-Step Guide

> This guide covers EVERY file you must touch to add a new AI agent CLI tool (e.g. Claude Code) to Echobird. 
> It is written so you can follow it with zero prior context.

## Prerequisites

Before starting, you need:
1. The agent's CLI command and arguments (e.g. `claude --json --message "hello"`)
2. The agent's config directory path (e.g. `~/.claude/`)
3. The agent's role file format and location (e.g. `~/.claude/agents/{role_id}.md`)
4. Read `/encoding` workflow first — all files must be UTF-8 no-BOM, LF line endings

---

## Architecture Overview

```
User clicks Send in Channels page
  → Frontend (Channels.tsx) calls Tauri command
  → Rust backend (channel_commands.rs) writes JSON to bridge stdin
  → Bridge binary (main.rs) invokes the agent CLI
  → Bridge parses agent output → returns JSON via stdout
  → Rust backend reads response → returns to frontend
```

**Key concept**: Bridge is a universal CLI translator. It receives Echobird JSON protocol commands and translates them into agent-specific CLI invocations.

---

## Step 1: Create Plugin Config

// turbo

Create `plugins/{agent-id}/plugin.json`:

```json
{
    "id": "claudecode",
    "name": "Claude Code",
    "protocol": "stdio-json",
    "bridge": {
        "linux": "bridge-linux",
        "darwin": "bridge-darwin",
        "win32": "bridge-win.exe"
    },
    "cli": {
        "command": "claude",
        "detectCommand": "claude --version",
        "args": ["--json", "--message"],
        "resumeArgs": ["--json", "--session-id", "{sessionId}", "--message"],
        "sessionArg": "--session-id",
        "sessionMode": "always",
        "modelArg": "--model",
        "systemPromptArg": "--system-prompt"
    }
}
```

**Reference**: See `plugins/openclaw/plugin.json` for a working example.

**Fields**:
- `id` — unique agent identifier, used throughout the codebase
- `protocol` — `"stdio-json"` (persistent subprocess) or `"cli-oneshot"` (one command per message)
- `detectCommand` — command to check if agent is installed (must exit 0 if installed)
- `args` — CLI args for new chat. The user's message text is appended at the end
- `resumeArgs` — CLI args for continuing a session. `{sessionId}` is replaced at runtime
- `sessionArg` — the CLI flag for session ID
- `modelArg` — CLI flag for model selection (null if agent doesn't support it)
- `systemPromptArg` — CLI flag for system prompt (null if not supported)

---

## Step 2: Update Bridge Source

File: `bridge-src/src/main.rs`

### 2a. Add role path in `handle_set_role()`

Find the `match agent_id` block (~line 508) and add your agent:

```rust
"claudecode" => home.join(".claude").join("agents").join(format!("{}.md", role_id)),
```

> [!CAUTION]
> **Critical decision**: Does this agent share one role file (like OpenClaw's SOUL.md) or have per-role files (like Claude Code's agents/{id}.md)?
> - **Shared file**: Always overwrite. Add `agent_id != "youragent"` to the idempotent skip check (~line 524)
> - **Per-role files**: Idempotent skip is safe (if file exists, skip download)

### 2b. Add role path in `handle_clear_role()`

Find the `match agent_id` block (~line 658) and add the same path:

```rust
"claudecode" => home.join(".claude").join("agents").join(format!("{}.md", role_id)),
```

### 2c. Verify `execute_chat()` works

Bridge reads CLI config from `bridge.json` (generated from `plugin.json`). The `execute_chat()` function constructs args from this config — it should work automatically for new agents. **No code change needed here.**

### 2d. Compile locally (Windows only, for testing)

```powershell
cd bridge-src
cargo build --release
copy target\release\bridge.exe ..\bridge\bridge-win.exe
```

> [!IMPORTANT]
> Only compile for YOUR platform during development. CI cross-compiles all 5 platform binaries on release.
> The 5 binaries in `bridge/` are bundled into the installer for local + offline LAN deployment.

---

## Step 3: Update Local Agent Detection

File: `src-tauri/src/commands/role_commands.rs`

Function: `detect_local_agents()` (~line 177)

Add your agent to the detection list. Detection checks:
1. Config directory exists (e.g. `~/.claude/`)
2. Executable is found in PATH or known locations

> [!WARNING]
> **Known pitfall**: Don't check ONLY config directory. Users may uninstall the app but leave config. 
> Check both config dir AND executable presence. See the OpenClaw detection fix (commit `a877698`).

---

## Step 4: Update Frontend — Agent Tool Selector

File: `src/components/AgentRolePicker.tsx`

### 4a. Add to AGENT_TOOLS array (~line 11)

```typescript
{ id: 'claudecode', name: 'Claude Code', icon: '/icons/tools/claudecode.svg', enabled: true },
```

Set `enabled: true` only when the agent is fully implemented and tested. Keep `false` during development.

### 4b. Add agent icon

Place the icon at `public/icons/tools/{agent-id}.svg` (or `.png`).

---

## Step 5: Update Frontend — AGENT_LIST in Channels.tsx

File: `src/pages/Channels.tsx`

Find the `AGENT_LIST` constant and add:

```typescript
{ id: 'claudecode', name: 'Claude Code' },
```

This list maps display names to plugin IDs for bridge communication.

---

## Step 6: Add Role URL Support

Roles are Markdown files hosted on GitHub. When a user selects a role, the bridge downloads the `.md` file and writes it to the agent's config directory.

> [!CAUTION]
> **Known pitfall**: NEVER use `echobird.ai/docs/roles/*.md` — Cloudflare Pages returns rendered HTML, not raw Markdown.
> Always use: `https://raw.githubusercontent.com/edison7009/Echobird-MotherAgent/main/docs/roles/{lang}/{filePath}`

The URL is constructed in `Channels.tsx` in the `sendMessage` function. The role file content is the persona definition that the agent reads.

---

## Step 7: Session Management

> [!IMPORTANT]
> **Critical lesson from OpenClaw**: Some agents only read their persona/role file at session start.
> After `set_role` succeeds, you MUST reset `bridgeSessionId` to `undefined` to force a new session.
> But only do this when the role actually CHANGES — use `lastAppliedRoleRef` to track.

This logic is already in `Channels.tsx`. If your new agent also reads its config only at session start, the existing code handles it. If the agent reads config per-message, no session reset is needed.

## Step 8: Add Mother Agent Install & Configure Docs

File: `docs/api/tools/install/{id}.json`

This file tells Mother Agent HOW to install and configure the agent on remote servers via SSH. Without this, Mother Agent cannot help users deploy the agent.

### 8a. Create install JSON

Use `docs/api/tools/install/openclaw.json` as the reference template:

```json
{
    "id": "youragent",
    "displayName": "Your Agent",
    "homepage": "https://...",
    "docs": "https://...",
    "github": "https://...",
    "install": {
        "one-liner (macOS / Linux / WSL2)": "curl -fsSL https://... | bash",
        "npm (if Node.js already installed)": "npm install -g youragent@latest",
        "note": "Installation notes, special requirements, etc."
    },
    "configure": {
        "note": "How to configure model API credentials on remote servers",
        "steps": ["1. ...", "2. ...", "3. ..."],
        "merge_script": "python3 -c \"...\"",
        "after_configure": "restart command",
        "verify": "verification command"
    }
}
```

**Key sections**:
- `install` — All installation methods (one-liner, npm, manual). Include `--no-onboard` variant for SSH
- `configure` — How to write API key / model config on the remote server. Include a `merge_script` that preserves existing config

### 8b. Register in index.json

Add the tool ID to `docs/api/tools/install/index.json`:

```json
{
    "ids": ["claudecode", "openclaw", "opencode", "youragent"]
}
```

> [!IMPORTANT]
> The `configure` section is critical for remote deployment. Without it, Mother Agent can install the agent but cannot configure the API key, making it unusable. Reference `openclaw.json`'s `merge_script` and `field_mapping` for the pattern.

---

## Key Files Reference

| File | What to change |
|------|---------------|
| `plugins/{id}/plugin.json` | **[NEW]** CLI config, detection, args |
| `bridge-src/src/main.rs` | Add to `handle_set_role()` + `handle_clear_role()` |
| `src-tauri/src/commands/role_commands.rs` | Add to `detect_local_agents()` |
| `src/components/AgentRolePicker.tsx` | Add to `AGENT_TOOLS[]`, set `enabled: true` |
| `src/pages/Channels.tsx` | Add to `AGENT_LIST[]` |
| `public/icons/tools/{id}.svg` | **[NEW]** Agent icon |
| `docs/api/tools/install/{id}.json` | **[NEW]** Mother Agent install & configure docs |
| `docs/api/tools/install/index.json` | Register new tool ID |

---

## Testing Checklist

- [ ] `detectCommand` works locally (agent detected as installed)
- [ ] Role picker shows agent tab as clickable (not greyed out)
- [ ] Selecting a role downloads the `.md` file (check agent config dir)
- [ ] Sending a message invokes the agent and returns a response
- [ ] Session continuity works (multi-turn conversation)
- [ ] Role switch forces new session (agent responds as new persona)
- [ ] Red dot notification works when response arrives on non-active channel
- [ ] Remote server detection works (if `bridgeDetectAgentsRemote` is implemented)
- [ ] Mother Agent can install the agent on a remote server using install docs
- [ ] Mother Agent can configure API key on a remote server using configure docs

---

## Pitfalls — Learned the Hard Way

1. **CI build order**: Bridge must compile BEFORE Tauri app, or the installer bundles stale binaries
2. **Role URL must be raw Markdown**: `echobird.ai` returns HTML, use `raw.githubusercontent.com`
3. **Shared role files must always overwrite**: If agent uses one file (like SOUL.md), skip the idempotent check
4. **Session reset on role change**: Some agents only read config at session start
5. **Don't create sub-agents**: OpenClaw's `agents add` creates agents with no model config. Always use the main agent
6. **detectByConfigDir is not enough**: Must also check executable exists
7. **UTF-8 no-BOM everywhere**: BOM breaks Tauri build. Use `/encoding` workflow
8. **Commit immediately after fixing bugs**: Uncommitted changes get lost between sessions
9. **Install docs are essential**: Without `docs/api/tools/install/{id}.json`, Mother Agent cannot deploy the agent remotely

