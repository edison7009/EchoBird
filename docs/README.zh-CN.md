<p align="center">
  <img src="../build/icon.png" alt="Echobird" width="120" />
</p>

<h1 align="center">Echobird</h1>

<p align="center">
  模型、智能体与 Vibe Coding 的枢纽。<br/>
  <sub>赛博朋克风格的 AI 时代控制台 — 基于 Tauri + Rust 构建。</sub>
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

## 🤖 MotherAgent — 核心功能

**MotherAgent** 是 Echobird 内置的自主 AI 智能体。与简单的聊天界面不同，MotherAgent 能够：

- 🧠 **思考与规划** — 将复杂任务分解为可执行的步骤
- 🔧 **使用工具** — 执行 Shell 命令、读取文件、浏览网页
- 🔄 **自我修正** — 检查自身输出并在出错时重试
- 💾 **持久记忆** — 对话记录在应用重启后依然保留
- ⚡ **支持任意 LLM** — 连接 OpenAI、Anthropic 或本地模型

> MotherAgent 将任何 LLM 变成具备自主能力的编程伙伴。

---

## ✨ Echobird 是什么？

Echobird 是一款桌面应用，为你的 AI 编程工具提供**可视化、统一的模型管理界面**。无需再手动翻配置文件 — 点一下，就能切换。

### 痛点

- 😫 在 Claude Code 等工具中切换模型需要手动编辑配置文件
- 🔄 每个工具都有自己的模型配置格式
- 🧩 没有方便的方式跨工具管理技能和扩展

### 解决方案

Echobird 是你所有 AI 编程工具的**中央控制面板**：

- 🎯 **一键切换模型** — 可视化切换任何支持工具的 AI 模型
- 🔀 **双协议支持** — OpenAI & Anthropic API，随时随地切换
- 🚇 **智能隧道代理** — 无需全局 VPN 即可访问受限 API
- 🧩 **技能浏览器** — 发现、安装和管理 AI 技能
- 🖥️ **本地模型服务器** — 通过 llama.cpp 本地运行开源模型（Qwen、DeepSeek、Llama）
- 🔌 **远程 LLM** — 连接远程 LLM 服务器，部署你自己的智能体
- 🌍 **28 种语言** — 完整国际化支持
- 🌃 **赛博朋克 UI** — 炫酷的霓虹绿终端美学

## 🖼️ 截图

### Model Nexus — 在一处管理所有 AI 模型
![Model Nexus](./1.png)

### App Manager — 一键为所有编程工具切换模型
![App Manager](./2.png)

### Local Server — 通过 llama.cpp 本地运行开源模型
![Local Server](./3.png)

### Skill Browser — 发现和安装 AI 技能
![Skill Browser](./4.png)

## 🚀 快速开始

### 下载

获取适合你平台的最新版本：

| 平台 | 下载 |
|------|------|
| Windows | [Echobird-x64-setup.exe](https://github.com/edison7009/Echobird-MotherAgent/releases/latest) |
| macOS (Apple Silicon) | [Echobird_aarch64.dmg](https://github.com/edison7009/Echobird-MotherAgent/releases/latest) |
| macOS (Intel) | [Echobird_x64.dmg](https://github.com/edison7009/Echobird-MotherAgent/releases/latest) |
| Linux | [Echobird_amd64.AppImage](https://github.com/edison7009/Echobird-MotherAgent/releases/latest) |

### Linux 说明

```bash
chmod +x Echobird_*.AppImage
./Echobird_*.AppImage
```

> 如果遇到 FUSE 错误：`sudo apt install libfuse2`

## 🔧 支持的工具

| 工具 | 状态 | 模型切换 | 协议 |
|------|------|----------|------|
| Claude Code | ✅ 已支持 | ✅ | Anthropic |
| OpenClaw | ✅ 已支持 | ✅ | OpenAI / Anthropic |
| Cline | ✅ 已支持 | ✅ | OpenAI |
| Roo Code | ✅ 已支持 | ✅ | OpenAI |
| Continue | ✅ 已支持 | ✅ | OpenAI |
| OpenCode | ✅ 已支持 | ✅ | OpenAI |
| Codex | ✅ 已支持 | ✅ | OpenAI |
| Aider | ✅ 已支持 | ✅ | OpenAI / Anthropic |
| ZeroClaw | ✅ 已支持 | ✅ | OpenAI |

## 🏗️ 技术栈

- **Tauri 2** — 跨平台桌面框架（Rust 后端）
- **Rust** — 高性能原生后端
- **React + TypeScript** — UI 框架
- **Vanilla CSS** — 自定义赛博朋克设计系统
- **Vite** — 构建工具
- **llama.cpp** — 本地模型推理引擎

## 📬 联系我们

- 📧 邮箱：[hi@echobird.ai](mailto:hi@echobird.ai)
- 🌐 网站：[echobird.ai](https://echobird.ai)

## ⭐ 支持

如果 Echobird 对你有帮助，请在 GitHub 上给个 ⭐ — 让更多人发现这个项目！

## 📄 许可证

[MIT](../LICENSE)

---

<p align="center">
  由 Echobird 团队用 💚 打造<br/>
  <sub>🌐 <a href="https://echobird.ai">echobird.ai</a></sub>
</p>
