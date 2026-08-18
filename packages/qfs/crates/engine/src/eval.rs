//! The relational operator kernels the [`MiniEvaluator`](crate::MiniEvaluator) runs over
//! [`RowBatch`]es: predicate evaluation, projection, sort, distinct, group/aggregate,
//! expand, hash-join, and set ops. Pure functions over owned values (no I/O).
//!
//! Each kernel returns a [`RowBatch`] (schema + rows). Predicate evaluation is total: an
//! incomparable / late-bound comparison evaluates to `false` (the row is filtered out)
//! rather than panicking — the planner already type-checked pushable predicates, and a
//! residual predicate over heterogeneous data degrades safely.

use std::cmp::Ordering;
use std::collections::BTreeSet;

use qfs_pushdown::{Aggregate, Aggregator, JoinOn, OrderKey, ScalarExpr, SetKind};
use qfs_types::{
    CmpOp, ColRef, Column, ColumnType, Fields, Literal, Name, Pattern, Predicate, Row, RowBatch,
    Schema, Value,
};

/// Evaluate a [`Predicate`] against a row under its schema. Total: a comparison whose
/// operands are not comparable evaluates to `false` (the row does not match).
#[must_use]
pub(crate) fn eval_predicate(p: &Predicate, schema: &Schema, row: &Row) -> bool {
    match p {
        Predicate::And(a, b) => eval_predicate(a, schema, row) && eval_predicate(b, schema, row),
        Predicate::Or(a, b) => eval_predicate(a, schema, row) || eval_predicate(b, schema, row),
        Predicate::Not(inner) => !eval_predicate(inner, schema, row),
        Predicate::Cmp(col, op, lit) => match resolve(col, schema, row) {
            Some(v) => cmp(&v, *op, lit),
            None => false,
        },
        Predicate::In(col, set) => match resolve(col, schema, row) {
            Some(v) => set.iter().any(|lit| cmp(&v, CmpOp::Eq, lit)),
            None => false,
        },
        Predicate::Between(col, low, high) => match resolve(col, schema, row) {
            Some(v) => cmp(&v, CmpOp::Ge, low) && cmp(&v, CmpOp::Le, high),
            None => false,
        },
        Predicate::Like(col, pattern) => match resolve(col, schema, row) {
            Some(Value::Text(s)) => like_match(&s, pattern),
            _ => false,
        },
    }
}

/// Resolve a [`ColRef`] to the row's value. A bare column is a positional lookup; a
/// dotted path navigates `Struct` fields. Missing/unnavigable ⇒ `None`.
fn resolve(col: &ColRef, schema: &Schema, row: &Row) -> Option<Value> {
    let (head, rest) = col.path.split_first()?;
    let idx = schema.columns.iter().position(|c| &c.name == head)?;
    let mut cur = row.values.get(idx)?.clone();
    for seg in rest {
        match cur {
            Value::Struct(fields) => cur = fields.get(seg)?.clone(),
            _ => return None,
        }
    }
    Some(cur)
}

/// Compare a runtime value to a literal under an operator. Numeric values widen
/// (`Int`/`Float`); text compares lexically; `Null` never matches a comparison.
fn cmp(v: &Value, op: CmpOp, lit: &Literal) -> bool {
    let ord = value_cmp(v, lit);
    match (op, ord) {
        (CmpOp::Eq, Some(Ordering::Equal)) => true,
        (CmpOp::Ne, Some(o)) => o != Ordering::Equal,
        (CmpOp::Lt, Some(Ordering::Less)) => true,
        (CmpOp::Le, Some(Ordering::Less | Ordering::Equal)) => true,
        (CmpOp::Gt, Some(Ordering::Greater)) => true,
        (CmpOp::Ge, Some(Ordering::Greater | Ordering::Equal)) => true,
        (CmpOp::Match, _) => match (v, lit) {
            (Value::Text(s), Literal::Text(p)) => regex_lite(s, p),
            _ => false,
        },
        _ => false,
    }
}

/// A partial ordering between a runtime value and a literal (numeric widening; text
/// lexical; bool false<true). Incomparable / null ⇒ `None`.
fn value_cmp(v: &Value, lit: &Literal) -> Option<Ordering> {
    match (v, lit) {
        (Value::Null, _) | (_, Literal::Null) => None,
        (Value::Int(a), Literal::Int(b)) => Some(a.cmp(b)),
        (Value::Int(a), Literal::Float(b)) => (*a as f64).partial_cmp(b),
        (Value::Float(a), Literal::Float(b)) => a.partial_cmp(b),
        (Value::Float(a), Literal::Int(b)) => a.partial_cmp(&(*b as f64)),
        (Value::Timestamp(a), Literal::Int(b)) => Some(a.cmp(b)),
        (Value::Text(a), Literal::Text(b)) => Some(a.as_str().cmp(b.as_str())),
        (Value::Bool(a), Literal::Bool(b)) => Some(a.cmp(b)),
        _ => None,
    }
}

