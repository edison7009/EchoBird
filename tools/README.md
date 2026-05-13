# EchoBird Tools Directory

This directory contains configuration and integration files for all AI tools supported by EchoBird.

## Directory Structure

Each tool has its own subdirectory with standardized configuration files:

```
tools/
├── claudecode/           # Claude Code CLI
│   ├── config.json       # Model configuration mapping
│   └── paths.json        # Installation paths per platform
├── codex/                # Codex CLI with proxy launcher
│   ├── codex-launcher.cjs
│   ├── config.json
│   └── paths.json
├── reversi/              # Embedded game tool
│   ├── config.json
│   └── default-skills/
└── ...                   # Other tools
```

## Tool Types

### 1. CLI Tools (External Binaries)

Tools that are installed as separate executables:

- **claudecode**: Claude Code CLI
- **codex**: Codex CLI (with protocol translation proxy)
- **aider**: Aider AI pair programming
- **cursor**: Cursor IDE
- **openclaw**: OpenClaw agent
- **hermes**: Hermes agent
- **coffeecli**: Coffee CLI
- **qwencode**: Qwen Code
- **opencode**: Open Code
- **openfang**: Open Fang
- **pi**: Pi assistant
- **picoclaw**: Pico Claw
- **nanobot**: Nano Bot

### 2. Desktop Apps

Tools that are desktop applications:

- **claudedesktop**: Claude Desktop
- **codexdesktop**: Codex Desktop
- **geminidesktop**: Gemini Desktop

### 3. Embedded Tools

Tools that run inside EchoBird's webview:

- **reversi**: AI-powered Reversi game
- **ai-trader**: AI trading assistant
- **fingpt**: Financial GPT
- **tradingagents**: Trading agents

## Configuration Files

### `config.json`

Defines how to read and write model configuration for each tool.

**Structure**:
```json
{
  "docs": "https://...",           // Official documentation URL
  "configFile": "~/.tool/config",  // Path to tool's config file
  "format": "json|toml|env",       // Config file format
  "read": {                        // How to read current config
    "model": ["path.to.model"],
    "baseUrl": ["path.to.baseUrl"],
    "apiKey": ["path.to.apiKey"]
  },
  "write": {                       // How to write new config
    "path.to.model": "model",
    "path.to.baseUrl": "baseUrl",
    "path.to.apiKey": "apiKey"
  }
}
```

**Example** (Claude Code):
```json
{
  "docs": "https://docs.anthropic.com/en/docs/claude-code/settings",
  "configFile": "~/.claude/settings.json",
  "format": "json",
  "read": {
    "model": ["env.ANTHROPIC_MODEL"],
    "baseUrl": ["env.ANTHROPIC_BASE_URL"],
    "apiKey": ["env.ANTHROPIC_AUTH_TOKEN", "env.ANTHROPIC_API_KEY"]
  },
  "write": {
    "env.ANTHROPIC_MODEL": "model",
    "env.ANTHROPIC_BASE_URL": "baseUrl",
    "env.ANTHROPIC_AUTH_TOKEN": "apiKey"
  }
}
```

### `paths.json`

Defines installation paths for each platform.

**Structure**:
```json
{
  "win32": {
    "binary": "path/to/binary.exe",
    "config": "path/to/config"
  },
  "darwin": {
    "binary": "path/to/binary",
    "config": "path/to/config"
  },
  "linux": {
    "binary": "path/to/binary",
    "config": "path/to/config"
  }
}
```

**Path Variables**:
- `~`: User home directory
- `$APPDATA`: Windows AppData directory
- `$LOCALAPPDATA`: Windows LocalAppData directory

## How Tools Are Loaded

1. **Scan**: `src-tauri/src/services/tool_manager.rs` scans the `tools/` directory
2. **Parse**: Each `config.json` and `paths.json` is parsed
3. **Detect**: Check if the tool is installed by looking for the binary path
4. **Display**: Show detected tools in the App Manager page
5. **Apply**: When user applies a model, write to the tool's config file

## Adding a New Tool

To add support for a new AI tool:

1. **Create directory**: `tools/your-tool/`

2. **Add `config.json`**:
   ```json
   {
     "docs": "https://your-tool-docs.com",
     "configFile": "~/.your-tool/config.json",
     "format": "json",
     "read": {
       "model": ["model"],
       "baseUrl": ["baseUrl"],
       "apiKey": ["apiKey"]
     },
     "write": {
       "model": "model",
       "baseUrl": "baseUrl",
       "apiKey": "apiKey"
     }
   }
   ```

3. **Add `paths.json`**:
   ```json
   {
     "win32": {
       "binary": "~/.your-tool/bin/your-tool.exe",
       "config": "~/.your-tool/config.json"
     },
     "darwin": {
       "binary": "~/.your-tool/bin/your-tool",
       "config": "~/.your-tool/config.json"
     },
     "linux": {
       "binary": "~/.your-tool/bin/your-tool",
       "config": "~/.your-tool/config.json"
     }
   }
   ```

4. **Test**: Restart EchoBird and check if the tool appears in App Manager

## Special Cases

### Codex CLI

Codex requires a protocol translation proxy because it only speaks Responses API, but most providers only support Chat Completions API.

See [codex/README.md](codex/README.md) for details.

### Embedded Tools

Embedded tools (Reversi, AI Trader, etc.) run inside EchoBird's webview and don't need external binaries.

Their runtime HTML is located in `public/tools/{id}.html`, not in `tools/{id}/`.

## Tool Configuration Formats

### JSON Format
```json
{
  "model": "claude-4-7",
  "baseUrl": "https://api.anthropic.com",
  "apiKey": "sk-..."
}
```

### TOML Format
```toml
model = "claude-4-7"
base_url = "https://api.anthropic.com"
api_key = "sk-..."
```

### Environment Variables
```bash
export MODEL="claude-4-7"
export BASE_URL="https://api.anthropic.com"
export API_KEY="sk-..."
```

## Security

- **API keys are encrypted** before being written to tool config files
- **Encryption key** is stored in the system keychain (Windows Credential Manager, macOS Keychain, Linux Secret Service)
- **Decryption** happens only when launching the tool

## Troubleshooting

### Tool not detected

1. Check if the binary exists at the path specified in `paths.json`
2. Check if the path uses the correct platform (`win32`, `darwin`, `linux`)
3. Check EchoBird logs: `~/.echobird/logs/`

### Model configuration not applied

1. Check if the tool's config file exists
2. Check if the `write` mapping in `config.json` is correct
3. Check if the tool is running (some tools reload config on restart)

### API key not working

1. Check if the API key is encrypted correctly
2. Check if the tool can decrypt the key (some tools don't support encrypted keys)
3. Try applying the model again

## Related Code

- **Tool scanning**: `src-tauri/src/services/tool_manager.rs`
- **Config reading**: `src-tauri/src/services/tool_config_manager.rs`
- **Config writing**: `src-tauri/src/commands/tool_commands.rs`
- **Frontend UI**: `src/pages/AppManager/`

## Contributing

See [CONTRIBUTING.md](../CONTRIBUTING.md) for development guidelines.

When adding a new tool:
1. Test on all platforms (Windows, macOS, Linux)
2. Test with different config formats (JSON, TOML, env)
3. Test encryption/decryption of API keys
4. Add documentation to this README
