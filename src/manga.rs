//! Manga / manhwa / manhua filename parser.
//!
//! Manhwa/manhua live here too — their filename grammar overlaps closely with manga.
//! Source-tag differences (Lezhin / Naver / Kakao for manhwa) are carried by
//! [`MangaSource`], not by a separate module.
//!
//! **v0 scope**: extension, volume (single-token `v01`, multi-token `Vol 1`,
//! decimal `v03.5`, range `v01-09`, also `(v01)` and `[Volume 11]` nested
//! forms), chapter (same prefixed patterns under `c`/`Ch`/`Chapter` plus
//! revision-suffix `Chapter11v2` and a bare-number fallback for
//! `Beelzebub_01_[Noodles].zip`-style filenames), group (first non-volume
//! bracketed token), source (`Digital`, `MangaPlus`). Range validation
//! rejects backward ranges (`vol_356-1` parses as 356, not 356-1).
//!
//! Season-style markers `S01` are handled as volume (Tower of God uses this
//! convention); single-letter `s` only matches with following digits, so
//! titles like "Sword" or "Spy" are safe.
//!
//! **Out of scope for v0** (intentional, documented gaps): title extraction,
//! language tags, revision *extraction* (the suffix is consumed but not
//! stored), oneshot detection, CJK volume markers (巻 / 卷 / 册), Cyrillic
//! markers (Том / Глава), Korean markers (권 / 장), Thai markers (เล่ม),
//! alpha-suffix decimal chapters (`Beelzebub_153b` = chapter 153.5),
//! `c001-006x1`-style chapter ranges with extra suffixes, and the rest of
//! `MangaSource` (Viz / Kodansha / Lezhin / Naver / Kakao). These come back
//! as the corpus pass-rate test pushes them up the priority list.

use crate::lexer::{Token, tokenize};
use crate::{ChapterNumber, Language, NumberRange};

/// Structured fields parsed from a single manga filename.
///
/// String fields borrow from the input (`'a`). Construct via [`parse`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ParsedManga<'a> {
    pub title: Option<&'a str>,
    /// Volume designator: `v01`, `v01-03`.
    pub volume: Option<NumberRange>,
    /// Chapter designator: `c045`, `c001-010`, `c045.5`.
    pub chapter: Option<NumberRange>,
    /// Scanlation group, typically inside `[...]`.
    pub group: Option<&'a str>,
    /// Source provenance (Digital, MangaPlus, scan, etc.).
    pub source: Option<MangaSource>,
    pub language: Option<Language>,
    /// Revision marker (`v2`, `v3`) — distinct from volume number.
    pub revision: Option<u8>,
    pub is_oneshot: bool,
    /// File extension without the leading dot (`cbz`, `cbr`, `zip`, `7z`, `pdf`).
    pub extension: Option<&'a str>,
}

/// Origin of the scanned/digital source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MangaSource {
    /// `(Digital)` tag — generic digital release, unspecified retailer.
    Digital,
    /// MANGA Plus by Shueisha.
    MangaPlus,
    /// Viz Media digital.
    Viz,
    /// Kodansha digital.
    Kodansha,
    /// Generic physical-source scan (no retailer specified).
    Scan,
    /// Lezhin Comics (manhwa).
    Lezhin,
    /// Naver Webtoon / Series (manhwa).
    Naver,
    /// Kakao Page (manhwa).
    Kakao,
}

/// Parse a manga filename into structured fields.
#[must_use]
pub fn parse(filename: &str) -> ParsedManga<'_> {
    let tokens = tokenize(filename);
    ParsedManga {
        title: None,
        volume: detect_volume(&tokens),
        chapter: detect_chapter(&tokens),
        group: detect_group(&tokens),
        source: detect_source(&tokens),
        language: None,
        revision: None,
        is_oneshot: false,
        extension: detect_extension(filename),
    }
}

const MANGA_EXTENSIONS: &[&str] = &["cbz", "cbr", "zip", "7z", "rar", "pdf"];

fn detect_extension(filename: &str) -> Option<&str> {
    let dot_pos = filename.rfind('.')?;
    let ext = &filename[dot_pos + 1..];
    // Bound the length — `Vol.0001 (Digital)` would otherwise match `0001 (Digital)` as ext.
    if ext.len() > 4 || ext.is_empty() {
        return None;
    }
    let lower = ext.to_ascii_lowercase();
    if MANGA_EXTENSIONS.contains(&lower.as_str()) {
        Some(ext)
    } else {
        None
    }
}

