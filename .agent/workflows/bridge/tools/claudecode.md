---
description: Claude Code Bridge integration — config, role injection, cli-oneshot specifics, pitfalls
---

# Claude Code — Bridge Integration Guide

> Last verified: 2026-03-22 | Bridge v3.2.2

## Configuration

### plugin.json (`plugins/claudecode/plugin.json`)
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
> `--dangerously-skip-permissions` is **required** — without it, claude prompts for interactive "trust folder" confirmation, hanging the automated CLI.

---

## Model Switching

Claude Code uses its own API key management. Bridge `handle_set_model("claudecode")` writes `~/.claude/settings.json`.

---

## Role Injection

### File path
```
~/.claude/agents/{role_id}.md   (per-role files, coexist)
```

Unlike shared-file agents (OpenClaw, PicoClaw), Claude Code supports **multiple coexisting role files**. Each role creates a separate `.md` file.

### CLI integration
Claude Code natively supports `--system-prompt-file` to point to a role file. This is the correct approach. The `--agent` flag is for subagent delegation (NOT persona loading) and causes hangs.

### YAML name extraction
Claude Code's `--agent` flag expects the `name` from the YAML frontmatter, which may differ from the filename. Bridge extracts the YAML `name` field from the role file.

---

## Output Format

With `--output-format json`, Claude Code returns structured JSON:
```json
{
  "result": "plain text response",
  "session_id": "abc-123",
  "usage": { ... }
}
```

Or nested format:
```json
{
  "result": { "content": [{ "text": "response text" }] }
}
```

Bridge `parse_agent_output()` handles both formats.

---

## Pitfalls & Solutions

### 1. `--agent` flag hangs in `-p` mode
`--agent` is for subagent delegation, NOT persona loading. Causes indefinite hang. Use `--system-prompt-file` instead.

### 2. `--system-prompt-file` is the correct approach
Point to the downloaded `.md` role file directly. Path must be absolute (e.g. `C:\Users\eben\.claude\agents\game-designer.md`).

### 3. Role state managed by Bridge (not Rust backend)
In the old architecture, `ONESHOT_STATE.active_role_id` tracked the active role in the Rust backend. This has been removed — Bridge now manages role state internally via `ACTIVE_ROLE`. The `set_role` JSON command writes the role file and stores the active role ID.

### 4. Raw JSON displayed instead of text
cli-oneshot responses bypass Bridge's `parse_agent_output()`. Must add Claude Code format (`result` field) parsing to response handler.

### 5. `&a[..30]` panics on Chinese text
Rust string indexing by byte offset crashes on multi-byte UTF-8 chars. Always use `safe_truncate()` or `str::is_char_boundary()`.

### 6. `--output-format json` is required
Without it, Claude Code returns plain text. With it, returns structured JSON with `result`, `session_id`, `usage`.

### 7. Multiple agent files in `~/.claude/agents/`
Claude Code auto-scans this directory. N roles = N files. Each chat must specify which one via `--system-prompt-file`.

---

## File Index

| File | Purpose |
|------|---------|
| `plugins/claudecode/plugin.json` | CLI config, cli-oneshot, detection |
| `tools/claudecode/paths.json` | Detection paths |
| `bridge-src/src/main.rs` | Bridge protocol: role file management |
