//! Manga / manhwa / manhua filename parser.
//!
//! Manhwa/manhua live here too — their filename grammar overlaps closely with
//! manga. Source-tag differences (Lezhin / Naver / Kakao for manhwa) are
//! carried by [`MangaSource`], not by a separate module.
//!
//! The generic parts of the pipeline (tokenization, volume/chapter/title
//! detection, CJK marker scanning, keyword tables) live in
//! [`crate::common`]; this module wires them together with
//! manga-specific extensions, group logic, and source-tag classification.
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
//! Batman `T2000`) are recognized. CJK markers `巻` / `卷` / `册` / `권` /
//! `장` / `话` / `話` / `章` / `回`, plus multi-token Cyrillic keywords
//! `Том` / `Тома` / `Глава` / `Главы`.
//!
//! **Out of scope**: language tags, revision *extraction* (the suffix is
//! consumed but not stored), oneshot detection, Korean multi-char prefix
//! `시즌`, Thai (`เล่ม`), alpha-suffix decimal chapters (`Beelzebub_153b` =
//! 153.5), `c001-006x1`-style chapter ranges, and the rest of `MangaSource`
//! (Viz / Kodansha / Lezhin / Naver / Kakao).

use std::borrow::Cow;

use crate::common;
use crate::lexer::{Token, tokenize};
use crate::{Language, NumberRange};

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
        title: common::detect_title(filename, &tokens, MANGA_EXTENSIONS),
        volume: common::detect_volume(&tokens),
        chapter: common::detect_chapter(&tokens),
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
    common::detect_extension(filename, MANGA_EXTENSIONS)
}

fn detect_group<'a>(tokens: &[Token<'a>]) -> Option<&'a str> {
    for t in tokens {
        if let Token::Bracketed(content) = t
            && !content.is_empty()
            && !common::contains_volume_keyword(content)
        {
            return Some(*content);
        }
    }
    None
}

/// Patterns that map bracketed/parenthesized content to a `MangaSource`.
/// Matches on exact equality OR prefix-followed-by-space-or-hyphen, so
/// `(Digital)` and `(Digital-HD)` and `(Digital HD)` all resolve to
/// `MangaSource::Digital`.
///
/// Longer patterns come first to avoid shadowing — `"viz media"` is checked
/// before `"viz"` so `(Viz Media)` classifies as `Viz` without matching
/// `"viz"` + trailing " media".
///
/// `MangaSource::Scan` is intentionally NOT listed: the generic `(Scan)` tag
/// is rare in real filenames, and `scan`-as-substring catches group names
/// like `[SlowManga&OverloadScans]` or `[Scans_Compressed]` as false
/// positives. Scan source is set externally by the consumer (e.g. from
/// sidecar metadata) when known.
const SOURCE_PATTERNS: &[(&str, MangaSource)] = &[
    ("manga plus", MangaSource::MangaPlus),
    ("mangaplus", MangaSource::MangaPlus),
    ("viz media", MangaSource::Viz),
    ("viz", MangaSource::Viz),
    ("kodansha usa", MangaSource::Kodansha),
    ("kodansha", MangaSource::Kodansha),
    ("lezhin comics", MangaSource::Lezhin),
    ("lezhin", MangaSource::Lezhin),
    ("naver webtoon", MangaSource::Naver),
    ("naver series", MangaSource::Naver),
    ("naver", MangaSource::Naver),
    ("kakao page", MangaSource::Kakao),
    ("kakao", MangaSource::Kakao),
    ("digital", MangaSource::Digital),
];

