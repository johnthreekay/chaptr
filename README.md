# chaptr

A filename tokenizer for manga, manhwa, manhua, and light novels, written in Rust.

`chaptr` takes a release filename `[Group] Title v03 c042.5 (Digital).cbz`, `Some Light Novel v05 (Yen Press) (Digital) [LuCaZ].epub`, and returns a struct with the parts named: title, volume, chapter, group, source, language, revision, extension. The shape mirrors what [anitomy-rs](https://github.com/Rapptz/anitomy-rs) does for anime release titles, but for the manga/LN side, where anitomy doesn't have a chapter or volume concept and treats `c042` as noise.

I'm building this because the next version of [Ryokan](https://github.com/johnthreekay/Ryokan) is being extended to cover manga and light novels, and the torrent-grab path needs structured filename parsing for dupe detection (same chapter from different scanlation groups), upgrade detection (`v2` revisions), range grabs (`c001-050 (Batch).cbz`), and Custom Format scoring. Doing that in template strings or one-off regexes inside the consumer is a known footgun, hence a real library.

## Status

**v0.0.1: scaffold only.** The crate compiles, the type surface and module tree are fixed, but `manga::parse` and `novel::parse` currently return default-constructed structs. The lexer is a stub that returns an empty token list. Lookup tables for scanlation groups are empty; LN publishers and scanners carry a small hand-seeded list from the design doc's named examples.

Real parser logic is gated on a corpus pass. The plan (see [implementation ordering](#implementation-ordering)) is to collect 500+ real manga and ~300+ real LN filenames first, then write the lexer and classifier branches against that corpus rather than against hypothetical inputs. This is the same pattern that made anitomy good.

If you need this to actually work today, it doesn't yet. Watch the repo, or come back when v0.1.0 ships.

## What it does (eventually)

```rust
use chaptr::manga;

let parsed = manga::parse("[MangaPlus] Chainsaw Man v12 c103 (Digital).cbz");
// parsed.title    == Some("Chainsaw Man")
// parsed.group    == Some("MangaPlus")
// parsed.volume   == Some(NumberRange::single(ChapterNumber::new(12)))
// parsed.chapter  == Some(NumberRange::single(ChapterNumber::new(103)))
// parsed.source   == Some(MangaSource::MangaPlus)
// parsed.extension == Some("cbz")
```

```rust
use chaptr::novel;

let parsed = novel::parse("Spice and Wolf v17 (Yen Press) (Digital) [LuCaZ].epub");
// parsed.title     == Some("Spice and Wolf")
// parsed.volume    == Some(NumberRange::single(ChapterNumber::new(17)))
// parsed.publisher == Some("Yen Press")
// parsed.scanner   == Some("LuCaZ")
// parsed.is_digital == true
// parsed.extension == Some("epub")
```

Both entry points are pure functions - string in, struct out, no I/O, no allocations beyond what the input forces. String fields borrow from the input via `&'a str`, so a `ParsedManga<'a>` is cheap to produce and discard.

### Cases the parser is being designed to handle

- **Chapter vs volume disambiguation.** `v03.cbz` and `c03.cbz` are different grabs and must be distinguishable.
- **Decimal chapters.** `c42.5` (omake / side stories), `c42v2` (revision of chapter 42), `c42.5v2` (revision of the omake) all map to distinct `(chapter, revision)` pairs. `ChapterNumber` is `(whole: u32, decimal: Option<u16>)` rather than `f64` to keep equality and ordering exact.
- **Range grabs.** `c001-050 (Batch).cbz` parses as `chapter: NumberRange { start: 1, end: Some(50) }`.
- **Multiple sources.** A `(Digital)` tag, `[MangaPlus]` group, and Lezhin/Naver/Kakao manhwa-native source tags all reduce to a single `MangaSource` enum.
- **Revision tracking.** `v2` / `v3` is parsed as a `revision: Option<u8>`, distinct from volume number.

## Scope

| In | Out |
|---|---|
| Manga, manhwa, manhua filenames (`.cbz`, `.cbr`, `.zip`, `.7z`, `.pdf`) | Anime — use [anitomy-rs](https://github.com/Rapptz/anitomy-rs) |
| Light novel filenames (`.epub`, `.pdf`, `.azw3`, `.mobi`, `.txt`) | Web novels — content is HTML scraped into Ryokan-controlled EPUBs, no external filename to parse |
| Pure string-in, struct-out parsing | Sidecar reading (`ComicInfo.xml`, OPF) — belongs one layer up in the consumer |
| Compile-time lookup tables for known groups, publishers, scanners | Network or filesystem I/O of any kind |

Manhwa and manhua live in the `manga` module — the filename grammar is close enough that splitting them out would just duplicate the lexer for no payoff. Source-tag differences (Lezhin / Naver / Kakao) are carried by the `MangaSource` enum.

## Module layout

```
src/
├── lib.rs          // shared types: ChapterNumber, NumberRange, Language
├── lexer.rs        // L1: tokenize bracketed/parenthesized/word/delimiter spans
├── manga.rs        // ParsedManga, MangaSource, manga::parse
├── novel.rs        // ParsedNovel, novel::parse
└── tables/
    ├── scanlator_groups.rs
    ├── ln_publishers.rs
    └── ln_scanners.rs
```

Both domain modules consume the same `Token` stream from `lexer`. A regression in the lexer must be regression-tested against both manga and LN corpora — duplicating lexer logic per-domain is the failure mode this layout exists to prevent.

## Building

```bash
cargo build
cargo run
```

Requires Rust 1.95+ (edition 2024). No native dependencies, no build script — pure Rust.

## Design notes

Key calls baked into the current scaffold:

- **One library, two modules, shared lexer.** Manga and LN diverge at L2 but consume the same L1 tokenization.
- **`ChapterNumber` is integer + optional u16 decimal**, not f64. Sort keys, equality, and hashing all work without precision footguns. Documented inline at the type definition.
- **Lookup tables are compile-time.** Currently `&'static [(&str, T)]` linear-scan slices; will graduate to `phf::Map` once any single table grows past ~20 entries.
- **No `Result` in the public API.** Parsing never fails — it produces a `ParsedX` with `None` fields for anything it can't extract. Callers that want to signal "totally unparseable" can check whether *any* field came back populated.
- **Inline `#[cfg(test)] mod tests`** at the bottom of each file. No top-level `tests/` directory.

### Implementation ordering

1. Build the corpus (500+ manga filenames, 300+ LN filenames). Sort by naming pattern.
2. Implement `lexer::tokenize` against the corpus.
3. Implement `manga::parse` (larger surface, more value, better corpus).
4. Implement `novel::parse` (narrower grammar).
5. Wire into Ryokan v2.0.0's manga/LN search handler.

## License

GPL-3.0-or-later. See [LICENSE](LICENSE).