fn detect_volume(tokens: &[Token]) -> Option<NumberRange> {
    // Pass 1: top-level. The vast majority of fixtures hit here.
    if let Some(r) = detect_volume_in_seq(tokens) {
        return Some(r);
    }
    // Pass 2: inside parens / brackets — captures `(v01)` and `[Volume 11]`
    // shapes where the volume marker was wrapped in another token. Re-tokenize
    // the inner content; the wrapped-once depth is enough for every corpus
    // case observed so far (no `((v01))` style nesting in the wild).
    for t in tokens {
        let inner = match t {
            Token::Parenthesized(s) | Token::Bracketed(s) => *s,
            _ => continue,
        };
        let inner_tokens = tokenize(inner);
        if let Some(r) = detect_volume_in_seq(&inner_tokens) {
            return Some(r);
        }
    }
    None
}

fn detect_volume_in_seq(tokens: &[Token]) -> Option<NumberRange> {
    for (i, t) in tokens.iter().enumerate() {
        let Token::Word(w) = t else { continue };

        // Single-token form: v01 / vol01 / volume01
        if let Some(num_str) = strip_volume_prefix(w)
            && let Ok(start_whole) = num_str.parse::<u32>()
        {
            return Some(build_range(tokens, i + 1, start_whole));
        }

        // Multi-token form: Vol / Volume + delim(s) + number
        if eq_ignore_ascii_case_any(w, &["vol", "volume"])
            && let Some(range) = parse_number_after_marker(tokens, i + 1)
        {
            return Some(range);
        }
    }
    None
}

fn detect_chapter(tokens: &[Token]) -> Option<NumberRange> {
    // Pass 1: top-level prefixed forms (c042 / Ch.4 / Chapter 12 / Chapter11v2)
    if let Some(r) = detect_chapter_in_seq(tokens) {
        return Some(r);
    }
    // Pass 2: inside parens / brackets
    for t in tokens {
        let inner = match t {
            Token::Parenthesized(s) | Token::Bracketed(s) => *s,
            _ => continue,
        };
        let inner_tokens = tokenize(inner);
        if let Some(r) = detect_chapter_in_seq(&inner_tokens) {
            return Some(r);
        }
    }
    // Pass 3: bare-number fallback for `Beelzebub_01_[Noodles].zip`,
    // `Hinowa ga CRUSH! 018`, and similar where the chapter is just a
    // number with no `c`/`Ch`/`Chapter` prefix.
    detect_bare_number_chapter(tokens)
}

fn detect_chapter_in_seq(tokens: &[Token]) -> Option<NumberRange> {
    for (i, t) in tokens.iter().enumerate() {
        let Token::Word(w) = t else { continue };

        // Single-token form: c001 / ch001 / chapter001 / chapter11v2 (revision suffix)
        if let Some(start_whole) = parse_chapter_num_from_word(w) {
            return Some(build_range(tokens, i + 1, start_whole));
        }

        // Multi-token form: Ch / Chapter + delim(s) + number
        if eq_ignore_ascii_case_any(w, &["ch", "chapter"])
            && let Some(range) = parse_number_after_marker(tokens, i + 1)
        {
            return Some(range);
        }
    }
    None
}

/// Parse a single Word as a chapter number with optional revision suffix.
///
/// Pattern: `<prefix><digits>[v<digits>]` where prefix is `chapter`/`ch`/`c`
/// (case-insensitive). Examples: `c042`, `Ch11v2`, `Chapter51v2`.
///
/// The revision component is silently consumed but not returned — `revision`
/// extraction is not in v0 scope. The point is to recognize that `Chapter11v2`
/// is chapter 11 (with revision 2) rather than treating it as malformed.
fn parse_chapter_num_from_word(word: &str) -> Option<u32> {
    // `chp` covers `vol01_chp02`-style filenames; tried before `ch` so it
    // matches "chp02" as prefix "chp" + digits "02" rather than "ch" + "p02".
    for prefix in ["chapter", "chp", "ch", "c"] {
        let Some(rest) = strip_prefix_ignore_ascii_case(word, prefix) else {
            continue;
        };
        let digits_end = rest.bytes().take_while(u8::is_ascii_digit).count();
        if digits_end == 0 {
            continue;
        }
        let Ok(num) = rest[..digits_end].parse::<u32>() else {
            continue;
        };
        let after = &rest[digits_end..];
        if after.is_empty() {
            return Some(num);
        }
        // Optional `v<digits>` revision suffix
        if let Some(rev_part) = strip_prefix_ignore_ascii_case(after, "v")
            && !rev_part.is_empty()
            && rev_part.chars().all(|c| c.is_ascii_digit())
        {
            return Some(num);
        }
    }
    None
}

