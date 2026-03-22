---
description: Monitor 6 supported agents for upstream breaking changes (CLI flags, config format, role injection, output format)
---

# Agent Compatibility Watch

EchoBird integrates 6 external agents. Any upstream CLI/config change could break our integration.
This workflow defines what to monitor and how to respond.

## Monitored Agents & Repos

| Agent | GitHub Repo | Install Package |
|-------|------------|----------------|
| OpenClaw | [openclaw/openclaw](https://github.com/openclaw/openclaw) | `npm i -g openclaw` |
| Claude Code | [anthropics/claude-code](https://github.com/anthropics/claude-code) | `npm i -g @anthropic-ai/claude-code` |
| ZeroClaw | [zeroclaw-labs/zeroclaw](https://github.com/zeroclaw-labs/zeroclaw) | `cargo install zeroclaw` |
| NanoBot | [HKUDS/nanobot](https://github.com/HKUDS/nanobot) | `pip install nanobot-ai` |
| PicoClaw | [sipeed/picoclaw](https://github.com/sipeed/picoclaw) | `go install github.com/sipeed/picoclaw@latest` |
| Hermes | [NousResearch/hermes-agent](https://github.com/NousResearch/hermes-agent) | `pip install hermes-agent` |

## Critical Integration Points

For each agent, these are the things that BREAK EchoBird if changed:

### 1. CLI Interface (affects plugin.json + Bridge)
- **Command name** — e.g. `claude`, `openclaw agent`
- **Chat flags** — e.g. `-p`, `-m`, `--message`, `--output-format json`
- **Resume flags** — e.g. `--resume {sessionId}`, `--session {sid}`
- **Model flag** — e.g. `--model`
- **Agent/role flag** — e.g. `--agent` (Claude Code YAML name matching!)
- **Output format** — JSON structure, field names, delimiters

### 2. Config File Format (affects model switching)
- **File path** — e.g. `~/.claude/settings.json`, `~/.openclaw/openclaw.json`
- **Key names** — e.g. `env.ANTHROPIC_MODEL`, `models.providers`
- **Format** — JSON vs TOML vs YAML

### 3. Role Injection (affects set_role in Bridge)
- **Role file path** — e.g. `~/.claude/agents/{name}.md`
- **File format** — YAML frontmatter fields (`name:`, `description:`)
- **Auto-discovery** — does agent auto-read file or need CLI flag?

### 4. Session Management
- **Session ID format** — UUID vs custom
- **Resume mechanism** — `--resume` vs `--session` vs gateway persistent

## GitHub Actions: Weekly Version Check

// turbo-all

The CI workflow `.github/workflows/agent-compat-watch.yml` runs weekly.
It checks each agent's latest version and compares with our known versions.

### Step 1: Check current known versions

```
cat docs/api/tools/install/*.json | jq -r '.id + ": " + (.install | to_entries | map(.value) | join(", "))'
```

### Step 2: If version bump detected, manually check

For each agent with a new version:

1. Read the CHANGELOG / release notes on GitHub
2. Search for keywords: `breaking`, `flag`, `--`, `config`, `deprecated`, `removed`
3. Check if any of the 4 critical integration points above changed
4. If changes found → open an issue with label `agent-compat`

### Step 3: Test locally

```powershell
# Quick integration test for each agent
claude --version
openclaw --version
zeroclaw --version
nanobot --version
picoclaw --version
hermes --version

# Test chat works
echo "hello" | claude -p --output-format json --dangerously-skip-permissions "test"
```

### Step 4: Update if needed

If breaking changes found:
1. Update `plugins/{agent}/plugin.json` — CLI flags, args, resumeArgs
2. Update Bridge `parse_agent_output()` if output format changed
3. Update `channel_commands.rs` model writing if config format changed
4. Update `docs/api/tools/install/{agent}.json` — install commands
5. Test on all platforms (Bridge cross-compile if needed)
6. Release patch version

## Manual Quick Check (for developers)

When a user reports "agent X stopped working after update":

1. Check agent version: `{agent} --version`
2. Check our plugin.json matches current CLI: `cat plugins/{agent}/plugin.json`
3. Test with our exact command: copy `args` from plugin.json and run manually
4. If mismatch → update plugin.json + test + release
