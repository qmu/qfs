//! Internal winnow grammar (t04, the full blueprint §3 pipe-SQL grammar). **Crate-private**
//! — winnow types never escape this module; [`parse`] returns the owned
//! [`crate::ParseError`] (fidelity guard G6).
//!
//! ## Token-stream input (t03 → t04)
//! Unlike the E0 spike (which parsed `&str` directly), this grammar consumes the
//! **t03 token stream**: `qfs_lang::lex` produces `Vec<Spanned<Token>>`, and winnow's
//! built-in `&[T]` [`winnow::stream::Stream`] impl drives the combinators over that
//! slice. Each AST node re-spans itself from the byte span carried by its tokens, so
//! diagnostics round-trip to source (blueprint §6/§8). The lexer already folds path
//! `@version`, size/typed literals, and operators; this module stitches multi-word
//! keywords (`GROUP BY`, `INSERT INTO`, …), which the lexer emits as adjacent tokens.
//!
//! ## Closed core, structurally (blueprint §3)
//! Keyword surface comes from the frozen `qfs_lang::Keyword` set — there is no second
//! transcription. The grammar rejects unknown core constructs (incomplete multi-word keywords,
//! reserved-word-as-identifier) but leaves the three registry seams open: `CALL
//! driver.action`, `fn(...)`, `DECODE/ENCODE fmt`, and `/driver/...` paths all parse
//! into string-named reference nodes without resolving the names.
//!
//! ## Panic-free
//! The workspace `unwrap/expect/panic = deny` lint applies (NOT relaxed here, unlike
//! the spike). Every combinator returns a `Result`; the boundary mapper turns a
//! winnow failure into a structured error without ever indexing-panicking.

use qfs_lang::token::{LitType, PathSeg};
use qfs_lang::{lex, Keyword, Span, Spanned, Token};
use qfs_types::{canonical_base_column_type, declared_type_path, DeclaredColumn};
use winnow::combinator::{alt, cut_err, fail, opt, preceded, repeat, separated};
use winnow::error::{ContextError, ErrMode, ParseError as WinnowParseError};
use winnow::token::any;
use winnow::{ModalResult, Parser};

use crate::ast::{
    ArmWrite, Assignment, CallRef, Codec, ConnectionDeclAst, DdlKind, EffectBody, EffectStmt,
    EffectVerb, Expr, FnRef, FollowRef, Ident, JoinOp, Literal, NamedArg, OfColumn, OfRef,
    OfTarget, Op, OrderKey, Param, PathExpr, PathRef, PathSegment, PipeOp, Pipeline, PlanWrap,
    PolicyRuleAst, PolicySubjectAst, Projection, ServerDdl, Source, Statement, SwitchArm,
    SwitchStage, TransformRef, TypeAnn, Values,
};
use crate::error::{ParseError, ParseErrorCode};

/// The parser input stream: a slice of spanned tokens (winnow drives this directly).
type Stream<'a> = &'a [Spanned<Token>];
/// The winnow modal error used internally; never escapes this module.
type Err = ErrMode<ContextError>;

/// Parse one qfs statement from source text into the owned [`Statement`] AST.
///
/// Lexes via `qfs_lang::lex`, then runs the winnow grammar over the token slice,
/// mapping any winnow failure into the owned [`ParseError`] at this boundary.
///
/// # Errors
/// Returns a [`ParseError`] on a lexing error or any grammar failure.
pub(crate) fn parse(input: &str) -> Result<Statement, ParseError> {
    let tokens = lex(input).map_err(|e| lex_to_parse_error(input, &e))?;
    let slice: Stream<'_> = &tokens;
    match statement.parse(slice) {
        Ok(stmt) => Ok(stmt),
        Err(e) => Err(map_error(input, &tokens, &e)),
    }
}

/// Map a lexer error into the parser's owned error type (the two diagnostic
/// surfaces compose; blueprint §6).
fn lex_to_parse_error(_input: &str, e: &qfs_lang::LexError) -> ParseError {
    ParseError::new(
        e.span.start as usize,
        e.span,
        ParseErrorCode::UnexpectedToken,
        vec!["a valid token".to_string()],
        "an unlexable character",
        format!("lexing failed: {}", e.kind.as_str()),
    )
}

/// Map winnow's token-stream `ParseError` onto the owned structured error. The
/// failure offset is a **token index**; we resolve it back to a byte span via the
/// offending token (or EOF). The `found` description names the token *kind*, never
/// its literal value (blueprint §8 secret hygiene).
fn map_error(
    _input: &str,
    tokens: &[Spanned<Token>],
    err: &WinnowParseError<Stream<'_>, ContextError>,
) -> ParseError {
    let idx = err.offset();
    let Some(found_tok) = tokens.get(idx) else {
        // EOF: point at the end of the last token (or byte 0 for empty input).
        let end = tokens.last().map_or(0, |t| t.span.end);
        return ParseError::new(
            end as usize,
            Span::new(end, end),
            ParseErrorCode::UnexpectedEof,
            vec!["more input".to_string()],
            "end of input",
            "unexpected end of input",
        );
    };
    let span = found_tok.span;
    let (code, message) = classify(&found_tok.node);
    ParseError::new(
        span.start as usize,
        span,
        code,
        expected_set(),
        describe(&found_tok.node),
        message,
    )
}

/// Classify the offending token into a structured code + message. Keyword-shaped
/// identifiers are flagged distinctly (an incomplete multi-word keyword like `group`
/// without `by`, or a keyword used where the grammar wants a name); reserved keywords
/// in identifier position are flagged too. Keyword recognition is case-insensitive
/// (t74, decision S), so this is no longer a *case* mistake.
fn classify(tok: &Token) -> (ParseErrorCode, String) {
    match tok {
        Token::Ident(s) if is_keyword_word(s) => (
            ParseErrorCode::UnknownKeyword,
            "closed-core keywords are lowercase (recognized case-insensitively); this keyword \
             is not valid here (blueprint §3, decision S)"
                .to_string(),
        ),
        Token::Keyword(_) => (
            ParseErrorCode::ReservedAsIdentifier,
            "a reserved keyword cannot be used here".to_string(),
        ),
        // A lone `=` where the grammar wanted a comparison or pipe boundary is almost
        // always a stale SQL-style equality. Steer to `==` (blueprint decision O, t70): `=`
        // binds (assignment / named-arg / SET), `==` compares.
        Token::Eq => (
            ParseErrorCode::UnexpectedToken,
            "`=` binds (assignment); use `==` for equivalence (blueprint decision O)".to_string(),
        ),
        _ => (
            ParseErrorCode::UnexpectedToken,
            "the grammar did not expect this token here".to_string(),
        ),
    }
}

/// Whether `s` is a closed-core keyword word (case-insensitive, t74) — a single-word
/// keyword, or a multi-word keyword lead/tail / contextual sub-word the lexer leaves
/// as a bare identifier (`group`, `by`, `into`, `materialized`, …). Used to give a
/// crisp diagnostic when such a word stands where a name/stage was expected (e.g. a
/// `group` with no `by`).
fn is_keyword_word(s: &str) -> bool {
    Keyword::from_word(s).is_some()
        || matches!(
            s.to_ascii_uppercase().as_str(),
            "GROUP"
                | "ORDER"
                | "INSERT"
                | "UPSERT"
                | "MATERIALIZED"
                | "BY"
                | "INTO"
                | "OF"
                | "ASC"
                | "DESC"
        )
}

/// A representative closed-core expected-set for a failure point (non-empty per the
/// structured-error contract, blueprint §6).
fn expected_set() -> Vec<String> {
    vec![
        "a source path".to_string(),
        Keyword::InsertInto.text().to_string(),
        Keyword::Create.text().to_string(),
        Keyword::Preview.text().to_string(),
        Keyword::Commit.text().to_string(),
        "|>".to_string(),
        "a path".to_string(),
    ]
}

/// Describe a token by *kind* (never its literal value — blueprint §8 secret hygiene).
fn describe(tok: &Token) -> String {
    match tok {
        Token::Keyword(k) => format!("keyword `{}`", k.text()),
        Token::Pipe => "`|>`".to_string(),
        Token::Eq => "`=`".to_string(),
        Token::EqEq
        | Token::Ne
        | Token::Lt
        | Token::Gt
        | Token::Le
        | Token::Ge
        | Token::Tilde
        | Token::Plus
        | Token::Minus
        | Token::Slash => "an operator".to_string(),
        Token::LParen => "`(`".to_string(),
        Token::RParen => "`)`".to_string(),
        Token::Comma => "`,`".to_string(),
        Token::Colon => "`:`".to_string(),
        Token::Dot => "`.`".to_string(),
        Token::Star => "`*`".to_string(),
        Token::Arrow => "`=>`".to_string(),
        Token::Ident(_) => "an identifier".to_string(),
        Token::Path(_) => "a path".to_string(),
        Token::Str(_) => "a string literal".to_string(),
        Token::Int(_) => "an integer literal".to_string(),
        Token::Float(_) => "a float literal".to_string(),
        Token::Bool(_) => "a boolean literal".to_string(),
        Token::Null => "`NULL`".to_string(),
        Token::Size { .. } => "a size literal".to_string(),
        Token::TypedLit { .. } => "a typed literal".to_string(),
        Token::Bytes(_) => "a bytes literal".to_string(),
        Token::LBrace => "`{`".to_string(),
        Token::RBrace => "`}`".to_string(),
        Token::LBracket => "`[`".to_string(),
        Token::RBracket => "`]`".to_string(),
        _ => "a token".to_string(),
    }
}

// ---- low-level token matchers --------------------------------------------

/// Match exactly the given closed-core keyword.
fn kw<'a>(k: Keyword) -> impl Parser<Stream<'a>, Span, Err> {
    any.verify_map(move |t: Spanned<Token>| match t.node {
        Token::Keyword(got) if got == k => Some(t.span),
        _ => None,
    })
}

/// Match an identifier word equal to `word`, **case-insensitively** (t74, decision S):
/// used for multi-word keyword tails/leads and the word operators / DDL sub-keywords the
/// lexer leaves as identifiers, e.g. `by`, `into`, `materialized`, `and`, `or`, `of`,
/// `asc`, `desc`. The `word` argument is written UPPERCASE by convention; recognition
/// folds case so `INSERT`/`Insert`/`insert` all match — keeping the closed core lowercase
/// (canonical) while accepting any case.
fn word<'a>(word: &'static str) -> impl Parser<Stream<'a>, Span, Err> {
    any.verify_map(move |t: Spanned<Token>| match t.node {
        Token::Ident(ref s) if s.eq_ignore_ascii_case(word) => Some(t.span),
        _ => None,
    })
}

/// Match a bare identifier, rejecting reserved keywords in identifier position.
fn ident(input: &mut Stream<'_>) -> ModalResult<Spanned<Ident>> {
    any.verify_map(|t: Spanned<Token>| match t.node {
        Token::Ident(s) => Some(Spanned::new(s, t.span)),
        _ => None,
    })
    .parse_next(input)
}

/// Match a single non-keyword token equal to the given punctuation/operator token.
fn punct<'a>(tok: Token) -> impl Parser<Stream<'a>, Span, Err> {
    any.verify_map(move |t: Spanned<Token>| if t.node == tok { Some(t.span) } else { None })
}

/// Multi-word keyword: `group by` (= `group` ident + `by` ident, case-insensitive).
fn group_by(input: &mut Stream<'_>) -> ModalResult<Span> {
    (word("GROUP"), word("BY"))
        .map(|(a, b)| Span::new(a.start, b.end))
        .parse_next(input)
}

/// Multi-word keyword: `ORDER BY`.
fn order_by(input: &mut Stream<'_>) -> ModalResult<Span> {
    (word("ORDER"), word("BY"))
        .map(|(a, b)| Span::new(a.start, b.end))
        .parse_next(input)
}

/// Multi-word keyword: `INSERT INTO`.
fn insert_into(input: &mut Stream<'_>) -> ModalResult<Span> {
    (word("INSERT"), word("INTO"))
        .map(|(a, b)| Span::new(a.start, b.end))
        .parse_next(input)
}

/// Multi-word keyword: `UPSERT INTO`.
fn upsert_into(input: &mut Stream<'_>) -> ModalResult<Span> {
    (word("UPSERT"), word("INTO"))
        .map(|(a, b)| Span::new(a.start, b.end))
        .parse_next(input)
}

// ---- paths ----------------------------------------------------------------

/// A `/driver/seg/seg` mount path (from a single lexer `Token::Path`), plus an
/// optional `AS OF '<ts>'` temporal coordinate (blueprint §4). The path/mount registry
/// seam: segments are raw strings, never resolved here.
fn path_expr(input: &mut Stream<'_>) -> ModalResult<PathExpr> {
    let head = any
        .verify_map(|t: Spanned<Token>| match t.node {
            Token::Path(segs) => Some(Spanned::new(segs, t.span)),
            _ => None,
        })
        .parse_next(input)?;
    let as_of = opt(preceded(
        (kw(Keyword::As), word("OF")),
        any.verify_map(|t: Spanned<Token>| match t.node {
            Token::Str(s) => Some(s),
            _ => None,
        }),
    ))
    .parse_next(input)?;
    // Drop a leading empty segment produced by the lexer's leading `/`.
    let segments: Vec<PathSegment> = head
        .node
        .into_iter()
        .filter(|s: &PathSeg| !(s.name.is_empty() && s.version.is_none()))
        .map(|s| PathSegment {
            name: s.name,
            version: s.version,
            glob: s.glob,
        })
        .collect();
    Ok(PathExpr {
        segments,
        as_of,
        span: head.span,
    })
}

// ---- expressions (precedence climbing over the frozen operator set) -------

/// The operator-set expression entry point. Precedence (low → high):
/// `OR` < `AND` < `NOT` < comparison/predicate < primary.
fn expr(input: &mut Stream<'_>) -> ModalResult<Expr> {
    or_expr(input)
}

fn or_expr(input: &mut Stream<'_>) -> ModalResult<Expr> {
    let first = and_expr(input)?;
    let rest: Vec<Expr> = repeat(0.., preceded(word("OR"), and_expr)).parse_next(input)?;
    Ok(rest.into_iter().fold(first, |acc, next| Expr::Binary {
        op: Op::Or,
        lhs: Box::new(acc),
        rhs: Box::new(next),
    }))
}

fn and_expr(input: &mut Stream<'_>) -> ModalResult<Expr> {
    let first = not_expr(input)?;
    let rest: Vec<Expr> = repeat(0.., preceded(word("AND"), not_expr)).parse_next(input)?;
    Ok(rest.into_iter().fold(first, |acc, next| Expr::Binary {
        op: Op::And,
        lhs: Box::new(acc),
        rhs: Box::new(next),
    }))
}

fn not_expr(input: &mut Stream<'_>) -> ModalResult<Expr> {
    let nots: Vec<Span> = repeat(0.., word("NOT")).parse_next(input)?;
    let inner = predicate(input)?;
    Ok(nots.into_iter().fold(inner, |acc, _| Expr::Unary {
        op: Op::Not,
        expr: Box::new(acc),
    }))
}

/// A comparison / predicate: `lhs (op rhs | IN (..) | BETWEEN a AND b | LIKE p |
/// op ANY (..) )?`.
fn predicate(input: &mut Stream<'_>) -> ModalResult<Expr> {
    let lhs = additive_expr(input)?;
    // Try the predicate tails in order.
    if let Some(tail) = opt(predicate_tail).parse_next(input)? {
        return Ok(tail(lhs));
    }
    Ok(lhs)
}

/// A boxed transform applying a predicate tail to its left-hand expression.
type TailFn = Box<dyn FnOnce(Expr) -> Expr>;

fn predicate_tail(input: &mut Stream<'_>) -> ModalResult<TailFn> {
    alt((
        // <op> ANY (set)  |  <op> rhs
        cmp_tail,
        in_tail,
        between_tail,
        like_tail,
    ))
    .parse_next(input)
}

fn cmp_op(input: &mut Stream<'_>) -> ModalResult<Op> {
    any.verify_map(|t: Spanned<Token>| match t.node {
        // Equivalence is `==` (blueprint decision O, t70). A lone `=` is the binding token
        // and is intentionally NOT a comparator here — it surfaces as a crisp error
        // steering to `==` (see `classify`).
        Token::EqEq => Some(Op::Eq),
        Token::Ne => Some(Op::Ne),
        Token::Lt => Some(Op::Lt),
        Token::Gt => Some(Op::Gt),
        Token::Le => Some(Op::Le),
        Token::Ge => Some(Op::Ge),
        Token::Tilde => Some(Op::Match),
        _ => None,
    })
    .parse_next(input)
}

fn cmp_tail(input: &mut Stream<'_>) -> ModalResult<TailFn> {
    let op = cmp_op(input)?;
    // `<op> ANY (set)` quantified comparison.
    if opt(word("ANY")).parse_next(input)?.is_some() {
        let set = paren_expr_list(input)?;
        return Ok(Box::new(move |lhs| Expr::AnyOp {
            op,
            expr: Box::new(lhs),
            set,
        }));
    }
    let rhs = additive_expr(input)?;
    Ok(Box::new(move |lhs| Expr::Binary {
        op,
        lhs: Box::new(lhs),
        rhs: Box::new(rhs),
    }))
}

fn in_tail(input: &mut Stream<'_>) -> ModalResult<TailFn> {
    let _ = word("IN").parse_next(input)?;
    let set = paren_expr_list(input)?;
    Ok(Box::new(move |lhs| Expr::In {
        expr: Box::new(lhs),
        set,
    }))
}

fn between_tail(input: &mut Stream<'_>) -> ModalResult<TailFn> {
    let _ = word("BETWEEN").parse_next(input)?;
    let low = additive_expr(input)?;
    let _ = word("AND").parse_next(input)?;
    let high = additive_expr(input)?;
    Ok(Box::new(move |lhs| Expr::Between {
        expr: Box::new(lhs),
        low: Box::new(low),
        high: Box::new(high),
    }))
}

fn like_tail(input: &mut Stream<'_>) -> ModalResult<TailFn> {
    let _ = word("LIKE").parse_next(input)?;
    let pat = additive_expr(input)?;
    Ok(Box::new(move |lhs| Expr::Like {
        expr: Box::new(lhs),
        pattern: Box::new(pat),
    }))
}

