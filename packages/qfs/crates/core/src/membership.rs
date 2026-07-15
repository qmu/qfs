//! Per-row **MEMBERSHIP** enforcement for a `CREATE TYPE … WHERE <pred>` refinement (blueprint
//! §5.4). A refinement predicate is a row-local, pure, total boolean; this module *checks a
//! delivered row against it* — the enforcement half of the contract, run at the write/`OF`
//! boundary (the declare-time well-formedness half is [`crate::ddl::types`]).
//!
//! The check is a pure evaluation: bind the row's columns into a value environment by `schema`,
//! evaluate the predicate through the pure lambda evaluator ([`crate::lambda`]) under a
//! capability-denied [`EvalCtx`] (no I/O, no `env`/`NOW`), and require the result be `true`. A
//! `false`, `NULL`, or non-boolean result is a contract violation (like a failed `CHECK`), returned
//! as a structured [`MembershipError`] naming the predicate and the columns it constrains — never
//! the offending row value (a contract error carries the shape, not the data).

use qfs_types::{Row, Schema, Value};

use qfs_parser::ast::Expr;

use crate::lambda::{eval_expr, LambdaValue, ValueEnv};
use crate::stdlib::{EvalCtx, NoEnv, StdlibRegistry};

/// Why a delivered row is not a member of a refined type (blueprint §5.4) — the row did not satisfy
/// the type's `WHERE` predicate. Secret-free: it names the predicate and the constrained columns,
/// never the row's values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MembershipError {
    /// A stable rendering of the refinement predicate that the row failed.
    pub predicate: String,
    /// The declared columns the predicate constrains (those it references).
    pub columns: Vec<String>,
    /// A stable reason code when the predicate could not be *evaluated* (rather than simply
    /// evaluating to a non-true result); `None` for a plain contract violation.
    pub eval_code: Option<&'static str>,
}

impl core::fmt::Display for MembershipError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "row is not a member of the refined type: it fails the predicate `{}`",
            self.predicate
        )?;
        if !self.columns.is_empty() {
            write!(f, " over column(s) {}", self.columns.join(", "))?;
        }
        if let Some(code) = self.eval_code {
            write!(f, " ({code})")?;
        }
        Ok(())
    }
}

impl std::error::Error for MembershipError {}

/// Check that `row` is a member of a refined type: that it satisfies `predicate` under `schema`
/// (blueprint §5.4). Pure — binds the row's columns, evaluates the predicate through the pure
/// lambda evaluator, and returns a structured refusal unless the result is `Bool(true)`.
///
/// # Errors
/// [`MembershipError`] when the predicate evaluates to `false`, `NULL`, a non-boolean, or cannot be
/// evaluated — the delivered row is not a member of the type.
pub fn check_membership(
    schema: &Schema,
    predicate: &Expr,
    row: &Row,
) -> Result<(), MembershipError> {
    // Bind every declared column to its row value (a shorter/longer row degrades a missing cell to
    // `Null`, matching the projection/late-binding discipline elsewhere).
    let mut env = ValueEnv::new();
    for (i, col) in schema.columns.iter().enumerate() {
        let value = row.values.get(i).cloned().unwrap_or(Value::Null);
        env = env.bind(col.name.clone(), LambdaValue::Data(value));
    }

    // A refinement is row-local and pure: context built-ins (`NOW`/`env`/…) were rejected at
    // DECLARE time, so a deterministic, capability-denied context is exactly right here.
    let no_env = NoEnv;
    let ctx = EvalCtx::pure(0, 0, &no_env);
    let stdlib = StdlibRegistry::with_core();

    let violation = |eval_code: Option<&'static str>| MembershipError {
        predicate: render_predicate(predicate),
        columns: referenced_columns(predicate, schema),
        eval_code,
    };

    match eval_expr(predicate, &env, &stdlib, &ctx) {
        Ok(LambdaValue::Data(Value::Bool(true))) => Ok(()),
        // A `false` / `NULL` / non-boolean result is a contract violation (a refinement must be
        // satisfied — an unsatisfied or indeterminate result is not membership).
        Ok(_) => Err(violation(None)),
        Err(e) => Err(violation(Some(e.code()))),
    }
}