/// A minimal `LIKE` matcher: `%` = any run, `_` = any single char. Anchored.
fn like_match(s: &str, pattern: &Pattern) -> bool {
    like_inner(s.as_bytes(), pattern.0.as_bytes())
}

fn like_inner(s: &[u8], p: &[u8]) -> bool {
    match p.split_first() {
        None => s.is_empty(),
        Some((b'%', rest)) => {
            // `%` matches zero or more chars: try every suffix of `s`.
            (0..=s.len()).any(|i| like_inner(&s[i..], rest))
        }
        Some((b'_', rest)) => !s.is_empty() && like_inner(&s[1..], rest),
        Some((c, rest)) => s.first() == Some(c) && like_inner(&s[1..], rest),
    }
}

/// A tiny anchored regex subset for `~`: only treats `.*`/`.` specially, otherwise a
/// substring test. Kept minimal (the residual rarely needs `~`); a full engine is E4.
fn regex_lite(s: &str, p: &str) -> bool {
    if let Some(inner) = p.strip_prefix(".*").and_then(|x| x.strip_suffix(".*")) {
        s.contains(inner)
    } else {
        s.contains(p)
    }
}

/// A column a stage named that the relation does not carry — the structured refusal that replaces
/// "resolve to `None`, so the row does not match" for a **described** schema.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct MissingColumn {
    /// The column the query named.
    pub name: Name,
    /// The columns the relation actually carries, in order.
    pub available: Vec<Name>,
}

/// Refuse a predicate that names a column absent from a **described** schema.
///
/// `resolve` maps two different situations onto the same `None` — *this column is not in the
/// schema* and *this value is null / this dotted path is unnavigable* — and `eval_predicate` turns
/// both into "the row does not match". For the second that is right (a total predicate); for the
/// first it made a typo indistinguishable from an honest empty result, at exit 0.
///
/// Only the **head** of each column reference is checked, and only against a NON-EMPTY schema:
///
/// - a dotted path (`meta.title`) navigates a `Json`/`Struct` value whose inner fields are
///   late-bound by design, so only `meta` has to exist;
/// - an EMPTY schema means the relation is undescribable (a driver that does not describe its
///   columns, or a relation whose shape is only known after a codec), and late-binding stays —
///   refusing there would false-reject a column that really is present at runtime.
///
/// `also_accepted` widens the described schema with names the SOURCE understands even though no
/// row carries them: a backend **search pseudo-column** (Drive's `fullText`, Gmail's `label`) is a
/// legitimate thing to filter on, so a facet passes its own list and the rest still refuses.
pub(crate) fn check_predicate_columns(
    schema: &Schema,
    p: &Predicate,
    also_accepted: &[&str],
) -> Result<(), MissingColumn> {
    if schema.columns.is_empty() {
        return Ok(());
    }
    let mut refs: Vec<&ColRef> = Vec::new();
    collect_col_refs(p, &mut refs);
    for col in refs {
        let Some(head) = col.path.first() else {
            continue;
        };
        if schema.column(head).is_none() && !also_accepted.contains(&head.as_str()) {
            let mut available = schema.column_names();
            available.extend(also_accepted.iter().map(|n| (*n).to_string()));
            return Err(MissingColumn {
                name: head.clone(),
                available,
            });
        }
    }
    Ok(())
}

/// Every column reference a predicate resolves, in evaluation order — `Cmp`, `In`, `Between` and
/// `Like` alike, through the whole boolean structure. All four take the same `resolve` path, so all
/// four are checked (a `NOT`/`OR` arm hides a typo just as well as a bare comparison).
fn collect_col_refs<'p>(p: &'p Predicate, out: &mut Vec<&'p ColRef>) {
    match p {
        Predicate::And(a, b) | Predicate::Or(a, b) => {
            collect_col_refs(a, out);
            collect_col_refs(b, out);
        }
        Predicate::Not(inner) => collect_col_refs(inner, out),
        Predicate::Cmp(col, _, _)
        | Predicate::In(col, _)
        | Predicate::Between(col, _, _)
        | Predicate::Like(col, _) => out.push(col),
    }
}

