#!/usr/bin/env python3
"""Sample Nyaa Literature category RSS feeds for the chaptr LN corpus.

Fetches a handful of pages from the Nyaa Literature category RSS, filters to
likely-LN entries (`.epub`/`.pdf`/`.azw3`/`.mobi` extensions or known LN
publisher/scanner keywords), and writes the raw filenames to
`corpus/raw/nyaa_literature.txt` for hand-picking into `corpus/novel_nyaa.json`.

The raw output is gitignored — we commit only the hand-picked, structured
corpus, not the bulk scrape (avoids bloat, avoids stale snapshots, avoids
publishing a list of someone else's torrent metadata under our repo).

Polite by construction:
  - 2 second delay between requests
  - User-Agent identifies the project + repo
  - RSS only (not HTML pages); RSS is the canonical low-cost surface for this
  - Default: 3 pages × 2 categories = 6 requests, ~12 seconds total

Usage:
    python3 tools/scrape_nyaa.py [--pages N]

Pure stdlib. No deps.
"""

import argparse
import os
import sys
import time
import urllib.parse
import urllib.request
from xml.etree import ElementTree as ET

USER_AGENT = "chaptr-corpus-sampler (https://github.com/johnthreekay/chaptr)"
DELAY_SECONDS = 2

# Nyaa Literature subcategories. 3_1 (English-translated) gets the most LN
# releases; 3_2 (Raw) adds Japanese-language coverage with native-script
# filenames. 3_3 (Non-English-translated) is mostly noise for LN purposes.
CATEGORIES = [
    ("3_1", "english-translated"),
    ("3_2", "raw"),
]

# Publisher-targeted search queries. The Nyaa Literature category is dominated
# by manga; without filtering, ~98% of RSS entries are manga. Searching for
# specific LN publisher names dramatically raises LN density. Each query is
# combined with c=3_0 (all Literature) so we get hits in either subcategory.
LN_PUBLISHER_QUERIES = [
    "yen press",
    "j-novel club",
    "seven seas",
    "vertical",
    "kodansha light novel",
    "one peace books",
    "lucaz",
    "stick",   # high-recall, will pull manga too — let the LN classifier filter
]

LN_EXTENSIONS = (".epub", ".pdf", ".azw3", ".mobi", ".txt")

# Substring hints that strongly suggest LN even when the RSS title doesn't end
# in a known extension (often it doesn't — Nyaa torrents are commonly named
# after the release set, not the file).
LN_HINTS = (
    "yen press",
    "j-novel club",
    "j-novel",
    "seven seas",
    "vertical",
    "kodansha",
    "one peace books",
    "cross infinite",
    "tentai books",
    "hanashi media",
    "lucaz",   # well-known LN scanner
    "stick",   # ditto, but high false-positive rate
    "[ln]",
    "(ln)",
    " ln ",
    "light novel",
    "ライトノベル",
    "ラノベ",
)


def is_likely_ln(title: str) -> bool:
    lower = title.lower()
    if lower.endswith(LN_EXTENSIONS):
        return True
    return any(hint in lower for hint in LN_HINTS)


def fetch_rss(category_id: str, page: int, query: str | None = None) -> bytes:
    url = f"https://nyaa.si/?page=rss&c={category_id}&p={page}"
    if query:
        url += f"&q={urllib.parse.quote(query)}"
    req = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
    with urllib.request.urlopen(req, timeout=30) as resp:
        return resp.read()


def extract_titles(rss_bytes: bytes) -> list[str]:
    root = ET.fromstring(rss_bytes)
    titles: list[str] = []
    for item in root.iter("item"):
        title_elem = item.find("title")
        if title_elem is not None and title_elem.text:
            titles.append(title_elem.text.strip())
    return titles


def main() -> int:
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument(
        "--pages",
        type=int,
        default=3,
        help="RSS pages to fetch per category (default 3, ~75 entries/page)",
    )
    p.add_argument(
        "--output",
        default="corpus/raw/nyaa_literature.txt",
        help="output path (default corpus/raw/nyaa_literature.txt)",
    )
    args = p.parse_args()

    seen: set[str] = set()
    by_category: dict[str, list[str]] = {name: [] for _, name in CATEGORIES}
    by_category["publisher-search"] = []
    fetched = 0

    # Pass 1: untargeted RSS — captures whatever's recent, regardless of publisher.
    for cat_id, cat_name in CATEGORIES:
        for page in range(1, args.pages + 1):
            print(f"# fetch  c={cat_id}  page={page}", file=sys.stderr)
            rss = fetch_rss(cat_id, page)
            fetched += 1
            for title in extract_titles(rss):
                if title in seen:
                    continue
                seen.add(title)
                if is_likely_ln(title):
                    by_category[cat_name].append(title)
            time.sleep(DELAY_SECONDS)

    # Pass 2: publisher-targeted search — dramatically raises LN density.
    # Pages are capped at 2 per query to keep total request count modest.
    for query in LN_PUBLISHER_QUERIES:
        for page in range(1, min(args.pages, 2) + 1):
            print(f"# fetch  q={query!r}  page={page}", file=sys.stderr)
            rss = fetch_rss("3_0", page, query=query)
            fetched += 1
            for title in extract_titles(rss):
                if title in seen:
                    continue
                seen.add(title)
                if is_likely_ln(title):
                    by_category["publisher-search"].append(title)
            time.sleep(DELAY_SECONDS)

    os.makedirs(os.path.dirname(args.output), exist_ok=True)
    with open(args.output, "w", encoding="utf-8") as f:
        f.write(f"# nyaa literature sample, {fetched} RSS pages fetched\n")
        f.write(f"# filtered to LN-likely titles via extension + publisher hints\n\n")
        for cat_name, titles in by_category.items():
            f.write(f"# === {cat_name} ({len(titles)} entries) ===\n")
            for t in titles:
                f.write(t + "\n")
            f.write("\n")

    # Also dump the full unfiltered list — useful for tuning the LN heuristic
    # and for hand-picking entries the heuristic missed.
    unfiltered_path = args.output.replace(".txt", "_unfiltered.txt")
    with open(unfiltered_path, "w", encoding="utf-8") as f:
        f.write(f"# nyaa literature sample, {fetched} RSS pages fetched\n")
        f.write(f"# unfiltered: every unique title returned by the RSS feeds\n\n")
        for t in sorted(seen):
            marker = "LN?" if is_likely_ln(t) else "   "
            f.write(f"{marker}  {t}\n")

    total = sum(len(v) for v in by_category.values())
    print(
        f"# wrote {total} likely-LN entries (filtered) and {len(seen)} unfiltered to:",
        file=sys.stderr,
    )
    print(f"#   {args.output}", file=sys.stderr)
    print(f"#   {unfiltered_path}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