/// Find an isolated all-digit Word that's plausibly a chapter number.
///
/// Heuristics, all required:
/// - Must come *after* at least one non-numeric Word (skips `100 Years Of Solitude`)
/// - Must not be year-shaped (1900-2099) — those are almost never chapter numbers
/// - Must not be immediately preceded by `Vol`/`Volume` keyword (skips the
///   number that belongs to the volume marker, e.g. the `5` in `Vol 5`)
/// - Combined-token volume markers like `v05` do *not* block a following bare
///   number — `v05 042` should yield chapter 42, not nothing
///
/// First match wins. Returns `None` if no match.
fn detect_bare_number_chapter(tokens: &[Token]) -> Option<NumberRange> {
    let mut saw_non_numeric_word = false;
    let mut prev_was_vol_keyword = false;

    for (i, t) in tokens.iter().enumerate() {
        match t {
            Token::Word(w) => {
                let all_digits = w.chars().all(|c| c.is_ascii_digit());
                if all_digits
                    && saw_non_numeric_word
                    && !prev_was_vol_keyword
                    && let Ok(n) = w.parse::<u32>()
                    && !looks_like_year(n)
                {
                    return Some(build_range(tokens, i + 1, n));
                }
                if !all_digits {
                    saw_non_numeric_word = true;
                }
                // Only the multi-token `Vol`/`Volume` keyword blocks the next
                // number; combined-token forms like `v05` already absorbed
                // their number into the same Word.
                prev_was_vol_keyword = eq_ignore_ascii_case_any(w, &["vol", "volume"]);
            }
            Token::Delimiter(_) => {} // delimiters don't reset prev_was_vol_keyword
            _ => prev_was_vol_keyword = false,
        }
    }
    None
}

fn looks_like_year(n: u32) -> bool {
    (1900..=2099).contains(&n)
}

/// Build a [`NumberRange`] starting at `start_whole`, then optionally consume
/// a `.<dec>` decimal suffix and an `-<num>[.<dec>]` range end from the
/// following tokens.
fn build_range(tokens: &[Token], cursor: usize, start_whole: u32) -> NumberRange {
    let mut start = ChapterNumber::new(start_whole);
    let mut next = cursor;

    if let Some((dec, after)) = parse_decimal_suffix(tokens, next) {
        start = ChapterNumber::with_decimal(start_whole, dec);
        next = after;
    }

    let end = parse_range_end(tokens, next, start);
    NumberRange { start, end }
}

/// Skip up to N delimiters, then read a number-shaped Word. Used for
/// `Vol 1` / `Vol. 1` / `Volume_01` / `Chapter 12` patterns.
fn parse_number_after_marker(tokens: &[Token], start: usize) -> Option<NumberRange> {
    const MAX_DELIMS: usize = 3;
    let mut i = start;
    let mut delims_skipped = 0usize;

    while i < tokens.len() && delims_skipped < MAX_DELIMS {
        match &tokens[i] {
            Token::Delimiter(_) => {
                delims_skipped += 1;
                i += 1;
            }
            Token::Word(num_str) => {
                let start_whole = num_str.parse::<u32>().ok()?;
                return Some(build_range(tokens, i + 1, start_whole));
            }
            _ => return None,
        }
    }
    None
}

fn parse_decimal_suffix(tokens: &[Token], pos: usize) -> Option<(u16, usize)> {
    if !matches!(tokens.get(pos), Some(Token::Delimiter('.'))) {
        return None;
    }
    let Some(Token::Word(dec_str)) = tokens.get(pos + 1) else {
        return None;
    };
    let dec = dec_str.parse::<u16>().ok()?;
    Some((dec, pos + 2))
}

