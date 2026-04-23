//! Known light-novel publishers, keyed by case-insensitive name match.
//!
//! Initial seed covers the publishers the design doc explicitly named plus the
//! handful of others that dominate English LN releases. Add as the corpus surfaces
//! more.

/// Canonical publisher names. Lookup is case-insensitive; the canonical form is
/// what gets returned and stored.
const PUBLISHERS: &[&str] = &[
    "Yen Press",
    "J-Novel Club",
    "Seven Seas",
    "Seven Seas Siren", // Seven Seas' adult-content imprint; stored distinctly
    "Vertical",
    "Kodansha",
    "One Peace Books",
    "Cross Infinite World",
    "Tentai Books",
    "Hanashi Media",
];

/// Lookup a publisher by name, case-insensitively. Returns the canonical form.
#[must_use]
pub fn lookup(name: &str) -> Option<&'static str> {
    PUBLISHERS
        .iter()
        .find(|p| p.eq_ignore_ascii_case(name))
        .copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_publisher_canonicalizes_case() {
        assert_eq!(lookup("yen press"), Some("Yen Press"));
        assert_eq!(lookup("J-NOVEL CLUB"), Some("J-Novel Club"));
    }

    #[test]
    fn unknown_publisher_returns_none() {
        assert_eq!(lookup("Some Random Press"), None);
    }
}
