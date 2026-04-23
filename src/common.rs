//! Shared detector helpers for the manga and novel parsers.
//!
//! Split out so that changes to generic parse logic (CJK markers, range
//! validation, title slicing, keyword tables) affect both domains
//! identically — the #24-lesson instinct applied at the classifier layer,
//! not just the lexer. Domain-specific wiring (`MangaSource` enum,
//! `MANGA_EXTENSIONS` vs `NOVEL_EXTENSIONS`, `ln_publishers` table) stays in
//! `manga.rs` / `novel.rs`; the items here are parameterized when they need
//! to know anything caller-specific (e.g. the known-extensions list for the
//! title-end fallback).
//!
//! Visibility is `pub(crate)` throughout — nothing here is part of chaptr's
//! public API.

use std::borrow::Cow;

use crate::lexer::{Token, tokenize};
use crate::{ChapterNumber, NumberRange};

// ---------- keyword and marker tables ----------

/// Multi-token volume-marker keywords — the marker is its own Word, separated
/// by delimiters from the following number. ASCII forms use case-insensitive
/// match via `eq_ignore_ascii_case`; Cyrillic forms need `to_lowercase()` for
/// case folding outside the ASCII range.
///
/// French `tome` is needed in multi-token form for `Conquistador_Tome_2` and
/// `Max_l_explorateur-_Tome_0`. The single-letter `t` prefix in
/// [`strip_volume_prefix`] separately covers combined-token forms like `t6`
/// and `T2000`. Thai (`เล่ม`) is omitted because Ryokan's intended upstream
/// is Nyaa's English-translated category, which doesn't see Thai-script
/// releases.
pub(crate) const VOL_MULTI_TOKEN_KEYWORDS: &[&str] = &[
    "vol", "volume", "tome", // French
    "том", "тома", // Russian (nominative + genitive)
];

pub(crate) const CH_MULTI_TOKEN_KEYWORDS: &[&str] = &[
    "ch",
    "chp",
    "chapter",
    "chapters",
    "episode", // Umineko / Noblesse use Episode-style chapter numbering
    "глава",
    "главы", // Russian
];

/// CJK postfix markers for volume — appear inside single Words like `1巻` or
/// `第03卷`. Unicode-letter chars; the lexer keeps them attached to adjacent
/// digits within a single `Word` token.
///
/// `장` (Korean: chapter) is included because Kavita's fixtures store
/// `13장` as a *volume*, not a chapter — matching that behavior preserves
/// fixture parity even where the linguistic mapping is debatable.
pub(crate) const CJK_VOLUME_MARKERS: &[char] = &['巻', '卷', '册', '권', '장'];

pub(crate) const CJK_CHAPTER_MARKERS: &[char] = &['话', '話', '章', '回'];

// ---------- extension detection ----------

/// Match the last `.<ext>` against a domain-supplied list.
pub(crate) fn detect_extension<'a>(filename: &'a str, known: &[&str]) -> Option<&'a str> {
    let dot_pos = filename.rfind('.')?;
    let ext = &filename[dot_pos + 1..];
    // Bound the length — `Vol.0001 (Digital)` would otherwise match `0001 (Digital)` as ext.
    if ext.len() > 4 || ext.is_empty() {
        return None;
    }
    let lower = ext.to_ascii_lowercase();
    if known.contains(&lower.as_str()) {
        Some(ext)
    } else {
        None
    }
}

// ---------- volume / chapter detection ----------

pub(crate) fn detect_volume(tokens: &[Token]) -> Option<NumberRange> {
    // Pass 1: top-level.
    if let Some(r) = detect_volume_in_seq(tokens) {
        return Some(r);
    }
    // Pass 2: inside parens / brackets — `(v01)` / `[Volume 11]`.
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
    // Pass 3: CJK postfix markers (1巻, 第03卷, 卷5, 1-3巻, 7.5권).
    detect_cjk_marker_seq(tokens, CJK_VOLUME_MARKERS)
}

pub(crate) fn detect_chapter(tokens: &[Token]) -> Option<NumberRange> {
    if let Some(r) = detect_chapter_in_seq(tokens) {
        return Some(r);
    }
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
    if let Some(r) = detect_cjk_marker_seq(tokens, CJK_CHAPTER_MARKERS) {
        return Some(r);
    }
    detect_bare_number_chapter(tokens)
}