fn parse_range_end(tokens: &[Token], pos: usize, start: ChapterNumber) -> Option<ChapterNumber> {
    if !matches!(tokens.get(pos), Some(Token::Delimiter('-'))) {
        return None;
    }
    let Some(Token::Word(num_str)) = tokens.get(pos + 1) else {
        return None;
    };
    let end_whole = num_str.parse::<u32>().ok()?;
    let mut end = ChapterNumber::new(end_whole);
    if let Some((dec, _)) = parse_decimal_suffix(tokens, pos + 2) {
        end = ChapterNumber::with_decimal(end_whole, dec);
    }
    // Reject backward ranges (vol_356-1 → not 356-1, just 356). Any pair where
    // end < start is far more likely to be the Mangapy `<vol>-<part>` syntax
    // than a real range.
    if end < start {
        return None;
    }
    Some(end)
}

fn detect_group<'a>(tokens: &[Token<'a>]) -> Option<&'a str> {
    for t in tokens {
        if let Token::Bracketed(content) = t
            && !content.is_empty()
            && !contains_volume_keyword(content)
        {
            return Some(*content);
        }
    }
    None
}

fn detect_source(tokens: &[Token]) -> Option<MangaSource> {
    for t in tokens {
        let content = match t {
            Token::Parenthesized(s) | Token::Bracketed(s) => *s,
            _ => continue,
        };
        let lower = content.to_ascii_lowercase();
        if lower == "digital" || lower.starts_with("digital-") || lower.starts_with("digital ") {
            return Some(MangaSource::Digital);
        }
        if lower == "mangaplus" || lower == "manga plus" {
            return Some(MangaSource::MangaPlus);
        }
        if lower == "viz" {
            return Some(MangaSource::Viz);
        }
        if lower == "kodansha" {
            return Some(MangaSource::Kodansha);
        }
    }
    None
}

// --- helpers ---

fn strip_volume_prefix(word: &str) -> Option<&str> {
    // Order matters: longer prefixes first so "volume" beats "vol" beats "v".
    // `s` covers season-style markers (`S01` → volume 1) for series like
    // Tower Of God; only matches `s\d+`, so titles like "Sword" / "Spy" are safe.
    for prefix in ["volume", "vol", "v", "s"] {
        if let Some(rest) = strip_prefix_ignore_ascii_case(word, prefix)
            && !rest.is_empty()
            && rest.chars().all(|c| c.is_ascii_digit())
        {
            return Some(rest);
        }
    }
    None
}

fn strip_prefix_ignore_ascii_case<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    // `str::split_at` panics on a non-char-boundary index, which happens when
    // `s` starts with a multi-byte char like `幽` (3 bytes) and `prefix` is
    // shorter than the first char. Use bytes for the comparison and `str::get`
    // for the safe boundary check.
    let head_bytes = s.as_bytes().get(..prefix.len())?;
    if head_bytes.eq_ignore_ascii_case(prefix.as_bytes()) {
        s.get(prefix.len()..)
    } else {
        None
    }
}

fn eq_ignore_ascii_case_any(word: &str, candidates: &[&str]) -> bool {
    candidates.iter().any(|c| word.eq_ignore_ascii_case(c))
}