/// Filter a batch by a predicate. Total: an unresolvable comparison drops the row.
///
/// This is the **driver-residual** form — the predicate a driver reports it could not express
/// exactly. Such a residual may legitimately name a backend **search pseudo-column** the described
/// schema does not carry (Drive's `fullText`, Gmail's `to`), so it is evaluated, never refused.
/// The caller-written `WHERE` goes through [`filter_checked`].
#[must_use]
pub(crate) fn filter(batch: RowBatch, p: &Predicate) -> RowBatch {
    let schema = batch.schema.clone();
    let rows = batch
        .rows
        .into_iter()
        .filter(|r| eval_predicate(p, &schema, r))
        .collect();
    RowBatch::new(schema, rows)
}

/// [`filter`] for a **caller-written `WHERE`**: refuse an unknown column against a described
/// schema instead of answering the empty relation a typo would otherwise produce.
///
/// # Errors
/// [`MissingColumn`] when the predicate names a column the (non-empty) schema does not carry.
pub(crate) fn filter_checked(batch: RowBatch, p: &Predicate) -> Result<RowBatch, MissingColumn> {
    check_predicate_columns(&batch.schema, p, &[])?;
    Ok(filter(batch, p))
}

/// [`project`] for a **caller-written `SELECT`**: refuse an unknown column against a described
/// schema instead of answering the rows-of-nothing a typo would otherwise produce.
///
/// The runtime twin of the planner's plan-time refusal (ticket 20260725113000). Both exist because
/// a projection reaches the rows by two roads: normally it is *pushed* and the planner is the last
/// place that still sees the described schema, but a post-decode or post-`expand` projection is a
/// local residual over a batch whose schema only exists here. An empty schema stays lenient in both
/// — late-bound is not wrong.
///
/// # Errors
/// [`MissingColumn`] when the projection names a column the (non-empty) batch schema does not carry.
pub(crate) fn project_checked(
    batch: RowBatch,
    columns: &[Name],
) -> Result<RowBatch, MissingColumn> {
    if !batch.schema.columns.is_empty() {
        for name in columns {
            if name != "*" && batch.schema.column(name).is_none() {
                return Err(MissingColumn {
                    name: name.clone(),
                    available: batch.schema.column_names(),
                });
            }
        }
    }
    Ok(project(batch, columns))
}

/// Project a batch to a column list (`*`/empty is identity). Unknown columns are dropped — the
/// total form. The caller-written `SELECT` goes through [`project_checked`].
#[must_use]
pub(crate) fn project(batch: RowBatch, columns: &[Name]) -> RowBatch {
    if columns.is_empty() || columns == ["*".to_string()] {
        return batch;
    }
    let indices: Vec<(usize, Column)> = columns
        .iter()
        .filter_map(|name| {
            batch
                .schema
                .columns
                .iter()
                .position(|c| &c.name == name)
                .map(|i| (i, batch.schema.columns[i].clone()))
        })
        .collect();
    let schema = Schema::new(indices.iter().map(|(_, c)| c.clone()).collect());
    let rows = batch
        .rows
        .into_iter()
        .map(|r| {
            Row::new(
                indices
                    .iter()
                    .map(|(i, _)| r.values.get(*i).cloned().unwrap_or(Value::Null))
                    .collect(),
            )
        })
        .collect();
    RowBatch::new(schema, rows)
}

/// Evaluate a per-row [`ScalarExpr`] against a row under its schema (t92). Total: a column
/// that does not resolve is `Null` (mirroring the projection/predicate late-binding), never a
/// panic. A `Struct`/`Array` constructor builds the mirrored [`Value`] from its evaluated
/// field/element expressions (field order preserved).
///
/// Public because the §13 declared-map write path evaluates a stored `VALUES (<expr>)` wire-body
/// expression against each incoming row here (the row bound as a single `row` struct column), the
/// write-side twin of the read facet reaching into [`MiniEvaluator`] — one evaluator, both paths.
#[must_use]
pub fn eval_value(expr: &ScalarExpr, schema: &Schema, row: &Row) -> Value {
    match expr {
        ScalarExpr::Col(col) => resolve(col, schema, row).unwrap_or(Value::Null),
        ScalarExpr::Lit(v) => v.clone(),
        ScalarExpr::Array(elems) => {
            Value::Array(elems.iter().map(|e| eval_value(e, schema, row)).collect())
        }
        ScalarExpr::Struct(fields) => Value::Struct(Fields::new(
            fields
                .iter()
                .map(|(name, e)| (name.clone(), eval_value(e, schema, row)))
                .collect(),
        )),
    }
}

