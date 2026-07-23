//! `qfs-exec` — the **execution / integration layer** (ticket t29): the end-to-end SELECT
//! read-path executor (the t20 carry-over closure) plus the one-shot CLI execution orchestration
//! (statement-source resolution, addressing validation, the PREVIEW/COMMIT safety gate, output
//! rendering, and the stable exit-code contract). The thin `qfs` bin / `qfs-cmd` dispatches into
//! [`run_oneshot`]; all the composition lives here.
//!
//! ## Crate topology (the t29 architectural decision)
//! `qfs-runtime`'s spine is deliberately `{qfs-plan, qfs-types}` and must not gain `qfs-core` /
//! `qfs-pushdown` / `qfs-engine`; `qfs-cmd` must stay logic-free (the t01 C4 guard forbids it a
//! direct `qfs-lang/plan/driver/codec/parser` dep). The read executor needs
//! `qfs-pushdown + qfs-engine + qfs-core` and async scans. So it lives **here**, in a new
//! integration crate that sits ABOVE the spine and composes those pieces. Every existing
//! confinement holds:
//!  - **Runtime minimal spine** — `qfs-exec` does **not** depend on `qfs-runtime`; it owns its
//!    own async [`ReadDriver`](read::ReadDriver) read seam (the runtime's write `ApplyDriver`
//!    only returns affected counts, never rows, so it is structurally not a read seam). The
//!    runtime confinement guard fires only on `qfs-runtime` consumers, so it is untouched.
//!  - **cmd logic-free** — `qfs-cmd → qfs-exec` is allowed (C4 forbids only
//!    `qfs-lang/plan/driver/codec/parser`; `qfs-exec` is none of them).
//!  - **No spine inversion** — nothing in the pure spine depends back onto `qfs-exec`, so tokio
//!    stays out of the spine's closure (`qfs-plan`'s purity dep-closure test is unaffected).

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

mod addressing;
mod codec;
pub mod collection;
pub mod declared;
mod dto;
mod error;
mod exec;
mod output;
mod read;
pub mod shell;

pub use codec::apply_codecs;
pub use collection::{
    collection_root, markdown_relation_describe_schema, read_registered_collection,
    to_root_relative,
};
pub use dto::{PlanPreview, ResultMeta, RowSet};
pub use error::{ErrorKind, ExecError, ExitCode};
pub use exec::{
    apply_commit, apply_via, block_on_read, block_on_read_with, build_plan, execute_read,
    execute_read_with, map_qfs_error, parse, plan_preview,
};
pub use output::{JsonRenderer, OutputFormat, Renderer, TableRenderer};
// Re-export the engine's residual predicate filter so a read facet in the binary (which the
// dep-direction guard keeps off qfs-engine directly) can apply a driver's pushed-WHERE residual —
// the rows a driver returns after pushing only the faithfully-renderable part of a predicate.
pub use qfs_engine::apply_residual;
// The refinement-predicate AST type (blueprint §5.4) rides through here so the terminal binary can
// name a declared type's `WHERE` predicate without taking a direct `qfs-parser` edge (it stays off
// the lower spine — same posture as `parse`/`ViewSpec`).
pub use qfs_parser::Expr;
// Re-export the transform-execution seam (blueprint §15) so `qfs-cmd` and the binary composition
// can supply the injected executor without a direct qfs-engine dep.
pub use qfs_engine::{TransformCall, TransformExecutor};
pub use read::{ReadDriver, ReadRegistry};
pub use shell::{Builtin, Completer, Outcome, Session, VfsPath};

// `run_describe` (ticket t39) is defined below in this module; re-export is implicit (pub fn).

use std::io::Write;

use qfs_core::{Engine, Plan};
use qfs_parser::{PlanWrap, Statement};

/// Where the one-shot statement text came from (exactly one source per invocation).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StmtSource {
    /// A positional `qfs run '<stmt>'`.
    Positional(String),
    /// A `-e <stmt>` flag.
    Expr(String),
    /// `qfs run -` (read the statement from stdin).
    Stdin(String),
}

impl StmtSource {
    /// The statement text, regardless of where it came from.
    #[must_use]
    pub fn text(&self) -> &str {
        match self {
            StmtSource::Positional(s) | StmtSource::Expr(s) | StmtSource::Stdin(s) => s,
        }
    }
}

/// Resolve exactly one statement source from the three mutually-exclusive inputs. Errors with a
/// `usage` error (exit 2) on zero or more than one source.
///
/// # Errors
/// [`ExecError`] (kind `usage`) if not exactly one of `positional` / `expr` / `stdin` is set.
pub fn resolve_source(
    positional: Option<String>,
    expr: Option<String>,
    stdin: Option<String>,
) -> Result<StmtSource, ExecError> {
    let mut sources = Vec::new();
    if let Some(s) = positional {
        sources.push(StmtSource::Positional(s));
    }
    if let Some(s) = expr {
        sources.push(StmtSource::Expr(s));
    }
    if let Some(s) = stdin {
        sources.push(StmtSource::Stdin(s));
    }
    match sources.len() {
        1 => Ok(sources.into_iter().next().unwrap_or_else(|| {
            // Unreachable: len == 1. Kept total without a panic for the lib lints.
            StmtSource::Expr(String::new())
        })),
        0 => Err(ExecError::usage(
            "no statement provided; pass a statement, `-e <stmt>`, or `-` for stdin",
        )),
        n => Err(ExecError::usage(format!(
            "{n} statement sources provided; exactly one of <stmt> / -e / - is allowed"
        ))),
    }
}

/// One render destination pair: data → stdout, errors → stderr (ticket t29).
pub struct Streams<'a> {
    /// Where rendered data goes (stdout).
    pub out: &'a mut dyn Write,
    /// Where rendered errors go (stderr).
    pub err: &'a mut dyn Write,
}

