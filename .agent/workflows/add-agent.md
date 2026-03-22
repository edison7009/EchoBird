---
description: How to add a new AI agent tool to Echobird (end-to-end, all touchpoints)
---

# Add Agent Tool -- Step-by-Step Guide

> This guide covers EVERY file you must touch to add a new AI agent CLI tool (e.g. Claude Code) to Echobird. 
> It is written so you can follow it with zero prior context.

## Prerequisites

Before starting, you need:
1. The agent's CLI command and arguments (e.g. `claude --json --message "hello"`)
2. The agent's config directory path (e.g. `~/.claude/`)
3. The agent's role file format and location (e.g. `~/.claude/agents/{role_id}.md`)
4. Read `/encoding` workflow first -- all files must be UTF-8 no-BOM, LF line endings

---

## Architecture Overview

```
User clicks Send in Channels page
  -> Frontend (Channels.tsx) calls Tauri command
  -> Rust backend (channel_commands.rs) writes JSON to bridge stdin
  -> Bridge binary (main.rs) invokes the agent CLI
  -> Bridge parses agent output -> returns JSON via stdout
  -> Rust backend reads response -> returns to frontend
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

### cli-oneshot Protocol (Claude Code example)

If the agent CLI does NOT support persistent stdin/stdout communication, use `"protocol": "cli-oneshot"` instead. Each message spawns a new CLI process:

```json
{
    "id": "claudecode",
    "name": "Claude Code",
    "protocol": "cli-oneshot",
    "cli": {
        "command": "claude",
        "detectCommand": "claude --version",
        "args": ["-p", "--dangerously-skip-permissions", "--output-format", "json"],
        "resumeArgs": ["-p", "--dangerously-skip-permissions", "--output-format", "json", "--resume", "{sessionId}"],
        "sessionArg": "--session-id",
        "modelArg": "--model",
        "systemPromptArg": "--system-prompt",
        "systemPromptWhen": "new-session",
        "agentArg": "--agent",
        "messageMode": "last-arg"
    }
}
```

> [!CAUTION]
> **cli-oneshot key differences**:
> - No `bridge` section needed (no persistent subprocess)
> - `messageMode: "last-arg"` -- message text is the LAST positional argument, not via `--message` flag
> - `systemPromptWhen: "new-session"` -- only inject prompt on first message (avoid redundant tokens)
> - Agent-specific flags like `--dangerously-skip-permissions` may be needed to bypass interactive prompts

**Reference**: See `plugins/openclaw/plugin.json` (stdio-json) and `plugins/claudecode/plugin.json` (cli-oneshot) for working examples.

**Fields**:
- `id` -- unique agent identifier, used throughout the codebase
- `protocol` -- `"stdio-json"` (persistent subprocess) or `"cli-oneshot"` (one command per message)
- `detectCommand` -- command to check if agent is installed (must exit 0 if installed)
- `args` -- CLI args for new chat. The user's message text is appended at the end
- `resumeArgs` -- CLI args for continuing a session. `{sessionId}` is replaced at runtime
- `sessionArg` -- the CLI flag for session ID
- `modelArg` -- CLI flag for model selection (null if agent doesn't support it)
- `systemPromptArg` -- CLI flag for system prompt (null if not supported)

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

### 2c. Add output format to `parse_agent_output()` (if using Bridge)

Bridge's `parse_agent_output()` extracts text and session_id from agent CLI output.
Different agents return different JSON formats:

- **OpenClaw**: `{"result": {"payloads": [{"text": "..."}], "meta": {"agentMeta": {"sessionId": "..."}}}}`
- **Claude Code**: `{"result": "plain text here", "session_id": "..."}` or `{"result": {"content": [{"text": "..."}]}}`

Add a new format branch in `parse_agent_output()` for your agent's output.

> [!IMPORTANT]
> **Deprecated**: Steps 2d/2e from older versions of this guide described a flow where cli-oneshot agents bypassed Bridge and used direct Rust backend functions (`bridge_chat_oneshot()`, `download_role_file_direct()`, `ONESHOT_STATE`). These have been **fully removed**. ALL agents — both `stdio-json` and `cli-oneshot` — now go through the Bridge binary uniformly. No special handling is needed in `channel_commands.rs`.

### 2f. Compile locally (Windows only, for testing)

```powershell
cd bridge-src
cargo build --release
copy target\release\echobird-bridge.exe ..\bridge\bridge-win.exe
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

## Step 4: Update Frontend -- Agent Tool Selector

File: `src/components/AgentRolePicker.tsx`

### 4a. Add to AGENT_TOOLS array (~line 11)

```typescript
{ id: 'claudecode', name: 'Claude Code', icon: '/icons/tools/claudecode.svg', enabled: true },
```

Set `enabled: true` only when the agent is fully implemented and tested. Keep `false` during development.

### 4b. Add agent icon

Place the icon at `public/icons/tools/{agent-id}.svg` (or `.png`).

---

## Step 5: Agent Lists and Detection

Three files contain hardcoded agent lists that MUST be updated when adding a new agent:

### 5a. Channels Page AGENT_LIST
File: `src/pages/Channels.tsx`

Find the `AGENT_LIST` constant and add the new agent. This controls the channel avatar/icon display.

```typescript
{ id: 'youragent', name: 'Your Agent', icon: '/icons/tools/youragent.png' },
```

### 5b. AgentRolePicker AGENT_TOOLS
File: `src/components/AgentRolePicker.tsx`

Find the `AGENT_TOOLS` constant and add the new agent. This controls the role picker agent selector tabs.

