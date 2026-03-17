---
description: Architecture reference for Echobird CLI Bridge — how it works, directory layout, role deployment, and adding new agent support
---

# Echobird CLI Bridge Workflow

## What is Bridge?

Bridge is the **communication layer** between the EchoBird desktop app and AI agents (OpenClaw, Claude Code, OpenCode, ZeroClaw). It translates EchoBird's JSON protocol into CLI commands for each agent.

## Two Directories — Why?

```
bridge/          ← 5 pre-compiled binaries (win/linux/darwin × arch), bundled in installer (~8 MB)
bridge-src/      ← Rust source code + Cargo.toml + target/, used only for development & CI compilation
```

- `bridge/` is listed in `tauri.conf.json` → `bundle.resources` so all binaries ship with the app
- `bridge-src/` is NOT bundled — it stays local for development and CI builds only
- **NEVER put source code or `target/` back into `bridge/`** — it would bloat the installer by hundreds of MB

## How Bridge Works

### Local Channel Flow
```
User sends message in Channels page
  → Channels.tsx calls bridgeSetRoleLocal() (if role selected)
  → Channels.tsx calls bridgeChatLocal(text, sessionId)
  → Rust backend (channel_commands.rs) writes JSON to bridge subprocess stdin
  → bridge-win.exe receives {"type":"set_role",...} or {"type":"chat",...}
  → Bridge invokes: openclaw agent --json --agent main --message "..."
  → Bridge parses OpenClaw JSON output → returns via stdout
  → Rust backend reads response → returns to frontend
```

### Remote Channel Flow
```
User sends message to remote server
  → Channels.tsx calls bridgeSetRoleRemote(serverId, ...)
  → Channels.tsx calls bridgeChatRemote(serverId, text, sessionId)
  → Rust backend pipes JSON through SSH to ~/echobird/echobird-bridge on remote
  → Same bridge binary on remote handles set_role / chat
  → Response comes back through SSH pipe → frontend
```

### Bridge JSON Protocol (stdin → stdout)

```json
// Ping
→ {"type": "ping"}
← {"type": "pong"}

// Detect installed agents
→ {"type": "detect_agents"}
← {"type": "agents_detected", "agents": [{"id":"openclaw","installed":true,"running":true}]}

// Set role (downloads role file and writes to agent's config directory)
→ {"type": "set_role", "agent_id": "openclaw", "role_id": "ai-engineer", "url": "https://echobird.ai/docs/roles/en/ai-engineer.md"}
← {"type": "role_set", "agent_id": "openclaw", "role_id": "ai-engineer", "installed": true, "path": "~/.openclaw/agency-agents/ai-engineer/SOUL.md"}

// Chat
→ {"type": "chat", "message": "Hello", "session_id": "abc-123"}
← {"type": "text", "text": "Hi!", "session_id": "abc-123"}
← {"type": "done", "session_id": "abc-123"}

// Clear role
→ {"type": "clear_role", "agent_id": "openclaw", "role_id": "ai-engineer"}
← {"type": "role_cleared", "agent_id": "openclaw", "role_id": "ai-engineer", "success": true}

// Start agent process
→ {"type": "start_agent", "agent_id": "openclaw"}
← {"type": "agent_started", "agent_id": "openclaw", "success": true}
```

## Role File Deployment

Bridge does NOT inject roles as system prompts. It **downloads and writes role files to each agent's config directory**:

| Agent | Role file path | Format |
|-------|---------------|--------|
| OpenClaw | `~/.openclaw/agency-agents/{role_id}/SOUL.md` | Markdown |
| Claude Code | `~/.claude/agents/{role_id}.md` | Markdown |
| OpenCode | `~/.config/opencode/agents/{role_id}.md` | Markdown |
| ZeroClaw | `~/.zeroclaw/workspace/skills/{role_id}/SKILL.md` | Markdown |

Role URLs follow pattern: `https://echobird.ai/docs/roles/{lang}/{filePath}`
- `lang` = `en` or `zh-Hans` based on user locale
- `filePath` = relative path from `roles-en.json`, e.g. `engineering/engineering-ai-engineer.md`
- CDN-accelerated (GFW-friendly), backed by upstream repos:
  - EN: `msitarzewski/agency-agents`
  - ZH: `jnMetaCode/agency-agents-zh`

