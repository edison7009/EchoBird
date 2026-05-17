#!/usr/bin/env python3
"""
Build an English-only AI news + projects feed from US/global sources.

Output: docs/pulse/latest-7d-en.json — same schema as the upstream
SuYxh/ai-news-aggregator latest-7d.json so the frontend can swap files
based on locale without code changes beyond the file name.

Sources:
  - Hacker News Algolia API (stories matching AI keywords)
  - AI lab blogs: OpenAI, Anthropic (via Olshansk RSS proxy), Google DeepMind,
    Google Research, Hugging Face
  - Tech media (AI-tagged feeds): TechCrunch AI, The Verge AI, Wired AI
  - Tech media (general feeds, AI-keyword filtered): Ars Technica, MIT
    Technology Review
  - arXiv RSS: cs.AI, cs.LG, cs.CL, cs.CV
  - Reddit RSS: r/MachineLearning, r/LocalLLaMA
  - Analyst blogs / newsletters: Import AI (Jack Clark), Chip Huyen
  - GitHub Trending (daily snapshot, filtered to AI/ML repos)

Each feed degrades silently: HTTP/parse failures log a warning and contribute
zero items; the workflow only fails if *every* source is empty.

The script is idempotent: it overwrites the output file each run. The
GitHub Action that invokes it commits only when the payload changes.
"""

from __future__ import annotations

import hashlib
import html
import json
import re
import sys
import time
from concurrent.futures import ThreadPoolExecutor
from datetime import datetime, timedelta, timezone
from pathlib import Path
from typing import Iterable

import feedparser  # type: ignore[import-untyped]
import requests
from bs4 import BeautifulSoup  # type: ignore[import-untyped]


# ─── Config ───────────────────────────────────────────────────────────────────

OUTPUT_PATH = Path(__file__).resolve().parent.parent / "docs" / "pulse" / "latest-7d-en.json"
WINDOW_DAYS = 7
NOW = datetime.now(timezone.utc)
WINDOW_START = NOW - timedelta(days=WINDOW_DAYS)

HEADERS = {
    "User-Agent": "Mozilla/5.0 (compatible; EchoBird-PulseBuilder/1.0)",
    "Accept": "application/json, application/rss+xml, text/html;q=0.9, */*;q=0.5",
}

HTTP_TIMEOUT = 20

# AI keyword set used to filter HN stories and GitHub Trending repos.
# Keep generous; the frontend has its own "all news" view, so over-inclusion
# is cheap and under-inclusion loses interesting items.
AI_KEYWORDS = [
    "AI", "A.I.", "AGI", "LLM", "GPT", "ChatGPT", "Claude", "Gemini",
    "OpenAI", "Anthropic", "DeepMind", "HuggingFace", "Hugging Face",
    "Mistral", "Llama", "Grok", "transformer", "neural network",
    "machine learning", "deep learning", "diffusion", "stable diffusion",
    "midjourney", "RAG", "fine-tuning", "embedding", "agent", "agentic",
    "MCP", "vibe coding", "copilot", "cursor", "codex",
]
AI_RE = re.compile(r"\b(" + "|".join(re.escape(k) for k in AI_KEYWORDS) + r")\b", re.IGNORECASE)

# Host blocklist: x.com / twitter.com items are individual KOL posts, not news.
# Kept in sync with scripts/filter_pulse.py (applied to the ZH feed at refresh time).
BLOCKED_HOST_RE = re.compile(r"^https?://([^/]+\.)?(x|twitter)\.com/", re.IGNORECASE)


# ─── HTTP helpers ─────────────────────────────────────────────────────────────


def fetch(url: str, *, timeout: int = HTTP_TIMEOUT) -> requests.Response | None:
    try:
        r = requests.get(url, headers=HEADERS, timeout=timeout)
        if r.status_code == 200:
            return r
        print(f"[warn] {url} → HTTP {r.status_code}", file=sys.stderr)
    except Exception as e:
        print(f"[warn] {url} → {e}", file=sys.stderr)
    return None


def stable_id(*parts: str) -> str:
    """Deterministic SHA1 of joined parts; matches upstream schema."""
    return hashlib.sha1("|".join(parts).encode("utf-8")).hexdigest()


def iso(dt: datetime) -> str:
    return dt.astimezone(timezone.utc).strftime("%Y-%m-%dT%H:%M:%S.000Z")


# ─── Source 1: Hacker News Algolia API ────────────────────────────────────────


