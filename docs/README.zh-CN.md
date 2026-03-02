<p align="center">
  <img src="../build/icon.png" alt="Echobird" width="120" />
</p>

<h1 align="center">Echobird</h1>

<p align="center">
  模型、智能体与 Vibe Coding 的中枢。<br/>
  <sub>AI 时代的赛博朋克控制台 — 基于 Tauri + Rust 构建。</sub>
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

## 🤖 MotherAgent — 部署模型。运行智能体。

**MotherAgent** 是你的自主 AI 智能体 —— 部署本地 LLM、连接远程模型、启动 OpenClaw，一站搞定。

- 🖥️ **本地 LLM 部署** — 一键通过内置 llama.cpp 在本机部署 Qwen、DeepSeek、Llama 等开源模型。数据永不离开设备。
- 🌐 **远程 LLM** — 即时接入 OpenAI、Anthropic、Google Gemini 或任意 OpenAI 兼容端点。
- 🦅 **部署 OpenClaw** — 直接从 MotherAgent 启动和管理 OpenClaw 智能体。无需终端。
- 💾 **持久会话** — 智能体对话在应用重启后依然保留，从上次进度继续。
- ⚡ **任意协议** — OpenAI API 与 Anthropic API，每个智能体独立配置，零配置切换。

---

## ✨ Echobird — 切换模型。不是配置文件。

Echobird 是所有 AI 编程工具的**可视化控制面板**。点一下，就切换。

- 🎯 **一键切换模型** — 可视化配置任何支持工具的 AI 模型。不再手动翻 JSON 文件。
- 🔀 **双协议支持** — OpenAI & Anthropic API，随时切换。
- 🚇 **智能隧道代理** — 无需全局 VPN 即可访问受限 API。
- 🧩 **技能浏览器** — 跨工具发现和安装 AI 技能。
- 🎮 **内置 AI 应用** — Reversi、AI 翻译，更多即将到来。
- 🌍 **28 种语言** — 面向全球开发者的完整国际化支持。

---

## 🖼️ 截图

### Model Nexus — 在一处管理所有 AI 模型
![Model Nexus](./1.png)

### App Manager — 一键为所有编程工具切换模型
![App Manager](./2.png)

### Local Server — 通过 llama.cpp 本地运行开源模型
![Local Server](./3.png)

### Skill Browser — 发现和安装 AI 技能
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

| 工具 | 协议 |
|------|------|
| OpenClaw | OpenAI / Anthropic |
| Claude Code | Anthropic |
| Cline | OpenAI |
| Roo Code | OpenAI |
| Continue | OpenAI |
| OpenCode | OpenAI |
| Codex | OpenAI |
| Aider | OpenAI / Anthropic |
| ZeroClaw | OpenAI |

---

## 🏗️ 技术栈

**Tauri 2** + **Rust** + **React** + **TypeScript** + **llama.cpp**

---

## 📬 联系我们

- 📧 [hi@echobird.ai](mailto:hi@echobird.ai)
- 🌐 [echobird.ai](https://echobird.ai)

---

<p align="center">
  由 Echobird 团队用 💚 打造<br/>
  <sub>⭐ <a href="https://github.com/edison7009/Echobird-MotherAgent">在 GitHub 上给个 Star</a> — 让更多人发现这个项目！</sub>
</p>
