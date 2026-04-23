//! Light novel filename parser.
//!
//! Web novels are explicitly out of scope: WN content is HTML scraped into
//! EPUBs by Ryokan-controlled scrapers in v2.1, which means the consumer
//! controls the output filename and there's nothing external to parse.
//!
//! Generic parse machinery (tokenize, volume detection, CJK markers, title
//! slicing) lives in [`crate::common`]; this module wires it with
//! LN-specific tables (`ln_publishers`, `ln_scanners`) and tags
//! (`(Digital)`, `(Premium)`, `{r2}` revision).
//!
//! **In scope (v0)**: extension (epub/pdf/azw3/mobi/txt), volume (same
//! machinery as manga), publisher (lookup against
//! [`crate::tables::ln_publishers`]), scanner (lookup against
//! [`crate::tables::ln_scanners`]), group (first leading bracket that ISN'T
//! a publisher or scanner — covers Nyaa upload-group prefixes like
//! `[Unpaid Ferryman]`), `is_digital` / `is_premium` parenthesized tags,
//! year (first paren whose leading digits are year-shaped), title (shared
//! title-slicing helper), revision (from `{r2}`-style curly tags).
//!
//! **Out of scope**: year ranges (`(2022-2024)` → `Some(2022)` only —
//! single u16 field for now), chapter numbers (LN filenames essentially
//! never carry these), alt-title separator handling (`|`/`/` between
//! Japanese and English titles is preserved as-is in the raw title slice
//! — consumers can split if needed).

use std::borrow::Cow;

use crate::common;
use crate::lexer::{Token, tokenize};
use crate::tables;
use crate::{Language, NumberRange};

/// Structured fields parsed from a single light-novel filename.
///
/// `#[non_exhaustive]` so additions to the LN model (year ranges,
/// language, a structured source-type field) can land in a minor version
/// instead of forcing a semver-major bump.
#[non_exhaustive]
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ParsedNovel<'a> {
    pub title: Option<Cow<'a, str>>,
    pub volume: Option<NumberRange>,
    /// Release/upload group — leading bracket like `[Unpaid Ferryman]` that
    /// isn't a known publisher or scanner. Distinct from [`Self::publisher`]
    /// and [`Self::scanner`].
    pub group: Option<&'a str>,
    /// Publishing house (Yen Press, J-Novel Club, Seven Seas, ...).
    pub publisher: Option<&'a str>,
    /// Scan / release credit (LuCaZ, Stick, ...).
    pub scanner: Option<&'a str>,
    pub language: Option<Language>,
    /// Year of release. Year *ranges* (`(2022-2024)`) currently capture
    /// only the first year — full-range support is a v1+ extension.
    pub year: Option<u16>,
    /// `(Premium)` tag — J-Novel Club premium-tier release.
    pub is_premium: bool,
    /// `(Digital)` tag.
    pub is_digital: bool,
    /// Revision marker from a `{r\d+}` curly-bracket tag.
    pub revision: Option<u8>,
    /// Extension without leading dot (`epub`, `pdf`, `azw3`, `mobi`, `txt`).
    pub extension: Option<&'a str>,
}

/// Parse a light-novel filename into structured fields.
#[must_use]
pub fn parse(filename: &str) -> ParsedNovel<'_> {
    let tokens = tokenize(filename);
    ParsedNovel {
        title: common::detect_title(filename, &tokens, NOVEL_EXTENSIONS),
        volume: common::detect_volume(&tokens),
        group: detect_group(&tokens),
        publisher: detect_publisher(&tokens),
        scanner: detect_scanner(&tokens),
        language: common::detect_language(&tokens),
        year: detect_year(&tokens),
        is_premium: detect_tag(&tokens, "premium"),
        is_digital: detect_tag(&tokens, "digital"),
        revision: detect_revision(&tokens),
        extension: detect_extension(filename),
    }
}

