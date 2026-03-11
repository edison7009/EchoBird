---
description: Update free models list - check for new models and changed free limits across 15 providers
---

# Update Free Models List

Periodically run this to keep `docs/api/free-models.json` up to date.

## Reference Sources

| What | URL |
|---|---|
| **Model IDs + tiers** | https://github.com/vava-nessa/free-coding-models/blob/main/sources.js |
| **Free limits** | https://github.com/cheahjs/free-llm-api-resources |

`sources.js` format: `[model_id, display_name, tier, swe_score, context]`  
Only include **S+** (≥70% SWE-bench) and **S** (60–70%) tier models.

---

## Provider Order (follow sources.js export sequence)

| # | Provider | Base URL |
|---|---|---|
| 1 | NVIDIA NIM | `https://integrate.api.nvidia.com/v1` |
| 2 | Groq | `https://api.groq.com/openai/v1` |
| 3 | Cerebras | `https://api.cerebras.ai/v1` |
| 4 | SambaNova | `https://api.sambanova.ai/v1` |
| 5 | OpenRouter | `https://openrouter.ai/api/v1` |
| 6 | Hugging Face | `https://router.huggingface.co/v1` |
| 7 | Fireworks | `https://api.fireworks.ai/inference/v1` |
| 8 | Hyperbolic | `https://api.hyperbolic.xyz/v1` |
| 9 | Scaleway | `https://api.scaleway.ai/v1` |
| 10 | ZAI | `https://api.z.ai/api/coding/paas/v4` |
| 11 | SiliconFlow | `https://api.siliconflow.com/v1` |
| 12 | Together AI | `https://api.together.xyz/v1` |
| 13 | Cloudflare | `https://api.cloudflare.com/client/v4/accounts/{id}/ai/v1` |
| 14 | Alibaba DashScope | `https://dashscope-intl.aliyuncs.com/compatible-mode/v1` |
| 15 | iFlow | `https://apis.iflow.cn/v1` |

> **Note:** Google AI Studio excluded — no S/S+ tier free models (Gemma is B/C tier only).

---

## Workflow Steps

1. Open `sources.js` and go through each provider's export block in order  
   - URL: https://github.com/vava-nessa/free-coding-models/blob/main/sources.js  
   - Filter: only rows where tier is `'S+'` or `'S'`  
   - Copy the `model_id` (first element) and `ctx` (fifth element) exactly

2. Cross-check limits with cheahjs README  
   - URL: https://github.com/cheahjs/free-llm-api-resources  
   - Look for: changed RPM/RPD numbers, providers removed from free tier

3. Update `docs/api/free-models.json`:
   - Keep provider order matching the table above
   - Per model: `id`, `name`, `context` (convert `128k` → `131072`), `limit`, `"free": true`
   - Remove models dropped below S tier
   - Update the top-level `"updated"` date field

4. Commit and push — `echobird.ai/free-models` updates automatically via GitHub Pages

---

## Rules

- **S+ and S tier only** — skip A+, A, A-, B+, B, C models
- **No Google AI Studio** — Gemma models are B/C tier, not worth listing
- **Per-model limits** where known — use cheahjs for accurate limit data
- Trial-credit providers (Fireworks, Hyperbolic, Together, SambaNova) are OK to include, mark limit clearly
