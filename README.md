<p align="center">
  <img src="docs/icon.png" alt="EchoBird" width="120" />
</p>

<h1 align="center">EchoBird</h1>

<h3 align="center">像专家一样部署 AI Agent — 不打开终端,不写配置文件,一键搞定。</h3>

<p align="center">
  一键安装 OpenClaw、Claude Code、NanoBot、PicoClaw、Hermes 等 AI Agent · 在 OpenAI / Anthropic / Gemini / 本地大模型间自由切换 · 用 llama.cpp / vLLM 把 Qwen、DeepSeek 跑在自己的机器上。
</p>

<p align="center">
  <a href="https://github.com/edison7009/EchoBird/releases">
    <img src="https://img.shields.io/github/v/release/edison7009/EchoBird?style=flat-square&color=00FF9D" alt="Release" />
  </a>
  <img src="https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-blue?style=flat-square" alt="Platform" />
  <img src="https://img.shields.io/badge/built%20with-Tauri%20%2B%20Rust-orange?style=flat-square" alt="Tauri + Rust" />
  <img src="https://img.shields.io/badge/license-MIT-green?style=flat-square" alt="MIT License" />
</p>

<p align="center">
  <a href="https://echobird.ai">官网</a> ·
  <a href="https://github.com/edison7009/EchoBird/releases/latest">下载</a> ·
  <a href="https://echobird.ai/free-models.html">今日免费模型</a>
</p>

---

## 为什么选 EchoBird?

哪怕你是 AI 新手,EchoBird 也能让你像专家一样指挥自己的 Agent —— 从安装、配置到运行,全程通过点击和对话完成。
**不需要终端命令,不需要改 JSON / TOML / .env,也不需要再去翻十篇部署文档。**

想用 **OpenClaw**、**Claude Code**、**NanoBot**、**PicoClaw**、**Hermes**、**ZeroClaw**?一键安装。
想把 **Qwen**、**DeepSeek**、**Llama** 跑在自己的机器上?一键部署。
想给所有 Agent 一起换模型?一键切换。

> EchoBird 把 Agent 安装、模型管理、本地大模型部署整合进同一个应用 —— 不管你是开发者,还是刚开始接触 AI。

---

## ✨ 核心功能

### 🚀 一键安装 — 主流 AI Agent 即装即用

- **自动检测已安装环境** — EchoBird 会扫描你机器上已有的 Agent,缺哪个,点一下就装好
- **内置启动器** — 不碰终端就能跑起任何支持的 Agent
- **即插即用插件** — 把一个 `plugin.json` 丢进 tools 目录,就能扩展新工具,无需改代码

### 🔀 一键切换 — 给所有 Agent 统一换模型

- **可视化 Model Nexus** — OpenAI、Anthropic、Gemini、DeepSeek、Ollama 以及任意自定义 endpoint,统一在一个面板管理
- **双协议支持** — OpenAI API + Anthropic API,按 Agent 单独指定协议,零配置改动
- **一键应用** — 选一张模型卡片,点一下就给目标 Agent 应用上,告别手动改 JSON / TOML / `.env`

### 💻 一键部署本地大模型 — Qwen、DeepSeek、Llama 跑在自己机器上

- **本地 LLM** — 内置 llama.cpp / vLLM / SGLang,所有数据都留在你的设备上
- **统一代理** — 自动暴露 OpenAI(`/v1`)与 Anthropic(`/anthropic`)两套端点,任何 Agent 都能直连
- **智能 GPU 检测** — 自动识别 NVIDIA GPU 并推荐最优参数

### 🤖 MotherAgent — 你的全自动 AI 助手

- 内建工具调用、技能系统,与所有模型完全兼容
- 直接在对话里让它帮你装 Agent、切模型、起服务
- 支持流程透明化:工具调用以可折叠卡片形式展示在对话中

### 🧩 还有更多

- 🌐 **智能隧道代理** — 不开 VPN 也能访问受地区限制的 API
- 🎮 **内置 AI 小应用** — 黑白棋、AI 翻译……
- 🌍 **多语言界面** — 完整 i18n 支持

---

## 🖼️ 截图

### Model Nexus — OpenAI / Anthropic / Gemini / DeepSeek / Ollama 统一管理
![Model Nexus](docs/1.png)

### App Manager — 一键给 OpenClaw / Claude Code / NanoBot 切换模型
![App Manager](docs/2.png)

### Local LLM — 通过 llama.cpp / vLLM / SGLang 在本地部署 Qwen / Llama / DeepSeek
![Local Server](docs/3.png)

### MotherAgent — 用对话指挥它装 Agent、起服务
![MotherAgent](docs/motheragent.png)

---

## 🚀 下载

| 平台 | 下载 |
|------|------|
| 🪟 Windows | [Echobird-x64-setup.exe](https://github.com/edison7009/EchoBird/releases/latest) |
| 🍎 macOS(Apple Silicon) | [Echobird_aarch64.dmg](https://github.com/edison7009/EchoBird/releases/latest) |
| 🍎 macOS(Intel) | [Echobird_x64.dmg](https://github.com/edison7009/EchoBird/releases/latest) |
| 🐧 Linux | [Echobird_amd64.AppImage](https://github.com/edison7009/EchoBird/releases/latest) |

**Linux 快速上手:**
```bash
chmod +x Echobird_*.AppImage
./Echobird_*.AppImage
# 缺 FUSE 的话: sudo apt install libfuse2
```

---

## 🔧 兼容性

### 支持的 Agent / 编程助手

| 工具 | 协议 | 安装方式 |
|------|------|---------|
| OpenClaw | OpenAI / Anthropic | 一键安装 |
| Claude Code | Anthropic | 一键安装 |
| OpenCode | OpenAI | 一键安装 |
| ZeroClaw | OpenAI | 一键安装 |
| NanoBot | OpenAI / Anthropic | 一键安装 |
| PicoClaw | OpenAI / Anthropic | 一键安装 |
| Hermes Agent | OpenAI / Anthropic | 一键安装 |
| Codex | OpenAI | 一键安装 |
| Cline | OpenAI | 配置接入 |
| Roo Code | OpenAI | 配置接入 |
| Continue | OpenAI | 配置接入 |
| Aider | OpenAI / Anthropic | 配置接入 |

### 本地 LLM 运行时

| 运行时 | 支持模型 | 平台 |
|--------|---------|------|
| llama.cpp | Qwen 3.5 / Llama 4 / DeepSeek / MiniMax M2.5 / GLM-5(GGUF) | Windows / macOS / Linux |
| vLLM | 任意 HuggingFace 模型 | Linux(CUDA) |
| SGLang | 任意 HuggingFace 模型 | Linux(CUDA) |

---

## 🏗️ 技术栈

**Tauri 2** + **Rust** + **React** + **TypeScript** + **llama.cpp**

---

## 📬 联系方式

- 📧 [hi@echobird.ai](mailto:hi@echobird.ai)(Bug 反馈)
- 🌐 [echobird.ai](https://echobird.ai)
- 🐛 [GitHub Issues](https://github.com/edison7009/EchoBird/issues)

---

<p align="center">
  <em>The last interface before the age of AI.</em><br/>
  Made with 💚 by the EchoBird Team<br/>
  <sub>⭐ <a href="https://github.com/edison7009/EchoBird">Star on GitHub</a> — 帮更多人发现这个项目</sub>
</p>