/// [`project_expr`] for a **caller-written renaming or computed `SELECT`**: refuse a column the
/// (non-empty) batch schema does not carry, instead of the one null per row `eval_value`'s total
/// resolution would otherwise produce (ticket 20260816191500).
///
/// The runtime twin of the planner's `check_project_expr_columns`, and the exact counterpart of
/// [`project_checked`] on the other road. The check lives HERE rather than inside `eval_value`
/// because that resolver is shared with `EXTEND`/`SET`, whose total form over a late-bound row is
/// deliberate and unchanged.
///
/// # Errors
/// [`MissingColumn`] when a projection term names a column the (non-empty) batch schema lacks.
pub(crate) fn project_expr_checked(
    batch: RowBatch,
    projections: &[(Name, ScalarExpr)],
) -> Result<RowBatch, MissingColumn> {
    if !batch.schema.columns.is_empty() {
        let mut refs: Vec<&qfs_types::ColRef> = Vec::new();
        for (_, expr) in projections {
            expr.col_refs(&mut refs);
        }
        for col in refs {
            let Some(head) = col.path.first() else {
                continue;
            };
            if batch.schema.column(head).is_none() {
                return Err(MissingColumn {
                    name: head.clone(),
                    available: batch.schema.column_names(),
                });
            }
        }
    }
    Ok(project_expr(batch, projections))
}

/// A **computed** projection (t92): each output column is a per-row [`ScalarExpr`] (a struct/
/// array constructor over the input columns). Unlike name-only [`project`], this evaluates an
/// expression per row, so `SELECT {filename: name, bytes: content} AS att` produces a real
/// `Struct` value. Output column types are late-bound (`Unknown`) — the value carries its shape.
/// The caller-written `SELECT` goes through [`project_expr_checked`].
#[must_use]
pub(crate) fn project_expr(batch: RowBatch, projections: &[(Name, ScalarExpr)]) -> RowBatch {
    let schema = Schema::new(
        projections
            .iter()
            .map(|(name, _)| Column::new(name.clone(), ColumnType::Unknown, true))
            .collect(),
    );
    let src = batch.schema;
    let rows = batch
        .rows
        .into_iter()
        .map(|r| {
            Row::new(
                projections
                    .iter()
                    .map(|(_, e)| eval_value(e, &src, &r))
                    .collect(),
            )
        })
        .collect();
    RowBatch::new(schema, rows)
}

/// `EXTEND`/`SET` (t92): add or overwrite columns with per-row computed values. An assignment
/// naming an existing column overwrites it in place; a new name is appended. Assignments apply
/// left-to-right over a progressively-updated row, so a later assignment can read an earlier
/// one. Output column types are late-bound (`Unknown`).
#[must_use]
pub(crate) fn extend(batch: RowBatch, assignments: &[(Name, ScalarExpr)]) -> RowBatch {
    // Resolve the output column layout once, recording each assignment's target index.
    let mut out_cols: Vec<Column> = batch.schema.columns.clone();
    let mut targets: Vec<usize> = Vec::with_capacity(assignments.len());
    for (name, _) in assignments {
        if let Some(i) = out_cols.iter().position(|c| &c.name == name) {
            out_cols[i].ty = ColumnType::Unknown;
            targets.push(i);
        } else {
            out_cols.push(Column::new(name.clone(), ColumnType::Unknown, true));
            targets.push(out_cols.len() - 1);
        }
    }
    let schema = Schema::new(out_cols);
    let width = schema.columns.len();
    let rows = batch
        .rows
        .into_iter()
        .map(|r| {
            // Pad the row to the full output width; evaluate each assignment against the
            // progressively-updated row (so a later assignment sees an earlier one).
            let mut values = r.values;
            values.resize(width, Value::Null);
            for ((_, expr), &idx) in assignments.iter().zip(&targets) {
                let cur = Row::new(values.clone());
                values[idx] = eval_value(expr, &schema, &cur);
            }
            Row::new(values)
        })
        .collect();
    RowBatch::new(schema, rows)
}

/// Cap a batch to at most `n` rows.
#[must_use]
pub(crate) fn limit(mut batch: RowBatch, n: u64) -> RowBatch {
    batch.rows.truncate(n as usize);
    batch
}

/// Stable-sort a batch by the order keys (deterministic; ties keep input order).
#[must_use]
pub(crate) fn sort(mut batch: RowBatch, keys: &[OrderKey]) -> RowBatch {
    let positions: Vec<(usize, bool)> = keys
        .iter()
        .filter_map(|k| {
            batch
                .schema
                .columns
                .iter()
                .position(|c| c.name == k.column)
                .map(|i| (i, k.descending))
        })
        .collect();
    batch.rows.sort_by(|a, b| {
        for (idx, desc) in &positions {
            let ord = order_values(a.values.get(*idx), b.values.get(*idx));
            let ord = if *desc { ord.reverse() } else { ord };
            if ord != Ordering::Equal {
                return ord;
            }
        }
        Ordering::Equal
    });
    batch
}

