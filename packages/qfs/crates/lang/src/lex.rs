//! The qfs lexer: a pure `&str -> Result<Vec<Spanned<Token>>, LexError>` scanner.
//!
//! This is the first stage of the language core (blueprint §2.2 "Pipe-SQL", §3 "closed
//! core"). It is **pure**: no I/O, no `World`, referentially transparent — the
//! foundation of the dry-runnable pipeline (blueprint §3 purity invariant). It is a
//! hand-written byte/char cursor rather than a combinator parser, so `qfs-lang`
//! keeps **zero dependencies** and stays trivially `wasm32`-clean (B7); the t04
//! parser (winnow) consumes the token stream this produces.
//!
//! ## Documented lexing decisions (blueprint §3/§4, t03 "Considerations")
//! * **Path vs. division/glob.** A `/` at a statement/operator boundary starts a
//!   [`Token::Path`]; a `/` after an expression operand is [`Token::Slash`]. Glob
//!   chars (`*`, `?`) inside a path segment set the segment's `glob` flag; raw segment
//!   text is preserved.
//! * **Quoted path segments (ticket 20260717120200).** A segment written `'…'` carries any
//!   character literally — spaces, `?`, `#`, `&`, `(`, Unicode — so a real-world file name is
//!   addressable as a single-file path (`/drive/my/'Q3 budget (final)?.xlsx'`). The one escape
//!   is a doubled quote (`''` → `'`); a quoted segment never sets the `glob` flag (a quoted `?`
//!   is a literal character, not a wildcard); a `/` inside quotes is refused, because the
//!   rendered path carries raw segment names and every driver re-splits it on `/`. Unquoted
//!   segments are untouched — the form is additive to the frozen grammar.
//! * **Size literal `25 MB`.** A bare integer immediately followed (across
//!   whitespace) by an uppercase word in the [`SizeUnit`] set folds into a
//!   [`Token::Size`]. Any other following word is left as a separate token, so a
//!   column literally named `MB` is unaffected.
//! * **Typed literal `DATE '…'`.** The introducer words `DATE`/`TIME`/`TIMESTAMP`
//!   immediately followed by a string literal fold into a [`Token::TypedLit`]; the
//!   inner string is captured raw and **not** validated (parser/runtime concern).
//!   A bare `DATE` not followed by a string stays an identifier.
//! * **`@version` in paths.** `@` binds to the preceding path segment; the raw ref
//!   text (git ref, S3 versionId, drive rev — blueprint §4) is preserved without
//!   interpretation.
//! * **Keyword case (t74, decision S).** Closed-core keywords are recognized
//!   **case-insensitively** and are canonically **lowercase** (`where`, `select`,
//!   `insert into`, …): the word lexer folds a word's case before matching
//!   [`Keyword::from_word`], so `SELECT`/`Select`/`select` all lex to the same
//!   [`Token::Keyword`]. Identifiers, paths, and literals stay case-sensitive data.
//! * **Multi-word keywords.** `group by`, `insert into`, … are emitted as separate
//!   adjacent tokens (their lead words surface as [`Token::Ident`]); composition is
//!   the parser's job (blueprint §3).
//!
//! ## Security note (no-live-creds by construction)
//! The lexer must never be fed credential material: errors quote source spans, so
//! anything in the source could surface in a diagnostic. There are no creds, no
//! network, and no filesystem access anywhere in this module.

use crate::error::{LexError, LexErrorKind};
use crate::keywords::Keyword;
use crate::span::{Span, Spanned};
use crate::token::{literal_word, LitType, PathSeg, SizeUnit, Token};

