//! Shared lexical layer (L1) for both `manga` and `novel`.
//!
//! Tokenizes a filename into delimiter-bounded spans without interpreting them.
//! Both domain parsers consume the same `Token` stream and apply their own L2+
//! classification on top.
//!
//! Per the project's #24 lesson, **never duplicate lexer logic across domains** —
//! a regression here must be regression-tested against both manga and LN corpora.

/// A lexical token from a filename.
///
/// Spans are zero-copy slices into the input — `Token<'a>` carries the input lifetime.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Token<'a> {
    /// Content inside `[...]` brackets, exclusive of the brackets themselves.
    Bracketed(&'a str),
    /// Content inside `(...)` parens, exclusive of the parens themselves.
    Parenthesized(&'a str),
    /// A run of word characters (letters, digits, underscore).
    Word(&'a str),
    /// A delimiter character — space, dot, dash, underscore, etc.
    Delimiter(char),
}

/// Tokenize a filename into a flat `Vec<Token>`.
///
/// **Stub** — returns an empty vector until the corpus pass is done and the L1
/// grammar is settled. See the design doc's "Implementation ordering" section.
#[must_use]
pub fn tokenize(_input: &str) -> Vec<Token<'_>> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenize_empty_returns_empty() {
        assert!(tokenize("").is_empty());
    }
}
