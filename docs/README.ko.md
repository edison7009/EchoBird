<p align="center">
  <img src="./icon.png" alt="Echobird" width="120" />
</p>

<h1 align="center">Echobird</h1>

<h3 align="center">원클릭으로 OpenClaw, Claude Code, ZeroClaw, Codex 설치. 모델 전환. LLM 배포.</h3>

<p align="center">
  하나의 앱으로 에이전트 설치, 모델 전환, 로컬/원격 LLM 배포, 하나의 Channels 화면에서 모든 에이전트 제어.<br/>
  <sub>크로스 플랫폼 데스크톱 AI 컨트롤 패널 — Tauri 2 + Rust로 구축.</sub>
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
  <a href="./README.ja.md">日本語</a> ·
  <strong>한국어</strong> ·
  <a href="./README.es.md">Español</a> ·
  <a href="./README.fr.md">Français</a> ·
  <a href="./README.de.md">Deutsch</a> ·
  <a href="./README.pt.md">Português</a> ·
  <a href="./README.ru.md">Русский</a> ·
  <a href="./README.ar.md">العربية</a>
</p>

---

## 왜 Echobird인가?

AI 초보자라도 Echobird를 사용하면 간단한 채팅으로 자신만의 Agent를 제어할 수 있습니다 — 설치부터 작업까지. 터미널 경험도, 설정 파일 편집도, 복잡한 배포도 필요 없습니다.

**OpenClaw**, **Claude Code**, **ZeroClaw**, **Codex**를 사용하고 싶으신가요? 원클릭으로 설치. 자신의 컴퓨터에서 **Qwen**, **DeepSeek**, **Llama**를 실행하고 싶으신가요? 원클릭으로 배포. 모델 전환이나 스킬 추가? 클릭 한 번이면 끝.

**Echobird는 하나의 앱으로 모든 것을 해결합니다** — 에이전트 설치, 모델 전환, LLM 배포, 모든 에이전트를 하나의 화면에서 제어 — 개발자든 AI 초보자든.

---

## ✨ 기능

### 🚀 원클릭 설치 — OpenClaw, Claude Code, OpenCode, ZeroClaw 등

- **자동 감지 및 설치** — Echobird가 설치된 에이전트를 감지하고 누락된 것을 원클릭으로 배포
- **플러그앤플레이** — `plugin.json`을 tools 폴더에 넣으면 바로 작동. 코드 변경 불필요
- **내장 런처** — 터미널 없이 지원되는 모든 에이전트를 시작

### 🔀 원클릭 모델 전환 — 모든 에이전트에서 즉시 모델 전환

- **비주얼 Model Nexus** — 모든 AI 모델(OpenAI, Anthropic, Gemini, DeepSeek, Ollama 또는 커스텀 엔드포인트)을 하나의 패널에서 관리
- **듀얼 프로토콜** — OpenAI API & Anthropic API. 에이전트마다 설정, 제로 설정 변경으로 전환
- **원클릭 적용** — 모델 카드를 선택하고 에이전트에 적용. JSON, TOML, `.env` 파일 편집 불필요

### 💻 원클릭 LLM 배포 — Qwen, DeepSeek, Llama, MiniMax를 로컬 또는 원격으로

- **로컬 LLM** — 내장 llama.cpp, vLLM, SGLang으로 오픈소스 모델 배포. 데이터는 기기를 벗어나지 않습니다
- **원격 LLM** — SSH를 통해 GPU 서버에 배포. Qwen 3.5, MiniMax M2.5, GLM-5 또는 GGUF/HuggingFace 모델을 원클릭으로 시작
- **통합 프록시** — OpenAI (`/v1`)와 Anthropic (`/anthropic`) 엔드포인트를 자동 제공. 모든 에이전트가 즉시 연결
- **스마트 GPU 감지** — NVIDIA GPU를 자동 감지하고 최적 설정 추천

### 📡 Channels — 하나의 화면에서 여러 에이전트 제어

