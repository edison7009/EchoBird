---
description: Update free models list - check for new models and changed free limits across 8 providers
---

# Update Free Models List

Periodically run this to keep `docs/api/free-models.json` up to date.

## Reference Sources

- **Model IDs**: https://docs.litellm.ai/docs/providers/ (one page per provider)
- **Free limits**: https://github.com/cheahjs/free-llm-api-resources (README, auto-updated daily)

---

## 8 Providers to Check

| Provider | LiteLLM page | Base URL |
|---|---|---|
| Google AI Studio | `/providers/gemini` | `https://generativelanguage.googleapis.com/v1beta/openai` |
| Groq | `/providers/groq` | `https://api.groq.com/openai/v1` |
| Cerebras | `/providers/cerebras` | `https://api.cerebras.ai/v1` |
| OpenRouter | `/providers/openrouter` | `https://openrouter.ai/api/v1` |
| Mistral AI | `/providers/mistral` | `https://api.mistral.ai/v1` |
| Cohere | `/providers/cohere` | `https://api.cohere.ai/v1` |
| Cloudflare Workers AI | `/providers/cloudflare_workers_ai` | `https://api.cloudflare.com/client/v4/accounts/{id}/ai/v1` |
| NVIDIA NIM | `/providers/nvidia_nim` | `https://integrate.api.nvidia.com/v1` |

---

## Workflow Steps

1. Open cheahjs README and scan for any **new providers or changed limits**
   - URL: https://github.com/cheahjs/free-llm-api-resources
   - Look for: new model names, changed RPM/RPD numbers, any providers removed from free tier

2. For each provider where cheahjs shows new/changed models, open the corresponding LiteLLM page to get the **exact model ID**
   - URL pattern: `https://docs.litellm.ai/docs/providers/<name>`
   - Copy the model ID exactly as shown (e.g., `meta-llama/llama-4-scout-17b-16e-instruct`)

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
- **Keep model list focused** — top 3–8 models per provider, prioritize most capable / most popular
- Do NOT include: Together AI, Baseten, Novita, Nebius, AI21 (these are trial credits only)
