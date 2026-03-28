# Translate roles-xx.json Files (26 Languages)

## Overview

Translate the `name` and `description` fields in 26 language-specific role JSON files under `d:\Echobird\docs\roles\`. Each file is currently a copy of `roles-en.json` (English source). The `roles-zh-Hans.json` (Simplified Chinese) already has its own translations — **do not touch it**.

## Source File

- **English source**: `d:\Echobird\docs\roles\roles-en.json` (1354 lines, ~85KB)

## Files to Translate

| # | File | Target Language |
|---|------|----------------|
| 1 | `roles-ar.json` | Arabic (العربية) |
| 2 | `roles-bn.json` | Bengali (বাংলা) |
| 3 | `roles-cs.json` | Czech (Čeština) |
| 4 | `roles-de.json` | German (Deutsch) |
| 5 | `roles-el.json` | Greek (Ελληνικά) |
| 6 | `roles-es.json` | Spanish (Español) |
| 7 | `roles-fa.json` | Persian (فارسی) |
| 8 | `roles-fi.json` | Finnish (Suomi) |
| 9 | `roles-fr.json` | French (Français) |
| 10 | `roles-he.json` | Hebrew (עברית) |
| 11 | `roles-hi.json` | Hindi (हिन्दी) |
| 12 | `roles-hu.json` | Hungarian (Magyar) |
| 13 | `roles-id.json` | Indonesian (Bahasa Indonesia) |
| 14 | `roles-it.json` | Italian (Italiano) |
| 15 | `roles-ja.json` | Japanese (日本語) |
| 16 | `roles-ko.json` | Korean (한국어) |
| 17 | `roles-ms.json` | Malay (Bahasa Melayu) |
| 18 | `roles-nl.json` | Dutch (Nederlands) |
| 19 | `roles-pl.json` | Polish (Polski) |
| 20 | `roles-pt.json` | Portuguese (Português) |
| 21 | `roles-ru.json` | Russian (Русский) |
| 22 | `roles-sv.json` | Swedish (Svenska) |
| 23 | `roles-th.json` | Thai (ไทย) |
| 24 | `roles-tr.json` | Turkish (Türkçe) |
| 25 | `roles-vi.json` | Vietnamese (Tiếng Việt) |
| 26 | `roles-zh-Hant.json` | Traditional Chinese (繁體中文) |

## Translation Rules

### What to Translate

Only **two fields** need translation per entry:

1. **`categories[].name`** — Translate the category display name
2. **`roles[].name`** — Translate the role display name
3. **`roles[].description`** — Translate the role description

### What NOT to Translate (Keep English As-Is)

- `categories[].id` — Keep exactly as-is (e.g. `"engineering"`)
- `roles[].id` — Keep exactly as-is (e.g. `"engineering-ai-engineer"`)
- `roles[].category` — Keep exactly as-is
- `roles[].filePath` — Keep exactly as-is (relative path, e.g. `"engineering/engineering-ai-engineer.md"`)
- `roles[].img` — Keep exactly as-is (full URL)
- Proper nouns / brand names in descriptions (e.g. `React`, `Kubernetes`, `Shopee`, `TikTok`, `WeChat`, `Douyin`, `FMOD`, `Unity`, `Unreal Engine`) — keep in English within the translated text

### JSON Structure Example

```json
{
  "categories": [
    {
      "id": "engineering",        // ← DO NOT translate
      "name": "エンジニアリング"    // ← TRANSLATE this
    }
  ],
  "roles": [
    {
      "id": "engineering-ai-engineer",    // ← DO NOT translate
      "name": "AIエンジニア",              // ← TRANSLATE this
      "description": "AIおよび機械学習...", // ← TRANSLATE this
      "category": "engineering",           // ← DO NOT translate
      "filePath": "engineering/engineering-ai-engineer.md",  // ← DO NOT translate
      "img": "https://echobird.ai/roles/en/engineering/engineering-ai-engineer.png"  // ← DO NOT translate
    }
  ]
}
```

### Quality Requirements

- Output must be valid JSON with UTF-8 encoding (no BOM)
- Use 2-space indentation, matching the English source format
- Line endings: LF (`\n`), not CRLF
- Translation should be natural and professional, not machine-literal
- Technical terms commonly used in English in the target locale should stay in English (e.g. `API`, `CI/CD`, `DevOps`, `SRE`)

## Execution Strategy

> [!TIP]
> Process one file at a time. Read `roles-en.json` as reference, then overwrite `roles-xx.json` with the translated version.

Suggested order: Start with high-priority languages (ja, ko, zh-Hant, de, fr, es, pt, ru) then proceed with the rest.
