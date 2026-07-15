//! WHERE → Gmail search `q=` pushdown (blueprint §7 "push down what the backend runs
//! natively; combine residual filters locally").
//!
//! Gmail's `messages.list` accepts a `q=` parameter in Gmail's search syntax (`from:`,
//! `subject:`, `after:`, `is:unread`, …). The planner lowers a typed `WHERE` predicate into
//! this query string for the subset of operators Gmail covers; predicates Gmail cannot express
//! are returned as **residual** for the engine to filter locally. This module is the pure
//! translation — it builds the `q=` string and reports the residual; it performs **no I/O** and
//! holds no token.
//!
//! ## Mapping (the covered subset)
//! Two classes of pushed term, and the residual discipline differs between them:
//!
//! **Exact** — the Gmail term means *exactly* the SQL predicate, so the predicate is fully
//! pushed and drops out of the residual (nothing to re-check locally):
//! - `label = 'INBOX'`     → `label:INBOX`     (exact label-id membership)
//! - `is_unread = true`    → `is:unread`       (exact `UNREAD`-label membership; `false` → `is:read`)
//! - a bare label scan (`/mail/<label>`) → its `label:<id>` term (exact directory scope)
//!
//! **Pre-filter (lossy)** — the Gmail term is *looser* than the SQL predicate, so it is pushed
//! as a cheap backend pre-filter to narrow the fetch, but the original predicate is **kept as
//! residual** and re-applied locally so only exactly-matching rows survive (over-fetch then
//! filter — blueprint §7; never wrong rows):
//! - `from = 'x@y'` → `from:x@y` (Gmail `from:` is an address/substring match, not exact
//!   `From`-header equality), and `to = 'x@y'` → `to:x@y` likewise.
//! - `subject = 'hello'` → `subject:hello` (Gmail `subject:` is a substring match —
//!   `subject:hello` matches `"hello world"`).
//! - `from`/`to`/`subject ~ 'p'` (`Match`) and `LIKE 'p'` → the same loose field operator
//!   (Gmail has no regex/`LIKE`-glob operator, so the term is a substring approximation).
//! - `date > <ts>` → `after:<unix>` and `date < <ts>` → `before:<unix>` (epoch-seconds; Gmail
//!   `after:`/`before:` are date-granular, so the bound is approximate). The bound may be an
//!   epoch-ms `Int` **or** a `date`/timestamp *string* (`'2024-01-01'`) — the string is coerced to
//!   epoch-ms, and the residual carries the coerced `Int` so the local re-check orders correctly.
//! - `date BETWEEN <low> AND <high>` → `after:<low> before:<high>` (both bounds coerced + pushed),
//!   the coerced BETWEEN kept as residual. BETWEEN on a **non-date** column still stays residual.
//! - `<a> AND <b>` → space-join (Gmail ANDs terms; each conjunct keeps its own residual).
//!
//! `OR`/`NOT`/`IN`/`BETWEEN`-on-non-date/unsupported columns push nothing and stay wholly
//! **residual** — a predicate qfs cannot push is always re-applied locally, never silently dropped.

use qfs_types::{CmpOp, ColRef, Literal, Predicate};

/// The pushed-down Gmail search string and the residual predicate the engine still filters
/// locally (blueprint §7). `query` is empty when nothing pushed down; `residual` is `None` when the
/// whole predicate pushed down.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PushdownResult {
    /// The Gmail `q=` search string (space-separated terms; empty if none pushed down).
    pub query: String,
    /// The predicate the backend could **not** express — the engine filters this locally.
    pub residual: Option<Predicate>,
}

/// Build the Gmail `q=` string for a `WHERE` predicate, scoped to an optional `label`
/// (`/mail/<label>` contributes an exact `label:<id>` term). Returns the pushed query + the
/// residual.
///
/// The translation is conservative about **correctness**, not about what it pushes: a term is
/// always pushed when Gmail can pre-filter on it, but a conjunct is dropped from the residual
/// **only** when its Gmail term means *exactly* the SQL predicate (the Exact class above —
/// `label`/`is_unread`). Every lossy term (the Pre-filter class — `from`/`to`/`subject`
/// `Eq`/`Match`/`LIKE`, and the `date` bounds) keeps its original predicate as residual so the
/// engine re-applies exact filtering locally. This honours blueprint §7's over-fetch-then-filter
/// contract: the result set is never wrong, even though Gmail's field operators are looser than
/// SQL equality.
#[must_use]
pub fn build_query(label: Option<&str>, predicate: Option<&Predicate>) -> PushdownResult {
    let mut terms: Vec<String> = Vec::new();
    if let Some(label) = label {
        terms.push(format!("label:{}", quote_term(label)));
    }
    let residual = match predicate {
        None => None,
        Some(p) => lower(p, &mut terms),
    };
    PushdownResult {
        query: terms.join(" "),
        residual,
    }
}

