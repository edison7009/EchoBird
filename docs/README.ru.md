<p align="center">
  <img src="./icon.png" alt="Echobird" width="120" />
</p>

<h1 align="center">Echobird</h1>

<h3 align="center">Установите OpenClaw, Claude Code, ZeroClaw и Codex в один клик. Переключайте модели. Разворачивайте LLM.</h3>

<p align="center">
  Одно приложение для установки агентов, переключения моделей, развёртывания локальных/удалённых LLM и управления всеми агентами с экрана Channels.<br/>
  <sub>Кроссплатформенная десктопная ИИ-панель управления — построена на Tauri 2 + Rust.</sub>
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
  <a href="./README.de.md">Deutsch</a> ·
  <a href="./README.pt.md">Português</a> ·
  <strong>Русский</strong> ·
  <a href="./README.ar.md">العربية</a>
</p>

---

## Почему Echobird?

Даже если вы новичок в ИИ, Echobird позволяет управлять собственным Agent — от установки до работы — через простой чат. Без опыта работы с терминалом, без конфигурационных файлов, без сложного развёртывания.

Хотите использовать **OpenClaw**, **Claude Code**, **ZeroClaw** или **Codex**? Один клик для установки. Запустить **Qwen**, **DeepSeek** или **Llama** на своём компьютере? Один клик для развёртывания. Переключить модель или добавить навыки? Нажмите — готово.

**Echobird — одно приложение для всего** — установка агентов, переключение моделей, развёртывание LLM и управление всеми агентами с одного экрана — будь вы разработчик или новичок в ИИ.

---

## ✨ Функции

### 🚀 Установка в Один Клик — OpenClaw, Claude Code, OpenCode, ZeroClaw и другие

- **Автоматическое обнаружение и установка** — Echobird обнаруживает установленные агенты и разворачивает недостающие одним кликом
- **Plug-and-play инструменты** — Поместите `plugin.json` в папку tools и всё работает. Без изменения кода
- **Встроенный лаунчер** — Запускайте любого поддерживаемого агента без терминала

### 🔀 Переключение Модели в Один Клик — Мгновенная смена модели для всех агентов

- **Визуальный Model Nexus** — Управляйте всеми ИИ-моделями (OpenAI, Anthropic, Gemini, DeepSeek, Ollama или любой пользовательский endpoint) в одной панели
- **Двойной протокол** — OpenAI API и Anthropic API. Переключайте по агенту без изменения конфигурации
- **Применение в один клик** — Выберите карточку модели, активируйте для любого агента. Без редактирования JSON, TOML или `.env`

### 💻 Развёртывание LLM в Один Клик — Запускайте Qwen, DeepSeek, Llama, MiniMax локально или удалённо

- **Локальный LLM** — Разворачивайте open-source модели через встроенный llama.cpp, vLLM или SGLang. Данные никогда не покидают устройство
- **Удалённый LLM** — Разворачивайте на любом GPU-сервере через SSH. Запускайте Qwen 3.5, MiniMax M2.5, GLM-5 или любую модель GGUF/HuggingFace одним кликом
- **Единый прокси** — Автоматически предоставляет endpoints OpenAI (`/v1`) и Anthropic (`/anthropic`). Подключайте любого агента мгновенно
- **Умное обнаружение GPU** — Автоматическое определение GPU NVIDIA и рекомендация оптимальных настроек

### 📡 Channels — Управление несколькими агентами с одного экрана

- **Мульти-агентные каналы** — Запускайте OpenClaw, ZeroClaw или любых Bridge-совместимых агентов параллельно
- **Локально и удалённо** — Локальные агенты через Bridge-протокол, удалённые через SSH-туннели. Один интерфейс, один опыт
- **Постоянные сессии** — Беседы агентов сохраняются после перезапуска. Продолжайте с того места, где остановились
- **MotherAgent** — Ваш автономный ИИ-агент с tool calling, системой навыков и полной гибкостью моделей

### 🧩 Дополнительные встроенные функции

- 🌐 **Умный туннельный прокси** — Доступ к гео-ограниченным API без полного VPN
- 🎯 **Браузер навыков** — Находите, переводите и устанавливайте ИИ-навыки одним кликом
- 🎮 **Встроенные ИИ-приложения** — Reversi, AI Translate и другие
- 🌍 **28 языков** — Полная интернационализация от английского до арабского

---

## 🖼️ Скриншоты

### Model Nexus — OpenAI, Anthropic, Gemini, DeepSeek, Ollama — всё в одной панели
![Model Nexus](./1.png)

### App Manager — Переключение модели одним кликом для OpenClaw, Claude Code, Codex и других
![App Manager](./2.png)

### Локальный LLM — Разворачивайте Qwen, Llama, DeepSeek локально через llama.cpp / vLLM / SGLang
![Local Server](./3.png)

### Браузер навыков — Переводите и устанавливайте Skills для OpenClaw, Claude Code и других одним кликом
![Skill Browser](./4.png)

---

## 🚀 Скачать

| Платформа | Скачать |
|-----------|---------|
| 🪟 Windows | [Echobird-x64-setup.exe](https://github.com/edison7009/Echobird-MotherAgent/releases/latest) |
| 🍎 macOS (Apple Silicon) | [Echobird_aarch64.dmg](https://github.com/edison7009/Echobird-MotherAgent/releases/latest) |
| 🍎 macOS (Intel) | [Echobird_x64.dmg](https://github.com/edison7009/Echobird-MotherAgent/releases/latest) |
| 🐧 Linux | [Echobird_amd64.AppImage](https://github.com/edison7009/Echobird-MotherAgent/releases/latest) |

**Быстрый старт Linux:**
```bash
chmod +x Echobird_*.AppImage
./Echobird_*.AppImage
# Нужен FUSE? sudo apt install libfuse2
```

---

## 🔧 Совместим с

### Агенты и инструменты программирования

| Инструмент | Протокол | Установка |
|------------|----------|-----------|
| OpenClaw | OpenAI / Anthropic | Один клик |
| Claude Code | Anthropic | Один клик |
| OpenCode | OpenAI | Один клик |
| ZeroClaw | OpenAI | Один клик |
| Codex | OpenAI | Один клик |
| Cline | OpenAI | Конфиг |
| Roo Code | OpenAI | Конфиг |
| Continue | OpenAI | Конфиг |
| Aider | OpenAI / Anthropic | Конфиг |

### Локальные LLM-рантаймы

| Рантайм | Модели | Платформа |
|---------|--------|-----------|
| llama.cpp | Qwen 3.5, Llama 4, DeepSeek, MiniMax M2.5, GLM-5 (GGUF) | Windows / macOS / Linux |
| vLLM | Любая модель HuggingFace | Linux (CUDA) |
| SGLang | Любая модель HuggingFace | Linux (CUDA) |

---

## 🏗️ Технологический стек

**Tauri 2** + **Rust** + **React** + **TypeScript** + **llama.cpp**

---

## 📬 Контакты

- 📧 [hi@echobird.ai](mailto:hi@echobird.ai)
- 🌐 [echobird.ai](https://echobird.ai)

---

<p align="center">
  <em>Киберпанк-панель управления для эпохи ИИ.</em><br/>
  Сделано с 💚 командой Echobird<br/>
  <sub>⭐ <a href="https://github.com/edison7009/Echobird-MotherAgent">Star на GitHub</a> — помогите другим найти этот проект!</sub>
</p>