const NOVEL_EXTENSIONS: &[&str] = &["epub", "pdf", "azw3", "mobi", "txt"];

fn detect_extension(filename: &str) -> Option<&str> {
    common::detect_extension(filename, NOVEL_EXTENSIONS)
}

/// The first leading bracketed Word that isn't a publisher, scanner, or
/// volume-keyword bracket. Walking stops at the first non-delimiter /
/// non-bracket token because, by convention, the release-group tag is
/// always at the start.
fn detect_group<'a>(tokens: &[Token<'a>]) -> Option<&'a str> {
    for token in tokens {
        match token {
            Token::Delimiter(_) => continue,
            Token::Bracketed(content) => {
                if content.is_empty() {
                    continue;
                }
                if tables::ln_publishers::lookup(content).is_some()
                    || tables::ln_scanners::lookup(content).is_some()
                    || common::contains_volume_keyword(content)
                {
                    return None;
                }
                return Some(*content);
            }
            _ => return None,
        }
    }
    None
}

fn detect_publisher(tokens: &[Token]) -> Option<&'static str> {
    for token in tokens {
        let content = match token {
            Token::Bracketed(s) | Token::Parenthesized(s) => *s,
            _ => continue,
        };
        if let Some(name) = tables::ln_publishers::lookup(content) {
            return Some(name);
        }
    }
    None
}

fn detect_scanner(tokens: &[Token]) -> Option<&'static str> {
    for token in tokens {
        let content = match token {
            Token::Bracketed(s) | Token::Parenthesized(s) => *s,
            _ => continue,
        };
        if let Some(name) = tables::ln_scanners::lookup(content) {
            return Some(name);
        }
    }
    None
}

/// True when any `(tag)` or `[tag]` bracket contains `needle` as a
/// case-insensitive whole-content match. Used for the boolean
/// `is_digital` and `is_premium` fields.
fn detect_tag(tokens: &[Token], needle: &str) -> bool {
    for token in tokens {
        let content = match token {
            Token::Parenthesized(s) | Token::Bracketed(s) => *s,
            _ => continue,
        };
        if content.eq_ignore_ascii_case(needle) {
            return true;
        }
    }
    false
}

/// First paren/bracket whose leading digits look like a year. Captures the
/// leading year of a range (`(2022-2024)` → 2022); single-year tags match
/// exactly. Year ranges are flagged in `_model_note` on the hand-picked
/// corpus but not yet stored structurally.
fn detect_year(tokens: &[Token]) -> Option<u16> {
    for token in tokens {
        let content = match token {
            Token::Parenthesized(s) | Token::Bracketed(s) => *s,
            _ => continue,
        };
        let first_digits: String = content.chars().take_while(|c| c.is_ascii_digit()).collect();
        if first_digits.is_empty() {
            continue;
        }
        if let Ok(n) = first_digits.parse::<u32>()
            && common::looks_like_year(n)
            && let Ok(n16) = u16::try_from(n)
        {
            return Some(n16);
        }
    }
    None
}