# Each query string is sent to https://hn.algolia.com/api/v1/search_by_date
# scoped to story-tagged items in the 7-day window. Algolia caps at 1000 hits
# per query; multiple narrow queries cover more ground than one broad one.
HN_QUERIES = [
    "AI", "LLM", "GPT", "ChatGPT", "Claude", "Gemini", "OpenAI", "Anthropic",
    "DeepMind", "Mistral", "Llama", "HuggingFace", "AGI", "RAG", "agent",
    "transformer", "diffusion", "embedding", "fine-tuning", "MCP",
]


def fetch_hn_stories() -> list[dict]:
    window_start_epoch = int(WINDOW_START.timestamp())
    out: list[dict] = []
    seen: set[str] = set()
    for q in HN_QUERIES:
        url = (
            "https://hn.algolia.com/api/v1/search_by_date"
            f"?tags=story&query={requests.utils.quote(q)}"
            f"&numericFilters=created_at_i>{window_start_epoch}"
            "&hitsPerPage=200"
        )
        r = fetch(url)
        if not r:
            continue
        try:
            data = r.json()
        except Exception:
            continue
        for hit in data.get("hits", []):
            obj_id = hit.get("objectID")
            if not obj_id or obj_id in seen:
                continue
            url_field = hit.get("url") or f"https://news.ycombinator.com/item?id={obj_id}"
            title = hit.get("title") or hit.get("story_title") or ""
            if not title:
                continue
            # Sanity gate: title must mention something AI-shaped. HN's full-text
            # search returns plenty of false positives (e.g. comments matching
            # "claude" the artist).
            if not AI_RE.search(title):
                continue
            created_iso = hit.get("created_at")  # Algolia gives ISO 8601
            if not created_iso:
                continue
            seen.add(obj_id)
            points = hit.get("points") or 0
            comments = hit.get("num_comments") or 0
            out.append({
                "id": stable_id("hn", obj_id),
                "site_id": "hackernews",
                "site_name": "Hacker News",
                "source": f"Hacker News ({points}pts, {comments}c)",
                "title": title,
                "url": url_field,
                "published_at": created_iso,
                "first_seen_at": created_iso,
                "last_seen_at": iso(NOW),
                "title_original": title,
                "title_en": title,
                "title_zh": None,
                "title_bilingual": title,
            })
    print(f"[hn] {len(out)} stories", file=sys.stderr)
    return out


# ─── Source 2: RSS feeds (AI labs, tech media, arXiv) ─────────────────────────


# Each spec: slug -> (display name, RSS URL, filter_ai, max_items).
#   filter_ai=False — feed is already AI-only (lab blog, AI-tagged media feed,
#                     arXiv AI category). Trust every entry.
#   filter_ai=True  — feed is a general tech firehose (Ars Technica, MIT TR,
#                     The Verge). Apply AI_RE on title or skip.
#   max_items       — cap retained items per feed. arXiv categories publish
#                     500+ papers/day so without a cap they'd swamp every
#                     other source. None means "take everything in window".
# Each feed parsed independently; failures are warnings, not fatal — a broken
# feed shouldn't blackhole the whole build.
#
# Source list cross-referenced with ai-news-daily/ai-news-daily.github.io
# (MIT, 70+ feeds) and verified working 2026-05. Anthropic has no native RSS,
# so we proxy through Olshansk/rss-feeds — periodically re-check if Anthropic
# ever publishes their own. The Verge retired their AI-specific feed (returns
# empty payload), so we fall back to the general feed + AI filter.
RSS_FEEDS: dict[str, tuple[str, str, bool, int | None]] = {
    # ── AI labs ─────────────────────────────────────────────
    "openai":         ("OpenAI",          "https://openai.com/news/rss.xml",                                                False, None),
    "anthropic":      ("Anthropic",       "https://raw.githubusercontent.com/Olshansk/rss-feeds/main/feeds/feed_anthropic_news.xml", False, None),
    "deepmind":       ("Google DeepMind", "https://deepmind.google/blog/rss.xml",                                           False, None),
    "googleai":       ("Google Research", "https://research.google/blog/rss/",                                              False, None),
    "huggingface":    ("Hugging Face",    "https://huggingface.co/blog/feed.xml",                                           False, None),
    # ── Tech media (AI-tagged feeds, pre-filtered upstream) ─
    "techcrunch_ai":  ("TechCrunch AI",   "https://techcrunch.com/category/artificial-intelligence/feed/",                  False, None),
    "wired_ai":       ("Wired AI",        "https://www.wired.com/feed/tag/ai/latest/rss",                                   False, None),
    # ── Tech media (general feeds, need keyword filter) ─────
    "theverge":       ("The Verge",       "https://www.theverge.com/rss/index.xml",                                         True,  None),
    "arstechnica":    ("Ars Technica",    "https://feeds.arstechnica.com/arstechnica/index",                                True,  None),
    "mit_tr":         ("MIT Tech Review", "https://www.technologyreview.com/feed/",                                         True,  None),
    # ── arXiv (per-category feeds, all AI-relevant) ─────────
    # Cap each category at 60 so the page isn't 90% papers. arXiv RSS sorts
    # newest-first, so the cap drops the bottom of yesterday's batch.
    "arxiv_ai":       ("arXiv cs.AI",     "https://export.arxiv.org/rss/cs.AI",                                             False, 60),
    "arxiv_lg":       ("arXiv cs.LG",     "https://export.arxiv.org/rss/cs.LG",                                             False, 60),
    "arxiv_cl":       ("arXiv cs.CL",     "https://export.arxiv.org/rss/cs.CL",                                             False, 60),
    "arxiv_cv":       ("arXiv cs.CV",     "https://export.arxiv.org/rss/cs.CV",                                             False, 60),
    # ── Reddit communities (RSS) ────────────────────────────
    # Reddit blocks generic user-agents with a 403/429 page; our HEADERS
    # already sets a unique UA, and these feeds degrade silently to 0 items
    # if Reddit rejects us. Picked the two highest-signal subs (research +
    # practitioner) — wider subs like r/singularity are too hype-heavy.
    "r_mlearning":    ("r/MachineLearning", "https://www.reddit.com/r/MachineLearning/.rss",                                False, 40),
    "r_localllama":   ("r/LocalLLaMA",      "https://www.reddit.com/r/LocalLLaMA/.rss",                                     False, 40),
    # ── Newsletters / analyst blogs ─────────────────────────
    # Import AI is Jack Clark's (Anthropic co-founder) weekly digest; Chip
    # Huyen writes about ML systems and LLM ops. Low-volume, high-signal.
    "import_ai":      ("Import AI",        "https://jack-clark.net/feed/",                                                  False, None),
    "chip_huyen":     ("Chip Huyen",       "https://huyenchip.com/feed.xml",                                                False, None),
}