/// A total ordering between two runtime values for sorting (Null sorts first).
fn order_values(a: Option<&Value>, b: Option<&Value>) -> Ordering {
    match (a, b) {
        (None, None) => Ordering::Equal,
        (None, _) | (Some(Value::Null), _) => Ordering::Less,
        (_, None) | (_, Some(Value::Null)) => Ordering::Greater,
        (Some(x), Some(y)) => match (x, y) {
            (Value::Int(p), Value::Int(q)) => p.cmp(q),
            (Value::Float(p), Value::Float(q)) => p.partial_cmp(q).unwrap_or(Ordering::Equal),
            (Value::Int(p), Value::Float(q)) => {
                (*p as f64).partial_cmp(q).unwrap_or(Ordering::Equal)
            }
            (Value::Float(p), Value::Int(q)) => {
                p.partial_cmp(&(*q as f64)).unwrap_or(Ordering::Equal)
            }
            (Value::Timestamp(p), Value::Timestamp(q)) => p.cmp(q),
            (Value::Text(p), Value::Text(q)) => p.cmp(q),
            (Value::Bool(p), Value::Bool(q)) => p.cmp(q),
            // Mixed/other kinds: compare by a stable debug rendering (deterministic).
            _ => format!("{x:?}").cmp(&format!("{y:?}")),
        },
    }
}

/// Deduplicate rows (preserving first-seen order), keyed by a stable rendering.
#[must_use]
pub(crate) fn distinct(batch: RowBatch) -> RowBatch {
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let schema = batch.schema.clone();
    let rows = batch
        .rows
        .into_iter()
        .filter(|r| seen.insert(row_key(r)))
        .collect();
    RowBatch::new(schema, rows)
}

/// A stable string key for a row (used by distinct / set ops / hash-join probe).
fn row_key(r: &Row) -> String {
    format!("{:?}", r.values)
}

/// A stable string key for a single value (hash-join key).
fn value_key(v: &Value) -> String {
    format!("{v:?}")
}

/// Group + aggregate a batch (blueprint §4). Empty `group_by` ⇒ a single output row over the
/// whole batch. Output schema is the group columns followed by one column per aggregate.
#[must_use]
pub(crate) fn aggregate(batch: RowBatch, group_by: &[Name], aggs: &[Aggregate]) -> RowBatch {
    let group_idx: Vec<usize> = group_by
        .iter()
        .filter_map(|g| batch.schema.columns.iter().position(|c| &c.name == g))
        .collect();
    let agg_idx: Vec<Option<usize>> = aggs
        .iter()
        .map(|a| batch.schema.columns.iter().position(|c| c.name == a.column))
        .collect();

    // Group rows by their group-column key, preserving first-seen group order.
    let mut order: Vec<String> = Vec::new();
    let mut groups: std::collections::BTreeMap<String, Vec<Row>> =
        std::collections::BTreeMap::new();
    let mut first_key_row: std::collections::BTreeMap<String, Row> =
        std::collections::BTreeMap::new();
    for row in batch.rows {
        let key: Vec<String> = group_idx
            .iter()
            .map(|i| value_key(row.values.get(*i).unwrap_or(&Value::Null)))
            .collect();
        let key = key.join("\u{1}");
        if !groups.contains_key(&key) {
            order.push(key.clone());
            first_key_row.insert(key.clone(), row.clone());
        }
        groups.entry(key).or_default().push(row);
    }
    // Whole-relation aggregate with no rows still yields one row (e.g. COUNT = 0).
    if group_by.is_empty() && order.is_empty() {
        order.push(String::new());
        groups.insert(String::new(), Vec::new());
    }

    let mut out_cols: Vec<Column> = group_by
        .iter()
        .map(|g| Column::new(g.clone(), ColumnType::Unknown, true))
        .collect();
    for a in aggs {
        let ty = match a.func {
            Aggregator::Count => ColumnType::Int,
            Aggregator::ArrayAgg => ColumnType::Array(Box::new(ColumnType::Unknown)),
            _ => ColumnType::Unknown,
        };
        out_cols.push(Column::new(a.output.clone(), ty, true));
    }
    let schema = Schema::new(out_cols);

    let mut out_rows = Vec::with_capacity(order.len());
    for key in &order {
        let rows = groups.get(key).cloned().unwrap_or_default();
        let mut values: Vec<Value> = Vec::new();
        if let Some(sample) = first_key_row.get(key) {
            for i in &group_idx {
                values.push(sample.values.get(*i).cloned().unwrap_or(Value::Null));
            }
        } else {
            for _ in &group_idx {
                values.push(Value::Null);
            }
        }
        for (a, idx) in aggs.iter().zip(&agg_idx) {
            values.push(run_aggregate(a.func, *idx, &rows));
        }
        out_rows.push(Row::new(values));
    }
    RowBatch::new(schema, out_rows)
}

