//! Known LN scanner / release-credit names, keyed by case-insensitive name match.
//!
//! These appear in filenames like `[LuCaZ]` or `[Stick]` and credit the person
//! responsible for converting a publisher's release into a clean EPUB. Seed list
//! comes from the design doc's named examples; expand as the corpus grows.

// Ordered roughly by corpus frequency. `Kobo` is Rakuten's e-reader platform
// rather than a hand-scan credit, but filenames carry it in the same
// scanner-shaped position so it lives here until a separate `source`-style
// field materializes for LN.
const SCANNERS: &[&str] = &[
    "LuCaZ",
    "Stick",
    "Ushi",
    "Oak",
    "nao",
    "CleanBookGuy",
    "Kobo",
    "faratnis",
    "Antithetical",
    "DigitalMangaFan",
];

/// Lookup a scanner credit by name, case-insensitively. Returns the canonical form.
#[must_use]
pub fn lookup(name: &str) -> Option<&'static str> {
    SCANNERS
        .iter()
        .find(|s| s.eq_ignore_ascii_case(name))
        .copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_scanner_canonicalizes_case() {
        assert_eq!(lookup("lucaz"), Some("LuCaZ"));
        assert_eq!(lookup("STICK"), Some("Stick"));
    }

    #[test]
    fn unknown_scanner_returns_none() {
        assert_eq!(lookup("nobody"), None);
    }
}
