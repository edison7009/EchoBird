<p align="center">
  <img src="./icon.png" alt="Echobird" width="120" />
</p>

<h1 align="center">Echobird</h1>

<h3 align="center">ثبّت OpenClaw وClaude Code وZeroClaw وCodex بنقرة واحدة. بدّل النماذج. انشر LLM.</h3>

<p align="center">
  تطبيق واحد لتثبيت الوكلاء، تبديل النماذج، نشر LLMs محلية/بعيدة، والتحكم بجميع الوكلاء من شاشة Channels واحدة.<br/>
  <sub>لوحة تحكم ذكاء اصطناعي متعددة المنصات — مبنية بـ Tauri 2 + Rust.</sub>
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
  <a href="./README.ru.md">Русский</a> ·
  <strong>العربية</strong>
</p>

---

## لماذا Echobird؟

حتى لو كنت مبتدئاً في الذكاء الاصطناعي، يتيح لك Echobird التحكم بوكيلك الخاص — من التثبيت إلى العمل — عبر محادثة بسيطة. بدون خبرة في الطرفية، بدون ملفات إعدادات، بدون نشر معقد.

تريد استخدام **OpenClaw** أو **Claude Code** أو **ZeroClaw** أو **Codex**؟ نقرة واحدة للتثبيت. تريد تشغيل **Qwen** أو **DeepSeek** أو **Llama** على جهازك؟ نقرة واحدة للنشر. تبديل النماذج أو إضافة مهارات؟ انقر وانتهى الأمر.

**Echobird يمنحك تطبيقاً واحداً لكل شيء** — تثبيت الوكلاء، تبديل النماذج، نشر LLMs، والتحكم بكل شيء من شاشة واحدة — سواء كنت مطوراً أو مبتدئاً في الذكاء الاصطناعي.

---

## ✨ الميزات

### 🚀 تثبيت بنقرة واحدة — OpenClaw وClaude Code وOpenCode وZeroClaw والمزيد

- **كشف وتثبيت تلقائي** — Echobird يكتشف الوكلاء المثبتين وينشر المفقودين بنقرة واحدة
- **أدوات جاهزة للاستخدام** — ضع `plugin.json` في مجلد tools ويعمل فوراً. بدون تغيير كود
- **مُشغّل مدمج** — شغّل أي وكيل مدعوم بدون لمس الطرفية

### 🔀 تبديل النموذج بنقرة واحدة — بدّل النماذج عبر جميع الوكلاء فوراً

- **Model Nexus المرئي** — أدر جميع نماذج الذكاء الاصطناعي (OpenAI وAnthropic وGemini وDeepSeek وOllama أو أي نقطة نهاية مخصصة) في لوحة واحدة
- **بروتوكول مزدوج** — OpenAI API وAnthropic API. بدّل لكل وكيل بدون تغيير إعدادات
- **تطبيق بنقرة** — اختر بطاقة نموذج وفعّلها لأي وكيل. بدون تعديل JSON أو TOML أو `.env`

### 💻 نشر LLM بنقرة واحدة — شغّل Qwen وDeepSeek وLlama وMiniMax محلياً أو عن بُعد

- **LLM محلي** — انشر نماذج مفتوحة المصدر عبر llama.cpp أو vLLM أو SGLang المدمجة. بياناتك لا تغادر جهازك أبداً
- **LLM بعيد** — انشر على أي خادم GPU عبر SSH. شغّل Qwen 3.5 أو MiniMax M2.5 أو GLM-5 أو أي نموذج GGUF/HuggingFace بنقرة
- **وكيل موحد** — يكشف تلقائياً نقاط نهاية OpenAI (`/v1`) وAnthropic (`/anthropic`). اربط أي وكيل فوراً
- **كشف GPU ذكي** — يكتشف تلقائياً NVIDIA GPUs ويوصي بالإعدادات المثلى

### 📡 Channels — تحكم بعدة وكلاء من شاشة واحدة

