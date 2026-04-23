# chaptr

A filename tokenizer for manga, manhwa, manhua, and light novels, written in Rust.

`chaptr` takes a release filename — `[Group] Title v03 c042.5 (Digital).cbz`, `Some Light Novel v05 (Yen Press) (Digital) [LuCaZ].epub` — and returns a struct with the parts named: title, volume, chapter, group, source, edition, language, revision, extension. The shape mirrors what [anitomy-rs](https://github.com/Rapptz/anitomy-rs) does for anime release titles, but for the manga/LN side, where anitomy doesn't have a chapter or volume concept and treats `c042` as noise.

It exists because the next version of [Ryokan](https://github.com/johnthreekay/Ryokan) (a self-hosted anime PVR) is being extended to cover manga and light novels, and the torrent-grab path needs structured filename parsing for dupe detection (same chapter from different scanlation groups), upgrade detection (`v2` revisions), range grabs (`c001-050 (Batch).cbz`), and Custom Format scoring. Doing that in template strings or one-off regexes inside the consumer is a known footgun, hence a real library.

## Install

```bash
cargo add chaptr
```

## Example

```rust
use chaptr::{manga, novel};

let m = manga::parse("[MangaPlus] Chainsaw Man v12 c103 (Digital).cbz");
assert_eq!(m.title.as_deref(), Some("Chainsaw Man"));
assert_eq!(m.group,             Some("MangaPlus"));
assert_eq!(m.source,            Some(manga::MangaSource::Digital));
assert_eq!(m.extension,         Some("cbz"));
// m.volume and m.chapter are structured NumberRange values

let n = novel::parse("[Unpaid Ferryman] Youjo Senki v01-23 (2018-2024) (Digital) (LuCaZ)");
assert_eq!(n.group,     Some("Unpaid Ferryman"));
assert_eq!(n.scanner,   Some("LuCaZ"));
assert_eq!(n.year,      Some(2018));
assert!(n.is_digital);
// n.volume is a range 1..=23
```

Both entry points are pure functions — string in, struct out, no I/O. String fields borrow from the input via `&'a str` where no normalization is needed. `title` is a `Cow<'a, str>` so underscore-as-space normalization (`B_Gata_H_Kei` → `"B Gata H Kei"`) borrows when possible and allocates only when it must.

## What it handles

**Volume detection:**
- Single-token: `v01`, `Vol01`, `Volume01`, `S01` (season-style), `t6` / `T2000` (French/Batman)
- Multi-token: `Vol 1`, `Vol. 1`, `Volume 11`, `Том 1` (Russian), `Tome 2` (French)
- Decimals: `v1.1`, `v03.5`
- Ranges: `v01-09`, `v16-17`, `Том 1-4`
- Nested in parens/brackets: `(v01)`, `[Volume 11]`
- CJK postfix: `1巻` (Japanese), `第03卷` (Chinese), `13장` (Korean)
- Range validation rejects backward ranges: `vol_356-1` → 356, not 356-1

**Chapter detection:**
- Single-token: `c001`, `Ch001`, `Chp02`, `Chapter001`, `Chapter11v2` (revision silently consumed)
- Multi-token: `Ch 4`, `Ch. 4`, `Chapter 12`, `Глава 3` (Russian), `Episode 406`
- Decimals: `c42.5`
- Ranges: `c001-008`
- CJK postfix: `第25话` / `章` / `回` / `회` / `화`
- Bare-number fallback: `Hinowa ga CRUSH! 018 (2019)` → 18 (requires following metadata)

**Title extraction:**
- Slices from after leading group bracket(s) to first marker/trailing-bracket/extension
- Normalizes underscores to spaces (`B_Gata_H_Kei` → `"B Gata H Kei"`)
- Skips leading paren/bracket/curly chains (`(一般コミック) [奥浩哉] いぬやしき` → `いぬやしき`)
- Trims trailing punctuation (`-`, `.`, `_`, `,`, `#`, `:`)
- Disambiguates title-vs-chapter numbers (`Kaiju No. 8 036` → title `Kaiju No. 8`, chapter 36)

**Group / publisher / scanner:**
- Manga: first non-volume-keyword bracketed token
- Novel: first leading bracket that isn't a known publisher or scanner
- Publisher/scanner lookup against compile-time tables (Yen Press, J-Novel Club, Seven Seas, LuCaZ, Stick, CleanBookGuy, etc.)

**Source (manga):** `Digital`, `MangaPlus`, `Viz`, `Kodansha`, `Lezhin`, `Naver`, `Kakao` — each with common aliases (`Viz Media`, `Kodansha USA`, `Naver Webtoon`, `Kakao Page`, `Digital-HD`).

**Edition (manga):** `Omnibus`, `Uncensored`, `Omnibus Edition` compounds.

**LN-specific:** `is_digital` / `is_premium` tags, year extraction, revision from `{r2}`-style curly tags.

## Scope

| In | Out |
|---|---|
| Manga, manhwa, manhua filenames (`.cbz`, `.cbr`, `.zip`, `.7z`, `.rar`, `.pdf`) | Anime — use [anitomy-rs](https://github.com/Rapptz/anitomy-rs) |
| Light novel filenames (`.epub`, `.pdf`, `.azw3`, `.mobi`, `.txt`) | Web novels — content is HTML scraped into controlled EPUBs, no external filename to parse |
| String-in, struct-out | Sidecar reading (`ComicInfo.xml`, OPF) — belongs one layer up in the consumer |
| Compile-time tables for known groups, publishers, scanners | Network or filesystem I/O |

Manhwa / manhua live in the `manga` module. Grammar is close enough that splitting them wouldn't pay for duplicated lexer logic — source-tag differences (Lezhin / Naver / Kakao) are carried by the `MangaSource` enum.

## Test coverage

- **145 unit tests**, clippy + fmt clean
- **`corpus/manga_kavita.json`** — 350 real-world fixtures lifted from [Kavita](https://github.com/Kareadita/Kavita)'s manga parser tests (GPL-3.0, per-entry attribution). Current aggregate pass rate: **98.5%**. Per-method:

  | Method | Rate |
  |---|---|
  | ParseVolumeTest | 100% |
  | ParseDuplicateVolumeTest | 90.5% (Thai-only failures) |
  | ParseChaptersTest | 100% |
  | ParseDuplicateChapterTest | 100% |
  | ParseExtraNumberChaptersTest | 100% |
  | ParseSeriesTest | 97.7% (Thai-only failures) |
  | ParseEditionTest | 100% |

- **`corpus/novel_nyaa.json`** — 8 hand-picked LN fixtures from Nyaa with full-struct field assertions. Current pass rate: **100%** (22/22 field asserts).

- **`corpus/chapters_mihon.json`** — 54 chapter-number edge cases from [Mihon](https://github.com/mihonapp/mihon) (Tachiyomi successor), Apache-2.0. Loaded for smoke but not asserted against — our chapter model is a `ChapterNumber { whole, decimal }` tuple while Mihon's is `f64`, so direct equality isn't meaningful.

- **`corpus/smoke_novel.txt`** — 512 real Nyaa LN filenames. `novel::parse` must not panic on any of them; includes false-positive manga entries deliberately so the LN parser also has to degrade gracefully on out-of-domain input.

## Known scope gaps

All documented in the module-level doc comments. The ones that show up as corpus failures:

- **Thai `เล่ม` / `เล่มที่`** — Ryokan's intended upstream (Nyaa English-translated) doesn't carry Thai script; supporting it would need lexer changes for Thai combining marks and additional keyword entries. The five remaining Kavita failures are all in this bucket.
- **X-suffix ranges** — `c001-006x1` (rare Kavita syntax)
- **Kavita "special" empty-series cases** — filenames like `Love Hina - Special.cbz` where Kavita expects empty series; no oneshot/special detection yet

Closed in 1.4.0: reverse-range CJK (`38-1화` → 38), `#N`-at-end chapter detection (`Episode 3 ... #02` → 2), mixed-prefix chapter range (`c01-c04` → 1-4), trailing title-dot preservation (`Hentai Ouji... Neko.`), Russian postfix Том (`5 Том Test` → vol 5).

Closed in 1.2.0: Korean `시즌` multi-char prefix (`시즌34삽화2` → volume 34), alpha-suffix decimals (`Beelzebub_153b` → 153.5 per Kavita convention).

## Design

- **One library, two modules, shared lexer.** `manga::parse` and `novel::parse` consume the same `Token` stream from `lexer::tokenize`; domain-specific L2 classifiers on top. Shared detectors (volume, chapter, title slicing, CJK markers) live in a private `common` module so a bug fix hits both domains identically.
- **`ChapterNumber` is `(whole: u32, decimal: Option<u16>)`**, not `f64`. Sort keys, equality, and hashing all work without precision footguns. Decimal chapters (`c42.5`) and revisions (`c42v2`) are distinct values, not colliding.
- **Lookup tables are compile-time** `&'static [(&str, T)]` slices. Graduates to `phf::Map` when any single table exceeds ~20 entries (none do yet).
- **No `Result` in the public API.** Parsing never fails — it produces a `Parsed_` with `None` fields for anything it can't extract.
- **Inline `#[cfg(test)] mod tests`** at the bottom of each file. No top-level `tests/` directory.

## Performance

Per-call cost is microseconds on modern hardware; tokenization plus a few
small token scans, no heap allocation in the common path (`title` only
allocates when underscore-to-space normalization forces it).

| Bench | Time |
|---|---|
| `manga::parse` on a typical filename | ~1.0 µs |
| `manga::parse` with CJK markers | ~1.5 µs |
| `novel::parse` on a typical filename | ~0.6 µs |
| 512-entry LN smoke corpus batch | ~0.8 ms (~660 K entries/sec) |

For a 100-torrent Nyaa search result batch, total parse time is under
0.15 ms — the parser isn't the bottleneck in a search pipeline.

(1.2.0 dropped ~33% off every bench by removing a per-Word `Vec<char>`
allocation on the CJK marker check path, which every parse walks through
regardless of whether the word has CJK chars.)

Run `cargo bench` for fresh numbers on your hardware. The `benches/`
directory has a [Criterion](https://github.com/bheisler/criterion.rs)
harness with representative inputs.

## Building locally

```bash
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
```

Requires Rust 1.95+ (edition 2024). No native dependencies, no build script.

## License

GPL-3.0-or-later. See [LICENSE](LICENSE).

Test corpus entries under `corpus/` are lifted with attribution from:
- [Kavita](https://github.com/Kareadita/Kavita) (GPL-3.0)
- [Mihon](https://github.com/mihonapp/mihon) (Apache-2.0, see [LICENSES/Apache-2.0.txt](LICENSES/Apache-2.0.txt))
