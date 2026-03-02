<p align="center">
  <img src="../build/icon.png" alt="Echobird" width="120" />
</p>

<h1 align="center">Echobird</h1>

<p align="center">
  The Nexus for <strong>Models</strong>, <strong>Agents</strong> & <strong>Vibe Coding</strong>.<br/>
  <sub>Echobird는 AI 코딩 도구 전반에 걸쳐 모델을 관리하기 위한 시각적이고 통합된 인터페이스를 제공하는 애플리케이션입니다.</sub>
</p>

<p align="center">
  <a href="https://github.com/edison7009/Echobird/releases">
    <img src="https://img.shields.io/github/v/release/edison7009/Echobird?style=flat-square&color=00FF9D" alt="Release" />
  </a>
  <img src="https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-blue?style=flat-square" alt="Platform" />
  <img src="https://img.shields.io/github/license/edison7009/Echobird?style=flat-square" alt="License" />
</p>

<p align="center">
  <a href="../README.md">English</a> · <a href="./README.zh-CN.md">简体中文</a> · <a href="./README.zh-TW.md">繁體中文</a> · <a href="./README.ja.md">日本語</a> · **한국어**
</p>

---

## ✨ Echobird란?

Echobird는 AI 코딩 도구 전반에 걸쳐 모델을 관리하기 위한 **시각적이고 통합된 인터페이스**를 제공하는 애플리케이션입니다.

### 문제점

- 😫 OpenClaw 같은 도구에서 AI 모델을 전환하려면 설정 파일을 수동으로 편집해야 함
- 🔄 각 도구마다 고유한 구성 형식이 있음
- 🧩 스킬과 확장 기능을 관리할 편리한 방법이 없음

### 솔루션

Echobird는 모든 AI 코딩 도구의 **중앙 제어 패널** 역할을 합니다:

- 🎯 **원클릭 모델 전환** — AI 모델을 시각적으로 전환
- 🔀 **듀얼 프로토콜** — OpenAI & Anthropic API 지원
- 🚇 **스마트 터널 프록시** — VPN 없이 지역 제한 API에 접근
- 🧩 **스킬 브라우저** — AI 스킬 검색, 설치, 관리
- 🖥️ **로컬 모델 서버** — llama.cpp로 오픈소스 모델 로컬 실행
- 🌍 **28개 언어** — 글로벌 대응 완전 국제화
- 🎮 **내장 AI 앱** — Reversi, AI 번역 등
- 🌃 **사이버펑크 UI** — 미래지향적 코딩 경험

## 🖼️ 스크린샷

### Model Nexus — 한 곳에서 모든 모델 관리
![Model Nexus](./1.png)

### App Manager — 원클릭 모델 전환
![App Manager](./2.png)

### Local Server — 로컬 실행
![Local Server](./3.png)

### Skill Browser — 검색 및 설치
![Skill Browser](./4.png)

## 🚀 빠른 시작

### 다운로드

최신 릴리스를 받으세요:

| 플랫폼 | 다운로드 |
|----------|----------|
| Windows  | [Echobird-Setup.exe](https://github.com/edison7009/Echobird/releases/latest) |
| macOS    | [Echobird.dmg](https://github.com/edison7009/Echobird/releases/latest) |
| Linux    | [Echobird.AppImage](https://github.com/edison7009/Echobird/releases/latest) |

### Linux 참고사항

```bash
chmod +x Echobird-*.AppImage
./Echobird-*.AppImage
```

> FUSE 오류가 발생하면： `sudo apt install libfuse2`

## 🔧 지원 도구

| 도구 | 상태 | 모델 전환 | 프로토콜 |
|------|--------|----------------|----------|
| OpenClaw | ✅ 지원 | ✅ | OpenAI / Anthropic |
| Claude Code | ✅ 지원 | ✅ | Anthropic |
| Cline | ✅ 지원 | ✅ | OpenAI |
| Continue | ✅ 지원 | ✅ | OpenAI |
| OpenCode | ✅ 지원 | ✅ | OpenAI |
| Codex | ✅ 지원 | ✅ | OpenAI |
| Roo Code | ✅ 지원 | ✅ | OpenAI |

## 🏗️ 기술 스택

- **Electron** — 프레임워크
- **React + TypeScript** — UI
- **Vanilla CSS** — 디자인
- **Vite** — 构建工具
- **llama.cpp** — 추론 엔진

## 🛠️ 개발

```bash
npm install
npm run dev
npm run build
```

## 🤝 기여

기여를 환영합니다!

We're especially looking for help with:
- 🍎 **macOS 测试**
- 🔧 **新工具集成**
- 🌐 **翻译改进**

## ⭐ 지원

Echobird가 유용하다면, GitHub에서 ⭐를 눌러주세요!

## 📄 라이선스

[MIT](../LICENSE)

---

<p align="center">
  Echobird 팀이 💚으로 제작<br/>
  <sub>📧 <a href="mailto:hi@echobird.ai">hi@echobird.ai</a></sub>
</p>