/// Tokenize one qfs statement into a flat, spanned token stream.
///
/// Pure and panic-free: arbitrary UTF-8 input yields `Ok(tokens)` or a single
/// [`LexError`] with a byte span — never a panic or abort. Every emitted token's
/// span slices back to the exact originating source substring (round-trip
/// invariant), which is load-bearing for the AI structured-error path (blueprint §6).
///
/// # Errors
/// Returns a [`LexError`] on an unterminated string, a bad escape, a malformed
/// number, or a stray character that cannot begin any token.
///
/// ```
/// use qfs_lang::lex::lex;
/// use qfs_lang::token::Token;
/// let toks = lex("WHERE mail").expect("valid");
/// assert_eq!(toks.len(), 2);
/// assert!(matches!(toks[1].node, Token::Ident(ref s) if s == "mail"));
/// ```
pub fn lex(src: &str) -> Result<Vec<Spanned<Token>>, LexError> {
    Lexer::new(src).run()
}

/// A char with its starting byte offset in the source.
#[derive(Clone, Copy)]
struct Ch {
    /// Byte offset of this char's first byte.
    at: usize,
    /// The decoded character.
    c: char,
}

struct Lexer {
    /// All `(byte_offset, char)` pairs of the source, indexed by char position.
    chars: Vec<Ch>,
    /// Current char-position cursor into `chars`.
    pos: usize,
    /// Total source byte length (the end offset for the final char/EOF).
    byte_len: usize,
    /// Whether whitespace before the next token crossed a logical statement line.
    after_line_break: bool,
    out: Vec<Spanned<Token>>,
}

impl Lexer {
    fn new(src: &str) -> Self {
        let chars: Vec<Ch> = src.char_indices().map(|(at, c)| Ch { at, c }).collect();
        Self {
            chars,
            pos: 0,
            byte_len: src.len(),
            after_line_break: true,
            out: Vec::new(),
        }
    }

    /// The char at `pos`, if any.
    fn peek(&self) -> Option<Ch> {
        self.chars.get(self.pos).copied()
    }

    /// The char at `pos + n`, if any.
    fn peek_n(&self, n: usize) -> Option<Ch> {
        self.chars.get(self.pos + n).copied()
    }

    /// Advance past one char.
    fn bump(&mut self) {
        self.pos += 1;
    }

    /// The byte offset of the char at `pos` (or `byte_len` at EOF).
    fn byte_at(&self) -> usize {
        self.chars.get(self.pos).map_or(self.byte_len, |ch| ch.at)
    }

    /// Push a token spanning `[start_byte, current_byte)`.
    fn push(&mut self, start_byte: usize, node: Token) {
        let end = self.byte_at();
        // Byte offsets are bounded by source length, which the workspace keeps
        // small (single statement); the `as u32` cast is safe for any realistic
        // input and saturates rather than wraps if somehow exceeded.
        let span = Span::new(
            u32::try_from(start_byte).unwrap_or(u32::MAX),
            u32::try_from(end).unwrap_or(u32::MAX),
        );
        self.out.push(Spanned::new(node, span));
        self.after_line_break = false;
    }

    /// Build a one-or-two-byte error span starting at `start_byte`.
    fn err(&self, start_byte: usize, kind: LexErrorKind) -> LexError {
        let end = self.byte_at();
        let end = if end > start_byte { end } else { self.byte_len };
        LexError::new(
            Span::new(
                u32::try_from(start_byte).unwrap_or(u32::MAX),
                u32::try_from(end).unwrap_or(u32::MAX),
            ),
            kind,
        )
    }

    fn run(mut self) -> Result<Vec<Spanned<Token>>, LexError> {
        while let Some(ch) = self.peek() {
            if ch.c.is_whitespace() {
                if ch.c == '\n' || ch.c == '\r' {
                    self.after_line_break = true;
                }
                self.bump();
                continue;
            }
            // Comments: line `--`, line `#`. Span-tracked (skipped) — spans of
            // emitted tokens never overlap a comment, satisfying the round-trip
            // invariant for the tokens that are produced.
            if ch.c == '#' {
                self.skip_line_comment();
                continue;
            }
            if ch.c == '-' && self.peek_n(1).is_some_and(|n| n.c == '-') {
                self.skip_line_comment();
                continue;
            }
            match ch.c {
                '/' if self.slash_starts_path() => self.lex_path()?,
                '\'' => self.lex_string()?,
                c if c.is_ascii_digit() => self.lex_number()?,
                c if is_ident_start(c) => self.lex_word()?,
                _ => self.lex_symbol()?,
            }
        }
        Ok(self.out)
    }

