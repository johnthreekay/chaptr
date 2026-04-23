//! Language tags, mapped from common filename shortcodes and full names
//! to [`crate::Language`] variants.
//!
//! Exact-match (case-insensitive) against the whole bracket/paren content.
//! We deliberately *don't* match on substrings — a filename like
//! `Engineering Manual.cbz` must not resolve to `English` because `en` is
//! a substring. Substring detection would also conflict with format tags
//! like `[Raw]` that carry a language implication (Japanese) but aren't
//! canonical language codes.
//!
//! Ordered by entry; `cn` and `chinese` bare (without a
//! simplified/traditional qualifier) default to `SimplifiedChinese`
//! because that matches the Nyaa release convention — TW/HK releases
//! tend to use the explicit `zh-tw` form.
//!
//! `Raw` / `Raws` are *not* mapped to Japanese here. They're format tags
//! that often imply Japanese source but aren't language declarations.
//! A consumer that wants to treat `[Raw]` as Japanese can do so
//! explicitly; we don't conflate format and language in the parse
//! output.

use crate::Language;

const LANGUAGE_TAGS: &[(&str, Language)] = &[
    // English
    ("en", Language::English),
    ("eng", Language::English),
    ("english", Language::English),
    // Japanese
    ("jp", Language::Japanese),
    ("ja", Language::Japanese),
    ("jpn", Language::Japanese),
    ("japanese", Language::Japanese),
    // Simplified Chinese — `cn` / `chinese` bare default here by Nyaa convention
    ("cn", Language::SimplifiedChinese),
    ("chinese", Language::SimplifiedChinese),
    ("sc", Language::SimplifiedChinese),
    ("zh", Language::SimplifiedChinese),
    ("zh-cn", Language::SimplifiedChinese),
    ("zh_cn", Language::SimplifiedChinese),
    ("zhcn", Language::SimplifiedChinese),
    ("simplified chinese", Language::SimplifiedChinese),
    ("chinese (simplified)", Language::SimplifiedChinese),
    ("简体中文", Language::SimplifiedChinese),
    // Traditional Chinese
    ("tc", Language::TraditionalChinese),
    ("tw", Language::TraditionalChinese),
    ("hk", Language::TraditionalChinese),
    ("zh-tw", Language::TraditionalChinese),
    ("zh_tw", Language::TraditionalChinese),
    ("zhtw", Language::TraditionalChinese),
    ("zh-hk", Language::TraditionalChinese),
    ("traditional chinese", Language::TraditionalChinese),
    ("chinese (traditional)", Language::TraditionalChinese),
    ("繁體中文", Language::TraditionalChinese),
    ("繁体中文", Language::TraditionalChinese),
    // Korean
    ("kr", Language::Korean),
    ("ko", Language::Korean),
    ("kor", Language::Korean),
    ("korean", Language::Korean),
    ("한국어", Language::Korean),
    // Spanish
    ("es", Language::Spanish),
    ("esp", Language::Spanish),
    ("spa", Language::Spanish),
    ("spanish", Language::Spanish),
    ("español", Language::Spanish),
    ("espanol", Language::Spanish),
    // French
    ("fr", Language::French),
    ("fra", Language::French),
    ("fre", Language::French),
    ("french", Language::French),
    ("français", Language::French),
    ("francais", Language::French),
    // German
    ("de", Language::German),
    ("ger", Language::German),
    ("deu", Language::German),
    ("german", Language::German),
    ("deutsch", Language::German),
    // Italian
    ("it", Language::Italian),
    ("ita", Language::Italian),
    ("italian", Language::Italian),
    ("italiano", Language::Italian),
    // Portuguese (default); pt-br/ptbr/etc collapse into Portuguese since
    // we don't distinguish regional variants in the enum
    ("pt", Language::Portuguese),
    ("por", Language::Portuguese),
    ("portuguese", Language::Portuguese),
    ("pt-br", Language::Portuguese),
    ("pt_br", Language::Portuguese),
    ("ptbr", Language::Portuguese),
    ("português", Language::Portuguese),
    ("portugues", Language::Portuguese),
    // Russian
    ("ru", Language::Russian),
    ("rus", Language::Russian),
    ("russian", Language::Russian),
    ("русский", Language::Russian),
    // Indonesian
    ("id", Language::Indonesian),
    ("idn", Language::Indonesian),
    ("ind", Language::Indonesian),
    ("indonesian", Language::Indonesian),
    ("bahasa", Language::Indonesian),
    ("bahasa indonesia", Language::Indonesian),
    // Vietnamese
    ("vi", Language::Vietnamese),
    ("vn", Language::Vietnamese),
    ("vie", Language::Vietnamese),
    ("vietnamese", Language::Vietnamese),
    ("tiếng việt", Language::Vietnamese),
    // Thai
    ("th", Language::Thai),
    ("tha", Language::Thai),
    ("thai", Language::Thai),
    ("ไทย", Language::Thai),
];