## Key Source Files

| File | Purpose |
|------|---------|
| `bridge-src/src/main.rs` | Bridge binary source (845 lines) — all protocol handling |
| `src-tauri/src/services/plugin_manager.rs` | `get_bridge_path()`, `bridge_dir()`, `plugins_dir()` |
| `src-tauri/src/commands/channel_commands.rs` | All bridge Tauri commands (start/stop/chat/set_role) |
| `src/api/tauri.ts` | Frontend API wrappers for bridge commands |
| `src/pages/Channels.tsx` | Frontend channel UI — calls bridge APIs |
| `plugins/{agent}/plugin.json` | Per-agent CLI config (args, protocol, detection) |
| `tauri.conf.json` | Bundle config — `resources` includes `../bridge/` |

## Plugin Protocols

Each agent uses one of two protocols defined in `plugin.json`:

- **`stdio-json`** (OpenClaw): Bridge runs as persistent subprocess, communicates via stdin/stdout JSON
- **`cli-oneshot`** (Claude Code, OpenCode, ZeroClaw): Bridge invokes CLI once per message, no persistent process

## Adding a New Agent

To add support for a new AI agent tool:

1. **Create plugin config**: `plugins/{agent-id}/plugin.json`
   ```json
   {
     "id": "newagent",
     "name": "New Agent",
     "protocol": "cli-oneshot",
     "cli": {
       "command": "newagent",
       "detectCommand": "newagent --version",
       "args": ["--non-interactive", "--message"],
       "systemPromptArg": "--system-prompt"
     }
   }
   ```

2. **Add to bridge source** (`bridge-src/src/main.rs`):
   - Add to `KNOWN_AGENTS` array (for detect_agents)
   - Add path mapping in `handle_set_role()` (for role deployment)
   - Add path mapping in `handle_clear_role()` (for role cleanup)
   - Add to `handle_start_agent()` match

3. **Recompile bridge** for all 5 platforms (CI does this)

4. **Add to AGENT_LIST** in frontend (`src/pages/Channels.tsx` or agent config)

5. **Add role URL** on CDN: `https://echobird.ai/docs/roles/{lang}/{role_id}.md`

## Offline Remote Deployment

When remote server has no internet:
1. Mother Agent detects remote OS/arch via `uname -s -m`
2. Maps to correct binary from local `bridge/` directory
3. SCPs binary to `~/echobird/echobird-bridge` on remote via `upload_file` tool
4. Sets executable permissions: `chmod +x ~/echobird/echobird-bridge`
5. Starts bridge: `nohup ~/echobird/echobird-bridge > /dev/null 2>&1 &`

## Bridge Binary Management

> **CRITICAL RULE: NEVER develop bridge for a single platform (e.g. Windows only) or a single agent (e.g. OpenClaw only). Bridge is ALL platforms × ALL agents from day one.**

### Where do the 5 binaries come from?

The 5 binaries in `bridge/` are from the **last CI build**. If bridge source has NOT changed, simply **copy them from the previous release** — no compilation needed.

### Workflow when bridge source changes

1. Modify `bridge-src/src/main.rs` (e.g. add new agent support)
2. Push to GitHub → CI cross-compiles all 5 targets automatically
3. CI outputs go to `bridge/` (replacing old binaries)
4. Next EchoBird release ships with the new binaries

### Workflow when bridge source has NOT changed

Just keep the existing 5 binaries in `bridge/` — they ship as-is with the next release. **Do NOT recompile locally.**

### CI cross-compilation targets

```
bridge-win.exe         ← x86_64-pc-windows-msvc
bridge-linux-x86_64    ← x86_64-unknown-linux-gnu
bridge-linux-aarch64   ← aarch64-unknown-linux-gnu
bridge-darwin-x86_64   ← x86_64-apple-darwin
bridge-darwin-aarch64  ← aarch64-apple-darwin
```

### Rules

- **No source changes = no recompilation.** Just reuse the 5 binaries from last version.
- Bridge version is tied to EchoBird version — recompile only when protocol changes.
- Tauri bundles `bridge/` via `tauri.conf.json` → `resources`.
- `bridge-src/` is NOT bundled — development and CI only.
