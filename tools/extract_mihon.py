#!/usr/bin/env python3
"""Extract assertChapter entries from Mihon's ChapterRecognitionTest into chaptr corpus JSON.

Usage:
    python3 tools/extract_mihon.py > corpus/chapters_mihon.json

Mihon's tests look like:

    assertChapter(mangaTitle, "Mokushiroku Alice Vol.1 Ch.4: Misrepresentation", 4.0)

We extract just the (chapter_name, expected_chapter_float) pairs. The first arg
is a Kotlin variable holding the canonical manga title, used by Mihon's parser
to strip the title from the chapter name before parsing. chaptr's parse() takes
just a filename, so we drop the manga title.

Pure stdlib. No deps.
"""

import json
import re
import sys
import urllib.request

URL = "https://raw.githubusercontent.com/mihonapp/mihon/main/domain/src/test/java/tachiyomi/domain/chapter/service/ChapterRecognitionTest.kt"
SOURCE_TAG = "mihon-ChapterRecognitionTest"

# Match: assertChapter(<identifier>, "string", <float>)
# - identifier is a Kotlin variable name (mangaTitle, etc.)
# - string handles \" and \\ escapes
# - float can be 4.0, .5, 567, 4.99, etc.
ASSERT_CHAPTER = re.compile(
    r'assertChapter\(\s*'
    r'\w+'                                      # manga title variable (discarded)
    r'\s*,\s*'
    r'"((?:[^"\\]|\\.)*)"'                      # group 1: chapter name string
    r'\s*,\s*'
    r'(-?\d+(?:\.\d+)?)'                        # group 2: expected chapter float
    r'\s*\)'
)


def unescape_kotlin(s: str) -> str:
    """Kotlin string escapes — same surface as C# for our purposes."""
    return (
        s.replace(r'\\', '\x00')
         .replace(r'\"', '"')
         .replace(r'\n', '\n')
         .replace(r'\t', '\t')
         .replace('\x00', '\\')
    )


def extract(text: str) -> list[dict]:
    entries = []
    for m in ASSERT_CHAPTER.finditer(text):
        name, chapter = m.groups()
        entries.append({
            "input": unescape_kotlin(name),
            "expected_chapter_f64": float(chapter),
            "source": SOURCE_TAG,
        })
    return entries


def main() -> int:
    req = urllib.request.Request(URL, headers={"User-Agent": "chaptr-corpus-extractor"})
    with urllib.request.urlopen(req, timeout=30) as resp:
        text = resp.read().decode("utf-8")
    entries = extract(text)
    print(json.dumps(entries, indent=2, ensure_ascii=False))
    print(f"# extracted {len(entries)} entries from {SOURCE_TAG}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
