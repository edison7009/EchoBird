<p align="center">
  <img src="./icon.png" alt="Echobird" width="120" />
</p>

<h1 align="center">EchoBird</h1>

<h3 align="center">KI-Agenten wie ein Profi bereitstellen — kein Terminal, keine Konfigurationsdateien, nur ein Klick.</h3>

<p align="center">
  OpenClaw, Claude Code, ZeroClaw und mehr installieren · Modelle auf lokalen und Remote-Servern wechseln · Alles auf einem Bildschirm.
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
  <a href="./README.ko.md">한국어</a> ·
  <a href="./README.es.md">Español</a> ·
  <a href="./README.fr.md">Français</a> ·
  <strong>Deutsch</strong> ·
  <a href="./README.pt.md">Português</a> ·
  <a href="./README.ru.md">Русский</a> ·
  <a href="./README.ar.md">العربية</a>
</p>

---

## Warum EchoBird?

Selbst als KI-Anfänger lässt EchoBird dich deinen eigenen Agent steuern — von der Einrichtung bis zur Arbeit — durch einfachen Chat. Keine Terminal-Erfahrung, keine Konfigurationsdateien, kein kompliziertes Deployment.

Du willst **OpenClaw**, **Claude Code**, **ZeroClaw** oder **Codex** nutzen? Ein Klick zum Installieren. **Qwen**, **DeepSeek** oder **Llama** auf deinem eigenen Rechner ausführen? Ein Klick zum Deployen. Modelle wechseln oder Skills hinzufügen? Klick, fertig.

**EchoBird gibt dir eine App für alles** — Agents installieren, Modelle wechseln, LLMs deployen und alles von einem Bildschirm steuern — egal ob Entwickler oder KI-Einsteiger.


<p align="center">
  <img src="./5.png" alt="Echobird Channels" width="100%" />
</p>

---

## ✨ Funktionen

### 🚀 Ein-Klick Installation — OpenClaw, Claude Code, OpenCode, ZeroClaw & mehr

- **Automatische Erkennung & Installation** — EchoBird erkennt installierte Agents und deployt fehlende mit einem Klick
- **Plug-and-Play Tools** — `plugin.json` in den Tools-Ordner legen und es funktioniert. Keine Code-Änderungen
- **Eingebauter Launcher** — Starte jeden unterstützten Agent ohne Terminal

### 🔀 Ein-Klick Modellwechsel — Modelle über alle Agents sofort wechseln

- **Visueller Model Nexus** — Verwalte alle KI-Modelle (OpenAI, Anthropic, Gemini, DeepSeek, Ollama oder benutzerdefinierte Endpoints) in einem Panel
- **Dual-Protokoll** — OpenAI API & Anthropic API. Pro Agent wechseln ohne Konfigurationsänderungen
- **Ein-Klick Anwendung** — Modellkarte auswählen, für jeden Agent aktivieren. Kein Editieren von JSON, TOML oder `.env`

### 💻 Ein-Klick LLM-Deployment — Qwen, DeepSeek, Llama, MiniMax lokal

- **Lokales LLM** — Open-Source-Modelle mit integriertem llama.cpp, vLLM oder SGLang deployen. Deine Daten verlassen nie das Gerät
- **Unified Proxy** — Stellt automatisch OpenAI (`/v1`) und Anthropic (`/anthropic`) Endpoints bereit. Jeden Agent sofort verbinden
- **Smarte GPU-Erkennung** — NVIDIA GPUs automatisch erkennen und optimale Einstellungen empfehlen

### 📡 Channels — Mehrere Agents von einem Bildschirm steuern

- **Multi-Agent Channels** — OpenClaw, ZeroClaw oder Bridge-kompatible Agents parallel ausführen
- **Lokal & Remote** — Lokale Agents über Bridge-Protokoll, Remote über SSH-Tunnel. Gleiche Oberfläche, gleiche Erfahrung
- **Persistente Sessions** — Agent-Gespräche überleben Neustarts. Genau dort weitermachen wo aufgehört
- **MotherAgent** — Dein autonomer KI-Agent mit Tool Calling, Skill-System und voller Modellflexibilität

### 🧩 Weitere integrierte Funktionen

- 🌐 **Smarter Tunnel-Proxy** — Geo-beschränkte APIs ohne komplettes VPN
- 🎮 **Eingebaute KI-Apps** — Reversi, AI Translate und mehr
- 🌍 **28 Sprachen** — Vollständige Internationalisierung von Englisch bis Arabisch

---

## 🖼️ Screenshots

### Model Nexus — OpenAI, Anthropic, Gemini, DeepSeek, Ollama — alles in einem Panel
![Model Nexus](./1.png)

### App Manager — Ein-Klick Modellwechsel für OpenClaw, Claude Code, Codex & mehr
![App Manager](./2.png)

### Lokales LLM — Qwen, Llama, DeepSeek lokal deployen via llama.cpp / vLLM / SGLang
![Local Server](./3.png)

### Channels — Mehrere Agenten auf einem Bildschirm steuern
![Channels](./4.png)

---

## 🚀 Download

| Plattform | Download |
|-----------|----------|
| 🪟 Windows | [Echobird-x64-setup.exe](https://github.com/edison7009/Echobird-MotherAgent/releases/latest) |
| 🍎 macOS (Apple Silicon) | [Echobird_aarch64.dmg](https://github.com/edison7009/Echobird-MotherAgent/releases/latest) |
| 🍎 macOS (Intel) | [Echobird_x64.dmg](https://github.com/edison7009/Echobird-MotherAgent/releases/latest) |
| 🐧 Linux | [Echobird_amd64.AppImage](https://github.com/edison7009/Echobird-MotherAgent/releases/latest) |

**Linux Schnellstart:**
```bash
chmod +x Echobird_*.AppImage
./Echobird_*.AppImage
# FUSE benötigt? sudo apt install libfuse2
```

---

## 🔧 Kompatibel mit

### Agents & Coding-Tools

| Tool | Protokoll | Installation |
|------|-----------|--------------|
| OpenClaw | OpenAI / Anthropic | Ein Klick |
| Claude Code | Anthropic | Ein Klick |
| OpenCode | OpenAI | Ein Klick |
| ZeroClaw | OpenAI | Ein Klick |
| Codex | OpenAI | Ein Klick |
| Cline | OpenAI | Config |
| Roo Code | OpenAI | Config |
| Continue | OpenAI | Config |
| Aider | OpenAI / Anthropic | Config |

### Lokale LLM-Runtimes

| Runtime | Modelle | Plattform |
|---------|---------|-----------|
| llama.cpp | Qwen 3.5, Llama 4, DeepSeek, MiniMax M2.5, GLM-5 (GGUF) | Windows / macOS / Linux |
| vLLM | Jedes HuggingFace-Modell | Linux (CUDA) |
| SGLang | Jedes HuggingFace-Modell | Linux (CUDA) |

---

## 🏗️ Tech Stack

**Tauri 2** + **Rust** + **React** + **TypeScript** + **llama.cpp**

---

## 📬 Kontakt

- 📧 [hi@EchoBird.ai](mailto:hi@EchoBird.ai) (Bug Reports)
- 🌐 [EchoBird.ai](https://echobird.ai)

---

<p align="center">
  <em>Das letzte Interface vor dem Zeitalter der KI.</em><br/>
  Mit 💚 vom EchoBird Team<br/>
  <sub>⭐ <a href="https://github.com/edison7009/Echobird-MotherAgent">Star auf GitHub</a> — hilft anderen das Projekt zu entdecken!</sub>
</p>
