# Echobird Channels Architecture

## Core Principle

> **⚠️ CRITICAL: Bridge is the SOLE communication layer for ALL agents, both LOCAL and REMOTE. The Tauri backend NEVER calls agent CLIs directly. No exceptions. Adding a new agent only requires a new `plugins/{agent_id}/plugin.json` — zero Tauri code changes for basic support.**

## Overview

Channels page lets users chat with AI agents installed on **local** or **remote** servers. Whether the agent is running on `127.0.0.1` or a remote Linux server via SSH, the message always flows through Bridge:

```
User → Channels.tsx → tauri.ts → channel_commands.rs → Bridge → Agent CLI
                                                         ↑
                                              ALL paths go through Bridge
                                              Local = persistent subprocess
                                              Remote = one-shot via SSH pipe
```

---

## Six Supported Agents

| Agent | Protocol | Language | Session | Role Mechanism |
|-------|----------|----------|---------|---------------|
| OpenClaw | stdio-json | Node.js | Gateway persistent | Overwrite `~/.openclaw/workspace/SOUL.md` |
| Claude Code | cli-oneshot | Node.js | `--resume {sid}` | Per-file `~/.claude/agents/{role_id}.md` + `--agent {YAML name}` |
| ZeroClaw | cli-oneshot | Rust | Not supported | Per-directory `~/.zeroclaw/workspace/skills/{role_id}/SKILL.md` |
| NanoBot | cli-oneshot | Python | `--session {sid}` | Overwrite `~/.nanobot/workspace/AGENTS.md` |
| PicoClaw | cli-oneshot | Go | `-s {sid}` | Overwrite `~/.picoclaw/workspace/AGENT.md` |
| Hermes | cli-oneshot | Python | `--resume {sid}` | Overwrite `~/.hermes/SOUL.md` |

---

## Plugin Config (plugin.json)

Each agent has a config at `plugins/{agent_id}/plugin.json`:

```jsonc
{
    "id": "claudecode",
    "name": "Claude Code",
    "protocol": "cli-oneshot",          // or "stdio-json"
    "cli": {
        "command": "claude",            // CLI binary name
        "detectCommand": "claude --version",
        "args": ["-p", "--dangerously-skip-permissions", "--output-format", "json"],
        "resumeArgs": ["-p", "--dangerously-skip-permissions", "--output-format", "json", "--resume", "{sessionId}"],
        "sessionArg": "--session-id",   // Parsed by Bridge for output session ID extraction
        "sessionMode": "always",        // "always" = always track session
        "modelArg": "--model",          // How to pass model override
        "agentArg": "--agent",          // How to pass role/agent name (Claude Code only)
        "systemPromptArg": "--system-prompt",
        "systemPromptWhen": "new-session",
        "messageMode": "last-arg"       // Message is appended as last positional arg
    }
}
```

### Key Fields

| Field | Purpose | Who Uses It |
|-------|---------|------------|
| `args` | Base CLI arguments for new sessions | Bridge (local), channel_commands.rs (remote) |
| `resumeArgs` | CLI arguments for session continuation (replaces `args` when `session_id` present) | channel_commands.rs (remote), Bridge (local) |
| `sessionArg` | Flag name to extract session ID from output | Bridge |
| `agentArg` | Flag for role injection (e.g. `--agent`) | Bridge (local via ACTIVE_ROLE), channel_commands.rs (remote via `--agent-arg`) |
| `messageMode` | `"last-arg"` = positional, `"--message"` = flag-based | Bridge |

---

## Communication Flow

> **Both local and remote use Bridge — the only difference is HOW Bridge runs:**
> - **Local**: Bridge runs as a **persistent subprocess** (started once, stdin/stdout streaming)
> - **Remote**: Bridge runs as a **one-shot command** (SSH pipes JSON into remote Bridge binary)
>
> The Tauri backend NEVER constructs raw agent CLI commands. It always delegates to Bridge.

### Local Channels

```
Channels.tsx
  → bridgeStart(pluginId)           // Start persistent Bridge subprocess with --config plugin.json
  → bridgeSetRoleLocal(agentId, roleId, url)  // Write role file (Bridge downloads .md)
  → bridgeChatLocal(message, sessionId, systemPrompt, roleName)
      // roleName = role.name from frontend (YAML frontmatter display name)
      // Passed as agent_name in JSON → Bridge uses it for --agent flag
      // agent_name in JSON OVERRIDES Bridge's ACTIVE_ROLE (priority: JSON > stored)
```

**Bridge runs as persistent subprocess** — it reads `plugin.json`, manages sessions. For Claude Code, `agent_name` from JSON takes priority over ACTIVE_ROLE for `--agent` injection.

### Remote Channels

```
Channels.tsx
  → bridgeSetRoleRemote(serverId, agentId, roleId, url)  // SSH: pipe set_role to remote Bridge
  → bridgeChatRemote(serverId, message, sessionId, pluginId, roleName)
    → channel_commands.rs builds SSH command:
      echo '{"type":"chat","message":"...","agent_name":"制度文件撰写专家"}' |
        ~/echobird/echobird-bridge --command 'claude -p --output-format json' --agent-arg '--agent'
```

