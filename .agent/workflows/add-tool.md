---
description: How to add a new tool to App Manager (plug-and-play, no code changes)
---

# Adding a New Tool to App Manager

Tools are **plug-and-play**: just create a directory under `tools/` with two JSON files. No Rust or TypeScript code changes are needed.

## Steps

1. Create directory `tools/{toolid}/` (lowercase, no hyphens)
2. Add tool icon to `public/icons/tools/{toolid}.svg` (fallback: `.png`)
3. Create `tools/{toolid}/paths.json` — detection & metadata
4. Create `tools/{toolid}/config.json` — model config mapping
5. Click **REFRESH** in App Manager — the tool appears automatically

## paths.json Template

```json
{
    "name": "ToolName",
    "category": "AgentOS|IDE|CLI|AutoTrading|Game|Utility",
    "apiProtocol": ["openai", "anthropic"],
    "docs": "https://...",
    "website": "https://...",
    "command": "tool-cli-command",
    "startCommand": "tool-cli-command start",
    "envVar": "TOOL_PATH",
    "configDir": "~/.toolname",
    "configFile": "~/.toolname/config.json",
    "requireConfigFile": false,
    "detectByConfigDir": false,
    "paths": {
        "win32": ["%APPDATA%/npm/tool.cmd"],
        "darwin": ["/usr/local/bin/tool"],
        "linux": ["/usr/local/bin/tool"]
    },
    "skillsPath": {
        "darwin": ["~/.toolname/skills"],
        "linux": ["~/.toolname/skills"]
    }
}
```

### Key fields

| Field | Required | Description |
|---|---|---|
| `name` | ✅ | Display name |
| `category` | ✅ | One of: `AgentOS`, `IDE`, `CLI`, `AutoTrading`, `Game`, `Utility` |
| `paths` | ✅ | Platform-specific executable paths for detection |
| `command` | ❌ | CLI command name (for PATH detection + version check) |
| `startCommand` | ❌ | Command to launch the tool (if different from `command`) |
| `detectByConfigDir` | ❌ | Set `true` for git-clone-installed tools (detect by directory existence) |
| `configDir` | ❌ | Config directory path (used with `detectByConfigDir`) |
| `requireConfigFile` | ❌ | If `true`, tool only shows as installed when config file exists |
| `skillsPath` | ❌ | Where to find installed skills |
| `names` | ❌ | i18n display names: `{"zh-CN": "...", "ja": "..."}` |
| `launchFile` | ❌ | For launchable tools (games) — HTML file relative to tool dir |

## config.json — Two Modes

### Mode A: Echobird Relay (custom tools)

For tools that don't have their own config file, or use a proprietary format. Model info is written to `~/.echobird/{toolid}.json`.

```json
{
    "configFile": "~/.echobird/toolname.json",
    "format": "json",
    "custom": true
}
```

> **IMPORTANT**: Setting `"custom": true` routes the tool through the Echobird relay automatically. No Rust code changes needed.

### Mode B: Generic JSON Mapping

For tools with their own JSON config file (like Claude Code). Define read/write field mappings.

```json
{
    "configFile": "~/.tool/settings.json",
    "format": "json",
    "read": {
        "model": ["env.MODEL_NAME"],
        "baseUrl": ["env.API_BASE_URL"],
        "apiKey": ["env.API_KEY"]
    },
    "write": {
        "env.MODEL_NAME": "model",
        "env.API_BASE_URL": "baseUrl",
        "env.API_KEY": "apiKey"
    }
}
```

## ⚠️ Rules

- **NEVER hardcode tool IDs** in `tool_config_manager.rs` or any Rust/TS file
- The `custom: true` flag in `config.json` auto-routes to Echobird relay
- Generic JSON mapping (Mode B) works via `config.json` read/write fields
- Only tools with special config formats (YAML/TOML/proprietary) need hardcoded handlers — and those are rare legacy cases
- The `scan_tools()` function clears its cache on every call, so REFRESH always picks up new directories