/// The read-time wiring the executor needs, beyond the engine's registries: the
/// [`ReadRegistry`] of `ReadDriver` scan implementations. Threaded separately so the `Engine`
/// (the introspective + commit registries) stays the t27 shape and a caller can register the
/// read facet independently (tests register only an in-memory fake).
pub struct ExecCtx<'a> {
    /// The shared engine (mount registry for describe/pushdown, secrets, codecs).
    pub engine: &'a Engine,
    /// The read-driver registry the read executor resolves each scan through.
    pub reads: &'a ReadRegistry,
    /// The injected real-commit hook (the binary's interpreter-backed applier). `None` falls back
    /// to the in-memory [`apply_commit`] (preview-grade, no real I/O) — what unit tests use.
    pub world_apply: Option<&'a WorldApply<'a>>,
    /// The active selectable **safety mode** (t59) governing this one-shot's commit gate, resolved
    /// by the binary from the deployment setting (`/sys/settings` → env → safe default). The
    /// `Default` is [`SafetyMode::AutonomousInPolicy`](qfs_core::SafetyMode) — the historical CLI
    /// posture (reversible-in-policy applies, irreversible needs `--commit-irreversible`).
    pub safety_mode: qfs_core::SafetyMode,
    /// The injected transform executor (blueprint §15): runs a `|> transform` stage's model call
    /// at the COMMIT boundary (binary-side, holding the `ModelProvider`; a deterministic mock in
    /// tests). `None` fails a transform COMMIT closed — PREVIEW never touches it.
    pub transform: Option<std::sync::Arc<dyn qfs_engine::TransformExecutor>>,
}

/// A binary-injected "apply this plan to the World" hook (blueprint §7 `COMMIT : Plan -> World`).
/// `qfs-exec` is deliberately confined off `qfs-runtime` (the interpreter), so the real commit is
/// supplied by the terminal binary, which owns the `qfs-runtime` interpreter + the live driver
/// registry. Returns `Ok(())` once every leg applied, or an [`ExecError`] on a commit failure.
pub type WorldApply<'a> = dyn Fn(&qfs_core::Plan) -> Result<(), ExecError> + 'a;

/// Execute one statement end-to-end and render the result, returning the process [`ExitCode`].
/// This is the single one-shot entry the CLI calls. Never panics; never touches a cwd.
///
/// Pipeline:
/// 1. validate addressing (absolute / `id:` only) — relative path → usage (exit 2);
/// 2. parse — bad syntax → `{"error":{"kind":"parse"}}` (exit 2);
/// 3. unwrap a PREVIEW/COMMIT wrapper (a trailing `COMMIT` forces apply);
/// 4. **read** statement → run the read executor → render `RowSet`;
/// 5. **effect** statement → build the plan; if pure or not committing render PREVIEW (exit 0),
///    unless it is a **destructive set-wide** plan without `--commit` (exit 4); on `--commit`
///    apply via the in-memory engine and render the committed summary.
pub fn run_oneshot(
    source: &StmtSource,
    ctx: &ExecCtx,
    fmt: OutputFormat,
    commit_flag: bool,
    irreversible_ack: bool,
    streams: &mut Streams,
) -> ExitCode {
    match run_oneshot_inner(source, ctx, fmt, commit_flag, irreversible_ack, streams) {
        Ok(code) => code,
        Err(err) => {
            let renderer = fmt.renderer();
            let _ = renderer.error(&err, streams.err);
            err.exit_code()
        }
    }
}

fn run_oneshot_inner(
    source: &StmtSource,
    ctx: &ExecCtx,
    fmt: OutputFormat,
    commit_flag: bool,
    irreversible_ack: bool,
    streams: &mut Streams,
) -> Result<ExitCode, ExecError> {
    let src = source.text();

    // 1. Addressing gate (no cwd in one-shot mode).
    addressing::validate(src)?;

    // 2. Parse.
    let stmt = parse(src)?;

    // 3. Unwrap a PREVIEW/COMMIT wrapper. A trailing `COMMIT` (the engine's keyword) forces
    //    apply, OR'd with the `--commit` switch — the CLI adds zero keywords.
    let (inner, commit) = unwrap_plan(&stmt, commit_flag);

    let renderer = fmt.renderer();

    // Classify by the program's *terminal* statement: a `LET` program (M6, t60) is a read or a
    // write according to the statement its bindings lead into. The full `inner` (bindings and
    // all) is handed to the read/effect path — the evaluator folds the bindings through it.
    match terminal_statement(inner) {
        // 4. Read path: the t20 carry-over closure. THREE reclassifications route a query to the
        // effect path: a pipeline terminating in a `|> CALL` to an EFFECT procedure (drive.copy,
        // mail.send, …), a pipeline terminating in a `|> switch` (blueprint §18 — its arms ARE
        // effects), and — blueprint §15 — a statement carrying a `|> transform` stage
        // ANYWHERE (mid-pipe, subquery, JOIN source, set-op branch, LET binding/body): the model
        // call spends tokens and is non-deterministic, so it is previewed/committed, never
        // silently read. A `CALL` to a result-returning procedure builds a pure (empty) plan and
        // falls through to the read. Every other read keeps its exact prior behaviour.
        Statement::Query(pipeline) => {
            // A fourth reclassification (blueprint §5.6): a statement carrying a `|> of <type>`
            // assertion routes through the evaluator so its plan-time structural/refinement check
            // runs against the *addressed*-path schema (the pushdown lowering sees only the driver
            // ROOT, too coarse). A passing `of` builds an empty plan and falls through to the read
            // below; a failing one surfaces the structured error from `build_plan` here.
            if matches!(
                pipeline.ops.last(),
                Some(qfs_parser::PipeOp::Call(_) | qfs_parser::PipeOp::Switch(_))
            ) || contains_transform(inner)
                || contains_of(inner)
            {
                let plan = build_plan(inner, ctx.engine)?;
                if !plan.nodes().is_empty() {
                    return preview_or_commit(
                        &plan,
                        inner,
                        commit,
                        irreversible_ack,
                        ctx,
                        renderer.as_ref(),
                        streams,
                    );
                }
            }
            let rows = block_on_read(inner, &ctx.engine.mounts, ctx.reads)?;
            renderer.rows(&rows, streams.out).map_err(io_err)?;
            Ok(ExitCode::Ok)
        }
        // 5. Effect / DDL / TRANSACTION path: PREVIEW by default, COMMIT on demand. A
        // `TRANSACTION { … }` block (M6, t62) lowers to one effect plan (reversible-only,
        // all-or-nothing), so it routes through the same plan/commit machinery as a plain effect.
        Statement::Effect(_) | Statement::Ddl(_) | Statement::Transaction { .. } => {
            let plan = build_plan(inner, ctx.engine)?;
            preview_or_commit(
                &plan,
                inner,
                commit,
                irreversible_ack,
                ctx,
                renderer.as_ref(),
                streams,
            )
        }
        // `terminal_statement` descends through PlanWrap/LET to a leaf, so these arms are
        // unreachable; kept total (no panic) by treating them as a pure preview.
        Statement::Plan(_) | Statement::Let { .. } => Ok(ExitCode::Ok),
    }
}