    /// Skip a line comment to end-of-line (the newline is left for the ws skip).
    fn skip_line_comment(&mut self) {
        while let Some(ch) = self.peek() {
            if ch.c == '\n' {
                break;
            }
            self.bump();
        }
    }

    /// Lex a `/`-led path: `/seg/seg(@ver)`, with glob flags per segment. A segment written
    /// `'…'` is a **quoted segment** ([`Self::consume_quoted_path_segment_name`]) whose content
    /// is wholly literal; anything else lexes exactly as it always has.
    fn lex_path(&mut self) -> Result<(), LexError> {
        let start = self.byte_at();
        let mut segs: Vec<PathSeg> = Vec::new();
        // Consume the leading '/' and each segment.
        while self.peek().is_some_and(|ch| ch.c == '/') {
            self.bump(); // consume '/'
            let (name, glob) = if self.peek().is_some_and(|ch| ch.c == '\'') {
                // A quoted segment is a literal NAME: never a glob, whatever it contains.
                (self.consume_quoted_path_segment_name()?, false)
            } else {
                self.consume_path_segment_name()
            };
            let version = self.consume_path_version();
            // An empty leading segment (e.g. trailing '/') is preserved as an
            // empty name so spans round-trip; the parser validates structure.
            segs.push(PathSeg::new(name, version, glob));
        }
        self.push(start, Token::Path(segs));
        Ok(())
    }

    /// Consume a **quoted path segment** name — `'…'` (ticket 20260717120200).
    ///
    /// Inside the quotes every character is literal: spaces, `?`, `*`, `#`, `&`, parentheses and
    /// Unicode all belong to the NAME. Real Drive/mail/filesystem names routinely carry them, and
    /// without this form such a file simply could not be written as a single-file path — the
    /// statement died in the lexer with `UNEXPECTED_CHAR` before parsing began, which is what
    /// pushed the operator onto the `remove <folder> where name == '…'` detour that then hit the
    /// over-delete bug (ticket 20260717102000). Unquoted segments lex exactly as before, so this
    /// is purely additive to the frozen grammar.
    ///
    /// The only escape is the SQL-style **doubled quote** (`''` → one literal `'`). There is
    /// deliberately no backslash escape: a segment is a name, not prose, so every `\` stays a
    /// literal `\` and a given name has exactly one spelling.
    ///
    /// A quoted segment never sets the glob flag — that is the whole point of the form:
    /// `/drive/my/'report?.pdf'` addresses the ONE file whose name contains `?`, while
    /// `/drive/my/report?.pdf` still globs, unchanged.
    ///
    /// # Errors
    /// [`LexErrorKind::PathSeparatorInQuotedSegment`] on a `/` inside the quotes — the separator
    /// is structural and every driver re-splits the rendered path on it (the rendered form
    /// carries raw names, not quotes), so such a segment could not survive the trip to a driver.
    /// [`LexErrorKind::UnterminatedString`] when the closing quote never arrives.
    /// [`LexErrorKind::UnexpectedChar`] when a token is glued to the closing quote.
    fn consume_quoted_path_segment_name(&mut self) -> Result<String, LexError> {
        let open = self.byte_at();
        self.bump(); // consume the opening quote
        let mut name = String::new();
        loop {
            let Some(ch) = self.peek() else {
                return Err(self.err(open, LexErrorKind::UnterminatedString));
            };
            match ch.c {
                '\'' => {
                    // A doubled quote is one literal quote; a lone quote closes the segment.
                    self.bump();
                    if self.peek().is_some_and(|n| n.c == '\'') {
                        name.push('\'');
                        self.bump();
                        continue;
                    }
                    self.reject_token_glued_to_quoted_segment()?;
                    return Ok(name);
                }
                '/' => return Err(self.err(ch.at, LexErrorKind::PathSeparatorInQuotedSegment)),
                _ => {
                    name.push(ch.c);
                    self.bump();
                }
            }
        }
    }

