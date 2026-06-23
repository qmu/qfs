//! The **injection-safe typed parameter rewrite** (t32, the hard part).
//!
//! The endpoint query is stored (t31) as a span-normalised, pre-parsed
//! [`cfs_core::StatementSpec`] — the AST, NOT source text. At request time this module walks
//! that AST and replaces every **bare column reference** ([`Expr::Col`]) whose identifier
//! matches a declared route/query param with an [`Expr::Lit`] carrying the **typed**
//! [`cfs_core::Value`] bound for it ([`crate::QueryArgs`]).
//!
//! ## Why this is injection-safe
//!   * The query is parsed exactly ONCE, at registration. The request value NEVER re-enters
//!     the parser and is NEVER concatenated into DSL text.
//!   * A param value becomes a single, typed [`Expr::Lit`] AST node. A malicious path param
//!     `'; REMOVE /mail/inbox` becomes `Expr::Lit(Literal::Str("'; REMOVE /mail/inbox"))` —
//!     one string-literal leaf. Its lowered plan is therefore **structurally identical** to
//!     the plan for a benign string (the injection golden asserts this).
//!   * The rewrite touches only `Expr::Col` nodes that match a declared param name; every
//!     other node (real column references, function calls, the source path) is untouched.
//!
//! ## Param convention
//! Because the frozen grammar has no `:param` placeholder token, a param slot is written as a
//! bare identifier matching a declared route param, and that identifier **must be distinct
//! from any column name** the query references (so the rewrite cannot ambiguously substitute a
//! column). The natural authoring form:
//! `CREATE ENDPOINT GET /items/:p_id AS (FROM /mock/items |> WHERE id = p_id)` — the route
//! param `:p_id` is distinct from the `id` column, so the LHS `id` stays a column and the RHS
//! `p_id` is the param slot bound from the path segment. EVERY [`Expr::Col`] whose identifier
//! is a declared param is substituted; a param name colliding with a column name would
//! (incorrectly) substitute the column too, hence the distinct-name convention. (When t34/t35
//! introduce a first-class `:param` token, this convention is replaced by an unambiguous
//! placeholder node — the rewrite stays the same, only the matched node kind changes.)

use cfs_parser::{
    CallRef, EffectBody, Expr, FnRef, JoinOp, Literal, PipeOp, Pipeline, PlanWrap, Projection,
    Source, Statement,
};

use cfs_core::Value;

use crate::params::QueryArgs;

/// Rewrite `stmt` in place: replace each declared-param column reference with its typed
/// literal. Only identifiers present in `args` are substituted; everything else is preserved.
/// This is the single mutation the request applies to the pre-parsed query — there is no
/// re-parse, so the request carries zero parse-time injection surface.
pub fn bind_params(stmt: &mut Statement, args: &QueryArgs) {
    match stmt {
        Statement::Query(p) => rewrite_pipeline(p, args),
        Statement::Effect(e) => {
            match &mut e.body {
                EffectBody::Values(v) => rewrite_values(v, args),
                EffectBody::Pipeline(p) => rewrite_pipeline(p, args),
                EffectBody::SetWhere { set, filter } => {
                    for a in set {
                        rewrite_expr(&mut a.value, args);
                    }
                    if let Some(f) = filter {
                        rewrite_expr(f, args);
                    }
                }
            }
            if let Some(projs) = &mut e.returning {
                for proj in projs {
                    rewrite_projection(proj, args);
                }
            }
        }
        Statement::Ddl(d) => {
            if let Some(p) = &mut d.do_plan {
                bind_params(p, args);
            }
            if let Some(q) = &mut d.as_query {
                bind_params(q, args);
            }
        }
        Statement::Plan(PlanWrap { inner, .. }) => bind_params(inner, args),
    }
}

fn rewrite_pipeline(p: &mut Pipeline, args: &QueryArgs) {
    rewrite_source(&mut p.source, args);
    for op in &mut p.ops {
        rewrite_pipe_op(op, args);
    }
}

fn rewrite_source(s: &mut Source, args: &QueryArgs) {
    match s {
        // A path source is a structural address (the mount), never a value slot — leave it.
        Source::Path(_) => {}
        Source::Values(v) => rewrite_values(v, args),
        Source::Subquery(p) => rewrite_pipeline(p, args),
    }
}

