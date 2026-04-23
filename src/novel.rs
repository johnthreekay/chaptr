//! Light novel filename parser.
//!
//! Web novels are explicitly out of scope: WN content is HTML scraped into EPUBs by
//! Ryokan-controlled scrapers in v2.1, which means the consumer controls the output
//! filename and there's nothing external to parse.

use crate::{Language, NumberRange};

/// Structured fields parsed from a single light-novel filename.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ParsedNovel<'a> {
    pub title: Option<&'a str>,
    pub volume: Option<NumberRange>,
    /// Publishing house (Yen Press, J-Novel Club, Seven Seas, ...).
    /// Looked up against [`crate::tables::ln_publishers`].
    pub publisher: Option<&'a str>,
    /// Scan/release credit (LuCaZ, Stick, ...).
    /// Looked up against [`crate::tables::ln_scanners`].
    pub scanner: Option<&'a str>,
    pub language: Option<Language>,
    pub year: Option<u16>,
    /// `(Premium)` tag — J-Novel Club premium-tier release.
    pub is_premium: bool,
    /// `(Digital)` tag.
    pub is_digital: bool,
    pub revision: Option<u8>,
    /// Extension without leading dot (`epub`, `pdf`, `azw3`, `mobi`, `txt`).
    pub extension: Option<&'a str>,
}

/// Parse a light-novel filename into structured fields.
///
/// **Stub** — currently returns a default `ParsedNovel` regardless of input.
/// Implementation lands after manga::parse, per the design doc's ordering.
#[must_use]
pub fn parse(_filename: &str) -> ParsedNovel<'_> {
    ParsedNovel::default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_empty_returns_default() {
        let p = parse("");
        assert_eq!(p, ParsedNovel::default());
    }

    /// Smoke-test against ~360 real LN filenames sampled from Nyaa.
    ///
    /// Asserts only that `parse()` does not panic on any of them — no
    /// expected-output assertions. The smoke corpus deliberately includes
    /// false-positive manga entries; a parser pinned to LN grammar must still
    /// degrade gracefully (return a `ParsedNovel` with mostly-`None` fields)
    /// when handed a manga string, not crash.
    ///
    /// Refresh the corpus by re-running `tools/scrape_nyaa.py` and promoting
    /// `corpus/raw/nyaa_literature.txt` → `corpus/smoke_novel.txt`.
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
        // Sanity-check the corpus didn't get truncated to nothing.
        assert!(
            count > 100,
            "smoke corpus shrank suspiciously: {count} entries"
        );
    }
}
