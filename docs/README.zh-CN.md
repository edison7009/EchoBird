<p align="center">
  <img src="./icon.png" alt="Echobird" width="120" />
</p>

<h1 align="center">Echobird</h1>

<h3 align="center">一键安装 OpenClaw、Claude Code、ZeroClaw 和 Codex。切换模型。部署大模型。</h3>

<p align="center">
  一个应用安装智能体、切换模型、部署本地/远程大模型，在一个 Channels 页面控制所有智能体。<br/>
  <sub>跨平台桌面 AI 控制台 — 基于 Tauri 2 + Rust 构建。</sub>
</p>

<p align="center">
  <a href="https://github.com/edison7009/Echobird-MotherAgent/releases">
    <img src="https://img.shields.io/github/v/release/edison7009/Echobird-MotherAgent?style=flat-square&color=00FF9D" alt="Release" />
  </a>
  <img src="https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-blue?style=flat-square" alt="Platform" />
  <img src="https://img.shields.io/badge/built%20with-Tauri%20%2B%20Rust-orange?style=flat-square" alt="Tauri + Rust" />
</p>

<p align="center">
  <a href="../README.md">English</a> ·
  <strong>简体中文</strong> ·
  <a href="./README.zh-TW.md">繁體中文</a> ·
  <a href="./README.ja.md">日本語</a> ·
  <a href="./README.ko.md">한국어</a> ·
  <a href="./README.es.md">Español</a> ·
  <a href="./README.fr.md">Français</a> ·
  <a href="./README.de.md">Deutsch</a> ·
  <a href="./README.pt.md">Português</a> ·
  <a href="./README.ru.md">Русский</a> ·
  <a href="./README.ar.md">العربية</a>
</p>

---

## 为什么选择 Echobird？

即使你是 AI 新手，Echobird 也能让你通过简单的对话来掌控自己的 Agent —— 从安装到工作，一气呵成。不需要终端经验，不需要编辑配置文件，不需要复杂的部署。

想用 **OpenClaw**、**Claude Code**、**ZeroClaw** 或 **Codex**？一键安装。想在自己的电脑上运行 **Qwen**、**DeepSeek** 或 **Llama**？一键部署。想切换模型或添加技能？点一下就搞定。

**Echobird 用一个应用搞定一切** —— 安装智能体、切换模型、部署大模型、在一个页面控制所有智能体 —— 无论你是开发者还是 AI 新手。

---

## ✨ 核心功能

### 🚀 一键安装 — OpenClaw、Claude Code、OpenCode、ZeroClaw 等

- **自动检测并安装** — Echobird 自动检测已安装的智能体，缺少的一键即可部署
- **即插即用工具** — 把 `plugin.json` 放到 tools 文件夹就能用，无需改代码
- **内置启动器** — 无需终端，直接启动任何支持的智能体

### 🔀 一键切换模型 — 跨所有智能体即时切换模型

- **可视化 Model Nexus** — 在一个面板管理所有 AI 模型（OpenAI、Anthropic、Gemini、DeepSeek、Ollama 或任意自定义端点）
- **双协议支持** — OpenAI API 与 Anthropic API，每个智能体独立配置，零配置切换
- **一键应用** — 选择模型卡片，为任意智能体开启。不再编辑 JSON、TOML 或 `.env` 文件

### 💻 一键部署大模型 — 本地或远程运行 Qwen、DeepSeek、Llama、MiniMax

- **本地大模型** — 通过内置 llama.cpp、vLLM 或 SGLang 部署开源模型。数据永不离开设备
- **远程大模型** — 通过 SSH 部署到任意 GPU 服务器。一键启动 Qwen 3.5、MiniMax M2.5、GLM-5 或任何 GGUF/HuggingFace 模型
- **统一代理** — 自动提供 OpenAI (`/v1`) 和 Anthropic (`/anthropic`) 双端点，任何智能体即连即用
- **智能 GPU 检测** — 自动检测 NVIDIA GPU 并推荐最优设置

