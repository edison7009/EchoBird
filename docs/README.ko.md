<p align="center">
  <img src="./icon.png" alt="Echobird" width="120" />
</p>

<h1 align="center">Echobird</h1>

<p align="center">
  모델, 에이전트, Vibe Coding 의 넥서스。<br/>
  <sub>AI 시대의 사이버펑크 컨트롤 패널 — Tauri + Rust 기반.</sub>
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

## 🤖 MotherAgent — 모델을 배포. 에이전트를 실행.

**MotherAgent**는 자율 AI 에이전트입니다 — 로컬 LLM 배포, 원격 모델 연결, OpenClaw 실행을 한 곳에서.

- 🖥️ **로컬 LLM 배포** — 내장 llama.cpp로 Qwen, DeepSeek, Llama 등을 원클릭 배포. 데이터는 내 기기를 떠나지 않습니다.
- 🌐 **원격 LLM** — OpenAI, Anthropic, Google Gemini 또는 OpenAI 호환 엔드포인트에 즉시 연결.
- 🦅 **OpenClaw 배포** — MotherAgent에서 직접 OpenClaw 에이전트를 실행·관리. 터미널 불필요.
- 💾 **영구 세션** — 앱을 재시작해도 에이전트 대화가 유지됩니다. 중단한 곳에서 이어서.
- ⚡ **모든 프로토콜** — OpenAI API & Anthropic API. 에이전트마다 프로토콜을 설정해 설정 변경 없이 전환.

---

## ✨ Echobird — 모델을 전환. 설정 파일이 아닌.

Echobird는 모든 AI 코딩 도구의 **비주얼 컨트롤 패널**입니다. 포인트, 클릭, 전환.

- 🎯 **원클릭 모델 전환** — 지원 도구의 AI 모델을 시각적으로 설정. JSON 파일 더 이상 없음.
- 🔀 **듀얼 프로토콜** — OpenAI & Anthropic API. 언제든지 전환.
- 🚇 **스마트 프록시** — VPN 없이 지역 제한 API 접근.
- 🧩 **스킬 브라우저** — 여러 도구에서 AI 스킬을 발견·설치.
- 🎮 **내장 AI 앱** — Reversi, AI 번역 등. 계속 추가 중.
- 🌍 **28개 언어** — 전 세계 개발자를 위한 완전 국제화.

---

## 🖼️ 스크린샷

### Model Nexus — 한 곳에서 모든 AI 모델 관리
![Model Nexus](./1.png)

### App Manager — 모든 코딩 도구의 모델을 원클릭 전환
![App Manager](./2.png)

### Local Server — llama.cpp로 오픈소스 모델 로컬 실행
![Local Server](./3.png)

### Skill Browser — AI 스킬 발견 및 설치
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
# FUSE 오류? sudo apt install libfuse2
```

---

## 🔧 지원 도구

| 도구 | 프로토콜 |
|------|---------|
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

## 🏗️ 기술 스택

**Tauri 2** + **Rust** + **React** + **TypeScript** + **llama.cpp**

---

## 📬 문의

- 📧 [hi@echobird.ai](mailto:hi@echobird.ai)
- 🌐 [echobird.ai](https://echobird.ai)

---

<p align="center">
  Echobird 팀이 💚 으로 제작<br/>
  <sub>⭐ <a href="https://github.com/edison7009/Echobird-MotherAgent">GitHub에서 스타 주기</a> — 더 많은 사람들이 발견할 수 있습니다!</sub>
</p>
