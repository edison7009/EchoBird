<p align="center">
  <img src="./icon.png" alt="Echobird" width="120" />
</p>

<h1 align="center">Echobird</h1>

<h3 align="center">一鍵安裝 OpenClaw、Claude Code、ZeroClaw 和 Codex。切換模型。部署大模型。</h3>

<p align="center">
  一個應用安裝智能體、切換模型、部署本地/遠端大模型，在一個 Channels 頁面控制所有智能體。<br/>
  <sub>跨平台桌面 AI 控制台 — 基於 Tauri 2 + Rust 構建。</sub>
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
  <a href="./README.zh-CN.md">简体中文</a> ·
  <strong>繁體中文</strong> ·
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

## 為什麼選擇 Echobird？

即使你是 AI 新手，Echobird 也能讓你透過簡單的對話來掌控自己的 Agent —— 從安裝到工作，一氣呵成。不需要終端經驗，不需要編輯設定檔，不需要複雜的部署。

想用 **OpenClaw**、**Claude Code**、**ZeroClaw** 或 **Codex**？一鍵安裝。想在自己的電腦上執行 **Qwen**、**DeepSeek** 或 **Llama**？一鍵部署。想切換模型或新增技能？點一下就搞定。

**Echobird 用一個應用搞定一切** —— 安裝智能體、切換模型、部署大模型、在一個頁面控制所有智能體 —— 無論你是開發者還是 AI 新手。

---

## ✨ 核心功能

### 🚀 一鍵安裝 — OpenClaw、Claude Code、OpenCode、ZeroClaw 等

- **自動偵測並安裝** — Echobird 自動偵測已安裝的智能體，缺少的一鍵即可部署
- **即插即用工具** — 把 `plugin.json` 放到 tools 資料夾就能用，無需改程式碼
- **內建啟動器** — 無需終端機，直接啟動任何支援的智能體

### 🔀 一鍵切換模型 — 跨所有智能體即時切換模型

- **視覺化 Model Nexus** — 在一個面板管理所有 AI 模型（OpenAI、Anthropic、Gemini、DeepSeek、Ollama 或任意自訂端點）
- **雙協議支援** — OpenAI API 與 Anthropic API，每個智能體獨立設定，零設定切換
- **一鍵套用** — 選擇模型卡片，為任意智能體開啟。不再編輯 JSON、TOML 或 `.env` 檔案

### 💻 一鍵部署大模型 — 本地或遠端執行 Qwen、DeepSeek、Llama、MiniMax

- **本地大模型** — 透過內建 llama.cpp、vLLM 或 SGLang 部署開源模型。資料永不離開裝置
- **遠端大模型** — 透過 SSH 部署到任意 GPU 伺服器。一鍵啟動 Qwen 3.5、MiniMax M2.5、GLM-5 或任何 GGUF/HuggingFace 模型
- **統一代理** — 自動提供 OpenAI (`/v1`) 和 Anthropic (`/anthropic`) 雙端點，任何智能體即連即用
- **智慧 GPU 偵測** — 自動偵測 NVIDIA GPU 並推薦最佳設定

### 📡 Channels — 一個頁面控制多個智能體

- **多智能體頻道** — 同時執行 OpenClaw、ZeroClaw 或任何 Bridge 相容智能體
- **本地與遠端** — 本地智能體透過 Bridge 協議，遠端智能體透過 SSH 通道。統一介面，統一體驗
- **持久工作階段** — 智能體對話在應用程式重啟後依然保留，繼續上次進度
- **MotherAgent** — 你的自主 AI 智能體，支援 Tool Calling、技能系統和完整模型彈性

### 🧩 更多內建功能

- 🌐 **智慧隧道代理** — 無需全域 VPN 即可存取受限 API
- 🎯 **技能瀏覽器** — 一鍵發現、翻譯和安裝 AI 技能
- 🎮 **內建 AI 應用** — Reversi、AI 翻譯等
- 🌍 **28 種語言** — 從英語到阿拉伯語的完整國際化支援

---

## 🖼️ 截圖

### Model Nexus — OpenAI、Anthropic、Gemini、DeepSeek、Ollama — 統一管理
![Model Nexus](./1.png)

### App Manager — 為 OpenClaw、Claude Code、Codex 等一鍵切換模型
![App Manager](./2.png)

### 本地大模型 — 透過 llama.cpp / vLLM / SGLang 本地部署 Qwen、Llama、DeepSeek
![Local Server](./3.png)

### 技能瀏覽器 — 為 OpenClaw、Claude Code 等一鍵翻譯和安裝 Skills
![Skill Browser](./4.png)

---

## 🚀 下載

| 平台 | 下載連結 |
|------|---------|
| 🪟 Windows | [Echobird-x64-setup.exe](https://github.com/edison7009/Echobird-MotherAgent/releases/latest) |
| 🍎 macOS (Apple Silicon) | [Echobird_aarch64.dmg](https://github.com/edison7009/Echobird-MotherAgent/releases/latest) |
| 🍎 macOS (Intel) | [Echobird_x64.dmg](https://github.com/edison7009/Echobird-MotherAgent/releases/latest) |
| 🐧 Linux | [Echobird_amd64.AppImage](https://github.com/edison7009/Echobird-MotherAgent/releases/latest) |

**Linux 快速啟動：**
```bash
chmod +x Echobird_*.AppImage
./Echobird_*.AppImage
# FUSE 報錯？執行：sudo apt install libfuse2
```

---

## 🔧 支援的工具

### 智能體與程式設計工具

| 工具 | 協議 | 安裝方式 |
|------|------|---------|
| OpenClaw | OpenAI / Anthropic | 一鍵安裝 |
| Claude Code | Anthropic | 一鍵安裝 |
| OpenCode | OpenAI | 一鍵安裝 |
| ZeroClaw | OpenAI | 一鍵安裝 |
| Codex | OpenAI | 一鍵安裝 |
| Cline | OpenAI | 設定 |
| Roo Code | OpenAI | 設定 |
| Continue | OpenAI | 設定 |
| Aider | OpenAI / Anthropic | 設定 |

### 本地大模型執行環境

| 執行環境 | 模型 | 平台 |
|----------|------|------|
| llama.cpp | Qwen 3.5、Llama 4、DeepSeek、MiniMax M2.5、GLM-5 (GGUF) | Windows / macOS / Linux |
| vLLM | 任何 HuggingFace 模型 | Linux (CUDA) |
| SGLang | 任何 HuggingFace 模型 | Linux (CUDA) |

---

## 🏗️ 技術棧

**Tauri 2** + **Rust** + **React** + **TypeScript** + **llama.cpp**

---

## 📬 聯絡我們

- 📧 [hi@echobird.ai](mailto:hi@echobird.ai) (Bug Reports)
- 🌐 [echobird.ai](https://echobird.ai)

---

<p align="center">
  <em>AI 時代前的最後一個介面。</em><br/>
  由 Echobird 團隊用 💚 打造<br/>
  <sub>⭐ <a href="https://github.com/edison7009/Echobird-MotherAgent">在 GitHub 上給個 Star</a> — 讓更多人發現這個專案！</sub>
</p>
