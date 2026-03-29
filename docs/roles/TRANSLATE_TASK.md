# Role JSON Translation Plan

## Current Status (as of 2026-03-29)

Two files exist and are maintained:

| File | Language | Status |
|------|----------|--------|
| `roles-en.json` | English | ✅ Source of truth |
| `roles-zh-Hans.json` | Simplified Chinese | ✅ Separately maintained |

All other 26 locale JSON files have been **deleted**. Non-zh-Hans users automatically fall back to `roles-en.json` via the CDN fallback logic in `src/api/roles.ts`.

---

## Chinese Locale Routing

Both Simplified and Traditional Chinese map to `roles-zh-Hans.json`:

```ts
// In src/api/roles.ts — resolveLocaleFileName()
if (locale.startsWith('zh')) return 'roles-zh-Hans.json';
```

This means `zh-TW`, `zh-HK`, `zh-Hant` all read `roles-zh-Hans.json`. No separate `roles-zh-Hant.json` is needed — the role names and descriptions are shared Chinese content.

---

## Target Languages for v1 Translation

Based on AI agent user demographics (developer-centric, GitHub/Claude/Cursor user base), the following **7 additional languages** cover ~85% of real users:

| Priority | File | Language | Justification |
|----------|------|----------|---------------|
| 🔴 High | `roles-ja.json` | Japanese (日本語) | Very high AI adoption rate in Japan; active Claude/ChatGPT community |
| 🔴 High | `roles-ko.json` | Korean (한국어) | Korea has one of the highest AI tool adoption rates globally |
| 🟡 Medium | `roles-de.json` | German (Deutsch) | Largest tech market in the EU |
| 🟡 Medium | `roles-pt.json` | Portuguese (Português) | Brazil is one of the fastest-growing AI user communities |
| 🟡 Medium | `roles-ru.json` | Russian (Русский) | Large active developer base |
| 🟡 Medium | `roles-fr.json` | French (Français) | France + Francophone Africa |
| 🟡 Medium | `roles-es.json` | Spanish (Español) | Largest AI user group in Latin America |

### Not prioritized (low ROI for AI agent users)

Languages with large populations but low AI developer density, or where English acceptance is high:
- Arabic, Hindi — wide populations but lower dev density
- Southeast Asian (vi/id/th/ms) — growing but lower priority for now
- Small EU languages (nl/sv/fi/pl/cs/el/hu/it) — English acceptance rate very high among their developers

---

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
- `roles[].filePath` — Keep exactly as-is (relative path)
- `roles[].img` — Keep exactly as-is (full URL)
- Proper nouns / brand names (e.g. `React`, `Kubernetes`, `TikTok`, `Unity`) — keep in English within translated text

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

---

## Execution Strategy

> [!TIP]
> Process one file at a time. Read `roles-en.json` as reference, then create `roles-xx.json` with the translated version.

Suggested order: `ja` → `ko` → `de` → `pt` → `ru` → `fr` → `es`
