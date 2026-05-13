<p align="center">
  <img src="docs/icon.png" alt="EchoBird" width="140" />
</p>

<h1 align="center">EchoBird</h1>

<p align="center"><strong>AI deployment, no more chicken-and-egg.</strong></p>
<p align="center"><sub>AI 部署,不再是先有鸡还是先有蛋。</sub></p>

<p align="center">
  <a href="https://github.com/edison7009/EchoBird/releases">
    <img src="https://img.shields.io/github/v/release/edison7009/EchoBird?style=flat-square&color=D97757" alt="Release" />
  </a>
  <img src="https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-blue?style=flat-square" alt="Platform" />
  <img src="https://img.shields.io/badge/built%20with-Tauri%20%2B%20Rust-orange?style=flat-square" alt="Tauri + Rust" />
  <img src="https://img.shields.io/badge/license-MIT-green?style=flat-square" alt="MIT License" />
</p>

<p align="center">
  <a href="https://echobird.ai">Website</a> ·
  <a href="https://github.com/edison7009/EchoBird/releases/latest">Download</a> ·
  <a href="README.zh-CN.md">中文 README</a>
</p>

---

## What is EchoBird?

Friends kept asking me to install **Claude Code**, **OpenClaw**, **Hermes Agent**… every machine was different, and some refused to pay for an LLM. Setup and explanations took forever. So I built **EchoBird** — an Agent inspired by **Songbird**, the female hacker in *Cyberpunk 2077* who fixes any tech problem for V. EchoBird installs every AI tool with one click.

## Highlights

- **One-click install** for Claude Code, OpenClaw, Hermes Agent, and more
- **Bring your own model** — OpenAI / Anthropic / local LLMs / relays, all in one place
- **Speed-test every model** in one click before you commit
- **Local LLM runtime** built in — pick a quant, hit START
- **Install & Repair Agent** — talk to it like a colleague when something breaks
- **Cross-platform** — Windows, macOS, Linux (x64 + arm64)

## Screenshots

### AI News & Star Projects — your daily AI brief

> Day & night, side by side — the rest of the screenshots below follow your GitHub theme.

<table>
<tr>
  <td width="50%"><img src="docs/screenshots/news-en-light.png" alt="AI News (Light)" /></td>
  <td width="50%"><img src="docs/screenshots/news-en-dark.png" alt="AI News (Dark)" /></td>
</tr>
<tr>
  <td align="center"><sub>☀️ Light theme</sub></td>
  <td align="center"><sub>🌙 Dark theme</sub></td>
</tr>
</table>

### Model Nexus — manage every model, speed-test in one click

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/screenshots/model-en-dark.png">
  <img alt="Model Nexus" src="docs/screenshots/model-en-light.png" width="100%">
</picture>

### App Manager — one-click launch, switch any tool's model in seconds

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/screenshots/app-en-dark.png">
  <img alt="App Manager" src="docs/screenshots/app-en-light.png" width="100%">
</picture>

### Local LLM — run models on your own machine

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/screenshots/localllm-en-dark.png">
  <img alt="Local LLM" src="docs/screenshots/localllm-en-light.png" width="100%">
</picture>

### Install & Repair Agent — chat-driven setup and troubleshooting

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/screenshots/agent-en-dark.png">
  <img alt="Install & Repair Agent" src="docs/screenshots/agent-en-light.png" width="100%">
</picture>

## Install

### One-line install

**Windows** (PowerShell)

```powershell
irm https://echobird.ai/install.ps1 | iex
```

**macOS / Linux**

```sh
curl -fsSL https://echobird.ai/install.sh | sh
```

The script auto-detects your OS, downloads the right package, and skips if you're already on the latest version.

### Or download a package

Latest release → <https://github.com/edison7009/EchoBird/releases/latest>

| Platform | Asset |
|---|---|
| Windows x64 | `EchoBird_<ver>_Windows_x64-setup.exe` |
| macOS (Apple Silicon) | `EchoBird_<ver>_macOS_arm64.dmg` |
| Linux x64 · Debian/Ubuntu | `EchoBird_<ver>_Linux_x64.deb` |
| Linux arm64 · Debian/Ubuntu | `EchoBird_<ver>_Linux_arm64.deb` |
| Linux x64 · Fedora/RHEL | `EchoBird_<ver>_Linux_x64.rpm` |
| Linux arm64 · Fedora/RHEL | `EchoBird_<ver>_Linux_arm64.rpm` |

## Architecture

EchoBird is built with a modern, layered architecture:

```
┌─────────────────────────────────────────────────────────────┐
│                     Frontend (React + TypeScript)            │
│  ┌──────────┬──────────┬──────────┬──────────┬──────────┐  │
│  │ AI News  │  Model   │   App    │  Local   │  Agent   │  │
│  │  Pulse   │  Nexus   │ Manager  │   LLM    │  Chat    │  │
│  └──────────┴──────────┴──────────┴──────────┴──────────┘  │
└─────────────────────────────────────────────────────────────┘
                            ↕ Tauri IPC
┌─────────────────────────────────────────────────────────────┐
│                    Backend (Rust + Tauri)                    │
│  ┌──────────────────────────────────────────────────────┐  │
│  │  Commands Layer (tool_commands, model_commands, etc) │  │
│  └──────────────────────────────────────────────────────┘  │
│  ┌──────────────────────────────────────────────────────┐  │
│  │  Services Layer                                       │  │
│  │  • tool_manager      • model_manager                  │  │
│  │  • agent_loop        • llm_client                     │  │
│  │  • local_llm         • process_manager                │  │
│  └──────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────┘
                            ↕
┌─────────────────────────────────────────────────────────────┐
│                    Tools & Integrations                      │
│  • Claude Code    • Codex CLI      • OpenClaw               │
│  • Hermes Agent   • Aider          • Cursor                 │
│  • Local LLM      • Embedded Tools (Reversi, AI Trader)     │
└─────────────────────────────────────────────────────────────┘
```

### Key Components

- **Frontend**: React + TypeScript + Tailwind CSS + Zustand
- **Backend**: Rust + Tauri 2.0
- **IPC**: Tauri's type-safe command system
- **Tools**: Standardized config format in `tools/` directory
- **Local LLM**: Built-in llama.cpp server with GPU acceleration

### Data Flow

1. **User Action** → Frontend component
2. **Tauri Command** → Rust backend handler
3. **Service Logic** → Business logic execution
4. **External Tool** → Config file modification or process spawn
5. **Response** → Frontend state update

For more details, see:
- [CONTRIBUTING.md](CONTRIBUTING.md) - Development guide
- [tools/README.md](tools/README.md) - Tool integration guide
- [tools/codex/README.md](tools/codex/README.md) - Codex proxy architecture

## License

MIT — see [LICENSE](LICENSE).

---

<p align="center">
  Made with 💚 by EchoBird Team<br>
  <sub>⭐ <a href="https://github.com/edison7009/EchoBird">Star on GitHub</a> · <a href="README.zh-CN.md">中文文档</a></sub>
</p>