/// First `{r\d+}` curly tag → revision number. Kavita's test fixture
/// `Sword Art Online Vol 10 - Alicization Running [Yen Press] [LuCaZ] {r2}.epub`
/// is the canonical example.
fn detect_revision(tokens: &[Token]) -> Option<u8> {
    for token in tokens {
        let Token::Curly(content) = token else {
            continue;
        };
        let Some(rest) = content
            .strip_prefix('r')
            .or_else(|| content.strip_prefix('R'))
        else {
            continue;
        };
        if !rest.is_empty()
            && rest.chars().all(|c| c.is_ascii_digit())
            && let Ok(n) = rest.parse::<u8>()
        {
            return Some(n);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ChapterNumber;

    fn single(n: ChapterNumber) -> NumberRange {
        NumberRange::single(n)
    }

    fn range(a: ChapterNumber, b: ChapterNumber) -> NumberRange {
        NumberRange::range(a, b)
    }

    fn ch(n: u32) -> ChapterNumber {
        ChapterNumber::new(n)
    }

    // ----- parse stub / empty input -----

    #[test]
    fn parse_empty_returns_default() {
        let p = parse("");
        assert_eq!(p, ParsedNovel::default());
    }

    #[test]
    fn parse_manga_extension_fed_to_novel_degrades_gracefully() {
        // `.cbz` is a manga extension — novel parser must not match it.
        let p = parse("Some Manga Title v01.cbz");
        assert_eq!(p.extension, None);
        assert_eq!(p.volume, Some(single(ch(1))));
    }

    #[test]
    fn parse_ascii_garbage_no_panic() {
        // Pin no-panic and no structured detections; title may pass
        // through as the raw slice since `.` is title-content.
        let p = parse("...---...");
        assert_eq!(p.volume, None);
        assert_eq!(p.group, None);
        assert_eq!(p.extension, None);
    }

    // ----- extension -----

    #[test]
    fn extension_known_ln_types() {
        for ext in ["epub", "pdf", "azw3", "mobi", "txt"] {
            let f = format!("Some Title v01.{ext}");
            assert_eq!(detect_extension(&f), Some(ext), "ext {ext}");
        }
    }

    #[test]
    fn extension_rejects_manga_ext() {
        // cbz is manga-side; novel parser rejects it.
        assert_eq!(detect_extension("Title v01.cbz"), None);
    }

    // ----- volume (shared machinery) -----

    #[test]
    fn volume_single_token_v01() {
        assert_eq!(parse("Title v01.epub").volume, Some(single(ch(1))));
    }

    #[test]
    fn volume_range() {
        assert_eq!(
            parse("Bofuri v01-17 [Yen Press] [Stick]").volume,
            Some(range(ch(1), ch(17)))
        );
    }

    // ----- publisher / scanner / group -----

    #[test]
    fn publisher_yen_press() {
        let p = parse("Sword Art Online Vol 10 [Yen Press] [LuCaZ]");
        assert_eq!(p.publisher, Some("Yen Press"));
    }

    #[test]
    fn publisher_case_insensitive() {
        let p = parse("Title v01 (YEN PRESS)");
        assert_eq!(p.publisher, Some("Yen Press"));
    }

    #[test]
    fn scanner_lucaz() {
        let p = parse("Title v01 [Yen Press] [LuCaZ]");
        assert_eq!(p.scanner, Some("LuCaZ"));
    }

    #[test]
    fn scanner_case_insensitive() {
        let p = parse("Title v01 (STICK)");
        assert_eq!(p.scanner, Some("Stick"));
    }

    #[test]
    fn group_is_leading_bracket_when_not_publisher_or_scanner() {
        let p = parse("[Unpaid Ferryman] Youjo Senki v01-23 (2018-2024) (Digital) (LuCaZ)");
        assert_eq!(p.group, Some("Unpaid Ferryman"));
        assert_eq!(p.scanner, Some("LuCaZ"));
    }

    #[test]
    fn group_is_none_when_leading_bracket_is_publisher() {
        let p = parse("[Yen Press] Sword Art Online v10");
        assert_eq!(p.group, None);
        assert_eq!(p.publisher, Some("Yen Press"));
    }

    #[test]
    fn group_is_none_when_leading_bracket_is_scanner() {
        let p = parse("[LuCaZ] Some Title v01.epub");
        assert_eq!(p.group, None);
        assert_eq!(p.scanner, Some("LuCaZ"));
    }

    // ----- is_digital / is_premium -----

    #[test]
    fn is_digital_detected() {
        assert!(parse("Title v01 (Digital).epub").is_digital);
        assert!(!parse("Title v01.epub").is_digital);
    }

    #[test]
    fn is_premium_detected() {
        assert!(parse("Title v01 (Premium) [J-Novel Club]").is_premium);
        assert!(!parse("Title v01 [J-Novel Club]").is_premium);
    }

    // ----- year -----

    #[test]
    fn year_single() {
        let p = parse("Title v01 (2019) [Yen Press]");
        assert_eq!(p.year, Some(2019));
    }

    #[test]
    fn year_range_captures_start() {
        // Year ranges currently collapse to the first year. Document &
        // pin the behavior; full-range support is v1+.
        let p = parse("Title v01-23 (2018-2024) (Digital) (LuCaZ)");
        assert_eq!(p.year, Some(2018));
    }

    #[test]
    fn year_rejects_non_year_digits() {
        let p = parse("Title v01 (Vol 2019)"); // 2019 isn't the leading-digit run
        assert_eq!(p.year, None);
    }

    // ----- revision -----

    #[test]
    fn revision_from_curly_r_tag() {
        let p = parse("Sword Art Online Vol 10 [Yen Press] [LuCaZ] {r2}.epub");
        assert_eq!(p.revision, Some(2));
    }

    #[test]
    fn revision_absent_returns_none() {
        let p = parse("Title v01 [Yen Press].epub");
        assert_eq!(p.revision, None);
    }

    // ----- title -----

    fn title_str<'a>(p: &'a ParsedNovel<'a>) -> Option<&'a str> {
        p.title.as_deref()
    }

    #[test]
    fn title_simple() {
        let p = parse("Sword Art Online Vol 10 [Yen Press] [LuCaZ].epub");
        assert_eq!(title_str(&p), Some("Sword Art Online"));
    }

    #[test]
    fn title_skips_leading_group() {
        let p = parse("[Unpaid Ferryman] Youjo Senki v01-23 (Digital) (LuCaZ)");
        assert_eq!(title_str(&p), Some("Youjo Senki"));
    }

    // ----- full parse example -----

    #[test]
    fn parse_full_grammar_run() {
        let p = parse("[Unpaid Ferryman] Youjo Senki v01-23 (2018-2024) (Digital) (LuCaZ)");
        assert_eq!(p.group, Some("Unpaid Ferryman"));
        assert_eq!(p.volume, Some(range(ch(1), ch(23))));
        assert_eq!(p.year, Some(2018));
        assert!(p.is_digital);
        assert_eq!(p.scanner, Some("LuCaZ"));
        assert_eq!(p.extension, None);
    }

    // ----- smoke corpus (no panic) -----

    /// Smoke-test against ~360 real LN filenames sampled from Nyaa.
    ///
    /// Asserts only that `parse()` doesn't panic on any of them — the
    /// corpus deliberately includes false-positive manga entries because
    /// an LN parser must degrade gracefully on out-of-domain input.
    #[test]
    fn smoke_corpus_does_not_panic() {
        const CORPUS: &str = include_str!("../corpus/smoke_novel.txt");
        let mut count = 0usize;
        for line in CORPUS.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let _ = parse(line);
            count += 1;
        }
        assert!(
            count > 100,
            "smoke corpus shrank suspiciously: {count} entries"
        );
    }

    /// Aggregate-stat sanity check on the smoke corpus — rough coverage
    /// floors for the common fields. Catches silent regressions where
    /// a refactor stops populating a field on the common path.
    ///
    /// Floors are well below current measured rates; this isn't meant
    /// to push quality up, just to wake up if a previously-detected
    /// field stops detecting on >10% of the corpus.
    #[test]
    fn smoke_corpus_aggregate_detection_rates() {
        const CORPUS: &str = include_str!("../corpus/smoke_novel.txt");
        let mut total = 0usize;
        let mut with_title = 0usize;
        let mut with_volume = 0usize;
        let mut with_ext = 0usize;

        for line in CORPUS.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            total += 1;
            let p = parse(line);
            if p.title.is_some() {
                with_title += 1;
            }
            if p.volume.is_some() {
                with_volume += 1;
            }
            if p.extension.is_some() {
                with_ext += 1;
            }
        }

        let rate = |n: usize| n as f64 / total.max(1) as f64;
        let title_rate = rate(with_title);
        let volume_rate = rate(with_volume);
        let ext_rate = rate(with_ext);

        eprintln!("\n--- smoke_corpus_aggregate_detection_rates ---");
        eprintln!("total:      {total}");
        eprintln!(
            "title:      {with_title}/{total} = {:.1}%",
            title_rate * 100.0
        );
        eprintln!(
            "volume:     {with_volume}/{total} = {:.1}%",
            volume_rate * 100.0
        );
        eprintln!("extension:  {with_ext}/{total} = {:.1}%", ext_rate * 100.0);

        // Floors set well below current rates. Raise if the corpus
        // improves; lower (with a comment explaining why) if the bar
        // isn't justifiable for some new input class.
        //
        // Extension rate is NOT asserted — the smoke corpus is lifted
        // from Nyaa torrent *directory* names, which never carry
        // `.epub`/`.pdf`. Current rate is 0%; that's expected, not a
        // regression. The stat is printed for visibility only.
        let _ = ext_rate;
        assert!(
            title_rate >= 0.95,
            "title detection rate {:.1}% below floor",
            title_rate * 100.0
        );
        assert!(
            volume_rate >= 0.75,
            "volume detection rate {:.1}% below floor",
            volume_rate * 100.0
        );
    }

    // ----- corpus pass rate: Nyaa hand-picked + Kavita -----

    #[test]
    fn corpus_nyaa_field_pass_rate() {
        const CORPUS: &str = include_str!("../corpus/novel_nyaa.json");
        let entries: Vec<serde_json::Value> = serde_json::from_str(CORPUS).unwrap();

        #[derive(Default)]
        struct Stats {
            pass: usize,
            total: usize,
            failures: Vec<String>,
        }
        let mut by_field: std::collections::BTreeMap<&'static str, Stats> = Default::default();

        for entry in &entries {
            let input = entry["input"].as_str().unwrap_or("");
            let expected = &entry["expected"];
            let p = parse(input);

            let mut check = |field: &'static str, got: Option<String>, want: Option<&str>| {
                let Some(want) = want else { return };
                let s = by_field.entry(field).or_default();
                s.total += 1;
                let got_str = got.unwrap_or_default();
                if got_str == want {
                    s.pass += 1;
                } else if s.failures.len() < 3 {
                    let truncated: String = input.chars().take(60).collect();
                    s.failures
                        .push(format!("{truncated}  {field}: want={want} got={got_str}"));
                }
            };

            check(
                "group",
                p.group.map(str::to_owned),
                expected.get("group").and_then(|v| v.as_str()),
            );
            check(
                "volume_range",
                p.volume.as_ref().map(format_range),
                expected.get("volume_range").and_then(|v| v.as_str()),
            );
            check(
                "publisher",
                p.publisher.map(str::to_owned),
                expected.get("publisher").and_then(|v| v.as_str()),
            );
            check(
                "scanner",
                p.scanner.map(str::to_owned),
                expected.get("scanner").and_then(|v| v.as_str()),
            );
            check(
                "language",
                p.language.map(|l| format!("{l:?}")),
                expected.get("language").and_then(|v| v.as_str()),
            );
            check(
                "extension",
                p.extension.map(str::to_owned),
                expected.get("extension").and_then(|v| v.as_str()),
            );
            check(
                "revision",
                p.revision.map(|r| r.to_string()),
                expected
                    .get("revision")
                    .and_then(|v| v.as_u64())
                    .map(|n| n.to_string())
                    .as_deref(),
            );

            // Boolean flags — exercised when the fixture declares an expected value.
            let mut check_bool = |field: &'static str, got: bool, want_key: &str| {
                let Some(want) = expected.get(want_key).and_then(|v| v.as_bool()) else {
                    return;
                };
                let s = by_field.entry(field).or_default();
                s.total += 1;
                if got == want {
                    s.pass += 1;
                } else if s.failures.len() < 3 {
                    let truncated: String = input.chars().take(60).collect();
                    s.failures
                        .push(format!("{truncated}  {field}: want={want} got={got}"));
                }
            };
            check_bool("is_digital", p.is_digital, "is_digital");
            check_bool("is_premium", p.is_premium, "is_premium");
        }

        eprintln!("\n--- corpus_nyaa_field_pass_rate ---");
        let mut total_pass = 0usize;
        let mut total_count = 0usize;
        for (field, s) in &by_field {
            total_pass += s.pass;
            total_count += s.total;
            let pct = if s.total > 0 {
                s.pass as f64 / s.total as f64 * 100.0
            } else {
                0.0
            };
            eprintln!("{:<20}  {:>3}/{:<3}  {:>5.1}%", field, s.pass, s.total, pct);
            for f in &s.failures {
                eprintln!("    FAIL: {f}");
            }
        }
        let aggregate = total_pass as f64 / total_count.max(1) as f64;
        eprintln!(
            "aggregate: {total_pass}/{total_count} = {:.1}%",
            aggregate * 100.0
        );

        // Floor set just below current measured rate so regressions surface.
        // Raise as the parser (or corpus) improves.
        assert!(
            aggregate >= 0.75,
            "Nyaa corpus pass rate {:.1}% dropped below floor",
            aggregate * 100.0
        );
    }

    #[test]
    fn corpus_kavita_book_pass_rate() {
        const CORPUS: &str = include_str!("../corpus/novel_kavita.json");
        let entries: Vec<serde_json::Value> = serde_json::from_str(CORPUS).unwrap();

        let mut pass = 0usize;
        let mut total = 0usize;
        let mut failures: Vec<String> = Vec::new();

        for entry in &entries {
            let source = entry["source"].as_str().unwrap_or("");
            let method = source.rsplit("::").next().unwrap_or("");
            let input = entry["input"].as_str().unwrap_or("");
            let p = parse(input);

            let (expected, actual): (Option<&str>, Option<String>) = match method {
                "ParseVolumeTest" => (
                    entry.get("expected_volume").and_then(|v| v.as_str()),
                    p.volume.as_ref().map(format_range),
                ),
                "ParseSeriesTest" => (
                    entry.get("expected_series").and_then(|v| v.as_str()),
                    p.title.as_deref().map(str::to_owned),
                ),
                _ => continue,
            };
            let Some(expected) = expected else { continue };
            total += 1;
            let actual_str = actual.unwrap_or_default();
            if actual_str == expected {
                pass += 1;
            } else if failures.len() < 5 {
                failures.push(format!("{}  want={} got={}", input, expected, actual_str));
            }
        }

        let rate = pass as f64 / total.max(1) as f64;
        eprintln!("\n--- corpus_kavita_book_pass_rate ---");
        eprintln!("{pass}/{total} = {:.1}%", rate * 100.0);
        for f in &failures {
            eprintln!("    FAIL: {f}");
        }
        // Only 5 entries in Kavita BookParsingTests so the floor is
        // lumpy (80% = 4/5, 60% = 3/5). Currently 4/5 passes — the
        // remaining failure is a trailing-subtitle-slicing edge case
        // we haven't implemented. 0.60 = 3/5 keeps the floor below
        // current and gives one fixture of regression headroom.
        assert!(
            rate >= 0.60,
            "Kavita book corpus rate {:.1}% below floor",
            rate * 100.0
        );
    }

    fn format_range(r: &NumberRange) -> String {
        let fmt = |n: ChapterNumber| match n.decimal {
            None => n.whole.to_string(),
            Some(d) => format!("{}.{}", n.whole, d),
        };
        match r.end {
            None => fmt(r.start),
            Some(end) => format!("{}-{}", fmt(r.start), fmt(end)),
        }
    }
}
