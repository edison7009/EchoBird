<p align="center">
  <img src="./icon.png" alt="Echobird" width="120" />
</p>

<h1 align="center">EchoBird</h1>

<h3 align="center">ワンクリックで OpenClaw、Claude Code、ZeroClaw、Codex をインストール。モデル切替。LLMデプロイ。</h3>

<p align="center">
  1つのアプリでエージェントのインストール、モデル切替、ローカル/リモートLLMのデプロイ、すべてのエージェントを1つのChannels画面で制御。<br/>
  <sub>クロスプラットフォーム デスクトップ AI コントロールパネル — Tauri 2 + Rust で構築。</sub>
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
  <a href="./README.zh-TW.md">繁體中文</a> ·
  <strong>日本語</strong> ·
  <a href="./README.ko.md">한국어</a> ·
  <a href="./README.es.md">Español</a> ·
  <a href="./README.fr.md">Français</a> ·
  <a href="./README.de.md">Deutsch</a> ·
  <a href="./README.pt.md">Português</a> ·
  <a href="./README.ru.md">Русский</a> ·
  <a href="./README.ar.md">العربية</a>
</p>

---

## なぜ EchoBird？

AI初心者でも、EchoBirdなら簡単なチャットで自分のAgentを操れます — セットアップから作業まで。ターミナルの知識も、設定ファイルの編集も、複雑なデプロイも不要です。

**OpenClaw**、**Claude Code**、**ZeroClaw**、**Codex** を使いたい？ワンクリックでインストール。自分のマシンで **Qwen**、**DeepSeek**、**Llama** を動かしたい？ワンクリックでデプロイ。モデル切替やスキル追加？クリックするだけ。

**EchoBirdは1つのアプリですべてを実現** — エージェントのインストール、モデル切替、LLMデプロイ、すべてのエージェントを1画面で制御 — 開発者でもAI初心者でも。


<p align="center">
  <img src="./5.png" alt="Echobird Channels" width="100%" />
</p>

---

## ✨ 機能

### 🚀 ワンクリックインストール — OpenClaw、Claude Code、OpenCode、ZeroClaw など

- **自動検出・インストール** — EchoBirdがインストール済みエージェントを検出、未インストールのものをワンクリックでデプロイ
- **プラグアンドプレイ** — `plugin.json` をtoolsフォルダに置くだけ。コード変更不要
- **内蔵ランチャー** — ターミナル不要で対応エージェントを起動

### 🔀 ワンクリックモデル切替 — すべてのエージェントで即座にモデルを切替

- **ビジュアル Model Nexus** — すべてのAIモデル（OpenAI、Anthropic、Gemini、DeepSeek、Ollama、カスタムエンドポイント）を1パネルで管理
- **デュアルプロトコル** — OpenAI API & Anthropic API。エージェントごとに設定、ゼロコンフィグで切替
- **ワンクリック適用** — モデルカードを選択してエージェントに適用。JSON、TOML、`.env` の編集は不要

### 💻 ワンクリックLLMデプロイ — Qwen、DeepSeek、Llama、MiniMaxをローカルまたはリモートで

- **ローカルLLM** — 内蔵llama.cpp、vLLM、SGLangでオープンソースモデルをデプロイ。データは端末外に出ません
- **リモートLLM** — SSH経由で任意のGPUサーバーにデプロイ。Qwen 3.5、MiniMax M2.5、GLM-5、任意のGGUF/HuggingFaceモデルをワンクリックで起動
- **統合プロキシ** — OpenAI (`/v1`) と Anthropic (`/anthropic`) の両エンドポイントを自動公開。どのエージェントもすぐに接続
- **スマートGPU検出** — NVIDIA GPUを自動検出し最適な設定を推奨

### 📡 Channels — 1画面で複数エージェントを制御

- **マルチエージェントチャンネル** — OpenClaw、ZeroClaw、Bridge互換エージェントを並行実行
- **ローカル&リモート** — ローカルはBridgeプロトコル、リモートはSSHトンネル。同じUI、同じ体験
- **永続セッション** — アプリ再起動後もエージェントの会話を保持。続きからすぐに再開
- **MotherAgent** — Tool Calling、スキルシステム、柔軟なモデル設定を備えた自律AIエージェント

### 🧩 その他の内蔵機能

- 🌐 **スマートトンネルプロキシ** — フルVPNなしでジオ制限APIにアクセス
- 🎮 **内蔵AIアプリ** — Reversi、AI翻訳など
- 🌍 **28言語** — 英語からアラビア語まで完全国際化対応

---

## 🖼️ スクリーンショット

### Model Nexus — OpenAI、Anthropic、Gemini、DeepSeek、Ollama — すべてを1パネルで
![Model Nexus](./1.png)

### App Manager — OpenClaw、Claude Code、Codexなどのモデルをワンクリック切替
![App Manager](./2.png)

### ローカルLLM — llama.cpp / vLLM / SGLang でQwen、Llama、DeepSeekをローカルデプロイ
![Local Server](./3.png)

---

## 🚀 ダウンロード

| プラットフォーム | ダウンロード |
|-----------------|-------------|
| 🪟 Windows | [Echobird-x64-setup.exe](https://github.com/edison7009/Echobird-MotherAgent/releases/latest) |
| 🍎 macOS (Apple Silicon) | [Echobird_aarch64.dmg](https://github.com/edison7009/Echobird-MotherAgent/releases/latest) |
| 🍎 macOS (Intel) | [Echobird_x64.dmg](https://github.com/edison7009/Echobird-MotherAgent/releases/latest) |
| 🐧 Linux | [Echobird_amd64.AppImage](https://github.com/edison7009/Echobird-MotherAgent/releases/latest) |

**Linuxクイックスタート：**
```bash
chmod +x Echobird_*.AppImage
./Echobird_*.AppImage
# FUSEが必要？ sudo apt install libfuse2
```

---

## 🔧 対応ツール

### エージェント＆コーディングツール

| ツール | プロトコル | インストール |
|--------|-----------|-------------|
| OpenClaw | OpenAI / Anthropic | ワンクリック |
| Claude Code | Anthropic | ワンクリック |
| OpenCode | OpenAI | ワンクリック |
| ZeroClaw | OpenAI | ワンクリック |
| Codex | OpenAI | ワンクリック |
| Cline | OpenAI | 設定 |
| Roo Code | OpenAI | 設定 |
| Continue | OpenAI | 設定 |
| Aider | OpenAI / Anthropic | 設定 |

### ローカルLLMランタイム

| ランタイム | モデル | プラットフォーム |
|-----------|--------|-----------------|
| llama.cpp | Qwen 3.5、Llama 4、DeepSeek、MiniMax M2.5、GLM-5 (GGUF) | Windows / macOS / Linux |
| vLLM | 任意のHuggingFaceモデル | Linux (CUDA) |
| SGLang | 任意のHuggingFaceモデル | Linux (CUDA) |

---

## 🏗️ 技術スタック

**Tauri 2** + **Rust** + **React** + **TypeScript** + **llama.cpp**

---

## 📬 お問い合わせ

- 📧 [hi@EchoBird.ai](mailto:hi@EchoBird.ai) (Bug Reports)
- 🌐 [EchoBird.ai](https://echobird.ai)

---

<p align="center">
  <em>AI時代の前にある、最後のインターフェース。</em><br/>
  EchoBird チームが 💚 を込めて開発<br/>
  <sub>⭐ <a href="https://github.com/edison7009/Echobird-MotherAgent">GitHubでStarを</a> — プロジェクトの発見に役立ちます！</sub>
</p>