def parse_rss_dt(entry) -> datetime | None:
    for field in ("published_parsed", "updated_parsed"):
        v = getattr(entry, field, None) or entry.get(field) if hasattr(entry, "get") else None
        if v:
            try:
                return datetime(*v[:6], tzinfo=timezone.utc)
            except Exception:
                pass
    return None


def fetch_rss_feed(slug: str, name: str, url: str, filter_ai: bool, max_items: int | None) -> list[dict]:
    r = fetch(url, timeout=15)
    if not r:
        return []
    try:
        feed = feedparser.parse(r.content)
    except Exception as e:
        print(f"[warn] {slug} parse error: {e}", file=sys.stderr)
        return []
    out: list[dict] = []
    skipped_off_topic = 0
    for entry in feed.entries:
        if max_items is not None and len(out) >= max_items:
            break
        title = (entry.get("title") or "").strip()
        link = (entry.get("link") or "").strip()
        if not title or not link:
            continue
        # Strip RSS-frequent HTML entities + tags from titles
        title = html.unescape(re.sub(r"<[^>]+>", "", title))
        # General-firehose feeds (Ars Technica, MIT TR, The Verge) carry
        # plenty of non-AI content. Skip anything whose title doesn't
        # mention an AI keyword — same gate used on Hacker News.
        if filter_ai and not AI_RE.search(title):
            skipped_off_topic += 1
            continue
        dt = parse_rss_dt(entry)
        if not dt:
            continue
        # arXiv RSS often lists items as "today" in UTC; only enforce the
        # lower bound, never trust future timestamps from upstream.
        if dt < WINDOW_START:
            continue
        out.append({
            "id": stable_id(slug, link),
            "site_id": slug,
            "site_name": name,
            "source": name,
            "title": title,
            "url": link,
            "published_at": iso(dt),
            "first_seen_at": iso(dt),
            "last_seen_at": iso(NOW),
            "title_original": title,
            "title_en": title,
            "title_zh": None,
            "title_bilingual": title,
        })
    extras = []
    if skipped_off_topic:
        extras.append(f"{skipped_off_topic} off-topic")
    if max_items is not None and len(out) >= max_items:
        extras.append(f"capped at {max_items}")
    suffix = f" ({', '.join(extras)})" if extras else ""
    print(f"[rss:{slug}] {len(out)} items{suffix}", file=sys.stderr)
    return out


def fetch_all_rss_feeds() -> list[dict]:
    out: list[dict] = []
    with ThreadPoolExecutor(max_workers=6) as ex:
        futures = [
            ex.submit(fetch_rss_feed, slug, name, url, filter_ai, max_items)
            for slug, (name, url, filter_ai, max_items) in RSS_FEEDS.items()
        ]
        for f in futures:
            out.extend(f.result())
    return out