fn detect_volume_in_seq(tokens: &[Token]) -> Option<NumberRange> {
    for (i, t) in tokens.iter().enumerate() {
        let Token::Word(w) = t else { continue };

        if let Some(num_str) = strip_volume_prefix(w)
            && let Ok(start_whole) = num_str.parse::<u32>()
        {
            return Some(build_range(tokens, i + 1, start_whole));
        }

        if is_volume_keyword(w)
            && let Some(range) = parse_number_after_marker(tokens, i + 1)
        {
            return Some(range);
        }
    }
    None
}

fn detect_chapter_in_seq(tokens: &[Token]) -> Option<NumberRange> {
    for (i, t) in tokens.iter().enumerate() {
        let Token::Word(w) = t else { continue };

        if let Some(start_whole) = parse_chapter_num_from_word(w) {
            return Some(build_range(tokens, i + 1, start_whole));
        }

        if is_chapter_keyword(w)
            && let Some(range) = parse_number_after_marker(tokens, i + 1)
        {
            return Some(range);
        }
    }
    None
}

/// Find an isolated all-digit Word that's plausibly a chapter number.
///
/// Heuristics, all required:
/// - Must come *after* at least one non-numeric Word (skips `100 Years Of Solitude`)
/// - Must not be year-shaped (1900-2099) — those are almost never chapters
/// - Must not be immediately preceded by `Vol`/`Volume` keyword
/// - Combined-token forms like `v05` do *not* block a following bare number
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
                prev_was_vol_keyword = is_volume_keyword(w);
            }
            Token::Delimiter(_) => {}
            _ => prev_was_vol_keyword = false,
        }
    }
    None
}

pub(crate) fn looks_like_year(n: u32) -> bool {
    (1900..=2099).contains(&n)
}

// ---------- number builders ----------

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
    // Reject backward ranges (Mangapy `vol_356-1` syntax).
    if end < start {
        return None;
    }
    Some(end)
}

// ---------- CJK marker scanning ----------

/// Scan `tokens` for any Word containing a marker char from `markers`.
/// Combines with preceding `<digits>-` (range) or `<digits>.` (decimal)
/// tokens if present. First match wins.
fn detect_cjk_marker_seq(tokens: &[Token], markers: &[char]) -> Option<NumberRange> {
    for (i, t) in tokens.iter().enumerate() {
        let Token::Word(w) = t else { continue };
        let Some(found_n) = find_cjk_marker_number_in_word(w, markers) else {
            continue;
        };

        if let Some(start_n) = preceding_range_start(tokens, i)
            && start_n <= found_n
        {
            return Some(NumberRange::range(
                ChapterNumber::new(start_n),
                ChapterNumber::new(found_n),
            ));
        }

        if let Some(combined) = preceding_decimal_combined(tokens, i, found_n) {
            return Some(NumberRange::single(combined));
        }

        return Some(NumberRange::single(ChapterNumber::new(found_n)));
    }
    None
}

