//! Known scanlation groups, keyed by their canonical bracketed name.
//!
//! Empty until the corpus pass — seeding strategy is one of the open design-doc
//! questions (Nyaa top uploaders vs MangaDex group list vs hand-curated).

/// Lookup a group by its bracketed name, case-sensitively.
///
/// Returns the canonical name if known. Currently always returns `None` — the table
/// is empty pending the corpus pass.
#[must_use]
pub fn lookup(_name: &str) -> Option<&'static str> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_group_returns_none() {
        assert_eq!(lookup("definitely-not-a-real-group"), None);
    }
}