/// `( <expr>, … )` argument/set list.
fn paren_expr_list(input: &mut Stream<'_>) -> ModalResult<Vec<Expr>> {
    let _ = punct(Token::LParen).parse_next(input)?;
    let items: Vec<Expr> = separated(0.., expr, punct(Token::Comma)).parse_next(input)?;
    let _ = punct(Token::RParen).parse_next(input)?;
    Ok(items)
}

fn additive_expr(input: &mut Stream<'_>) -> ModalResult<Expr> {
    let first = multiplicative_expr(input)?;
    let rest: Vec<(Op, Expr)> = repeat(0.., (add_op, multiplicative_expr)).parse_next(input)?;
    Ok(rest.into_iter().fold(first, |acc, (op, rhs)| Expr::Binary {
        op,
        lhs: Box::new(acc),
        rhs: Box::new(rhs),
    }))
}

fn multiplicative_expr(input: &mut Stream<'_>) -> ModalResult<Expr> {
    let first = primary(input)?;
    let rest: Vec<(Op, Expr)> = repeat(0.., (mul_op, primary)).parse_next(input)?;
    Ok(rest.into_iter().fold(first, |acc, (op, rhs)| Expr::Binary {
        op,
        lhs: Box::new(acc),
        rhs: Box::new(rhs),
    }))
}

fn add_op(input: &mut Stream<'_>) -> ModalResult<Op> {
    any.verify_map(|t: Spanned<Token>| match t.node {
        Token::Plus => Some(Op::Add),
        Token::Minus => Some(Op::Sub),
        _ => None,
    })
    .parse_next(input)
}

fn mul_op(input: &mut Stream<'_>) -> ModalResult<Op> {
    any.verify_map(|t: Spanned<Token>| match t.node {
        Token::Star => Some(Op::Mul),
        Token::Slash => Some(Op::Div),
        _ => None,
    })
    .parse_next(input)
}

/// A primary expression: literal, lambda, parenthesised expr, `*`, function call,
/// dotted path / column.
///
/// `lambda` is tried **before** `paren_expr`: both start with `(`, but only a lambda has
/// the trailing `=> <body>` after the parameter list. The lambda parser backtracks
/// cleanly when the `( … )` is not followed by `=>`, so a plain parenthesised expression
/// `(a == b)` still parses as `paren_expr`.
fn primary(input: &mut Stream<'_>) -> ModalResult<Expr> {
    alt((
        // Composite constructors first — their `[`/`{` opener is unambiguous in value
        // position and their elements are full sub-expressions (t92, generalised).
        array_expr,
        struct_expr,
        scalar_literal.map(Expr::Lit),
        lambda,
        paren_expr,
        fn_call,
        dotted_path,
    ))
    .parse_next(input)
}

/// An array constructor `[ e1, e2, … ]` (t92, generalised): comma-separated element
/// **expressions**, empty `[]` allowed. The `[` opener is backtrackable (so a non-array
/// `primary` alternative can still match); once opened, a missing `]` is a hard error.
fn array_expr(input: &mut Stream<'_>) -> ModalResult<Expr> {
    let _ = punct(Token::LBracket).parse_next(input)?;
    let elems: Vec<Expr> = separated(0.., expr, punct(Token::Comma)).parse_next(input)?;
    let _ = cut_err(punct(Token::RBracket)).parse_next(input)?;
    Ok(Expr::Array(elems))
}

/// A struct constructor `{ name: value, … }` (t92, generalised): comma-separated
/// `name: <expr>` fields in insertion order, empty `{}` allowed. The field name is a bare
/// identifier or a keyword-in-name-position ([`column_name`]). The `{` opener is
/// backtrackable; once opened, a malformed field or missing `}` is a hard error.
fn struct_expr(input: &mut Stream<'_>) -> ModalResult<Expr> {
    let _ = punct(Token::LBrace).parse_next(input)?;
    let fields: Vec<(String, Expr)> =
        separated(0.., struct_field, punct(Token::Comma)).parse_next(input)?;
    let _ = cut_err(punct(Token::RBrace)).parse_next(input)?;
    Ok(Expr::Struct(fields))
}

/// One `name: <expr>` field of a struct constructor.
fn struct_field(input: &mut Stream<'_>) -> ModalResult<(String, Expr)> {
    let name = column_name(input)?;
    let _ = cut_err(punct(Token::Colon)).parse_next(input)?;
    let value = cut_err(expr).parse_next(input)?;
    Ok((name.node, value))
}

/// A lambda literal `( params ) => <expr>` — a first-class value (M6 ticket t61,
/// decision H "functions are values"). **No keyword is added**: the form rides the
/// expression grammar and reuses the existing `=>` arrow token (also used by named call
/// args); the parenthesised parameter list is what distinguishes a lambda from a
/// named-arg pair or a parenthesised sub-expression.
///
/// The whole production is backtrackable up to the `=>`: if a `( … )` group is **not**
/// followed by `=>` it is not a lambda, so `opt` lets the enclosing `alt` fall through to
/// `paren_expr`. Once `=>` is seen the body is parsed as a full expression (a lambda body
/// is expression-only — a lambda is a pure transformation, it can name no effect, blueprint §3).
fn lambda(input: &mut Stream<'_>) -> ModalResult<Expr> {
    let _ = punct(Token::LParen).parse_next(input)?;
    let params: Vec<Param> = separated(0.., lambda_param, punct(Token::Comma)).parse_next(input)?;
    let _ = punct(Token::RParen).parse_next(input)?;
    // The arrow is the commit point: only here do we know this `( … )` is a lambda and
    // not a parenthesised expression. `opt` (no `cut_err`) lets a non-lambda backtrack.
    let _ = punct(Token::Arrow).parse_next(input)?;
    let body = expr(input)?;
    Ok(Expr::Lambda {
        params,
        body: Box::new(body),
    })
}

/// One lambda parameter: a bare name with an optional `: <type>` annotation. The
/// annotation is parsed-and-retained (`Option<TypeAnn>`). The type checker enforces the
/// canonical §5 vocabulary (`text`/`int`/`array<text>`/`struct<id:int>`/…) plus `Resource`;
/// this parser keeps retired or misspelled scalar tokens as text so they become structured
/// plan-time errors instead of syntax failures.
fn lambda_param(input: &mut Stream<'_>) -> ModalResult<Param> {
    let name = ident(input)?;
    let ty = opt(preceded(punct(Token::Colon), lambda_type_annotation)).parse_next(input)?;
    Ok(Param {
        name: name.node,
        ty,
    })
}

/// A lambda parameter type annotation in the canonical §5 type-literal grammar.
///
/// `Resource` is the one non-column value annotation and is case-sensitive; scalar column tokens
/// are normalized to lowercase, and recursive column types serialize to the exact
/// [`ColumnType::parse`](qfs_types::ColumnType::parse) form.
fn lambda_type_annotation(input: &mut Stream<'_>) -> ModalResult<TypeAnn> {
    if let Some(resource) = opt(resource_type_annotation).parse_next(input)? {
        return Ok(TypeAnn { name: resource });
    }
    lambda_column_type(input).map(|name| TypeAnn { name })
}

fn resource_type_annotation(input: &mut Stream<'_>) -> ModalResult<String> {
    any.verify_map(|t: Spanned<Token>| match t.node {
        Token::Ident(s) if s == "Resource" => Some(s),
        _ => None,
    })
    .parse_next(input)
}

fn lambda_column_type(input: &mut Stream<'_>) -> ModalResult<String> {
    if opt(word("ARRAY")).parse_next(input)?.is_some() {
        let _ = cut_err(punct(Token::Lt)).parse_next(input)?;
        let elem = cut_err(lambda_column_type).parse_next(input)?;
        let _ = cut_err(punct(Token::Gt)).parse_next(input)?;
        return Ok(format!("array<{elem}>"));
    }
    if opt(word("STRUCT")).parse_next(input)?.is_some() {
        if opt(punct(Token::Ne)).parse_next(input)?.is_some() {
            return Ok("struct<>".to_string());
        }
        if opt(punct(Token::Lt)).parse_next(input)?.is_some() {
            let fields: Vec<(String, String)> = cut_err(separated(
                0..,
                lambda_struct_type_field,
                punct(Token::Comma),
            ))
            .parse_next(input)?;
            let _ = cut_err(punct(Token::Gt)).parse_next(input)?;
            let inner: Vec<String> = fields
                .iter()
                .map(|(name, ty)| format!("{name}:{ty}"))
                .collect();
            return Ok(format!("struct<{}>", inner.join(",")));
        }
        return Ok("struct<>".to_string());
    }
    Ok(ident(input)?.node.to_ascii_lowercase())
}

fn lambda_struct_type_field(input: &mut Stream<'_>) -> ModalResult<(String, String)> {
    let name = ident(input)?.node;
    let _ = cut_err(punct(Token::Colon)).parse_next(input)?;
    let ty = cut_err(lambda_column_type).parse_next(input)?;
    Ok((name, ty))
}

fn paren_expr(input: &mut Stream<'_>) -> ModalResult<Expr> {
    let _ = punct(Token::LParen).parse_next(input)?;
    let e = expr(input)?;
    let _ = punct(Token::RParen).parse_next(input)?;
    Ok(e)
}

/// A registry function call `name(args)` — the function registry seam (blueprint §3). The
/// name is a string; resolution (incl. receiver-typed alias resolution) is E2.
fn fn_call(input: &mut Stream<'_>) -> ModalResult<Expr> {
    let name = ident(input)?;
    let open = punct(Token::LParen).parse_next(input)?;
    let args: Vec<Expr> = separated(0.., expr, punct(Token::Comma)).parse_next(input)?;
    let close = punct(Token::RParen).parse_next(input)?;
    Ok(Expr::Fn(FnRef {
        name: name.node,
        args,
        span: Span::new(name.span.start.min(open.start), close.end),
    }))
}

/// A dotted path `a.b.c` (struct navigation, blueprint §4) or a bare column. The leading
/// segment is a bare identifier; trailing `.seg`s are struct navigation.
fn dotted_path(input: &mut Stream<'_>) -> ModalResult<Expr> {
    let head = ident(input)?;
    let rest: Vec<Spanned<Ident>> =
        repeat(0.., preceded(punct(Token::Dot), ident)).parse_next(input)?;
    if rest.is_empty() {
        Ok(Expr::Col(head.node))
    } else {
        let mut segs = vec![head.node];
        segs.extend(rest.into_iter().map(|s| s.node));
        Ok(Expr::Path(segs))
    }
}

/// A single-token scalar literal (string/int/float/bool/null/size/typed/bytes).
fn scalar_literal(input: &mut Stream<'_>) -> ModalResult<Literal> {
    any.verify_map(|t: Spanned<Token>| match t.node {
        Token::Str(s) => Some(Literal::Str(s)),
        Token::Int(i) => Some(Literal::Int(i)),
        Token::Float(f) => Some(Literal::Float(f)),
        Token::Bool(b) => Some(Literal::Bool(b)),
        Token::Null => Some(Literal::Null),
        Token::Size { value, unit } => Some(Literal::Size {
            value,
            unit: unit.text().to_string(),
        }),
        Token::TypedLit { ty, raw } => Some(Literal::Typed {
            ty: lit_type_text(ty).to_string(),
            raw,
        }),
        Token::Bytes(b) => Some(Literal::Bytes(b)),
        _ => None,
    })
    .parse_next(input)
}

fn lit_type_text(ty: LitType) -> &'static str {
    ty.text()
}

// ---- projections / assignments / order keys -------------------------------

/// A `SELECT`/`AGGREGATE` projection: `*` or `<expr> [AS <alias>]`.
fn projection(input: &mut Stream<'_>) -> ModalResult<Projection> {
    if opt(punct(Token::Star)).parse_next(input)?.is_some() {
        return Ok(Projection::Star);
    }
    let e = expr(input)?;
    let alias = opt(preceded(kw(Keyword::As), ident))
        .parse_next(input)?
        .map(|s| s.node);
    Ok(Projection::Expr { expr: e, alias })
}

fn projection_list(input: &mut Stream<'_>) -> ModalResult<Vec<Projection>> {
    separated(1.., projection, punct(Token::Comma)).parse_next(input)
}

/// An `EXTEND`/`SET` assignment: `<name> = <expr>`.
fn assignment(input: &mut Stream<'_>) -> ModalResult<Assignment> {
    let name = ident(input)?;
    let _ = punct(Token::Eq).parse_next(input)?;
    let value = expr(input)?;
    Ok(Assignment {
        name: name.node,
        value,
    })
}

fn assignment_list(input: &mut Stream<'_>) -> ModalResult<Vec<Assignment>> {
    separated(1.., assignment, punct(Token::Comma)).parse_next(input)
}

/// An `ORDER BY` key: `<expr> [ASC|DESC]`.
fn order_key(input: &mut Stream<'_>) -> ModalResult<OrderKey> {
    let e = expr(input)?;
    let descending = if opt(word("DESC")).parse_next(input)?.is_some() {
        true
    } else {
        let _ = opt(word("ASC")).parse_next(input)?;
        false
    };
    Ok(OrderKey {
        expr: e,
        descending,
    })
}

// ---- sources --------------------------------------------------------------

/// A pipeline source: `VALUES …`, `( <pipeline> )`, a `/driver/...` path, or a bare
/// identifier naming a `LET`-bound relation (M6, ticket t60). The bare-identifier form is
/// tried **last** so it never shadows a keyword/path/values form; a reserved keyword is
/// already a `Token::Keyword` (not an `Ident`), so it cannot be mistaken for a name source.
fn source(input: &mut Stream<'_>) -> ModalResult<Source> {
    alt((
        values.map(Source::Values),
        subquery_source,
        path_expr.map(Source::Path),
        ident.map(|s| Source::Name(s.node)),
    ))
    .parse_next(input)
}

fn subquery_source(input: &mut Stream<'_>) -> ModalResult<Source> {
    let _ = punct(Token::LParen).parse_next(input)?;
    let inner = pipeline(input)?;
    let _ = punct(Token::RParen).parse_next(input)?;
    Ok(Source::Subquery(Box::new(inner)))
}

/// `VALUES [(<cols>)] (<row>), (<row>) …` — an inline literal relation.
fn values(input: &mut Stream<'_>) -> ModalResult<Values> {
    let _ = kw(Keyword::Values).parse_next(input)?;
    // Optional column list: a parenthesised list of bare identifiers, only when it
    // is immediately followed by another `(` row.
    let columns = opt(value_column_list).parse_next(input)?;
    // Rows are `(..)` groups, optionally comma-separated. The first row is required;
    // subsequent rows are each preceded by an optional comma (so both `(1)(2)` and
    // `(1),(2)` parse).
    let first = paren_expr_list(input)?;
    let rest: Vec<Vec<Expr>> =
        repeat(0.., preceded(opt(punct(Token::Comma)), paren_expr_list)).parse_next(input)?;
    let mut rows = vec![first];
    rows.extend(rest);
    Ok(Values { columns, rows })
}

/// A **column name** in a name-only position: a bare identifier, or a reserved keyword
/// spelled as a name. Keywords are reserved (t74, case-insensitive), but a column name in
/// a `VALUES (…)` list is unambiguous, so a keyword token contributes its canonical
/// lowercase text as the name. This keeps schema fields whose names collide with keyword
/// words usable without quoting — e.g. the `/server/jobs` field `every` and the
/// `/server/triggers` field `on` — which decision S's lowercase keyword set would
/// otherwise shadow. It is a pure surface accommodation: no effect, no new vocabulary.
fn column_name(input: &mut Stream<'_>) -> ModalResult<Spanned<Ident>> {
    any.verify_map(|t: Spanned<Token>| match t.node {
        Token::Ident(s) => Some(Spanned::new(s, t.span)),
        Token::Keyword(k) => Some(Spanned::new(k.text().to_string(), t.span)),
        _ => None,
    })
    .parse_next(input)
}

/// A `VALUES` column list `(a, b)` that is followed by a row `(` (lookahead). We
/// only treat a leading paren-group as columns when all its members are column
/// names ([`column_name`]: a bare identifier or a keyword-in-name-position) AND a
/// second `(` (the first row) follows — otherwise the group is itself the first/only
/// row and this parser backtracks. winnow `&[T]` streams are `Copy`, so the post-list
/// cursor is restored after the non-consuming lookahead.
fn value_column_list(input: &mut Stream<'_>) -> ModalResult<Vec<Ident>> {
    let _ = punct(Token::LParen).parse_next(input)?;
    let cols: Vec<Spanned<Ident>> =
        separated(1.., column_name, punct(Token::Comma)).parse_next(input)?;
    let _ = punct(Token::RParen).parse_next(input)?;
    // Non-consuming lookahead for a following row `(`.
    let after_cols = *input;
    if punct(Token::LParen).parse_next(input).is_err() {
        return Err(ErrMode::Backtrack(ContextError::new()));
    }
    *input = after_cols; // restore: the row parser re-consumes the `(`.
    Ok(cols.into_iter().map(|s| s.node).collect())
}

// ---- pipe operations ------------------------------------------------------

fn pipeline(input: &mut Stream<'_>) -> ModalResult<Pipeline> {
    // Decision R (t73): the source position needs no `FROM`. A leading `/path`, a `(subquery)`,
    // a `VALUES …` literal, or a `LET`-bound name *is* the source. Slash lexing is
    // position-sensitive: a source-boundary slash is a path; an operand-following slash is
    // arithmetic division. The source is parsed backtrackably so a non-pipeline statement
    // (an effect/DDL/plan wrapper) can still win in the enclosing `alt`.
    let source = source(input)?;
    // Once a `|>` is consumed we are committed to a pipe op: `cut_err` turns an
    // inner failure into a non-backtracking error so the diagnostic points *inside*
    // the op (a dangling `where`, an incomplete multi-word keyword) instead of back at the `|>`.
    let ops: Vec<PipeOp> =
        repeat(0.., preceded(punct(Token::Pipe), cut_err(pipe_op))).parse_next(input)?;
    Ok(Pipeline { source, ops })
}

fn pipe_op(input: &mut Stream<'_>) -> ModalResult<PipeOp> {
    alt((
        alt((
            where_op,
            select_op,
            extend_op,
            set_op,
            aggregate_op,
            group_by_op,
            order_by_op,
            limit_op,
            distinct_op,
        )),
        alt((
            join_op,
            union_op,
            except_op,
            intersect_op,
            as_op,
            expand_op,
            decode_op,
            encode_op,
            call_op,
        )),
        transform_op,
        switch_op,
        follow_op,
        of_op,
    ))
    .parse_next(input)
}