/// A stable, secret-free rendering of a refinement predicate for the error message. Uses the AST's
/// structural form (the predicate is a *declaration*, never row data).
fn render_predicate(expr: &Expr) -> String {
    format!("{expr:?}")
}

/// The declared columns a predicate references (its head column / struct-navigation heads that
/// resolve against `schema`), for the error's `columns` list. Deduplicated, declaration order.
fn referenced_columns(expr: &Expr, schema: &Schema) -> Vec<String> {
    let mut names = Vec::new();
    collect_columns(expr, &mut names);
    schema
        .columns
        .iter()
        .filter(|c| names.iter().any(|n| n == &c.name))
        .map(|c| c.name.clone())
        .collect()
}

/// Walk an `Expr`, collecting bare-column and struct-navigation head identifiers.
fn collect_columns(expr: &Expr, out: &mut Vec<String>) {
    match expr {
        Expr::Col(name) => out.push(name.clone()),
        Expr::Path(segs) => {
            if let Some(head) = segs.first() {
                out.push(head.clone());
            }
        }
        Expr::Fn(fnref) => {
            for arg in &fnref.args {
                collect_columns(arg, out);
            }
        }
        Expr::Lambda { body, .. } => collect_columns(body, out),
        Expr::Binary { lhs, rhs, .. } => {
            collect_columns(lhs, out);
            collect_columns(rhs, out);
        }
        Expr::Unary { expr, .. } => collect_columns(expr, out),
        Expr::In { expr, set } | Expr::AnyOp { expr, set, .. } => {
            collect_columns(expr, out);
            for m in set {
                collect_columns(m, out);
            }
        }
        Expr::Between { expr, low, high } => {
            collect_columns(expr, out);
            collect_columns(low, out);
            collect_columns(high, out);
        }
        Expr::Like { expr, pattern } => {
            collect_columns(expr, out);
            collect_columns(pattern, out);
        }
        Expr::Array(elems) => {
            for e in elems {
                collect_columns(e, out);
            }
        }
        Expr::Struct(fields) => {
            for (_, e) in fields {
                collect_columns(e, out);
            }
        }
        Expr::Lit(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use qfs_types::{Column, ColumnType};

    fn refinement(create_type: &str) -> Expr {
        let stmt = qfs_parser::parse_statement(create_type).expect("parses");
        let qfs_parser::Statement::Effect(effect) = stmt else {
            panic!("expected an effect");
        };
        let qfs_parser::EffectBody::Values(values) = &effect.body else {
            panic!("expected VALUES");
        };
        let cols = values.columns.as_ref().unwrap();
        let idx = cols.iter().position(|c| c == "body").unwrap();
        let qfs_parser::Expr::Lit(qfs_parser::Literal::Str(body)) = &values.rows[0][idx] else {
            panic!("string body");
        };
        let v: serde_json::Value = serde_json::from_str(body).unwrap();
        serde_json::from_value(v.get("where").unwrap().clone()).unwrap()
    }

    fn email_schema() -> Schema {
        Schema::new(vec![Column::new("value", ColumnType::Text, true)])
    }

    #[test]
    fn a_conforming_row_passes() {
        let pred = refinement("CREATE TYPE email (value text) WHERE value LIKE '%@%'");
        let row = Row::new(vec![Value::Text("a@b.com".to_string())]);
        assert_eq!(check_membership(&email_schema(), &pred, &row), Ok(()));
    }

    #[test]
    fn a_violating_row_returns_the_structured_error_naming_the_predicate() {
        let pred = refinement("CREATE TYPE email (value text) WHERE value LIKE '%@%'");
        let row = Row::new(vec![Value::Text("nope".to_string())]);
        let err = check_membership(&email_schema(), &pred, &row).unwrap_err();
        assert_eq!(err.columns, vec!["value".to_string()]);
        assert!(
            err.predicate.contains("Like"),
            "predicate: {}",
            err.predicate
        );
        assert_eq!(
            err.eval_code, None,
            "a plain contract violation, not an eval error"
        );
    }

    #[test]
    fn a_null_result_is_a_violation() {
        // `value < 'm'` over a NULL cell is not `Bool(true)` — a contract violation.
        let pred = refinement("CREATE TYPE email (value text) WHERE value < 'm'");
        let row = Row::new(vec![Value::Null]);
        assert!(check_membership(&email_schema(), &pred, &row).is_err());
    }
}