    /// After a quoted segment's closing quote only a separator (`/`), a version pin (`@`), or a
    /// token boundary may follow. Without this guard `/x/'a'b` would quietly lex as the path
    /// `/x/a` plus a stray identifier `b` — silently addressing something other than what was
    /// written. Refusing is the honest answer.
    fn reject_token_glued_to_quoted_segment(&mut self) -> Result<(), LexError> {
        if let Some(ch) = self.peek() {
            if ch.c != '/' && ch.c != '@' && !is_path_delimiter(ch.c) {
                let at = ch.at;
                self.bump();
                return Err(self.err(at, LexErrorKind::UnexpectedChar(ch.c)));
            }
        }
        Ok(())
    }

    /// Consume one path segment name (until `/`, `@`, or a delimiter); report
    /// whether it contained a glob char. A `?` additionally opens **query-string mode** for the
    /// rest of the segment: `=` and `&` — otherwise statement delimiters — become part of the
    /// name, so a wire path may carry a query suffix (`/files/{file}?create_download_url=1`,
    /// blueprint §13). Glob-`?` usage is unaffected (a glob segment carries no `=`).
    fn consume_path_segment_name(&mut self) -> (String, bool) {
        let mut name = String::new();
        let mut glob = false;
        let mut in_query = false;
        while let Some(ch) = self.peek() {
            if ch.c == '/' || ch.c == '@' {
                break;
            }
            if is_path_delimiter(ch.c) && !(in_query && (ch.c == '=' || ch.c == '&')) {
                break;
            }
            if ch.c == '*' || ch.c == '?' {
                glob = true;
            }
            if ch.c == '?' {
                in_query = true;
            }
            name.push(ch.c);
            self.bump();
        }
        (name, glob)
    }

    /// Consume an optional `@version` ref bound to the current segment.
    fn consume_path_version(&mut self) -> Option<String> {
        if self.peek().is_none_or(|ch| ch.c != '@') {
            return None;
        }
        self.bump(); // consume '@'
        let mut ver = String::new();
        while let Some(ch) = self.peek() {
            // The ref runs until the next path separator or a delimiter. Dots are allowed
            // (`@v1.2`), and so is `~` — a git relative-ref char (`@HEAD~1`, `@main~2`) that is
            // otherwise the `~` match operator; inside a path-version run it is part of the ref, not
            // an operator. (`^`, `{`, `}` are already non-delimiters, so `@HEAD^`, `@v1.2^{}` lex.)
            if ch.c == '/' || (is_path_delimiter(ch.c) && ch.c != '~') {
                break;
            }
            ver.push(ch.c);
            self.bump();
        }
        Some(ver)
    }

    /// Lex a single-quoted string literal with escape handling.
    fn lex_string(&mut self) -> Result<(), LexError> {
        let start = self.byte_at();
        self.bump(); // consume opening quote
        let mut value = String::new();
        loop {
            let Some(ch) = self.peek() else {
                return Err(self.err(start, LexErrorKind::UnterminatedString));
            };
            match ch.c {
                '\'' => {
                    self.bump(); // consume closing quote
                    self.push(start, Token::Str(value));
                    return Ok(());
                }
                '\\' => {
                    self.bump(); // consume backslash
                    let Some(esc) = self.peek() else {
                        return Err(self.err(start, LexErrorKind::UnterminatedString));
                    };
                    let decoded = match esc.c {
                        'n' => '\n',
                        't' => '\t',
                        'r' => '\r',
                        '\\' => '\\',
                        '\'' => '\'',
                        '0' => '\0',
                        _ => {
                            let bad_start = esc.at.saturating_sub(1);
                            self.bump(); // consume the bad escape char for the span
                            return Err(self.err(bad_start, LexErrorKind::BadEscape));
                        }
                    };
                    value.push(decoded);
                    self.bump(); // consume the escape char
                }
                _ => {
                    value.push(ch.c);
                    self.bump();
                }
            }
        }
    }