fn where_op(input: &mut Stream<'_>) -> ModalResult<PipeOp> {
    preceded(kw(Keyword::Where), cut_err(expr))
        .map(PipeOp::Where)
        .parse_next(input)
}

fn select_op(input: &mut Stream<'_>) -> ModalResult<PipeOp> {
    preceded(kw(Keyword::Select), cut_err(projection_list))
        .map(PipeOp::Select)
        .parse_next(input)
}

fn extend_op(input: &mut Stream<'_>) -> ModalResult<PipeOp> {
    preceded(kw(Keyword::Extend), cut_err(assignment_list))
        .map(PipeOp::Extend)
        .parse_next(input)
}

fn set_op(input: &mut Stream<'_>) -> ModalResult<PipeOp> {
    preceded(kw(Keyword::Set), cut_err(assignment_list))
        .map(PipeOp::Set)
        .parse_next(input)
}

fn aggregate_op(input: &mut Stream<'_>) -> ModalResult<PipeOp> {
    preceded(kw(Keyword::Aggregate), cut_err(projection_list))
        .map(PipeOp::Aggregate)
        .parse_next(input)
}

fn group_by_op(input: &mut Stream<'_>) -> ModalResult<PipeOp> {
    preceded(group_by, separated(1.., expr, punct(Token::Comma)))
        .map(PipeOp::GroupBy)
        .parse_next(input)
}

fn order_by_op(input: &mut Stream<'_>) -> ModalResult<PipeOp> {
    preceded(order_by, separated(1.., order_key, punct(Token::Comma)))
        .map(PipeOp::OrderBy)
        .parse_next(input)
}

fn limit_op(input: &mut Stream<'_>) -> ModalResult<PipeOp> {
    let _ = kw(Keyword::Limit).parse_next(input)?;
    let n = any
        .verify_map(|t: Spanned<Token>| match t.node {
            Token::Int(i) => Some(i),
            _ => None,
        })
        .parse_next(input)?;
    Ok(PipeOp::Limit(n))
}

fn distinct_op(input: &mut Stream<'_>) -> ModalResult<PipeOp> {
    kw(Keyword::Distinct)
        .map(|_| PipeOp::Distinct)
        .parse_next(input)
}

fn join_op(input: &mut Stream<'_>) -> ModalResult<PipeOp> {
    let _ = kw(Keyword::Join).parse_next(input)?;
    let src = source(input)?;
    let _ = kw(Keyword::On).parse_next(input)?;
    let on = expr(input)?;
    Ok(PipeOp::Join(JoinOp { source: src, on }))
}

fn union_op(input: &mut Stream<'_>) -> ModalResult<PipeOp> {
    preceded(kw(Keyword::Union), pipeline)
        .map(|p| PipeOp::Union(Box::new(p)))
        .parse_next(input)
}

fn except_op(input: &mut Stream<'_>) -> ModalResult<PipeOp> {
    preceded(kw(Keyword::Except), pipeline)
        .map(|p| PipeOp::Except(Box::new(p)))
        .parse_next(input)
}

fn intersect_op(input: &mut Stream<'_>) -> ModalResult<PipeOp> {
    preceded(kw(Keyword::Intersect), pipeline)
        .map(|p| PipeOp::Intersect(Box::new(p)))
        .parse_next(input)
}

fn as_op(input: &mut Stream<'_>) -> ModalResult<PipeOp> {
    preceded(kw(Keyword::As), ident)
        .map(|s| PipeOp::As(s.node))
        .parse_next(input)
}

fn expand_op(input: &mut Stream<'_>) -> ModalResult<PipeOp> {
    let _ = kw(Keyword::Expand).parse_next(input)?;
    let field = dotted_path_ref(input)?;
    Ok(PipeOp::Expand(field))
}

/// A dotted path reference `a.b.c` returned as a list of identifiers (for `EXPAND`).
fn dotted_path_ref(input: &mut Stream<'_>) -> ModalResult<PathRef> {
    let head = ident(input)?;
    let rest: Vec<Spanned<Ident>> =
        repeat(0.., preceded(punct(Token::Dot), ident)).parse_next(input)?;
    let mut segs = vec![head.node];
    segs.extend(rest.into_iter().map(|s| s.node));
    Ok(segs)
}

fn decode_op(input: &mut Stream<'_>) -> ModalResult<PipeOp> {
    let _ = kw(Keyword::Decode).parse_next(input)?;
    codec(input).map(PipeOp::Decode)
}

fn encode_op(input: &mut Stream<'_>) -> ModalResult<PipeOp> {
    let _ = kw(Keyword::Encode).parse_next(input)?;
    codec(input).map(PipeOp::Encode)
}

/// A codec format name — the codec registry seam (blueprint §4). A bare identifier (string
/// name), resolved later.
fn codec(input: &mut Stream<'_>) -> ModalResult<Codec> {
    let fmt = ident(input)?;
    Ok(Codec {
        fmt: fmt.node,
        span: fmt.span,
    })
}

/// `TRANSFORM <name>` — the model-calling pipe stage (blueprint §15, decision W).
///
/// `transform` is a **contextual identifier** (matched by [`word`], NOT a frozen keyword —
/// the keyword set stays 39); `<name>` is a bare identifier naming a declared
/// `CREATE TRANSFORM` definition, resolved later against the transform registry.
fn transform_op(input: &mut Stream<'_>) -> ModalResult<PipeOp> {
    let start = word("transform").parse_next(input)?;
    let name = ident(input)?;
    Ok(PipeOp::Transform(TransformRef {
        name: name.node,
        span: Span::new(start.start, name.span.end),
    }))
}

/// `FOLLOW <field>` — the declared-driver second-fetch stage (blueprint §13, ticket
/// 20260711121526).
///
/// `follow` is a **contextual identifier** (matched by [`word`], NOT a frozen keyword — the
/// keyword set stays 39, the `transform`/`switch` lesson); `<field>` is a bare identifier naming
/// the delivered-row field whose text value is the follow URL. Shape only here — "only valid in
/// a declared view body" is a structured lowering/eval refusal, not a parse error.
fn follow_op(input: &mut Stream<'_>) -> ModalResult<PipeOp> {
    let start = word("follow").parse_next(input)?;
    let field = ident(input)?;
    Ok(PipeOp::Follow(FollowRef {
        field: field.node,
        span: Span::new(start.start, field.span.end),
    }))
}

/// `OF <name>` or `OF (<col> <type>, …)` — the general, any-position, plan-time type assertion
/// (blueprint §5.6). `of` is a **contextual identifier** (matched by [`word`], NOT a frozen
/// keyword — the keyword set stays 39; `of` is already `word("OF")` in the DDL). Two target forms:
///
/// - `of customer` / `of chatwork/message` — a bare/qualified declared type NAME, canonicalized to
///   its `/type/<name>` catalog path (§5.5) and resolved later against the type-def registry.
/// - `of (priority text, reason text)` — an inline anonymous structural type literal reusing the
///   `CREATE TABLE` column-list production (§5.2).
///
/// A `/type/…` PATH target is the §5.7 category error and does NOT parse: the named branch begins
/// with an `ident`, so a `Token::Path` there fails, and the inline branch requires `(` — so
/// `of /type/x` matches neither, exactly like `transform /path`. The structural/refinement check
/// itself is plan-time (blueprint §5.6); the parser validates shape only.
fn of_op(input: &mut Stream<'_>) -> ModalResult<PipeOp> {
    let start = word("of").parse_next(input)?;
    // Inline structural literal `(…)` vs a named target. `word("of")` already committed us to an
    // `of` stage, so a malformed target from here is a hard error, not an `alt` fallthrough.
    if opt(punct(Token::LParen)).parse_next(input)?.is_some() {
        let cols: Vec<TableColumnDef> =
            cut_err(separated(1.., table_column_def, punct(Token::Comma))).parse_next(input)?;
        let close = cut_err(punct(Token::RParen)).parse_next(input)?;
        let columns = cols
            .into_iter()
            .map(|c| OfColumn {
                name: c.name,
                ty: c.ty,
                nullable: c.nullable,
                primary_key: c.primary_key,
                unique: c.unique,
            })
            .collect();
        return Ok(PipeOp::Of(OfRef {
            target: OfTarget::Inline(columns),
            span: Span::new(start.start, close.end),
        }));
    }
    // Named target: mirror `type_name` (canonicalize to `/type/<name>`, reject a `/type/…` path)
    // while capturing the trailing span for a full-stage diagnostic. A leading path token fails the
    // `ident` below — the category-error rejection.
    let head = cut_err(ident).parse_next(input)?;
    let rest: Vec<Spanned<Ident>> =
        repeat(0.., preceded(punct(Token::Slash), cut_err(ident))).parse_next(input)?;
    let end = rest.last().map_or(head.span.end, |s| s.span.end);
    let mut segments = vec![head.node];
    segments.extend(rest.into_iter().map(|s| s.node));
    let name =
        declared_type_path(&segments.join("/")).ok_or_else(|| ErrMode::Cut(ContextError::new()))?;
    Ok(PipeOp::Of(OfRef {
        target: OfTarget::Named(name),
        span: Span::new(start.start, end),
    }))
}

/// `SWITCH <col> { '<label>' => <arm>, …, else => <arm> }` — the model-routing pipe stage
/// (blueprint §18).
///
/// `switch` and `else` are **contextual identifiers** (matched by [`word`], NOT frozen keywords —
/// the keyword set stays 39, the `transform` lesson); `{`/`}` are the existing brace tokens and
/// `=>` the existing arrow. Each arm's right-hand side is a bare pipeline continuation over the
/// arm's routed partition, optionally terminated by an `INSERT`/`UPSERT INTO` write. The parser
/// validates shape only — arm-list semantics (exactly one trailing `else`, unique labels, the
/// effect-terminal typing rule, discriminant existence) are structured *eval*-time errors, so the
/// diagnostics can name columns and labels instead of token positions.
fn switch_op(input: &mut Stream<'_>) -> ModalResult<PipeOp> {
    let start = word("switch").parse_next(input)?;
    // Committed once `switch <ident>` is seen: everything after is cut (non-backtracking) so a
    // malformed arm points inside the stage, not back at the `|>`.
    let discriminant = ident(input)?;
    let _ = cut_err(punct(Token::LBrace)).parse_next(input)?;
    let mut arms = Vec::new();
    loop {
        arms.push(cut_err(switch_arm).parse_next(input)?);
        if opt(punct(Token::Comma)).parse_next(input)?.is_none() {
            break;
        }
    }
    let end = cut_err(punct(Token::RBrace)).parse_next(input)?;
    Ok(PipeOp::Switch(SwitchStage {
        discriminant: discriminant.node,
        arms,
        span: Span::new(start.start, end.end),
    }))
}

/// One switch arm: `'<label>' => <body>` or `else => <body>`.
///
/// The body is parsed against a **bounded token sub-slice** ([`switch_arm_body_end`]): the greedy
/// comma-separated list parsers inside an arm (`select a, b`, `RETURNING x, y`) would otherwise
/// consume the arm-separator comma AND the next arm's label (a string literal parses as a
/// projection expression). The bound is exact, not heuristic: at arm depth a `'<label>' =>` /
/// `else =>` token pair can ONLY be an arm boundary (no expression form puts `=>` after a bare
/// string literal — a lambda's parameter list is parenthesised).
fn switch_arm(input: &mut Stream<'_>) -> ModalResult<SwitchArm> {
    // The label: a string literal, or the contextual word `else` for the default arm.
    let (label, label_span) = alt((
        any.verify_map(|t: Spanned<Token>| match t.node {
            Token::Str(s) => Some((Some(s), t.span)),
            _ => None,
        }),
        word("else").map(|span| (None, span)),
    ))
    .parse_next(input)?;
    let _ = cut_err(punct(Token::Arrow)).parse_next(input)?;
    let end = switch_arm_body_end(input);
    if end == 0 {
        // An empty arm body (`'a' => ,` / `'a' => }`) — fail at the boundary token.
        return cut_err(fail::<_, SwitchArm, _>).parse_next(input);
    }
    let mut body: Stream<'_> = &input[..end];
    let (ops, write) = switch_arm_body(&mut body)?;
    if !body.is_empty() {
        // The body parser must consume the whole bounded slice (e.g. a write followed by more
        // ops — a write is terminal). Report at the first unconsumed token.
        return cut_err(fail::<_, SwitchArm, _>).parse_next(&mut body);
    }
    let body_end = input[end - 1].span.end;
    *input = &input[end..];
    Ok(SwitchArm {
        label,
        ops,
        write,
        span: Span::new(label_span.start, body_end),
    })
}

/// The exclusive end index of the current arm's body within `toks`: the first top-level arm
/// boundary. A boundary is the switch block's closing `}` at arm depth, or a `,` at arm depth
/// followed by `'<label>' =>` / `else =>`. Nested `(`/`[`/`{` groups (call arguments, struct
/// literals, a nested switch) are depth-tracked so their commas and braces never split an arm.
fn switch_arm_body_end(toks: Stream<'_>) -> usize {
    let mut depth = 0usize;
    for (i, t) in toks.iter().enumerate() {
        match &t.node {
            Token::LParen | Token::LBracket | Token::LBrace => depth += 1,
            Token::RParen | Token::RBracket => depth = depth.saturating_sub(1),
            Token::RBrace => {
                if depth == 0 {
                    return i;
                }
                depth -= 1;
            }
            Token::Comma if depth == 0 => {
                let is_label = match toks.get(i + 1).map(|t| &t.node) {
                    Some(Token::Str(_)) => true,
                    Some(Token::Ident(s)) if s.eq_ignore_ascii_case("else") => true,
                    _ => false,
                };
                if is_label && matches!(toks.get(i + 2).map(|t| &t.node), Some(Token::Arrow)) {
                    return i;
                }
            }
            _ => {}
        }
    }
    toks.len()
}

/// An arm's body: zero or more `|>`-chained pipe ops, optionally terminated by an
/// `INSERT`/`UPSERT INTO` write ([`switch_arm_write`]). A leading write (`'a' => INSERT INTO /x`)
/// is the no-op-continuation arm. The write is terminal — trailing tokens after it are left for
/// [`switch_arm`]'s full-consumption check to reject.
fn switch_arm_body(input: &mut Stream<'_>) -> ModalResult<(Vec<PipeOp>, Option<ArmWrite>)> {
    if let Some(w) = opt(switch_arm_write).parse_next(input)? {
        return Ok((Vec::new(), Some(w)));
    }
    let mut ops = vec![cut_err(pipe_op).parse_next(input)?];
    let mut write = None;
    while opt(punct(Token::Pipe)).parse_next(input)?.is_some() {
        if let Some(w) = opt(switch_arm_write).parse_next(input)? {
            write = Some(w);
            break;
        }
        ops.push(cut_err(pipe_op).parse_next(input)?);
    }
    Ok((ops, write))
}

/// An arm's terminal write: `INSERT INTO <path> [RETURNING …]` / `UPSERT INTO <path> [RETURNING …]`.
/// No `VALUES`/`FROM` body — the routed partition *is* the written relation (blueprint §18-B).
fn switch_arm_write(input: &mut Stream<'_>) -> ModalResult<ArmWrite> {
    let (start, verb) = alt((
        insert_into.map(|s| (s, EffectVerb::Insert)),
        upsert_into.map(|s| (s, EffectVerb::Upsert)),
    ))
    .parse_next(input)?;
    let target = cut_err(path_expr).parse_next(input)?;
    let returning = opt(returning_clause).parse_next(input)?;
    let end = target.span.end;
    Ok(ArmWrite {
        verb,
        target,
        returning,
        span: Span::new(start.start, end),
    })
}

/// `CALL driver.action(args)` — the procedure registry seam (blueprint §3). Shape only;
/// names are strings resolved later (capability gating deferred to E2).
fn call_op(input: &mut Stream<'_>) -> ModalResult<PipeOp> {
    let call_span = kw(Keyword::Call).parse_next(input)?;
    let driver = ident(input)?;
    let _ = punct(Token::Dot).parse_next(input)?;
    let action = ident(input)?;
    let args = opt(named_arg_list).parse_next(input)?.unwrap_or_default();
    let end = action.span.end;
    Ok(PipeOp::Call(CallRef {
        driver: driver.node,
        action: action.node,
        args,
        span: Span::new(call_span.start, end),
    }))
}

/// `( arg, … )` for a `CALL`, each arg positional or `name => value`.
fn named_arg_list(input: &mut Stream<'_>) -> ModalResult<Vec<NamedArg>> {
    let _ = punct(Token::LParen).parse_next(input)?;
    let args: Vec<NamedArg> = separated(0.., named_arg, punct(Token::Comma)).parse_next(input)?;
    let _ = punct(Token::RParen).parse_next(input)?;
    Ok(args)
}

fn named_arg(input: &mut Stream<'_>) -> ModalResult<NamedArg> {
    // Try `name => value` first; backtrack to a positional value otherwise.
    if let Some((name, value)) = opt(named_arg_kv).parse_next(input)? {
        return Ok(NamedArg {
            name: Some(name),
            value,
        });
    }
    let value = expr(input)?;
    Ok(NamedArg { name: None, value })
}

fn named_arg_kv(input: &mut Stream<'_>) -> ModalResult<(Ident, Expr)> {
    let name = ident(input)?;
    let _ = punct(Token::Arrow).parse_next(input)?;
    let value = expr(input)?;
    Ok((name.node, value))
}

// ---- effect statements ----------------------------------------------------

fn effect_stmt(input: &mut Stream<'_>) -> ModalResult<EffectStmt> {
    alt((insert_stmt, upsert_stmt, update_stmt, remove_stmt)).parse_next(input)
}

fn insert_stmt(input: &mut Stream<'_>) -> ModalResult<EffectStmt> {
    let _ = insert_into(input)?;
    write_target(input, EffectVerb::Insert)
}

fn upsert_stmt(input: &mut Stream<'_>) -> ModalResult<EffectStmt> {
    let _ = upsert_into(input)?;
    write_target(input, EffectVerb::Upsert)
}