/// The outcome of lowering one comparison/`LIKE` into a Gmail search term.
enum Lowered {
    /// The Gmail term means *exactly* the SQL predicate — push it and drop the predicate from
    /// the residual (nothing to re-check). Only `label`/`is_unread` qualify.
    Exact(String),
    /// The Gmail term is *looser* than the SQL predicate — push `term` as a backend pre-filter but
    /// keep `residual` (the exact predicate) for the engine to re-apply locally. `residual` is
    /// usually the original comparison, but a `date` bound carries the **coerced** epoch-ms form
    /// (`date <op> Int(ms)`) so the local re-check compares `Timestamp`↔`Int`, not `Timestamp`↔the
    /// original date *string* the engine cannot order against.
    PreFilter { term: String, residual: Predicate },
}

/// Lower one predicate, appending its pushed terms to `terms` and returning the residual
/// (the part the engine must still filter locally). A conjunction pushes each conjunct
/// independently; an exact conjunct drops out of the residual while a lossy conjunct stays in
/// it (pushed *and* kept). Any other shape that does not map cleanly stays wholly residual.
fn lower(p: &Predicate, terms: &mut Vec<String>) -> Option<Predicate> {
    match p {
        // AND distributes: push each side, AND the residuals back together.
        Predicate::And(a, b) => {
            let ra = lower(a, terms);
            let rb = lower(b, terms);
            match (ra, rb) {
                (None, None) => None,
                (Some(r), None) | (None, Some(r)) => Some(r),
                (Some(ra), Some(rb)) => Some(Predicate::And(Box::new(ra), Box::new(rb))),
            }
        }
        Predicate::Cmp(col, op, lit) => match lower_cmp(col, *op, lit) {
            // Exact mapping: fully pushed, no residual.
            Some(Lowered::Exact(term)) => {
                terms.push(term);
                None
            }
            // Lossy mapping: push the pre-filter term BUT keep the exact (coerced) predicate as
            // residual so the engine re-checks it — over-fetch then filter, never wrong rows (blueprint §7).
            Some(Lowered::PreFilter { term, residual }) => {
                terms.push(term);
                Some(residual)
            }
            None => Some(p.clone()),
        },
        // `date BETWEEN <low> AND <high>` → `after:<low> before:<high>` (both bounds pushed as a
        // date-granular pre-filter), keeping the coerced-to-epoch-ms BETWEEN as residual so the
        // engine re-applies the exact bounds locally. An unparseable bound pushes nothing and
        // stays wholly residual — never a silent no-op, never a wrong row.
        Predicate::Between(col, low, high) if field_of(col) == Some("date") => {
            match (date_literal_to_ms(low), date_literal_to_ms(high)) {
                (Some(lo), Some(hi)) => {
                    terms.push(format!("after:{}", lo / 1000));
                    terms.push(format!("before:{}", hi / 1000));
                    Some(Predicate::Between(
                        col.clone(),
                        Literal::Int(lo),
                        Literal::Int(hi),
                    ))
                }
                _ => Some(p.clone()),
            }
        }
        Predicate::Like(col, pattern) => match field_of(col) {
            // `LIKE` has no Gmail operator; the field substring is a loose pre-filter only, so
            // push it and keep the `LIKE` predicate as residual for exact local re-matching.
            Some(field @ ("from" | "to" | "subject")) => {
                terms.push(format!("{field}:{}", quote_term(&pattern.0)));
                Some(p.clone())
            }
            _ => Some(p.clone()),
        },
        // OR / NOT / IN / BETWEEN — Gmail's term ANDing does not express these cleanly, so they
        // stay residual and the engine filters locally (correctness over completeness, blueprint §7).
        other => Some(other.clone()),
    }
}

