<p align="center">
  <img src="./icon.png" alt="Echobird" width="120" />
</p>

<h1 align="center">EchoBird</h1>

<h3 align="center">Déployez des agents IA comme un pro — sans terminal, sans configuration, en un seul clic.</h3>

<p align="center">
  Installez OpenClaw, Claude Code, ZeroClaw et bien plus en un clic · Changez librement de modèle sur les serveurs locaux · Contrôlez tous les agents depuis un seul écran.
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
  <strong>Français</strong> ·
  <a href="./README.de.md">Deutsch</a> ·
  <a href="./README.pt.md">Português</a> ·
  <a href="./README.ru.md">Русский</a> ·
  <a href="./README.ar.md">العربية</a>
</p>

---

## Pourquoi EchoBird ?

Même en tant que débutant en IA, EchoBird vous permet de contrôler votre propre Agent — de l'installation au travail — par un simple chat. Pas besoin d'expérience en terminal, pas de fichiers de configuration, pas de déploiements compliqués.

Vous voulez utiliser **OpenClaw**, **Claude Code**, **ZeroClaw** ou **Codex** ? Un clic pour installer. Exécuter **Qwen**, **DeepSeek** ou **Llama** sur votre propre machine ? Un clic pour déployer. Changer de modèle ou ajouter des skills ? Pointez, cliquez, c'est fait.

**EchoBird vous offre une seule app pour tout** — installer des agents, changer de modèle, déployer des LLMs et tout contrôler depuis un écran — que vous soyez développeur ou débutant en IA.

<p align="center">
  <img src="./5.png" alt="Echobird Channels" width="100%" />
</p>

---

## ✨ Fonctionnalités

### 🚀 Installation en Un Clic — OpenClaw, Claude Code, OpenCode, ZeroClaw et plus

- **Détection et installation automatiques** — EchoBird détecte les agents installés et déploie ceux manquants en un clic
- **Outils plug-and-play** — Déposez un `plugin.json` dans le dossier tools et ça marche. Aucun changement de code
- **Lanceur intégré** — Démarrez tout agent compatible sans toucher au terminal

### 🔀 Changement de Modèle en Un Clic — Changez de modèle sur tous les agents instantanément

- **Model Nexus visuel** — Gérez tous vos modèles IA (OpenAI, Anthropic, Gemini, DeepSeek, Ollama ou tout endpoint personnalisé) dans un panneau
- **Double protocole** — OpenAI API et Anthropic API. Changez par agent sans modification de configuration
- **Application en un clic** — Sélectionnez une carte modèle, activez-la pour tout agent. Plus d'édition de JSON, TOML ou `.env`

### 💻 Déploiement LLM en Un Clic — Exécutez Qwen, DeepSeek, Llama, MiniMax localement

- **LLM local** — Déployez des modèles open-source avec llama.cpp, vLLM ou SGLang intégrés. Vos données ne quittent jamais l'appareil
- **Proxy unifié** — Expose automatiquement les endpoints OpenAI (`/v1`) et Anthropic (`/anthropic`). Connectez tout agent instantanément
- **Détection GPU intelligente** — Détecte les GPU NVIDIA et recommande les paramètres optimaux

### 📡 Channels — Contrôlez plusieurs agents depuis un écran

- **Canaux multi-agents** — Exécutez OpenClaw, ZeroClaw ou tout agent compatible Bridge en parallèle
- **Local et distant** — Agents locaux via protocole Bridge, distants via tunnels SSH. Même interface, même expérience
- **Sessions persistantes** — Les conversations de l'agent survivent au redémarrage. Reprenez où vous en étiez
- **MotherAgent** — Votre agent IA autonome avec tool calling, système de skills et flexibilité totale de modèles

### 🧩 Plus de fonctions intégrées

- 🌐 **Proxy tunnel intelligent** — Accédez aux APIs géo-restreintes sans VPN complet
- 🎮 **Apps IA intégrées** — Reversi, AI Translate et plus
- 🌍 **28 langues** — Internationalisation complète de l'anglais à l'arabe

---

## 🖼️ Captures d'écran

### Model Nexus — OpenAI, Anthropic, Gemini, DeepSeek, Ollama — tout dans un panneau
![Model Nexus](./1.png)

### App Manager — Changement de modèle en un clic pour OpenClaw, Claude Code, Codex et plus
![App Manager](./2.png)

### LLM Local — Déployez Qwen, Llama, DeepSeek localement via llama.cpp / vLLM / SGLang
![Local Server](./3.png)

### Channels — Contrôlez plusieurs agents depuis un écran
![Channels](./4.png)

---

## 🚀 Télécharger

| Plateforme | Téléchargement |
|------------|----------------|
| 🪟 Windows | [Echobird-x64-setup.exe](https://github.com/edison7009/Echobird-MotherAgent/releases/latest) |
| 🍎 macOS (Apple Silicon) | [Echobird_aarch64.dmg](https://github.com/edison7009/Echobird-MotherAgent/releases/latest) |
| 🍎 macOS (Intel) | [Echobird_x64.dmg](https://github.com/edison7009/Echobird-MotherAgent/releases/latest) |
| 🐧 Linux | [Echobird_amd64.AppImage](https://github.com/edison7009/Echobird-MotherAgent/releases/latest) |

**Démarrage rapide Linux :**
```bash
chmod +x Echobird_*.AppImage
./Echobird_*.AppImage
# Besoin de FUSE ? sudo apt install libfuse2
```

---

## 🔧 Compatible avec

### Agents et outils de codage

| Outil | Protocole | Installation |
|-------|-----------|--------------|
| OpenClaw | OpenAI / Anthropic | Un clic |
| Claude Code | Anthropic | Un clic |
| OpenCode | OpenAI | Un clic |
| ZeroClaw | OpenAI | Un clic |
| NanoBot | OpenAI / Anthropic | Un clic |
| PicoClaw | OpenAI / Anthropic | Un clic |
| Hermes Agent | OpenAI / Anthropic | Un clic |
| Codex | OpenAI | Un clic |
| Cline | OpenAI | Config |
| Roo Code | OpenAI | Config |
| Continue | OpenAI | Config |
| Aider | OpenAI / Anthropic | Config |

### Runtimes LLM locaux

| Runtime | Modèles | Plateforme |
|---------|---------|------------|
| llama.cpp | Qwen 3.5, Llama 4, DeepSeek, MiniMax M2.5, GLM-5 (GGUF) | Windows / macOS / Linux |
| vLLM | Tout modèle HuggingFace | Linux (CUDA) |
| SGLang | Tout modèle HuggingFace | Linux (CUDA) |

---

## 🏗️ Stack technique

**Tauri 2** + **Rust** + **React** + **TypeScript** + **llama.cpp**

---

## 📬 Contact

- 📧 [hi@EchoBird.ai](mailto:hi@EchoBird.ai) (Bug Reports)
- 🌐 [EchoBird.ai](https://echobird.ai)

---

<p align="center">
  <em>La dernière interface avant l'ère de l'IA.</em><br/>
  Fait avec 💚 par l'équipe EchoBird<br/>
  <sub>⭐ <a href="https://github.com/edison7009/Echobird-MotherAgent">Star sur GitHub</a> — aidez d'autres à découvrir le projet !</sub>
</p>