/// PREVIEW-by-default / COMMIT-on-demand handling of a built effect [`Plan`] — shared by the
/// effect-statement path and a query that terminated in a `CALL` to an effect procedure (so both
/// surfaces preview, gate, and commit identically). Renders the dry-run PREVIEW when not
/// committing (refusing a destructive set-wide plan on the commit-required exit class), and on
/// `--commit` applies through the selected safety mode composed on the t37 irreversible floor.
fn preview_or_commit(
    plan: &Plan,
    inner: &Statement,
    commit: bool,
    irreversible_ack: bool,
    ctx: &ExecCtx,
    renderer: &dyn Renderer,
    streams: &mut Streams,
) -> Result<ExitCode, ExecError> {
    if commit {
        // The selectable safety mode (t59) governs this one-shot's commit, composed on the t37
        // irreversible floor. `qfs run … --commit` is a NON-INTERACTIVE one-shot (no TTY to
        // confirm on), so a held effect is refused unless the operator passed the explicit
        // `--commit-irreversible` ack. `within_policy` is `true`: the CLI one-shot trusts the
        // local operator's capability set (gating already ran at parse time; a server face gates
        // with its POLICY instead). The mode then decides autonomous-in-policy / approve-everything
        // / policy-only, and a hold is a clean fail-closed refusal that applies ZERO effects.
        let ack = if irreversible_ack {
            qfs_core::Ack::Granted
        } else {
            qfs_core::Ack::Absent
        };
        if qfs_core::IrreversibleGuard::decide(plan, ctx.safety_mode, true, ack)
            != qfs_core::SafetyDecision::AutoCommit
        {
            // Render the PREVIEW so the operator sees exactly what would have applied, then refuse
            // on the commit-required exit class (the code distinguishes the held cause).
            let summary = plan_preview(plan);
            renderer.plan(&summary, streams.out).map_err(io_err)?;
            let (code, message) = held_commit_reason(ctx.safety_mode, plan);
            return Err(ExecError::new(ErrorKind::CommitRequired, code, message));
        }
        // §15 transform orchestration at the COMMIT boundary: the injected executor is wrapped
        // in a producing-row counter so the plan's consent nodes carry the exact affected count
        // into the ledger. The gate above already ran, so this executes only on a going-through
        // commit — PREVIEW structurally cannot reach the executor.
        let transform_read = contains_transform(inner);
        let counting = if transform_read {
            ctx.transform
                .clone()
                .map(|inner_exec| std::sync::Arc::new(CountingTransformExecutor::new(inner_exec)))
        } else {
            None
        };
        let injected: Option<std::sync::Arc<dyn qfs_engine::TransformExecutor>> = counting
            .clone()
            .map(|c| c as std::sync::Arc<dyn qfs_engine::TransformExecutor>);

        // A terminal `|> switch` statement (blueprint §18) commits through the routing boundary:
        // the source materializes ONCE (the model, if any, runs here), rows partition by the
        // discriminant, each taken arm's continuation folds over its partition and lands in its
        // consented effect node, and an arm with an empty partition is PRUNED — previewed and
        // consented, but never fired. Intercepted before the generic read/materialize paths,
        // which cannot see a switch (its lowering is effect-side only).
        if let Some((source, stage)) = terminal_switch(inner) {
            return commit_switch(
                plan, &source, stage, counting, injected, ctx, renderer, streams,
            );
        }

        // A transform-bearing statement whose TERMINAL is a read (blueprint §15 / §14): run the
        // read WITH the executor (upstream read → model call → OUTPUT membership → downstream
        // segment, all inside the engine walk), ledger the consent nodes with the refined count,
        // and render the committed-read envelope — the §14 `RowSet` carries rows + `meta.affected`
        // (non-null signals effects ran), never just a commit summary.
        if transform_read {
            if let Statement::Query(_) = terminal_statement(inner) {
                let mut rows =
                    block_on_read_with(inner, &ctx.engine.mounts, ctx.reads, injected.clone())?;
                let affected = counting.as_ref().map_or(0, |c| c.produced());
                let mut to_apply = plan.clone();
                refine_transform_affected(&mut to_apply, affected);
                // The source READ(s) were already serviced by `block_on_read_with` above; the only
                // nodes left to apply are the transform-consent markers. Drop the source reads so
                // they are never dispatched to a driver's WRITE applier — one that services no READ
                // fails "<verb> is not serviced" (round-6 Gmail defect: a read-terminal transform
                // over /mail/inbox failed at commit though the same source reads fine on the
                // switch/write-terminal path, which already strips source reads via
                // `consume_source_into_write`).
                strip_source_reads(&mut to_apply);
                apply_via(&to_apply, ctx.world_apply)?;
                rows.meta.affected = Some(affected as i64);
                renderer.rows(&rows, streams.out).map_err(io_err)?;
                return Ok(ExitCode::Ok);
            }
        }

        // Commit-boundary materialization (blueprint §7): a pipeline/`FROM`-sourced write buffers
        // its source rows into the write node's `args` here, above the interpreter, right before
        // apply — so the read side runs only once the commit is actually going through (never on a
        // held/refused commit). A `VALUES`/`SET` write is untouched (a no-op). A transform inside
        // the source pipeline composes here: the injected executor runs the model stage during
        // materialization, so the write's `args` carry the OUTPUT rows.
        let mut to_apply = plan.clone();
        materialize_pipeline_source(inner, &mut to_apply, ctx.engine, ctx.reads, injected)?;
        if let Some(c) = counting.as_ref() {
            refine_transform_affected(&mut to_apply, c.produced());
        }
        let summary = apply_via(&to_apply, ctx.world_apply)?;
        renderer.plan(&summary, streams.out).map_err(io_err)?;
        Ok(ExitCode::Ok)
    } else if is_destructive_set(plan) {
        // A destructive set-wide plan requires explicit commit (exit 4). Still render the PREVIEW
        // so the operator/agent sees the affected counts.
        let summary = plan_preview(plan);
        renderer.plan(&summary, streams.out).map_err(io_err)?;
        Err(ExecError::new(
            ErrorKind::CommitRequired,
            "commit_required",
            "destructive set-wide plan: re-run with --commit (or a trailing COMMIT) to apply",
        ))
    } else {
        let summary = plan_preview(plan);
        renderer.plan(&summary, streams.out).map_err(io_err)?;
        Ok(ExitCode::Ok)
    }
}

