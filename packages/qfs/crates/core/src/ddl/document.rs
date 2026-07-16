//! The `.qfs` **document splitter** — the one chunker every `.qfs`-reading surface shares.
//!
//! A `.qfs` document (a server boot config, a provisioning source-of-truth document, a
//! `connections.qfs`) is a `;`-separated sequence of statements. Splitting it looks trivial and is
//! not: `--`, `#` and `;` all mean different things depending on whether they sit at a token
//! boundary, inside a `'…'` string, or inside a `/`-led path token. Getting that wrong does not
//! produce a tidy error — it silently truncates a statement, swallows its terminating `;`, and
//! merges the next statement into it.
//!
//! So this splitter does not re-derive the lexicon: it **runs the lexer** ([`qfs_lang::lex`]) and
//! splits on the top-level [`Token::Semicolon`] tokens it emits. Every rule about comments,
//! strings, escapes and path tokens is therefore the lexer's, by construction rather than by
//! imitation — including the ones an imitation reliably gets wrong:
//!
//! - `-` is **not** a path delimiter ([`qfs_lang::lex`]'s `is_path_delimiter`), so `--` inside a
//!   path token (`/local/a--b.txt`) is path text, not a comment. Outside a path it is a comment
//!   even when glued to the previous token, because the lexer tests for a comment at each token
//!   boundary.
//! - Whether a `/` opens a path at all depends on the *preceding token stream* (`slash_starts_path`
//!   consults the previous token and a keyword table). No line-local scanner can answer that.
//! - `;` is not a path delimiter either, so a `;` inside a path token does not end a statement.
//!
//! # Why the lexer rather than a hand-rolled scanner
//! Two hand-rolled scanners preceded this one and disagreed with each other on the same text. The
//! rules above are the reason a third would have disagreed too: mirroring `slash_starts_path`
//! means duplicating a private keyword table and tracking the token stream, which is a lexer with
//! extra steps. `qfs-core` already depends on `qfs-lang`, so there is no cost to using the real one.
//!
//! # Line attribution
//! Each statement is attributed to the 1-based line of its **first token** — not its first
//! non-blank byte — so a leading comment never steals the attribution. Both loaders build
//! line-located errors from it.

use qfs_lang::lex::lex;
use qfs_lang::token::Token;

/// A document that could not be tokenized, located on a line.
///
/// Carries the same `code`/`message` shape the parser produces for a lex failure, so a caller maps
/// it onto its own line-located parse error without inventing wording.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SplitError {
    /// 1-based line the offending byte sits on.
    pub line: usize,
    /// Stable classification (`UNTERMINATED_STRING`, `BAD_ESCAPE`, …).
    pub code: String,
    /// Human-readable detail.
    pub message: String,
}

/// The 1-based line the byte at `offset` sits on.
fn line_of(source: &str, offset: usize) -> usize {
    let end = offset.min(source.len());
    1 + source[..end].bytes().filter(|b| *b == b'\n').count()
}

/// Split a `.qfs` document into its statements, each with the 1-based line of its first token.
///
/// Statements are separated by top-level `;`. Comments and blank runs between statements are not
/// returned; a `--`/`#`/`;` inside a string or a path token is content, never punctuation. A
/// trailing statement without a closing `;` is returned; empty runs (`;;`) are skipped.
///
/// # Errors
/// [`SplitError`] when the document does not tokenize. Because the lexer reads the whole document,
/// a lex failure anywhere means no statements are returned — a broken document applies nothing
/// rather than applying its prefix and then failing.
pub fn split_document(source: &str) -> Result<Vec<(usize, String)>, SplitError> {
    let tokens = lex(source).map_err(|e| SplitError {
        line: line_of(source, e.span.start as usize),
        code: e.kind.as_str().to_string(),
        message: format!("lexing failed: {}", e.kind.as_str()),
    })?;

    let mut out = Vec::new();
    // Byte range of the current statement: from its first token's start to its last token's end.
    let mut first: Option<usize> = None;
    let mut last_end: usize = 0;

    for t in &tokens {
        if matches!(t.node, Token::Semicolon) {
            if let Some(start) = first.take() {
                push_stmt(source, start, last_end, &mut out);
            }
            continue;
        }
        if first.is_none() {
            first = Some(t.span.start as usize);
        }
        last_end = t.span.end as usize;
    }
    if let Some(start) = first {
        push_stmt(source, start, last_end, &mut out);
    }
    Ok(out)
}

