---
description: Architecture reference for Echobird CLI Bridge — how it works, directory layout, role deployment, and adding new agent support
---

# Echobird CLI Bridge Workflow

## What is Bridge?

Bridge is the **communication layer** between the EchoBird desktop app and AI agents (OpenClaw, Claude Code, ZeroClaw, NanoBot, PicoClaw, Hermes Agent). It translates EchoBird's JSON protocol into CLI commands for each agent.

> **UPSTREAM FIRST**: When encountering issues with agents, ALWAYS check these references BEFORE developing custom solutions:
> 1. **https://github.com/msitarzewski/agency-agents** — upstream agent files, usage patterns, how --agent works
> 2. Each agent's official docs/GitHub (e.g. `claude --help`, OpenClaw docs)
> 3. Do NOT invent custom injection methods (--system-prompt, --system-prompt-file) without verifying the official approach first

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
  → Rust backend writes JSON to Bridge subprocess stdin
  → Bridge subprocess (started with --config plugins/{agent}/plugin.json)
  → Bridge reads plugin.json to determine CLI command (openclaw/claude/zeroclaw/...)
  → Bridge invokes: {agent CLI} {args} --message "..."
  → Bridge parses output → returns JSON via stdout
  → Rust backend reads response → returns to frontend
```

> **KEY PRINCIPLE**: Bridge is the **sole communication layer** for ALL agents,
> both local and remote. The Tauri backend NEVER calls agent CLIs directly.
> Adding a new agent only requires a new `plugins/{agent_id}/plugin.json`.


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
→ {"type": "set_role", "agent_id": "openclaw", "role_id": "ai-engineer", "url": "https://echobird.ai/roles/en/engineering/engineering-ai-engineer.md"}
← {"type": "role_set", "agent_id": "openclaw", "role_id": "ai-engineer", "installed": true, "path": "~/.openclaw/workspace/SOUL.md"}

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
| OpenClaw | `~/.openclaw/workspace/SOUL.md` (overwrites main agent) | Markdown |
| Claude Code | `~/.claude/agents/{role_id}.md` | Markdown |
| ZeroClaw | `~/.zeroclaw/workspace/skills/{role_id}/SKILL.md` | Markdown |
| NanoBot | `~/.nanobot/workspace/AGENTS.md` (overwrites) | Markdown |
| PicoClaw | `~/.picoclaw/workspace/AGENT.md` (overwrites) | Markdown |
| Hermes Agent | `~/.hermes/SOUL.md` (overwrites) | Markdown |

Role URLs are stored as **full URLs** in each `roles-{lang}.json`:
- `filePath` = complete URL, e.g. `https://echobird.ai/roles/en/engineering/engineering-ai-engineer.md`
- `img` = complete URL, e.g. `https://echobird.ai/roles/en/engineering/engineering-ai-engineer.png`
- Frontend uses `role.filePath` directly as the download URL — **no hardcoded language routing**
- Adding a new language: just create `docs/roles/roles-{lang}.json` with full URLs — zero code changes
- Locale → JSON mapping is automatic: `resolveLocaleFileName()` derives `roles-{prefix}.json` from locale, falls back to `roles-en.json`
- Upstream repos:
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
- **`cli-oneshot`** (Claude Code, ZeroClaw, NanoBot, PicoClaw, Hermes Agent): Bridge invokes CLI once per message, no persistent process

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

4. **Agent list is now dynamic** — loaded from `plugins/` at runtime. No hardcoded AGENT_LIST needed.

5. **Add role URL** on CDN: `https://raw.githubusercontent.com/edison7009/Echobird-MotherAgent/main/docs/roles/{lang}/{role_id}.md`

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
3. **CI builds bridge FIRST, copies to `bridge/`, THEN builds Tauri app** (order matters!)
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

## Lessons Learned (v3.0.0 – v3.0.4)

> Hard-won experience from debugging role deployment. **Read before adding any new agent.**

### 1. CI Build Order: Bridge BEFORE Tauri

Tauri bundles `bridge/` as resources during build. If bridge is compiled AFTER Tauri, the installer ships stale binaries. **Always: `cd bridge-src && cargo build` → copy to `bridge/` → THEN `tauri-action`.**

### 2. Role URL Must Return Raw Markdown

`echobird.ai/roles/*.md` returns raw markdown via Cloudflare Pages. Each role's `filePath` in the JSON is already the full download URL (e.g. `https://echobird.ai/roles/en/engineering/...`). The frontend passes this URL directly — no URL construction needed. If Cloudflare ever renders HTML for `.md` files, switch to `raw.githubusercontent.com`.

### 3. OpenClaw SOUL.md: Always Overwrite

OpenClaw ships with a **default SOUL.md** in `~/.openclaw/workspace/`. If bridge checks `if file.exists() { skip }`, it will never deploy the user's role. For agents where roles share the same target file (like OpenClaw's SOUL.md), always overwrite.

### 4. Session Must Reset on Role Change

OpenClaw reads SOUL.md only at **session start**. Changing SOUL.md mid-session has no effect. After `set_role` succeeds, clear `bridgeSessionId` to force a new session. But only when the role actually changes — use `lastAppliedRoleRef` to track.

### 5. Don't Create New Agents

`openclaw agents add {role_id}` creates a new agent workspace but with **no model/API config**. Only the `main` agent has the user's configured models. Write SOUL.md to `~/.openclaw/workspace/SOUL.md` (main workspace) and keep `--agent main`.

### 6. bridge-src/ Compiles, bridge/ Ships

- `bridge-src/` = Rust source + Cargo.toml. CI compiles here.
- `bridge/` = 5 compiled binaries. Tauri bundles these into the installer.
- **CI copies compiled output from `bridge-src/target/` to `bridge/`** before Tauri build.
- These binaries serve local channel communication AND offline LAN deployment.

### 7. Per-Agent Role Path Differences

| Agent | Strategy | Why |
|-------|----------|-----|
| OpenClaw | Overwrite `workspace/SOUL.md` | Single workspace, roles share same file |
| Claude Code | Create `agents/{role_id}.md` | Per-role files, coexist |
| ZeroClaw | Create `skills/{role_id}/SKILL.md` | Per-role directories, coexist |
| NanoBot | Overwrite `workspace/AGENTS.md` | Single workspace, roles share same file |
| PicoClaw | Overwrite `workspace/AGENT.md` | Single workspace, mtime-tracked |
| Hermes Agent | Overwrite `SOUL.md` | Auto-read on each `hermes chat` |

For agents with per-role files, idempotent skip (`if exists, skip download`) is safe. For agents with shared files (OpenClaw, NanoBot, PicoClaw, Hermes), always overwrite.

### 8. Role filePath Is a Full URL

`filePath` in each `roles-{lang}.json` is a **complete URL** (e.g. `https://echobird.ai/roles/en/engineering/engineering-ai-engineer.md`). The frontend uses it directly: `const roleUrl = role.filePath;` — no URL construction, no language prefix logic. Adding a new language only requires creating a new `roles-{lang}.json` file with full URLs.