**Bridge runs as one-shot** — no persistent state. Key differences from local:
- `resumeArgs` from plugin.json are resolved in `channel_commands.rs` (not Bridge)
- `--agent-arg` flag tells Bridge what CLI flag to use for role injection
- `agent_name` in input JSON tells Bridge WHICH role to inject

---

## Model Configuration

### How Models Reach Each Agent

| Agent | Config File | Key Fields | Written By |
|-------|------------|------------|-----------|
| OpenClaw | `~/.openclaw/openclaw.json` | `models.providers.eb_xxx` + `agents.defaults.model.primary` | `bridge_set_remote_model` / `apply_openclaw` |
| Claude Code | `~/.claude/settings.json` | `env.ANTHROPIC_MODEL`, `env.ANTHROPIC_BASE_URL`, `env.ANTHROPIC_API_KEY` | `bridge_set_remote_model` / `apply_generic_json` |
| ZeroClaw | `~/.zeroclaw/config.toml` | `[model]` section | `bridge_set_remote_model` |
| NanoBot | `~/.nanobot/config.json` | `providers` object | `bridge_set_remote_model` |
| PicoClaw | `~/.picoclaw/config.json` | `provider` + `model` | `bridge_set_remote_model` |
| Hermes | `~/.hermes/config.json` | `provider` + `model` | `bridge_set_remote_model` |

### Protocol Filtering in UI

- **Claude Code**: Only shows models with `anthropicUrl` (Anthropic protocol required)
- **All others**: Show all models (OpenAI-compatible)
- Model ID passed is `modelId` (e.g. `MiniMax-M2.7`), not `internalId` (e.g. `m-198c5a`)

---

## Role (Agent Persona) System

### Role Lifecycle

```
1. User selects role in Channels UI
2. Frontend calls bridgeSetRoleLocal/Remote(agentId, roleId, roleUrl)
3. Bridge downloads .md file from roleUrl → writes to agent's role path
4. On chat: Bridge injects --agent {roleId} (Claude Code) or agent auto-reads file (others)
5. Frontend tracks lastAppliedRoleRef[channelKey] to avoid redundant role setting
6. Session resets on role change (clear bridgeSessionId)
```

### Role File Paths (per agent)

| Agent | Strategy | Role Path | CLI Injection |
|-------|----------|-----------|--------------|
| OpenClaw | Overwrite single file | `~/.openclaw/workspace/SOUL.md` | `--agent main` (always main) |
| Claude Code | Per-role coexisting files | `~/.claude/agents/{role_id}.md` | `--agent {YAML name}` |
| ZeroClaw | Per-role directories | `~/.zeroclaw/workspace/skills/{role_id}/SKILL.md` | None (auto-discovery) |
| NanoBot | Overwrite single file | `~/.nanobot/workspace/AGENTS.md` | None (auto-read) |
| PicoClaw | Overwrite single file | `~/.picoclaw/workspace/AGENT.md` | None (mtime tracking) |
| Hermes | Overwrite single file | `~/.hermes/SOUL.md` | None (auto-read each session) |

> **Claude Code is unique**: `--agent` matches by the **YAML frontmatter `name:` field** inside the `.md` file, NOT the filename. For example, `~/.claude/agents/narrative-designer.md` contains `name: 叙事设计师`, so `--agent 叙事设计师` works but `--agent narrative-designer` does NOT. Frontend passes `role.name` (display name from roles JSON) directly as `agent_name` in the Bridge JSON — this bypasses Bridge's ACTIVE_ROLE and provides the correct YAML name without requiring Bridge to parse the file.

### Role Source (Upstream)

