<p align="center">
  <img src="../build/icon.png" alt="Echobird" width="120" />
</p>

<h1 align="center">Echobird</h1>

<p align="center">
  模型、智能體與 Vibe Coding 的中樞。<br/>
  <sub>AI 時代的賽博龐克控制台 — 基於 Tauri + Rust 構建。</sub>
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

## 🤖 MotherAgent — 部署模型。執行智能體。

**MotherAgent** 是你的自主 AI 智能體 —— 部署本地 LLM、連接遠端模型、啟動 OpenClaw，一站搞定。

- 🖥️ **本地 LLM 部署** — 一鍵透過內建 llama.cpp 在本機部署 Qwen、DeepSeek、Llama 等開源模型。資料永不離開裝置。
- 🌐 **遠端 LLM** — 即時接入 OpenAI、Anthropic、Google Gemini 或任意 OpenAI 相容端點。
- 🦅 **部署 OpenClaw** — 直接從 MotherAgent 啟動和管理 OpenClaw 智能體。無需終端機。
- 💾 **持久工作階段** — 智能體對話在應用程式重啟後依然保留，從上次進度繼續。
- ⚡ **任意協議** — OpenAI API 與 Anthropic API，每個智能體獨立設定，零設定切換。

---

## ✨ Echobird — 切換模型。不是設定檔。

Echobird 是所有 AI 程式設計工具的**視覺化控制面板**。點一下，就切換。

- 🎯 **一鍵切換模型** — 視覺化設定任何支援工具的 AI 模型。不再手動翻 JSON 檔案。
- 🔀 **雙協議支援** — OpenAI & Anthropic API，隨時切換。
- 🚇 **智慧隧道代理** — 無需全域 VPN 即可存取受限 API。
- 🧩 **技能瀏覽器** — 跨工具發現和安裝 AI 技能。
- 🎮 **內建 AI 應用** — Reversi、AI 翻譯，更多即將到來。
- 🌍 **28 種語言** — 面向全球開發者的完整國際化支援。

---

## 🖼️ 截圖

### Model Nexus — 在一處管理所有 AI 模型
![Model Nexus](./1.png)

### App Manager — 一鍵為所有程式設計工具切換模型
![App Manager](./2.png)

### Local Server — 透過 llama.cpp 本地執行開源模型
![Local Server](./3.png)

### Skill Browser — 發現和安裝 AI 技能
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
# FUSE 錯誤？執行：sudo apt install libfuse2
```

---

## 🔧 支援的工具

| 工具 | 協議 |
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

## 🏗️ 技術棧

**Tauri 2** + **Rust** + **React** + **TypeScript** + **llama.cpp**

---

## 📬 聯絡我們

- 📧 [hi@echobird.ai](mailto:hi@echobird.ai)
- 🌐 [echobird.ai](https://echobird.ai)

---

<p align="center">
  由 Echobird 團隊用 💚 打造<br/>
  <sub>⭐ <a href="https://github.com/edison7009/Echobird-MotherAgent">在 GitHub 上給個 Star</a> — 讓更多人發現這個專案！</sub>
</p>
