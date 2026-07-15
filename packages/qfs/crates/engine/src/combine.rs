//! The [`CombineEngine`] seam and the in-house [`MiniEvaluator`] (ADR-0002).
//!
//! [`CombineEngine`] is the trait that keeps the DuckDB-vs-own decision reversible: a
//! `DuckDbEngine` could implement it behind a non-default feature without touching
//! callers. The shipped impl is [`MiniEvaluator`], which walks a
//! [`PhysicalPlan`](qfs_pushdown::PhysicalPlan), pulling one [`RowBatch`] per native
//! [`Scan`](qfs_pushdown::PhysicalPlan::Scan) from the supplied [`ScanResults`] and
//! folding each local [`Combine`](qfs_pushdown::PhysicalPlan::Combine) op over its inputs.

use std::sync::Arc;

use qfs_pushdown::{CombineOp, PhysicalPlan};
use qfs_types::{RowBatch, Schema, TransformMode};

use crate::eval;
use crate::scan::{Cursor, ScanResults};

/// One `|> transform <name>` stage the engine asks the injected executor to run: the definition
/// name, the derived cardinality mode, and the declared OUTPUT schema the returned rows must
/// satisfy. The executor re-resolves the FULL definition (provider/model/effort/secret reference)
/// itself — the plan carries only what shapes the relation, never a credential.
#[derive(Debug)]
pub struct TransformCall<'a> {
    /// The declared transform definition name.
    pub name: &'a str,
    /// The derived cardinality mode (row-wise / relation-wise / extraction).
    pub mode: TransformMode,
    /// The declared OUTPUT schema the returned rows are checked against.
    pub output: &'a Schema,
}

/// The injected transform-execution seam (blueprint §15, decision W): the engine stays pure — the
/// model call is performed by an executor the COMMIT boundary injects (binary-side, holding the
/// `ModelProvider`; a deterministic mock in tests). PREVIEW never constructs a `MiniEvaluator`
/// with an executor, so a preview structurally cannot call a model.
pub trait TransformExecutor: Send + Sync {
    /// Run one transform stage over the upstream rows, returning the OUTPUT rows.
    ///
    /// # Errors
    /// A structured, secret-free reason string (provider failure, credential resolution failure,
    /// unknown definition). The engine wraps it as [`EngineError::TransformFailed`].
    fn execute(&self, call: &TransformCall<'_>, input: RowBatch) -> Result<RowBatch, String>;
}

/// A structured engine error. `#[non_exhaustive]` for forward compatibility.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum EngineError {
    /// The plan referenced more native scans than [`ScanResults`] supplied (a
    /// caller/runtime mismatch — the batcher must produce one batch per scan leaf).
    MissingScanResult {
        /// How many scan results were available.
        available: usize,
    },
    /// A binary combine op (`HashJoin`/set op) received other than two inputs.
    Arity {
        /// The op label that had the wrong input count.
        op: &'static str,
        /// The number of inputs actually supplied.
        inputs: usize,
    },
    /// A `|> TRANSFORM <name>` stage reached an evaluator with **no injected executor**
    /// (blueprint §15). The classifier routes every transform-bearing statement through
    /// PREVIEW/COMMIT, and only the commit boundary injects the executor — so this fires only on
    /// a path that must not execute a model (e.g. a direct read of a transform pipeline).
    /// Fail-closed, truthful, never silent no-op rows.
    TransformNoExecutor {
        /// The referenced transform definition name.
        name: String,
    },
    /// The injected transform executor failed (provider error, credential resolution failure,
    /// unknown definition). The reason is structured and secret-free by the executor contract.
    TransformFailed {
        /// The transform definition name.
        name: String,
        /// The executor's secret-free failure reason.
        reason: String,
    },
    /// The rows the executor returned violate the definition's declared OUTPUT schema — a
    /// declared column is missing or an undeclared column was produced. The model's output is
    /// untrusted; the declared OUTPUT is the contract downstream stages type-checked against.
    TransformOutputMismatch {
        /// The transform definition name.
        name: String,
        /// Which column violated the membership (missing or undeclared).
        column: String,
    },
}

impl EngineError {
    /// A stable, machine-readable code (blueprint §6).
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            EngineError::MissingScanResult { .. } => "missing_scan_result",
            EngineError::Arity { .. } => "engine_arity",
            EngineError::TransformNoExecutor { .. } => "transform_no_executor",
            EngineError::TransformFailed { .. } => "transform_failed",
            EngineError::TransformOutputMismatch { .. } => "transform_output_mismatch",
        }
    }
}