```typescript
{ id: 'youragent', name: 'Your Agent', icon: '/icons/tools/youragent.png', enabled: true },
```

### 5c. Local Agent Detection
File: `src-tauri/src/commands/role_commands.rs`

Find the `agents` array in `detect_local_agents()` and add the new agent CLI command name.

```rust
("youragent", "Your Agent", "youragent"),  // the CLI command name
```

> [!CAUTION]
> **Missing any of these 3 files will cause bugs:** missing from AGENT_LIST = wrong icon in Channels, missing from AGENT_TOOLS = agent tab won't appear in role picker, missing from detect_local_agents = agent shows as 'available' even when not installed.

---

## Step 6: Add Role URL Support

Roles are Markdown files hosted on GitHub. When a user selects a role, the bridge downloads the `.md` file and writes it to the agent's config directory.

> [!CAUTION]
> **Known pitfall**: NEVER use `echobird.ai/docs/roles/*.md` -- Cloudflare Pages returns rendered HTML, not raw Markdown.
> Always use: `https://raw.githubusercontent.com/edison7009/Echobird-MotherAgent/main/docs/roles/{lang}/{filePath}`

The URL is constructed in `Channels.tsx` in the `sendMessage` function. The role file content is the persona definition that the agent reads.

---

## Step 7: Session Management

> [!IMPORTANT]
> **Critical lesson from OpenClaw**: Some agents only read their persona/role file at session start.
> After `set_role` succeeds, you MUST reset `bridgeSessionId` to `undefined` to force a new session.
> But only do this when the role actually CHANGES -- use `lastAppliedRoleRef` with `agentId:roleId` key.

> [!CAUTION]
> **Role dedup key must include agent ID!** If only comparing `role.id`, switching from OpenClaw to Claude Code with the same role will SKIP `set_role`. Use `${agentId}:${role.id}` as the dedup key.

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
- `install` -- All installation methods (one-liner, npm, manual). Include `--no-onboard` variant for SSH
- `configure` -- How to write API key / model config on the remote server. Include a `merge_script` that preserves existing config

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
| `tools/{id}/paths.json` | **[NEW]** Detection paths, `startCommand` for Launch app |
| `tools/{id}/config.json` | **[NEW]** Config mapping (how to write API key/model) |
| `bridge-src/src/main.rs` | Add to `handle_set_role()` + `handle_clear_role()` + `parse_agent_output()` |
| `src-tauri/src/commands/channel_commands.rs` | Bridge subprocess management (start/stop/chat/set_role — all agents) |
| `src-tauri/src/services/tool_patcher.rs` | **Add patcher function** + register in `patch_tool()` dispatch |
| `src-tauri/src/services/plugin_manager.rs` | Add new CLI fields (e.g. `agent_arg`) to `CliConfig` struct |
| `src-tauri/src/commands/role_commands.rs` | Add to `detect_local_agents()` |
| `src/components/AgentRolePicker.tsx` | Add to agent tools array, set `enabled: true` |
| `src/pages/Channels.tsx` | Add to agent list array |
| `src/pages/AppManager.tsx` | Add to `toolModelConfig` initial state |
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
- [ ] App Manager model selection works (model appears, can select)
- [ ] App Manager Launch app starts the daemon/gateway
- [ ] Patcher writes correct config after model apply

---

## Pitfalls -- Learned the Hard Way

### General
1. **CI build order**: Bridge must compile BEFORE Tauri app, or the installer bundles stale binaries
2. **Role URL must be raw Markdown**: `echobird.ai` returns HTML, use `raw.githubusercontent.com`
3. **Shared role files must always overwrite**: If agent uses one file (like SOUL.md), skip the idempotent check
4. **Session reset on role change**: Some agents only read config at session start
5. **Don't create sub-agents**: OpenClaw's `agents add` creates agents with no model config. Always use the main agent
6. **detectByConfigDir is not enough**: Must also check executable exists
7. **UTF-8 no-BOM everywhere**: BOM breaks Tauri build. Use `/encoding` workflow
8. **Commit immediately after fixing bugs**: Uncommitted changes get lost between sessions
9. **Install docs are essential**: Without `docs/api/tools/install/{id}.json`, Mother Agent cannot deploy the agent remotely
10. **App Manager `toolModelConfig` must include the new agent ID**: Without it, model selection silently fails because `toolModelConfig[toolId]` returns `undefined` instead of `null`
11. **`tools/{id}/paths.json` needs `startCommand`**: Without it, Launch app falls back to the bare CLI command which may just show help and exit
12. **Patcher must be registered in `patch_tool()` dispatch**: Adding `patch_youragent()` without registering it in the match block means App Manager model-apply does nothing

### Agent-Specific Pitfalls

> **Moved to per-agent files.** See `.agent/workflows/bridge/tools/` for detailed pitfalls per agent:
> - [claudecode.md](bridge/tools/claudecode.md) — Claude Code (cli-oneshot, `--system-prompt-file`, `--agent` hang)
> - [openclaw.md](bridge/tools/openclaw.md) — OpenClaw (stdio-json, SOUL.md overwrite, session reset)
> - [zeroclaw.md](bridge/tools/zeroclaw.md) — ZeroClaw (TOML config, custom provider 404)
> - [nanobot.md](bridge/tools/nanobot.md) — NanoBot (Python module, `split_whitespace`)
> - [picoclaw.md](bridge/tools/picoclaw.md) — PicoClaw (Go binary, AGENT.md injection, unknown protocol)
> - [hermes.md](bridge/tools/hermes.md) — Hermes (-Q/-q flags, no Windows, SOUL.md auto-read)