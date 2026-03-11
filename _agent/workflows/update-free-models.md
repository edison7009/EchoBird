---
description: Update free models list - check for new models and changed free limits across 8 providers
---

# Update Free Models List

Periodically run this to keep `docs/api/free-models.json` up to date.

## Reference Sources

- **Model IDs + providers**: https://github.com/vava-nessa/free-coding-models/blob/main/sources.js
  - Actively maintained (87+ releases), covers NVIDIA/Groq/Cerebras/OpenRouter/Cloudflare etc.
  - Each entry: `[model_id, display_name, tier, swe_score, context]`
- **Free limits**: https://github.com/cheahjs/free-llm-api-resources (README, auto-updated daily)

---

## 8 Providers to Check

| Provider | Base URL |
|---|---|
| Google AI Studio | `https://generativelanguage.googleapis.com/v1beta/openai` |
| Groq | `https://api.groq.com/openai/v1` |
| Cerebras | `https://api.cerebras.ai/v1` |
| OpenRouter | `https://openrouter.ai/api/v1` |
| Mistral AI | `https://api.mistral.ai/v1` |
| Cohere | `https://api.cohere.ai/v1` |
| Cloudflare Workers AI | `https://api.cloudflare.com/client/v4/accounts/{id}/ai/v1` |
| NVIDIA NIM | `https://integrate.api.nvidia.com/v1` |

---

## Workflow Steps

1. Open `sources.js` and scan for **new model IDs** per provider section  
   - URL: https://github.com/vava-nessa/free-coding-models/blob/main/sources.js  
   - Each provider has its own export block (e.g. `export const groq = [...]`)  
   - Copy model IDs exactly as listed (e.g. `meta-llama/llama-4-scout-17b-16e-preview`)

2. Cross-check limits with cheahjs README  
   - URL: https://github.com/cheahjs/free-llm-api-resources  
   - Look for: changed RPM/RPD numbers, providers removed from free tier

3. Update `docs/api/free-models.json`:
   - Add new models with: `id`, `name`, `context`, `limit`, `"free": true`
   - Update changed limits on existing models
   - Remove models that are no longer free
   - Update the top-level `"updated"` date field

4. Commit and push — the page at `echobird.ai/free-models` updates automatically via GitHub Pages

---

## Rules

- **Only include providers with ongoing free tiers** — no one-time trial credits
- **Per-model limits** where known (e.g., `"5 RPM / 20 req/day"`) — not just blanket provider limits
- **Keep model list focused** — top 5–10 models per provider, prioritize highest tier (S+/S/A+)
- Do NOT include: Together AI, Baseten, Novita, Nebius, AI21 (these are trial credits only)
