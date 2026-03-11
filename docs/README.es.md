<p align="center">
  <img src="./icon.png" alt="EchoBird" width="120" />
</p>

<h1 align="center">EchoBird</h1>

<h3 align="center">Instala OpenClaw, Claude Code, ZeroClaw y Codex con un clic. Cambia modelos. Despliega LLMs.</h3>

<p align="center">
  Una app para instalar agentes, cambiar modelos, desplegar LLMs locales/remotos y controlar todos los agentes desde una pantalla Channels.<br/>
  <sub>Panel de control de IA de escritorio multiplataforma — construido con Tauri 2 + Rust.</sub>
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
  <strong>Español</strong> ·
  <a href="./README.fr.md">Français</a> ·
  <a href="./README.de.md">Deutsch</a> ·
  <a href="./README.pt.md">Português</a> ·
  <a href="./README.ru.md">Русский</a> ·
  <a href="./README.ar.md">العربية</a>
</p>

---

## ¿Por qué EchoBird?

Incluso como principiante en IA, EchoBird te permite controlar tu propio Agent — desde la instalación hasta el trabajo — a través de un simple chat. Sin experiencia en terminal, sin archivos de configuración, sin despliegues complicados.

¿Quieres usar **OpenClaw**, **Claude Code**, **ZeroClaw** o **Codex**? Un clic para instalar. ¿Quieres ejecutar **Qwen**, **DeepSeek** o **Llama** en tu propia máquina? Un clic para desplegar. ¿Cambiar modelos o añadir skills? Apunta, haz clic, listo.

**EchoBird te da una app para todo** — instalar agentes, cambiar modelos, desplegar LLMs y controlar todo desde una pantalla — seas desarrollador o recién empieces con la IA.

---

## ✨ Características

### 🚀 Instalación con Un Clic — OpenClaw, Claude Code, OpenCode, ZeroClaw y más

- **Detección e instalación automática** — EchoBird detecta los agentes instalados y despliega los faltantes con un clic
- **Herramientas plug-and-play** — Coloca un `plugin.json` en la carpeta tools y funciona. Sin cambios de código
- **Lanzador integrado** — Inicia cualquier agente compatible sin tocar la terminal

### 🔀 Cambio de Modelo con Un Clic — Cambia modelos en todos los agentes al instante

- **Model Nexus visual** — Gestiona todos tus modelos de IA (OpenAI, Anthropic, Gemini, DeepSeek, Ollama o cualquier endpoint personalizado) en un panel
- **Protocolo dual** — OpenAI API y Anthropic API. Cambia por agente sin cambios de configuración
- **Aplicar con un clic** — Selecciona una tarjeta de modelo, actívala para cualquier agente. Sin editar JSON, TOML o `.env`

### 💻 Despliegue de LLM con Un Clic — Ejecuta Qwen, DeepSeek, Llama, MiniMax local o remotamente

- **LLM local** — Despliega modelos de código abierto con llama.cpp, vLLM o SGLang integrados. Tus datos nunca salen del dispositivo
- **LLM remoto** — Despliega en cualquier servidor GPU vía SSH. Inicia Qwen 3.5, MiniMax M2.5, GLM-5 o cualquier modelo GGUF/HuggingFace con un clic
- **Proxy unificado** — Expone automáticamente endpoints OpenAI (`/v1`) y Anthropic (`/anthropic`). Conecta cualquier agente al instante
- **Detección inteligente de GPU** — Detecta GPUs NVIDIA y recomienda configuración óptima

### 📡 Channels — Controla múltiples agentes desde una pantalla

- **Canales multi-agente** — Ejecuta OpenClaw, ZeroClaw o cualquier agente compatible con Bridge en paralelo
- **Local y remoto** — Agentes locales vía protocolo Bridge, remotos vía túneles SSH. Misma interfaz, misma experiencia
- **Sesiones persistentes** — Las conversaciones del agente sobreviven al reinicio de la app. Continúa donde lo dejaste
- **MotherAgent** — Tu agente de IA autónomo con tool calling, sistema de skills y flexibilidad total de modelos

### 🧩 Más funciones integradas

- 🌐 **Proxy túnel inteligente** — Accede a APIs geo-restringidas sin VPN completo
- 🎯 **Explorador de Skills** — Descubre, traduce e instala skills de IA con un clic
- 🎮 **Apps de IA integradas** — Reversi, AI Translate y más
- 🌍 **28 idiomas** — Internacionalización completa del inglés al árabe

---

## 🖼️ Capturas de pantalla

### Model Nexus — OpenAI, Anthropic, Gemini, DeepSeek, Ollama — todo en un panel
![Model Nexus](./1.png)

### App Manager — Cambio de modelo con un clic para OpenClaw, Claude Code, Codex y más
![App Manager](./2.png)

### LLM Local — Despliega Qwen, Llama, DeepSeek localmente con llama.cpp / vLLM / SGLang
![Local Server](./3.png)

### Explorador de Skills — Traduce e instala Skills para OpenClaw, Claude Code y más con un clic
![Skill Browser](./4.png)

---

## 🚀 Descargar

| Plataforma | Descarga |
|------------|----------|
| 🪟 Windows | [Echobird-x64-setup.exe](https://github.com/edison7009/Echobird-MotherAgent/releases/latest) |
| 🍎 macOS (Apple Silicon) | [Echobird_aarch64.dmg](https://github.com/edison7009/Echobird-MotherAgent/releases/latest) |
| 🍎 macOS (Intel) | [Echobird_x64.dmg](https://github.com/edison7009/Echobird-MotherAgent/releases/latest) |
| 🐧 Linux | [Echobird_amd64.AppImage](https://github.com/edison7009/Echobird-MotherAgent/releases/latest) |

**Inicio rápido en Linux:**
```bash
chmod +x Echobird_*.AppImage
./Echobird_*.AppImage
# ¿Necesitas FUSE? sudo apt install libfuse2
```

---

## 🔧 Compatible con

### Agentes y herramientas de codificación

| Herramienta | Protocolo | Instalación |
|-------------|-----------|-------------|
| OpenClaw | OpenAI / Anthropic | Un clic |
| Claude Code | Anthropic | Un clic |
| OpenCode | OpenAI | Un clic |
| ZeroClaw | OpenAI | Un clic |
| Codex | OpenAI | Un clic |
| Cline | OpenAI | Config |
| Roo Code | OpenAI | Config |
| Continue | OpenAI | Config |
| Aider | OpenAI / Anthropic | Config |

### Runtimes de LLM local

| Runtime | Modelos | Plataforma |
|---------|---------|------------|
| llama.cpp | Qwen 3.5, Llama 4, DeepSeek, MiniMax M2.5, GLM-5 (GGUF) | Windows / macOS / Linux |
| vLLM | Cualquier modelo HuggingFace | Linux (CUDA) |
| SGLang | Cualquier modelo HuggingFace | Linux (CUDA) |

---

## 🏗️ Stack tecnológico

**Tauri 2** + **Rust** + **React** + **TypeScript** + **llama.cpp**

---

## 📬 Contacto

- 📧 [hi@EchoBird.ai](mailto:hi@EchoBird.ai) (Bug Reports)
- 🌐 [EchoBird.ai](https://echobird.ai)

---

<p align="center">
  <em>La última interfaz antes de la era de la IA.</em><br/>
  Hecho con 💚 por el equipo EchoBird<br/>
  <sub>⭐ <a href="https://github.com/edison7009/Echobird-MotherAgent">Star en GitHub</a> — ¡ayuda a que otros descubran el proyecto!</sub>
</p>