fn contains_volume_keyword(s: &str) -> bool {
    let lower = s.to_ascii_lowercase();
    // Conservative — full word match would be more correct but the bracket
    // contents are typically short (group names) and the false-positive risk
    // (a group named "Volunteer Scans") is rare.
    lower.contains("volume") || lower.starts_with("vol ") || lower.starts_with("vol.")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ch(whole: u32) -> ChapterNumber {
        ChapterNumber::new(whole)
    }

    fn ch_dec(whole: u32, dec: u16) -> ChapterNumber {
        ChapterNumber::with_decimal(whole, dec)
    }

    fn single(n: ChapterNumber) -> NumberRange {
        NumberRange::single(n)
    }

    fn range(a: ChapterNumber, b: ChapterNumber) -> NumberRange {
        NumberRange::range(a, b)
    }

    // ----- extension -----

    #[test]
    fn extension_known_manga_types() {
        for ext in ["cbz", "cbr", "zip", "7z", "pdf", "rar"] {
            let f = format!("Title v01.{ext}");
            assert_eq!(detect_extension(&f), Some(ext), "ext {ext}");
        }
    }

    #[test]
    fn extension_preserves_case() {
        assert_eq!(detect_extension("Title.CBZ"), Some("CBZ"));
    }

    #[test]
    fn extension_rejects_unknown() {
        assert_eq!(detect_extension("Title.png"), None);
        assert_eq!(detect_extension("Title.txt"), None); // text is LN-side
    }

    #[test]
    fn extension_rejects_long_garbage() {
        // "Vol.0001" should not match `.0001` as an extension
        assert_eq!(detect_extension("Vol.0001 Ch.001"), None);
    }

    // ----- volume -----

    #[test]
    fn volume_single_token_v01() {
        assert_eq!(parse("Title v01.cbz").volume, Some(single(ch(1))));
        assert_eq!(parse("Title v0001.cbz").volume, Some(single(ch(1))));
        assert_eq!(parse("Title V01.cbz").volume, Some(single(ch(1))));
    }

    #[test]
    fn volume_multi_token_vol_dot_space() {
        assert_eq!(parse("Title Vol. 1.cbz").volume, Some(single(ch(1))));
        assert_eq!(parse("Title Vol.1.cbz").volume, Some(single(ch(1))));
        assert_eq!(parse("Title Vol 1.cbz").volume, Some(single(ch(1))));
        assert_eq!(parse("Title Volume 11.cbz").volume, Some(single(ch(11))));
    }

    #[test]
    fn volume_decimal() {
        assert_eq!(parse("Title v1.1.cbz").volume, Some(single(ch_dec(1, 1))));
        assert_eq!(parse("Title v03.5.cbz").volume, Some(single(ch_dec(3, 5))));
    }

    #[test]
    fn volume_range() {
        assert_eq!(
            parse("Title v16-17.cbz").volume,
            Some(range(ch(16), ch(17)))
        );
        assert_eq!(parse("Title v01-03.cbz").volume, Some(range(ch(1), ch(3))));
    }

    #[test]
    fn volume_first_wins_on_duplicate() {
        // Per Kavita's ParseDuplicateVolumeTest: first marker wins.
        let p = parse("One Piece - Vol 2 Ch 1.1 - Volume 4 Omakes");
        assert_eq!(p.volume, Some(single(ch(2))));
    }

    #[test]
    fn volume_absent_returns_none() {
        assert_eq!(parse("Just a title.cbz").volume, None);
    }

    // ----- chapter -----

    #[test]
    fn chapter_single_token_c001() {
        assert_eq!(parse("Title c001.cbz").chapter, Some(single(ch(1))));
        assert_eq!(parse("Title C42.cbz").chapter, Some(single(ch(42))));
    }

    #[test]
    fn chapter_multi_token_ch_chapter() {
        assert_eq!(parse("Title Ch.4.cbz").chapter, Some(single(ch(4))));
        assert_eq!(parse("Title Ch. 4.cbz").chapter, Some(single(ch(4))));
        assert_eq!(parse("Title Chapter 12.cbz").chapter, Some(single(ch(12))));
    }

    #[test]
    fn chapter_decimal() {
        assert_eq!(
            parse("Title c42.5.cbz").chapter,
            Some(single(ch_dec(42, 5)))
        );
    }

    #[test]
    fn chapter_range() {
        assert_eq!(
            parse("Title c001-008.cbz").chapter,
            Some(range(ch(1), ch(8)))
        );
    }

    // ----- group -----

    #[test]
    fn group_leading_bracket() {
        assert_eq!(parse("[Yen Press] Title v01.epub").group, Some("Yen Press"));
    }

    #[test]
    fn group_trailing_bracket() {
        assert_eq!(parse("Title v01 [LuCaZ].cbz").group, Some("LuCaZ"));
    }

    #[test]
    fn group_skips_volume_in_brackets() {
        // [Volume 11] is not a group, it's a volume marker. Per v0 scope we
        // don't extract the volume from it, but we do correctly ignore it as group.
        let p = parse("Tonikaku Cawaii [Volume 11].cbz");
        assert_eq!(p.group, None);
    }

    // ----- source -----

    #[test]
    fn source_digital() {
        assert_eq!(
            parse("Title v01 (Digital).cbz").source,
            Some(MangaSource::Digital)
        );
        assert_eq!(
            parse("Title v01 (Digital-HD).cbz").source,
            Some(MangaSource::Digital)
        );
    }

    #[test]
    fn source_mangaplus() {
        assert_eq!(
            parse("Title v01 (MangaPlus).cbz").source,
            Some(MangaSource::MangaPlus)
        );
    }

    #[test]
    fn source_absent_when_no_known_tag() {
        assert_eq!(parse("Title v01.cbz").source, None);
    }

    // ----- end-to-end -----

    #[test]
    fn parse_canonical_kavita_example() {
        let p = parse("BTOOOM! v01 (2013) (Digital) (Shadowcat-Empire)");
        assert_eq!(p.volume, Some(single(ch(1))));
        assert_eq!(p.source, Some(MangaSource::Digital));
        assert_eq!(p.extension, None); // no .cbz/etc on this fixture
    }

    #[test]
    fn parse_full_grammar_run() {
        let p = parse("[Yen Press] Sword Art Online v10 c042.5 (Digital).epub");
        assert_eq!(p.group, Some("Yen Press"));
        assert_eq!(p.volume, Some(single(ch(10))));
        assert_eq!(p.chapter, Some(single(ch_dec(42, 5))));
        assert_eq!(p.source, Some(MangaSource::Digital));
        assert_eq!(p.extension, None); // .epub is LN-side
    }

    // ----- v1 additions -----

    #[test]
    fn volume_in_parens() {
        // (v01) form — the volume marker is wrapped in parens, not at top level.
        let p = parse("Gokukoku no Brynhildr - c001-008 (v01) [TrinityBAKumA]");
        assert_eq!(p.volume, Some(single(ch(1))));
    }

    #[test]
    fn volume_in_brackets() {
        // [Volume 11] — volume marker wrapped in brackets.
        assert_eq!(
            parse("Tonikaku Cawaii [Volume 11].cbz").volume,
            Some(single(ch(11)))
        );
    }

    #[test]
    fn volume_season_prefix_s01() {
        // Tower-of-God-style S01 = season 1 = volume 1.
        let p = parse("Tower Of God S01 014.cbz");
        assert_eq!(p.volume, Some(single(ch(1))));
    }

    #[test]
    fn chapter_revision_suffix_chapter_n_v_n() {
        // `Chapter11v2` = chapter 11 (revision 2). Revision is silently
        // consumed — extracting it into ParsedManga.revision is v2+ scope.
        assert_eq!(
            parse("Yumekui-Merry_DKThias_Chapter11v2.zip").chapter,
            Some(single(ch(11)))
        );
        assert_eq!(parse("Title c042v2.zip").chapter, Some(single(ch(42))));
    }

    #[test]
    fn chapter_chp_prefix() {
        // `vol01_chp02` — `chp` is a third common chapter prefix.
        assert_eq!(
            parse("[Hidoi]_Amaenaideyo_MS_vol01_chp02.rar").chapter,
            Some(single(ch(2)))
        );
    }

    #[test]
    fn chapter_bare_number_after_title() {
        // No `c`/`Ch`/`Chapter` prefix — just an isolated number after title words.
        assert_eq!(
            parse("Hinowa ga CRUSH! 018 (2019) (Digital).cbz").chapter,
            Some(single(ch(18)))
        );
        assert_eq!(
            parse("Beelzebub_01_[Noodles].zip").chapter,
            Some(single(ch(1)))
        );
    }

    #[test]
    fn chapter_bare_number_skips_year() {
        // 1900-2099 are excluded; otherwise `Title (2019)` would give chapter 2019.
        // Year inside parens is already invisible (we don't look in parens for
        // bare numbers), but a year at top-level shouldn't trip us either.
        assert_eq!(parse("Title 2019 Edition").chapter, None);
    }

    #[test]
    fn chapter_bare_number_skips_leading_numeric_title() {
        // First Word can't be the chapter — it's the title's first word.
        assert_eq!(parse("100 Years Of Solitude").chapter, None);
    }

    #[test]
    fn chapter_bare_number_skips_after_vol_keyword() {
        // The `5` after `Vol` belongs to the volume marker, not the chapter.
        // Combined-token forms like `v05 042` should NOT block the 042 though
        // (the 5 was already absorbed into v05).
        let p = parse("Series Vol 5 - 042");
        assert_eq!(p.volume, Some(single(ch(5))));
        assert_eq!(p.chapter, Some(single(ch(42))));
    }

    #[test]
    fn range_rejects_backward_end() {
        // Mangapy `vol_356-1` syntax — the `-1` isn't a range end (1 < 356).
        // Should parse as just 356.
        assert_eq!(parse("vol_356-1").volume, Some(single(ch(356))));
    }

    // ----- corpus -----

    /// Run `parse()` against every Kavita fixture and report per-method pass
    /// rates. Asserts a minimum aggregate rate so regressions surface, and
    /// prints first-N failures so it's easy to see what's not working.
    ///
    /// As the parser improves, raise `MIN_AGGREGATE_PASS_RATE`.
    #[test]
    fn corpus_kavita_pass_rate() {
        const CORPUS: &str = include_str!("../corpus/manga_kavita.json");
        // After v1 (nested-paren, bare-number, revision-suffix, range
        // validation, `chp`/`s` prefixes) we hit ~74%. Threshold is set just
        // below current measured rate so a real regression surfaces here,
        // not a few-pp drop from edge-case work. Raise as the parser improves.
        const MIN_AGGREGATE_PASS_RATE: f64 = 0.70;

        let entries: Vec<serde_json::Value> = serde_json::from_str(CORPUS).unwrap();

        #[derive(Default)]
        struct Bucket {
            pass: usize,
            total: usize,
            failures: Vec<(String, String, String)>, // (input, expected, actual)
        }
        let mut by_method: std::collections::BTreeMap<String, Bucket> = Default::default();

        for entry in &entries {
            let source = entry["source"].as_str().unwrap_or("");
            let method = source.rsplit("::").next().unwrap_or("").to_string();
            let input = entry["input"].as_str().unwrap_or("");

            let (expected_field, actual_str): (Option<&str>, Option<String>) = match method.as_str()
            {
                "ParseVolumeTest" | "ParseDuplicateVolumeTest" => (
                    entry.get("expected_volume").and_then(|v| v.as_str()),
                    parse(input).volume.as_ref().map(format_range),
                ),
                "ParseChaptersTest"
                | "ParseDuplicateChapterTest"
                | "ParseExtraNumberChaptersTest" => (
                    entry.get("expected_chapter").and_then(|v| v.as_str()),
                    parse(input).chapter.as_ref().map(format_range),
                ),
                _ => continue, // series, edition not yet implemented
            };

            // Skip entries with null expectations (Kavita's LooseLeafVolume sentinel)
            // until v0 scope grows to model "no value" assertions.
            let Some(expected) = expected_field else {
                continue;
            };

            let bucket = by_method.entry(method).or_default();
            bucket.total += 1;

            let actual_str = actual_str.unwrap_or_else(|| "None".to_string());
            if actual_str == expected {
                bucket.pass += 1;
            } else if bucket.failures.len() < 5 {
                bucket
                    .failures
                    .push((input.to_string(), expected.to_string(), actual_str));
            }
        }

        let mut total_pass = 0usize;
        let mut total_count = 0usize;
        eprintln!("\n--- corpus_kavita_pass_rate ---");
        for (method, bucket) in &by_method {
            total_pass += bucket.pass;
            total_count += bucket.total;
            let pct = if bucket.total > 0 {
                bucket.pass as f64 / bucket.total as f64 * 100.0
            } else {
                0.0
            };
            eprintln!(
                "{:<35}  {:>4}/{:<4}  {:>5.1}%",
                method, bucket.pass, bucket.total, pct
            );
            for (input, expected, actual) in &bucket.failures {
                let truncated: String = input.chars().take(70).collect();
                eprintln!("    FAIL: {truncated}  expected={expected}  got={actual}");
            }
        }
        let aggregate = total_pass as f64 / total_count.max(1) as f64;
        eprintln!(
            "aggregate: {total_pass}/{total_count} = {:.1}%",
            aggregate * 100.0
        );

        assert!(
            aggregate >= MIN_AGGREGATE_PASS_RATE,
            "aggregate pass rate {:.1}% dropped below floor {:.0}% — regression?",
            aggregate * 100.0,
            MIN_AGGREGATE_PASS_RATE * 100.0
        );
    }

    fn format_range(r: &NumberRange) -> String {
        match r.end {
            None => format_chapter_number(r.start),
            Some(end) => format!(
                "{}-{}",
                format_chapter_number(r.start),
                format_chapter_number(end)
            ),
        }
    }

    fn format_chapter_number(n: ChapterNumber) -> String {
        match n.decimal {
            None => n.whole.to_string(),
            Some(d) => format!("{}.{}", n.whole, d),
        }
    }
}
