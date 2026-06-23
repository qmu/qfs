//! WHERE → Slack `conversations.history` query-param pushdown (RFD-0001 §6 "push down what the
//! backend runs natively; combine residual filters locally").
//!
//! ## Scoped to `oldest`/`latest`/`limit` (the ticket scopes richer pushdown to E3)
//! `conversations.history`/`.replies` accept a **time window** + a page size as native params:
//! `oldest`/`latest` bound the message `ts` range and `limit` caps the page. The planner lowers a
//! typed `WHERE` on the `ts` column into these params; anything Slack cannot express stays
//! **residual** for the engine to filter locally.
//!
//! ## The t20 lesson — TRUTHFUL residual (the headline invariant)
//! A Slack time-window param is an **inclusive/exclusive boundary** on `ts`, so a `ts >= X` lowers
//! to `oldest=X` (Slack's `oldest` is inclusive by default) and `ts <= X` to `latest=X`. These are
//! exact for the boundary they express, so the conjunct **drops** from the residual. Any other
//! predicate (`text LIKE`, `user =`, `OR`, …) Slack cannot push, so it is **kept** residual and the
//! engine re-filters locally — over-fetch then filter, never wrong rows. This module is the pure
//! translation: it builds the param list + the residual; it performs **no I/O** and holds no token.

use cfs_types::{CmpOp, ColRef, Literal, Predicate};

/// The pushed-down Slack query params and the residual predicate the engine still filters locally
/// (RFD §6). `params` is empty when nothing pushed down; `residual` is `None` when the whole
/// predicate pushed down exactly.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PushdownResult {
    /// The `(key, value)` query params to add to the request (stable order).
    pub params: Vec<(String, String)>,
    /// The predicate the backend could **not** express — the engine filters this locally.
    pub residual: Option<Predicate>,
}

/// Build the Slack history query params for a `WHERE` predicate on a message log. Returns the
/// pushed params (`oldest`/`latest`) + the residual.
///
/// Only the message-log nodes (messages/replies/dms) push a time window; the relational `users`
/// directory and the blob `files` namespace push nothing here (their predicate stays wholly
/// residual — correctness over completeness).
#[must_use]
pub fn build_params(predicate: Option<&Predicate>) -> PushdownResult {
    let mut params: Vec<(String, String)> = Vec::new();
    let residual = match predicate {
        None => None,
        Some(p) => lower(p, &mut params),
    };
    PushdownResult { params, residual }
}

/// Lower one predicate, appending its pushed params to `params` and returning the residual. A
/// conjunction pushes each conjunct independently; an exact `ts`-boundary conjunct drops out of the
/// residual while every other conjunct stays in it. Any non-`AND` shape stays wholly residual.
fn lower(p: &Predicate, params: &mut Vec<(String, String)>) -> Option<Predicate> {
    match p {
        Predicate::And(a, b) => {
            let ra = lower(a, params);
            let rb = lower(b, params);
            match (ra, rb) {
                (None, None) => None,
                (Some(r), None) | (None, Some(r)) => Some(r),
                (Some(ra), Some(rb)) => Some(Predicate::And(Box::new(ra), Box::new(rb))),
            }
        }
        Predicate::Cmp(col, op, lit) => match lower_cmp(col, *op, lit) {
            // The `ts` boundary param means exactly the comparison — push it, drop the predicate.
            Some(param) => {
                params.push(param);
                None
            }
            None => Some(p.clone()),
        },
        // OR / NOT / IN / BETWEEN / LIKE — Slack's window params cannot express these, so they
        // stay residual and the engine filters locally (correctness over completeness).
        other => Some(other.clone()),
    }
}

/// Lower a single comparison on `ts` into a Slack window param, or `None` if Slack cannot express
/// it. `ts >= X` / `ts > X` → `oldest=X`; `ts <= X` / `ts < X` → `latest=X`. Slack's `oldest` is
/// inclusive and `latest` inclusive; the boundary the param expresses is exactly the comparison's
/// boundary, so the residual can be dropped (the param is the whole truth for that conjunct).
fn lower_cmp(col: &ColRef, op: CmpOp, lit: &Literal) -> Option<(String, String)> {
    let field = field_of(col)?;
    if field != "ts" {
        return None;
    }
    let value = match lit {
        Literal::Text(v) => v.clone(),
        Literal::Int(v) => v.to_string(),
        _ => return None,
    };
    match op {
        CmpOp::Gt | CmpOp::Ge => Some(("oldest".to_string(), value)),
        CmpOp::Lt | CmpOp::Le => Some(("latest".to_string(), value)),
        _ => None,
    }
}

/// The single-segment column name of a [`ColRef`], if it is a bare column (not a dotted path).
fn field_of(col: &ColRef) -> Option<&str> {
    match col.path.as_slice() {
        [one] => Some(one.as_str()),
        _ => None,
    }
}