/// Resolve a bracket/paren content string to a [`Language`] variant.
///
/// Case-insensitive for ASCII entries; exact match for non-ASCII (CJK,
/// Cyrillic, Thai script have no meaningful case distinction). Returns
/// `None` if the content isn't a recognized language tag.
#[must_use]
pub fn lookup(content: &str) -> Option<Language> {
    if content.is_ascii() {
        for (tag, lang) in LANGUAGE_TAGS {
            if content.eq_ignore_ascii_case(tag) {
                return Some(*lang);
            }
        }
        return None;
    }
    let lower = content.to_lowercase();
    for (tag, lang) in LANGUAGE_TAGS {
        if lower == *tag {
            return Some(*lang);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_codes_case_insensitive() {
        assert_eq!(lookup("EN"), Some(Language::English));
        assert_eq!(lookup("en"), Some(Language::English));
        assert_eq!(lookup("JP"), Some(Language::Japanese));
    }

    #[test]
    fn full_names_ascii() {
        assert_eq!(lookup("English"), Some(Language::English));
        assert_eq!(lookup("JAPANESE"), Some(Language::Japanese));
        assert_eq!(lookup("french"), Some(Language::French));
    }

    #[test]
    fn bare_chinese_defaults_to_simplified() {
        // Nyaa convention: unqualified `Chinese` / `CN` / `zh` =
        // Simplified. TW/HK releases use explicit regional tags.
        assert_eq!(lookup("Chinese"), Some(Language::SimplifiedChinese));
        assert_eq!(lookup("CN"), Some(Language::SimplifiedChinese));
        assert_eq!(lookup("zh"), Some(Language::SimplifiedChinese));
    }

    #[test]
    fn traditional_chinese_explicit() {
        assert_eq!(lookup("zh-tw"), Some(Language::TraditionalChinese));
        assert_eq!(lookup("TC"), Some(Language::TraditionalChinese));
        assert_eq!(
            lookup("Traditional Chinese"),
            Some(Language::TraditionalChinese)
        );
    }

    #[test]
    fn cjk_script_tags() {
        assert_eq!(lookup("简体中文"), Some(Language::SimplifiedChinese));
        assert_eq!(lookup("繁體中文"), Some(Language::TraditionalChinese));
        assert_eq!(lookup("한국어"), Some(Language::Korean));
    }

    #[test]
    fn cyrillic_tag() {
        assert_eq!(lookup("русский"), Some(Language::Russian));
        assert_eq!(lookup("РУССКИЙ"), Some(Language::Russian));
    }

    #[test]
    fn unknown_returns_none() {
        assert_eq!(lookup("nobody"), None);
        assert_eq!(lookup("Raw"), None); // format tag, not language
        assert_eq!(lookup(""), None);
    }

    #[test]
    fn portuguese_variants_collapse() {
        assert_eq!(lookup("pt"), Some(Language::Portuguese));
        assert_eq!(lookup("PT-BR"), Some(Language::Portuguese));
        assert_eq!(lookup("ptbr"), Some(Language::Portuguese));
    }
}
