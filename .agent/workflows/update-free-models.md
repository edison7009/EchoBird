---
description: Update free models list - check for new models and changed free limits across 15 providers
---

# Update Free Models List

Periodically run this to keep `docs/api/free-models.json` up to date.

## Reference Sources

| Priority | What | URL |
|---|---|---|
| **1 (primary)** | NVIDIA NIM catalog (newest models first) | https://build.nvidia.com/explore/discover |
| **2 (secondary)** | Model tiers + SWE scores | https://github.com/vava-nessa/free-coding-models/blob/main/sources.js |
| **3 (limits)** | Free tier rate limits | https://github.com/cheahjs/free-llm-api-resources |

> **Important:** `sources.js` lags behind NVIDIA NIM releases (e.g. MiniMax M2.7, DeepSeek V4, GLM 5.1). Always check NVIDIA NIM catalog directly first for new models, then cross-reference tiers from sources.js.
>
> `sources.js` format: `[model_id, display_name, tier, swe_score, context]`  
> Only include **S+** (≥70% SWE-bench) and **S** (60–70%) tier models.  
> For brand-new models not yet in sources.js, assume S+ tier and add them.

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

---

## Additional Manual Sources

These providers are NOT in `sources.js` and must be checked manually:

| Provider | Models Page | Limits Page | Base URL |
|---|---|---|---|
| **ModelScope** | https://modelscope.cn/models?filter=inference_type | https://modelscope.cn/docs/model-service/API-Inference/limits | `https://api-inference.modelscope.cn/v1` |

- ModelScope: 2000 RPD shared pool, requires Alibaba Cloud account + real-name verification
- Filter by "API-Inference" tag, only include S+/S tier models (GLM-5, DeepSeek V3.2, Qwen3.5, etc.)
- Model IDs use `org/model-name` format (e.g. `ZhipuAI/GLM-5`)
