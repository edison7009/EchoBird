<p align="center">
  <img src="build/icon.png" alt="Echobird" width="120" />
</p>

<h1 align="center">Echobird</h1>

<p align="center">
  The Nexus for <strong>Models</strong>, <strong>Agents</strong> &amp; <strong>Vibe Coding</strong>.<br/>
  <sub>A cyberpunk control panel for the AI era — built with Tauri + Rust.</sub>
</p>

<p align="center">
  <a href="https://github.com/edison7009/Echobird-MotherAgent/releases">
    <img src="https://img.shields.io/github/v/release/edison7009/Echobird-MotherAgent?style=flat-square&color=00FF9D" alt="Release" />
  </a>
  <img src="https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-blue?style=flat-square" alt="Platform" />
  <img src="https://img.shields.io/badge/built%20with-Tauri%20%2B%20Rust-orange?style=flat-square" alt="Tauri + Rust" />
</p>

<p align="center">
  <strong>English</strong> ·
  <a href="./docs/README.zh-CN.md">简体中文</a> ·
  <a href="./docs/README.zh-TW.md">繁體中文</a> ·
  <a href="./docs/README.ja.md">日本語</a> ·
  <a href="./docs/README.ko.md">한국어</a> ·
  <a href="./docs/README.es.md">Español</a> ·
  <a href="./docs/README.fr.md">Français</a> ·
  <a href="./docs/README.de.md">Deutsch</a> ·
  <a href="./docs/README.pt.md">Português</a> ·
  <a href="./docs/README.ru.md">Русский</a> ·
  <a href="./docs/README.ar.md">العربية</a>
</p>

---

## 🤖 MotherAgent — The Star Feature

**MotherAgent** is Echobird's built-in autonomous AI agent. Unlike simple chat interfaces, MotherAgent can:

- 🧠 **Think and plan** — breaks down complex tasks into actionable steps
- 🔧 **Use tools** — executes shell commands, reads files, browses the web
- 🔄 **Self-correct** — reviews its own output and retries on failure
- 💾 **Persistent memory** — conversations survive app restarts
- ⚡ **Work with any LLM** — connect your own OpenAI, Anthropic, or local model

> MotherAgent turns any LLM into a capable, autonomous coding partner.

---

## ✨ What is Echobird?

Echobird is a desktop application that provides a **visual, unified interface** for managing AI models across your coding tools. No more digging through config files — just point, click, and switch.

### The Problem

- 😫 Switching AI models in tools like Claude Code requires editing config files manually
- 🔄 Each tool has its own model configuration format
- 🧩 No easy way to manage skills and extensions across tools

### The Solution

Echobird acts as a **central control panel** for all your AI-powered coding tools:

- 🎯 **One-Click Model Switching** — Visually switch AI models for any supported tool
- 🔀 **Dual Protocol** — OpenAI & Anthropic API support, switch models anytime
- 🚇 **Smart Tunnel Proxy** — Access geo-restricted APIs without a full VPN
- 🧩 **Skill Browser** — Discover, install, and manage AI skills
- 🖥️ **Local LLM Server** — Run open-source models (Qwen, DeepSeek, Llama) locally via llama.cpp
- 🔌 **Remote LLM** — Connect to remote LLM servers and deploy your own agents
- 🌍 **28 Languages** — Full i18n support for a global audience
-  **Cyberpunk UI** — Stunning neon-green terminal aesthetic

## 🖼️ Screenshots

### Model Nexus — Manage all your AI models in one place
![Model Nexus](docs/1.png)

### App Manager — One-click model switching for all coding tools
![App Manager](docs/2.png)

### Local Server — Run open-source models locally with llama.cpp
![Local Server](docs/3.png)

### Skill Browser — Discover and install AI skills
![Skill Browser](docs/4.png)

## 🚀 Quick Start

### Download

Get the latest release for your platform:

| Platform | Download |
|----------|----------|
| Windows  | [Echobird-x64-setup.exe](https://github.com/edison7009/Echobird-MotherAgent/releases/latest) |
| macOS (Apple Silicon) | [Echobird_aarch64.dmg](https://github.com/edison7009/Echobird-MotherAgent/releases/latest) |
| macOS (Intel) | [Echobird_x64.dmg](https://github.com/edison7009/Echobird-MotherAgent/releases/latest) |
| Linux | [Echobird_amd64.AppImage](https://github.com/edison7009/Echobird-MotherAgent/releases/latest) |

### Linux Notes

```bash
chmod +x Echobird_*.AppImage
./Echobird_*.AppImage
```

> If you encounter FUSE errors: `sudo apt install libfuse2`

## 🔧 Supported Tools

| Tool | Status | Model Switching | Protocol |
|------|--------|-----------------|----------|
| Claude Code | ✅ Supported | ✅ | Anthropic |
| OpenClaw | ✅ Supported | ✅ | OpenAI / Anthropic |
| Cline | ✅ Supported | ✅ | OpenAI |
| Roo Code | ✅ Supported | ✅ | OpenAI |
| Continue | ✅ Supported | ✅ | OpenAI |
| OpenCode | ✅ Supported | ✅ | OpenAI |
| Codex | ✅ Supported | ✅ | OpenAI |
| Aider | ✅ Supported | ✅ | OpenAI / Anthropic |
| ZeroClaw | ✅ Supported | ✅ | OpenAI |

## 🏗️ Tech Stack

- **Tauri 2** — Cross-platform desktop framework (Rust backend)
- **Rust** — High-performance native backend
- **React + TypeScript** — UI framework
- **Vanilla CSS** — Custom cyberpunk design system
- **Vite** — Build tool
- **llama.cpp** — Local model inference engine

##  Contact

- 📧 Email: [hi@echobird.ai](mailto:hi@echobird.ai)
- 🌐 Website: [echobird.ai](https://echobird.ai)

## ⭐ Support

If you find Echobird useful, please give it a ⭐ on GitHub — it helps others discover the project!

## 📄 License

[MIT](LICENSE)

---

<p align="center">
  Made with 💚 by the Echobird Team<br/>
  <sub>🌐 <a href="https://echobird.ai">echobird.ai</a></sub>
</p>