### 📡 Channels — 一个页面控制多个智能体

- **多智能体频道** — 同时运行 OpenClaw、ZeroClaw 或任何 Bridge 兼容智能体
- **本地与远程** — 本地智能体通过 Bridge 协议，远程智能体通过 SSH 隧道。统一界面，统一体验
- **持久会话** — 智能体对话在应用重启后依然保留，继续上次进度
- **MotherAgent** — 你的自主 AI 智能体，支持 Tool Calling、技能系统和完整模型灵活性

### 🧩 更多内置功能

- 🌐 **智能隧道代理** — 无需全局 VPN 即可访问受限 API
- 🎯 **技能浏览器** — 一键发现、翻译和安装 AI 技能
- 🎮 **内置 AI 应用** — Reversi、AI 翻译等
- 🌍 **28 种语言** — 从英语到阿拉伯语的完整国际化支持

---

## 🖼️ 截图

### Model Nexus — OpenAI、Anthropic、Gemini、DeepSeek、Ollama — 统一管理
![Model Nexus](./1.png)

### App Manager — 为 OpenClaw、Claude Code、Codex 等一键切换模型
![App Manager](./2.png)

### 本地大模型 — 通过 llama.cpp / vLLM / SGLang 本地部署 Qwen、Llama、DeepSeek
![Local Server](./3.png)

### 技能浏览器 — 为 OpenClaw、Claude Code 等一键翻译和安装 Skills
![Skill Browser](./4.png)

---

## 🚀 下载

| 平台 | 下载链接 |
|------|---------|
| 🪟 Windows | [Echobird-x64-setup.exe](https://github.com/edison7009/Echobird-MotherAgent/releases/latest) |
| 🍎 macOS (Apple Silicon) | [Echobird_aarch64.dmg](https://github.com/edison7009/Echobird-MotherAgent/releases/latest) |
| 🍎 macOS (Intel) | [Echobird_x64.dmg](https://github.com/edison7009/Echobird-MotherAgent/releases/latest) |
| 🐧 Linux | [Echobird_amd64.AppImage](https://github.com/edison7009/Echobird-MotherAgent/releases/latest) |

**Linux 快速启动：**
```bash
chmod +x Echobird_*.AppImage
./Echobird_*.AppImage
# FUSE 报错？运行：sudo apt install libfuse2
```

---

## 🔧 支持的工具

### 智能体与编程工具

| 工具 | 协议 | 安装方式 |
|------|------|---------|
| OpenClaw | OpenAI / Anthropic | 一键安装 |
| Claude Code | Anthropic | 一键安装 |
| OpenCode | OpenAI | 一键安装 |
| ZeroClaw | OpenAI | 一键安装 |
| Codex | OpenAI | 一键安装 |
| Cline | OpenAI | 配置 |
| Roo Code | OpenAI | 配置 |
| Continue | OpenAI | 配置 |
| Aider | OpenAI / Anthropic | 配置 |

### 本地大模型运行时

| 运行时 | 模型 | 平台 |
|--------|------|------|
| llama.cpp | Qwen 3.5、Llama 4、DeepSeek、MiniMax M2.5、GLM-5 (GGUF) | Windows / macOS / Linux |
| vLLM | 任何 HuggingFace 模型 | Linux (CUDA) |
| SGLang | 任何 HuggingFace 模型 | Linux (CUDA) |

---

## 🏗️ 技术栈

**Tauri 2** + **Rust** + **React** + **TypeScript** + **llama.cpp**

---

## 📬 联系我们

- 📧 [hi@echobird.ai](mailto:hi@echobird.ai)
- 🌐 [echobird.ai](https://echobird.ai)

---

<p align="center">
  <em>AI 时代前的最后一个界面。</em><br/>
  由 Echobird 团队用 💚 打造<br/>
  <sub>⭐ <a href="https://github.com/edison7009/Echobird-MotherAgent">在 GitHub 上给个 Star</a> — 让更多人发现这个项目！</sub>
</p>