/// Find a marker char in `word` and return the digits adjacent to it.
/// Tries postfix-style first (digits before marker), then prefix-style
/// (digits after). Public at crate level because the title-end scanner
/// also uses it to recognize CJK-marked Words.
pub(crate) fn find_cjk_marker_number_in_word(word: &str, markers: &[char]) -> Option<u32> {
    let chars: Vec<char> = word.chars().collect();
    for (i, c) in chars.iter().enumerate() {
        if !markers.contains(c) {
            continue;
        }
        // Postfix-style: <digits><marker>
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
        // Prefix-style: <marker><digits>
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

// ---------- prefix / keyword matching ----------

/// Strip a volume prefix (`v`/`vol`/`volume`/`s`/`t`) from `word` and return
/// the digit tail if the rest is all ASCII digits.
pub(crate) fn strip_volume_prefix(word: &str) -> Option<&str> {
    // Order matters: longer prefixes first so "volume" beats "vol" beats "v".
    // `s` covers season-style markers (`S01` → vol 1) for Tower of God;
    // `t` covers French tome abbreviation (`t6`) and Batman `T2000`.
    // Both single-letter prefixes only match `<letter>\d+`, so titles like
    // "Sword", "Spy", "Tower", "Title", "Test" strip to non-digit tails.
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

/// Parse a single Word as a chapter number with optional revision suffix.
///
/// Pattern: `<prefix><digits>[v<digits>]` where prefix is
/// `chapter`/`chp`/`ch`/`c` (case-insensitive). Examples: `c042`, `Ch11v2`,
/// `Chapter51v2`, `Chp02`.
///
/// The revision component is silently consumed but not returned — revision
/// extraction lives in the domain modules if they want it.
pub(crate) fn parse_chapter_num_from_word(word: &str) -> Option<u32> {
    // `chp` tried before `ch` so it matches "chp02" as prefix "chp" + digits "02"
    // rather than "ch" + "p02" (which would fail the all-digit check anyway,
    // but the shorter-prefix-first form is clearer).
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
        if let Some(rev_part) = strip_prefix_ignore_ascii_case(after, "v")
            && !rev_part.is_empty()
            && rev_part.chars().all(|c| c.is_ascii_digit())
        {
            return Some(num);
        }
    }
    None
}

pub(crate) fn is_volume_keyword(word: &str) -> bool {
    keyword_match(word, VOL_MULTI_TOKEN_KEYWORDS)
}

pub(crate) fn is_chapter_keyword(word: &str) -> bool {
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

fn strip_prefix_ignore_ascii_case<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    // `str::split_at` panics on a non-char-boundary index (CJK chars are
    // multi-byte; a single-letter prefix lands inside them). Use bytes for
    // the comparison and `str::get` for the safe boundary check.
    let head_bytes = s.as_bytes().get(..prefix.len())?;
    if head_bytes.eq_ignore_ascii_case(prefix.as_bytes()) {
        s.get(prefix.len()..)
    } else {
        None
    }
}

pub(crate) fn contains_volume_keyword(s: &str) -> bool {
    let lower = s.to_ascii_lowercase();
    // Conservative — full word match would be more correct but bracket
    // contents are typically short (group names) and the false-positive risk
    // (a group named "Volunteer Scans") is rare.
    lower.contains("volume") || lower.starts_with("vol ") || lower.starts_with("vol.")
}

// ---------- title detection ----------

/// Find the title slice within `filename`, returning the cleaned form.
///
/// Algorithm:
///   1. `title_end` = byte position of the first marker token, the first
///      non-leading bracket/paren, or (if none) the extension dot.
///   2. `title_start` = position right after the leading group bracket if
///      one is present at the start; otherwise 0.
///   3. Slice `filename[title_start..title_end]`, trim surrounding junk,
///      replace `_` with ` ` (Cow::Owned only when needed).
pub(crate) fn detect_title<'a>(
    filename: &'a str,
    tokens: &[Token<'a>],
    known_extensions: &[&str],
) -> Option<Cow<'a, str>> {
    let title_end = find_title_end(filename, tokens, known_extensions);
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

fn find_title_end(filename: &str, tokens: &[Token<'_>], known_extensions: &[&str]) -> usize {
    // Skip the FIRST bracket if it appears before any Word token — that's the
    // leading group, not a title-end signal. Subsequent brackets/parens DO
    // signal end-of-title (trailing groups, year tags, source tags, etc.).
    let mut leading_bracket_skipped = false;
    let mut seen_word = false;
    let mut saw_non_numeric_word = false;
    let mut prev_was_vol_keyword = false;

    for token in tokens {
        match token {
            Token::Word(w) => {
                seen_word = true;
                let all_digits = w.chars().all(|c| c.is_ascii_digit());

                let is_marker = strip_volume_prefix(w).is_some()
                    || is_volume_keyword(w)
                    || parse_chapter_num_from_word(w).is_some()
                    || is_chapter_keyword(w)
                    || find_cjk_marker_number_in_word(w, CJK_VOLUME_MARKERS).is_some()
                    || find_cjk_marker_number_in_word(w, CJK_CHAPTER_MARKERS).is_some();
                if is_marker && let Some(pos) = token_position_in(filename, token) {
                    return pos;
                }

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

    if let Some(dot) = filename.rfind('.') {
        let ext = &filename[dot + 1..];
        if !ext.is_empty()
            && ext.len() <= 4
            && known_extensions.contains(&ext.to_ascii_lowercase().as_str())
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
pub(crate) fn token_position_in(filename: &str, token: &Token<'_>) -> Option<usize> {
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