/// Shared tail for `INSERT INTO`/`UPSERT INTO`: `<path> [|> ENCODE <fmt>] ( VALUES… |
/// <pipeline> ) [RETURNING …]`.
///
/// The optional `|> ENCODE <fmt>` between the target and the body names the wire-body encoding
/// of a §13 declared MAP upload (`… |> ENCODE multipart VALUES (row)`, ticket 20260711121526).
/// It desugars onto the EXISTING `EffectBody::Pipeline` shape — the `VALUES` source with one
/// `ENCODE` stage — so the closed statement/AST shapes are untouched and the stored body
/// round-trips through serde like any other effect.
fn write_target(input: &mut Stream<'_>, verb: EffectVerb) -> ModalResult<EffectStmt> {
    let target = path_expr(input)?;
    let encode = opt(preceded(
        punct(Token::Pipe),
        preceded(kw(Keyword::Encode), codec),
    ))
    .parse_next(input)?;
    let body = match encode {
        Some(codec) => {
            // Committed to the encode form: the body must be a VALUES literal (the declared-map
            // shape; a sub-pipeline body writes rows, not one wire body).
            let vals = cut_err(values).parse_next(input)?;
            EffectBody::Pipeline(Box::new(Pipeline {
                source: Source::Values(vals),
                ops: vec![PipeOp::Encode(codec)],
            }))
        }
        None => alt((
            values.map(EffectBody::Values),
            pipeline.map(|p| EffectBody::Pipeline(Box::new(p))),
        ))
        .parse_next(input)?,
    };
    let returning = opt(returning_clause).parse_next(input)?;
    Ok(EffectStmt {
        verb,
        target,
        body,
        returning,
    })
}

fn update_stmt(input: &mut Stream<'_>) -> ModalResult<EffectStmt> {
    let _ = kw(Keyword::Update).parse_next(input)?;
    let target = path_expr(input)?;
    let _ = kw(Keyword::Set).parse_next(input)?;
    let set = assignment_list(input)?;
    let filter = opt(preceded(kw(Keyword::Where), expr)).parse_next(input)?;
    let returning = opt(returning_clause).parse_next(input)?;
    Ok(EffectStmt {
        verb: EffectVerb::Update,
        target,
        body: EffectBody::SetWhere { set, filter },
        returning,
    })
}

fn remove_stmt(input: &mut Stream<'_>) -> ModalResult<EffectStmt> {
    let remove = kw(Keyword::Remove).parse_next(input)?;
    // `REMOVE TRANSFORM <name>` — the definition-layer drop for a transform (blueprint §15):
    // `TRANSFORM` is a contextual noun (no `DROP` keyword). Desugars to `REMOVE /transform WHERE
    // name == '<name>'` — a `REMOVE` effect is inherently irreversible (plan-side), so the commit
    // gate demands the explicit acknowledgement, exactly like `REMOVE TABLE`.
    if opt(word("TRANSFORM")).parse_next(input)?.is_some() {
        let name = cut_err(ident).parse_next(input)?.node;
        let filter = Expr::Binary {
            op: Op::Eq,
            lhs: Box::new(Expr::Col("name".to_string())),
            rhs: Box::new(Expr::Lit(Literal::Str(name))),
        };
        return Ok(EffectStmt {
            verb: EffectVerb::Remove,
            target: PathExpr {
                segments: vec![plain_segment("transform")],
                as_of: None,
                span: remove,
            },
            body: EffectBody::SetWhere {
                set: Vec::new(),
                filter: Some(filter),
            },
            returning: None,
        });
    }
    // `REMOVE TABLE /sql/<conn>/<table>` — the definition-layer drop (ADR 0009): `REMOVE` is the
    // one frozen destructive verb, `TABLE` the contextual noun (no `DROP` keyword). Desugars to
    // the catalog remove `REMOVE /sql/<conn> WHERE name == '<table>'` — inherently irreversible,
    // so the commit gate demands the explicit acknowledgement.
    if opt(word("TABLE")).parse_next(input)?.is_some() {
        let full = cut_err(path_expr).parse_next(input)?;
        let Some((last, parent)) = full.segments.split_last() else {
            return Err(ErrMode::Cut(ContextError::new()));
        };
        if parent.len() < 2 {
            return Err(ErrMode::Cut(ContextError::new()));
        }
        let filter = Expr::Binary {
            op: Op::Eq,
            lhs: Box::new(Expr::Col("name".to_string())),
            rhs: Box::new(Expr::Lit(Literal::Str(last.name.clone()))),
        };
        return Ok(EffectStmt {
            verb: EffectVerb::Remove,
            target: PathExpr {
                segments: parent.to_vec(),
                as_of: None,
                span: full.span,
            },
            body: EffectBody::SetWhere {
                set: Vec::new(),
                filter: Some(filter),
            },
            returning: None,
        });
    }
    let target = path_expr(input)?;
    let filter = opt(preceded(kw(Keyword::Where), expr)).parse_next(input)?;
    let returning = opt(returning_clause).parse_next(input)?;
    Ok(EffectStmt {
        verb: EffectVerb::Remove,
        target,
        body: EffectBody::SetWhere {
            set: Vec::new(),
            filter,
        },
        returning,
    })
}

fn returning_clause(input: &mut Stream<'_>) -> ModalResult<Vec<Projection>> {
    preceded(kw(Keyword::Returning), projection_list).parse_next(input)
}

// ---- CONNECT / DISCONNECT — defined-path bindings (EPIC 20260701100000) -----
//
// A `CONNECT /<path> TO <driver> [AT '<loc>'] [SECRET '<ref>']` binds a user-chosen PATH to a
// driver + credential; `CONNECT /<path> TO /<existing>` is an ALIAS (reuse a connection);
// `DISCONNECT /<path>` removes a binding. `CONNECT`/`DISCONNECT` are **contextual idents** (like
// `CONNECTION`/`SECRET`/`AT`) — NO new frozen keyword (the t31 `AT` lesson).
//
// They are **SUGAR** over a write to the `/sys/paths` administration node (the defined-path binding
// registry, persisted in the Project DB) — exactly as `CREATE JOB` is sugar over `INSERT INTO
// /server/jobs`. Desugaring here (in the parser, to the existing `Statement::Effect`) means the
// whole effect→preview→commit machinery, the capability gate, and the CLOSED-CORE `Statement` set
// are reused UNCHANGED: no new `Statement` variant, no new `EffectKind`. A full/alias CONNECT is an
// `UPSERT INTO /sys/paths` (upsert on `path`); a DISCONNECT is a `REMOVE /sys/paths/<path>`.

/// The `/sys/paths` binding-registry columns, in the order the desugar emits them (the sys applier
/// reads them by NAME, so the order is internal). A defined path stores selectors + metadata only —
/// `secret_ref` is a REFERENCE (`env:`/`vault:`), never a secret value.
const PATH_BINDING_COLUMNS: [&str; 8] = [
    "path",
    "driver",
    "at",
    "secret_ref",
    "alias_of",
    "host",
    "account",
    "app",
];

/// A plain (unversioned, non-glob) path segment — the shape the synthetic `/sys/paths` target uses.
fn plain_segment(name: &str) -> PathSegment {
    PathSegment {
        name: name.to_string(),
        version: None,
        glob: false,
    }
}

/// Render a parsed defined [`PathExpr`] to its canonical `/a/b/c` string (the value stored in the
/// binding's `path` column). Segments only — version/glob/`AS OF` coordinates are not part of a
/// mount identity.
fn canonical_path(path: &PathExpr) -> String {
    let mut out = String::new();
    for seg in &path.segments {
        out.push('/');
        out.push_str(&seg.name);
    }
    out
}

/// Build the single-row `VALUES` payload the `/sys/paths` upsert carries. Absent optional fields are
/// `NULL` (an alias has no driver/at/secret; a full connect has no alias target).
#[allow(clippy::too_many_arguments)]
fn binding_values(
    path: &str,
    driver: Option<&str>,
    at: Option<&str>,
    secret_ref: Option<&str>,
    alias_of: Option<&str>,
    host: Option<&str>,
    account: Option<&str>,
    app: Option<&str>,
) -> Values {
    let lit = |v: Option<&str>| {
        v.map_or(Expr::Lit(Literal::Null), |s| {
            Expr::Lit(Literal::Str(s.to_string()))
        })
    };
    Values {
        columns: Some(
            PATH_BINDING_COLUMNS
                .iter()
                .map(|c| (*c).to_string())
                .collect(),
        ),
        rows: vec![vec![
            Expr::Lit(Literal::Str(path.to_string())),
            lit(driver),
            lit(at),
            lit(secret_ref),
            lit(alias_of),
            lit(host),
            lit(account),
            lit(app),
        ]],
    }
}

/// Wrap a binding row into the `INSERT INTO /sys/paths …` effect. Like the other gated `/sys`
/// writes (`/sys/settings`, `/sys/billing`), the SURFACE verb is `INSERT` and the backend applies
/// **upsert-on-`path`** semantics (re-connecting a path replaces its binding) — an `INSERT` is
/// reversible, so a CONNECT auto-applies under the default safety mode (only DISCONNECT's `REMOVE`
/// needs an irreversible ack).
fn upsert_sys_paths(values: Values, span: Span) -> Statement {
    Statement::Effect(EffectStmt {
        verb: EffectVerb::Insert,
        target: PathExpr {
            segments: vec![plain_segment("sys"), plain_segment("paths")],
            as_of: None,
            span,
        },
        body: EffectBody::Values(values),
        returning: None,
    })
}

/// One `<name> <type> [PRIMARY KEY | UNIQUE | NOT NULL]*` column of a `CREATE TABLE` — the parsed
/// definition the desugar lowers into a `{ name, type, nullable, primary_key, unique }` struct
/// literal (the catalog row shape the SQL driver's applier decodes, ADR 0009 §1).
struct TableColumnDef {
    name: String,
    ty: String,
    nullable: bool,
    primary_key: bool,
    unique: bool,
}

/// A column type token in the §5.4 definition language. Base qfs column types stay as their
/// canonical lowercase token; any other bare or qualified name is a declared-type reference and is
/// stored as its absolute `/type/...` catalog path. Resolution stays out of the parser.
fn table_column_type_ref(input: &mut Stream<'_>) -> ModalResult<String> {
    let head = ident(input)?;
    let mut segments: Vec<String> = vec![head.node];
    let rest: Vec<Spanned<Ident>> =
        repeat(0.., preceded(punct(Token::Slash), cut_err(ident))).parse_next(input)?;
    for seg in rest {
        segments.push(seg.node);
    }
    if segments.len() == 1 {
        if let Some(base) = canonical_base_column_type(&segments[0]) {
            return Ok(base.to_string());
        }
    }
    declared_type_path(&segments.join("/")).ok_or_else(|| ErrMode::Cut(ContextError::new()))
}

/// Parse one column definition: a column name, a type name, then constraint words in any order.
/// `PRIMARY KEY` / `UNIQUE` / `NOT NULL` are contextual words (`NULL` is its literal token) — no
/// frozen keyword is added.
fn table_column_def(input: &mut Stream<'_>) -> ModalResult<TableColumnDef> {
    let name = ident(input)?.node;
    let ty = cut_err(table_column_type_ref).parse_next(input)?;
    let mut nullable = true;
    let mut primary_key = false;
    let mut unique = false;
    loop {
        if opt(word("PRIMARY")).parse_next(input)?.is_some() {
            let _ = cut_err(word("KEY")).parse_next(input)?;
            primary_key = true;
            continue;
        }
        if opt(word("UNIQUE")).parse_next(input)?.is_some() {
            unique = true;
            continue;
        }
        if opt(word("NOT")).parse_next(input)?.is_some() {
            let _ = cut_err(punct(Token::Null)).parse_next(input)?;
            nullable = false;
            continue;
        }
        break;
    }
    Ok(TableColumnDef {
        name,
        ty,
        nullable,
        primary_key,
        unique,
    })
}

/// `CREATE TABLE /sql/<conn>/<table> (<columns>…)` or `CREATE TABLE /sql/<conn>/<table> OF <name>` —
/// the relational **definition-layer** statement (ADR 0009 + blueprint §5.4). `TABLE`/`OF` are
/// contextual idents (zero new frozen keywords). Pure sugar (the t31 invariant): desugars to the
/// catalog write `INSERT INTO /sql/<conn> (name, columns|of_type) VALUES (…)` — exactly the plan the
/// raw catalog write builds, so the shell, preview/commit, capability gate, and applier all see one
/// shape. `columns` and `OF` are mutually exclusive; the binary-side SQL contract facet resolves
/// the declared type into columns at commit time.
fn create_table_stmt(input: &mut Stream<'_>) -> ModalResult<Statement> {
    let _ = kw(Keyword::Create).parse_next(input)?;
    let _ = word("TABLE").parse_next(input)?;
    // Committed after the noun: a malformed CREATE TABLE is a crisp error, never a fallthrough.
    let full = cut_err(path_expr).parse_next(input)?;
    // The path names the table INSIDE its catalog: at least `/sql/<conn>/<table>`.
    let Some((last, parent)) = full.segments.split_last() else {
        return Err(ErrMode::Cut(ContextError::new()));
    };
    if parent.len() < 2 {
        return Err(ErrMode::Cut(ContextError::new()));
    }
    let (columns, row): (Vec<String>, Vec<Expr>) = if opt(word("OF")).parse_next(input)?.is_some() {
        let of_type = cut_err(type_name).parse_next(input)?;
        (
            vec!["name".to_string(), "of_type".to_string()],
            vec![
                Expr::Lit(Literal::Str(last.name.clone())),
                Expr::Lit(Literal::Str(of_type)),
            ],
        )
    } else {
        let _ = cut_err(punct(Token::LParen)).parse_next(input)?;
        let cols: Vec<TableColumnDef> =
            cut_err(separated(1.., table_column_def, punct(Token::Comma))).parse_next(input)?;
        let _ = cut_err(punct(Token::RParen)).parse_next(input)?;
        let col_exprs: Vec<Expr> = cols
            .iter()
            .map(|c| {
                Expr::Struct(vec![
                    ("name".to_string(), Expr::Lit(Literal::Str(c.name.clone()))),
                    ("type".to_string(), Expr::Lit(Literal::Str(c.ty.clone()))),
                    ("nullable".to_string(), Expr::Lit(Literal::Bool(c.nullable))),
                    (
                        "primary_key".to_string(),
                        Expr::Lit(Literal::Bool(c.primary_key)),
                    ),
                    ("unique".to_string(), Expr::Lit(Literal::Bool(c.unique))),
                ])
            })
            .collect();
        (
            vec!["name".to_string(), "columns".to_string()],
            vec![
                Expr::Lit(Literal::Str(last.name.clone())),
                Expr::Array(col_exprs),
            ],
        )
    };
    Ok(Statement::Effect(EffectStmt {
        verb: EffectVerb::Insert,
        target: PathExpr {
            segments: parent.to_vec(),
            as_of: None,
            span: full.span,
        },
        body: EffectBody::Values(Values {
            columns: Some(columns),
            rows: vec![row],
        }),
        returning: None,
    }))
}

// ---------------------------------------------------------------------------
// §15 transform definitions — CREATE TRANSFORM / REMOVE TRANSFORM (blueprint §15, decision W).
//
// Like CREATE TABLE / CREATE DRIVER, these are **SUGAR** desugared HERE (in the parser) to an
// ordinary `INSERT INTO /transform` / `REMOVE /transform` effect — so the whole
// effect→preview→commit machinery, the capability gate, and the CLOSED-CORE `Statement` set are
// reused UNCHANGED (no new `Statement` variant, no new `EffectKind`). Every noun (`TRANSFORM`/
// `INPUT`/`OUTPUT`/`PROVIDER`/`MODEL`/`EFFORT` + the type words `array`/`struct`) is a contextual
// UPPERCASE ident (the `TABLE`/`CONNECTION` lesson) — ZERO new frozen keywords.
//
// **A transform definition is credential-free by construction (blueprint §15):** `SECRET` carries
// only a REFERENCE (`env:<VAR>` / `vault:<path>`), never an inline value — a non-reference is a
// parse error. The declared INPUT/OUTPUT schemas are stored as column-descriptor JSON (the
// CREATE TABLE `columns` convention); each column's type is the canonical string
// `qfs_types::ColumnType::parse` rehydrates (`text`, `bytes`, `array<struct<sku:text>>`, …).

/// The `/transform` definition-registry columns, in the order the desugar emits them (the transform
/// applier reads them by NAME, so the order is internal). Definition text + selectors + a secret
/// REFERENCE only — there is structurally no column an inline secret value could ride in.
const TRANSFORM_DECL_COLUMNS: [&str; 7] = [
    "name",
    "input",
    "output",
    "provider",
    "model",
    "effort",
    "secret_ref",
];