    /// Lex a numeric literal (int or float), optionally folding a trailing size
    /// unit into a [`Token::Size`].
    fn lex_number(&mut self) -> Result<(), LexError> {
        let start = self.byte_at();
        let mut raw = String::new();
        let mut is_float = false;
        while let Some(ch) = self.peek() {
            if ch.c.is_ascii_digit() {
                raw.push(ch.c);
                self.bump();
            } else if ch.c == '.'
                && !is_float
                && self.peek_n(1).is_some_and(|n| n.c.is_ascii_digit())
            {
                is_float = true;
                raw.push('.');
                self.bump();
            } else {
                break;
            }
        }

        if is_float {
            let value: f64 = raw
                .parse()
                .map_err(|_| self.err(start, LexErrorKind::BadNumber))?;
            self.push(start, Token::Float(value));
            return Ok(());
        }

        // Integer. Try to fold a following size unit (`25 MB`).
        if let Some(unit) = self.peek_size_unit() {
            // Re-parse as u64 for the size magnitude.
            let value: u64 = raw
                .parse()
                .map_err(|_| self.err(start, LexErrorKind::BadNumber))?;
            self.consume_size_unit_word();
            self.push(start, Token::Size { value, unit });
            return Ok(());
        }

        let value: i64 = raw
            .parse()
            .map_err(|_| self.err(start, LexErrorKind::BadNumber))?;
        self.push(start, Token::Int(value));
        Ok(())
    }

    /// Peek across whitespace for an uppercase word that is a [`SizeUnit`].
    /// Does not advance the cursor.
    fn peek_size_unit(&self) -> Option<SizeUnit> {
        let mut i = self.pos;
        // Skip whitespace (but not a newline-separated next statement boundary —
        // a unit must be on the same logical run; we allow spaces/tabs only).
        while let Some(ch) = self.chars.get(i) {
            if ch.c == ' ' || ch.c == '\t' {
                i += 1;
            } else {
                break;
            }
        }
        // The word must have been preceded by at least one space to be a unit
        // (`25MB` is not a size literal in this grammar — units are space-set).
        if i == self.pos {
            return None;
        }
        let mut word = String::new();
        while let Some(ch) = self.chars.get(i) {
            if is_ident_continue(ch.c) {
                word.push(ch.c);
                i += 1;
            } else {
                break;
            }
        }
        SizeUnit::from_word(&word)
    }

    /// Consume the whitespace + size-unit word previously confirmed by
    /// [`Self::peek_size_unit`].
    fn consume_size_unit_word(&mut self) {
        while self.peek().is_some_and(|ch| ch.c == ' ' || ch.c == '\t') {
            self.bump();
        }
        while self.peek().is_some_and(|ch| is_ident_continue(ch.c)) {
            self.bump();
        }
    }

    /// Lex an identifier-shaped word and classify it: keyword, boolean/null
    /// literal, typed-literal introducer, or bare identifier.
    fn lex_word(&mut self) -> Result<(), LexError> {
        let start = self.byte_at();
        let mut word = String::new();
        while let Some(ch) = self.peek() {
            if is_ident_continue(ch.c) {
                word.push(ch.c);
                self.bump();
            } else {
                break;
            }
        }

        // Typed literal: DATE/TIME/TIMESTAMP immediately followed by a string.
        if let Some(ty) = LitType::from_word(&word) {
            if let Some(raw) = self.try_typed_literal_string()? {
                self.push(start, Token::TypedLit { ty, raw });
                return Ok(());
            }
        }

        // Hex bytes literal `X'48656c6c6f'` (SQL-style): the introducer word is exactly
        // `x`/`X` and a string quote follows **immediately** (no space, unlike a typed
        // literal). The hex is decoded now so a `Token::Bytes` always carries valid bytes;
        // a non-hex digit or an odd length is a `BadHexBytes` lex error.
        if word.eq_ignore_ascii_case("x") && self.peek().is_some_and(|ch| ch.c == '\'') {
            let before = self.out.len();
            self.lex_string()?;
            let raw = match self.out.get(before).map(|t| &t.node) {
                Some(Token::Str(s)) => {
                    let r = s.clone();
                    self.out.truncate(before);
                    r
                }
                _ => unreachable!("lex_string pushes exactly one Str token"),
            };
            let bytes =
                decode_hex(&raw).ok_or_else(|| self.err(start, LexErrorKind::BadHexBytes))?;
            self.push(start, Token::Bytes(bytes));
            return Ok(());
        }

        // Boolean / null literal words.
        if let Some(tok) = literal_word(&word) {
            self.push(start, tok);
            return Ok(());
        }

        // Reserved single-word keyword (case-insensitive; canonical form is lowercase, t74).
        if let Some(kw) = Keyword::from_word(&word) {
            self.push(start, Token::Keyword(kw));
            return Ok(());
        }

        // Otherwise a bare identifier.
        self.push(start, Token::Ident(word));
        Ok(())
    }