/// The commit-boundary materialization cap (blueprint §7, ticket 20260704164315): a
/// pipeline/`FROM`-sourced write re-executes its source through the read engine and buffers the
/// produced rows in memory before the write applies. Beyond this many rows the copy is refused with
/// the in-driver remedy — a large *same-driver* copy belongs in the driver (`cp` / `CALL
/// drive.copy`, named parks for pushdown/streaming), not the generic materialization channel that
/// would buffer the whole result set.
const MAX_MATERIALIZED_ROWS: usize = 10_000;

/// **Commit-boundary materialization** (blueprint §7): a write whose source is a pipeline/`FROM`
/// query (`… |> upsert into <dst>`, `INSERT … FROM`) carries no literal `VALUES` payload — its plan
/// is a `Read` dependency marker feeding a write node with empty `args`. At `--commit`, ABOVE the
/// interpreter, re-execute that source through the existing (cross-driver) read engine and embed the
/// produced rows into the write effect's `args.rows` — the same channel a `VALUES` write uses. The
/// `Read` node stays in the plan (so preview and commit have the same shape) but is never dispatched:
/// the interpreter ledgers it applied as a dependency marker (so `LocalEffect::Scan` never scans a
/// single file → `ENOTDIR`). A no-source (`VALUES`/`SET`) write is a no-op here.
///
/// # Errors
/// The source read's [`ExecError`], or a `usage` error if the materialization exceeds the cap.
pub(crate) fn materialize_pipeline_source(
    inner: &Statement,
    plan: &mut Plan,
    engine: &Engine,
    reads: &ReadRegistry,
    transform: Option<std::sync::Arc<dyn qfs_engine::TransformExecutor>>,
) -> Result<(), ExecError> {
    use qfs_parser::EffectBody;
    let Statement::Effect(effect) = terminal_statement(inner) else {
        return Ok(());
    };
    let EffectBody::Pipeline(source) = &effect.body else {
        return Ok(());
    };
    // Re-execute the source pipeline through the read engine (the bare read demonstrably returns the
    // bytes; the engine is already cross-driver). A `|> transform` stage in the source runs through
    // the injected executor here — at the commit boundary, never on a held/refused commit.
    let source_stmt = Statement::Query((**source).clone());
    let rows = block_on_read_with(&source_stmt, &engine.mounts, reads, transform)?;
    if rows.len() > MAX_MATERIALIZED_ROWS {
        return Err(ExecError::new(
            ErrorKind::Usage,
            "materialization_too_large",
            format!(
                "the copy source produced {} rows, over the {MAX_MATERIALIZED_ROWS}-row \
                 commit-materialization cap; for a large same-driver copy use an in-driver form \
                 (`cp`, or `CALL drive.copy`)",
                rows.len()
            ),
        ));
    }
    let batch = qfs_core::RowBatch::new(rows.schema, rows.rows);
    consume_source_into_write(plan, batch);
    Ok(())
}

/// Embed materialized source rows into a pipeline-sourced write and drop the consumed source
/// `Read` node (the pure plan transform behind [`materialize_pipeline_source`]). The write is the
/// plan's single non-`Read` node; the rows land in its `args` (the same channel `VALUES` uses) and
/// its affected estimate is refined to the exact count. The source `Read` node(s) the write depends
/// on (a dep tuple is `(parent, child)`, so a source read is a `parent` of the write) are removed —
/// so the pipeline source never reaches a driver (no single-file directory scan → no `ENOTDIR`),
/// while a genuine driver read-effect (e.g. the REST `GET`-at-commit) is untouched.
fn consume_source_into_write(plan: &mut Plan, batch: qfs_core::RowBatch) {
    let count = batch.rows.len() as u64;
    let Some(write_id) = plan
        .nodes
        .iter()
        .find(|n| !matches!(n.kind, qfs_core::EffectKind::Read) && !is_transform_consent(n))
        .map(|n| n.id)
    else {
        return;
    };
    for node in &mut plan.nodes {
        if node.id == write_id {
            node.args = batch;
            if matches!(node.est_affected, qfs_core::Affected::Unknown) {
                node.est_affected = qfs_core::Affected::Exact(count);
            }
            break;
        }
    }
    let source_reads: Vec<_> = plan
        .deps
        .iter()
        .filter(|(_parent, child)| *child == write_id)
        .map(|(parent, _child)| *parent)
        .filter(|id| {
            plan.node(*id)
                .is_some_and(|n| matches!(n.kind, qfs_core::EffectKind::Read))
        })
        .collect();
    plan.nodes.retain(|n| !source_reads.contains(&n.id));
    plan.deps
        .retain(|(parent, child)| !(*child == write_id && source_reads.contains(parent)));
}

/// Drop every `Read` node (and its dep edges) from a read-terminal transform plan before the
/// consent-ledger apply. The read side already ran through the read engine
/// ([`block_on_read_with`]); the only nodes left to apply are the transform-consent markers, so a
/// source `Read` must never reach a driver's write applier. A driver whose applier services no READ
/// (Gmail, GitHub, Slack) would otherwise reject it — the write-terminal path removes source reads
/// for the same reason ([`consume_source_into_write`]).
fn strip_source_reads(plan: &mut Plan) {
    let reads: Vec<_> = plan
        .nodes
        .iter()
        .filter(|n| matches!(n.kind, qfs_core::EffectKind::Read))
        .map(|n| n.id)
        .collect();
    plan.nodes
        .retain(|n| !matches!(n.kind, qfs_core::EffectKind::Read));
    plan.deps
        .retain(|(parent, child)| !reads.contains(parent) && !reads.contains(child));
}

/// If the statement's terminal is a query pipeline ending in `|> switch` (blueprint §18), return
/// the SOURCE statement (the pipeline minus the switch — what materializes once at the commit
/// boundary) and the switch stage. `None` for every other statement shape.
fn terminal_switch(stmt: &Statement) -> Option<(Statement, &qfs_parser::SwitchStage)> {
    let Statement::Query(pipeline) = terminal_statement(stmt) else {
        return None;
    };
    let Some(qfs_parser::PipeOp::Switch(stage)) = pipeline.ops.last() else {
        return None;
    };
    let source = Statement::Query(qfs_parser::Pipeline {
        source: pipeline.source.clone(),
        ops: pipeline.ops[..pipeline.ops.len() - 1].to_vec(),
    });
    Some((source, stage))
}