/// `CREATE TRANSFORM <name> INPUT (<col> <type>, …) OUTPUT (<col> <type>, …) PROVIDER <p> MODEL <m>
/// [EFFORT <e>] [SECRET '<ref>']` — a model-calling transform definition (blueprint §15, decision
/// W). Desugars to `INSERT INTO /transform` with the INPUT/OUTPUT schemas serialised as
/// column-descriptor JSON. INPUT/OUTPUT/PROVIDER/MODEL are required; EFFORT/SECRET are optional;
/// clauses may appear in any order (the sugar shape). A `SECRET` that is not an `env:`/`vault:`
/// reference is rejected here (no inline secret).
fn create_transform_stmt(input: &mut Stream<'_>) -> ModalResult<Statement> {
    let create = kw(Keyword::Create).parse_next(input)?;
    let _ = word("TRANSFORM").parse_next(input)?;
    // Committed after the noun: a malformed CREATE TRANSFORM is a crisp error, never a fallthrough.
    let name = cut_err(ident).parse_next(input)?.node;
    let mut input_schema: Option<String> = None;
    let mut output_schema: Option<String> = None;
    let mut provider: Option<String> = None;
    let mut model: Option<String> = None;
    let mut effort: Option<String> = None;
    let mut secret_ref: Option<String> = None;
    loop {
        if input_schema.is_none() {
            if let Some(v) = opt(transform_input_clause).parse_next(input)? {
                input_schema = Some(v);
                continue;
            }
        }
        if output_schema.is_none() {
            if let Some(v) = opt(transform_output_clause).parse_next(input)? {
                output_schema = Some(v);
                continue;
            }
        }
        if provider.is_none() {
            if let Some(v) = opt(transform_provider_clause).parse_next(input)? {
                provider = Some(v);
                continue;
            }
        }
        if model.is_none() {
            if let Some(v) = opt(transform_model_clause).parse_next(input)? {
                model = Some(v);
                continue;
            }
        }
        if effort.is_none() {
            if let Some(v) = opt(transform_effort_clause).parse_next(input)? {
                effort = Some(v);
                continue;
            }
        }
        if secret_ref.is_none() {
            if let Some(v) = opt(conn_secret_clause).parse_next(input)? {
                secret_ref = Some(v);
                continue;
            }
        }
        break;
    }
    let (Some(input_schema), Some(output_schema), Some(provider), Some(model)) =
        (input_schema, output_schema, provider, model)
    else {
        // A transform needs its INPUT/OUTPUT shapes and its PROVIDER/MODEL.
        return Err(ErrMode::Cut(ContextError::new()));
    };
    // `SECRET` is a REFERENCE (`env:`/`vault:`), NEVER an inline value — an inline secret is a parse
    // error (the credential-free-definition contract; the token lives in the vault/account layer).
    // The scheme list duplicates `qfs_core::ddl::transform::is_secret_reference` (the storage-side
    // re-validation): the parser deliberately depends only on qfs-lang (wasm-minimal), so the two
    // sites must be kept in sync by hand.
    if let Some(s) = &secret_ref {
        if !(s.starts_with("env:") || s.starts_with("vault:")) {
            return Err(ErrMode::Cut(ContextError::new()));
        }
    }
    let opt_lit =
        |v: Option<String>| v.map_or(Expr::Lit(Literal::Null), |s| Expr::Lit(Literal::Str(s)));
    let values = Values {
        columns: Some(
            TRANSFORM_DECL_COLUMNS
                .iter()
                .map(|c| (*c).to_string())
                .collect(),
        ),
        rows: vec![vec![
            Expr::Lit(Literal::Str(name)),
            Expr::Lit(Literal::Str(input_schema)),
            Expr::Lit(Literal::Str(output_schema)),
            Expr::Lit(Literal::Str(provider)),
            Expr::Lit(Literal::Str(model)),
            opt_lit(effort),
            opt_lit(secret_ref),
        ]],
    };
    Ok(Statement::Effect(EffectStmt {
        verb: EffectVerb::Insert,
        target: PathExpr {
            segments: vec![plain_segment("transform")],
            as_of: None,
            span: create,
        },
        body: EffectBody::Values(values),
        returning: None,
    }))
}

/// `INPUT ( <col> <type>, … )` — the declared input schema, serialised to column-descriptor JSON.
fn transform_input_clause(input: &mut Stream<'_>) -> ModalResult<String> {
    transform_schema_clause(input, "INPUT")
}

/// `OUTPUT ( <col> <type>, … )` — the declared output schema, serialised to column-descriptor JSON.
fn transform_output_clause(input: &mut Stream<'_>) -> ModalResult<String> {
    transform_schema_clause(input, "OUTPUT")
}

/// `<KEYWORD> ( <col> <type>, … )` — a parenthesised column list serialised to the canonical
/// `[{"name":…,"type":…,"nullable":…}]` JSON the backend rehydrates via `ColumnType::parse`.
fn transform_schema_clause(input: &mut Stream<'_>, kw_word: &'static str) -> ModalResult<String> {
    let _ = word(kw_word).parse_next(input)?;
    let _ = cut_err(punct(Token::LParen)).parse_next(input)?;
    let cols: Vec<(String, String, bool)> =
        cut_err(separated(1.., transform_column_def, punct(Token::Comma))).parse_next(input)?;
    let _ = cut_err(punct(Token::RParen)).parse_next(input)?;
    let arr: Vec<serde_json::Value> = cols
        .iter()
        .map(|(n, t, nullable)| serde_json::json!({ "name": n, "type": t, "nullable": nullable }))
        .collect();
    Ok(serde_json::Value::Array(arr).to_string())
}

/// One `<name> <type> [NOT NULL]` column of a transform INPUT/OUTPUT clause. Returns
/// `(name, canonical_type, nullable)`.
fn transform_column_def(input: &mut Stream<'_>) -> ModalResult<(String, String, bool)> {
    let name = ident(input)?.node;
    let ty = cut_err(transform_type).parse_next(input)?;
    let mut nullable = true;
    if opt(word("NOT")).parse_next(input)?.is_some() {
        let _ = cut_err(punct(Token::Null)).parse_next(input)?;
        nullable = false;
    }
    Ok((name, ty, nullable))
}

/// A transform column type in the canonical grammar `ColumnType::parse` reads back: a scalar token
/// (`text`/`int`/`bytes`/…), `array<TYPE>`, or `struct[<name TYPE, …>]`. `array`/`struct` are
/// contextual idents (no frozen keyword); the angle brackets are the `<`/`>` (`Lt`/`Gt`) tokens.
fn transform_type(input: &mut Stream<'_>) -> ModalResult<String> {
    if opt(word("ARRAY")).parse_next(input)?.is_some() {
        let _ = cut_err(punct(Token::Lt)).parse_next(input)?;
        let elem = cut_err(transform_type).parse_next(input)?;
        let _ = cut_err(punct(Token::Gt)).parse_next(input)?;
        return Ok(format!("array<{elem}>"));
    }
    if opt(word("STRUCT")).parse_next(input)?.is_some() {
        // The field list is optional: a bare `struct` is the empty record `struct<>`.
        // Adjacent `<>` lexes as a single `Token::Ne` (maximal munch), so the explicit
        // empty form `struct<>` arrives as one token, never `Lt Gt`.
        if opt(punct(Token::Ne)).parse_next(input)?.is_some() {
            return Ok("struct<>".to_string());
        }
        if opt(punct(Token::Lt)).parse_next(input)?.is_some() {
            let fields: Vec<(String, String, bool)> =
                cut_err(separated(0.., transform_column_def, punct(Token::Comma)))
                    .parse_next(input)?;
            let _ = cut_err(punct(Token::Gt)).parse_next(input)?;
            let inner: Vec<String> = fields.iter().map(|(n, t, _)| format!("{n}:{t}")).collect();
            return Ok(format!("struct<{}>", inner.join(",")));
        }
        return Ok("struct<>".to_string());
    }
    // A scalar type token (`text`/`int`/`bytes`/…), normalised to lowercase for the canonical form.
    let t = ident(input)?.node;
    Ok(t.to_ascii_lowercase())
}

/// `PROVIDER <p>` — the model provider selector (a bare ident or a quoted string).
fn transform_provider_clause(input: &mut Stream<'_>) -> ModalResult<String> {
    transform_word_or_string_clause(input, "PROVIDER")
}

/// `MODEL <m>` — the model name the provider is asked for (a bare ident or a quoted string).
fn transform_model_clause(input: &mut Stream<'_>) -> ModalResult<String> {
    transform_word_or_string_clause(input, "MODEL")
}

/// `EFFORT <e>` — the optional effort/budget hint (a bare ident or a quoted string).
fn transform_effort_clause(input: &mut Stream<'_>) -> ModalResult<String> {
    transform_word_or_string_clause(input, "EFFORT")
}

/// A `<KEYWORD> <value>` clause whose value is either a bare identifier or a quoted string (a model
/// id like `claude-sonnet-5` is a bare ident; a spaced value can be quoted). `<KEYWORD>` is a
/// contextual ident (no frozen keyword).
fn transform_word_or_string_clause(
    input: &mut Stream<'_>,
    kw_word: &'static str,
) -> ModalResult<String> {
    let _ = word(kw_word).parse_next(input)?;
    cut_err(alt((string_value, ident.map(|s| s.node)))).parse_next(input)
}

// ---------------------------------------------------------------------------
// §13 declared drivers (self-hosting integrations) — CREATE DRIVER / CREATE TYPE /
// declared parameterized CREATE VIEW / CREATE MAP.
//
// Like CREATE TABLE and CONNECT, these are **SUGAR** desugared HERE (in the parser) to an
// ordinary `INSERT INTO /sys/drivers` effect — so the whole effect→preview→commit machinery,
// the capability gate, and the CLOSED-CORE `Statement` set are reused UNCHANGED (no new
// `Statement` variant, no new `EffectKind`). Every noun (`DRIVER`/`TYPE`/`MAP`/`AUTH`/`BEARER`/
// `HEADER`/`OAUTH2`/`PAGINATE`/`CURSOR`/`LINK`/`OF`/`IRREVERSIBLE` + the sub-clause words) is a
// contextual UPPERCASE ident (the `TABLE`/`CONNECTION` lesson) — ZERO new frozen keywords.
//
// **Scripts are credential-free by construction (blueprint §13):** no AUTH form can carry a
// secret value — `BEARER` takes no argument, `HEADER` carries only the header NAME, `OAUTH2`
// carries only URLs + scopes. The token lives in the account layer (§8), never in a script.
//
// Structured driver config (auth/pagination) and view/map bodies are stored as JSON text: auth
// and pagination as small descriptors mirroring the shipped `AuthStrategy`/`Pagination` sums, and
// each view/map body as its serde AST JSON (the parser crate serializes; the evaluator rehydrates
// via serde without re-parse). The `/sys/drivers` row shape is experimental and may hard-break.

/// The `/sys/drivers` declaration-registry columns, in the order the desugar emits them (the sys
/// applier reads them by NAME, so the order is internal). One flat table tags each declaration by
/// `kind`; per-kind fields are `NULL` for the kinds that don't use them.
const DRIVER_DECL_COLUMNS: [&str; 9] = [
    "kind",         // driver | type | view | map
    "name",         // driver name (driver) or node path (type/view/map)
    "base_url",     // driver AT '<url>'
    "auth",         // driver auth descriptor JSON (never a secret)
    "pagination",   // driver pagination descriptor JSON
    "of_type",      // declared view's OF <type-path>
    "verb",         // declared map's mapped verb / CALL signature
    "body",         // type columns JSON | view/map body statement JSON
    "irreversible", // declared map's IRREVERSIBLE flag
];

/// Build the single-row `VALUES (...)` a `/sys/drivers` declaration carries. Absent per-kind fields
/// are `NULL` (mirrors `binding_values`).
#[allow(clippy::too_many_arguments)]
fn driver_row_values(
    kind: &str,
    name: &str,
    base_url: Option<&str>,
    auth: Option<&str>,
    pagination: Option<&str>,
    of_type: Option<&str>,
    verb: Option<&str>,
    body: Option<&str>,
    irreversible: bool,
) -> Values {
    let lit = |v: Option<&str>| {
        v.map_or(Expr::Lit(Literal::Null), |s| {
            Expr::Lit(Literal::Str(s.to_string()))
        })
    };
    Values {
        columns: Some(
            DRIVER_DECL_COLUMNS
                .iter()
                .map(|c| (*c).to_string())
                .collect(),
        ),
        rows: vec![vec![
            Expr::Lit(Literal::Str(kind.to_string())),
            Expr::Lit(Literal::Str(name.to_string())),
            lit(base_url),
            lit(auth),
            lit(pagination),
            lit(of_type),
            lit(verb),
            lit(body),
            Expr::Lit(Literal::Bool(irreversible)),
        ]],
    }
}

/// Wrap a declaration row into the `INSERT INTO /sys/drivers …` effect (like [`upsert_sys_paths`]).
fn insert_sys_drivers(values: Values, span: Span) -> Statement {
    Statement::Effect(EffectStmt {
        verb: EffectVerb::Insert,
        target: PathExpr {
            segments: vec![plain_segment("sys"), plain_segment("drivers")],
            as_of: None,
            span,
        },
        body: EffectBody::Values(values),
        returning: None,
    })
}

/// Serialize a validated body statement to its deterministic serde JSON (stored in the `body`
/// column; the evaluator rehydrates it via serde). A valid AST never fails to serialize; a
/// serializer error maps to a cut parse error rather than a panic (the panic-free invariant).
fn body_to_json(stmt: &Statement) -> ModalResult<String> {
    serde_json::to_string(stmt).map_err(|_| ErrMode::Cut(ContextError::new()))
}

