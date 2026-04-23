//! Manga / manhwa / manhua filename parser.
//!
//! Manhwa/manhua live here too — their filename grammar overlaps closely with manga.
//! Source-tag differences (Lezhin / Naver / Kakao for manhwa) are carried by
//! [`MangaSource`], not by a separate module.
//!
//! **In scope**: extension, volume (single-token `v01`, multi-token `Vol 1`,
//! decimal `v03.5`, range `v01-09`, also `(v01)` and `[Volume 11]` nested
//! forms), chapter (same prefixed patterns under `c`/`Ch`/`Chp`/`Chapter`
//! plus revision-suffix `Chapter11v2` and a bare-number fallback for
//! `Beelzebub_01_[Noodles].zip`-style filenames), group (first non-volume
//! bracketed token), source (`Digital`, `MangaPlus`), title (slice from
//! after the leading group bracket up to the first marker, trailing
//! bracket, or extension dot; underscores normalized to spaces). Range
//! validation rejects backward ranges (`vol_356-1` parses as 356, not 356-1).
//!
//! Single-letter prefixes `s` (Tower of God's `S01`) and `t` (French `t6`,
//! Batman `T2000`) are recognized — only `<letter>\d+` matches, so titles
//! like "Sword" or "Tower" don't collide.
//!
//! **CJK markers**: `巻` (Japanese), `卷` / `册` (Chinese), `권` / `장`
//! (Korean) as postfix volume markers within a single Word. The Chinese
//! `第N卷`/`第N册` prefix-and-suffix combo and the rare prefix-only `卷N`
//! also work. Decimals (`7.5권`) and ranges (`1-3巻`) are reconstructed
//! from preceding tokens. CJK chapter markers `话` / `話` / `章` / `回`
//! follow the same machinery.
//!
//! **Multi-token Cyrillic keywords**: `Том` / `Тома` (volume), `Глава` /
//! `Главы` (chapter). Russian translations of CJK manga occasionally land
//! on Nyaa.
//!
//! **Out of scope** (intentional, documented gaps): title extraction,
//! language tags, revision *extraction* (the suffix is consumed but not
//! stored), oneshot detection, Korean multi-char prefix `시즌`, Thai
//! `เล่ม` (Ryokan's source — Nyaa English-translated lit category — doesn't
//! carry Thai), alpha-suffix decimal chapters (`Beelzebub_153b` = 153.5),
//! `c001-006x1`-style chapter ranges with extra suffixes, and the rest of
//! `MangaSource` (Viz / Kodansha / Lezhin / Naver / Kakao). These come back
//! as the corpus pass-rate test pushes them up the priority list.

use std::borrow::Cow;

use crate::lexer::{Token, tokenize};
use crate::{ChapterNumber, Language, NumberRange};

/// Structured fields parsed from a single manga filename.
///
/// String fields borrow from the input (`'a`) where possible. The `title`
/// field is a `Cow` because Kavita-style normalization replaces `_` with ` `
/// (`B_Gata_H_Kei` → `"B Gata H Kei"`) — we borrow the original slice when
/// no replacement is needed, and only allocate when it is.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ParsedManga<'a> {
    pub title: Option<Cow<'a, str>>,
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
        title: detect_title(filename, &tokens),
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
    // Pass 3: CJK postfix markers (1巻, 第03卷, 卷5, 卷2第25话, 1-3巻, 7.5권).
    detect_cjk_marker_seq(tokens, CJK_VOLUME_MARKERS)
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

        // Multi-token form: Vol / Volume / Tome / Том / Тома + delim(s) + number
        if is_volume_keyword(w)
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
    // Pass 3: CJK postfix markers (第25话, 第N章, N回).
    if let Some(r) = detect_cjk_marker_seq(tokens, CJK_CHAPTER_MARKERS) {
        return Some(r);
    }
    // Pass 4: bare-number fallback for `Beelzebub_01_[Noodles].zip`,
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

        // Multi-token form: Ch / Chapter / Глава + delim(s) + number
        if is_chapter_keyword(w)
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
                // Only the multi-token `Vol`/`Volume`/`Том`/`เล่ม` keyword
                // blocks the next number; combined-token forms like `v05`
                // already absorbed their number into the same Word.
                prev_was_vol_keyword = is_volume_keyword(w);
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