fn run_aggregate(func: Aggregator, col: Option<usize>, rows: &[Row]) -> Value {
    let vals: Vec<&Value> = match col {
        Some(i) => rows
            .iter()
            .filter_map(|r| r.values.get(i))
            .filter(|v| !matches!(v, Value::Null))
            .collect(),
        None => Vec::new(),
    };
    match func {
        Aggregator::Count => Value::Int(if col.is_some() {
            vals.len() as i64
        } else {
            rows.len() as i64
        }),
        Aggregator::Sum => {
            let mut acc = 0.0_f64;
            let mut any_float = false;
            for v in &vals {
                match v {
                    Value::Int(n) => acc += *n as f64,
                    Value::Float(f) => {
                        any_float = true;
                        acc += f;
                    }
                    _ => {}
                }
            }
            if any_float {
                Value::Float(acc)
            } else {
                Value::Int(acc as i64)
            }
        }
        Aggregator::Min => fold_extreme(&vals, Ordering::Less),
        Aggregator::Max => fold_extreme(&vals, Ordering::Greater),
        // `ARRAY_AGG(col)` collects the column's per-row values in row order into one `Array`.
        // Unlike the numeric aggregates it keeps every row's cell (including nulls) — it is a
        // faithful collect, not a fold — so N input rows pack into one Array of N elements.
        Aggregator::ArrayAgg => Value::Array(match col {
            Some(i) => rows
                .iter()
                .filter_map(|r| r.values.get(i).cloned())
                .collect(),
            None => Vec::new(),
        }),
    }
}

fn fold_extreme(vals: &[&Value], want: Ordering) -> Value {
    let mut best: Option<&Value> = None;
    for v in vals {
        match best {
            None => best = Some(v),
            Some(b) => {
                if order_values(Some(v), Some(b)) == want {
                    best = Some(v);
                }
            }
        }
    }
    best.cloned().unwrap_or(Value::Null)
}

/// What `EXPAND` refuses (ticket 20260717180200): the two errors [`Schema::expand`] has always
/// documented and the executed path used to swallow — one discarded by `unwrap_or`, the other
/// short-circuited before the check ever ran.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ExpandError {
    /// The named column is not in the (described) schema.
    Unknown(MissingColumn),
    /// The column is present but is not a collection — a scalar or a `Json` blob.
    NotExpandable {
        /// The column `EXPAND` named.
        field: Name,
        /// Its declared type, rendered.
        ty: String,
    },
}

