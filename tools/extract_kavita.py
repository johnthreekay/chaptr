#!/usr/bin/env python3
"""Extract InlineData entries from Kavita's parsing test files into chaptr corpus JSON.

Usage:
    python3 tools/extract_kavita.py manga > corpus/manga_kavita.json
    python3 tools/extract_kavita.py default > corpus/default_kavita.json
    python3 tools/extract_kavita.py book > corpus/novel_kavita.json

Walks the file once, tracking the current `[Theory] public void <Method>(...)`
context so each `[InlineData(...)]` gets tagged with which test method it
belongs to. This matters because Kavita's parsing test files mix volume,
series, chapter, and edition assertions in the same file — without method
context, all entries get mislabeled as the same field.

Methods that test non-string outputs (booleans, enums) are skipped.

Pure stdlib. No deps.
"""

import json
import re
import sys
import urllib.request

KAVITA_FILES = {
    "manga": (
        "https://raw.githubusercontent.com/Kareadita/Kavita/develop/Kavita.Services.Tests/Parsing/MangaParsingTests.cs",
        "kavita-MangaParsingTests",
    ),
    "default": (
        "https://raw.githubusercontent.com/Kareadita/Kavita/develop/Kavita.Services.Tests/Parsers/DefaultParserTests.cs",
        "kavita-DefaultParserTests",
    ),
    "book": (
        "https://raw.githubusercontent.com/Kareadita/Kavita/develop/Kavita.Services.Tests/Parsing/BookParsingTests.cs",
        "kavita-BookParsingTests",
    ),
}

# Method-name → corpus field name. Methods returning non-string outputs
# (booleans for IsSpecial / IsManga, enums for ParseFormat) are dropped — chaptr
# doesn't have equivalent fields and faking them would just bloat the corpus.
METHOD_TO_FIELD = {
    "ParseVolumeTest":               "expected_volume",
    "ParseDuplicateVolumeTest":      "expected_volume",
    "ParseSeriesTest":               "expected_series",
    "ParseChaptersTest":             "expected_chapter",
    "ParseExtraNumberChaptersTest":  "expected_chapter",
    "ParseDuplicateChapterTest":     "expected_chapter",
    "ParseEditionTest":              "expected_edition",
    "ParseYearTest":                 "expected_year",
    "ParseSeriesSortTest":           "expected_series_sort",
    "ParseLocalizedSeriesTest":      "expected_localized_series",
    # Default parser tests:
    "ParseFromFallbackFolders":      "expected_series",
    "Parse_MangaSeries":             "expected_series",
    "Parse_MangaVolume":             "expected_volume",
    # Book/LN tests:
    "ParseSeries":                   "expected_series",
    "ParseVolume":                   "expected_volume",
}

METHOD_DECL = re.compile(r'public\s+void\s+(\w+)\s*\(')

# Match: [InlineData("input", "expected")] or [InlineData("input", Parser.Sentinel)]
INLINE_DATA = re.compile(
    r'\[InlineData\(\s*'
    r'"((?:[^"\\]|\\.)*)"'                      # group 1: input
    r'\s*,\s*'
    r'(?:"((?:[^"\\]|\\.)*)"|Parser\.(\w+)|(true|false))'  # group 2: str | group 3: sentinel | group 4: bool
    r'\s*\)\]'
)


def unescape_csharp(s: str) -> str:
    return (
        s.replace(r'\\', '\x00')
         .replace(r'\"', '"')
         .replace(r'\n', '\n')
         .replace(r'\t', '\t')
         .replace('\x00', '\\')
    )


def extract(text: str, source_tag: str) -> list[dict]:
    entries: list[dict] = []
    pending: list[dict] = []  # InlineData seen before the next method declaration
    current_method: str | None = None

    for line in text.splitlines():
        m_decl = METHOD_DECL.search(line)
        if m_decl:
            current_method = m_decl.group(1)
            # Flush pending entries against this method
            field = METHOD_TO_FIELD.get(current_method)
            if field is not None:
                for e in pending:
                    e["source"] = f"{source_tag}::{current_method}"
                    if e.pop("_is_bool", False):
                        # IsSpecial-style boolean tests aren't useful for chaptr — drop
                        continue
                    e[field] = e.pop("_expected")
                    if "_sentinel" in e:
                        # Kavita's Parser.LooseLeafVolume etc. sentinels mean
                        # "no value detected" — preserve the name for traceability
                        # and set the expected field to None.
                        e[field] = None
                    entries.append(e)
            pending = []
            continue

        m_data = INLINE_DATA.search(line)
        if m_data:
            input_str, expected_str, sentinel, bool_str = m_data.groups()
            entry: dict = {"input": unescape_csharp(input_str)}
            if expected_str is not None:
                entry["_expected"] = unescape_csharp(expected_str)
            elif sentinel is not None:
                entry["_expected"] = None
                entry["_sentinel"] = sentinel
            else:
                # Boolean — flag for dropping at flush time
                entry["_is_bool"] = True
                entry["_expected"] = bool_str
            pending.append(entry)

    # Tail flush — any pending entries after the last method declaration are dropped
    # (shouldn't happen in well-formed test files, but log it if it does).
    if pending:
        print(f"# warning: {len(pending)} InlineData entries after last method declaration, dropped",
              file=sys.stderr)

    return entries


def main() -> int:
    if len(sys.argv) != 2 or sys.argv[1] not in KAVITA_FILES:
        print(f"usage: {sys.argv[0]} {{{ '|'.join(KAVITA_FILES) }}}", file=sys.stderr)
        return 2
    url, source_tag = KAVITA_FILES[sys.argv[1]]
    req = urllib.request.Request(url, headers={"User-Agent": "chaptr-corpus-extractor"})
    with urllib.request.urlopen(req, timeout=30) as resp:
        text = resp.read().decode("utf-8")
    entries = extract(text, source_tag)
    print(json.dumps(entries, indent=2, ensure_ascii=False))

    # Per-method count summary to stderr
    by_method: dict[str, int] = {}
    for e in entries:
        method = e["source"].split("::", 1)[1]
        by_method[method] = by_method.get(method, 0) + 1
    print(f"# extracted {len(entries)} entries from {source_tag}:", file=sys.stderr)
    for m, n in sorted(by_method.items(), key=lambda kv: -kv[1]):
        print(f"#   {m}: {n}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
