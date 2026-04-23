//! Filename tokenizer for manga, manhwa, manhua, and light novels.
//!
//! `chaptr` exposes two domain entry points — [`manga::parse`] and [`novel::parse`] —
//! built on a shared lexical layer ([`lexer`]). Inputs are filename strings; outputs
//! are zero-allocation structs whose string fields borrow from the input.
//!
//! No I/O, no network, no archive reading. Sidecar parsing (ComicInfo.xml, OPF) is
//! deliberately out of scope and belongs one layer up in the consumer.

pub mod lexer;
pub mod manga;
pub mod novel;
pub mod tables;

/// A chapter or volume number.
///
/// Stored as `(whole, Option<decimal>)` rather than `f64` because:
/// - `c42.5` (omake), `c42v2` (revision), and `c42.5v2` must all be distinguishable
///   and exactly comparable. f64 round-trips like `0.1 + 0.2 != 0.3` make total
///   `Ord` uncomfortable for sort keys and DB primary keys.
/// - The decimal component fits comfortably in `u16` — three-digit decimals
///   (`c42.125`) exist but no real corpus filename has been seen with more.
///
/// Comparison is lexicographic over `(whole, decimal)`. `None` decimal sorts
/// *before* `Some(0)`, which matches "c42 comes before c42.0" — a degenerate case
/// in practice but worth nailing down.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct ChapterNumber {
    pub whole: u32,
    pub decimal: Option<u16>,
}

impl ChapterNumber {
    #[must_use]
    pub const fn new(whole: u32) -> Self {
        Self {
            whole,
            decimal: None,
        }
    }

    #[must_use]
    pub const fn with_decimal(whole: u32, decimal: u16) -> Self {
        Self {
            whole,
            decimal: Some(decimal),
        }
    }
}

/// A single chapter/volume number or an inclusive range (`c001-050`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NumberRange {
    pub start: ChapterNumber,
    pub end: Option<ChapterNumber>,
}

impl NumberRange {
    #[must_use]
    pub const fn single(n: ChapterNumber) -> Self {
        Self {
            start: n,
            end: None,
        }
    }

    #[must_use]
    pub const fn range(start: ChapterNumber, end: ChapterNumber) -> Self {
        Self {
            start,
            end: Some(end),
        }
    }

    #[must_use]
    pub const fn is_range(&self) -> bool {
        self.end.is_some()
    }
}

/// Languages detected from filename tags (`[EN]`, `[JP]`, `(English)`, `(Raw)`, etc.).
///
/// Intentionally narrow — only languages observed in the corpus get variants. Add as
/// the corpus grows; do not pre-populate from ISO-639 since most entries would never
/// appear in practice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Language {
    English,
    Japanese,
    SimplifiedChinese,
    TraditionalChinese,
    Korean,
    Spanish,
    French,
    German,
    Italian,
    Portuguese,
    Russian,
    Indonesian,
    Vietnamese,
    Thai,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chapter_number_orders_lexicographically() {
        let a = ChapterNumber::new(42);
        let b = ChapterNumber::with_decimal(42, 5);
        let c = ChapterNumber::new(43);
        assert!(a < b);
        assert!(b < c);
    }

    #[test]
    fn chapter_number_none_decimal_sorts_before_some_zero() {
        // None < Some(0) per the doc comment on ChapterNumber. Pin the invariant.
        let bare = ChapterNumber::new(42);
        let zero = ChapterNumber::with_decimal(42, 0);
        assert!(bare < zero);
    }

    #[test]
    fn number_range_single_vs_range() {
        let single = NumberRange::single(ChapterNumber::new(5));
        let range = NumberRange::range(ChapterNumber::new(1), ChapterNumber::new(50));
        assert!(!single.is_range());
        assert!(range.is_range());
    }
}