/// `EXPAND <field>` — explode a nested collection column into rows (blueprint §4). An `Array`
/// of structs flattens each element's fields; an `Array` of scalars yields one row per
/// element; a `Struct` flattens one level.
///
/// **A column it cannot explode is refused, not passed through.** Before, an absent column returned
/// the batch unchanged, and a `Json`/scalar column had [`Schema::expand`]'s `NotExpandable`
/// discarded by an `unwrap_or` — so the stage produced output byte-identical to its input at exit 0
/// and the caller could not tell it had done nothing.
///
/// The one late-bound case that still passes through is a column typed `Unknown`: that means "not
/// known yet" (an `EXTEND`-computed column, an aggregate output, a relation whose shape only
/// resolves at runtime), not "known to be a scalar", so refusing it would reject a value that
/// really is an array in the rows. An EMPTY schema — an undescribable relation — is the same case
/// and is likewise never refused.
///
/// **A struct element splices BY FIELD NAME, never positionally** (ticket 20260725103000). Before,
/// each element contributed `fields.into_values()` — an ordered value list with no reference to the
/// names — so an array whose elements carry DIFFERENT key sets (exactly what a real API returns:
/// Slack omits `thread_ts` and `subtype` on messages that have none) shifted every later column,
/// silently, with no diagnostic. A field a given element omits is now `Null` in that element's row.
///
/// # Errors
/// [`ExpandError`] for an absent column or a non-collection one.
pub(crate) fn expand(batch: RowBatch, field: &Name) -> Result<RowBatch, ExpandError> {
    // An undescribable relation stays late-bound: nothing here is known to be wrong.
    if batch.schema.columns.is_empty() {
        return Ok(batch);
    }
    let Some(idx) = batch.schema.columns.iter().position(|c| &c.name == field) else {
        return Err(ExpandError::Unknown(MissingColumn {
            name: field.clone(),
            available: batch.schema.column_names(),
        }));
    };
    // Output schema: replace the field column per the type model's `expand`. A late-bound
    // (`Unknown`) column has no declared element type, so the replacement columns are the UNION of
    // the field names the rows actually carry; anything else propagates `NotExpandable`.
    let (schema, splice_names) = if matches!(batch.schema.columns[idx].ty, ColumnType::Unknown) {
        match observed_struct_fields(&batch.rows, idx) {
            Some(names) => (
                replace_column(&batch.schema, idx, &names),
                Some(names.clone()),
            ),
            None => (batch.schema.clone(), None),
        }
    } else {
        let schema = batch
            .schema
            .expand(field)
            .map_err(|_| ExpandError::NotExpandable {
                field: field.clone(),
                ty: render_type(&batch.schema.columns[idx].ty),
            })?;
        // Only a struct flattening splices by name. An array of SCALARS replaces the column with
        // one column of the same name, where a struct element's field lookup would be meaningless.
        let flattens_struct = match &batch.schema.columns[idx].ty {
            ColumnType::Struct(_) => true,
            ColumnType::Array(elem) => matches!(elem.as_ref(), ColumnType::Struct(_)),
            _ => false,
        };
        let names = flattens_struct.then(|| {
            let width = schema.columns.len() + 1 - batch.schema.columns.len();
            schema.columns[idx..idx + width]
                .iter()
                .map(|c| c.name.clone())
                .collect::<Vec<Name>>()
        });
        (schema, names)
    };
    let names = splice_names.as_deref();
    let mut out_rows = Vec::new();
    for row in batch.rows {
        let target = row.values.get(idx).cloned().unwrap_or(Value::Null);
        match target {
            Value::Array(items) => {
                for item in items {
                    out_rows.push(splice_row(&row, idx, expand_item(item, names)));
                }
            }
            item @ Value::Struct(_) => {
                out_rows.push(splice_row(&row, idx, expand_item(item, names)));
            }
            // A Null / late-bound value in an expandable column: keep the row unchanged, widened to
            // the replacement columns so every row still matches the schema. (A genuinely
            // non-collection COLUMN was refused above, against the schema.)
            other => out_rows.push(splice_row(&row, idx, expand_item(other, names))),
        }
    }
    Ok(RowBatch::new(schema, out_rows))
}

/// The union of the field names the struct values at column `idx` carry, in first-seen order — the
/// replacement columns for expanding a late-bound (`Unknown`) column, where no declared element type
/// says what the shape is. `None` when no row carries a struct there, which is the array-of-scalars
/// (or genuinely-empty) case the caller keeps late-bound.
fn observed_struct_fields(rows: &[Row], idx: usize) -> Option<Vec<Name>> {
    let mut names: Vec<Name> = Vec::new();
    let mut saw_struct = false;
    for row in rows {
        match row.values.get(idx) {
            Some(Value::Struct(fields)) => {
                saw_struct = true;
                note_field_names(fields, &mut names);
            }
            Some(Value::Array(items)) => {
                for item in items {
                    if let Value::Struct(fields) = item {
                        saw_struct = true;
                        note_field_names(fields, &mut names);
                    }
                }
            }
            _ => {}
        }
    }
    saw_struct.then_some(names)
}

/// Append `fields`' names to `names`, skipping any already seen (first-seen order is the schema's).
fn note_field_names(fields: &Fields, names: &mut Vec<Name>) {
    for (n, _) in fields.iter() {
        if !names.iter().any(|seen| seen == n) {
            names.push(n.clone());
        }
    }
}

/// Replace column `idx` of `schema` with one late-bound, nullable column per name in `names`.
fn replace_column(schema: &Schema, idx: usize, names: &[Name]) -> Schema {
    let mut out = Vec::with_capacity(schema.columns.len() + names.len());
    out.extend_from_slice(&schema.columns[..idx]);
    out.extend(
        names
            .iter()
            .map(|n| Column::new(n.clone(), ColumnType::Unknown, true)),
    );
    out.extend_from_slice(&schema.columns[idx + 1..]);
    Schema::new(out)
}