/// Push `source[start..end]` as a statement attributed to the line `start` sits on.
fn push_stmt(source: &str, start: usize, end: usize, out: &mut Vec<(usize, String)>) {
    let text = source[start..end].trim();
    if !text.is_empty() {
        out.push((line_of(source, start), text.to_string()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_on_top_level_semicolons_and_attributes_lines() {
        let src = "CREATE VIEW v AS /local/a.txt;\nCREATE VIEW w AS /local/b.txt;\n";
        let got = split_document(src).unwrap();
        assert_eq!(
            got,
            vec![
                (1, "CREATE VIEW v AS /local/a.txt".to_string()),
                (2, "CREATE VIEW w AS /local/b.txt".to_string()),
            ]
        );
    }

    #[test]
    fn a_double_dash_inside_an_unquoted_path_is_path_text_not_a_comment() {
        // The headline defect: the old line-local stripper cut at the `--`, swallowing the `;`
        // and merging the next statement into this one.
        let src = "CREATE VIEW v AS /local/data/a--b.txt;\nCREATE VIEW w AS /local/c.txt;\n";
        let got = split_document(src).unwrap();
        assert_eq!(got.len(), 2, "the `;` must still terminate statement 1");
        assert_eq!(got[0].1, "CREATE VIEW v AS /local/data/a--b.txt");
        assert_eq!(got[1], (2, "CREATE VIEW w AS /local/c.txt".to_string()));
    }

    #[test]
    fn a_double_dash_inside_a_quoted_locator_is_literal() {
        let src = "CREATE CONNECTION x DRIVER sqlite AT '/data/a--b.db';";
        let got = split_document(src).unwrap();
        assert_eq!(got.len(), 1);
        assert!(got[0].1.contains("'/data/a--b.db'"));
    }

    #[test]
    fn a_semicolon_inside_a_quoted_locator_does_not_split() {
        let src =
            "CREATE CONNECTION x DRIVER sqlite AT '/data/a;b.db';\nCREATE VIEW w AS /local/c.txt;";
        let got = split_document(src).unwrap();
        assert_eq!(got.len(), 2);
        assert!(got[0].1.contains("'/data/a;b.db'"));
    }

    #[test]
    fn a_trailing_hash_comment_containing_a_semicolon_does_not_split() {
        // The old stripper only honoured `#` at the start of a trimmed line, so the `;` inside this
        // trailing comment split off a bogus statement and raised UNEXPECTED_EOF.
        let src = "CREATE VIEW v AS /local/c.txt; # note; more\n";
        let got = split_document(src).unwrap();
        assert_eq!(got, vec![(1, "CREATE VIEW v AS /local/c.txt".to_string())]);
    }

    #[test]
    fn a_trailing_double_dash_comment_containing_a_semicolon_does_not_split() {
        let src = "CREATE VIEW v AS /local/c.txt; -- note; more\n";
        let got = split_document(src).unwrap();
        assert_eq!(got, vec![(1, "CREATE VIEW v AS /local/c.txt".to_string())]);
    }

    #[test]
    fn a_whole_line_hash_comment_does_not_steal_the_line_attribution() {
        let src = "# a comment with ; inside\nCREATE VIEW v AS /local/c.txt;\n";
        let got = split_document(src).unwrap();
        assert_eq!(got, vec![(2, "CREATE VIEW v AS /local/c.txt".to_string())]);
    }

    #[test]
    fn an_escaped_quote_does_not_desynchronise_quote_tracking() {
        // The previous quote-aware scanner toggled on every `'`, so a `\'` escape flipped it back
        // and everything after read as unquoted.
        let src = "CREATE CONNECTION x DRIVER sqlite AT '/data/o\\'brien--x.db';\nCREATE VIEW w AS /local/c.txt;";
        let got = split_document(src).unwrap();
        assert_eq!(got.len(), 2, "the escape must not end the string early");
        assert_eq!(got[1], (2, "CREATE VIEW w AS /local/c.txt".to_string()));
    }

    #[test]
    fn a_statement_without_a_trailing_semicolon_is_returned() {
        let got = split_document("CREATE VIEW v AS /local/c.txt").unwrap();
        assert_eq!(got, vec![(1, "CREATE VIEW v AS /local/c.txt".to_string())]);
    }

    #[test]
    fn empty_and_comment_only_documents_yield_no_statements() {
        assert!(split_document("").unwrap().is_empty());
        assert!(split_document("\n\n").unwrap().is_empty());
        assert!(split_document("# just a note\n-- and another\n")
            .unwrap()
            .is_empty());
        assert!(split_document(";;").unwrap().is_empty());
    }

    #[test]
    fn a_multi_line_statement_is_attributed_to_its_first_token() {
        let src = "\n\nCREATE VIEW v\n  AS /local/c.txt;\n";
        let got = split_document(src).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].0, 3);
    }

    #[test]
    fn server_config_vocabulary_splits_with_comments_and_semicolons() {
        // Moved from the server crate when the splitter was unified into core: a `;` inside a
        // comment must not split, whole-line and trailing comments are skipped, and the reported
        // line is where the statement's content starts.
        let text =
            "# a comment with ; inside\nCREATE POLICY p; -- trailing\nCREATE JOB j EVERY '1h';";
        let stmts = split_document(text).unwrap();
        assert_eq!(stmts.len(), 2, "two statements, comment `;` ignored");
        assert_eq!(stmts[0].0, 2, "first statement starts on line 2");
        assert!(stmts[0].1.starts_with("CREATE POLICY"));
        assert!(stmts[1].1.starts_with("CREATE JOB"));
    }

    #[test]
    fn a_lex_failure_is_reported_on_its_line() {
        let src = "CREATE VIEW v AS /local/a.txt;\nCREATE CONNECTION x DRIVER sqlite AT '/unterminated;\n";
        let err = split_document(src).unwrap_err();
        assert_eq!(err.line, 2);
        assert_eq!(err.code, "UNTERMINATED_STRING");
        assert!(err.message.contains("lexing failed"));
    }
}