/// The COMMIT boundary of a terminal `|> switch` statement (blueprint §18-C): materialize the
/// source once (the model stage, if any, runs here — exactly once), partition the produced rows
/// by the discriminant column (an unmatched or non-text value falls to `else`), fold each taken
/// arm's continuation over its partition, land the routed rows in the arm's consented write node
/// (the `VALUES` channel), prune untaken arms (previewed-but-not-fired, `affected: 0` spend), and
/// apply the pruned union through the ordinary interpreter.
#[allow(clippy::too_many_arguments)]
fn commit_switch(
    plan: &Plan,
    source: &Statement,
    stage: &qfs_parser::SwitchStage,
    counting: Option<std::sync::Arc<CountingTransformExecutor>>,
    injected: Option<std::sync::Arc<dyn qfs_engine::TransformExecutor>>,
    ctx: &ExecCtx,
    renderer: &dyn Renderer,
    streams: &mut Streams,
) -> Result<ExitCode, ExecError> {
    // 1. Materialize the source once through the read engine (the same channel a
    //    pipeline-sourced write uses), under the same cap.
    let rows = block_on_read_with(source, &ctx.engine.mounts, ctx.reads, injected)?;
    if rows.rows.len() > MAX_MATERIALIZED_ROWS {
        return Err(ExecError::new(
            ErrorKind::Usage,
            "materialization_too_large",
            format!(
                "the switch source produced {} rows, over the {MAX_MATERIALIZED_ROWS}-row \
                 commit-materialization cap",
                rows.rows.len()
            ),
        ));
    }

    // 2. Partition by the discriminant column, in arm declaration order. The evaluator already
    //    verified the column against the folded schema where it was concrete; a late-bound
    //    source that still lacks it at materialization is a structured refusal (never a
    //    silently-empty routing).
    let Some(discr) = rows
        .schema
        .columns
        .iter()
        .position(|c| c.name == stage.discriminant)
    else {
        return Err(ExecError::new(
            ErrorKind::Usage,
            "switch_discriminant_unknown",
            format!(
                "the switch discriminant column `{}` is not carried by the materialized source",
                stage.discriminant
            ),
        ));
    };
    let else_idx = stage
        .arms
        .iter()
        .position(|a| a.label.is_none())
        .ok_or_else(|| {
            // The evaluator's shape check makes this unreachable; stay structured.
            ExecError::new(
                ErrorKind::Internal,
                "switch_shape",
                "switch has no else arm at commit",
            )
        })?;
    let mut partitions: Vec<Vec<qfs_core::Row>> = stage.arms.iter().map(|_| Vec::new()).collect();
    let schema = rows.schema.clone();
    for row in rows.rows {
        let taken = row
            .values
            .get(discr)
            .and_then(|v| match v {
                qfs_core::Value::Text(s) => Some(s.as_str()),
                _ => None,
            })
            .and_then(|label| {
                stage
                    .arms
                    .iter()
                    .position(|a| a.label.as_deref() == Some(label))
            })
            .unwrap_or(else_idx);
        partitions[taken].push(row);
    }

    // 3. Map arms onto their consented effect nodes: the evaluator lowers one effect node per
    //    arm (plus a Read marker per write arm and the §15 consent nodes), sequenced in
    //    declaration order — so the arm nodes are exactly the non-Read, non-consent nodes in id
    //    order. A count mismatch is an invariant break, surfaced structurally.
    let mut to_apply = plan.clone();
    let arm_nodes: Vec<qfs_core::NodeId> = to_apply
        .nodes
        .iter()
        .filter(|n| !matches!(n.kind, qfs_core::EffectKind::Read) && !is_transform_consent(n))
        .map(|n| n.id)
        .collect();
    if arm_nodes.len() != stage.arms.len() {
        return Err(ExecError::new(
            ErrorKind::Internal,
            "switch_plan_shape",
            format!(
                "switch plan carries {} arm effect nodes for {} arms",
                arm_nodes.len(),
                stage.arms.len()
            ),
        ));
    }

    // 4. Fold each taken arm's continuation over its partition and embed; prune untaken arms.
    let mut pruned: Vec<qfs_core::NodeId> = Vec::new();
    for ((arm, node_id), partition) in stage.arms.iter().zip(&arm_nodes).zip(partitions) {
        if partition.is_empty() {
            pruned.push(*node_id);
            continue;
        }
        if arm.write.is_none() {
            // A terminal-CALL arm: the node's args are the call's literal arguments; a
            // non-empty partition simply lets it fire.
            continue;
        }
        let routed = eval_arm_ops(arm, qfs_core::RowBatch::new(schema.clone(), partition), ctx)?;
        for node in &mut to_apply.nodes {
            if node.id == *node_id {
                node.est_affected = qfs_core::Affected::Exact(routed.rows.len() as u64);
                node.args = routed;
                break;
            }
        }
    }
    // Remove untaken arm nodes AND every source-Read marker (each write's rows are embedded now;
    // the markers must never dispatch to a driver), BRIDGING each removed node's dependencies
    // (every parent → every child) so the consent → arm₁ → arm₂ → … chain survives the removal.
    // Dropping a pruned node's edges without bridging was the live-round ordering bug: the later
    // arm lost ALL its deps, became a free node, and the interpreter could fire it BEFORE an
    // earlier arm — breaking §18-C declaration order AND letting an arm fire after an earlier
    // arm had already failed (no fail-stop).
    pruned.extend(
        to_apply
            .nodes
            .iter()
            .filter(|n| matches!(n.kind, qfs_core::EffectKind::Read) && !is_transform_consent(n))
            .map(|n| n.id),
    );
    prune_nodes_bridging(&mut to_apply, &pruned);

    // 5. Refine the §15 consent spend from what actually ran, then apply the pruned union.
    if let Some(c) = counting.as_ref() {
        refine_transform_affected(&mut to_apply, c.produced());
    }
    let summary = apply_via(&to_apply, ctx.world_apply)?;
    renderer.plan(&summary, streams.out).map_err(io_err)?;
    Ok(ExitCode::Ok)
}