- EN: [msitarzewski/agency-agents](https://github.com/msitarzewski/agency-agents)
- ZH: [jnMetaCode/agency-agents-zh](https://github.com/jnMetaCode/agency-agents-zh)
- URLs stored as `filePath` in `roles-{lang}.json` (full URL, no construction needed)

---

## Session Management

### Session ID Flow

```
1. First message: no session_id → Bridge creates new session
2. Agent response includes session_id → stored in frontend as bridgeSessionId
3. Subsequent messages: pass session_id → Bridge/channel_commands.rs uses resumeArgs
4. Role change: clear bridgeSessionId → forces new session
```

### Local vs Remote Session Handling

| Aspect | Local | Remote |
|--------|-------|--------|
| Session storage | Bridge subprocess state | Frontend `bridgeSessionId` |
| Resume args | Bridge reads plugin.json | channel_commands.rs reads plugin.json, substitutes `{sessionId}` |
| Agent arg | Frontend passes `role.name` as `agent_name` in JSON (overrides ACTIVE_ROLE) | Frontend passes `role.name` as `agent_name` in JSON + `--agent-arg` flag |

---

## Output Parsing

### Bridge Protocol (stdout)

```json
{"type":"status","agent":"claude","version":"3.2.0","ready":true}
{"type":"text","text":"Hello!","session_id":"abc-123"}
{"type":"done","session_id":"abc-123"}
```

### Parsing Logic (channel_commands.rs)

```rust
for line in result.stdout.lines() {
    if let Ok(json) = serde_json::from_str(line) {
        match json["type"] {
            "text" => extract text + session_id,
            "done" => extract session_id,
            "error" => return Err(message),
        }
    }
}
// Fallback: only dump raw stdout when NO valid Bridge JSON was parsed
if response_text.is_empty() && !parsed_bridge_json && !result.stdout.is_empty() {
    response_text = result.stdout.clone();  // Bridge completely failed
}
```

### Agent-Specific Output Quirks

| Agent | Output Format | Cleanup Needed |
|-------|--------------|---------------|
| OpenClaw | JSON with `result.payloads[].text` + `meta.agentMeta.sessionId` | Bridge unwraps |
| Claude Code | JSON with `result` string + `session_id` | Bridge unwraps |
| ZeroClaw | ANSI color codes + log lines | Bridge strips |
| NanoBot | Clean text with cat emoji delimiter | Bridge strips |
| PicoClaw | ANSI + Go structured logs + `<think>` blocks | Bridge strips |
| Hermes | Clean text (with `--quiet` flag) | None needed |

---

## Pitfalls & Lessons Learned (v3.2.0)

### 1. Claude Code `--agent` Uses YAML Name, NOT Filename

> ⚠️ This is the most critical lesson — cost hours of debugging.

- `~/.claude/agents/narrative-designer.md` has YAML frontmatter: `name: 叙事设计师`
- `claude agents list` shows agents by their YAML `name`, not filename
- `--agent narrative-designer` → **SILENT FAILURE** (agent not found, Claude uses default behavior)
- `--agent 叙事设计师` → **WORKS** (correct YAML name match)
- **Solution**: Frontend passes `role.name` (display name from roles JSON) as `agent_name` in Bridge JSON. This value matches the YAML `name` field because both come from the same upstream source. No Bridge recompilation needed — JSON `agent_name` overrides Bridge's ACTIVE_ROLE.

### 2. `--resume` ≠ `--session-id` (Claude Code)

- `--session-id {uuid}` → Create **NEW** session with specific UUID (NOT resume!)
- `--resume {uuid}` → Resume an **EXISTING** session by its UUID
- Bridge `--command` mode hardcodes `--session-id` in resume_args
- **Fix**: `channel_commands.rs` uses plugin.json's `resumeArgs` (which has `--resume`) instead of Bridge's hardcoded default

### 3. Raw JSON Fallback Must Check `parsed_bridge_json`

Only fall back to raw stdout when Bridge returned NO valid protocol JSON lines (`parsed_bridge_json = false`). Previously, empty text + valid JSON would dump raw protocol to chat bubbles.

### 4. Session Must Reset on Role Change

OpenClaw (and others) read role files only at session start. Changing SOUL.md mid-session has no effect. After `set_role`, clear `bridgeSessionId` to force new session. Track with `lastAppliedRoleRef`.

### 5. Windows Encoding

Always use `safe_truncate()` for log messages containing multi-byte UTF-8 (Chinese). Slicing at byte boundaries panics in Rust.

### 6. Remote Bridge Auto-Deploy

`ensure_remote_bridge()` checks version match — if remote Bridge is older than local, it auto-deploys via SCP. Bridge version is tied to Echobird version.

### 7. Bridge Recompilation is Expensive

Bridge has 5 cross-platform binaries. Avoid changes that require Bridge recompilation when a Tauri-side or frontend-side fix is possible. Example: passing `role.name` from frontend is better than having Bridge parse YAML from role files.

### 8. Chat Scroll: Bottom-Anchored by Default (Channels + MotherAgent)

> **Rule**: The chat view MUST stay at bottom at all times. This is the default state. The ONLY exception is when the user manually scrolls up.

- **User sends message** → auto-scroll to bottom
- **Agent response arrives (incremental)** → keep scrolling to bottom as messages grow
- **Open/switch channel** → scroll to bottom
- **Re-open the page** → scroll to bottom

**Implementation**: `isProgrammaticScrollRef` flag prevents `handleScroll` from overriding `autoFollowRef` during programmatic `scrollIntoView` calls. All scroll-to-bottom calls go through `doScrollToBottom()` helper which sets the flag, scrolls, then clears it after 100ms.

---

## Key Source Files

| File | Purpose |
|------|---------|
| `bridge-src/src/main.rs` | Bridge binary — protocol handling, execute_chat, parse_agent_output, extract_yaml_name |
| `src-tauri/src/commands/channel_commands.rs` | All Tauri commands: chat, set_role, detect_agents, model read/write |
| `src-tauri/src/services/plugin_manager.rs` | Plugin config loading, bridge path resolution |
| `src/pages/Channels.tsx` | Frontend UI — role selection, message sending, chat display |
| `src/api/tauri.ts` | Frontend API wrappers for all bridge commands |
| `plugins/{agent}/plugin.json` | Per-agent CLI config |
| `.agent/workflows/bridge-cli.md` | Architecture reference workflow |
| `internal-docs/TOOLS.md` | Tool system architecture (App Manager page) |
