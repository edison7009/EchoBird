<p align="center">
  <img src="build/icon.png" alt="Echobird" width="120" />
</p>

<h1 align="center">Echobird</h1>

<h3 align="center">One-click install OpenClaw, Claude Code, ZeroClaw & Codex. Switch models. Deploy LLMs.</h3>

<p align="center">
  One app to install agents, switch models, deploy local/remote LLMs, and control all agents from one Channels screen.<br/>
  <sub>A cross-platform desktop AI control panel — built with Tauri 2 + Rust.</sub>
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

## Why Echobird?

Modern AI coding relies on CLI agents like **OpenClaw**, **Claude Code**, **OpenCode**, **ZeroClaw**, and **Codex** — but installing, configuring, and switching between them is painful. Each tool has its own config format, its own model settings, and its own deployment quirks.

**Echobird gives you one desktop app to install, configure, and deploy them all.** Switch models, deploy LLMs, and control multiple agents — all with one click.

---

## ✨ Features

### 🚀 One-Click Install — OpenClaw, Claude Code, OpenCode, ZeroClaw & more

- **Auto-detect & install** — Echobird detects which agents are installed and lets you deploy missing ones in one click
- **Plug-and-play tools** — Drop a `plugin.json` in the tools folder and it just works. No code changes needed
- **Built-in launcher** — Start any supported agent without touching the terminal

### 🔀 One-Click Model Switch — Switch models across all agents instantly

- **Visual Model Nexus** — Manage all your AI models (OpenAI, Anthropic, Gemini, DeepSeek, Ollama, or any custom endpoint) in one panel
- **Dual Protocol** — OpenAI API & Anthropic API. Switch protocols per agent with zero config changes
- **One-click apply** — Select a model card, toggle it on for any agent. No more editing JSON, TOML, or `.env` files

### 💻 One-Click Deploy LLM — Run Qwen, DeepSeek, Llama, MiniMax locally or remotely

- **Local LLM** — Deploy open-source models via built-in llama.cpp, vLLM, or SGLang. Your data never leaves your device
- **Remote LLM** — Deploy to any GPU server via SSH. One-click start Qwen 3.5, MiniMax M2.5, GLM-5, or any GGUF/HuggingFace model
- **Unified Proxy** — Automatically exposes both OpenAI (`/v1`) and Anthropic (`/anthropic`) endpoints. Connect any agent instantly
- **Smart GPU Detection** — Auto-detect NVIDIA GPUs and recommend optimal settings

### 📡 Channels — Control multiple agents from one screen

- **Multi-agent channels** — Run OpenClaw, ZeroClaw, or any Bridge-compatible agent side by side
- **Local & Remote** — Local agents via Bridge Protocol, remote agents via SSH tunnels. Same interface, same experience
- **Persistent Sessions** — Agent conversations survive app restarts. Pick up exactly where you left off
- **MotherAgent** — Your autonomous AI agent with tool calling, skill system, and full model flexibility

### 🧩 More Built-in

- 🌐 **Smart Tunnel Proxy** — Access geo-restricted APIs without a full VPN
- 🎯 **Skill Browser** — One-click discover, translate, and install AI skills across agents
- 🎮 **Built-in AI Apps** — Reversi, AI Translate, and more
- 🌍 **28 Languages** — Full i18n from English to Arabic

---

## 🖼️ Screenshots

### Model Nexus — OpenAI, Anthropic, Gemini, DeepSeek, Ollama — all in one panel
![Model Nexus](docs/1.png)

### App Manager — One-click model switching for OpenClaw, Claude Code, Codex & more
![App Manager](docs/2.png)

### Local LLM — Deploy Qwen, Llama, DeepSeek locally via llama.cpp / vLLM / SGLang
![Local Server](docs/3.png)

### Skill Browser — One-click translate & install Skills for OpenClaw, Claude Code & more
![Skill Browser](docs/4.png)

---

## 🚀 Download

| Platform | Download |
|----------|----------|
| 🪟 Windows | [Echobird-x64-setup.exe](https://github.com/edison7009/Echobird-MotherAgent/releases/latest) |
| 🍎 macOS (Apple Silicon) | [Echobird_aarch64.dmg](https://github.com/edison7009/Echobird-MotherAgent/releases/latest) |
| 🍎 macOS (Intel) | [Echobird_x64.dmg](https://github.com/edison7009/Echobird-MotherAgent/releases/latest) |
| 🐧 Linux | [Echobird_amd64.AppImage](https://github.com/edison7009/Echobird-MotherAgent/releases/latest) |

**Linux quick start:**
```bash
chmod +x Echobird_*.AppImage
./Echobird_*.AppImage
# Need FUSE? sudo apt install libfuse2
```

---

## 🔧 Works With

### Agents & Coding Tools

| Tool | Protocol | Install |
|------|----------|----------|
| OpenClaw | OpenAI / Anthropic | One-click |
| Claude Code | Anthropic | One-click |
| OpenCode | OpenAI | One-click |
| ZeroClaw | OpenAI | One-click |
| Codex | OpenAI | One-click |
| Cline | OpenAI | Config |
| Roo Code | OpenAI | Config |
| Continue | OpenAI | Config |
| Aider | OpenAI / Anthropic | Config |

### Local LLM Runtimes

| Runtime | Models | Platform |
|---------|--------|----------|
| llama.cpp | Qwen 3.5, Llama 4, DeepSeek, MiniMax M2.5, GLM-5 (GGUF) | Windows / macOS / Linux |
| vLLM | Any HuggingFace model | Linux (CUDA) |
| SGLang | Any HuggingFace model | Linux (CUDA) |

---

## 🏗️ Tech Stack

**Tauri 2** + **Rust** + **React** + **TypeScript** + **llama.cpp**

---

## 📬 Contact

- 📧 [hi@echobird.ai](mailto:hi@echobird.ai)
- 🌐 [echobird.ai](https://echobird.ai)

---

<p align="center">
  Made with 💚 by the Echobird Team<br/>
  <sub>⭐ <a href="https://github.com/edison7009/Echobird-MotherAgent">Star on GitHub</a> — it helps others discover the project!</sub>
</p>
