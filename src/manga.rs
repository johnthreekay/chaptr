//! Manga / manhwa / manhua filename parser.
//!
//! Manhwa/manhua live here too — their filename grammar overlaps closely with manga.
//! Source-tag differences (Lezhin / Naver / Kakao for manhwa) are carried by
//! [`MangaSource`], not by a separate module.

use crate::{Language, NumberRange};

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
///
/// Manhwa-native sources (Lezhin / Naver / Kakao) live here too rather than in a
/// separate enum; they're a property of the scan, not of a different parsing grammar.
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
///
/// **Stub** — currently returns a default `ParsedManga` regardless of input.
/// The L2+ classifier will be implemented after the lexer corpus pass lands.
#[must_use]
pub fn parse(_filename: &str) -> ParsedManga<'_> {
    ParsedManga::default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_empty_returns_default() {
        let p = parse("");
        assert_eq!(p, ParsedManga::default());
    }
}
