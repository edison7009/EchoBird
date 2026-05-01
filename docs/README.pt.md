<p align="center">
  <img src="./icon.png" alt="Echobird" width="120" />
</p>

<h1 align="center">EchoBird</h1>

<h3 align="center">Implante agentes de IA como um profissional — sem terminal, sem configuração, com apenas um clique.</h3>

<p align="center">
  Instale OpenClaw, Claude Code, ZeroClaw e muito mais com um clique · Mude de modelo livremente em servidores locais · Controle todos os agentes em uma única tela.
</p>

<p align="center">
  <a href="https://github.com/edison7009/Echobird/releases">
    <img src="https://img.shields.io/github/v/release/edison7009/Echobird?style=flat-square&color=00FF9D" alt="Release" />
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
  <a href="./README.de.md">Deutsch</a> ·
  <strong>Português</strong> ·
  <a href="./README.ru.md">Русский</a> ·
  <a href="./README.ar.md">العربية</a>
</p>

---

## Por que EchoBird?

Mesmo como iniciante em IA, o EchoBird permite que você controle seu próprio Agent — da instalação ao trabalho — através de um simples chat. Sem experiência em terminal, sem arquivos de configuração, sem implantações complicadas.

Quer usar **OpenClaw**, **Claude Code**, **ZeroClaw** ou **Codex**? Um clique para instalar. Quer executar **Qwen**, **DeepSeek** ou **Llama** na sua própria máquina? Um clique para implantar. Trocar modelos ou adicionar skills? Aponte, clique, pronto.

**EchoBird oferece um app para tudo** — instalar agentes, trocar modelos, implantar LLMs e controlar tudo de uma tela — seja você desenvolvedor ou iniciante em IA.

<p align="center">
  <img src="./5.png" alt="Echobird Channels" width="100%" />
</p>

---

## ✨ Recursos

### 🚀 Instalação com Um Clique — OpenClaw, Claude Code, OpenCode, ZeroClaw e mais

- **Detecção e instalação automáticas** — EchoBird detecta agentes instalados e implanta os faltantes com um clique
- **Ferramentas plug-and-play** — Coloque um `plugin.json` na pasta tools e funciona. Sem alterações de código
- **Lançador integrado** — Inicie qualquer agente compatível sem tocar no terminal

### 🔀 Troca de Modelo com Um Clique — Troque modelos em todos os agentes instantaneamente

- **Model Nexus visual** — Gerencie todos os seus modelos de IA (OpenAI, Anthropic, Gemini, DeepSeek, Ollama ou qualquer endpoint personalizado) em um painel
- **Protocolo duplo** — OpenAI API e Anthropic API. Troque por agente sem alterações de configuração
- **Aplicar com um clique** — Selecione um cartão de modelo, ative para qualquer agente. Sem editar JSON, TOML ou `.env`

### 💻 Implantação de LLM com Um Clique — Execute Qwen, DeepSeek, Llama, MiniMax local

- **LLM local** — Implante modelos open-source com llama.cpp, vLLM ou SGLang integrados. Seus dados nunca saem do dispositivo
- **Proxy unificado** — Expõe automaticamente endpoints OpenAI (`/v1`) e Anthropic (`/anthropic`). Conecte qualquer agente instantaneamente
- **Detecção inteligente de GPU** — Detecta GPUs NVIDIA e recomenda configurações ideais

### 📡 Channels — Controle múltiplos agentes de uma tela

- **Canais multi-agente** — Execute OpenClaw, ZeroClaw ou qualquer agente compatível com Bridge em paralelo
- **Local e remoto** — Agentes locais via protocolo Bridge, remotos via túneis SSH. Mesma interface, mesma experiência
- **Sessões persistentes** — Conversas do agente sobrevivem a reinicializações. Continue de onde parou
- **MotherAgent** — Seu agente de IA autônomo com tool calling, sistema de skills e flexibilidade total de modelos

### 🧩 Mais recursos integrados

- 🌐 **Proxy túnel inteligente** — Acesse APIs com restrição geográfica sem VPN completo
- 🎮 **Apps de IA integrados** — Reversi, AI Translate e mais
- 🌍 **28 idiomas** — Internacionalização completa do inglês ao árabe

---

## 🖼️ Capturas de tela

### Model Nexus — OpenAI, Anthropic, Gemini, DeepSeek, Ollama — tudo em um painel
![Model Nexus](./1.png)

### App Manager — Troca de modelo com um clique para OpenClaw, Claude Code, Codex e mais
![App Manager](./2.png)

### LLM Local — Implante Qwen, Llama, DeepSeek localmente via llama.cpp / vLLM / SGLang
![Local Server](./3.png)

### Channels — Controle múltiplos agentes de uma tela
![Channels](./4.png)

---

## 🚀 Download

| Plataforma | Download |
|------------|----------|
| 🪟 Windows | [Echobird-x64-setup.exe](https://github.com/edison7009/Echobird/releases/latest) |
| 🍎 macOS (Apple Silicon) | [Echobird_aarch64.dmg](https://github.com/edison7009/Echobird/releases/latest) |
| 🍎 macOS (Intel) | [Echobird_x64.dmg](https://github.com/edison7009/Echobird/releases/latest) |
| 🐧 Linux | [Echobird_amd64.AppImage](https://github.com/edison7009/Echobird/releases/latest) |

**Início rápido Linux:**
```bash
chmod +x Echobird_*.AppImage
./Echobird_*.AppImage
# Precisa de FUSE? sudo apt install libfuse2
```

---

## 🔧 Compatível com

### Agentes e ferramentas de codificação

| Ferramenta | Protocolo | Instalação |
|------------|-----------|------------|
| OpenClaw | OpenAI / Anthropic | Um clique |
| Claude Code | Anthropic | Um clique |
| OpenCode | OpenAI | Um clique |
| ZeroClaw | OpenAI | Um clique |
| NanoBot | OpenAI / Anthropic | Um clique |
| PicoClaw | OpenAI / Anthropic | Um clique |
| Hermes Agent | OpenAI / Anthropic | Um clique |
| Codex | OpenAI | Um clique |
| Cline | OpenAI | Config |
| Roo Code | OpenAI | Config |
| Continue | OpenAI | Config |
| Aider | OpenAI / Anthropic | Config |

### Runtimes de LLM local

| Runtime | Modelos | Plataforma |
|---------|---------|------------|
| llama.cpp | Qwen 3.5, Llama 4, DeepSeek, MiniMax M2.5, GLM-5 (GGUF) | Windows / macOS / Linux |
| vLLM | Qualquer modelo HuggingFace | Linux (CUDA) |
| SGLang | Qualquer modelo HuggingFace | Linux (CUDA) |

---

## 🏗️ Stack técnico

**Tauri 2** + **Rust** + **React** + **TypeScript** + **llama.cpp**

---

## 📬 Contato

- 📧 [hi@EchoBird.ai](mailto:hi@EchoBird.ai) (Bug Reports)
- 🌐 [EchoBird.ai](https://echobird.ai)

---

<p align="center">
  <em>A última interface antes da era da IA.</em><br/>
  Feito com 💚 pelo time EchoBird<br/>
  <sub>⭐ <a href="https://github.com/edison7009/Echobird">Star no GitHub</a> — ajude outros a descobrir o projeto!</sub>
</p>