/// A short, stable rendering of a column type for a refusal message (`json`, `text`, `array`, …).
fn render_type(ty: &ColumnType) -> String {
    match ty {
        ColumnType::Array(_) => "an array".to_string(),
        ColumnType::Struct(_) => "a struct".to_string(),
        ColumnType::Json => "a json value".to_string(),
        ColumnType::Unknown => "late-bound".to_string(),
        other => format!("a scalar ({other:?})").to_lowercase(),
    }
}

/// Flatten one expanded element into the row's replacement values.
///
/// With `names` (the replacement columns' names) the element's fields are read **by name**, so an
/// element that omits an optional key contributes `Null` in that column instead of shifting every
/// later one. Without them the column expands to a single value and the element rides through whole.
fn expand_item(item: Value, names: Option<&[Name]>) -> Vec<Value> {
    match (item, names) {
        (Value::Struct(fields), Some(names)) => names
            .iter()
            .map(|n| fields.get(n).cloned().unwrap_or(Value::Null))
            .collect(),
        (Value::Struct(fields), None) => fields.into_values(),
        // A non-struct value where struct columns were expected (a `Null` element, or a ragged
        // array carrying a scalar): every replacement column is absent for this element.
        (_, Some(names)) => vec![Value::Null; names.len()],
        (other, None) => vec![other],
    }
}

/// Replace position `idx` of `row` with `replacement` values (de-nesting in place).
fn splice_row(row: &Row, idx: usize, replacement: Vec<Value>) -> Row {
    let mut values = Vec::with_capacity(row.values.len() + replacement.len());
    values.extend_from_slice(&row.values[..idx]);
    values.extend(replacement);
    values.extend_from_slice(&row.values[idx + 1..]);
    Row::new(values)
}

/// A hash join over two batches on `on.left = on.right` (blueprint §7 federation). Builds a
/// hash table on the right, probes with the left; output columns are the left schema
/// followed by the right schema with collisions disambiguated ([`Schema::join`]).
#[must_use]
pub(crate) fn hash_join(left: RowBatch, right: RowBatch, on: &JoinOn) -> RowBatch {
    let schema = left.schema.join(&right.schema);
    let Some(lk) = left.schema.columns.iter().position(|c| c.name == on.left) else {
        return RowBatch::new(schema, Vec::new());
    };
    let Some(rk) = right.schema.columns.iter().position(|c| c.name == on.right) else {
        return RowBatch::new(schema, Vec::new());
    };
    // Build side: map right join-key → rows.
    let mut table: std::collections::BTreeMap<String, Vec<Row>> = std::collections::BTreeMap::new();
    for row in &right.rows {
        let key = value_key(row.values.get(rk).unwrap_or(&Value::Null));
        table.entry(key).or_default().push(row.clone());
    }
    let mut out_rows = Vec::new();
    for lrow in &left.rows {
        let key = value_key(lrow.values.get(lk).unwrap_or(&Value::Null));
        if let Some(matches) = table.get(&key) {
            for rrow in matches {
                let mut values = lrow.values.clone();
                values.extend(rrow.values.clone());
                out_rows.push(Row::new(values));
            }
        }
    }
    RowBatch::new(schema, out_rows)
}

/// A set op over two batches (blueprint §4). `UNION` is the distinct union; `EXCEPT` is left
/// rows absent from the right; `INTERSECT` is rows present in both. Keyed by a stable row
/// rendering; the output schema is the left schema (sides are union-compatible).
#[must_use]
pub(crate) fn set_op(left: RowBatch, right: RowBatch, kind: SetKind) -> RowBatch {
    let schema = left.schema.clone();
    let right_keys: BTreeSet<String> = right.rows.iter().map(row_key).collect();
    match kind {
        SetKind::Union => {
            let mut seen: BTreeSet<String> = BTreeSet::new();
            let mut rows = Vec::new();
            for r in left.rows.into_iter().chain(right.rows) {
                if seen.insert(row_key(&r)) {
                    rows.push(r);
                }
            }
            RowBatch::new(schema, rows)
        }
        SetKind::Except => {
            let mut seen: BTreeSet<String> = BTreeSet::new();
            let rows = left
                .rows
                .into_iter()
                .filter(|r| !right_keys.contains(&row_key(r)) && seen.insert(row_key(r)))
                .collect();
            RowBatch::new(schema, rows)
        }
        SetKind::Intersect => {
            let mut seen: BTreeSet<String> = BTreeSet::new();
            let rows = left
                .rows
                .into_iter()
                .filter(|r| right_keys.contains(&row_key(r)) && seen.insert(row_key(r)))
                .collect();
            RowBatch::new(schema, rows)
        }
    }
}