/// Remove `remove` nodes from `plan`, BRIDGING each removed node's dependency edges: every
/// parent of a removed node becomes a parent of each of its children, so the transitive
/// ordering that flowed THROUGH the removed node survives its removal. Nodes are removed one at
/// a time (a removed node's bridged edges can themselves be re-bridged when a chain of removed
/// nodes sits between two survivors). Duplicate edges are deduplicated at the end.
fn prune_nodes_bridging(plan: &mut Plan, remove: &[qfs_core::NodeId]) {
    for id in remove {
        let parents: Vec<qfs_core::NodeId> = plan
            .deps
            .iter()
            .filter(|(_, child)| child == id)
            .map(|(parent, _)| *parent)
            .collect();
        let children: Vec<qfs_core::NodeId> = plan
            .deps
            .iter()
            .filter(|(parent, _)| parent == id)
            .map(|(_, child)| *child)
            .collect();
        plan.deps
            .retain(|(parent, child)| parent != id && child != id);
        for p in &parents {
            for c in &children {
                plan.deps.push((*p, *c));
            }
        }
        plan.nodes.retain(|n| n.id != *id);
    }
    plan.deps.sort_unstable();
    plan.deps.dedup();
}

/// Fold one switch arm's continuation ops over its routed partition (blueprint §18-C). The arm
/// pipeline is planned over a synthetic `VALUES` source (a local leaf nothing pushes into), and
/// the partition batch is supplied directly as that leaf's scan result — the engine's residual
/// re-application does the rest. Arms passed the row-local vocabulary check at plan time, so the
/// physical plan has exactly one scan leaf and no model stage.
fn eval_arm_ops(
    arm: &qfs_parser::SwitchArm,
    partition: qfs_core::RowBatch,
    ctx: &ExecCtx,
) -> Result<qfs_core::RowBatch, ExecError> {
    if arm.ops.is_empty() {
        return Ok(partition);
    }
    let synthetic = Statement::Query(qfs_parser::Pipeline {
        source: qfs_parser::Source::Values(qfs_parser::Values {
            columns: None,
            rows: Vec::new(),
        }),
        ops: arm.ops.clone(),
    });
    let physical = qfs_core::plan_query(&synthetic, &ctx.engine.mounts).map_err(|e| {
        ExecError::new(
            ErrorKind::Internal,
            "switch_arm_plan",
            format!("switch arm continuation failed to plan: {e:?}"),
        )
    })?;
    if physical.scans().len() != 1 {
        return Err(ExecError::new(
            ErrorKind::Internal,
            "switch_arm_plan",
            "switch arm continuation planned more than one scan leaf",
        ));
    }
    use qfs_engine::CombineEngine as _;
    let out = qfs_engine::MiniEvaluator::new()
        .execute(&physical, qfs_engine::ScanResults::new(vec![partition]))
        .map_err(|e| {
            ExecError::new(
                ErrorKind::CommitFailed,
                "switch_arm_eval",
                format!("switch arm continuation failed over its partition: {e}"),
            )
        })?;
    Ok(out)
}

/// The stable `(code, message)` for a one-shot commit HELD by the active safety mode (t59). An
/// irreversible plan keeps the historical `irreversible_ack_required` contract (so the exit-class
/// code is stable across modes); a reversible plan held by the *approve-everything* mode reports
/// `approval_required`. Both fail closed on the `commit_required` exit class (exit 4) with a
/// secret-free message naming the ack to supply.
fn held_commit_reason(mode: qfs_core::SafetyMode, plan: &qfs_core::Plan) -> (&'static str, String) {
    if plan.is_irreversible() {
        (
            "irreversible_ack_required",
            "plan contains an irreversible effect (REMOVE / CALL); re-run with \
             --commit-irreversible to apply (or in an interactive session)"
                .to_string(),
        )
    } else {
        (
            "approval_required",
            format!(
                "the `{mode}` safety mode holds every write for explicit approval; re-run with \
                 --commit-irreversible to apply this write"
            ),
        )
    }
}

/// The terminal statement a program leads into, descending through `LET` bindings (M6, t60) and
/// `PREVIEW`/`COMMIT` wrappers to the underlying read/effect/DDL leaf. Used to route a `LET`
/// program to the read or effect path — the leaf is never a `LET` or a `Plan`.
fn terminal_statement(stmt: &Statement) -> &Statement {
    match stmt {
        Statement::Let { body, .. } => terminal_statement(body),
        Statement::Plan(PlanWrap { inner, .. }) => terminal_statement(inner),
        other => other,
    }
}

/// Whether a statement carries a `|> transform` stage ANYWHERE — the whole-tree classifier
/// (blueprint §15): mid-pipe, subquery source, `JOIN` source, set-op branch, `LET` binding and
/// body, and an effect body pipeline all classify the statement as effect-bearing.
fn contains_transform(stmt: &Statement) -> bool {
    use qfs_parser::EffectBody;
    match stmt {
        Statement::Query(p) => pipeline_has_transform(p),
        Statement::Effect(e) => match &e.body {
            EffectBody::Pipeline(p) => pipeline_has_transform(p),
            _ => false,
        },
        Statement::Plan(PlanWrap { inner, .. }) => contains_transform(inner),
        Statement::Let { value, body, .. } => contains_transform(value) || contains_transform(body),
        Statement::Transaction { body, .. } => body.iter().any(contains_transform),
        Statement::Ddl(_) => false,
    }
}

fn pipeline_has_transform(p: &qfs_parser::Pipeline) -> bool {
    use qfs_parser::{PipeOp, Source};
    let source_has = |s: &Source| match s {
        Source::Subquery(sub) => pipeline_has_transform(sub),
        _ => false,
    };
    source_has(&p.source)
        || p.ops.iter().any(|op| match op {
            PipeOp::Transform(_) => true,
            PipeOp::Join(j) => source_has(&j.source),
            PipeOp::Union(sub) | PipeOp::Except(sub) | PipeOp::Intersect(sub) => {
                pipeline_has_transform(sub)
            }
            _ => false,
        })
}

/// Whether a statement carries a `|> of <type>` assertion (blueprint §5.6). A pure read reaches only
/// the pushdown lowering, whose leaf schema is the driver ROOT — too coarse for `of`'s structural
/// check. So a read carrying an `of` is routed through the EVALUATOR (`build_plan`), which describes
/// the *addressed* path and runs `check_of_assertion` against the true schema; a mismatch surfaces
/// there before the read ever runs. The mirror of [`contains_transform`]'s routing.
fn contains_of(stmt: &Statement) -> bool {
    use qfs_parser::EffectBody;
    match stmt {
        Statement::Query(p) => pipeline_has_of(p),
        Statement::Effect(e) => match &e.body {
            EffectBody::Pipeline(p) => pipeline_has_of(p),
            _ => false,
        },
        Statement::Plan(PlanWrap { inner, .. }) => contains_of(inner),
        Statement::Let { value, body, .. } => contains_of(value) || contains_of(body),
        Statement::Transaction { body, .. } => body.iter().any(contains_of),
        Statement::Ddl(_) => false,
    }
}