/// Lower a single comparison into a Gmail search term, tagged [`Lowered::Exact`] when the term
/// means exactly the predicate or [`Lowered::PreFilter`] when it is a looser pre-filter, or
/// `None` if Gmail cannot express it at all.
fn lower_cmp(col: &ColRef, op: CmpOp, lit: &Literal) -> Option<Lowered> {
    let field = field_of(col)?;
    match (field, op, lit) {
        // Header/text equality and regex-match map to Gmail's field operators, but `from:`/`to:`
        // are address/substring matches and `subject:` is a substring match — all LOOSER than
        // SQL `=` (and than `~`'s regex). Pre-filter only; the engine re-checks the exact pred.
        (f @ ("from" | "to" | "subject"), CmpOp::Eq | CmpOp::Match, Literal::Text(v)) => {
            Some(Lowered::PreFilter {
                term: format!("{f}:{}", quote_term(v)),
                residual: Predicate::Cmp(col.clone(), op, lit.clone()),
            })
        }
        // A label-id equality scopes to exactly that label (`label:` is exact label membership).
        ("label", CmpOp::Eq, Literal::Text(v)) => {
            Some(Lowered::Exact(format!("label:{}", quote_term(v))))
        }
        // is_unread = true → is:unread; = false → is:read. Exact UNREAD-label membership.
        ("is_unread", CmpOp::Eq, Literal::Bool(b)) => Some(Lowered::Exact(format!(
            "is:{}",
            if *b { "unread" } else { "read" }
        ))),
        // Date range → after:/before: with a unix-seconds bound. Accepts an epoch-ms `Int` OR a
        // `date`/timestamp **string** (`'2024-01-01'`, `'2024-01-01T12:00:00Z'`) — the string is
        // coerced to epoch-ms here (previously it hit the `_ => None` arm and the whole predicate
        // silently vanished, so time-range search was impossible). The Gmail bound is date-granular
        // and the ms→s truncation drops sub-second precision, so it is a pre-filter; the residual
        // carries the coerced `date <op> Int(ms)` for the engine's exact local re-check.
        ("date", CmpOp::Gt | CmpOp::Ge, lit) => {
            date_literal_to_ms(lit).map(|ms| Lowered::PreFilter {
                term: format!("after:{}", ms / 1000),
                residual: Predicate::Cmp(col.clone(), op, Literal::Int(ms)),
            })
        }
        ("date", CmpOp::Lt | CmpOp::Le, lit) => {
            date_literal_to_ms(lit).map(|ms| Lowered::PreFilter {
                term: format!("before:{}", ms / 1000),
                residual: Predicate::Cmp(col.clone(), op, Literal::Int(ms)),
            })
        }
        _ => None,
    }
}

/// Coerce a `date` literal to epoch **milliseconds** for the `after:`/`before:` lowering. An
/// `Int` is already epoch-ms; a `Text` literal is parsed as a civil date (`YYYY-MM-DD`, midnight
/// UTC) or a `YYYY-MM-DDThh:mm:ss` timestamp. Returns `None` for a shape we cannot parse, so the
/// caller keeps the predicate residual instead of pushing a bogus bound (never silently wrong).
fn date_literal_to_ms(lit: &Literal) -> Option<i64> {
    match lit {
        Literal::Int(ms) => Some(*ms),
        Literal::Text(s) => parse_date_to_ms(s),
        _ => None,
    }
}

/// Parse `YYYY-MM-DD` (→ midnight UTC) or `YYYY-MM-DDThh:mm:ss…` into epoch **milliseconds**, or
/// `None` if the leading shape is not a civil date. Pure integer math (Howard Hinnant), no dep.
fn parse_date_to_ms(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    // The date part `YYYY-MM-DD` is mandatory (with `-` separators).
    if b.len() < 10 || b.get(4) != Some(&b'-') || b.get(7) != Some(&b'-') {
        return None;
    }
    let num = |a: usize, z: usize| -> Option<i64> { s.get(a..z).and_then(|p| p.parse().ok()) };
    let (y, mo, d) = (num(0, 4)?, num(5, 7)?, num(8, 10)?);
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) {
        return None;
    }
    let days = days_from_civil(y, mo as u32, d as u32);
    // Optional `Thh:mm:ss` (seconds granularity; a trailing zone/fraction is ignored — the ms→s
    // truncation makes the Gmail bound date-granular anyway, and the residual re-checks exactly).
    let (h, mi, se) = if b.len() >= 19 && matches!(b.get(10), Some(&b'T') | Some(&b' ')) {
        (
            num(11, 13).unwrap_or(0),
            num(14, 16).unwrap_or(0),
            num(17, 19).unwrap_or(0),
        )
    } else {
        (0, 0, 0)
    };
    Some((days * 86_400 + h * 3600 + mi * 60 + se) * 1000)
}

/// Days since the Unix epoch for a civil date (Howard Hinnant's algorithm). Pure integer math.
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let m = i64::from(m);
    let d = i64::from(d);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The single-segment column name of a [`ColRef`], if it is a bare column (not a dotted path).
fn field_of(col: &ColRef) -> Option<&str> {
    match col.path.as_slice() {
        [one] => Some(one.as_str()),
        _ => None,
    }
}

/// Quote a Gmail search term value when it contains whitespace, so a multi-word subject stays
/// one term (`subject:"two words"`). A value with no whitespace is emitted bare.
fn quote_term(value: &str) -> String {
    if value.chars().any(char::is_whitespace) {
        format!("\"{}\"", value.replace('"', ""))
    } else {
        value.to_string()
    }
}