fn detect_source(tokens: &[Token]) -> Option<MangaSource> {
    for t in tokens {
        let content = match t {
            Token::Parenthesized(s) | Token::Bracketed(s) => *s,
            _ => continue,
        };
        let lower = content.to_ascii_lowercase();
        for (pattern, source) in SOURCE_PATTERNS {
            if lower == *pattern {
                return Some(*source);
            }
            if let Some(rest) = lower.strip_prefix(*pattern)
                && matches!(rest.chars().next(), Some('-' | ' '))
            {
                return Some(*source);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ChapterNumber;

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
        let p = parse("One Piece - Vol 2 Ch 1.1 - Volume 4 Omakes");
        assert_eq!(p.volume, Some(single(ch(2))));
    }

    #[test]
    fn volume_absent_returns_none() {
        assert_eq!(parse("Just a title.cbz").volume, None);
    }

    #[test]
    fn volume_in_parens() {
        let p = parse("Gokukoku no Brynhildr - c001-008 (v01) [TrinityBAKumA]");
        assert_eq!(p.volume, Some(single(ch(1))));
    }

    #[test]
    fn volume_in_brackets() {
        assert_eq!(
            parse("Tonikaku Cawaii [Volume 11].cbz").volume,
            Some(single(ch(11)))
        );
    }

    #[test]
    fn volume_season_prefix_s01() {
        let p = parse("Tower Of God S01 014.cbz");
        assert_eq!(p.volume, Some(single(ch(1))));
    }

    #[test]
    fn range_rejects_backward_end() {
        // Mangapy `vol_356-1` syntax — the `-1` isn't a range end (1 < 356).
        assert_eq!(parse("vol_356-1").volume, Some(single(ch(356))));
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

    #[test]
    fn chapter_revision_suffix_chapter_n_v_n() {
        // `Chapter11v2` = chapter 11 (revision silently consumed).
        assert_eq!(
            parse("Yumekui-Merry_DKThias_Chapter11v2.zip").chapter,
            Some(single(ch(11)))
        );
        assert_eq!(parse("Title c042v2.zip").chapter, Some(single(ch(42))));
    }

    #[test]
    fn chapter_chp_prefix() {
        assert_eq!(
            parse("[Hidoi]_Amaenaideyo_MS_vol01_chp02.rar").chapter,
            Some(single(ch(2)))
        );
    }

    #[test]
    fn chapter_bare_number_after_title() {
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
        assert_eq!(parse("Title 2019 Edition").chapter, None);
    }

    #[test]
    fn chapter_bare_number_skips_leading_numeric_title() {
        assert_eq!(parse("100 Years Of Solitude").chapter, None);
    }

    #[test]
    fn chapter_bare_number_skips_after_vol_keyword() {
        let p = parse("Series Vol 5 - 042");
        assert_eq!(p.volume, Some(single(ch(5))));
        assert_eq!(p.chapter, Some(single(ch(42))));
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

    #[test]
    fn source_viz_variants() {
        assert_eq!(parse("Title v01 (Viz).cbz").source, Some(MangaSource::Viz));
        assert_eq!(
            parse("Title v01 (Viz Media).cbz").source,
            Some(MangaSource::Viz)
        );
    }

    #[test]
    fn source_kodansha_variants() {
        assert_eq!(
            parse("Title v01 (Kodansha).cbz").source,
            Some(MangaSource::Kodansha)
        );
        assert_eq!(
            parse("Title v01 (Kodansha USA).cbz").source,
            Some(MangaSource::Kodansha)
        );
    }

    #[test]
    fn source_lezhin_manhwa() {
        assert_eq!(
            parse("Title c042 (Lezhin).cbz").source,
            Some(MangaSource::Lezhin)
        );
        assert_eq!(
            parse("Title c042 (Lezhin Comics).cbz").source,
            Some(MangaSource::Lezhin)
        );
    }

    #[test]
    fn source_naver_manhwa() {
        assert_eq!(
            parse("Title c042 (Naver).cbz").source,
            Some(MangaSource::Naver)
        );
        assert_eq!(
            parse("Title c042 (Naver Webtoon).cbz").source,
            Some(MangaSource::Naver)
        );
        assert_eq!(
            parse("Title c042 (Naver Series).cbz").source,
            Some(MangaSource::Naver)
        );
    }

    #[test]
    fn source_kakao_manhwa() {
        assert_eq!(
            parse("Title c042 (Kakao).cbz").source,
            Some(MangaSource::Kakao)
        );
        assert_eq!(
            parse("Title c042 (Kakao Page).cbz").source,
            Some(MangaSource::Kakao)
        );
    }

    #[test]
    fn source_ignores_substring_matches_in_group_names() {
        // `[SlowManga&OverloadScans]` is a group name, not a scan-source tag.
        // Generic `scan` is intentionally NOT in SOURCE_PATTERNS, so this
        // doesn't match anything.
        let p = parse("Title v01 [SlowManga&OverloadScans]");
        assert_eq!(p.source, None);
    }

    #[test]
    fn source_case_insensitive() {
        assert_eq!(
            parse("Title c042 (LEZHIN).cbz").source,
            Some(MangaSource::Lezhin)
        );
        assert_eq!(
            parse("Title c042 (kakao page).cbz").source,
            Some(MangaSource::Kakao)
        );
    }

    // ----- CJK markers -----

    #[test]
    fn volume_cjk_postfix_japanese() {
        let p = parse("スライム倒して300年 1巻");
        assert_eq!(p.volume, Some(single(ch(1))));
    }

    #[test]
    fn volume_cjk_postfix_chinese_prefix_combo() {
        let p = parse("幽游白书完全版 第03卷 天下");
        assert_eq!(p.volume, Some(single(ch(3))));
    }

    #[test]
    fn volume_cjk_postfix_chinese_ce() {
        assert_eq!(parse("阿衰online 第1册").volume, Some(single(ch(1))));
    }

    #[test]
    fn volume_cjk_prefix_only() {
        assert_eq!(parse("卷5 Test").volume, Some(single(ch(5))));
    }

    #[test]
    fn volume_cjk_compound_with_chapter() {
        let p = parse("【TFO汉化】迷你偶像漫画卷2第25话");
        assert_eq!(p.volume, Some(single(ch(2))));
    }

    #[test]
    fn volume_cjk_range() {
        let p = parse("スライム倒して300年 1-3巻");
        assert_eq!(p.volume, Some(range(ch(1), ch(3))));
    }

    #[test]
    fn volume_cjk_decimal_combined() {
        let p = parse("몰루 아카이브 7.5권");
        assert_eq!(p.volume, Some(single(ch_dec(7, 5))));
    }

    #[test]
    fn volume_korean_jang() {
        assert_eq!(parse("동의보감 13장").volume, Some(single(ch(13))));
    }

    #[test]
    fn volume_russian_tom_keyword() {
        let p = parse("Kebab Том 1 Глава 3");
        assert_eq!(p.volume, Some(single(ch(1))));
    }

    #[test]
    fn volume_russian_tom_range() {
        let p = parse("Манга Том 1-4");
        assert_eq!(p.volume, Some(range(ch(1), ch(4))));
    }

    #[test]
    fn volume_french_tome_multi_token() {
        assert_eq!(parse("Conquistador_Tome_2").volume, Some(single(ch(2))));
    }

    #[test]
    fn volume_t_prefix_combined_token() {
        assert_eq!(
            parse("Daredevil - t6 - 10 - (2019)").volume,
            Some(single(ch(6)))
        );
        assert_eq!(parse("Batgirl T2000 #57").volume, Some(single(ch(2000))));
    }

    #[test]
    fn chapter_russian_glava_keyword() {
        let p = parse("Kebab Том 1 Глава 3");
        assert_eq!(p.chapter, Some(single(ch(3))));
    }

    // ----- v4 edge cases -----

    #[test]
    fn chapter_kaiju_title_collision() {
        // `Kaiju No. 8 036` — the `8` is part of the title, not the chapter.
        // Metadata-lookahead rule: `8` is followed by `036` (digits, not a
        // group code, not a paren) → not chapter. `036` is followed by
        // `(2021)` → chapter.
        let p = parse("Kaiju No. 8 036 (2021) (Digital)");
        assert_eq!(p.chapter, Some(single(ch(36))));
    }

    #[test]
    fn chapter_bare_number_followed_by_group_code() {
        // `Beelzebub_172_RHS.zip` — `RHS` is an all-uppercase group code,
        // so `172` is the chapter even without a bracket/paren after.
        let p = parse("Beelzebub_172_RHS.zip");
        assert_eq!(p.chapter, Some(single(ch(172))));
    }

    #[test]
    fn chapter_bare_number_with_decimal_attached() {
        // `017.5` — lexer splits as `017`, Delim('.'), `5`. Without the
        // fast-path in bare_number_followed_by_metadata, the `017` would
        // be rejected (next non-delim is `5`, not a group code).
        let p = parse("Goblin Slayer Side Story - Year One 017.5");
        assert_eq!(p.chapter, Some(single(ch_dec(17, 5))));
    }

    #[test]
    fn chapter_bare_number_range_attached() {
        // `001-003` range — same fast-path reasoning as decimal.
        let p = parse("Bleach 001-003");
        assert_eq!(p.chapter, Some(range(ch(1), ch(3))));
    }

    #[test]
    fn chapter_russian_glava_postfix() {
        // `Манга 2 Глава` — Russian has `Глава` as postfix chapter keyword.
        // Bare-number `2` is followed by `Глава` (chapter keyword) → counts
        // as metadata-adjacent, so `2` becomes the chapter.
        let p = parse("Манга 2 Глава");
        assert_eq!(p.chapter, Some(single(ch(2))));
    }

    #[test]
    fn chapter_korean_hwa_marker() {
        // Korean 화 (talk/chapter) postfix marker.
        let p = parse("조선왕조실톡 106화");
        assert_eq!(p.chapter, Some(single(ch(106))));
    }

    #[test]
    fn chapter_korean_hoe_marker() {
        // Korean 회 (round/chapter) postfix marker.
        let p = parse("자유록 13회");
        assert_eq!(p.chapter, Some(single(ch(13))));
    }

    #[test]
    fn title_accel_world_colon() {
        // Trailing `:` trimmed from the title (before a volume keyword).
        let p = parse("Accel World: Vol 1");
        assert_eq!(p.title.as_deref(), Some("Accel World"));
    }

    #[test]
    fn title_skips_leading_paren_bracket_chain() {
        // `(一般コミック) [奥浩哉] いぬやしき 第09巻` — both leading
        // brackets/parens should be skipped by title_start, and the chain
        // of leading bracket-tokens should all be skipped by title_end.
        let p = parse("(一般コミック) [奥浩哉] いぬやしき 第09巻");
        assert_eq!(p.title.as_deref(), Some("いぬやしき"));
    }

    // ----- title -----

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
        let p = parse("B_Gata_H_Kei_v01[SlowManga&OverloadScans]");
        assert_eq!(title_str(&p), Some("B Gata H Kei"));
        assert!(matches!(p.title, Some(Cow::Owned(_))));
    }

    #[test]
    fn title_borrows_when_no_underscore() {
        let p = parse("BTOOOM! v01 (2013) (Digital) (Shadowcat-Empire)");
        assert_eq!(title_str(&p), Some("BTOOOM!"));
        assert!(matches!(p.title, Some(Cow::Borrowed(_))));
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
        assert_eq!(title_str(&p), Some("100 Years Of Solitude"));
    }

    #[test]
    fn title_bare_number_chapter_stops_title_walk() {
        let p = parse("APOSIMZ 017 (2018) (Digital) (danke-Empire).cbz");
        assert_eq!(title_str(&p), Some("APOSIMZ"));
    }

    #[test]
    fn title_trims_trailing_comma() {
        let p = parse("Kedouin Makoto - Corpse Party Musume, Chapter 19");
        assert_eq!(title_str(&p), Some("Kedouin Makoto - Corpse Party Musume"));
    }

    #[test]
    fn title_trims_trailing_hash() {
        let p = parse("Kodoja #001 (March 2016)");
        assert_eq!(title_str(&p), Some("Kodoja"));
    }

    #[test]
    fn title_returns_none_when_only_marker() {
        let p = parse("v001");
        assert_eq!(p.title, None);
    }

    #[test]
    fn title_handles_cjk() {
        let p = parse("スライム倒して300年 1巻");
        assert_eq!(title_str(&p), Some("スライム倒して300年"));
    }

    // ----- end-to-end -----

    #[test]
    fn parse_canonical_kavita_example() {
        let p = parse("BTOOOM! v01 (2013) (Digital) (Shadowcat-Empire)");
        assert_eq!(p.volume, Some(single(ch(1))));
        assert_eq!(p.source, Some(MangaSource::Digital));
        assert_eq!(p.extension, None);
    }

    #[test]
    fn parse_full_grammar_run() {
        let p = parse("[Yen Press] Sword Art Online v10 c042.5 (Digital).epub");
        assert_eq!(p.group, Some("Yen Press"));
        assert_eq!(p.volume, Some(single(ch(10))));
        assert_eq!(p.chapter, Some(single(ch_dec(42, 5))));
        assert_eq!(p.source, Some(MangaSource::Digital));
        assert_eq!(p.extension, None);
    }

    // ----- corpus -----

    /// Run `parse()` against every Kavita fixture and report per-method pass
    /// rates. Asserts a minimum aggregate rate so regressions surface, and
    /// prints first-N failures so it's easy to see what's not working.
    #[test]
    fn corpus_kavita_pass_rate() {
        const CORPUS: &str = include_str!("../corpus/manga_kavita.json");
        const MIN_AGGREGATE_PASS_RATE: f64 = 0.90;

        let entries: Vec<serde_json::Value> = serde_json::from_str(CORPUS).unwrap();

        #[derive(Default)]
        struct Bucket {
            pass: usize,
            total: usize,
            failures: Vec<(String, String, String)>,
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

            let Some(expected) = expected_field else {
                continue;
            };

            let bucket = by_method.entry(method).or_default();
            bucket.total += 1;

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