- **قنوات متعددة الوكلاء** — شغّل OpenClaw أو ZeroClaw أو أي وكيل متوافق مع Bridge بالتوازي
- **محلي وبعيد** — الوكلاء المحليون عبر بروتوكول Bridge، البعيدون عبر أنفاق SSH. نفس الواجهة، نفس التجربة
- **جلسات دائمة** — محادثات الوكيل تبقى بعد إعادة التشغيل. استأنف من حيث توقفت
- **MotherAgent** — وكيلك المستقل للذكاء الاصطناعي مع tool calling ونظام مهارات ومرونة كاملة في النماذج

### 🧩 المزيد من الميزات المدمجة

- 🌐 **وكيل نفق ذكي** — ادخل إلى APIs المحظورة جغرافياً بدون VPN كامل
- 🎯 **متصفح المهارات** — اكتشف وترجم وثبّت مهارات الذكاء الاصطناعي بنقرة
- 🎮 **تطبيقات ذكاء اصطناعي مدمجة** — Reversi وAI Translate والمزيد
- 🌍 **28 لغة** — دعم كامل للتدويل من الإنجليزية إلى العربية

---

## 🖼️ لقطات الشاشة

### Model Nexus — OpenAI وAnthropic وGemini وDeepSeek وOllama — الكل في لوحة واحدة
![Model Nexus](./1.png)

### App Manager — تبديل النموذج بنقرة لـ OpenClaw وClaude Code وCodex والمزيد
![App Manager](./2.png)

### LLM محلي — انشر Qwen وLlama وDeepSeek محلياً عبر llama.cpp / vLLM / SGLang
![Local Server](./3.png)

### متصفح المهارات — ترجم وثبّت Skills لـ OpenClaw وClaude Code والمزيد بنقرة
![Skill Browser](./4.png)

---

## 🚀 تحميل

| المنصة | تحميل |
|--------|-------|
| 🪟 Windows | [Echobird-x64-setup.exe](https://github.com/edison7009/Echobird-MotherAgent/releases/latest) |
| 🍎 macOS (Apple Silicon) | [Echobird_aarch64.dmg](https://github.com/edison7009/Echobird-MotherAgent/releases/latest) |
| 🍎 macOS (Intel) | [Echobird_x64.dmg](https://github.com/edison7009/Echobird-MotherAgent/releases/latest) |
| 🐧 Linux | [Echobird_amd64.AppImage](https://github.com/edison7009/Echobird-MotherAgent/releases/latest) |

**بدء سريع في Linux:**
```bash
chmod +x Echobird_*.AppImage
./Echobird_*.AppImage
# تحتاج FUSE؟ sudo apt install libfuse2
```

---

## 🔧 متوافق مع

### الوكلاء وأدوات البرمجة

| الأداة | البروتوكول | التثبيت |
|--------|-----------|---------|
| OpenClaw | OpenAI / Anthropic | نقرة واحدة |
| Claude Code | Anthropic | نقرة واحدة |
| OpenCode | OpenAI | نقرة واحدة |
| ZeroClaw | OpenAI | نقرة واحدة |
| Codex | OpenAI | نقرة واحدة |
| Cline | OpenAI | إعدادات |
| Roo Code | OpenAI | إعدادات |
| Continue | OpenAI | إعدادات |
| Aider | OpenAI / Anthropic | إعدادات |

### بيئات تشغيل LLM المحلية

| بيئة التشغيل | النماذج | المنصة |
|-------------|---------|--------|
| llama.cpp | Qwen 3.5, Llama 4, DeepSeek, MiniMax M2.5, GLM-5 (GGUF) | Windows / macOS / Linux |
| vLLM | أي نموذج HuggingFace | Linux (CUDA) |
| SGLang | أي نموذج HuggingFace | Linux (CUDA) |

---

## 🏗️ المكدس التقني

**Tauri 2** + **Rust** + **React** + **TypeScript** + **llama.cpp**

---

## 📬 تواصل

- 📧 [hi@echobird.ai](mailto:hi@echobird.ai) (Bug Reports)
- 🌐 [echobird.ai](https://echobird.ai)

---

<p align="center">
  <em>آخر واجهة قبل عصر الذكاء الاصطناعي.</em><br/>
  صُنع بـ 💚 من فريق Echobird<br/>
  <sub>⭐ <a href="https://github.com/edison7009/Echobird-MotherAgent">Star على GitHub</a> — ساعد الآخرين في اكتشاف المشروع!</sub>
</p>