impl std::fmt::Display for EngineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EngineError::MissingScanResult { available } => {
                write!(f, "missing scan result (only {available} available)")
            }
            EngineError::Arity { op, inputs } => {
                write!(f, "combine op `{op}` expected 2 inputs, got {inputs}")
            }
            EngineError::TransformNoExecutor { name } => write!(
                f,
                "transform '{name}' cannot execute here: no transform executor is injected \
                 (a model call runs only at the COMMIT boundary, blueprint §15)"
            ),
            EngineError::TransformFailed { name, reason } => {
                write!(f, "transform '{name}' failed: {reason}")
            }
            EngineError::TransformOutputMismatch { name, column } => write!(
                f,
                "transform '{name}' returned rows violating its declared OUTPUT schema \
                 at column '{column}'"
            ),
        }
    }
}

impl std::error::Error for EngineError {}

/// The local combine engine seam (ticket t14): execute a residual physical plan over the
/// native scan results, returning the final rows. Keeping this a trait is what makes the
/// engine choice (ADR-0002: own [`MiniEvaluator`]) reversible.
pub trait CombineEngine {
    /// Execute `plan` over `scans`, returning the combined [`RowBatch`].
    ///
    /// # Errors
    /// [`EngineError`] if the plan and scan results disagree on shape.
    fn execute(&self, plan: &PhysicalPlan, scans: ScanResults) -> Result<RowBatch, EngineError>;
}

/// The in-house relational evaluator (ADR-0002): a small, dependency-light, wasm-friendly
/// implementation of the residual operator set. The heavy lifting is pushed down, so this
/// only ever runs the cross-source remainder. Optionally carries the injected
/// [`TransformExecutor`] (COMMIT boundary only); with none, a `Transform` op fails closed.
#[derive(Default, Clone)]
pub struct MiniEvaluator {
    transform: Option<Arc<dyn TransformExecutor>>,
}

impl std::fmt::Debug for MiniEvaluator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MiniEvaluator")
            .field("transform", &self.transform.is_some())
            .finish()
    }
}

impl MiniEvaluator {
    /// Construct the evaluator with NO transform executor (the read/preview shape): a
    /// `Transform` op fails closed with [`EngineError::TransformNoExecutor`].
    #[must_use]
    pub fn new() -> Self {
        Self { transform: None }
    }

    /// Construct the evaluator with the injected transform executor — the COMMIT-boundary shape
    /// (blueprint §15): a `Transform` op runs the model stage through it.
    #[must_use]
    pub fn with_transform(transform: Arc<dyn TransformExecutor>) -> Self {
        Self {
            transform: Some(transform),
        }
    }
}

impl CombineEngine for MiniEvaluator {
    fn execute(&self, plan: &PhysicalPlan, scans: ScanResults) -> Result<RowBatch, EngineError> {
        let mut cursor = scans.into_cursor();
        eval_node(plan, &mut cursor, self.transform.as_deref())
    }
}

/// Recursively evaluate a physical node, pulling the next scan batch at each leaf.
fn eval_node(
    plan: &PhysicalPlan,
    cursor: &mut Cursor,
    transform: Option<&dyn TransformExecutor>,
) -> Result<RowBatch, EngineError> {
    match plan {
        // A native scan: pull the next driver-produced batch. The batch is authoritative
        // for the rows; the `ScanNode.schema` is the planner's resolved schema, consumed
        // by the residual above, not re-applied here.
        PhysicalPlan::Scan(_scan) => cursor
            .next_batch()
            .ok_or(EngineError::MissingScanResult { available: 0 }),
        PhysicalPlan::Combine { op, inputs } => eval_combine(op, inputs, cursor, transform),
    }
}