- **멀티 에이전트 채널** — OpenClaw, ZeroClaw 또는 Bridge 호환 에이전트를 병렬 실행
- **로컬 & 원격** — 로컬 에이전트는 Bridge 프로토콜, 원격 에이전트는 SSH 터널. 동일한 UI, 동일한 경험
- **영구 세션** — 앱 재시작 후에도 에이전트 대화가 유지. 중단한 곳에서 이어서
- **MotherAgent** — Tool Calling, 스킬 시스템, 유연한 모델 설정을 갖춘 자율 AI 에이전트

### 🧩 추가 내장 기능

- 🌐 **스마트 터널 프록시** — 풀 VPN 없이 지역 제한 API 접근
- 🎯 **스킬 브라우저** — AI 스킬을 원클릭으로 검색, 번역, 설치
- 🎮 **내장 AI 앱** — Reversi, AI 번역 등
- 🌍 **28개 언어** — 영어부터 아랍어까지 완전 국제화

---

## 🖼️ 스크린샷

### Model Nexus — OpenAI, Anthropic, Gemini, DeepSeek, Ollama — 모두 하나의 패널에서
![Model Nexus](./1.png)

### App Manager — OpenClaw, Claude Code, Codex 등의 모델을 원클릭 전환
![App Manager](./2.png)

### 로컬 LLM — llama.cpp / vLLM / SGLang으로 Qwen, Llama, DeepSeek 로컬 배포
![Local Server](./3.png)

### 스킬 브라우저 — OpenClaw, Claude Code 등의 Skills를 원클릭 번역 및 설치
![Skill Browser](./4.png)

---

## 🚀 다운로드

| 플랫폼 | 다운로드 |
|--------|---------|
| 🪟 Windows | [Echobird-x64-setup.exe](https://github.com/edison7009/Echobird-MotherAgent/releases/latest) |
| 🍎 macOS (Apple Silicon) | [Echobird_aarch64.dmg](https://github.com/edison7009/Echobird-MotherAgent/releases/latest) |
| 🍎 macOS (Intel) | [Echobird_x64.dmg](https://github.com/edison7009/Echobird-MotherAgent/releases/latest) |
| 🐧 Linux | [Echobird_amd64.AppImage](https://github.com/edison7009/Echobird-MotherAgent/releases/latest) |

**Linux 빠른 시작:**
```bash
chmod +x Echobird_*.AppImage
./Echobird_*.AppImage
# FUSE 필요? sudo apt install libfuse2
```

---

## 🔧 지원 도구

### 에이전트 & 코딩 도구

| 도구 | 프로토콜 | 설치 |
|------|----------|------|
| OpenClaw | OpenAI / Anthropic | 원클릭 |
| Claude Code | Anthropic | 원클릭 |
| OpenCode | OpenAI | 원클릭 |
| ZeroClaw | OpenAI | 원클릭 |
| Codex | OpenAI | 원클릭 |
| Cline | OpenAI | 설정 |
| Roo Code | OpenAI | 설정 |
| Continue | OpenAI | 설정 |
| Aider | OpenAI / Anthropic | 설정 |

### 로컬 LLM 런타임

| 런타임 | 모델 | 플랫폼 |
|--------|------|--------|
| llama.cpp | Qwen 3.5, Llama 4, DeepSeek, MiniMax M2.5, GLM-5 (GGUF) | Windows / macOS / Linux |
| vLLM | 모든 HuggingFace 모델 | Linux (CUDA) |
| SGLang | 모든 HuggingFace 모델 | Linux (CUDA) |

---

## 🏗️ 기술 스택

**Tauri 2** + **Rust** + **React** + **TypeScript** + **llama.cpp**

---

## 📬 문의

- 📧 [hi@echobird.ai](mailto:hi@echobird.ai) (Bug Reports)
- 🌐 [echobird.ai](https://echobird.ai)

---

<p align="center">
  <em>AI 시대의 사이버펑크 컨트롤 패널.</em><br/>
  Echobird 팀이 💚으로 제작<br/>
  <sub>⭐ <a href="https://github.com/edison7009/Echobird-MotherAgent">GitHub에서 Star 주기</a> — 더 많은 사람들이 프로젝트를 발견할 수 있도록!</sub>
</p>