# ─── Source 3: GitHub Trending (daily snapshot) ───────────────────────────────


# We poll the daily trending pages for a handful of relevant languages.
# GitHub's trending HTML changes occasionally; the parser uses defensive
# selectors and skips repos it can't parse cleanly.
TRENDING_LANGS = ["", "python", "typescript", "javascript", "rust", "go"]


def parse_trending(html_text: str) -> list[dict]:
    soup = BeautifulSoup(html_text, "html.parser")
    rows = soup.select("article.Box-row")
    out: list[dict] = []
    for row in rows:
        a = row.select_one("h2 a")
        if not a:
            continue
        href = a.get("href", "").strip()
        if not href.startswith("/"):
            continue
        repo = href.lstrip("/")
        url = f"https://github.com/{repo}"
        # Description sits in a sibling <p>
        desc_el = row.select_one("p")
        desc = desc_el.get_text(strip=True) if desc_el else ""
        title = f"{repo} — {desc}" if desc else repo
        out.append({"repo": repo, "url": url, "title": title, "desc": desc})
    return out


def fetch_github_trending() -> list[dict]:
    seen: set[str] = set()
    out: list[dict] = []
    for lang in TRENDING_LANGS:
        url = f"https://github.com/trending/{lang}?since=daily" if lang else "https://github.com/trending?since=daily"
        r = fetch(url, timeout=15)
        if not r:
            continue
        for repo in parse_trending(r.text):
            if repo["repo"] in seen:
                continue
            # Filter to AI/ML-relevant projects: keyword in title or description
            haystack = repo["title"] + " " + repo.get("desc", "")
            if not AI_RE.search(haystack):
                continue
            seen.add(repo["repo"])
            now_iso = iso(NOW)
            out.append({
                "id": stable_id("ghtrend", repo["repo"]),
                "site_id": "github-trending",
                "site_name": "GitHub Trending",
                "source": "GitHub Trending",
                "title": repo["title"],
                "url": repo["url"],
                # Trending doesn't expose a stable "first appeared" timestamp,
                # so we mark it with the current run time. Frontend's
                # itemTs() falls back to first_seen_at when published_at is
                # null, which keeps these grouped under today's date.
                "published_at": None,
                "first_seen_at": now_iso,
                "last_seen_at": now_iso,
                "title_original": repo["title"],
                "title_en": repo["title"],
                "title_zh": None,
                "title_bilingual": repo["title"],
            })
        # Be a polite scraper; GitHub doesn't have an official trending API
        # and Cloudflare-fronts the page.
        time.sleep(0.3)
    print(f"[ghtrend] {len(out)} repos", file=sys.stderr)
    return out


# ─── Merge + write ────────────────────────────────────────────────────────────


def dedupe_by_url(items: Iterable[dict]) -> list[dict]:
    by_url: dict[str, dict] = {}
    for it in items:
        u = it["url"]
        if u not in by_url:
            by_url[u] = it
    return list(by_url.values())


def main() -> int:
    print(f"[info] window = {WINDOW_DAYS}d, output = {OUTPUT_PATH}", file=sys.stderr)
    items: list[dict] = []
    items.extend(fetch_hn_stories())
    items.extend(fetch_all_rss_feeds())
    items.extend(fetch_github_trending())

    items = dedupe_by_url(items)
    items = [it for it in items if not BLOCKED_HOST_RE.match(it.get("url") or "")]
    # Sort newest-first by best available timestamp.
    def ts_key(it: dict) -> str:
        return it.get("published_at") or it.get("first_seen_at") or ""
    items.sort(key=ts_key, reverse=True)

    site_stats: dict[str, int] = {}
    sources: set[str] = set()
    for it in items:
        site_stats[it["site_id"]] = site_stats.get(it["site_id"], 0) + 1
        sources.add(it["source"])

    payload = {
        "generated_at": iso(NOW),
        "window_hours": WINDOW_DAYS * 24,
        "total_items": len(items),
        "total_items_ai_raw": len(items),
        "total_items_raw": len(items),
        "total_items_all_mode": len(items),
        "topic_filter": "ai+ml (en-only)",
        "archive_total": len(items),
        "site_count": len(site_stats),
        "source_count": len(sources),
        "site_stats": site_stats,
        "items": items,
    }

    OUTPUT_PATH.parent.mkdir(parents=True, exist_ok=True)
    OUTPUT_PATH.write_text(json.dumps(payload, ensure_ascii=False, indent=0), encoding="utf-8")
    print(f"[done] wrote {len(items)} items → {OUTPUT_PATH}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