    /// If the next non-space token is a string literal, consume it and return its
    /// raw resolved content (for a typed literal). Otherwise leave the cursor put.
    fn try_typed_literal_string(&mut self) -> Result<Option<String>, LexError> {
        // Look ahead across spaces/tabs for an opening quote.
        let mut i = self.pos;
        while let Some(ch) = self.chars.get(i) {
            if ch.c == ' ' || ch.c == '\t' {
                i += 1;
            } else {
                break;
            }
        }
        if self.chars.get(i).is_none_or(|ch| ch.c != '\'') {
            return Ok(None);
        }
        // Advance the real cursor to the quote, then reuse the string lexer's
        // body by capturing the token it pushes.
        self.pos = i;
        let before = self.out.len();
        self.lex_string()?;
        // lex_string pushed exactly one Str token; extract its content.
        if let Some(spanned) = self.out.get(before) {
            if let Token::Str(s) = &spanned.node {
                let raw = s.clone();
                self.out.truncate(before);
                return Ok(Some(raw));
            }
        }
        Ok(None)
    }

    /// Lex symbolic operators and structural punctuation with longest-match.
    fn lex_symbol(&mut self) -> Result<(), LexError> {
        let start = self.byte_at();
        let Some(ch) = self.peek() else {
            return Ok(());
        };
        let next = self.peek_n(1).map(|n| n.c);
        let two = |a: char, b: char| ch.c == a && next == Some(b);

        let token = if two('|', '>') {
            self.bump();
            self.bump();
            Token::Pipe
        } else if two('<', '=') {
            self.bump();
            self.bump();
            Token::Le
        } else if two('>', '=') {
            self.bump();
            self.bump();
            Token::Ge
        } else if two('<', '>') {
            self.bump();
            self.bump();
            Token::Ne
        } else if two('=', '>') {
            self.bump();
            self.bump();
            Token::Arrow
        } else if two('=', '=') {
            // Maximal munch: `==` (equivalence) before a lone `=` (bind). The `=>`
            // (Arrow) check above already won where applicable, so the three forms
            // `=>` / `==` / `=` are each unambiguous (blueprint decision O, ticket t70).
            self.bump();
            self.bump();
            Token::EqEq
        } else {
            let single = match ch.c {
                '=' => Token::Eq,
                '<' => Token::Lt,
                '>' => Token::Gt,
                '~' => Token::Tilde,
                '+' => Token::Plus,
                '-' => Token::Minus,
                '/' => Token::Slash,
                '(' => Token::LParen,
                ')' => Token::RParen,
                '{' => Token::LBrace,
                '}' => Token::RBrace,
                '[' => Token::LBracket,
                ']' => Token::RBracket,
                ';' => Token::Semicolon,
                ',' => Token::Comma,
                ':' => Token::Colon,
                '.' => Token::Dot,
                '*' => Token::Star,
                other => {
                    self.bump();
                    return Err(self.err(start, LexErrorKind::UnexpectedChar(other)));
                }
            };
            self.bump();
            single
        };
        self.push(start, token);
        Ok(())
    }

