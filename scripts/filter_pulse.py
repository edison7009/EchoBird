#!/usr/bin/env python3
"""
Strip blocklisted-host items from pulse JSON files in-place.

The upstream ZH aggregator (SuYxh/ai-news-aggregator) wraps a lot of
individual KOL X/Twitter posts as "news" items — roughly 8% of the 7d
window. They're editorial / promotional rather than reporting, so we drop
them at refresh time. This runs in the GHA workflow after curl-ing the
upstream JSON and again from build_en_pulse.py for symmetry.

Schema-aware: rewrites `total_items` and `items`, leaves the rest of the
payload (site_stats, archive_total, etc.) untouched.
"""

from __future__ import annotations

import json
import re
import sys
from pathlib import Path

# Match the full host segment (incl. any subdomain). Two hosts:
#   x.com / *.x.com
#   twitter.com / *.twitter.com
BLOCKED_HOST_RE = re.compile(r"^https?://([^/]+\.)?(x|twitter)\.com/", re.IGNORECASE)


def filter_file(path: Path) -> tuple[int, int]:
    text = path.read_text(encoding="utf-8")
    payload = json.loads(text)
    items = payload.get("items") or []
    before = len(items)
    kept = [it for it in items if not BLOCKED_HOST_RE.match(it.get("url") or "")]
    payload["items"] = kept
    payload["total_items"] = len(kept)
    # Preserve the upstream's indent style so refresh commits show only the
    # actual content delta, not a wholesale reformat. SuYxh's ZH files ship
    # with indent=2; our EN builder writes indent=0. Sniff by peeking at the
    # first child line — indented vs flush-left.
    indent = 2 if len(text) > 2 and text[1] == "\n" and text[2] == " " else 0
    path.write_text(json.dumps(payload, ensure_ascii=False, indent=indent), encoding="utf-8")
    return before, len(kept)


def main(argv: list[str]) -> int:
    if len(argv) < 2:
        print("usage: filter_pulse.py FILE [FILE ...]", file=sys.stderr)
        return 2
    for arg in argv[1:]:
        path = Path(arg)
        before, after = filter_file(path)
        dropped = before - after
        print(f"{path}: {before} → {after} ({dropped} dropped)")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