/// Find the title slice within `filename`, returning the cleaned form.
///
/// Algorithm:
///   1. `title_end` = byte position of the first marker token, the first
///      non-leading bracket/paren, or (if none) the extension dot.
///   2. `title_start` = position right after the leading group bracket if
///      one is present at the start; otherwise 0.
///   3. Slice `filename[title_start..title_end]`, trim surrounding junk
///      characters, replace `_` with ` `.
///
/// Returns `None` when the resulting slice is empty.
fn detect_title<'a>(filename: &'a str, tokens: &[Token<'a>]) -> Option<Cow<'a, str>> {
    let title_end = find_title_end(filename, tokens);
    let title_start = find_title_start(filename, tokens);
    if title_start >= title_end {
        return None;
    }
    let raw = filename.get(title_start..title_end)?;
    let cleaned = clean_title(raw);
    if cleaned.is_empty() {
        None
    } else {
        Some(cleaned)
    }
}

fn find_title_end(filename: &str, tokens: &[Token<'_>]) -> usize {
    // Skip the FIRST bracket if it appears before any Word token — that's the
    // leading group, not a title-end signal. Subsequent brackets/parens DO
    // signal end-of-title (trailing groups, year tags, source tags, etc.).
    let mut leading_bracket_skipped = false;
    let mut seen_word = false;
    // For the bare-number chapter heuristic (mirroring detect_bare_number_chapter):
    let mut saw_non_numeric_word = false;
    let mut prev_was_vol_keyword = false;

    for token in tokens {
        match token {
            Token::Word(w) => {
                seen_word = true;
                let all_digits = w.chars().all(|c| c.is_ascii_digit());

                // Explicit marker (vol/chapter prefix or keyword, or CJK postfix).
                let is_marker = strip_volume_prefix(w).is_some()
                    || is_volume_keyword(w)
                    || parse_chapter_num_from_word(w).is_some()
                    || is_chapter_keyword(w)
                    || find_cjk_marker_number_in_word(w, CJK_VOLUME_MARKERS).is_some()
                    || find_cjk_marker_number_in_word(w, CJK_CHAPTER_MARKERS).is_some();
                if is_marker && let Some(pos) = token_position_in(filename, token) {
                    return pos;
                }

                // Bare-number chapter (`APOSIMZ 017`, `Beelzebub_172_RHS`,
                // `Dr. STONE 136`). Same fence as `detect_bare_number_chapter`
                // — must follow at least one non-numeric Word, not after a
                // Vol keyword, not year-shaped.
                if all_digits
                    && saw_non_numeric_word
                    && !prev_was_vol_keyword
                    && let Ok(n) = w.parse::<u32>()
                    && !looks_like_year(n)
                    && let Some(pos) = token_position_in(filename, token)
                {
                    return pos;
                }

                if !all_digits {
                    saw_non_numeric_word = true;
                }
                prev_was_vol_keyword = is_volume_keyword(w);
            }
            Token::Bracketed(_) | Token::Parenthesized(_) | Token::Curly(_) => {
                if !seen_word && !leading_bracket_skipped {
                    leading_bracket_skipped = true;
                    continue;
                }
                if let Some(pos) = token_position_in(filename, token) {
                    return pos;
                }
            }
            Token::Delimiter(_) => {}
        }
    }

    // No marker, no trailing bracket — fall back to the extension dot if
    // the trailing `.<ext>` is a known manga extension.
    if let Some(dot) = filename.rfind('.') {
        let ext = &filename[dot + 1..];
        if !ext.is_empty()
            && ext.len() <= 4
            && MANGA_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str())
        {
            return dot;
        }
    }
    filename.len()
}

fn find_title_start(filename: &str, tokens: &[Token<'_>]) -> usize {
    for token in tokens {
        match token {
            Token::Delimiter(_) => continue,
            Token::Bracketed(content) => {
                // `[Volume 11]` is a marker, not a group; title starts at 0.
                if contains_volume_keyword(content) {
                    return 0;
                }
                // Otherwise: leading bracket is a group; title starts after `]`.
                let open_pos = token_position_in(filename, token).unwrap_or(0);
                return open_pos + 2 + content.len(); // [ + content + ]
            }
            _ => return 0,
        }
    }
    0
}

fn clean_title(s: &str) -> Cow<'_, str> {
    // Trim leading/trailing punctuation that isn't part of a real title.
    // Notable inclusions: `_` (delimiter substitute), `,` (e.g. `Corpse Party
    // Musume,`), `#` (e.g. `Kodoja #001` slices to `Kodoja #`).
    let trimmed =
        s.trim_matches(|c: char| c.is_whitespace() || matches!(c, '-' | '.' | '_' | ',' | '#'));
    if trimmed.contains('_') {
        Cow::Owned(trimmed.replace('_', " ").trim().to_owned())
    } else {
        Cow::Borrowed(trimmed)
    }
}

/// Recover a token's byte position within `filename` via pointer arithmetic.
///
/// Tokens carry `&str` slices into `filename`; we reverse `slice.as_ptr() -
/// filename.as_ptr()` to find the position. For bracketed variants the
/// stored slice excludes the brackets themselves, so subtract one byte for
/// the opener (`[`/`(`/`{` are all single-byte ASCII).
///
/// Returns `None` for `Delimiter` (no slice to anchor on) and for tokens
/// that didn't originate from `filename` (e.g. produced by a recursive
/// `tokenize` of a sub-slice from a different allocation — not a path
/// chaptr's parser actually takes today, but the guard is cheap).
fn token_position_in(filename: &str, token: &Token<'_>) -> Option<usize> {
    let (slice, opener_offset) = match token {
        Token::Word(s) => (*s, 0),
        Token::Bracketed(s) | Token::Parenthesized(s) | Token::Curly(s) => (*s, 1),
        Token::Delimiter(_) => return None,
    };
    let base = filename.as_ptr() as usize;
    let s_ptr = slice.as_ptr() as usize;
    let pos = s_ptr.checked_sub(base)?;
    if pos > filename.len() {
        return None;
    }
    pos.checked_sub(opener_offset)
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
    // Tower of God; `t` covers French tome abbreviation (`t6` → vol 6) and
    // the Batman/Batgirl `T2000`-style numbering. Both single-letter
    // prefixes only match `<letter>\d+`, so titles like "Sword", "Spy",
    // "Tower", "Title", "Test" all strip cleanly to non-digit suffixes
    // and don't match.
    for prefix in ["volume", "vol", "v", "s", "t"] {
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

/// Multi-token volume-marker keywords (the marker is its own Word, separated
/// by delimiters from the following number). ASCII forms use case-insensitive
/// match via `eq_ignore_ascii_case`; Cyrillic forms need `to_lowercase()` for
/// case folding outside the ASCII range.
///
/// French `tome` is needed in multi-token form for `Conquistador_Tome_2` and
/// `Max_l_explorateur-_Tome_0`. The single-letter `t` prefix in
/// [`strip_volume_prefix`] separately covers combined-token forms like `t6`
/// and `T2000`. Thai (`เล่ม`) is omitted because Ryokan's intended upstream
/// is Nyaa's English-translated literature category, which doesn't see
/// Thai-script manga.
const VOL_MULTI_TOKEN_KEYWORDS: &[&str] = &[
    "vol", "volume", "tome", // French
    "том", "тома", // Russian (nominative + genitive)
];

const CH_MULTI_TOKEN_KEYWORDS: &[&str] = &[
    "ch",
    "chp",
    "chapter",
    "chapters",
    "episode", // Umineko / Noblesse use Episode-style chapter numbering
    "глава",
    "главы", // Russian
];

/// CJK postfix markers for volume — appear in single Words like `1巻` or
/// `第03卷`. Unicode-letter chars; the lexer keeps them attached to adjacent
/// digits within a single `Word` token.
///
/// `장` (Korean: chapter) is included because Kavita's fixtures store
/// `13장` as a *volume*, not a chapter — matching that behavior preserves
/// fixture parity even where the linguistic mapping is debatable.
const CJK_VOLUME_MARKERS: &[char] = &['巻', '卷', '册', '권', '장'];

const CJK_CHAPTER_MARKERS: &[char] = &['话', '話', '章', '回'];

fn is_volume_keyword(word: &str) -> bool {
    keyword_match(word, VOL_MULTI_TOKEN_KEYWORDS)
}

fn is_chapter_keyword(word: &str) -> bool {
    keyword_match(word, CH_MULTI_TOKEN_KEYWORDS)
}

fn keyword_match(word: &str, keywords: &[&str]) -> bool {
    // Fast path: ASCII case-insensitive match avoids the to_lowercase
    // allocation for the common case (English keywords on ASCII filenames).
    if word.is_ascii() {
        return keywords.iter().any(|k| word.eq_ignore_ascii_case(k));
    }
    let lower = word.to_lowercase();
    keywords.iter().any(|k| lower == *k)
}

/// Scan `tokens` for any Word containing a CJK marker char from `markers`,
/// extract the number, and combine with preceding `<digits>-` (range) or
/// `<digits>.` (decimal) tokens if present. First match wins.
fn detect_cjk_marker_seq(tokens: &[Token], markers: &[char]) -> Option<NumberRange> {
    for (i, t) in tokens.iter().enumerate() {
        let Token::Word(w) = t else { continue };
        let Some(found_n) = find_cjk_marker_number_in_word(w, markers) else {
            continue;
        };

        // Range: preceding tokens are [Word(digits), Delim('-')]
        if let Some(start_n) = preceding_range_start(tokens, i)
            && start_n <= found_n
        {
            return Some(NumberRange::range(
                ChapterNumber::new(start_n),
                ChapterNumber::new(found_n),
            ));
        }

        // Decimal: preceding tokens are [Word(digits), Delim('.')]. The
        // `found_n` is the fractional part — combine into ChapterNumber.
        if let Some(combined) = preceding_decimal_combined(tokens, i, found_n) {
            return Some(NumberRange::single(combined));
        }

        return Some(NumberRange::single(ChapterNumber::new(found_n)));
    }
    None
}

/// Find a marker char in `word` and return the digits adjacent to it.
/// Tries postfix-style first (digits before marker), then prefix-style
/// (digits after). Skips markers with no adjacent digits.
fn find_cjk_marker_number_in_word(word: &str, markers: &[char]) -> Option<u32> {
    let chars: Vec<char> = word.chars().collect();
    for (i, c) in chars.iter().enumerate() {
        if !markers.contains(c) {
            continue;
        }
        // Postfix-style: <digits><marker>, e.g. 1巻 or 第03卷
        let mut start = i;
        while start > 0 && chars[start - 1].is_ascii_digit() {
            start -= 1;
        }
        if start < i {
            let s: String = chars[start..i].iter().collect();
            if let Ok(n) = s.parse::<u32>() {
                return Some(n);
            }
        }
        // Prefix-style: <marker><digits>, e.g. 卷5
        let mut end = i + 1;
        while end < chars.len() && chars[end].is_ascii_digit() {
            end += 1;
        }
        if end > i + 1 {
            let s: String = chars[i + 1..end].iter().collect();
            if let Ok(n) = s.parse::<u32>() {
                return Some(n);
            }
        }
    }
    None
}

fn preceding_range_start(tokens: &[Token], i: usize) -> Option<u32> {
    if i < 2 {
        return None;
    }
    let Token::Delimiter('-') = tokens.get(i - 1)? else {
        return None;
    };
    let Token::Word(s) = tokens.get(i - 2)? else {
        return None;
    };
    s.parse::<u32>().ok()
}

fn preceding_decimal_combined(
    tokens: &[Token],
    i: usize,
    fractional: u32,
) -> Option<ChapterNumber> {
    if i < 2 {
        return None;
    }
    let Token::Delimiter('.') = tokens.get(i - 1)? else {
        return None;
    };
    let Token::Word(s) = tokens.get(i - 2)? else {
        return None;
    };
    let whole = s.parse::<u32>().ok()?;
    if fractional > u32::from(u16::MAX) {
        return None;
    }
    #[expect(clippy::cast_possible_truncation, reason = "bounded by check above")]
    Some(ChapterNumber::with_decimal(whole, fractional as u16))
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

    // ----- CJK markers (v2) -----

    #[test]
    fn volume_cjk_postfix_japanese() {
        // 1巻 → vol 1 (Japanese)
        let p = parse("スライム倒して300年 1巻");
        assert_eq!(p.volume, Some(single(ch(1))));
    }

    #[test]
    fn volume_cjk_postfix_chinese_prefix_combo() {
        // 第03卷 → vol 3 (Chinese: "the 3rd volume")
        let p = parse("幽游白书完全版 第03卷 天下");
        assert_eq!(p.volume, Some(single(ch(3))));
    }

    #[test]
    fn volume_cjk_postfix_chinese_ce() {
        // 第1册 → vol 1 (Chinese alt marker 册)
        assert_eq!(parse("阿衰online 第1册").volume, Some(single(ch(1))));
    }

    #[test]
    fn volume_cjk_prefix_only() {
        // 卷5 → vol 5 (less common — marker before digits)
        assert_eq!(parse("卷5 Test").volume, Some(single(ch(5))));
    }

    #[test]
    fn volume_cjk_compound_with_chapter() {
        // 卷2第25话 → vol 2 (volume marker takes precedence over chapter
        // marker because volume detection runs first)
        let p = parse("【TFO汉化】迷你偶像漫画卷2第25话");
        assert_eq!(p.volume, Some(single(ch(2))));
    }

    #[test]
    fn volume_cjk_range() {
        // 1-3巻 → range(1, 3) — preceding-tokens lookup picks up the start
        let p = parse("スライム倒して300年 1-3巻");
        assert_eq!(p.volume, Some(range(ch(1), ch(3))));
    }

    #[test]
    fn volume_cjk_decimal_combined() {
        // 7.5권 → 7.5 (preceding `7.` combines with marker's adjacent `5`)
        let p = parse("몰루 아카이브 7.5권");
        assert_eq!(p.volume, Some(single(ch_dec(7, 5))));
    }

    #[test]
    fn volume_korean_jang() {
        // 13장 → vol 13 (Kavita treats 장 as volume even though linguistically
        // it means chapter — fixture parity).
        assert_eq!(parse("동의보감 13장").volume, Some(single(ch(13))));
    }

    #[test]
    fn volume_russian_tom_keyword() {
        // Том 1 → vol 1 (Russian "tom" = volume, multi-token form)
        let p = parse("Kebab Том 1 Глава 3");
        assert_eq!(p.volume, Some(single(ch(1))));
    }

    #[test]
    fn volume_russian_tom_range() {
        // Том 1-4 → range(1, 4)
        let p = parse("Манга Том 1-4");
        assert_eq!(p.volume, Some(range(ch(1), ch(4))));
    }

    #[test]
    fn volume_french_tome_multi_token() {
        // Tome_2 → vol 2 (French "tome" = volume, in multi-token form;
        // single-letter `t` covers the combined `t6` form separately)
        assert_eq!(parse("Conquistador_Tome_2").volume, Some(single(ch(2))));
    }

    #[test]
    fn volume_t_prefix_combined_token() {
        // t6 / T2000 → single-token `t<digits>` (French abbreviation, Batman)
        assert_eq!(
            parse("Daredevil - t6 - 10 - (2019)").volume,
            Some(single(ch(6)))
        );
        assert_eq!(parse("Batgirl T2000 #57").volume, Some(single(ch(2000))));
    }

    #[test]
    fn chapter_russian_glava_keyword() {
        // Глава 3 → chapter 3 (Russian "glava" = chapter)
        let p = parse("Kebab Том 1 Глава 3");
        assert_eq!(p.chapter, Some(single(ch(3))));
    }

    // ----- title (v3) -----

    fn title_str<'a>(p: &'a ParsedManga<'a>) -> Option<&'a str> {
        p.title.as_deref()
    }

    #[test]
    fn title_simple_extracts_before_volume_marker() {
        let p = parse("Killing Bites Vol. 0001 Ch. 0001 - Galactica Scanlations (gb)");
        assert_eq!(title_str(&p), Some("Killing Bites"));
    }

    #[test]
    fn title_underscores_replaced_with_spaces() {
        // Allocates: Cow::Owned because of the replacement.
        let p = parse("B_Gata_H_Kei_v01[SlowManga&OverloadScans]");
        assert_eq!(title_str(&p), Some("B Gata H Kei"));
        assert!(matches!(p.title, Some(std::borrow::Cow::Owned(_))));
    }

    #[test]
    fn title_borrows_when_no_underscore() {
        // No replacement needed → Cow::Borrowed, zero allocation.
        let p = parse("BTOOOM! v01 (2013) (Digital) (Shadowcat-Empire)");
        assert_eq!(title_str(&p), Some("BTOOOM!"));
        assert!(matches!(p.title, Some(std::borrow::Cow::Borrowed(_))));
    }

    #[test]
    fn title_skips_leading_group_bracket() {
        let p = parse("[xPearse] Kyochuu Rettou Volume 1 [English] [Manga] [Volume Scans]");
        assert_eq!(title_str(&p), Some("Kyochuu Rettou"));
    }

    #[test]
    fn title_stops_at_trailing_bracket_metadata() {
        let p = parse("Tonikaku Cawaii [Volume 11].cbz");
        assert_eq!(title_str(&p), Some("Tonikaku Cawaii"));
    }

    #[test]
    fn title_falls_back_to_extension_dot_when_no_marker() {
        let p = parse("100 Years Of Solitude.cbz");
        // No marker, no trailing bracket, .cbz is a known extension.
        assert_eq!(title_str(&p), Some("100 Years Of Solitude"));
    }

    #[test]
    fn title_bare_number_chapter_stops_title_walk() {
        // `017` is a bare-number chapter — title should stop there, not
        // continue to the extension.
        let p = parse("APOSIMZ 017 (2018) (Digital) (danke-Empire).cbz");
        assert_eq!(title_str(&p), Some("APOSIMZ"));
    }

    #[test]
    fn title_trims_trailing_comma() {
        // `Kedouin Makoto - Corpse Party Musume, Chapter 19` — title shouldn't
        // include the trailing comma after `Musume`.
        let p = parse("Kedouin Makoto - Corpse Party Musume, Chapter 19");
        assert_eq!(title_str(&p), Some("Kedouin Makoto - Corpse Party Musume"));
    }

    #[test]
    fn title_trims_trailing_hash() {
        // `Kodoja #001 (March 2016)` — title slices to `Kodoja #`, hash should trim off.
        let p = parse("Kodoja #001 (March 2016)");
        assert_eq!(title_str(&p), Some("Kodoja"));
    }

    #[test]
    fn title_returns_none_when_only_marker() {
        // `v001` has no title before the marker.
        let p = parse("v001");
        assert_eq!(p.title, None);
    }

    #[test]
    fn title_handles_cjk() {
        // CJK title with postfix vol marker — title slice should preserve
        // the CJK characters intact.
        let p = parse("スライム倒して300年 1巻");
        assert_eq!(title_str(&p), Some("スライム倒して300年"));
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
        // After v3 (title detection + bare-number stop + chp/chapters/episode
        // keywords + #/comma trim) we hit ~88.6% on 315 entries (added the
        // 129-entry ParseSeriesTest method). Threshold set below current rate
        // so a real regression surfaces here, not a few-pp drop from
        // edge-case work. Raise as the parser improves.
        const MIN_AGGREGATE_PASS_RATE: f64 = 0.85;

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
                "ParseSeriesTest" => (
                    entry.get("expected_series").and_then(|v| v.as_str()),
                    parse(input).title.as_deref().map(str::to_owned),
                ),
                _ => continue, // edition not yet implemented
            };

            // Skip entries with null expectations (Kavita's LooseLeafVolume sentinel)
            // until v0 scope grows to model "no value" assertions.
            let Some(expected) = expected_field else {
                continue;
            };

            let bucket = by_method.entry(method).or_default();
            bucket.total += 1;

            // Empty expected (e.g. ParseSeriesTest expects "" for `v001`)
            // should match a `None` parse result; using unwrap_or_default
            // maps `None` → `""` so the comparison succeeds.
            let actual_str = actual_str.unwrap_or_default();
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