    /// A slash starts a path unless the previous emitted token is an expression operand.
    /// That keeps `/mail/inbox`, `JOIN /x`, `UPSERT INTO /dst /src`, and `( /x |> ... )` as
    /// paths, while `a/b`, `a / b`, and `(a) / b` become arithmetic division.
    fn slash_starts_path(&self) -> bool {
        if self.after_line_break {
            return true;
        }
        let Some(prev) = self.out.last().map(|t| &t.node) else {
            return true;
        };
        if let Token::Ident(s) = prev {
            if self.after_call_signature() {
                return true;
            }
            return path_boundary_word(s);
        }
        !matches!(
            prev,
            Token::Str(_)
                | Token::Int(_)
                | Token::Float(_)
                | Token::Bool(_)
                | Token::Null
                | Token::Size { .. }
                | Token::TypedLit { .. }
                | Token::Bytes(_)
                | Token::RParen
                | Token::RBrace
                | Token::RBracket
        )
    }

    fn after_call_signature(&self) -> bool {
        let n = self.out.len();
        if n < 4 {
            return false;
        }
        matches!(
            (
                &self.out[n - 4].node,
                &self.out[n - 3].node,
                &self.out[n - 2].node,
                &self.out[n - 1].node,
            ),
            (
                Token::Keyword(Keyword::Call),
                Token::Ident(_),
                Token::Dot,
                Token::Ident(_)
            )
        )
    }
}

/// Grammar words that can immediately precede a path. Most are lexed as dedicated keywords, but
/// some context words intentionally stay identifiers so that open function names remain available.
fn path_boundary_word(s: &str) -> bool {
    matches!(
        s.to_ascii_uppercase().as_str(),
        "AS" | "AT"
            | "CALL"
            | "CONNECT"
            | "CREATE"
            | "DISCONNECT"
            | "EXCEPT"
            | "FROM"
            | "INSERT"
            | "INTO"
            | "INTERSECT"
            | "JOIN"
            | "MAP"
            | "ON"
            | "OF"
            | "REMOVE"
            | "TABLE"
            | "TO"
            | "TRANSFORM"
            | "UNION"
            | "UPDATE"
            | "UPSERT"
            | "VIEW"
            | "WITH"
    )
}

/// Decode an even-length ASCII hex string to raw bytes. Returns `None` on an odd
/// length or a non-hex digit (the `X'…'` bytes-literal validation). An empty string
/// decodes to zero bytes.
fn decode_hex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len() / 2);
    for pair in bytes.chunks(2) {
        let hi = (pair[0] as char).to_digit(16)?;
        let lo = (pair[1] as char).to_digit(16)?;
        out.push((hi * 16 + lo) as u8);
    }
    Some(out)
}

/// Whether `c` can start a bare identifier / word.
fn is_ident_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_'
}

/// Whether `c` can continue a bare identifier / word.
fn is_ident_continue(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// Whether `c` ends a path segment / version run (operator or structural char).
///
/// `;` is here because it is **structural punctuation** — the transaction grammar's item separator
/// (`transaction { effect_stmt ; effect_stmt }`) and the `.qfs` document format's statement
/// separator. Before it was listed, a path swallowed the `;` glued to its right, so a
/// documented `transaction { … |> insert into /a/b; … }` raised UNEXPECTED_TOKEN while the same
/// text with one space before the `;` parsed — and the `.qfs` document splitter could not use the
/// lexer at all, because the terminator it needed to find never became a token. The cost is that a
/// bare path can no longer carry a literal `;`, which puts `;` in exactly the same class as the
/// `#` and `,` already listed here.
fn is_path_delimiter(c: char) -> bool {
    c.is_whitespace()
        || matches!(
            c,
            '|' | '=' | '<' | '>' | '~' | '(' | ')' | ',' | '\'' | '#' | ';'
        )
}