fn pipeline_has_of(p: &qfs_parser::Pipeline) -> bool {
    use qfs_parser::{PipeOp, Source};
    let source_has = |s: &Source| match s {
        Source::Subquery(sub) => pipeline_has_of(sub),
        _ => false,
    };
    source_has(&p.source)
        || p.ops.iter().any(|op| match op {
            PipeOp::Of(_) => true,
            PipeOp::Join(j) => source_has(&j.source),
            PipeOp::Union(sub) | PipeOp::Except(sub) | PipeOp::Intersect(sub) => {
                pipeline_has_of(sub)
            }
            _ => false,
        })
}

/// Whether an effect node is a §15 transform consent/audit node (the `transform.<name>` CALL the
/// evaluator emits): the target driver is `transform` and the proc rides its namespace.
fn is_transform_consent(node: &qfs_core::EffectNode) -> bool {
    matches!(&node.kind, qfs_core::EffectKind::Call(proc) if proc.as_str().starts_with("transform."))
        && node.target.driver.as_str() == "transform"
}

/// Refine every transform consent node's `Unknown` affected estimate to the EXACT count of rows
/// the model stages produced (summed by the counting wrapper), so the ledger and the committed
/// summary carry the real spend rather than a placeholder.
fn refine_transform_affected(plan: &mut Plan, produced: u64) {
    for node in &mut plan.nodes {
        if is_transform_consent(node) {
            node.est_affected = qfs_core::Affected::Exact(produced);
        }
    }
}

/// A [`TransformExecutor`](qfs_engine::TransformExecutor) wrapper that counts the rows the model
/// stages produce, so the commit boundary can refine the consent nodes' affected estimates and
/// the committed-read envelope's `meta.affected` from what actually ran.
struct CountingTransformExecutor {
    inner: std::sync::Arc<dyn qfs_engine::TransformExecutor>,
    produced: std::sync::atomic::AtomicU64,
}

impl CountingTransformExecutor {
    fn new(inner: std::sync::Arc<dyn qfs_engine::TransformExecutor>) -> Self {
        Self {
            inner,
            produced: std::sync::atomic::AtomicU64::new(0),
        }
    }

    fn produced(&self) -> u64 {
        self.produced.load(std::sync::atomic::Ordering::Relaxed)
    }
}

impl qfs_engine::TransformExecutor for CountingTransformExecutor {
    fn execute(
        &self,
        call: &qfs_engine::TransformCall<'_>,
        input: qfs_core::RowBatch,
    ) -> Result<qfs_core::RowBatch, String> {
        let out = self.inner.execute(call, input)?;
        self.produced
            .fetch_add(out.rows.len() as u64, std::sync::atomic::Ordering::Relaxed);
        Ok(out)
    }
}

/// Execute `qfs describe <path>` (ticket t39): resolve `path` to its driver in the describe
/// registry, fold the driver's pure introspective half into a [`qfs_core::DescribeReport`], and
/// render it via the t29 output layer (human table / JSON). Returns the process [`ExitCode`].
///
/// DESCRIBE is **pure** — no creds, no I/O, no network: the registry holds describe-only drivers
/// and only the introspective half is touched (the applier seam is never reached). An
/// unresolvable path or a non-describable node renders a structured error (exit 2/3) — the
/// agent-legible failure path — never a panic.
pub fn run_describe(
    path: &str,
    describe: &qfs_core::MountRegistry,
    fmt: OutputFormat,
    streams: &mut Streams,
) -> ExitCode {
    match run_describe_inner(path, describe, fmt, streams) {
        Ok(code) => code,
        Err(err) => {
            let renderer = fmt.renderer();
            let _ = renderer.error(&err, streams.err);
            err.exit_code()
        }
    }
}

fn run_describe_inner(
    path: &str,
    describe: &qfs_core::MountRegistry,
    fmt: OutputFormat,
    streams: &mut Streams,
) -> Result<ExitCode, ExecError> {
    // Addressing gate: DESCRIBE addresses an absolute path (no cwd in one-shot mode), same as
    // `qfs run`. A relative path is a usage error (exit 2).
    addressing::validate_path(path)?;

    // The host-realm path canon (decision P / owner ruling 2026-07-16): peel a
    // `/hosts/local/<svc>/…` address to its service path before routing, and refuse the retired
    // bare spelling of a host-realm-only mount with the canonical pointer — DESCRIBE teaches,
    // so it must not keep teaching a retired address.
    let canonical = describe.canonicalize_host_path(path).map_err(|e| {
        ExecError::new(ErrorKind::Capability, e.code(), e.to_string()).with_path(path)
    })?;

    // 番地の`@選択` (plan.md, 閉包の原理): a trailing selection segment names a ROW, and a
    // row is a node — it answers describe. Split it off, describe the BASE, then refine the
    // report into the row view (validated against the driver's DECLARED child key).
    let (base, selection) = qfs_core::split_selection(&canonical);

    // Resolve the path to its describe-only driver (longest-mount-prefix match).
    let (driver, _rest) = describe.resolve_path(base).ok_or_else(|| {
        ExecError::new(
            ErrorKind::Capability,
            "unknown_mount",
            format!("no driver is mounted for `{path}` (describe registry)"),
        )
        .with_path(path)
    })?;

    // Fold the introspective half into the report — pure, no I/O, no creds (described at the
    // peeled SERVICE path: the driver speaks its own mount namespace).
    let report = qfs_core::DescribeReport::from_driver(driver.as_ref(), &qfs_core::Path::new(base))
        .map_err(|e| ExecError::from_qfs(&e))?;
    let report = match selection {
        Some(raw) => report.for_selected_row(&canonical, raw).map_err(|e| {
            ExecError::new(ErrorKind::Usage, e.code(), e.to_string()).with_path(path)
        })?,
        None => report,
    };

    let renderer = fmt.renderer();
    renderer.describe(&report, streams.out).map_err(io_err)?;
    Ok(ExitCode::Ok)
}