fn eval_combine(
    op: &CombineOp,
    inputs: &[PhysicalPlan],
    cursor: &mut Cursor,
    transform: Option<&dyn TransformExecutor>,
) -> Result<RowBatch, EngineError> {
    match op {
        // Unary ops: one input.
        CombineOp::Filter(p) => Ok(eval::filter(unary(inputs, cursor, transform)?, p)),
        CombineOp::Project(cols) => Ok(eval::project(unary(inputs, cursor, transform)?, cols)),
        CombineOp::ProjectExpr(terms) => {
            Ok(eval::project_expr(unary(inputs, cursor, transform)?, terms))
        }
        CombineOp::Extend(asgns) => Ok(eval::extend(unary(inputs, cursor, transform)?, asgns)),
        CombineOp::Limit(n) => Ok(eval::limit(unary(inputs, cursor, transform)?, *n)),
        CombineOp::Sort(keys) => Ok(eval::sort(unary(inputs, cursor, transform)?, keys)),
        CombineOp::Distinct => Ok(eval::distinct(unary(inputs, cursor, transform)?)),
        CombineOp::Aggregate {
            group_by,
            aggregates,
        } => Ok(eval::aggregate(
            unary(inputs, cursor, transform)?,
            group_by,
            aggregates,
        )),
        CombineOp::Expand(field) => Ok(eval::expand(unary(inputs, cursor, transform)?, field)),
        // Binary ops: two inputs.
        CombineOp::HashJoin(on) => {
            let (l, r) = binary(inputs, cursor, "HashJoin", transform)?;
            Ok(eval::hash_join(l, r, on))
        }
        CombineOp::SetOp(kind) => {
            let (l, r) = binary(inputs, cursor, kind.label(), transform)?;
            Ok(eval::set_op(l, r, *kind))
        }
        // §15 (decision W): the model stage. Run the upstream, hand the rows to the INJECTED
        // executor (the engine itself never calls a model), then enforce the declared OUTPUT
        // schema membership over what came back — the model's output is untrusted, and downstream
        // stages type-checked against the declared OUTPUT. No executor = fail closed.
        CombineOp::Transform {
            name,
            output_schema,
            mode,
        } => {
            let input = unary(inputs, cursor, transform)?;
            let Some(exec) = transform else {
                return Err(EngineError::TransformNoExecutor { name: name.clone() });
            };
            let call = TransformCall {
                name,
                mode: *mode,
                output: output_schema,
            };
            let out =
                exec.execute(&call, input)
                    .map_err(|reason| EngineError::TransformFailed {
                        name: name.clone(),
                        reason,
                    })?;
            check_output_membership(name, output_schema, &out)?;
            // The relation downstream stages see carries the DECLARED OUTPUT schema (with its
            // provenance tagging from the plan), values reordered to the declared column order —
            // rows are positional, so re-schemaing without reordering would mislabel values.
            Ok(reorder_to_declared(output_schema, out))
        }
    }
}

/// Enforce the declared-OUTPUT membership over the executor's returned batch: every declared
/// OUTPUT column must be present, and no undeclared column may be produced. Order-insensitive
/// (the batch is re-schemaed to the declared OUTPUT by the caller after this check).
fn check_output_membership(
    name: &str,
    declared: &Schema,
    returned: &RowBatch,
) -> Result<(), EngineError> {
    for col in &declared.columns {
        if returned.schema.column(&col.name).is_none() {
            return Err(EngineError::TransformOutputMismatch {
                name: name.to_string(),
                column: col.name.clone(),
            });
        }
    }
    if let Some(extra) = returned
        .schema
        .columns
        .iter()
        .find(|c| declared.column(&c.name).is_none())
    {
        return Err(EngineError::TransformOutputMismatch {
            name: name.to_string(),
            column: extra.name.clone(),
        });
    }
    Ok(())
}

/// Re-shape the executor's batch onto the DECLARED OUTPUT schema: values are reordered to the
/// declared column order (membership was already checked, so every declared column resolves).
/// A row missing a trailing value degrades to `Null` rather than panicking.
fn reorder_to_declared(declared: &Schema, out: RowBatch) -> RowBatch {
    let indices: Vec<Option<usize>> = declared
        .columns
        .iter()
        .map(|c| out.schema.columns.iter().position(|rc| rc.name == c.name))
        .collect();
    let rows = out
        .rows
        .into_iter()
        .map(|row| {
            let values = indices
                .iter()
                .map(|idx| {
                    idx.and_then(|i| row.values.get(i).cloned())
                        .unwrap_or(qfs_types::Value::Null)
                })
                .collect();
            qfs_types::Row::new(values)
        })
        .collect();
    RowBatch::new(declared.clone(), rows)
}

/// Evaluate the single child of a unary combine op.
fn unary(
    inputs: &[PhysicalPlan],
    cursor: &mut Cursor,
    transform: Option<&dyn TransformExecutor>,
) -> Result<RowBatch, EngineError> {
    match inputs {
        [child] => eval_node(child, cursor, transform),
        other => Err(EngineError::Arity {
            op: "unary",
            inputs: other.len(),
        }),
    }
}

/// Evaluate the two children of a binary combine op, **left-to-right** so the scan cursor
/// stays aligned with [`PhysicalPlan::scans`](qfs_pushdown::PhysicalPlan::scans).
fn binary(
    inputs: &[PhysicalPlan],
    cursor: &mut Cursor,
    op: &'static str,
    transform: Option<&dyn TransformExecutor>,
) -> Result<(RowBatch, RowBatch), EngineError> {
    match inputs {
        [l, r] => {
            let left = eval_node(l, cursor, transform)?;
            let right = eval_node(r, cursor, transform)?;
            Ok((left, right))
        }
        other => Err(EngineError::Arity {
            op,
            inputs: other.len(),
        }),
    }
}
