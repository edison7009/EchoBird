<p align="center">
  <img src="docs/icon.png" alt="Echobird" width="120" />
</p>

<h1 align="center">Echobird</h1>

<h3 align="center">One-click install OpenClaw, Claude Code, ZeroClaw & Codex. Switch models. Deploy LLMs.</h3>

<p align="center">
  One app to install agents, switch models, deploy local/remote LLMs, and control all agents from one Channels screen.<br/>
  <sub>A cross-platform desktop AI control panel 鈥?built with Tauri 2 + Rust.</sub>
</p>

<p align="center">
  <a href="https://github.com/edison7009/Echobird-MotherAgent/releases">
    <img src="https://img.shields.io/github/v/release/edison7009/Echobird-MotherAgent?style=flat-square&color=00FF9D" alt="Release" />
  </a>
  <img src="https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-blue?style=flat-square" alt="Platform" />
  <img src="https://img.shields.io/badge/built%20with-Tauri%20%2B%20Rust-orange?style=flat-square" alt="Tauri + Rust" />
</p>

<p align="center">
  <strong>English</strong> 路
  <a href="./docs/README.zh-CN.md">绠�浣撲腑鏂?/a> 路
  <a href="./docs/README.zh-TW.md">绻侀珨涓枃</a> 路
  <a href="./docs/README.ja.md">鏃ユ湰瑾?/a> 路
  <a href="./docs/README.ko.md">頃滉淡鞏?/a> 路
  <a href="./docs/README.es.md">Espa帽ol</a> 路
  <a href="./docs/README.fr.md">Fran莽ais</a> 路
  <a href="./docs/README.de.md">Deutsch</a> 路
  <a href="./docs/README.pt.md">Portugu锚s</a> 路
  <a href="./docs/README.ru.md">袪褍褋褋泻懈泄</a> 路
  <a href="./docs/README.ar.md">丕賱毓乇亘賷丞</a>
</p>

---

## Why Echobird?

Even as an AI beginner, Echobird lets you command your own Agent 鈥?from setup to work 鈥?through simple chat. No terminal expertise, no config files, no deployment headaches.

Want to use **OpenClaw**, **Claude Code**, **ZeroClaw**, or **Codex**? One click to install. Want to run **Qwen**, **DeepSeek**, or **Llama** on your own machine? One click to deploy. Want to switch models or add skills? Point, click, done.

**Echobird gives you one app to install agents, switch models, deploy LLMs, and control everything from one screen** 鈥?whether you're a developer or just getting started with AI.

---

## 鉁?Features

### 馃殌 One-Click Install 鈥?OpenClaw, Claude Code, OpenCode, ZeroClaw & more

- **Auto-detect & install** 鈥?Echobird detects which agents are installed and lets you deploy missing ones in one click
- **Plug-and-play tools** 鈥?Drop a `plugin.json` in the tools folder and it just works. No code changes needed
- **Built-in launcher** 鈥?Start any supported agent without touching the terminal

### 馃攢 One-Click Model Switch 鈥?Switch models across all agents instantly

- **Visual Model Nexus** 鈥?Manage all your AI models (OpenAI, Anthropic, Gemini, DeepSeek, Ollama, or any custom endpoint) in one panel
- **Dual Protocol** 鈥?OpenAI API & Anthropic API. Switch protocols per agent with zero config changes
- **One-click apply** 鈥?Select a model card, toggle it on for any agent. No more editing JSON, TOML, or `.env` files

### 馃捇 One-Click Deploy LLM 鈥?Run Qwen, DeepSeek, Llama, MiniMax locally or remotely

- **Local LLM** 鈥?Deploy open-source models via built-in llama.cpp, vLLM, or SGLang. Your data never leaves your device
- **Remote LLM** 鈥?Deploy to any GPU server via SSH. One-click start Qwen 3.5, MiniMax M2.5, GLM-5, or any GGUF/HuggingFace model
- **Unified Proxy** 鈥?Automatically exposes both OpenAI (`/v1`) and Anthropic (`/anthropic`) endpoints. Connect any agent instantly
- **Smart GPU Detection** 鈥?Auto-detect NVIDIA GPUs and recommend optimal settings

### 馃摗 Channels 鈥?Control multiple agents from one screen

- **Multi-agent channels** 鈥?Run OpenClaw, ZeroClaw, or any Bridge-compatible agent side by side
- **Local & Remote** 鈥?Local agents via Bridge Protocol, remote agents via SSH tunnels. Same interface, same experience
- **Persistent Sessions** 鈥?Agent conversations survive app restarts. Pick up exactly where you left off
- **MotherAgent** 鈥?Your autonomous AI agent with tool calling, skill system, and full model flexibility

### 馃З More Built-in

- 馃寪 **Smart Tunnel Proxy** 鈥?Access geo-restricted APIs without a full VPN
- 馃幆 **Skill Browser** 鈥?One-click discover, translate, and install AI skills across agents
- 馃幃 **Built-in AI Apps** 鈥?Reversi, AI Translate, and more
- 馃實 **28 Languages** 鈥?Full i18n from English to Arabic

---

## 馃柤锔?Screenshots

### Model Nexus 鈥?OpenAI, Anthropic, Gemini, DeepSeek, Ollama 鈥?all in one panel
![Model Nexus](docs/1.png)

### App Manager 鈥?One-click model switching for OpenClaw, Claude Code, Codex & more
![App Manager](docs/2.png)

### Local LLM 鈥?Deploy Qwen, Llama, DeepSeek locally via llama.cpp / vLLM / SGLang
![Local Server](docs/3.png)

### Skill Browser 鈥?One-click translate & install Skills for OpenClaw, Claude Code & more
![Skill Browser](docs/4.png)

---

## 馃殌 Download

| Platform | Download |
|----------|----------|
| 馃獰 Windows | [Echobird-x64-setup.exe](https://github.com/edison7009/Echobird-MotherAgent/releases/latest) |
| 馃崕 macOS (Apple Silicon) | [Echobird_aarch64.dmg](https://github.com/edison7009/Echobird-MotherAgent/releases/latest) |
| 馃崕 macOS (Intel) | [Echobird_x64.dmg](https://github.com/edison7009/Echobird-MotherAgent/releases/latest) |
| 馃惂 Linux | [Echobird_amd64.AppImage](https://github.com/edison7009/Echobird-MotherAgent/releases/latest) |

**Linux quick start:**
```bash
chmod +x Echobird_*.AppImage
./Echobird_*.AppImage
# Need FUSE? sudo apt install libfuse2
```

---

## 馃敡 Works With

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

## 馃彈锔?Tech Stack

**Tauri 2** + **Rust** + **React** + **TypeScript** + **llama.cpp**

---

## 馃摤 Contact

- 馃摟 [hi@echobird.ai](mailto:hi@echobird.ai)
- 馃寪 [echobird.ai](https://echobird.ai)

---

<p align="center">
  <em>The last interface before the age of AI.</em><br/>
  Made with 馃挌 by the Echobird Team<br/>
  <sub>猸?<a href="https://github.com/edison7009/Echobird-MotherAgent">Star on GitHub</a> 鈥?it helps others discover the project!</sub>
</p>