fn rewrite_values(v: &mut cfs_parser::Values, args: &QueryArgs) {
    for row in &mut v.rows {
        for e in row {
            rewrite_expr(e, args);
        }
    }
}

fn rewrite_pipe_op(op: &mut PipeOp, args: &QueryArgs) {
    match op {
        PipeOp::Where(e) => rewrite_expr(e, args),
        PipeOp::Select(projs) | PipeOp::Aggregate(projs) => {
            for p in projs {
                rewrite_projection(p, args);
            }
        }
        PipeOp::Extend(assigns) | PipeOp::Set(assigns) => {
            for a in assigns {
                rewrite_expr(&mut a.value, args);
            }
        }
        PipeOp::GroupBy(exprs) => {
            for e in exprs {
                rewrite_expr(e, args);
            }
        }
        PipeOp::OrderBy(keys) => {
            for k in keys {
                rewrite_expr(&mut k.expr, args);
            }
        }
        PipeOp::Join(JoinOp { source, on }) => {
            rewrite_source(source, args);
            rewrite_expr(on, args);
        }
        PipeOp::Union(p) | PipeOp::Except(p) | PipeOp::Intersect(p) => rewrite_pipeline(p, args),
        PipeOp::Call(c) => rewrite_call(c, args),
        // No expression payload to rewrite.
        PipeOp::Limit(_)
        | PipeOp::Distinct
        | PipeOp::As(_)
        | PipeOp::Expand(_)
        | PipeOp::Decode(_)
        | PipeOp::Encode(_) => {}
    }
}

fn rewrite_projection(p: &mut Projection, args: &QueryArgs) {
    if let Projection::Expr { expr, .. } = p {
        rewrite_expr(expr, args);
    }
}

fn rewrite_call(c: &mut CallRef, args: &QueryArgs) {
    for a in &mut c.args {
        rewrite_expr(&mut a.value, args);
    }
}

fn rewrite_fn(f: &mut FnRef, args: &QueryArgs) {
    for a in &mut f.args {
        rewrite_expr(a, args);
    }
}

/// The core substitution: a bare [`Expr::Col`] whose identifier is a declared param becomes a
/// typed [`Expr::Lit`]. Recurses into composite expressions. A [`Expr::Path`] (struct
/// navigation `a.b`) is NOT a param slot — params are single bare identifiers — so it is left
/// as a column reference even if its first segment happens to match a param name.
fn rewrite_expr(e: &mut Expr, args: &QueryArgs) {
    match e {
        Expr::Col(ident) => {
            if let Some(value) = args.get(ident) {
                *e = Expr::Lit(value_to_literal(value));
            }
        }
        Expr::Fn(f) => rewrite_fn(f, args),
        Expr::Binary { lhs, rhs, .. } => {
            rewrite_expr(lhs, args);
            rewrite_expr(rhs, args);
        }
        Expr::Unary { expr, .. } => rewrite_expr(expr, args),
        Expr::In { expr, set } | Expr::AnyOp { expr, set, .. } => {
            rewrite_expr(expr, args);
            for s in set {
                rewrite_expr(s, args);
            }
        }
        Expr::Between { expr, low, high } => {
            rewrite_expr(expr, args);
            rewrite_expr(low, args);
            rewrite_expr(high, args);
        }
        Expr::Like { expr, pattern } => {
            rewrite_expr(expr, args);
            rewrite_expr(pattern, args);
        }
        // A literal is already a value; a path is struct navigation, not a param slot.
        Expr::Lit(_) | Expr::Path(_) => {}
    }
}

/// Convert a typed [`cfs_core::Value`] to a parser [`Literal`] AST node. This is a pure data
/// mapping — the value's content is carried verbatim into a typed leaf, never re-parsed.
/// `Null` becomes the null literal; structured/blob values (which a scalar param never
/// produces from [`crate::params::infer_value`]) degrade to their textual form rather than
/// panicking, keeping the rewrite total + injection-safe.
fn value_to_literal(value: &Value) -> Literal {
    match value {
        Value::Null => Literal::Null,
        Value::Bool(b) => Literal::Bool(*b),
        Value::Int(i) | Value::Timestamp(i) => Literal::Int(*i),
        Value::Float(f) => Literal::Float(*f),
        Value::Text(s) => Literal::Str(s.clone()),
        // A scalar request param never yields these; carry a safe textual rendering so the
        // substitution is total (the value is still a single typed literal node).
        other => Literal::Str(format!("{other:?}")),
    }
}
