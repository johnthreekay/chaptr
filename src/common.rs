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
//! Visibility is mostly `pub(crate)` — the three generic detectors
//! (`detect_volume`, `detect_chapter`, `detect_chapter_revision`) are
//! `pub` and re-exported from the crate root for consumers who want to
//! build their own L2 classifier on top of the shared lexer. Everything
//! else (helpers, keyword tables, title-slicing internals) stays
//! crate-private.

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

/// Multi-char CJK volume *prefixes* — positioned before the digits rather
/// than after, e.g. Korean `시즌3` ("season 3"). Separate from the
/// single-char postfix markers because the detection algorithm is
/// different (prefix strip + leading-digits vs scan-for-marker + adjacent-
/// digits).
pub(crate) const CJK_VOLUME_PREFIXES: &[&str] = &[
    "시즌", // Korean: season
];

/// `회` and `화` are Korean chapter markers — 회 literally means "round"
/// or "chapter", 화 means "talk/chapter". Both appear postfix-style as in
/// `13회` (chapter 13) and `106화` (chapter 106).
pub(crate) const CJK_CHAPTER_MARKERS: &[char] = &['话', '話', '章', '回', '회', '화'];

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

pub fn detect_volume(tokens: &[Token]) -> Option<NumberRange> {
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

pub fn detect_chapter(tokens: &[Token]) -> Option<NumberRange> {
    // `#N` at end-of-filename outranks keyword-based detection when no
    // CJK chapter marker is present. Kavita treats `Episode 3 ... #02`
    // as chapter 2 — the hash-number carries the chapter, `Episode N` is
    // a title-side section indicator. Guarded against CJK markers so
    // `13회#2` still resolves via the 회 marker (chapter 13, not 2).
    if let Some(r) = detect_hash_chapter(tokens) {
        return Some(r);
    }
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

/// `#N` chapter detection, restricted to the trailing slot of the filename.
///
/// Matches `<Delim('#')> <Word(digits)>` followed only by delimiters and a
/// possible extension Word (`.cbz`, `.7z`). Earlier `#N` occurrences
/// (`Monster #8 Ch. 001`) don't match because non-delimiter non-extension
/// tokens follow them.
///
/// Returns `None` if any CJK chapter marker is present — `13회#2`
/// resolves to 13 via CJK, not 2 via hash.
fn detect_hash_chapter(tokens: &[Token]) -> Option<NumberRange> {
    // Fast path: the overwhelming majority of filenames have no `#`, so
    // scan once for the delimiter before paying the CJK-guard cost.
    if !tokens.iter().any(|t| matches!(t, Token::Delimiter('#'))) {
        return None;
    }

    for t in tokens {
        if let Token::Word(w) = t
            && find_cjk_marker_number_in_word(w, CJK_CHAPTER_MARKERS).is_some()
        {
            return None;
        }
    }

    for i in 0..tokens.len() {
        let Token::Delimiter('#') = tokens[i] else {
            continue;
        };
        let Some(Token::Word(num_str)) = tokens.get(i + 1) else {
            continue;
        };
        let Ok(n) = num_str.parse::<u32>() else {
            continue;
        };

        let at_end = tokens[i + 2..].iter().all(|t| match t {
            Token::Delimiter(_) => true,
            Token::Word(w) => looks_like_extension(w),
            _ => false,
        });
        if at_end {
            return Some(NumberRange::single(ChapterNumber::new(n)));
        }
    }
    None
}

fn detect_volume_in_seq(tokens: &[Token]) -> Option<NumberRange> {
    for (i, t) in tokens.iter().enumerate() {
        let Token::Word(w) = t else { continue };

        if let Some(num_str) = strip_volume_prefix(w)
            && let Ok(start_whole) = num_str.parse::<u32>()
        {
            return Some(build_range(tokens, i + 1, start_whole, false));
        }

        // CJK multi-char prefix: `시즌3`, `시즌34삽화2`. Leading digits
        // after the prefix are the volume number; any trailing non-digit
        // tail is ignored (what the corpus expects for 시즌34삽화2 — vol
        // 34, trailing 삽화2 is noise).
        if let Some(start_whole) = strip_cjk_prefix_volume(w) {
            return Some(build_range(tokens, i + 1, start_whole, false));
        }

        if is_volume_keyword(w) {
            if let Some(range) = parse_number_after_marker(tokens, i + 1, false) {
                return Some(range);
            }
            // Postfix-style: `5 Том Test` (Russian). Only runs when the
            // forward scan failed, and only for the Russian keyword —
            // English `Vol` with no following number is overwhelmingly a
            // title fragment, not a postfix marker.
            if is_postfix_volume_keyword(w)
                && let Some(range) = parse_number_before_marker(tokens, i)
            {
                return Some(range);
            }
        }
    }
    None
}

/// Russian `том`/`тома` is the only volume keyword the corpus uses in
/// postfix position (`5 Том Test` → vol 5). Gating postfix detection to
/// this narrow set avoids picking up trailing title digits for English
/// fixtures that put `Vol` in a title (`Accel World: Vol.1` style).
fn is_postfix_volume_keyword(word: &str) -> bool {
    if word.is_ascii() {
        return false;
    }
    let lower = word.to_lowercase();
    matches!(lower.as_str(), "том" | "тома")
}

fn parse_number_before_marker(tokens: &[Token], marker_idx: usize) -> Option<NumberRange> {
    // Symmetric to [`parse_number_after_marker`] but walking leftward.
    // Same 3-delimiter cap to reject `5 ... ... Том` runs where the
    // number isn't attached to the keyword.
    const MAX_DELIMS: usize = 3;
    if marker_idx == 0 {
        return None;
    }
    let mut i = marker_idx;
    let mut delims_skipped = 0usize;
    while i > 0 {
        i -= 1;
        match &tokens[i] {
            Token::Delimiter(_) => {
                delims_skipped += 1;
                if delims_skipped > MAX_DELIMS {
                    return None;
                }
            }
            Token::Word(num_str) => {
                let n = num_str.parse::<u32>().ok()?;
                if looks_like_year(n) {
                    return None;
                }
                return Some(NumberRange::single(ChapterNumber::new(n)));
            }
            _ => return None,
        }
    }
    None
}

fn strip_cjk_prefix_volume(word: &str) -> Option<u32> {
    for prefix in CJK_VOLUME_PREFIXES {
        let Some(rest) = word.strip_prefix(prefix) else {
            continue;
        };
        let digits_end = rest.bytes().take_while(u8::is_ascii_digit).count();
        if digits_end == 0 {
            continue;
        }
        if let Ok(n) = rest[..digits_end].parse::<u32>() {
            return Some(n);
        }
    }
    None
}

fn detect_chapter_in_seq(tokens: &[Token]) -> Option<NumberRange> {
    for (i, t) in tokens.iter().enumerate() {
        let Token::Word(w) = t else { continue };

        if let Some(start_whole) = parse_chapter_num_from_word(w) {
            return Some(build_range(tokens, i + 1, start_whole, true));
        }

        if is_chapter_keyword(w)
            && let Some(range) = parse_number_after_marker(tokens, i + 1, true)
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
/// - **Must be followed by "metadata" — a bracket/paren/curly, an
///   all-uppercase group-code Word (`RHS`, `MT`, `SCX`), or end-of-tokens.**
///   Without this check `Kaiju No. 8 036` would pick `8` as chapter (it's
///   part of the title); with it, `8` is skipped because it's followed by
///   another digit Word, and `036` is picked because it's followed by
///   `(2021)`.
pub(crate) fn detect_bare_number_chapter(tokens: &[Token]) -> Option<NumberRange> {
    // If an explicit VOLUME marker exists later in the token stream,
    // bare-number candidates BEFORE it are part of the title, not the
    // chapter. Zom 100 - Tome 2 / Kaiju No. 8 / The 100 Girlfriends family:
    // numbers embedded in series titles shouldn't become chapter numbers
    // just because they happen to be followed by a volume keyword.
    //
    // Chapter markers (Ch, Chapter, Глава, 话) do *not* gate here.
    // Russian postfix syntax `Манга 2 Глава` means "chapter 2" — the `2`
    // is the chapter value. Chapter-keyword positioning carries no
    // title-vs-chapter ambiguity (a postfix chapter keyword confirms the
    // preceding number IS the chapter), so we don't gate against it.
    let first_vol_marker = find_first_volume_marker(tokens);
    let min_index = first_vol_marker.map_or(0, |p| p + 1);

    let mut saw_non_numeric_word = false;
    let mut prev_was_vol_keyword = false;

    for (i, t) in tokens.iter().enumerate() {
        match t {
            Token::Word(w) => {
                let all_digits = w.chars().all(|c| c.is_ascii_digit());

                if i >= min_index
                    && all_digits
                    && saw_non_numeric_word
                    && !prev_was_vol_keyword
                    && let Ok(n) = w.parse::<u32>()
                    && !looks_like_year(n)
                    && bare_number_followed_by_metadata(tokens, i + 1)
                {
                    return Some(build_range(tokens, i + 1, n, false));
                }

                // Alpha-suffix case: `153b` → 153.5 (Kavita convention).
                if i >= min_index
                    && !all_digits
                    && saw_non_numeric_word
                    && !prev_was_vol_keyword
                    && let Some(n) = parse_extended_number(w)
                    && n.decimal.is_some()
                    && !looks_like_year(n.whole)
                    && bare_number_followed_by_metadata(tokens, i + 1)
                {
                    return Some(NumberRange::single(n));
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

/// Position of the first Word token that's an explicit VOLUME-kind marker
/// (volume keyword, vol prefix like `v01`/`S01`/`sp02`/`t6`, CJK vol
/// prefix like `시즌3`, CJK vol postfix like `1巻`). Used to gate
/// bare-number chapter candidates: volume markers mean "there's
/// structured metadata"; numbers before them in the title are title
/// content, not chapter declarations.
fn find_first_volume_marker(tokens: &[Token]) -> Option<usize> {
    tokens.iter().position(|t| match t {
        Token::Word(w) => {
            is_volume_keyword(w)
                || strip_volume_prefix(w).is_some()
                || strip_cjk_prefix_volume(w).is_some()
                || find_cjk_marker_number_in_word(w, CJK_VOLUME_MARKERS).is_some()
        }
        _ => false,
    })
}

/// Position of the first Word token that's an explicit structured marker
/// of any kind (volume or chapter). Used by title detection to disable
/// bare-number title stops: if any marker is coming up, the walk should
/// reach it naturally rather than getting cut short by an incidental
/// number earlier in the filename.
fn find_first_structured_marker(tokens: &[Token]) -> Option<usize> {
    tokens.iter().position(|t| match t {
        Token::Word(w) => {
            is_volume_keyword(w)
                || strip_volume_prefix(w).is_some()
                || strip_cjk_prefix_volume(w).is_some()
                || find_cjk_marker_number_in_word(w, CJK_VOLUME_MARKERS).is_some()
                || is_chapter_keyword(w)
                || parse_chapter_num_from_word(w).is_some()
                || find_cjk_marker_number_in_word(w, CJK_CHAPTER_MARKERS).is_some()
        }
        _ => false,
    })
}

/// True when the bare number at `start_idx - 1` is "followed by something
/// chapter-y" rather than "followed by more title text":
/// - Immediately followed by `.<digits>` or `-<digits>` — decimal / range
///   extension of this number (`017.5`, `001-003`). Counts as "attached".
/// - First non-delimiter after is a bracket/paren/curly (`(2021)`, `[MD]`).
/// - First non-delimiter Word after is an all-uppercase group-code (`RHS`,
///   `MT`) or a volume/chapter keyword in a postfix position (`Глава`,
///   `Vol`, `Ch`).
/// - End of tokens.
///
/// Otherwise the bare number is presumed part of the title (`Kaiju No. 8`,
/// `The 100 Girlfriends`).
pub(crate) fn bare_number_followed_by_metadata(tokens: &[Token], start_idx: usize) -> bool {
    // Fast paths for "number is attached to something chapter-y" cases:
    //   `N.<digits>`         decimal extension (`017.5`)
    //   `N-<digits>`         range extension (`001-003`)
    //   `N-<digits><alpha>`  range with Kavita alpha-suffix (`150-153b`)
    //   `N.<ext>`            filename extension (`29.rar`, `01.jpg`)
    if let Some(Token::Delimiter(delim)) = tokens.get(start_idx)
        && let Some(Token::Word(next_w)) = tokens.get(start_idx + 1)
        && !next_w.is_empty()
    {
        match delim {
            '.' if (next_w.chars().all(|ch| ch.is_ascii_digit())
                || looks_like_extension(next_w)) =>
            {
                return true;
            }
            '-' if parse_extended_number(next_w).is_some() => {
                return true;
            }
            _ => {}
        }
    }
    for t in &tokens[start_idx.min(tokens.len())..] {
        match t {
            Token::Delimiter(_) => continue,
            Token::Bracketed(_) | Token::Parenthesized(_) | Token::Curly(_) => return true,
            Token::Word(w) => {
                return is_group_code_word(w) || is_chapter_keyword(w) || is_volume_keyword(w);
            }
        }
    }
    true
}

/// A Word like `rar` / `cbz` / `jpg` / `epub` — 2-5 ASCII alphanumeric chars
/// with at least one letter. Used *only* in the `.<ext>` fast-path of
/// [`bare_number_followed_by_metadata`]; a raw Word of this shape is far
/// too permissive to count as metadata on its own ("the" and "of" would
/// match), so the extension check is gated on a preceding `.` delimiter.
fn looks_like_extension(w: &str) -> bool {
    (2..=5).contains(&w.len())
        && w.bytes().all(|b| b.is_ascii_alphanumeric())
        && w.bytes().any(|b| b.is_ascii_alphabetic())
}

/// A Word like `RHS` / `MT` / `SCX` — all-uppercase ASCII letters (plus
/// optional digits) that typically indicate a scanlator group attribution
/// rather than more title text. Rejects lowercase words like `Girlfriends`,
/// all-digit words like `036` (no letters = not a code), and empty strings.
fn is_group_code_word(w: &str) -> bool {
    !w.is_empty()
        && w.chars().any(|c| c.is_ascii_alphabetic())
        && w.chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
}

pub(crate) fn looks_like_year(n: u32) -> bool {
    (1900..=2099).contains(&n)
}

/// Parse a Word as a `ChapterNumber`, tolerating non-digit suffixes:
///
/// - Bare digits (`"153"`) → `ChapterNumber::new(153)`
/// - Digits + a single ASCII lowercase letter (`"153b"`) →
///   `ChapterNumber::with_decimal(153, 5)` (Kavita convention: any single
///   alpha suffix = .5, per `Beelzebub_153b_RHS.zip` → expected 153.5)
/// - Digits + any other non-digit suffix (`"4삽화2"`, `"4th"`) →
///   `ChapterNumber::new(4)` — just the leading digits, rest is noise
///
/// Returns `None` for words that don't start with digits (`"v05"`, `"Title"`).
pub(crate) fn parse_extended_number(word: &str) -> Option<ChapterNumber> {
    let digits_end = word.bytes().take_while(u8::is_ascii_digit).count();
    if digits_end == 0 {
        return None;
    }
    let whole = word.get(..digits_end)?.parse::<u32>().ok()?;
    let suffix = word.get(digits_end..)?;
    if suffix.is_empty() {
        return Some(ChapterNumber::new(whole));
    }
    // Single lowercase ASCII letter → Kavita's "extra" convention → .5
    if suffix.len() == 1 && suffix.as_bytes()[0].is_ascii_lowercase() {
        return Some(ChapterNumber::with_decimal(whole, 5));
    }
    // Any other suffix → just the leading digits
    Some(ChapterNumber::new(whole))
}

// ---------- number builders ----------

/// Build a [`NumberRange`] starting at `start_whole`, then optionally consume
/// a `.<dec>` decimal suffix and an `-<num>[.<dec>]` range end from the
/// following tokens.
///
/// `allow_chapter_prefix_end` = `true` lets `parse_range_end` accept a
/// chapter-prefixed end word (`c01-c04` → 1-4). Chapter callers pass
/// `true`; volume callers pass `false` so `v01-c001` is not misread as
/// a volume range into a chapter word.
fn build_range(
    tokens: &[Token],
    cursor: usize,
    start_whole: u32,
    allow_chapter_prefix_end: bool,
) -> NumberRange {
    let mut start = ChapterNumber::new(start_whole);
    let mut next = cursor;

    if let Some((dec, after)) = parse_decimal_suffix(tokens, next) {
        start = ChapterNumber::with_decimal(start_whole, dec);
        next = after;
    }

    let end = parse_range_end(tokens, next, start, allow_chapter_prefix_end);
    NumberRange { start, end }
}

/// Skip up to N delimiters, then read a number-shaped Word. Used for
/// `Vol 1` / `Vol. 1` / `Volume_01` / `Chapter 12` patterns.
fn parse_number_after_marker(
    tokens: &[Token],
    start: usize,
    allow_chapter_prefix_end: bool,
) -> Option<NumberRange> {
    // Cap at 3 delimiters between the marker keyword and its number. The
    // common forms are:
    //   `Vol 1`    — 1 delim (space)
    //   `Vol. 1`   — 2 delims (dot + space)
    //   `Vol . 1`  — 3 delims (space + dot + space)
    // Anything longer is almost certainly not the keyword's number — it's
    // the keyword being part of the title with a separate number later.
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
                return Some(build_range(
                    tokens,
                    i + 1,
                    start_whole,
                    allow_chapter_prefix_end,
                ));
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

fn parse_range_end(
    tokens: &[Token],
    pos: usize,
    start: ChapterNumber,
    allow_chapter_prefix_end: bool,
) -> Option<ChapterNumber> {
    if !matches!(tokens.get(pos), Some(Token::Delimiter('-'))) {
        return None;
    }
    let Some(Token::Word(num_str)) = tokens.get(pos + 1) else {
        return None;
    };
    // `parse_extended_number` handles bare digits, alpha-suffix (.5),
    // and any-other-suffix (leading digits only) in one shot. If the
    // caller is a chapter detector and the plain parse fails, also
    // accept a chapter-prefixed end word (`c01-c04`, `ch01-ch04`). This
    // is gated off for volume callers to avoid `v01-c001[MD]` matching
    // the chapter word as a volume range end.
    let mut end = parse_extended_number(num_str).or_else(|| {
        if allow_chapter_prefix_end {
            parse_chapter_num_from_word(num_str).map(ChapterNumber::new)
        } else {
            None
        }
    })?;
    // If the end was bare digits with no alpha suffix, allow a trailing
    // `.<dec>` decimal from the following tokens (rare: `c001-008.5`).
    if end.decimal.is_none()
        && let Some((dec, _)) = parse_decimal_suffix(tokens, pos + 2)
    {
        end = ChapterNumber::with_decimal(end.whole, dec);
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

        if let Some(start_n) = preceding_range_start(tokens, i) {
            if start_n <= found_n {
                return Some(NumberRange::range(
                    ChapterNumber::new(start_n),
                    ChapterNumber::new(found_n),
                ));
            }
            // Backward range: `38-1화` reads as "chapter 38, part 1" (not
            // a range from 38 down to 1). Kavita treats the larger
            // leading number as the chapter; the trailing sub-part is
            // noise from our perspective.
            return Some(NumberRange::single(ChapterNumber::new(start_n)));
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
///
/// Zero-alloc implementation (1.2.0): uses `char_indices()` + byte-level
/// ASCII digit scan + `str::get` slicing, avoiding the `Vec<char>` +
/// `String::from_iter` allocations the 1.0/1.1 version paid per call.
/// ASCII digit bytes (0x30-0x39) are always single-byte in UTF-8 and
/// can't appear inside a multi-byte CJK char, so byte-walking backward
/// through digit bytes is safe and stays on char boundaries.
pub(crate) fn find_cjk_marker_number_in_word(word: &str, markers: &[char]) -> Option<u32> {
    let bytes = word.as_bytes();
    for (marker_start, c) in word.char_indices() {
        if !markers.contains(&c) {
            continue;
        }
        let marker_end = marker_start + c.len_utf8();

        // Postfix-style: digits immediately before the marker.
        let mut digit_start = marker_start;
        while digit_start > 0 && bytes[digit_start - 1].is_ascii_digit() {
            digit_start -= 1;
        }
        if digit_start < marker_start
            && let Some(digits) = word.get(digit_start..marker_start)
            && let Ok(n) = digits.parse::<u32>()
        {
            return Some(n);
        }

        // Prefix-style: digits immediately after the marker.
        let mut digit_end = marker_end;
        while digit_end < bytes.len() && bytes[digit_end].is_ascii_digit() {
            digit_end += 1;
        }
        if digit_end > marker_end
            && let Some(digits) = word.get(marker_end..digit_end)
            && let Ok(n) = digits.parse::<u32>()
        {
            return Some(n);
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
    // `sp` covers `SP02`-style special-chapter markers (`Grand Blue ... SP02
    // Extra`) where the content is semantically a special rather than a true
    // volume — but treating it as volume-like is enough to stop title walks
    // at the marker. Must come *before* `s` in the list so `SP02` matches as
    // `sp` + `02` (two-letter prefix) rather than `s` + `P02` (which would
    // fail the all-digits check on the tail and leave `SP02` unrecognized).
    for prefix in ["volume", "vol", "v", "sp", "s", "t"] {
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
/// The revision component is silently consumed here — [`detect_chapter_revision`]
/// extracts it separately. Two entry points let a caller populate both
/// `chapter: NumberRange` and `revision: u8` without a single shared state
/// machine, at the cost of walking the tokens twice (the tokens list is small
/// and we'd rather duplicate than introduce a two-valued return).
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

/// Scan `tokens` for a chapter-with-revision word (`c042v2`, `Chapter11v2`,
/// `Ch11v2`, `Chp02v3`) and return the revision number (`2`, `3`, …).
///
/// Companion to [`parse_chapter_num_from_word`]: that function consumes the
/// revision suffix but only returns the chapter number; this one pulls out
/// just the revision so a domain parser can populate `ParsedManga.revision`.
pub fn detect_chapter_revision(tokens: &[Token]) -> Option<u8> {
    for t in tokens {
        let Token::Word(w) = t else { continue };
        if let Some(rev) = chapter_revision_from_word(w) {
            return Some(rev);
        }
    }
    None
}

fn chapter_revision_from_word(word: &str) -> Option<u8> {
    for prefix in ["chapter", "chp", "ch", "c"] {
        let Some(rest) = strip_prefix_ignore_ascii_case(word, prefix) else {
            continue;
        };
        let digits_end = rest.bytes().take_while(u8::is_ascii_digit).count();
        if digits_end == 0 {
            continue;
        }
        let after = &rest[digits_end..];
        if let Some(rev_part) = strip_prefix_ignore_ascii_case(after, "v")
            && !rev_part.is_empty()
            && rev_part.chars().all(|c| c.is_ascii_digit())
            && let Ok(rev) = rev_part.parse::<u8>()
        {
            return Some(rev);
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
    // Skip the LEADING CHAIN of brackets/parens/curlies — e.g. for
    // `(一般コミック) [奥浩哉] いぬやしき 第09巻` both the paren and the
    // bracket are metadata, and the title runs from after both. Once we've
    // seen a Word, any subsequent bracket/paren/curly signals end-of-title.
    let mut seen_word = false;
    let mut saw_non_numeric_word = false;
    let mut prev_was_vol_keyword = false;

    // If any explicit marker exists later, the walk will naturally stop
    // there — so disable bare-number stops, which would otherwise cut the
    // title too early (Zom 100 - Tome 2 → "Zom" instead of "Zom 100";
    // Monster #8 Ch. 001 → "Monster" instead of "Monster #8").
    let has_later_marker = find_first_structured_marker(tokens).is_some();

    for (token_index, token) in tokens.iter().enumerate() {
        match token {
            Token::Word(w) => {
                seen_word = true;
                let all_digits = w.chars().all(|c| c.is_ascii_digit());

                let is_marker = strip_volume_prefix(w).is_some()
                    || strip_cjk_prefix_volume(w).is_some()
                    || is_volume_keyword(w)
                    || parse_chapter_num_from_word(w).is_some()
                    || is_chapter_keyword(w)
                    || find_cjk_marker_number_in_word(w, CJK_VOLUME_MARKERS).is_some()
                    || find_cjk_marker_number_in_word(w, CJK_CHAPTER_MARKERS).is_some();
                if is_marker && let Some(pos) = token_position_in(filename, token) {
                    return pos;
                }

                if !has_later_marker
                    && all_digits
                    && saw_non_numeric_word
                    && !prev_was_vol_keyword
                    && let Ok(n) = w.parse::<u32>()
                    && !looks_like_year(n)
                    && bare_number_followed_by_metadata(tokens, token_index + 1)
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
                if !seen_word {
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
    // Skip a chain of leading brackets/parens/curlies — e.g. for
    // `(一般コミック) [奥浩哉] いぬやしき 第09巻` both the paren and the
    // bracket are metadata and the title starts after them.
    let mut cursor = 0;
    for token in tokens {
        match token {
            Token::Delimiter(_) => continue,
            Token::Bracketed(content) => {
                // `[Volume 11]` is a marker, not a group; title starts at 0.
                if contains_volume_keyword(content) {
                    return 0;
                }
                let open_pos = token_position_in(filename, token).unwrap_or(0);
                cursor = open_pos + 2 + content.len();
            }
            Token::Parenthesized(content) | Token::Curly(content) => {
                let open_pos = token_position_in(filename, token).unwrap_or(0);
                cursor = open_pos + 2 + content.len();
            }
            _ => return cursor,
        }
    }
    cursor
}

fn clean_title(s: &str) -> Cow<'_, str> {
    // Trim leading/trailing punctuation that isn't part of a real title.
    // Notable inclusions: `_` (delimiter substitute), `,` (e.g. `Corpse Party
    // Musume,`), `#` (e.g. `Kodoja #001` slices to `Kodoja #`), `:` (e.g.
    // `Accel World:` before a `Vol` tag).
    //
    // `.` is deliberately NOT trimmed: Kavita fixtures keep series titles with
    // a trailing period intact (`Hentai Ouji to Warawanai Neko.`). The
    // extension dot is stripped earlier in [`find_title_end`], so a dot that
    // reaches here is title content.
    let trimmed =
        s.trim_matches(|c: char| c.is_whitespace() || matches!(c, '-' | '_' | ',' | '#' | ':'));
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
