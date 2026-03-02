<p align="center">
  <img src="../build/icon.png" alt="Echobird" width="120" />
</p>

<h1 align="center">Echobird</h1>

<p align="center">
  The Nexus for <strong>Models</strong>, <strong>Agents</strong> & <strong>Vibe Coding</strong>.<br/>
  <sub>Echobird 是一款桌面应用，为你的 AI 编程工具提供可视化、统一的模型管理界面。无需再手动翻配置文件 —— 点一下，就能切换。</sub>
</p>

<p align="center">
  <a href="https://github.com/edison7009/Echobird/releases">
    <img src="https://img.shields.io/github/v/release/edison7009/Echobird?style=flat-square&color=00FF9D" alt="Release" />
  </a>
  <img src="https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-blue?style=flat-square" alt="Platform" />
  <img src="https://img.shields.io/github/license/edison7009/Echobird?style=flat-square" alt="License" />
</p>

<p align="center">
  <a href="../README.md">English</a> · **简体中文** · <a href="./README.zh-TW.md">繁體中文</a> · <a href="./README.ja.md">日本語</a> · <a href="./README.ko.md">한국어</a>
</p>

---

## ✨ Echobird 是什么？

Echobird 是一款桌面应用，为你的 AI 编程工具提供**可视化、统一的模型管理界面**。无需再手动翻配置文件 —— 点一下，就能切换。

### 痛点

- 😫 在 OpenClaw 等工具中切换模型需要手动编辑配置文件
- 🔄 每个工具都有自己的模型配置格式
- 🧩 没有方便的方式管理技能和扩展

### 解决方案

Echobird 是你所有 AI 编程工具的**中央控制面板**：

- 🎯 **一键切换模型** — 可视化切换任何支持工具的 AI 模型
- 🔀 **双协议支持** — OpenAI & Anthropic API 支持，随时随地切换模型
- 🚇 **智能隧道代理** — 无需全局 VPN 即可访问受限 API，仅代理 API 流量
- 🧩 **技能浏览器** — 发现、安装和管理 AI 技能
- 🖥️ **本地模型服务器** — 通过 llama.cpp 本地运行开源模型（Qwen、DeepSeek、Llama）
- 🌍 **28 种语言** — 完整国际化支持
- 🎮 **内置 AI 应用** — 交互式 AI 游戏和工具，如 Reversi 和 AI 翻译
- 🌃 **赛博朋克 UI** — 炫酷的霓虹绿终端美学，让编程充满未来感

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
|----------|----------|
| Windows  | [Echobird-Setup.exe](https://github.com/edison7009/Echobird/releases/latest) |
| macOS    | [Echobird.dmg](https://github.com/edison7009/Echobird/releases/latest) |
| Linux    | [Echobird.AppImage](https://github.com/edison7009/Echobird/releases/latest) |

### Linux 说明

```bash
chmod +x Echobird-*.AppImage
./Echobird-*.AppImage
```

> 如果遇到 FUSE 错误： `sudo apt install libfuse2`

## 🔧 支持的工具

| 工具 | 状态 | 模型切换 | 协议 |
|------|--------|----------------|----------|
| OpenClaw | ✅ 已支持 | ✅ | OpenAI / Anthropic |
| Claude Code | ✅ 已支持 | ✅ | Anthropic |
| Cline | ✅ 已支持 | ✅ | OpenAI |
| Continue | ✅ 已支持 | ✅ | OpenAI |
| OpenCode | ✅ 已支持 | ✅ | OpenAI |
| Codex | ✅ 已支持 | ✅ | OpenAI |
| Roo Code | ✅ 已支持 | ✅ | OpenAI |

## 🏗️ 技术栈

- **Electron** — 跨平台桌面框架
- **React + TypeScript** — UI 框架
- **Vanilla CSS** — 自定义赛博朋克设计系统
- **Vite** — 构建工具
- **llama.cpp** — 本地模型推理引擎

## 🛠️ 开发

```bash
npm install
npm run dev
npm run build
```

## 🤝 贡献

欢迎贡献！随时提交 Issue 或 Pull Request。

We're especially looking for help with:
- 🍎 **macOS 测试**
- 🔧 **新工具集成**
- 🌐 **翻译改进**

## ⭐ 支持

如果 Echobird 对你有帮助，请在 GitHub 上给个 ⭐ —— 让更多人发现这个项目！

## 📄 许可证

[MIT](../LICENSE)

---

<p align="center">
  由 Echobird 团队用 💚 打造<br/>
  <sub>📧 <a href="mailto:hi@echobird.ai">hi@echobird.ai</a></sub>
</p>