/// `CREATE DRIVER <name> AT '<base-url>' AUTH <auth> [PAGINATE <page>]` — a declared driver.
/// Desugars to `INSERT INTO /sys/drivers` with `kind='driver'`. `AT` is required (a driver names
/// its wire host); `AUTH`/`PAGINATE` are optional and collected in any order (the sugar shape).
fn create_driver_stmt(input: &mut Stream<'_>) -> ModalResult<Statement> {
    let create = kw(Keyword::Create).parse_next(input)?;
    let _ = word("DRIVER").parse_next(input)?;
    // Committed after the noun: a malformed CREATE DRIVER is a crisp error, never a fallthrough.
    let name = cut_err(ident).parse_next(input)?.node;
    let mut base_url = None;
    let mut auth = None;
    let mut pagination = None;
    loop {
        if base_url.is_none() {
            if let Some(v) = opt(conn_at_clause).parse_next(input)? {
                base_url = Some(v);
                continue;
            }
        }
        if auth.is_none() {
            if let Some(v) = opt(driver_auth_clause).parse_next(input)? {
                auth = Some(v);
                continue;
            }
        }
        if pagination.is_none() {
            if let Some(v) = opt(driver_paginate_clause).parse_next(input)? {
                pagination = Some(v);
                continue;
            }
        }
        break;
    }
    let Some(base_url) = base_url else {
        // A driver must declare its wire host.
        return Err(ErrMode::Cut(ContextError::new()));
    };
    let auth = auth.unwrap_or_else(|| r#"{"kind":"none"}"#.to_string());
    let values = driver_row_values(
        "driver",
        &name,
        Some(&base_url),
        Some(&auth),
        pagination.as_deref(),
        None,
        None,
        None,
        false,
    );
    Ok(insert_sys_drivers(values, create))
}

/// `AUTH ( NONE | BEARER | HEADER '<name>' | OAUTH2 (authorize '<url>' token '<url>' scopes
/// '<…>') )` — the auth descriptor. Returns a JSON descriptor mirroring the shipped `AuthStrategy`
/// sum. **No variant carries a secret:** `BEARER` takes no value, `HEADER` only the header name,
/// `OAUTH2` only URLs + scopes.
fn driver_auth_clause(input: &mut Stream<'_>) -> ModalResult<String> {
    let _ = word("AUTH").parse_next(input)?;
    cut_err(alt((
        word("NONE").map(|_| r#"{"kind":"none"}"#.to_string()),
        word("BEARER").map(|_| r#"{"kind":"bearer"}"#.to_string()),
        auth_header_clause,
        auth_account_clause,
        auth_oauth2_clause,
    )))
    .parse_next(input)
}

/// `ACCOUNT '<provider>'` — bind the driver's auth to an EXISTING account provider's stored
/// credential (`AUTH ACCOUNT 'google'`, `AUTH ACCOUNT 'github'`). The declaration names only the
/// provider; the live bearer token is resolved from that provider's account/vault machinery at wire
/// time (running an OAuth refresh where the provider needs it), so an OAuth service is expressible in
/// the declared model without the declaration ever carrying a secret — the token stays in the vault.
fn auth_account_clause(input: &mut Stream<'_>) -> ModalResult<String> {
    let _ = word("ACCOUNT").parse_next(input)?;
    let provider = cut_err(string_value).parse_next(input)?;
    Ok(serde_json::json!({ "kind": "account", "provider": provider }).to_string())
}

/// `HEADER '<name>'` — a custom-header auth scheme carrying only the header NAME (the value lives in
/// the account layer).
fn auth_header_clause(input: &mut Stream<'_>) -> ModalResult<String> {
    let _ = word("HEADER").parse_next(input)?;
    let name = cut_err(string_value).parse_next(input)?;
    Ok(serde_json::json!({ "kind": "header", "name": name }).to_string())
}

/// `OAUTH2 (authorize '<url>' token '<url>' scopes '<…>')` — declares the OAuth2 endpoints + scopes
/// (all three required, any order). Carries no secret; the browser-consent values live in the vault.
fn auth_oauth2_clause(input: &mut Stream<'_>) -> ModalResult<String> {
    let _ = word("OAUTH2").parse_next(input)?;
    let _ = cut_err(punct(Token::LParen)).parse_next(input)?;
    let mut authorize = None;
    let mut token = None;
    let mut scopes = None;
    loop {
        if authorize.is_none() {
            if let Some(v) = opt(preceded(word("AUTHORIZE"), string_value)).parse_next(input)? {
                authorize = Some(v);
                continue;
            }
        }
        if token.is_none() {
            if let Some(v) = opt(preceded(word("TOKEN"), string_value)).parse_next(input)? {
                token = Some(v);
                continue;
            }
        }
        if scopes.is_none() {
            if let Some(v) = opt(preceded(word("SCOPES"), string_value)).parse_next(input)? {
                scopes = Some(v);
                continue;
            }
        }
        break;
    }
    let _ = cut_err(punct(Token::RParen)).parse_next(input)?;
    let (Some(authorize), Some(token), Some(scopes)) = (authorize, token, scopes) else {
        return Err(ErrMode::Cut(ContextError::new()));
    };
    Ok(
        serde_json::json!({ "kind": "oauth2", "authorize": authorize, "token": token, "scopes": scopes })
            .to_string(),
    )
}

/// `PAGINATE ( CURSOR (next '<field>' param '<name>' MAX <n>) | LINK MAX <n> )` — the pagination
/// descriptor, mirroring the shipped `Pagination` sum. Returns a JSON descriptor.
fn driver_paginate_clause(input: &mut Stream<'_>) -> ModalResult<String> {
    let _ = word("PAGINATE").parse_next(input)?;
    cut_err(alt((paginate_cursor_clause, paginate_link_clause))).parse_next(input)
}

/// `CURSOR (next '<field>' param '<name>' MAX <n>)` — cursor pagination (all three required).
fn paginate_cursor_clause(input: &mut Stream<'_>) -> ModalResult<String> {
    let _ = word("CURSOR").parse_next(input)?;
    let _ = cut_err(punct(Token::LParen)).parse_next(input)?;
    let mut next_field = None;
    let mut param = None;
    let mut max_pages = None;
    loop {
        if next_field.is_none() {
            if let Some(v) = opt(preceded(word("NEXT"), string_value)).parse_next(input)? {
                next_field = Some(v);
                continue;
            }
        }
        if param.is_none() {
            if let Some(v) = opt(preceded(word("PARAM"), string_value)).parse_next(input)? {
                param = Some(v);
                continue;
            }
        }
        if max_pages.is_none() {
            if let Some(v) = opt(preceded(word("MAX"), int_literal)).parse_next(input)? {
                max_pages = Some(v);
                continue;
            }
        }
        break;
    }
    let _ = cut_err(punct(Token::RParen)).parse_next(input)?;
    let (Some(next_field), Some(param), Some(max_pages)) = (next_field, param, max_pages) else {
        return Err(ErrMode::Cut(ContextError::new()));
    };
    Ok(
        serde_json::json!({ "kind": "cursor", "next_field": next_field, "param": param, "max_pages": max_pages })
            .to_string(),
    )
}

/// `LINK MAX <n>` — RFC 5988 Link-header pagination bounded by a page ceiling.
fn paginate_link_clause(input: &mut Stream<'_>) -> ModalResult<String> {
    let _ = word("LINK").parse_next(input)?;
    let max_pages = cut_err(preceded(word("MAX"), int_literal)).parse_next(input)?;
    Ok(serde_json::json!({ "kind": "link", "max_pages": max_pages }).to_string())
}

/// A bare non-negative integer literal (for `MAX <n>`).
fn int_literal(input: &mut Stream<'_>) -> ModalResult<i64> {
    any.verify_map(|t: Spanned<Token>| match t.node {
        Token::Int(i) => Some(i),
        _ => None,
    })
    .parse_next(input)
}

/// A definition NAME reference (blueprint §5.5): a bare, possibly `/`-qualified name — `email`,
/// `chatwork/message`, `cloudflare/zone` — canonicalized to the stored catalog path `/type/<segs>`.
/// **Paths are data, names are definitions:** a definition (type/transform) is referenced by NAME,
/// never by a `/type/…` path — that path is the *category error* the §5.5 rule forbids at a
/// reference site.
///
/// The name lexes as a leading [`Token::Ident`] plus zero or more `/` + identifier segments
/// (`chatwork` `/` `message`). A LEADING `/` — the retired `/type/…` path form — lexes as a
/// `Token::Path` with no leading ident, so the opening [`ident`] fails: the caller's `cut_err`
/// promotes that to a crisp parse error, steering back to the bare name form. `/type` stays the
/// catalog/shell face (`ls /type`, `describe /type/customer`) — those PATH forms address data (the
/// catalog), not a definition, and are unaffected.
fn type_name(input: &mut Stream<'_>) -> ModalResult<String> {
    let head = ident(input)?;
    let mut segments: Vec<String> = vec![head.node];
    let rest: Vec<Spanned<Ident>> =
        repeat(0.., preceded(punct(Token::Slash), cut_err(ident))).parse_next(input)?;
    for seg in rest {
        segments.push(seg.node);
    }
    declared_type_path(&segments.join("/")).ok_or_else(|| ErrMode::Cut(ContextError::new()))
}

/// `CREATE TYPE <name> ( <col> <type> [PRIMARY KEY|UNIQUE|NOT NULL], … ) [WHERE <pred>]` — a declared
/// type (the outward contract a declared view delivers `OF`). `<name>` is a bare, possibly qualified
/// NAME (`email`, `chatwork/message`) canonicalized to the stored catalog path `/type/<name>`
/// (§5.5); a leading `/type/…` PATH is rejected. Reuses the `CREATE TABLE` column-list parser;
/// desugars to `INSERT INTO /sys/drivers` with `kind='type'` and the columns as JSON in `body`.
fn create_type_stmt(input: &mut Stream<'_>) -> ModalResult<Statement> {
    let create = kw(Keyword::Create).parse_next(input)?;
    let _ = word("TYPE").parse_next(input)?;
    let name = cut_err(type_name).parse_next(input)?;
    let _ = cut_err(punct(Token::LParen)).parse_next(input)?;
    let cols: Vec<TableColumnDef> =
        cut_err(separated(1.., table_column_def, punct(Token::Comma))).parse_next(input)?;
    let _ = cut_err(punct(Token::RParen)).parse_next(input)?;
    // An optional row-local refinement predicate (blueprint §5.4): `WHERE <pred>`. Stored as the
    // serde value of the parsed `Expr` under `"where"`; well-formedness (row-local, pure, total,
    // boolean) is checked at DECLARE time and enforced as per-row MEMBERSHIP at the write/`OF`
    // boundary — never proof-carrying, contract-checked like a CHECK constraint.
    let refinement = opt(preceded(kw(Keyword::Where), cut_err(expr))).parse_next(input)?;
    let body = type_body_json(&cols, refinement.as_ref());
    let values = driver_row_values(
        "type",
        &name,
        None,
        None,
        None,
        None,
        None,
        Some(&body),
        false,
    );
    Ok(insert_sys_drivers(values, create))
}

/// Render a declared type's body to a JSON OBJECT `{"columns":[…],"where":<Expr|null>}` (blueprint
/// §5.4). Each column is `{name,type,nullable,primary_key,unique}` (the same shape `CREATE TABLE`
/// lowers a catalog row to); `where` is the serde value of the refinement `Expr` (a `None` predicate
/// serializes to `null`). This is a deliberate hard break of the pre-§5.4 array body shape
/// (pre-release, no back-compat).
fn type_body_json(cols: &[TableColumnDef], refinement: Option<&Expr>) -> String {
    let columns: Vec<DeclaredColumn> = cols
        .iter()
        .map(|c| DeclaredColumn {
            name: c.name.clone(),
            ty: c.ty.clone(),
            nullable: c.nullable,
            primary_key: c.primary_key,
            unique: c.unique,
        })
        .collect();
    serde_json::json!({ "columns": columns, "where": refinement }).to_string()
}

/// `CREATE VIEW /<path>[/{param}] [OF <name>] AS <pipeline>` — a declared, possibly parameterized,
/// view (a read over the wire mount).
///
/// **The dispatch is principled (§5.5), not incidental.** A view's OWN name discriminates the two
/// `CREATE VIEW` forms by what KIND of thing the name denotes: a **PATH** name (`CREATE VIEW
/// /chatwork/rooms`) is a *readable data surface* — a mount other queries `FROM` — so it is the
/// declared view; a **BARE** name (`CREATE VIEW recent`) is a *server binding*, an operator handle
/// that is not itself a path, so it soft-fails here and `server_ddl` claims it. The `OF <name>`
/// reference, by contrast, points at a DEFINITION (a declared type), so it is a bare, possibly
/// qualified NAME canonicalized to `/type/<name>` (§5.5) — never a `/type/…` path. The view's own
/// path stays a PATH; only the definition it delivers `OF` is name-ified. Desugars to `INSERT INTO
/// /sys/drivers` with `kind='view'`, the `OF` type in `of_type`, and the pipeline body as JSON.
fn create_declared_view_stmt(input: &mut Stream<'_>) -> ModalResult<Statement> {
    let create = kw(Keyword::Create).parse_next(input)?;
    let _ = kw(Keyword::View).parse_next(input)?;
    // A PATH name (leading `/`) is the declared-view discriminator; a bare ident soft-fails here so
    // the server-binding `CREATE VIEW <name>` claims it via `server_ddl`.
    let path = path_expr.parse_next(input)?;
    validate_template_path(&path)?;
    // Committed: from here a malformed declared view is a crisp error, not a fallthrough. The `OF`
    // reference is a definition NAME (§5.5), already canonicalized to `/type/<name>` by `type_name`.
    let of_type = opt(preceded(word("OF"), cut_err(type_name))).parse_next(input)?;
    let _ = cut_err(kw(Keyword::As)).parse_next(input)?;
    let body = cut_err(inner_statement).parse_next(input)?;
    let body_json = body_to_json(&body)?;
    let name = canonical_path(&path);
    let values = driver_row_values(
        "view",
        &name,
        None,
        None,
        None,
        of_type.as_deref(),
        None,
        Some(&body_json),
        false,
    );
    Ok(insert_sys_drivers(values, create))
}

/// `CREATE MAP <verb|CALL <driver>.<action>> /<node> AS <effect> [IRREVERSIBLE]` — a declared write
/// or CALL mapping from a universal verb on a declared node to a wire effect. Desugars to `INSERT
/// INTO /sys/drivers` with `kind='map'`.
fn create_map_stmt(input: &mut Stream<'_>) -> ModalResult<Statement> {
    let create = kw(Keyword::Create).parse_next(input)?;
    let _ = word("MAP").parse_next(input)?;
    let verb = cut_err(map_verb).parse_next(input)?;
    let node = cut_err(path_expr).parse_next(input)?;
    validate_template_path(&node)?;
    let _ = cut_err(kw(Keyword::As)).parse_next(input)?;
    let body = cut_err(inner_statement).parse_next(input)?;
    let irreversible = opt(word("IRREVERSIBLE")).parse_next(input)?.is_some();
    let body_json = body_to_json(&body)?;
    let name = canonical_path(&node);
    let values = driver_row_values(
        "map",
        &name,
        None,
        None,
        None,
        None,
        Some(&verb),
        Some(&body_json),
        irreversible,
    );
    Ok(insert_sys_drivers(values, create))
}

// ---- CREATE ACCOUNT — in-language account declaration (20260703040000) -------
//
// `CREATE ACCOUNT <provider> '<account>'` declares a service account (google/github/slack/objstore/
// cf) in the QUERY LANGUAGE, the in-language twin of `qfs account add`. It is SUGAR over an
// `INSERT INTO /sys/accounts` effect (like `CONNECT` over `/sys/paths`): no new `Statement` variant,
// no new frozen keyword (`ACCOUNT` is a contextual ident, already used by `CONNECT … ACCOUNT`). The
// apply path RECORDS CONSENT (the `connection_consent` ledger, gated on a signed-in operator — RFD
// §4.5 / the t54 gate); the token VALUE stays OUT-OF-BAND (stdin import / paste-back consent), never
// in the statement text (RFD §10). The `provider`/`account` are selectors, never a secret.

/// The `/sys/accounts` account-registry columns the desugar emits (the sys applier reads by NAME).
/// Selectors only — `secret_ref` is a REFERENCE (`env:`/`vault:`), resolved at USE time, NEVER an
/// inline secret value; the token itself is sealed out-of-band.
const ACCOUNT_COLUMNS: [&str; 4] = ["provider", "account", "app", "secret_ref"];

/// Build the single-row `VALUES (provider, account, app, secret_ref)` the `/sys/accounts` insert
/// carries. `secret_ref` is a reference selector, never a secret value.
fn account_values(
    provider: &str,
    account: &str,
    app: Option<&str>,
    secret_ref: Option<&str>,
) -> Values {
    let opt_lit = |v: Option<&str>| {
        v.map_or(Expr::Lit(Literal::Null), |s| {
            Expr::Lit(Literal::Str(s.to_string()))
        })
    };
    Values {
        columns: Some(ACCOUNT_COLUMNS.iter().map(|c| (*c).to_string()).collect()),
        rows: vec![vec![
            Expr::Lit(Literal::Str(provider.to_string())),
            Expr::Lit(Literal::Str(account.to_string())),
            opt_lit(app),
            opt_lit(secret_ref),
        ]],
    }
}

/// Wrap an account row into the `INSERT INTO /sys/accounts …` effect (like [`upsert_sys_paths`]).
fn insert_sys_accounts(values: Values, span: Span) -> Statement {
    Statement::Effect(EffectStmt {
        verb: EffectVerb::Insert,
        target: PathExpr {
            segments: vec![plain_segment("sys"), plain_segment("accounts")],
            as_of: None,
            span,
        },
        body: EffectBody::Values(values),
        returning: None,
    })
}

/// `CREATE ACCOUNT <provider> '<account>'` — declare a service account in the query language.
/// `<provider>` is a bare ident (`google`/`github`/`slack`/`objstore`/`cf`); `<account>` is a quoted
/// label (a Google email, or a credential label). Desugars to `INSERT INTO /sys/accounts`; the apply
/// path records consent under the signed-in operator (the token is sealed out-of-band). `ACCOUNT` is
/// a contextual ident (no new frozen keyword).
fn create_account_stmt(input: &mut Stream<'_>) -> ModalResult<Statement> {
    let create = kw(Keyword::Create).parse_next(input)?;
    let _ = word("ACCOUNT").parse_next(input)?;
    // Committed after the noun: a malformed CREATE ACCOUNT is a crisp error, never a fallthrough.
    let provider = cut_err(ident).parse_next(input)?.node;
    let account = cut_err(string_value).parse_next(input)?;
    // APP / SECRET ride as optional clauses in either order (the sugar-shape clause loop, mirroring
    // CONNECT). `SECRET '<ref>'` reuses `conn_secret_clause` so the reference grammar is identical
    // to CONNECT — a reference (`env:`/`vault:`), never an inline secret.
    let mut app: Option<String> = None;
    let mut secret_ref: Option<String> = None;
    loop {
        if app.is_none() {
            if let Some(v) = opt(conn_app_clause).parse_next(input)? {
                app = Some(v);
                continue;
            }
        }
        if secret_ref.is_none() {
            if let Some(v) = opt(conn_secret_clause).parse_next(input)? {
                secret_ref = Some(v);
                continue;
            }
        }
        break;
    }
    // `SECRET` is a REFERENCE (`env:`/`vault:`), NEVER an inline value — an inline non-reference
    // secret is a parse error (the credential-free-declaration contract; references only). The
    // scheme list mirrors `conn_secret_clause`'s consumers and `secret_ref.rs::resolve_secret_ref`.
    if let Some(s) = &secret_ref {
        if !(s.starts_with("env:") || s.starts_with("vault:")) {
            return Err(ErrMode::Cut(ContextError::new()));
        }
    }
    Ok(insert_sys_accounts(
        account_values(&provider, &account, app.as_deref(), secret_ref.as_deref()),
        create,
    ))
}

/// A declared-map verb: a universal verb (`INSERT`/`UPSERT`/`UPDATE`/`REMOVE`) or a
/// `CALL <driver>.<action>` signature. Returns the canonical label (e.g. `"INSERT"`,
/// `"CALL github.merge"`).
fn map_verb(input: &mut Stream<'_>) -> ModalResult<String> {
    if opt(kw(Keyword::Call)).parse_next(input)?.is_some() {
        let driver = cut_err(ident).parse_next(input)?.node;
        let _ = cut_err(punct(Token::Dot)).parse_next(input)?;
        let action = cut_err(ident).parse_next(input)?.node;
        return Ok(format!("CALL {driver}.{action}"));
    }
    policy_verb_token(input)
}

/// Reject a path whose `{param}` template segment collides with a glob (`*`/`?`) or an `@version`
/// coordinate — a template segment must be a clean `{name}`. Non-template paths pass through. Keeps
/// the `{param}` seam from overlapping the existing glob / `@version` path coordinates. A
/// query-string suffix (`{file}?create_download_url=1`, blueprint §13 wire paths) is validated on
/// the name BEFORE the `?` — the query rides behind the template, never inside it (the lexer's
/// query mode set the glob flag for the `?`, which is fine here because the pre-`?` head is what
/// must be clean).
fn validate_template_path(path: &PathExpr) -> ModalResult<()> {
    for seg in &path.segments {
        let head = seg
            .name
            .split_once('?')
            .map_or(seg.name.as_str(), |(h, _)| h);
        let is_template = head.contains('{') || head.contains('}');
        if is_template {
            let clean = head.starts_with('{')
                && head.ends_with('}')
                && head.matches('{').count() == 1
                && head.matches('}').count() == 1
                && head.len() > 2;
            if !clean || head.contains('*') || seg.version.is_some() {
                return Err(ErrMode::Cut(ContextError::new()));
            }
        }
    }
    Ok(())
}

/// `CONNECT /<path> TO ( <driver> [AT '…'] [SECRET '…'] | /<existing-path> )` — the defined-path
/// declaration. Disambiguated by what follows `TO`: a leading-`/` path is an ALIAS (reuse the
/// connection); a bare driver ident is a FULL connect. Desugars to an `UPSERT INTO /sys/paths`.
fn connect_stmt(input: &mut Stream<'_>) -> ModalResult<Statement> {
    let _ = word("CONNECT").parse_next(input)?;
    // Committed after the verb: a malformed CONNECT is a crisp error, never a silent fallthrough.
    let user = cut_err(path_expr).parse_next(input)?;
    if user.segments.is_empty() {
        // `CONNECT /` names no path.
        return Err(ErrMode::Cut(ContextError::new()));
    }
    let user_path = canonical_path(&user);
    let span = user.span;
    let _ = cut_err(word("TO")).parse_next(input)?;
    // ALIAS arm: a leading-`/` target path (a `Token::Path`) reusing an existing connection.
    if let Some(target) = opt(path_expr).parse_next(input)? {
        if target.segments.is_empty() {
            return Err(ErrMode::Cut(ContextError::new()));
        }
        let target_path = canonical_path(&target);
        let values = binding_values(
            &user_path,
            None,
            None,
            None,
            Some(&target_path),
            None,
            None,
            None,
        );
        return Ok(upsert_sys_paths(values, span));
    }
    // FULL connect: a bare driver ident, then optional `AT '<loc>'` / `SECRET '<ref>'` in any order.
    let driver = cut_err(ident).parse_next(input)?.node;
    let (at, secret, host, account, app) = connect_secret_clauses(input)?;
    let values = binding_values(
        &user_path,
        Some(&driver),
        at.as_deref(),
        secret.as_deref(),
        None,
        host.as_deref(),
        account.as_deref(),
        app.as_deref(),
    );
    Ok(upsert_sys_paths(values, span))
}

/// The optional `AT '<locator>'` / `SECRET '<ref>'` / `HOST '<name>'` / `ACCOUNT '<label>'` tail of
/// a full `CONNECT`, collected in any order (the sugar shape, like the `CREATE CONNECTION` clause
/// loop). Reuses the shared connection-clause parsers so the contextual idents are recognised
/// identically. `HOST`/`ACCOUNT` are the ADR 0008 mount coordinate: which qfs host owns the mount
/// (absent = the implicit `local`) and the service-account LABEL it binds (never a token) —
/// contextual idents like `AT`/`SECRET`, NOT frozen keywords (the t31 lesson).
#[allow(clippy::type_complexity)]
fn connect_secret_clauses(
    input: &mut Stream<'_>,
) -> ModalResult<(
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
)> {
    let mut at = None;
    let mut secret = None;
    let mut host = None;
    let mut account = None;
    let mut app = None;
    loop {
        if at.is_none() {
            if let Some(v) = opt(conn_at_clause).parse_next(input)? {
                at = Some(v);
                continue;
            }
        }
        if secret.is_none() {
            if let Some(v) = opt(conn_secret_clause).parse_next(input)? {
                secret = Some(v);
                continue;
            }
        }
        if host.is_none() {
            if let Some(v) = opt(conn_host_clause).parse_next(input)? {
                host = Some(v);
                continue;
            }
        }
        if account.is_none() {
            if let Some(v) = opt(conn_account_clause).parse_next(input)? {
                account = Some(v);
                continue;
            }
        }
        if app.is_none() {
            if let Some(v) = opt(conn_app_clause).parse_next(input)? {
                app = Some(v);
                continue;
            }
        }
        break;
    }
    Ok((at, secret, host, account, app))
}

/// `DISCONNECT /<path>` — remove a defined path. Desugars to `REMOVE /sys/paths/<path>` (the user
/// path rides as the segments after `paths`, so a multi-segment defined path removes cleanly).
fn disconnect_stmt(input: &mut Stream<'_>) -> ModalResult<Statement> {
    let _ = word("DISCONNECT").parse_next(input)?;
    let user = cut_err(path_expr).parse_next(input)?;
    if user.segments.is_empty() {
        return Err(ErrMode::Cut(ContextError::new()));
    }
    let mut segments = vec![plain_segment("sys"), plain_segment("paths")];
    segments.extend(user.segments.iter().cloned());
    Ok(Statement::Effect(EffectStmt {
        verb: EffectVerb::Remove,
        target: PathExpr {
            segments,
            as_of: None,
            span: user.span,
        },
        body: EffectBody::SetWhere {
            set: Vec::new(),
            filter: None,
        },
        returning: None,
    }))
}

// ---- writes as pipeline stages (decision Q, t72) --------------------------

/// The head of a terminal **pipe-stage write** (decision Q, t72): the write verb plus its
/// *own* operands — a target path for `INSERT INTO`/`UPSERT INTO`, the `SET` assignments for
/// `UPDATE`, nothing for `REMOVE`. The rows the write CONSUMES are the upstream pipeline (the
/// relation flowing into the stage), supplied separately by [`build_pipe_effect`]. Kept as a
/// small enum so the upstream → [`EffectStmt`] lowering is shared with — and byte-for-byte
/// identical to — the verb-leading [`effect_stmt`] form: two spellings, one plan.
enum PipeWriteHead {
    /// `|> INSERT INTO <target>` — the upstream is the inserted relation.
    Insert(PathExpr),
    /// `|> UPSERT INTO <target>` — the upstream is the upserted relation.
    Upsert(PathExpr),
    /// `|> UPDATE SET <assignments>` — target/filter are lifted from the upstream.
    Update(Vec<Assignment>),
    /// `|> REMOVE` — target/filter are lifted from the upstream.
    Remove,
}

/// Parse a terminal write stage's head after the `|>` has been consumed. Each arm backtracks
/// cleanly on its leading verb (so a non-write stage falls through to the ordinary
/// [`pipe_op`]); once a verb is committed the remainder is `cut_err` so a malformed write is a
/// crisp error pointing *inside* the stage, not a silent fallthrough.
fn pipe_write_head(input: &mut Stream<'_>) -> ModalResult<PipeWriteHead> {
    alt((
        preceded(insert_into, cut_err(path_expr)).map(PipeWriteHead::Insert),
        preceded(upsert_into, cut_err(path_expr)).map(PipeWriteHead::Upsert),
        preceded(
            kw(Keyword::Update),
            cut_err(preceded(kw(Keyword::Set), assignment_list)),
        )
        .map(PipeWriteHead::Update),
        kw(Keyword::Remove).map(|_| PipeWriteHead::Remove),
    ))
    .parse_next(input)
}

/// Lower a terminal pipe-stage write into the SAME [`EffectStmt`] the verb-leading
/// [`effect_stmt`] builds (decision Q: `… |> INSERT INTO /b` and `INSERT INTO /b …` are two
/// renderings of one effect, so they MUST produce one plan). `source`/`ops` are the upstream
/// relation the write consumes; an optional trailing `|> RETURNING …` stage (the pipe-stage
/// spelling of the verb-leading inline `RETURNING`) carries the projection.
fn build_pipe_effect(
    input: &mut Stream<'_>,
    head: PipeWriteHead,
    source: Source,
    ops: Vec<PipeOp>,
) -> ModalResult<EffectStmt> {
    // `RETURNING` rides as its own trailing `|>` stage in the pipe-stage form (the verb-leading
    // form takes it inline); both populate the same `EffectStmt.returning`. `opt` checkpoints,
    // so a missing `|> RETURNING` restores the cursor for the enclosing `eof`/clause.
    let returning = opt(preceded(
        punct(Token::Pipe),
        preceded(kw(Keyword::Returning), projection_list),
    ))
    .parse_next(input)?;
    match head {
        PipeWriteHead::Insert(target) => Ok(EffectStmt {
            verb: EffectVerb::Insert,
            target,
            body: EffectBody::Pipeline(Box::new(Pipeline { source, ops })),
            returning,
        }),
        PipeWriteHead::Upsert(target) => Ok(EffectStmt {
            verb: EffectVerb::Upsert,
            target,
            body: EffectBody::Pipeline(Box::new(Pipeline { source, ops })),
            returning,
        }),
        PipeWriteHead::Update(set) => {
            let (target, filter) = lift_target_filter(source, ops)?;
            Ok(EffectStmt {
                verb: EffectVerb::Update,
                target,
                body: EffectBody::SetWhere { set, filter },
                returning,
            })
        }
        PipeWriteHead::Remove => {
            let (target, filter) = lift_target_filter(source, ops)?;
            Ok(EffectStmt {
                verb: EffectVerb::Remove,
                target,
                body: EffectBody::SetWhere {
                    set: Vec::new(),
                    filter,
                },
                returning,
            })
        }
    }
}

/// A pipe-stage `UPDATE`/`REMOVE` rewrites rows *in place* on its upstream mount, so the
/// upstream must be a bare `/path` optionally narrowed by `WHERE` predicates — precisely what
/// the verb-leading `UPDATE <path> … WHERE …` / `REMOVE <path> WHERE …` expresses. Lift the
/// path as the target and AND-fold the `WHERE`s into the one filter, so the two spellings lower
/// identically. Any other upstream shape (a `SELECT`/`JOIN`/codec stage, a `VALUES`/subquery/
/// name source) cannot lower to that form and is a crisp error — decision Q: anything that is
/// neither legal form is rejected, never silently accepted.
fn lift_target_filter(source: Source, ops: Vec<PipeOp>) -> ModalResult<(PathExpr, Option<Expr>)> {
    let Source::Path(target) = source else {
        return Err(ErrMode::Cut(ContextError::new()));
    };
    let mut filter: Option<Expr> = None;
    for op in ops {
        let PipeOp::Where(pred) = op else {
            return Err(ErrMode::Cut(ContextError::new()));
        };
        filter = Some(match filter {
            None => pred,
            Some(prev) => Expr::Binary {
                op: Op::And,
                lhs: Box::new(prev),
                rhs: Box::new(pred),
            },
        });
    }
    Ok((target, filter))
}

/// A statement-position pipeline that may terminate in a **write stage** (decision Q, t72).
/// Without a terminal write it is an ordinary read [`Statement::Query`]; with one it lowers to
/// the SAME [`Statement::Effect`] the verb-leading [`effect_stmt`] builds. The write stage is
/// recognised ONLY here (statement position) — never inside a `(subquery)`, a `JOIN`/`UNION`
/// arm, or a `LET` value, all of which keep using the pure [`pipeline`] — so a write can never
/// hide in a read context and the §3 safety floor holds structurally.
fn pipeline_or_effect(input: &mut Stream<'_>) -> ModalResult<Statement> {
    let source = source(input)?;
    let mut ops: Vec<PipeOp> = Vec::new();
    let mut write: Option<PipeWriteHead> = None;
    loop {
        // A `|>` boundary. `&[T]` streams are `Copy`, so a non-`|>` token restores the cursor
        // for the enclosing parser (the `eof`, or a `LET` body / DDL clause that follows).
        let checkpoint = *input;
        if punct(Token::Pipe).parse_next(input).is_err() {
            *input = checkpoint;
            break;
        }
        // A terminal write verb wins over a normal op; a non-write stage backtracks (its leading
        // verb never matched) and falls through to the ordinary, cut-on-commit pipe op.
        if let Some(head) = opt(pipe_write_head).parse_next(input)? {
            write = Some(head);
            break;
        }
        ops.push(cut_err(pipe_op).parse_next(input)?);
    }
    match write {
        Some(head) => build_pipe_effect(input, head, source, ops).map(Statement::Effect),
        None => Ok(Statement::Query(Pipeline { source, ops })),
    }
}

// ---- server DDL -----------------------------------------------------------

fn server_ddl(input: &mut Stream<'_>) -> ModalResult<ServerDdl> {
    let _ = kw(Keyword::Create).parse_next(input)?;
    let kind = ddl_kind(input)?;
    let name = ident(input)?;
    // Clause grammar is permissive (sugar shape, not full validation): collect the
    // optional ON / EVERY / AS / DO clauses in any order.
    let mut on = None;
    let mut every = None;
    let mut as_query = None;
    let mut where_pred = None;
    let mut do_plan = None;
    let mut policy_rules: Vec<PolicyRuleAst> = Vec::new();
    let mut policy_attach: Option<String> = None;
    let mut driver: Option<String> = None;
    let mut at_locator: Option<String> = None;
    let mut secret_ref: Option<String> = None;
    loop {
        // `CREATE CONNECTION <name> DRIVER <driver> [AT '<loc>'] [SECRET '<ref>']` — the
        // connection-declaration clauses (contextual idents, no frozen keyword). Probed first for
        // the CONNECTION form so `DRIVER`/`SECRET` are consumed before the generic clause probes.
        if matches!(kind, DdlKind::Connection) {
            if driver.is_none() {
                if let Some(v) = opt(conn_driver_clause).parse_next(input)? {
                    driver = Some(v);
                    continue;
                }
            }
            if at_locator.is_none() {
                if let Some(v) = opt(conn_at_clause).parse_next(input)? {
                    at_locator = Some(v);
                    continue;
                }
            }
            if secret_ref.is_none() {
                if let Some(v) = opt(conn_secret_clause).parse_next(input)? {
                    secret_ref = Some(v);
                    continue;
                }
            }
        }
        // The `POLICY <name>` ATTACHMENT clause (t35) on a binding DDL — the policy a fired
        // plan commits under. `POLICY` is a frozen keyword (no new keyword). Only on a
        // non-POLICY DDL (for `CREATE POLICY` the leading POLICY is the kind, not an attach).
        if !matches!(kind, DdlKind::Policy) && policy_attach.is_none() {
            if let Some(v) = opt(policy_attach_clause).parse_next(input)? {
                policy_attach = Some(v);
                continue;
            }
        }
        // `CREATE POLICY … ALLOW … DENY …` rule clauses (t35). Parsed FIRST for the POLICY
        // form so the `ALLOW`/`DENY` contextual idents (not frozen keywords) are consumed
        // before the generic clause probes. A rule may carry its own `ON <driver-glob>`, so
        // this must win over the generic `on_clause` inside the POLICY form.
        if matches!(kind, DdlKind::Policy) {
            if let Some(rule) = opt(policy_rule_clause).parse_next(input)? {
                policy_rules.push(rule);
                continue;
            }
        }
        if on.is_none() {
            if let Some(v) = opt(on_clause).parse_next(input)? {
                on = Some(v);
                continue;
            }
        }
        if every.is_none() {
            if let Some(v) = opt(every_clause).parse_next(input)? {
                every = Some(v);
                continue;
            }
        }
        if as_query.is_none() {
            if let Some(v) = opt(as_clause).parse_next(input)? {
                as_query = Some(Box::new(v));
                continue;
            }
        }
        if where_pred.is_none() {
            if let Some(v) = opt(ddl_where_clause).parse_next(input)? {
                where_pred = Some(Box::new(v));
                continue;
            }
        }
        if do_plan.is_none() {
            if let Some(v) = opt(do_clause).parse_next(input)? {
                do_plan = Some(Box::new(v));
                continue;
            }
        }
        break;
    }
    let target = vec![
        "server".to_string(),
        ddl_kind_segment(kind).to_string(),
        name.node.clone(),
    ];
    Ok(ServerDdl {
        kind,
        name: name.node,
        target,
        do_plan,
        as_query,
        where_pred,
        every,
        on,
        policy_rules,
        policy: policy_attach,
        connection: (driver.is_some() || at_locator.is_some() || secret_ref.is_some()).then(|| {
            Box::new(ConnectionDeclAst {
                driver,
                at_locator,
                secret_ref,
            })
        }),
    })
}

/// `POLICY <name>` — the binding attachment clause (t35): the `/server/policies` row a fired
/// plan commits under. `POLICY` IS a frozen keyword; the name is a bare identifier.
fn policy_attach_clause(input: &mut Stream<'_>) -> ModalResult<String> {
    let _ = kw(Keyword::Policy).parse_next(input)?;
    ident(input).map(|s| s.node)
}

/// `(ALLOW|DENY) <verbs> [ON <driver-glob>]` — one `CREATE POLICY` rule clause (t35).
///
/// ## Keyword-freeze (the t31 `AT` lesson)
/// `ALLOW`/`DENY`/`ALL` are **NOT** in the frozen blueprint §3 keyword table; only `POLICY`/`ON` and
/// the verbs (`SELECT`/`UPDATE`/`REMOVE`/`CALL` as keywords; `INSERT`/`UPSERT` as the
/// `INTO`-lead idents) are frozen. So this binds over the **existing surface**: `ALLOW`/`DENY`/
/// `ALL` are matched as contextual identifiers ([`word`], case-insensitive) — adding no new closed-core
/// keyword — exactly as t31 bound `AT` and the DDL handles `MATERIALIZED`.
fn policy_rule_clause(input: &mut Stream<'_>) -> ModalResult<PolicyRuleAst> {
    let allow =
        alt((word("ALLOW").map(|_| true), word("DENY").map(|_| false))).parse_next(input)?;
    let (verbs, all_token) = policy_verb_list(input)?;
    // The optional per-rule `ON <driver-glob>` scope (`ON` IS a frozen keyword).
    let driver = opt(preceded(kw(Keyword::On), raw_token_text)).parse_next(input)?;
    // t57: the optional richer axes, collected in any order (sugar shape, like the DDL clauses).
    // `FOR`/`AT` are contextual UPPERCASE idents (no new keyword, the t31 `AT` lesson); `WHERE` is
    // a frozen keyword whose body is an ORDINARY expression (`member_of(...)` via `Expr::Fn`).
    let mut subject = None;
    let mut scope = None;
    let mut condition = None;
    loop {
        if subject.is_none() {
            if let Some(v) = opt(policy_for_clause).parse_next(input)? {
                subject = Some(v);
                continue;
            }
        }
        if scope.is_none() {
            if let Some(v) = opt(policy_at_clause).parse_next(input)? {
                scope = Some(v);
                continue;
            }
        }
        if condition.is_none() {
            if let Some(v) = opt(policy_where_clause).parse_next(input)? {
                condition = Some(v);
                continue;
            }
        }
        break;
    }
    Ok(PolicyRuleAst {
        allow,
        verbs,
        all_token,
        driver,
        subject,
        scope,
        condition,
    })
}

/// `FOR (user|role|group) <name>` — the optional t57 actor clause. `FOR` and the kind words are
/// contextual UPPERCASE idents (matched case-insensitively via [`word`]), so this adds NO frozen
/// keyword (the t31 `AT` lesson). The name is a bare identifier.
fn policy_for_clause(input: &mut Stream<'_>) -> ModalResult<PolicySubjectAst> {
    let _ = word("FOR").parse_next(input)?;
    let kind = alt((
        word("USER").map(|_| "user"),
        word("ROLE").map(|_| "role"),
        word("GROUP").map(|_| "group"),
    ))
    .parse_next(input)?;
    let name = ident(input)?;
    Ok(PolicySubjectAst {
        kind: kind.to_string(),
        name: name.node,
    })
}

/// `AT <path-glob>` — the optional t57 realm-scoped path clause. `AT` is a contextual ident (no
/// new keyword); the glob is captured as raw path text (`/members/alice/**`).
fn policy_at_clause(input: &mut Stream<'_>) -> ModalResult<String> {
    let _ = word("AT").parse_next(input)?;
    raw_token_text(input)
}

/// `WHERE <expr>` — the optional t57 conditional grant. `WHERE` IS a frozen keyword; the body is
/// an ordinary expression (a `member_of('/directories/...')` call — the `Expr::Fn` "functions are
/// values" seam), so NO keyword is added for the predicate itself.
fn policy_where_clause(input: &mut Stream<'_>) -> ModalResult<Expr> {
    let _ = kw(Keyword::Where).parse_next(input)?;
    expr(input)
}

/// A POLICY rule's verb list: the bare `ALL` token, or a comma-separated list of verbs. The
/// verbs span both lexer shapes — `SELECT`/`UPDATE`/`REMOVE`/`CALL` are reserved keyword
/// tokens, while `INSERT`/`UPSERT` are the bare `INTO`-lead idents (case-insensitive) — so this accepts
/// either. Returns `(verb_labels, was_all_token)`.
fn policy_verb_list(input: &mut Stream<'_>) -> ModalResult<(Vec<String>, bool)> {
    if opt(word("ALL")).parse_next(input)?.is_some() {
        return Ok((vec!["ALL".to_string()], true));
    }
    let mut verbs = vec![policy_verb_token(input)?];
    while opt(punct(Token::Comma)).parse_next(input)?.is_some() {
        verbs.push(policy_verb_token(input)?);
    }
    Ok((verbs, false))
}

/// A single POLICY verb token: a reserved verb keyword (`SELECT`/`UPDATE`/`REMOVE`/`CALL`) or a
/// bare verb ident (`insert`/`upsert`, case-insensitive). Returns the canonical uppercase label.
fn policy_verb_token(input: &mut Stream<'_>) -> ModalResult<String> {
    any.verify_map(|t: Spanned<Token>| match t.node {
        Token::Keyword(Keyword::Select) => Some("SELECT".to_string()),
        Token::Keyword(Keyword::Update) => Some("UPDATE".to_string()),
        Token::Keyword(Keyword::Remove) => Some("REMOVE".to_string()),
        Token::Keyword(Keyword::Call) => Some("CALL".to_string()),
        // `insert`/`upsert` are the multi-word verb leads (bare idents), matched
        // case-insensitively (t74) and normalized to the canonical uppercase label.
        Token::Ident(ref s) if s.eq_ignore_ascii_case("INSERT") => Some("INSERT".to_string()),
        Token::Ident(ref s) if s.eq_ignore_ascii_case("UPSERT") => Some("UPSERT".to_string()),
        _ => None,
    })
    .parse_next(input)
}

/// `WHERE <pred>` — the optional TRIGGER guard (t34). `WHERE` is a frozen keyword, so this adds no
/// new keyword. The predicate expression is wrapped as a `Statement::Query` over an EMPTY `VALUES`
/// source plus a single `PipeOp::Where(pred)`, so it round-trips through the downstream
/// `StatementSpec` (serde over the AST) with no new node kind: the dispatcher (t34) extracts the
/// `Where` op's `Expr` and evaluates it over `NEW.*`. The empty source is a structural carrier only
/// — it is never read; the dispatcher binds `NEW.*` into the predicate before evaluating it.
fn ddl_where_clause(input: &mut Stream<'_>) -> ModalResult<Statement> {
    let _ = kw(Keyword::Where).parse_next(input)?;
    let pred = expr(input)?;
    Ok(Statement::Query(Pipeline {
        source: Source::Values(Values {
            columns: None,
            rows: Vec::new(),
        }),
        ops: vec![PipeOp::Where(pred)],
    }))
}

fn ddl_kind(input: &mut Stream<'_>) -> ModalResult<DdlKind> {
    alt((
        kw(Keyword::Endpoint).map(|_| DdlKind::Endpoint),
        kw(Keyword::Trigger).map(|_| DdlKind::Trigger),
        kw(Keyword::Job).map(|_| DdlKind::Job),
        materialized_view.map(|()| DdlKind::MaterializedView),
        kw(Keyword::View).map(|_| DdlKind::View),
        kw(Keyword::Webhook).map(|_| DdlKind::Webhook),
        kw(Keyword::Policy).map(|_| DdlKind::Policy),
        // `CONNECTION` is a contextual ident (no frozen keyword), like `materialized`.
        word("CONNECTION").map(|_| DdlKind::Connection),
    ))
    .parse_next(input)
}

/// `DRIVER <driver>` — the connection's driver kind (a bare ident). `DRIVER` is a contextual ident.
fn conn_driver_clause(input: &mut Stream<'_>) -> ModalResult<String> {
    let _ = word("DRIVER").parse_next(input)?;
    ident(input).map(|s| s.node)
}

/// `AT '<locator>'` — the connection's non-secret location, a quoted string (consistent with the
/// rest of the grammar's literal locators). `AT` is a contextual ident.
fn conn_at_clause(input: &mut Stream<'_>) -> ModalResult<String> {
    let _ = word("AT").parse_next(input)?;
    string_value(input)
}

/// `SECRET '<ref>'` — the connection's secret REFERENCE (`env:<VAR>` / `vault:<path>`), a quoted
/// string. `SECRET` is a contextual ident; the value is a reference, never an inline secret.
fn conn_secret_clause(input: &mut Stream<'_>) -> ModalResult<String> {
    let _ = word("SECRET").parse_next(input)?;
    string_value(input)
}

/// `HOST '<name>'` — which qfs host owns the mount (ADR 0008 §1; absent = the implicit embedded
/// `local` host). A contextual ident, never a frozen keyword.
fn conn_host_clause(input: &mut Stream<'_>) -> ModalResult<String> {
    let _ = word("HOST").parse_next(input)?;
    string_value(input)
}

/// `ACCOUNT '<label>'` — the service-account LABEL the mount binds (ADR 0008 §4: the mount carries
/// the account, e.g. a Google email). A label/selector, never a token; a contextual ident, never a
/// frozen keyword.
fn conn_account_clause(input: &mut Stream<'_>) -> ModalResult<String> {
    let _ = word("ACCOUNT").parse_next(input)?;
    string_value(input)
}

/// `APP '<label>'` — the OAuth app LABEL that minted or services this account/mount.
fn conn_app_clause(input: &mut Stream<'_>) -> ModalResult<String> {
    let _ = word("APP").parse_next(input)?;
    string_value(input)
}

/// A single string-literal token's text (the `'…'` body).
fn string_value(input: &mut Stream<'_>) -> ModalResult<String> {
    any.verify_map(|t: Spanned<Token>| match t.node {
        Token::Str(s) => Some(s),
        _ => None,
    })
    .parse_next(input)
}

/// `materialized view` — `materialized` is an ident the lexer leaves bare (matched
/// case-insensitively), followed by the `view` keyword.
fn materialized_view(input: &mut Stream<'_>) -> ModalResult<()> {
    let _ = word("MATERIALIZED").parse_next(input)?;
    let _ = kw(Keyword::View).parse_next(input)?;
    Ok(())
}

fn ddl_kind_segment(kind: DdlKind) -> &'static str {
    match kind {
        DdlKind::Endpoint => "endpoints",
        DdlKind::Trigger => "triggers",
        DdlKind::Job => "jobs",
        DdlKind::View => "views",
        DdlKind::MaterializedView => "materialized_views",
        DdlKind::Webhook => "webhooks",
        DdlKind::Policy => "policies",
        DdlKind::Connection => "connections",
    }
}

/// `ON <event>` — the event/route token captured as raw text (an identifier, path,
/// or string). Stored unparsed (sugar; downstream desugars).
fn on_clause(input: &mut Stream<'_>) -> ModalResult<String> {
    let _ = kw(Keyword::On).parse_next(input)?;
    raw_token_text(input)
}

/// `EVERY <interval>` — interval captured as raw text.
fn every_clause(input: &mut Stream<'_>) -> ModalResult<String> {
    let _ = kw(Keyword::Every).parse_next(input)?;
    raw_token_text(input)
}

/// `AS <statement>` — the backing query for an ENDPOINT/VIEW.
fn as_clause(input: &mut Stream<'_>) -> ModalResult<Statement> {
    preceded(kw(Keyword::As), inner_statement).parse_next(input)
}

/// `DO <statement>` — the effect-plan body for a TRIGGER/JOB.
fn do_clause(input: &mut Stream<'_>) -> ModalResult<Statement> {
    preceded(kw(Keyword::Do), inner_statement).parse_next(input)
}

/// Capture a single token's raw textual form (for `ON`/`EVERY` operands). Names a
/// kind for non-textual tokens to avoid leaking literal values is unnecessary here
/// because these operands are routes/intervals, not credentials.
fn raw_token_text(input: &mut Stream<'_>) -> ModalResult<String> {
    any.verify_map(|t: Spanned<Token>| match t.node {
        Token::Ident(s) => Some(s),
        // A keyword word standing as a raw label operand (e.g. an `on webhook` event kind,
        // where `webhook` collides with the keyword). Recognized case-insensitively (t74);
        // its canonical lowercase text is the label. A pure surface accommodation — these
        // operands are routes/intervals/event-kinds, never effects.
        Token::Keyword(k) => Some(k.text().to_string()),
        Token::Str(s) => Some(s),
        Token::Int(i) => Some(i.to_string()),
        Token::Size { value, unit } => Some(format!("{value} {}", unit.text())),
        // A `Token::Path` is a leading-slash mount path (`/mail/inbox`); the lexer drops the
        // leading slash from the segments, so we re-prepend it. This is what lets a
        // `CREATE TRIGGER … ON /mail/inbox …` round-trip to the `/mail/inbox` source path the
        // watchtower (t34) re-queries — a slash-less `mail/inbox` would not resolve as a mount.
        Token::Path(segs) => Some(format!(
            "/{}",
            segs.into_iter()
                .map(|s| s.name)
                .collect::<Vec<_>>()
                .join("/")
        )),
        _ => None,
    })
    .parse_next(input)
}

// ---- statement top level --------------------------------------------------

/// An inner statement (no trailing-EOF requirement) — used for nested `AS`/`DO`
/// clauses and `PREVIEW`/`COMMIT` wrappers.
fn inner_statement(input: &mut Stream<'_>) -> ModalResult<Statement> {
    alt((
        plan_wrap,
        // A `TRANSACTION { … }` block (M6, t62): a distinct leading keyword, so order-independent.
        transaction_block,
        // `CREATE TABLE` (ADR 0009): the relational definition-layer statement, probed before the
        // server-DDL family so the contextual `TABLE` noun is claimed here. Backtracks cleanly
        // when the noun after `CREATE` is a server-DDL kind.
        // The `CREATE …` family, nested in one `alt` (keeps the outer tuple within winnow's arity):
        // `CREATE TABLE`, the §13 declared-driver forms (`DRIVER`/`TYPE`/`MAP` + the PATH-named
        // declared `VIEW`), then the server-binding DDL. Each backtracks cleanly to the next when
        // the noun after `CREATE` doesn't match (a bare-ident `CREATE VIEW <name>` falls to
        // `server_ddl`).
        alt((
            create_table_stmt,
            // §15 (decision W): `CREATE TRANSFORM <name> INPUT (…) OUTPUT (…) …` — a model-calling
            // transform definition, desugars to `INSERT INTO /transform`. Probed in the CREATE
            // family; backtracks cleanly when the noun after `CREATE` is not `TRANSFORM`.
            create_transform_stmt,
            create_driver_stmt,
            create_type_stmt,
            create_declared_view_stmt,
            create_map_stmt,
            // `CREATE ACCOUNT <provider> '<label>'` (20260703040000): the in-language account
            // declaration, desugars to `INSERT INTO /sys/accounts`. Backtracks cleanly when the
            // noun after `CREATE` is not `ACCOUNT`.
            create_account_stmt,
            server_ddl.map(Statement::Ddl),
        )),
        // CONNECT/DISCONNECT (EPIC 20260701100000): defined-path bindings, contextual-ident verbs
        // that desugar to a `/sys/paths` effect. Probed before the generic effect/pipeline forms so
        // the `connect`/`disconnect` lead idents are consumed as verbs, not a bare source name.
        connect_stmt,
        disconnect_stmt,
        // Verb-leading effect (`INSERT INTO …`, the source-less `VALUES` literal form among
        // them) wins first; a source-leading pipeline then either reads (`Statement::Query`)
        // or terminates in a write stage and lowers to the same `Statement::Effect` (t72).
        effect_stmt.map(Statement::Effect),
        pipeline_or_effect,
    ))
    .parse_next(input)
}

/// One statement inside a `TRANSACTION { … }` block (M6, ticket t62): an **effect** statement
/// only — a verb-leading `INSERT/UPSERT/UPDATE/REMOVE …` (the source-less `VALUES` literal form
/// among them) or a terminal pipe-stage write (`/path |> … |> REMOVE`). A pure read pipeline, a
/// nested `TRANSACTION`, a `LET`, a DDL, or a `PREVIEW`/`COMMIT` wrapper is rejected here (the
/// shape gate) so the block stays a thin, reversible-only wrapper over [`EffectStmt`]s; the
/// reversibility check itself is the eval-time gate, not this parser.
fn transaction_item(input: &mut Stream<'_>) -> ModalResult<Statement> {
    // Verb-leading effect wins first; backtracks cleanly when the next item is absent (`}`).
    if let Some(e) = opt(effect_stmt).parse_next(input)? {
        return Ok(Statement::Effect(e));
    }
    // Otherwise a source-leading pipeline that MUST terminate in a write stage. A bare read is
    // not a legal transaction member: once a non-effect is parsed it is a crisp authoring error
    // (`cut`), never a silent accept — decision G's reversible-only block holds no read.
    match pipeline_or_effect(input)? {
        eff @ Statement::Effect(_) => Ok(eff),
        _ => Err(ErrMode::Cut(ContextError::new())),
    }
}

/// A `TRANSACTION { <effect> ; <effect> ; … }` block (M6, ticket t62, decision G): a
/// reversible-only, all-or-nothing group of effect statements in commit-point (source) order.
/// Once `TRANSACTION` is consumed we are committed (`cut_err` on the braces), so a malformed
/// block is a crisp error pointing *inside* it. The effects are `;`-separated (a trailing `;` is
/// allowed) and an empty block parses to an empty (trivially reversible) plan. Reversibility is
/// enforced at eval time (`EvalError::IrreversibleInTransaction`), not here.
fn transaction_block(input: &mut Stream<'_>) -> ModalResult<Statement> {
    let open = kw(Keyword::Transaction).parse_next(input)?;
    let _ = cut_err(punct(Token::LBrace)).parse_next(input)?;
    let body: Vec<Statement> =
        separated(0.., transaction_item, punct(Token::Semicolon)).parse_next(input)?;
    // An optional trailing `;` after the final effect (winnow's `separated` leaves it unconsumed).
    let _ = opt(punct(Token::Semicolon)).parse_next(input)?;
    let close = cut_err(punct(Token::RBrace)).parse_next(input)?;
    Ok(Statement::Transaction {
        body,
        span: Span::new(open.start, close.end),
    })
}

/// `PREVIEW <stmt>` / `COMMIT <stmt>` — the plan wrapper (blueprint §7).
fn plan_wrap(input: &mut Stream<'_>) -> ModalResult<Statement> {
    let (commit, span) = alt((
        kw(Keyword::Preview).map(|s| (false, s)),
        kw(Keyword::Commit).map(|s| (true, s)),
    ))
    .parse_next(input)?;
    let inner = inner_statement(input)?;
    Ok(Statement::Plan(PlanWrap {
        commit,
        inner: Box::new(inner),
        span,
    }))
}

/// A `LET <name> = <pipeline>` binding followed by the rest of the program (M6, ticket
/// t60). Once `LET` is consumed we are committed (`cut_err`), so a malformed binding is a
/// crisp error pointing *inside* the binding. The `value` is restricted to a `pipeline`
/// (a relation) — never an effect — so a `LET` cannot smuggle a write into a pure context;
/// the `body` is the rest of the program, with `name` in scope.
fn let_binding(input: &mut Stream<'_>) -> ModalResult<Statement> {
    let _ = kw(Keyword::Let).parse_next(input)?;
    let name = cut_err(ident).parse_next(input)?;
    let _ = cut_err(punct(Token::Eq)).parse_next(input)?;
    let value = cut_err(let_value).parse_next(input)?;
    let body = cut_err(program_seq).parse_next(input)?;
    Ok(Statement::Let {
        name: name.node,
        value: Box::new(value),
        body: Box::new(body),
    })
}

/// The bound value of a `LET` (M6, ticket t60 + t61): a **relation** (a pipeline) OR a
/// first-class **value** (a lambda or a scalar expression — t61, decision H "functions are
/// values"). A *named function* is just a `LET`-bound lambda (`LET f = (x) => …`), so no
/// `DEF` keyword is needed.
///
/// The alternatives are ordered so each shape wins unambiguously:
/// 1. `lambda` first — a `( params ) => body` value; tried before `pipeline` because a
///    bare `(x)` would otherwise be read as a parenthesised sub-pipeline source.
/// 2. `pipeline` — the t60 relation binding (`/path |> …`, a bound name, `VALUES`, a
///    subquery). Unchanged, so every existing relation `LET` parses exactly as before.
/// 3. `expr` — a scalar value binding (`LET cutoff = '2026-03-27'`), reached only when the
///    value is neither a lambda nor a pipeline source.
///
/// A value binding (lambda/scalar) is retained as a single-cell `VALUES` relation so the
/// `Statement::Let { value: Box<Statement> }` shape — and its governance variant lock — is
/// **untouched** (no new `Statement` variant): the bound expression rides in the relation's
/// one row, available verbatim to a later type-checker / value runtime.
fn let_value(input: &mut Stream<'_>) -> ModalResult<Statement> {
    alt((
        lambda.map(wrap_value_binding),
        pipeline.map(Statement::Query),
        // A composite or scalar value binding (`LET atts = [ … ]`, `LET cutoff = '2026-03-27'`).
        // Restricted to a *constructor / literal* so it cannot collide with the `pipeline`
        // source forms above: a bare identifier stays a `LET`-bound relation name (`LET b = a`,
        // t60), and a fn-call (`map(…)`) belongs in a pipeline stage, not as a bare `LET` value
        // — keeping every existing relation binding parsing exactly as before.
        array_expr.map(wrap_value_binding),
        struct_expr.map(wrap_value_binding),
        scalar_literal.map(|lit| wrap_value_binding(Expr::Lit(lit))),
    ))
    .parse_next(input)
}

/// Wrap a `LET`-bound value expression (a lambda or scalar) into the single-cell `VALUES`
/// relation that carries it, so a value binding reuses the existing relation-valued
/// `Statement::Let` shape without adding a `Statement` variant (see [`let_value`]).
fn wrap_value_binding(value: Expr) -> Statement {
    Statement::Query(Pipeline {
        source: Source::Values(Values {
            columns: None,
            rows: vec![vec![value]],
        }),
        ops: vec![],
    })
}

/// One program: zero or more `LET` bindings in scope for the final statement (blueprint §1.2 —
/// statements with no terminator). Encoded as a let-in nesting (see [`Statement::Let`]).
/// A bare (non-`LET`) statement is the program's terminal statement; the top-level
/// `eof` then rejects a second non-binding statement (`FROM a` `FROM b`) as trailing input.
fn program_seq(input: &mut Stream<'_>) -> ModalResult<Statement> {
    alt((let_binding, inner_statement)).parse_next(input)
}

/// The top-level program parser: a `LET`-binding sequence then a final statement, then
/// end-of-input (one statement per line, `;`-free — blueprint §1.2 statement model).
fn statement(input: &mut Stream<'_>) -> ModalResult<Statement> {
    let stmt = program_seq(input)?;
    winnow::combinator::eof(input)?;
    Ok(stmt)
}
