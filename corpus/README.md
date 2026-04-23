# Corpus

Hand-curated and lifted-from-upstream test fixtures for `chaptr`.

The corpus is the test suite. Each file here is a JSON array of entries with the
shape `{ input, expected_*, source }`. The `expected_*` fields are partial
assertions — most upstream-lifted fixtures only assert one field per entry
(volume, or chapter, or series), and we honor that scoping rather than
fabricating richer expectations we can't verify.

## Files

| File | Origin | Entries | Asserts | License |
|---|---|---|---|---|
| `manga_kavita.json` | Kavita `MangaParsingTests.cs` | 350 | one of: `volume`, `series`, `chapter`, `edition` (per-entry, tagged in `source`) | GPL-3.0 |
| `novel_kavita.json` | Kavita `BookParsingTests.cs` | 5 | `volume` or `series` | GPL-3.0 |
| `chapters_mihon.json` | Mihon `ChapterRecognitionTest.kt` | 54 | `chapter` as `f64` | Apache-2.0 |

Kavita's `DefaultParserTests.cs` was inspected and excluded — its fixtures are
folder-path strings (`/manga/Btooom!/Vol.1/Chapter 1/1.cbz`), not pure
filenames. Folder-aware parsing is a layer above chaptr's surface; consumers
that want it concatenate path components themselves.

The same input filename frequently appears under multiple Kavita test methods
— e.g. `Killing Bites Vol. 0001 Ch. 0001` shows up once asserting volume = "1",
once asserting chapter = "1", once asserting series = "Killing Bites". That's
350 *entries*, not 350 unique filenames; expect roughly 120-150 unique inputs.

## Origin and license

- **Kavita** — https://github.com/Kareadita/Kavita, GPL-3.0. Compatible with
  chaptr's GPL-3.0-or-later. No additional NOTICE required beyond the source
  attribution embedded in each `source` field.
- **Mihon** — https://github.com/mihonapp/mihon, Apache-2.0. Compatible with
  GPL-3.0-or-later via Apache-to-GPLv3 one-way compatibility. Apache requires
  preserving the license text and original attribution; see
  `../LICENSES/Apache-2.0.txt` and the `source` field on each Mihon-derived
  entry.

Re-extraction of upstream fixtures (e.g. when Kavita or Mihon adds new test
cases) is supported by `tools/extract_kavita.py` and `tools/extract_mihon.py` —
both are pure stdlib Python, no dependencies.

## Adding new entries

The corpus is meant to grow incrementally as bugs surface. The pattern, copied
from anitomy:

1. A real-world filename produces a wrong parse.
2. Add it to the appropriate corpus file with the expected (correct) parse.
3. Watch the test fail.
4. Fix the parser. Test passes.
5. Commit.

Don't add fixtures speculatively. If you can't tie a fixture to a real release
or a real bug, it's not pulling its weight.

## Schema

Common fields across all corpus files:

- `input` — the filename string, exactly as it would appear on disk.
- `source` — origin tag (`"kavita-MangaParsingTests"`, `"mihon-ChapterRecognitionTest"`,
  `"nyaa"`, `"hand-added"`, etc.). Used for attribution and to know which
  upstream extractor will overwrite this entry on re-extraction (don't
  hand-edit upstream-sourced entries — they get clobbered).

File-specific fields:

- `expected_volume` / `expected_series` / `expected_chapter` / `expected_edition`
  (manga_kavita, novel_kavita) — Kavita's asserted value as a string. Exactly
  one of these is present per entry, matching which Kavita test method it came
  from. The method name is the suffix of `source` (e.g.
  `kavita-MangaParsingTests::ParseVolumeTest`). Values may be `null` when
  Kavita's `Parser.LooseLeafVolume` sentinel was used (= "no value detected"),
  with the original sentinel name preserved in `_sentinel` for traceability.
- `expected_chapter_f64` (chapters_mihon) — Mihon's asserted chapter value as
  a float. Handle decimals exactly; chaptr's `ChapterNumber` round-trips these
  via `(whole, decimal)` decomposition.

The chaptr test code translates between its own `ParsedManga` / `ChapterNumber`
shape and these per-field asserts at comparison time. See `src/manga.rs` tests
when they land.