/// Unwrap a `PREVIEW`/`COMMIT` plan wrapper, returning the inner statement and whether to commit
/// (a trailing `COMMIT` OR the `--commit` switch). PREVIEW/COMMIT nest at most once in practice;
/// the loop is defensive.
fn unwrap_plan(stmt: &Statement, commit_flag: bool) -> (&Statement, bool) {
    let mut cur = stmt;
    let mut commit = commit_flag;
    while let Statement::Plan(PlanWrap {
        commit: c, inner, ..
    }) = cur
    {
        commit = commit || *c;
        cur = inner;
    }
    (cur, commit)
}

/// Whether a plan is **destructive over a set** — the exit-4 gate. Grammar-agnostic: it reads the
/// plan's effect metadata (`irreversible` + the affected estimate), never CLI keywords. A plan is
/// destructive-set-wide iff it has an irreversible effect that could touch **more than one** row
/// (an `AtMost(n>1)` / `Unknown` estimate). A single-row irreversible effect (`Exact(0)`/`(1)`,
/// `AtMost(1)`) is not "over a set" and previews at exit 0.
#[must_use]
fn is_destructive_set(plan: &Plan) -> bool {
    use qfs_core::Affected;
    plan.nodes().iter().any(|n| {
        n.irreversible
            && match n.est_affected {
                Affected::Exact(c) | Affected::AtMost(c) => c > 1,
                Affected::Unknown => true,
            }
    })
}

/// Wrap a writer `io::Error` as an internal [`ExecError`] (a broken stdout/stderr pipe is not a
/// user error, but it must not panic).
fn io_err(e: std::io::Error) -> ExecError {
    ExecError::new(ErrorKind::Internal, "io_error", e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prune_nodes_bridging_preserves_transitive_order() {
        use qfs_core::{DriverId, EffectKind, EffectNode, PlanBuilder, Target, VfsPath};
        // The §18 switch commit shape: consent → read₁ → write₁ → read₂ → write₂. Pruning the
        // two Read markers must BRIDGE the chain to consent → write₁ → write₂ — dropping the
        // edges instead was the live-round ordering bug (write₂ became dep-free and could fire
        // before write₁, breaking declaration order and fail-stop).
        let mut b = PlanBuilder::new();
        let mk = |b: &mut PlanBuilder, kind: EffectKind| {
            let id = b.next_id();
            b.push(EffectNode::new(
                id,
                kind,
                Target::new(DriverId::new("t"), VfsPath::new("/t")),
            ))
        };
        let consent = mk(
            &mut b,
            EffectKind::Call(qfs_core::ProcId::new("transform.x")),
        );
        let read1 = mk(&mut b, EffectKind::Read);
        let write1 = mk(&mut b, EffectKind::Insert);
        let read2 = mk(&mut b, EffectKind::Read);
        let write2 = mk(&mut b, EffectKind::Insert);
        b.depends_on(read1, consent);
        b.depends_on(write1, read1);
        b.depends_on(read2, write1);
        b.depends_on(write2, read2);
        let mut plan = b.build();
        prune_nodes_bridging(&mut plan, &[read1, read2]);
        assert_eq!(plan.nodes.len(), 3, "the two Read markers are gone");
        assert!(
            plan.deps.contains(&(consent, write1)),
            "consent → write₁ bridged: {:?}",
            plan.deps
        );
        assert!(
            plan.deps.contains(&(write1, write2)),
            "write₁ → write₂ bridged (declaration order survives): {:?}",
            plan.deps
        );
        assert_eq!(plan.deps.len(), 2, "no dangling or duplicate edges");
    }

    #[test]
    fn consume_source_into_write_embeds_rows_and_drops_the_read() {
        use qfs_core::{
            Column, ColumnType, DriverId, EffectKind, EffectNode, NodeId, PlanBuilder, Row,
            RowBatch, Schema, Target, Value, VfsPath,
        };
        // The plan shape of `/local/src.txt |> upsert into /local/dst.txt`: a `Read` dependency
        // marker feeding a write node with empty args (ticket 20260704164315 / blueprint §7).
        let mut b = PlanBuilder::new();
        let read = b.push(EffectNode::new(
            NodeId(0),
            EffectKind::Read,
            Target::new(DriverId::new("local"), VfsPath::new("/local/src.txt")),
        ));
        let write = b.push(EffectNode::new(
            NodeId(1),
            EffectKind::Upsert,
            Target::new(DriverId::new("local"), VfsPath::new("/local/dst.txt")),
        ));
        b.depends_on(write, read);
        let mut plan = b.build();
        assert_eq!(plan.nodes.len(), 2, "Read + write before materialization");
        assert!(plan.node(NodeId(1)).unwrap().args.rows.is_empty());

        // Materialize a one-row source (a blob `content` column).
        let batch = RowBatch::new(
            Schema::new(vec![Column::new("content", ColumnType::Bytes, false)]),
            vec![Row::new(vec![Value::Bytes(b"hello".to_vec())])],
        );
        consume_source_into_write(&mut plan, batch);

        // The source Read node is consumed (gone); only the write remains, now carrying the rows.
        assert_eq!(plan.nodes.len(), 1, "the consumed source Read is dropped");
        let w = plan.node(NodeId(1)).expect("the write survives");
        assert_eq!(
            w.args.rows.len(),
            1,
            "the source rows land in the write args"
        );
        assert_eq!(w.args.rows[0].values[0], Value::Bytes(b"hello".to_vec()));
        assert!(
            plan.deps.iter().all(|(_p, c)| *c != NodeId(1)),
            "the write's source dependency edge is dropped"
        );
    }

    #[test]
    fn resolve_source_requires_exactly_one() {
        assert!(resolve_source(Some("a".into()), None, None).is_ok());
        assert!(resolve_source(None, Some("a".into()), None).is_ok());
        assert!(resolve_source(None, None, Some("a".into())).is_ok());
        // Zero sources.
        let e = resolve_source(None, None, None).unwrap_err();
        assert_eq!(e.kind.as_str(), "usage");
        // Two sources.
        let e = resolve_source(Some("a".into()), Some("b".into()), None).unwrap_err();
        assert_eq!(e.kind.as_str(), "usage");
    }

    #[test]
    fn stmt_source_text_unwraps() {
        assert_eq!(StmtSource::Expr("x".into()).text(), "x");
        assert_eq!(StmtSource::Stdin("y".into()).text(), "y");
    }
}
