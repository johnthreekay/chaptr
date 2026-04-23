//! Shared lexical layer (L1) for both `manga` and `novel`.
//!
//! Tokenizes a filename into bracket/paren/curly spans plus runs of word
//! characters separated by single delimiter chars. The lexer doesn't interpret
//! anything — `Vol`, `c042`, and `epub` are all just `Word` tokens at this
//! layer; recognizing them as volume marker / chapter marker / extension is L2's
//! job.
//!
//! Per the project's #24 lesson, **never duplicate lexer logic across domains** —
//! a regression here must be regression-tested against both manga and LN corpora.

/// A lexical token from a filename.
///
/// Spans are zero-copy slices into the input — `Token<'a>` carries the input lifetime.
/// Bracket/paren/curly variants exclude the surrounding markers from their slice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Token<'a> {
    /// Content inside `[...]`, exclusive of the brackets themselves.
    Bracketed(&'a str),
    /// Content inside `(...)`, exclusive of the parens themselves.
    Parenthesized(&'a str),
    /// Content inside `{...}`, exclusive of the curlies themselves.
    /// Used in fixtures for revision (`{r2}`) and edition (`{Special Edition}`) tags.
    Curly(&'a str),
    /// A run of word characters: alphanumeric (Unicode), apostrophe, `!`, `?`.
    /// Underscore and hyphen are *delimiters*, not word chars — corpus filenames
    /// commonly use `_` as a space substitute and `-` to separate ranges.
    Word(&'a str),
    /// Any single non-word, non-bracket character: space, `.`, `_`, `-`, `:`, `,`, etc.
    Delimiter(char),
}

/// Tokenize a filename into a flat `Vec<Token>`.
///
/// **Bracket recovery on EOF**: an unclosed `[`, `(`, or `{` consumes the rest
/// of the input and is emitted as the corresponding bracket-shaped variant
/// without its closing marker. This is forgiving but lossy at round-trip — the
/// closing marker has to be synthesized, which is fine for parsing intent but
/// means the round-trip property only holds for well-formed inputs. Corpus
/// filenames in the wild are well-formed, so this matters mostly for fuzz
/// inputs and adversarial cases.
#[must_use]
pub fn tokenize(input: &str) -> Vec<Token<'_>> {
    let mut tokens = Vec::new();
    let mut chars = input.char_indices().peekable();

    while let Some((i, c)) = chars.next() {
        match c {
            '[' => tokens.push(Token::Bracketed(consume_until(input, &mut chars, i, ']'))),
            '(' => tokens.push(Token::Parenthesized(consume_until(
                input, &mut chars, i, ')',
            ))),
            '{' => tokens.push(Token::Curly(consume_until(input, &mut chars, i, '}'))),
            c if is_word_char(c) => {
                let start = i;
                let mut end = i + c.len_utf8();
                while let Some(&(j, c)) = chars.peek() {
                    if is_word_char(c) {
                        end = j + c.len_utf8();
                        chars.next();
                    } else {
                        break;
                    }
                }
                tokens.push(Token::Word(&input[start..end]));
            }
            _ => tokens.push(Token::Delimiter(c)),
        }
    }

    tokens
}

/// Consume from the iterator up to and including `closer`, returning the slice
/// between the opening marker (at `open_idx`) and the closer (exclusive).
/// On EOF without finding `closer`, returns the slice to end-of-input.
fn consume_until<'a, I>(
    input: &'a str,
    chars: &mut std::iter::Peekable<I>,
    open_idx: usize,
    closer: char,
) -> &'a str
where
    I: Iterator<Item = (usize, char)>,
{
    let start = open_idx + 1; // skip the opener byte (always 1 byte for ASCII brackets)
    let mut end = input.len();
    for (j, c) in chars.by_ref() {
        if c == closer {
            end = j;
            break;
        }
    }
    &input[start..end]
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '\'' | '!' | '?')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(input: &str) -> String {
        tokenize(input)
            .iter()
            .map(|t| match t {
                Token::Bracketed(s) => format!("[{s}]"),
                Token::Parenthesized(s) => format!("({s})"),
                Token::Curly(s) => format!("{{{s}}}"),
                Token::Word(s) => (*s).to_string(),
                Token::Delimiter(c) => c.to_string(),
            })
            .collect()
    }

    #[test]
    fn empty_returns_empty() {
        assert!(tokenize("").is_empty());
    }

    #[test]
    fn single_word() {
        assert_eq!(tokenize("hello"), vec![Token::Word("hello")]);
    }

    #[test]
    fn words_separated_by_space() {
        assert_eq!(
            tokenize("foo bar"),
            vec![
                Token::Word("foo"),
                Token::Delimiter(' '),
                Token::Word("bar")
            ]
        );
    }

    #[test]
    fn brackets_paren_curly() {
        assert_eq!(tokenize("[a]"), vec![Token::Bracketed("a")]);
        assert_eq!(tokenize("(b)"), vec![Token::Parenthesized("b")]);
        assert_eq!(tokenize("{c}"), vec![Token::Curly("c")]);
    }

    #[test]
    fn empty_brackets() {
        assert_eq!(tokenize("[]"), vec![Token::Bracketed("")]);
        assert_eq!(tokenize("()"), vec![Token::Parenthesized("")]);
        assert_eq!(tokenize("{}"), vec![Token::Curly("")]);
    }

    #[test]
    fn underscore_and_hyphen_are_delimiters() {
        // Corpus uses `_` as space-substitute and `-` for ranges. Both must
        // split words rather than join them.
        assert_eq!(
            tokenize("foo_bar-baz"),
            vec![
                Token::Word("foo"),
                Token::Delimiter('_'),
                Token::Word("bar"),
                Token::Delimiter('-'),
                Token::Word("baz"),
            ]
        );
    }

    #[test]
    fn dot_is_delimiter_so_decimals_split() {
        // L2 reconstructs `4.5` as a decimal chapter — the lexer doesn't.
        // Pin this so a future "be smart about decimals" lexer change
        // surfaces here as a test failure rather than silently changing
        // ChapterNumber parsing semantics.
        assert_eq!(
            tokenize("4.5"),
            vec![Token::Word("4"), Token::Delimiter('.'), Token::Word("5"),]
        );
    }

    #[test]
    fn punctuation_in_titles_stays_with_word() {
        // `Toradora!` and `BTOOOM!` carry the `!` as part of the title; same
        // for `?`. Apostrophes in `Don't` likewise stay attached.
        assert_eq!(tokenize("Toradora!"), vec![Token::Word("Toradora!")]);
        assert_eq!(tokenize("Don't"), vec![Token::Word("Don't")]);
    }

    #[test]
    fn cjk_letters_are_word_chars() {
        assert_eq!(tokenize("漫画"), vec![Token::Word("漫画")]);
        assert_eq!(
            tokenize("漫画 巻1"),
            vec![
                Token::Word("漫画"),
                Token::Delimiter(' '),
                Token::Word("巻1"),
            ]
        );
    }

    #[test]
    fn full_filename() {
        let tokens = tokenize("[Yen Press] Title v01 (Digital).epub");
        assert_eq!(
            tokens,
            vec![
                Token::Bracketed("Yen Press"),
                Token::Delimiter(' '),
                Token::Word("Title"),
                Token::Delimiter(' '),
                Token::Word("v01"),
                Token::Delimiter(' '),
                Token::Parenthesized("Digital"),
                Token::Delimiter('.'),
                Token::Word("epub"),
            ]
        );
    }

    #[test]
    fn unclosed_bracket_consumes_to_eof() {
        // Forgiving recovery — emit what we have. Round-trip won't be exact
        // for these inputs (the synthesized closer adds a char), see
        // `unclosed_bracket_round_trip_is_lossy` below.
        assert_eq!(tokenize("[Foo"), vec![Token::Bracketed("Foo")]);
        assert_eq!(tokenize("(Bar"), vec![Token::Parenthesized("Bar")]);
        assert_eq!(tokenize("{Baz"), vec![Token::Curly("Baz")]);
    }

    #[test]
    fn round_trip_is_lossless_for_well_formed_input() {
        for input in [
            "[Yen Press] Sword Art Online Vol 10 (Digital).epub",
            "[Suihei Kiki]_Kasumi_Otoko_no_Ko_[Taruby]_v1.1.zip",
            "Mokushiroku Alice Vol. 1 Ch. 4: Misrepresentation",
            "[Unpaid Ferryman] Youjo Senki | The Saga of Tanya the Evil v01-23 (2018-2024) (Digital) (LuCaZ)",
            "Sword Art Online Vol 10 - Alicization Running [Yen Press] [LuCaZ] {r2}.epub",
            "漫画 巻1",
            "",
        ] {
            assert_eq!(round_trip(input), input, "round-trip failed for {input:?}");
        }
    }

    #[test]
    fn unclosed_bracket_round_trip_is_lossy() {
        // Document the asymmetry: unclosed brackets get a synthesized closer
        // on round-trip. This is by design — the alternative is dropping
        // tokens, which loses more information.
        assert_eq!(round_trip("[Foo"), "[Foo]");
    }

    /// Smoke-test against the LN corpus snapshot. Asserts only that
    /// tokenization completes and produces non-empty output for every
    /// non-trivial line — no specific token-shape assertions.
    #[test]
    fn smoke_corpus_tokenizes_without_panic() {
        const CORPUS: &str = include_str!("../corpus/smoke_novel.txt");
        let mut lines_processed = 0usize;
        for line in CORPUS.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let tokens = tokenize(line);
            // Any non-trivial input should produce at least one token.
            assert!(!tokens.is_empty(), "tokenize returned empty for: {line:?}");
            lines_processed += 1;
        }
        assert!(
            lines_processed > 100,
            "smoke corpus shrank suspiciously: {lines_processed} lines"
        );
    }
}
