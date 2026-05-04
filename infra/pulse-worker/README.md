# echobird-pulse Cloudflare Worker

Pure pass-through proxy for multiple GitHub-hosted data sources used by the
EchoBird desktop app, served from `echobird.ai`. Edge-cached, CORS open, GET only.

## Routes

| Path                           | Upstream                                                                  | Used by             |
|--------------------------------|---------------------------------------------------------------------------|---------------------|
| `/pulse/latest-24h.json` (etc) | [SuYxh/ai-news-aggregator](https://github.com/SuYxh/ai-news-aggregator)   | AI 资讯 / 明星项目 |
| `/courses/README.md`           | [dair-ai/ML-YouTube-Courses](https://github.com/dair-ai/ML-YouTube-Courses) | AI 公开课           |

Add a new source by appending an entry to `ROUTES` in [src/index.js](src/index.js)
and a matching pattern in [wrangler.toml](wrangler.toml).

Edge cache TTLs (per file extension and route):
- `latest-24h.json` — 30 min (upstream refreshes every 2h)
- `latest-7d.json`  — 1 hour
- `archive.json`    — 6 hours
- `/courses/**`     — 6 hours (courses change slowly)

CORS is open (`*`) so the Tauri WebView can fetch directly. GET / HEAD only.

## Deploy

```bash
cd infra/pulse-worker
npm i -g wrangler            # if not already installed
wrangler login               # one-time
wrangler deploy              # ships to echobird.ai/pulse/*
```

The route binding is in [wrangler.toml](wrangler.toml). If your Cloudflare zone is
not `echobird.ai`, edit `routes[].zone_name` before deploying.

## Verify

```bash
curl -sSI https://echobird.ai/pulse/manifest.json | head -10
curl -s   https://echobird.ai/pulse/manifest.json | head -5
```

You should see `cf-cache-status: HIT` after the second request.

## Failure mode

The desktop client has GitHub raw as a fallback, so if this Worker goes down or the
custom domain is detached, the app keeps working — just slower for users behind
restrictive networks. To swap upstream away from `duanyytop/agents-radar`, edit
`UPSTREAM_BASE` in [src/index.js](src/index.js) and redeploy.
