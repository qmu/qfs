//! The **pure evaluator** (blueprint §3 purity invariant, §7 effect-plan, ticket t07):
//! the pass that turns a parsed + resolved [`qfs_parser::Statement`] into a
//! [`qfs_plan::Plan`] (the effect-plan) and a logical query [`PlanSource`] — performing
//! **no I/O**.
//!
//! "A statement is a plan." Query stages fold left into a logical relation description
//! ([`PlanSource`]) whose output [`Schema`](qfs_types::Schema) is threaded stage by
//! stage (using the t05 schema algebra as the single source of truth); write operators
//! (`INSERT`/`UPSERT`/`UPDATE`/`REMOVE`) and `CALL driver.action(...)` **construct**
//! [`EffectNode`](qfs_plan::EffectNode)s and assemble them into a [`Plan`] DAG with
//! declared dependencies and the `irreversible` flag set. The single impure boundary —
//! `COMMIT : Plan -> World` — is t10's interpreter and is explicitly out of scope here.
//!
//! ## Purity invariant (the safety property, blueprint §3)
//! No function in this module takes or returns a `World`, an HTTP client, or a token; it
//! reads only the read-only [`MountRegistry`] (longest-prefix path router → driver) and
//! the driver's pure [`describe`](qfs_driver::Driver::describe) schema. Building and
//! previewing a plan therefore perform **no** network calls — this is what makes every
//! statement dry-runnable and golden-testable without credentials.
//!
//! ## Verb pipeline (t06 carry-over O2 + t09 mirror)
//! The AST-verb translation goes through the canonical
//! [`write_verb_for`](crate::write_verb_for) ∘ [`kind_for_verb`](qfs_plan::kind_for_verb)
//! pipeline — an **exhaustive match with no `_` arm** at each hop. `qfs_parser::EffectVerb`
//! must therefore stay **non-`#[non_exhaustive]`** (it is, per t04) so that adding a verb
//! breaks the cross-crate match and forces this evaluator and t06's resolver to be
//! updated rather than silently dropping the new verb. The [`WriteVerb`](qfs_plan::WriteVerb)
//! / `EffectVerb` mirror is intentional (the t06 Architect note): `qfs-plan` mirrors the
//! four verbs so it stays parser-free (acyclic spine), and the evaluator bridges the two.

use qfs_driver::{Driver, Path, Verb};
use qfs_parser::{
    CallRef, EffectBody, EffectStmt, EffectVerb, Expr, Literal, OfRef, OfTarget, PipeOp, Pipeline,
    PlanWrap, Projection, Source, Statement, SwitchArm, SwitchStage, Values,
};
use qfs_plan::{
    kind_for_verb, Affected, EffectKind, EffectNode, NodeId, Plan, PlanBuilder, ProcId, Target,
    VfsPath,
};
use qfs_types::{Column, ColumnType, DriverId, Fields, Name, Row, RowBatch, Schema, Value};

use crate::registry::MountRegistry;
use crate::resolve::{write_verb_for, ResolveError, Resolver};
use crate::stdlib::{BuiltinEval, FnError, StdlibRegistry};

/// A logical relation node — a **description** of how a query produces rows, never an
/// executed scan (blueprint §3 purity). The fold of a `FROM` source + `|>` pipe ops produces
/// a tree of these; each node carries its computed output [`Schema`] so the next stage
/// (and a `RETURNING`/write that consumes it) types against one source of truth.
///
/// t09 deferred the relational node enum to the evaluator (no `PlanSource` type exists in
/// `qfs-plan`); this is that owned, vendor-free description. It references paths and column
/// names only — never a driver SDK struct (blueprint §11).
#[derive(Debug, Clone, PartialEq)]
pub enum PlanSource {
    /// `/driver/...` — a logical scan of a path (a description, not a read).
    Scan {
        /// The driver this relation reads from.
        driver: DriverId,
        /// The virtual path scanned.
        path: VfsPath,
        /// The node's output schema (from the driver's pure `describe`).
        schema: Schema,
    },
    /// `FROM VALUES (..),(..)` — an inline literal relation.
    Values {
        /// The inferred schema of the literal rows.
        schema: Schema,
    },
    /// A `WHERE` filter over an input relation (schema-preserving).
    Filter {
        /// The filtered input.
        input: Box<PlanSource>,
    },
    /// A `SELECT`/projection narrowing the input columns.
    Project {
        /// The projected input.
        input: Box<PlanSource>,
        /// The projected output schema.
        schema: Schema,
    },
    /// An `EXTEND`/`SET` adding or overwriting columns.
    Extend {
        /// The extended input.
        input: Box<PlanSource>,
        /// The output schema after extension.
        schema: Schema,
    },
    /// `LIMIT`/`DISTINCT`/`ORDER BY`/`GROUP BY`/`AS` — a schema-preserving shaping op.
    Shape {
        /// The shaped input.
        input: Box<PlanSource>,
    },
    /// `EXPAND <field>` — explode a nested collection into rows (blueprint §4).
    Expand {
        /// The expanded input.
        input: Box<PlanSource>,
        /// The output schema after the field is exploded.
        schema: Schema,
    },
    /// `DECODE`/`ENCODE <fmt>` — a codec-registry seam node (a description only; the
    /// codec lookup is the registry's concern, the schema is late-bound).
    Codec {
        /// The decoded/encoded input.
        input: Box<PlanSource>,
        /// The codec format name.
        fmt: String,
    },
    /// `UNION`/`EXCEPT`/`INTERSECT` — a set operation over two relations; the output
    /// schema is the column-wise `unify` of the two sides (blueprint §4).
    SetOp {
        /// The left input.
        lhs: Box<PlanSource>,
        /// The right input.
        rhs: Box<PlanSource>,
        /// The unified output schema.
        schema: Schema,
    },
    /// `JOIN <source> ON <expr>` — the concatenation of both sides' columns.
    Join {
        /// The left input.
        lhs: Box<PlanSource>,
        /// The right input.
        rhs: Box<PlanSource>,
        /// The joined output schema.
        schema: Schema,
    },
    /// `|> TRANSFORM <name>` — the model-calling stage (blueprint §15, decision W). **Schema-
    /// transforming**: the relation becomes the definition's declared OUTPUT, so downstream
    /// `where`/`order by`/`select` type-check against it. (This is a schema-fold artifact; the row
    /// execution refuses in the engine until the execution ticket wires the applier.)
    Transform {
        /// The upstream relation.
        input: Box<PlanSource>,
        /// The definition's OUTPUT schema (the relation's schema after the stage).
        schema: Schema,
    },
}

impl PlanSource {
    /// The output schema of this relation node — the contract the next stage and any
    /// `RETURNING`/consuming write types against.
    #[must_use]
    pub fn schema(&self) -> &Schema {
        match self {
            PlanSource::Scan { schema, .. }
            | PlanSource::Values { schema }
            | PlanSource::Project { schema, .. }
            | PlanSource::Extend { schema, .. }
            | PlanSource::Expand { schema, .. }
            | PlanSource::SetOp { schema, .. }
            | PlanSource::Join { schema, .. }
            | PlanSource::Transform { schema, .. } => schema,
            PlanSource::Filter { input }
            | PlanSource::Shape { input }
            | PlanSource::Codec { input, .. } => input.schema(),
        }
    }
}

/// The value a statement evaluates to (blueprint §3): a query yields a logical relation; a
/// write/`CALL` yields a [`Plan`]. Owned, no vendor types.
#[derive(Debug, Clone, PartialEq)]
pub enum EvalValue {
    /// A pure read pipeline evaluated to its logical relation description.
    Relation(PlanSource),
    /// A write/effect statement evaluated to its effect-plan DAG.
    Plan(Plan),
}

impl EvalValue {
    /// The relation, if this value is a query result.
    #[must_use]
    pub fn as_relation(&self) -> Option<&PlanSource> {
        match self {
            EvalValue::Relation(r) => Some(r),
            EvalValue::Plan(_) => None,
        }
    }

    /// The plan, if this value is a write/effect result.
    #[must_use]
    pub fn as_plan(&self) -> Option<&Plan> {
        match self {
            EvalValue::Plan(p) => Some(p),
            EvalValue::Relation(_) => None,
        }
    }
}

/// The structured, AI-consumable evaluation error (blueprint §6). Resolution failures (unknown
/// driver/proc, capability denial, ambiguous alias) surface as [`EvalError::Resolve`];
/// schema/type failures (unknown column, ill-typed projection) surface as
/// [`EvalError::Type`]; an unrouted target path is its own arm. Credentials never appear.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum EvalError {
    /// Name resolution rejected the statement (delegated to the t06 resolver): unknown
    /// driver/procedure, capability denial, ambiguous/unprovided alias, arity/arg mismatch.
    Resolve(ResolveError),
    /// A schema/type rule rejected the statement (delegated to the t05 type model):
    /// unknown column, non-expandable field, incomparable types.
    Type(qfs_types::TypeError),
    /// A `fn(...)` registry-function call was ill-formed (delegated to the t08 stdlib):
    /// unknown function, wrong arity, or an aggregate used outside an `AGGREGATE` context.
    Fn(FnError),
    /// A `FROM` / effect-target path did not route to any registered driver mount, so no
    /// schema could be described for it. Carries the path for AI recovery.
    UnroutedPath {
        /// The path that failed to route to a mounted driver.
        path: String,
    },
    /// An effect-target path violated the host-realm path canon (decision P / owner ruling
    /// 2026-07-16): a non-local `/hosts/<h>/…` host, a `/hosts` with no host segment, a
    /// cross-realm service path, or a retired bare spelling of a host-realm-only mount. The
    /// inner error carries the canonical pointer for AI recovery.
    HostScope(crate::registry::HostScopeError),
    /// An `INSERT … VALUES` cell was not a constant the planner can lower to a literal effect
    /// payload (a function call, column reference, or other runtime expression). VALUES must be
    /// constants — use an `INSERT … FROM <query>` for computed rows. Carries a secret-free detail.
    NonLiteralValues {
        /// A machine-facing description of the offending expression form.
        detail: String,
    },
    /// A driver's own write lowering ([`Driver::plan_write`](qfs_driver::Driver::plan_write))
    /// rejected the statement (e.g. a malformed `INSERT INTO /git/<repo>/commits`). Carries the
    /// driver's secret-free reason.
    DriverWrite {
        /// A machine-facing description of why the driver could not lower the write.
        detail: String,
    },
    /// An `UPDATE`/`REMOVE`'s `WHERE` filter could not be carried to the applier as **complete**
    /// `col == const` equality keys (a non-equality comparison, an `OR`, a non-constant side, or
    /// contradictory duplicate bindings). Appliers resolve a filtered write from the selector
    /// channel alone, so silently dropping any part of the filter widens an irreversible write to
    /// rows/nodes the author never addressed (ticket 20260717102000: a partially-dropped REMOVE
    /// filter deleted a whole table / trashed a whole folder). Fail closed at plan time instead.
    WriteFilterUnsupported {
        /// The effect-target path, for AI recovery.
        path: String,
        /// A machine-facing description of the unsupported filter form.
        detail: String,
    },
    /// A `TRANSACTION { … }` block (M6, ticket t62, decision G) contained an **irreversible**
    /// effect. A transaction promises all-or-nothing rollback, so every effect inside MUST be
    /// reversible — an irreversible one (a `REMOVE`, an irreversible `CALL`) is rejected here, at
    /// plan time, *before* anything touches the world (the strongest safety posture; outside a
    /// transaction the same effect would merely need an extra ack). Carries the offending effect's
    /// secret-free label so the author (human or AI) can lift it OUT of the block.
    IrreversibleInTransaction {
        /// The offending effect's stable label (e.g. `REMOVE`, `CALL`).
        effect: String,
    },
    /// A lambda (M6, ticket t61) was applied with the wrong number of arguments — e.g. a
    /// `(x) => …` one-param lambda called with two arguments, or a `reduce` lambda that does
    /// not take `(acc, element)`. A *typed*, AI-consumable error, never a panic. Carries the
    /// declared parameter count and the count supplied.
    LambdaArity {
        /// The number of parameters the lambda declares.
        expected: usize,
        /// The number of arguments supplied at application.
        found: usize,
    },
    /// A value used in **function position** (the lambda argument of `map`/`filter`/`reduce`,
    /// or the callee of an application) was not a function/lambda (M6, ticket t61) — e.g.
    /// `map(coll, 3)`. Carries a secret-free description of the offending value's shape so the
    /// author can supply a lambda instead.
    NotAFunction {
        /// A machine-facing description of the non-function value's shape.
        detail: String,
    },
    /// A lambda parameter annotation used a retired or unknown type token. An unannotated
    /// parameter is the spelling for late-bound; annotations must use the canonical §5 type
    /// vocabulary plus the non-column `Resource` value.
    UnknownTypeAnnotation {
        /// The annotation token as written.
        name: String,
        /// Accepted canonical tokens / forms, for AI recovery.
        accepted: Vec<&'static str>,
    },
    /// A `TRANSFORM <name>` stage (blueprint §15, decision W) named a definition that is not
    /// installed in the plan-time registry (no such `CREATE TRANSFORM`, or the definitions were
    /// not wired). Carries the referenced definition name so an AI can create/rename it.
    TransformNotExecutable {
        /// The referenced (unresolved) transform definition name.
        name: String,
    },
    /// A `TRANSFORM <name>` stage whose declared INPUT names a column the incoming relation does
    /// not carry (surplus incoming columns are fine; a MISSING declared input is a plan-time
    /// error). Carries the definition and the missing column so the author can fix the pipeline.
    TransformInputMissing {
        /// The transform definition name.
        name: String,
        /// The declared INPUT column absent from the incoming relation.
        column: String,
    },
    /// A `SWITCH` stage appeared anywhere but the LAST op of a top-level query pipeline
    /// (blueprint §18): mid-pipe, in a subquery/JOIN source/set-op branch, in a `LET` binding,
    /// or inside an effect body. A switch routes rows to effect arms, so it is terminal-only.
    SwitchNotTerminal,
    /// A `FOLLOW <field>` stage reached the general query evaluator (blueprint §13). The follow
    /// is a declared-driver body stage — the declared evaluator splits it out and performs the
    /// second wire GET; in any other context it has no meaning.
    FollowOutsideDeclaredBody,
    /// A `SWITCH` arm list is ill-shaped (blueprint §18): no `else` arm, an `else` arm not
    /// written last, or a duplicate label. Carries a machine-facing detail naming the problem.
    SwitchShape {
        /// A machine-facing description of the arm-list problem.
        detail: String,
    },
    /// A `SWITCH` discriminant column is absent from the incoming relation (blueprint §18).
    /// Carries the referenced column and the available columns for AI recovery.
    SwitchDiscriminantUnknown {
        /// The discriminant column as written.
        column: String,
        /// The columns the incoming relation actually carries.
        available: Vec<String>,
    },
    /// A `SWITCH` arm whose continuation is not effect-terminal (blueprint §18 typing rule).
    /// This slice implements effect-routing switch only — every arm must end in an
    /// `INSERT`/`UPSERT INTO` write or an effect `CALL`; an all-pure switch (arms with a
    /// unifiable relation output) is deferred and recorded in §18.
    SwitchArmNotEffect {
        /// The offending arm's label (`else` for the default arm).
        label: String,
    },
    /// A `SWITCH` arm continuation used a pipe op outside the row-local vocabulary this slice
    /// routes (blueprint §18): a JOIN/set-op/EXPAND/codec/TRANSFORM/nested-SWITCH inside an arm.
    /// Carries the arm label and the op's stable name.
    SwitchArmOpUnsupported {
        /// The offending arm's label (`else` for the default arm).
        label: String,
        /// The unsupported op's stable keyword name.
        op: String,
    },
    /// A `|> of <name>` assertion (blueprint §5.6) named a declared type not present in the plan-time
    /// type-def registry — its `/type/<name>` catalog row is missing, or no System DB was wired. The
    /// twin of [`TransformNotExecutable`](EvalError::TransformNotExecutable) for the type catalog.
    OfTypeUnresolved {
        /// The referenced (unresolved) declared type name (canonical `/type/<name>`).
        name: String,
    },
    /// A `|> of <type>` assertion (blueprint §5.6) failed its plan-time STRUCTURAL check: the
    /// relation's computed schema does not match the asserted type. `of` never coerces — the author
    /// fixes the pipeline or the type. Carries the differing columns for AI recovery.
    OfAssertionFailed {
        /// The asserted type: a `/type/<name>` for a named assertion, or `(inline)` for a literal.
        ty: String,
        /// Asserted columns absent from the relation.
        missing: Vec<String>,
        /// Relation columns the asserted type does not declare.
        unexpected: Vec<String>,
        /// Common columns whose concrete types differ: `(column, expected_token, actual_token)`.
        /// A column left `unknown` on either side is conservatively skipped (the honest gap meter),
        /// so only statically-known concrete mismatches appear here.
        mismatched: Vec<(String, String, String)>,
    },
}

impl EvalError {
    /// A stable, machine-readable code an AI-facing caller branches on (blueprint §6).
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            EvalError::Resolve(e) => e.code(),
            EvalError::Type(e) => e.code(),
            EvalError::Fn(e) => e.code(),
            EvalError::UnroutedPath { .. } => "unrouted_path",
            EvalError::HostScope(e) => e.code(),
            EvalError::NonLiteralValues { .. } => "non_literal_values",
            EvalError::DriverWrite { .. } => "driver_write",
            EvalError::WriteFilterUnsupported { .. } => "write_filter_unsupported",
            EvalError::IrreversibleInTransaction { .. } => "irreversible_in_transaction",
            EvalError::LambdaArity { .. } => "lambda_arity",
            EvalError::NotAFunction { .. } => "not_a_function",
            EvalError::UnknownTypeAnnotation { .. } => "unknown_type_annotation",
            EvalError::TransformNotExecutable { .. } => "transform_not_executable",
            EvalError::TransformInputMissing { .. } => "transform_input_missing",
            EvalError::SwitchNotTerminal => "switch_not_terminal",
            EvalError::FollowOutsideDeclaredBody => "follow_outside_declared_body",
            EvalError::SwitchShape { .. } => "switch_shape",
            EvalError::SwitchDiscriminantUnknown { .. } => "switch_discriminant_unknown",
            EvalError::SwitchArmNotEffect { .. } => "switch_arm_not_effect",
            EvalError::SwitchArmOpUnsupported { .. } => "switch_arm_op_unsupported",
            EvalError::OfTypeUnresolved { .. } => "of_type_unresolved",
            EvalError::OfAssertionFailed { .. } => "of_assertion_failed",
        }
    }
}

impl From<ResolveError> for EvalError {
    fn from(e: ResolveError) -> Self {
        EvalError::Resolve(e)
    }
}

impl From<qfs_types::TypeError> for EvalError {
    fn from(e: qfs_types::TypeError) -> Self {
        EvalError::Type(e)
    }
}

impl From<FnError> for EvalError {
    fn from(e: FnError) -> Self {
        EvalError::Fn(e)
    }
}

/// The pure evaluator (blueprint §3, t07): a read-only pass over a parsed [`Statement`] that
/// folds the query side into a [`PlanSource`] and the write side into a [`Plan`]. Holds
/// the [`MountRegistry`] read-only (for `describe` schema + capability/procedure
/// resolution) and a [`Resolver`] for the t06 name/capability gate; performs no I/O.
pub struct Evaluator<'r> {
    mounts: &'r MountRegistry,
    resolver: Resolver<'r>,
    /// The function registry (t08), consulted to type `fn(...)` calls in expression
    /// position. `None` keeps t07's late-bound behaviour (a `fn(...)` projects an
    /// `Unknown` column and is not registry-checked); `Some` tightens it — the function's
    /// declared return type drives the projected column type and an unknown function /
    /// aggregate-in-`WHERE` becomes a structured [`EvalError::Fn`].
    stdlib: Option<&'r StdlibRegistry>,
}

impl<'r> Evaluator<'r> {
    /// Build an evaluator over a mount registry (no function registry — `fn(...)` calls
    /// stay late-bound, t07 behaviour).
    #[must_use]
    pub fn new(mounts: &'r MountRegistry) -> Self {
        Self {
            mounts,
            resolver: Resolver::new(mounts),
            stdlib: None,
        }
    }

    /// Build an evaluator wired to the function registry (t08): `fn(...)` calls are typed
    /// against the stdlib, so a projection over a built-in carries the built-in's declared
    /// return type and an unknown / mis-contexted function is a structured error.
    #[must_use]
    pub fn with_stdlib(mounts: &'r MountRegistry, stdlib: &'r StdlibRegistry) -> Self {
        Self {
            mounts,
            resolver: Resolver::new(mounts),
            stdlib: Some(stdlib),
        }
    }

    /// The declared return type of a `fn(name, ...)` call against the function registry,
    /// or a structured [`FnError`] (unknown function / wrong arity / aggregate misuse). The
    /// `under_aggregate` flag enforces the aggregate-vs-scalar dispatch rule (blueprint §3): an
    /// aggregate is legal only under `AGGREGATE`, a scalar is rejected where an aggregate
    /// is required. A prelude-alias name (receiver-scoped) types as `Unknown` here — its
    /// resolution is t06's receiver-typing concern, not the function registry's.
    ///
    /// # Errors
    /// [`FnError`] for an unknown function, an arity mismatch, or an aggregate/scalar used
    /// in the wrong context.
    pub fn type_of_fn(
        &self,
        name: &str,
        argc: usize,
        under_aggregate: bool,
    ) -> Result<ColumnType, FnError> {
        let Some(reg) = self.stdlib else {
            return Ok(ColumnType::Unknown);
        };
        let Some(builtin) = reg.builtin(name) else {
            // Not a core built-in. If a driver ships it as a prelude alias, it is a
            // receiver-typed callable (t06), late-bound here; otherwise it is unknown.
            if reg.alias_providers(name).is_empty() {
                return Err(FnError::UnknownFunction {
                    name: name.to_string(),
                });
            }
            return Ok(ColumnType::Unknown);
        };
        if !builtin.sig.accepts_arity(argc) {
            return Err(FnError::Arity {
                name: name.to_string(),
                expected: builtin.sig.min_args,
                found: argc,
            });
        }
        // Aggregate-vs-scalar dispatch (blueprint §3): a typed error, never a panic.
        let is_aggregate = matches!(builtin.eval, BuiltinEval::Aggregate(_));
        if is_aggregate && !under_aggregate {
            return Err(FnError::AggregateOutsideAggregate {
                name: name.to_string(),
            });
        }
        if !is_aggregate && under_aggregate && matches!(builtin.eval, BuiltinEval::Scalar(_)) {
            // A bare scalar directly under AGGREGATE (not wrapping/aggregating) is the
            // dual misuse — but scalars are legal *inside* an aggregate's argument, so we
            // only flag a top-level scalar projection. The caller passes `under_aggregate`
            // only for the projection head, so this stays correct.
            return Err(FnError::ScalarInAggregate {
                name: name.to_string(),
            });
        }
        Ok(builtin.sig.returns.clone())
    }

    /// Type-check a filter/predicate expression at **plan time** against `schema` (decision T,
    /// ticket t75). The static primitive type checker is the **stdlib-wired tightening** —
    /// mirroring t08's `fn(...)` return typing: without a function registry the expression stays
    /// late-bound (the t07 behaviour, so `Evaluator::new` is unchanged); with one wired, the plan
    /// pass enforces the primitive type lattice (comparisons, built-in argument types, lambda
    /// parameters/bodies) before any effect node is constructed. Pure — no I/O.
    ///
    /// # Errors
    /// [`EvalError::Type`] for an incomparable comparison / predicate operand, or
    /// [`EvalError::Fn`] for a call typed against a bad argument / aggregate context.
    fn typecheck_predicate(&self, expr: &Expr, schema: &Schema) -> Result<(), EvalError> {
        if let Some(stdlib) = self.stdlib {
            crate::typeck::check_expr(expr, &crate::typeck::TyEnv::new(), schema, Some(stdlib))?;
        }
        Ok(())
    }

    /// Evaluate a statement to its [`EvalValue`] (blueprint §3 entry point). Resolution (the
    /// t06 capability/procedure gate) runs **first**, so a denied verb or unknown
    /// procedure never reaches a plan; then the pure fold builds the relation/plan.
    ///
    /// # Errors
    /// [`EvalError`] for any unresolvable name, capability denial, ill-typed stage, or
    /// unrouted path.
    pub fn eval(&self, stmt: &Statement) -> Result<EvalValue, EvalError> {
        // Resolve-time gate first (blueprint §6): denied verbs / unknown procs / unbound names
        // fail before a plan exists.
        self.resolver.resolve_statement(stmt)?;
        let env = Env::default();
        let value = self.eval_inner(stmt, &env)?;

        // §15 (decision W): a statement carrying a `|> transform` stage ANYWHERE (mid-pipe,
        // subquery, JOIN source, set-op branch, LET binding/body) is EFFECT-BEARING — the model
        // call spends tokens and is non-deterministic — regardless of its terminal shape. Emit
        // one irreversible consent/audit node per stage so PREVIEW shows the spend (provider/
        // model/effort, from the definition metadata, never a secret) and COMMIT is gated. The
        // row payloads themselves flow exec-side, above the interpreter (`EffectOutput` stays
        // `{id, affected}`; the pure engine stays pure).
        let transforms = collect_transform_names(stmt);
        if transforms.is_empty() {
            return Ok(value);
        }
        // A TRANSACTION promises rollback; a model call cannot be undone. Reject at plan time,
        // exactly like any other irreversible effect inside a block (decision G).
        if matches!(terminal_of(stmt), Statement::Transaction { .. }) {
            return Err(EvalError::IrreversibleInTransaction {
                effect: "TRANSFORM".to_string(),
            });
        }
        let consent = self.transform_consent_plan(&transforms)?;
        match value {
            // An effect terminal (a transform feeding a write / an effect CALL): the consent
            // nodes sequence BEFORE the write, so the commit-point order is model → write.
            EvalValue::Plan(plan) => {
                let (consent, used) = relabel_plan(consent, 0);
                let (main, _) = relabel_plan(plan, used);
                Ok(EvalValue::Plan(consent.then(main)))
            }
            // A read terminal: a Read dependency marker for the upstream source feeds the
            // consent chain (the same shape a pipeline-sourced write plans with).
            EvalValue::Relation(rel) => {
                let read_plan = {
                    let mut b = PlanBuilder::new();
                    let read = EffectNode::new(b.next_id(), EffectKind::Read, source_target(&rel))
                        .with_affected(Affected::Unknown);
                    b.push(read);
                    b.build()
                };
                let (read_plan, used) = relabel_plan(read_plan, 0);
                let (consent, _) = relabel_plan(consent, used);
                Ok(EvalValue::Plan(read_plan.then(consent)))
            }
        }
    }

    /// Build the transform consent/audit sub-plan: one irreversible node per `|> transform`
    /// stage, in pipeline order, each chained after the previous (the commit-point order is the
    /// stage order). The node's `args` carry the SPEND-LEGIBILITY row — definition name,
    /// provider, model, effort, derived mode — resolved from the plan-time registry; a secret
    /// reference is never included (it resolves executor-side at COMMIT only).
    fn transform_consent_plan(&self, names: &[String]) -> Result<Plan, EvalError> {
        let mut builder = PlanBuilder::new();
        let mut prev: Option<NodeId> = None;
        for name in names {
            let def = self
                .mounts
                .transform_defs()
                .get(name)
                .ok_or_else(|| EvalError::TransformNotExecutable { name: name.clone() })?;
            let schema = Schema::new(vec![
                Column::new("transform", ColumnType::Text, false),
                Column::new("provider", ColumnType::Text, true),
                Column::new("model", ColumnType::Text, true),
                Column::new("effort", ColumnType::Text, true),
                Column::new("mode", ColumnType::Text, false),
            ]);
            let row = Row::new(vec![
                Value::Text(name.clone()),
                Value::Text(def.provider.clone()),
                Value::Text(def.model.clone()),
                def.effort.clone().map_or(Value::Null, Value::Text),
                Value::Text(def.mode.token().to_string()),
            ]);
            let id = builder.next_id();
            let node = EffectNode::new(
                id,
                EffectKind::Call(ProcId::new(format!("transform.{name}"))),
                Target::new(
                    DriverId::new("transform"),
                    VfsPath::new(format!("/transform/{name}")),
                ),
            )
            .with_args(RowBatch::new(schema, vec![row]))
            .with_affected(Affected::Unknown)
            .irreversible(true);
            builder.push(node);
            if let Some(p) = prev {
                builder.depends_on(id, p);
            }
            prev = Some(id);
        }
        Ok(builder.build())
    }

    fn eval_inner(&self, stmt: &Statement, env: &Env) -> Result<EvalValue, EvalError> {
        match stmt {
            Statement::Query(pipeline) => match pipeline.ops.last() {
                // A pipeline terminating in `|> call driver.proc(...)` to an EFFECT procedure is
                // itself an effect: it lowers to a `Call` effect plan so PREVIEW/COMMIT see the
                // effect and the irreversible gate has something to gate. WITHOUT this the
                // pipeline folds to a read relation and the call is silently dropped (the
                // driver-side `EffectKind::Call` apply path exists but is never reached). A CALL
                // to a result-returning procedure stays a read, and a CALL anywhere but the tail
                // keeps its read-through fold (`fold_op`) — the effect is only materialised when
                // an effect call is the pipeline's result. (See [`Self::eval_terminal_call`].)
                Some(PipeOp::Call(call)) => self.eval_terminal_call(pipeline, call, env),
                // A pipeline terminating in `|> switch <col> { … }` (blueprint §18) is an
                // effect: it lowers to the UNION of every arm's effect plan, so PREVIEW/COMMIT
                // see the full declared effect set before any model output routes a row.
                Some(PipeOp::Switch(stage)) => self.eval_terminal_switch(pipeline, stage, env),
                _ => Ok(EvalValue::Relation(self.fold_query(pipeline, env)?)),
            },
            Statement::Effect(effect) => Ok(EvalValue::Plan(self.eval_write(effect, env)?)),
            // PREVIEW/COMMIT are transparent to evaluation: the plan they wrap is the
            // inner statement's plan (the dry-run/apply distinction is t10's interpreter).
            Statement::Plan(PlanWrap { inner, .. }) => self.eval_inner(inner, env),
            // A `LET` binding (M6, t60): fold the bound relation once, bind it for the body's
            // scope (lexical, shadowing allowed — `with` returns a child env), then evaluate
            // the body. The binding flows in as a `PlanSource` (a selector/relation), never a
            // secret — so the purity floor holds: a `LET` introduces no I/O and no effect node.
            Statement::Let { name, value, body } => {
                let rel = self.eval_relation(value, env)?;
                let inner = env.with(name, rel);
                self.eval_inner(body, &inner)
            }
            // A `TRANSACTION { … }` block (M6, t62): lower the body into ONE plan in commit-point
            // (source) order, then enforce the reversible-only invariant. This is the parse/eval-
            // time gate — no I/O, fully dry-runnable; an irreversible effect inside is a hard error.
            Statement::Transaction { body, .. } => {
                Ok(EvalValue::Plan(self.eval_transaction(body, env)?))
            }
            // Server DDL desugars to `/server/...` effects in a later epic; here it
            // evaluates to an empty plan (no effect node to construct yet, ticket scope).
            Statement::Ddl(_) => Ok(EvalValue::Plan(Plan::pure())),
        }
    }

    /// Lower a `TRANSACTION { … }` block (M6, ticket t62, decision G) into ONE effect [`Plan`] and
    /// enforce the **reversible-only** invariant. Each body statement is an effect (grammar-
    /// enforced); its sub-plan is sequenced after the previous one with [`Plan::then`], so the
    /// nodes carry a deterministic **commit-point ordering** (the block's source order) that
    /// [`topo_order`](qfs_plan::topo_order) recovers for the all-or-nothing apply.
    ///
    /// The guard then walks every node: if any is inherently irreversible
    /// ([`EffectKind::is_inherently_irreversible`] — `REMOVE` always) OR carries the per-node
    /// [`EffectNode::irreversible`] flag (a driver/proc that declared a `CALL` irreversible at
    /// plan time), the whole block is rejected with [`EvalError::IrreversibleInTransaction`] and
    /// **zero** effects are applied — a transaction promises rollback, so it may hold no effect
    /// that cannot be undone. This is *stricter* than the outside-transaction
    /// [`IrreversibleGuard`](crate::IrreversibleGuard), which only requires an extra ack.
    ///
    /// # Errors
    /// [`EvalError`] for any unresolvable/ill-typed body effect, or
    /// [`EvalError::IrreversibleInTransaction`] if any effect inside is irreversible.
    fn eval_transaction(&self, body: &[Statement], env: &Env) -> Result<Plan, EvalError> {
        let mut plan = Plan::pure();
        // Each member plan is built by its own `PlanBuilder` starting node ids at 0, so the ids
        // would collide when combined. Shift every member into a fresh, contiguous id range before
        // sequencing so the assembled DAG has unique ids (the `validate`/topo invariant holds).
        let mut next_base: u32 = 0;
        for stmt in body {
            // Each member is an effect (the grammar admits nothing else); fold its plan in. A
            // non-plan value would be a grammar/invariant break, surfaced structurally not by panic.
            let member = match self.eval_inner(stmt, env)? {
                EvalValue::Plan(p) => p,
                EvalValue::Relation(_) => {
                    return Err(EvalError::NonLiteralValues {
                        detail: "a TRANSACTION body holds only effect statements".to_string(),
                    })
                }
            };
            let (member, used) = relabel_plan(member, next_base);
            next_base += used;
            // Sequence: `then` makes every later effect depend on the earlier ones, so the
            // commit-point order is exactly the block's source order (the topo walk is total).
            plan = plan.then(member);
        }
        // Reversible-only gate (decision G): fire on the inherent classification AND the per-node
        // flag, so a driver that marks a `CALL` irreversible at plan time is caught too.
        if let Some(node) = plan
            .nodes()
            .iter()
            .find(|n| n.irreversible || n.kind.is_inherently_irreversible())
        {
            return Err(EvalError::IrreversibleInTransaction {
                effect: node.kind.label().to_string(),
            });
        }
        Ok(plan)
    }

    /// Evaluate a `LET` binding's value to its relation [`PlanSource`]. The grammar restricts
    /// a binding value to a relation (a `Statement::Query` pipeline), so this always folds a
    /// query; the non-relation arm is unreachable in practice (an effect value fails to parse)
    /// and is mapped to a structured error rather than panicking (lib code stays panic-free).
    fn eval_relation(&self, stmt: &Statement, env: &Env) -> Result<PlanSource, EvalError> {
        match self.eval_inner(stmt, env)? {
            EvalValue::Relation(rel) => Ok(rel),
            EvalValue::Plan(_) => Err(EvalError::NonLiteralValues {
                detail: "a LET binds a relation, not an effect".to_string(),
            }),
        }
    }

    // ---- Query side: fold pipe stages into a logical relation ----

    /// Left-fold a read pipeline into a [`PlanSource`], threading the output schema
    /// through each `|>` stage (blueprint §2.2). The source schema comes from the driver's
    /// pure `describe`; each op transforms it via the t05 schema algebra.
    fn fold_query(&self, pipeline: &Pipeline, env: &Env) -> Result<PlanSource, EvalError> {
        let mut src = self.eval_source(&pipeline.source, env)?;
        for op in &pipeline.ops {
            src = self.fold_op(src, op, env)?;
        }
        Ok(src)
    }

    /// Evaluate a pipeline source into the base [`PlanSource`] (blueprint §2.2). A bare-identifier
    /// source (`FROM <name>`) resolves to its `LET`-bound relation in `env` (M6, t60) — the
    /// stored selector flows in, never re-reading a mount.
    fn eval_source(&self, source: &Source, env: &Env) -> Result<PlanSource, EvalError> {
        match source {
            // A `LET`-bound name substitutes its stored relation (t60). Resolution already
            // validated the name; a miss here is still a structured error, never a panic.
            Source::Name(name) => env.get(name).cloned().ok_or_else(|| {
                EvalError::Resolve(ResolveError::UnknownBinding { name: name.clone() })
            }),
            Source::Path(path) => {
                let full = render_path(
                    &path
                        .segments
                        .iter()
                        .map(|s| s.name.clone())
                        .collect::<Vec<_>>(),
                );
                let (driver, sub) = self
                    .mounts
                    .resolve_path(&full)
                    .ok_or_else(|| EvalError::UnroutedPath { path: full.clone() })?;
                let vfs = format!("/{}/{}", driver.id().as_str(), sub);
                let schema = describe_schema(driver.as_ref(), &vfs)?;
                Ok(PlanSource::Scan {
                    driver: driver.id(),
                    path: VfsPath::new(vfs),
                    schema,
                })
            }
            Source::Values(values) => Ok(PlanSource::Values {
                schema: values_schema(values),
            }),
            Source::Subquery(inner) => self.fold_query(inner, env),
        }
    }

    /// Fold one pipe op onto the current relation, computing its output schema (blueprint §3).
    fn fold_op(&self, input: PlanSource, op: &PipeOp, env: &Env) -> Result<PlanSource, EvalError> {
        match op {
            // Schema-preserving filter. The filter predicate is **type-checked at plan time**
            // against the input schema (decision T, ticket t75) when the function registry is
            // wired: a mismatched comparison (`WHERE total == 'paid'` over an `i64` column), a
            // built-in handed a bad argument type, or a lambda applied to the wrong element
            // type is a structured plan-time error here — before any I/O, so a type-failing
            // pipeline never reaches preview/commit.
            PipeOp::Where(predicate) => {
                self.typecheck_predicate(predicate, input.schema())?;
                Ok(PlanSource::Filter {
                    input: Box::new(input),
                })
            }
            // Projection narrows to the named columns (t05 `project` is the source of truth).
            // `SELECT` types `fn(...)` projections as scalars; `AGGREGATE` types them under
            // the aggregate-context rule (t08 dispatch).
            PipeOp::Select(projs) => {
                let schema = self.project_schema(input.schema(), projs, false)?;
                Ok(PlanSource::Project {
                    input: Box::new(input),
                    schema,
                })
            }
            PipeOp::Aggregate(projs) => {
                let schema = self.project_schema(input.schema(), projs, true)?;
                Ok(PlanSource::Project {
                    input: Box::new(input),
                    schema,
                })
            }
            // EXTEND/SET add or overwrite columns (Unknown-typed: pure expr typing is
            // late-bound here; the column env carries names so the next stage resolves).
            PipeOp::Extend(asgns) | PipeOp::Set(asgns) => {
                let mut schema = input.schema().clone();
                for a in asgns {
                    match schema.columns.iter_mut().find(|c| c.name == a.name) {
                        Some(col) => col.ty = ColumnType::Unknown,
                        None => schema.columns.push(Column::new(
                            a.name.clone(),
                            ColumnType::Unknown,
                            true,
                        )),
                    }
                }
                Ok(PlanSource::Extend {
                    input: Box::new(input),
                    schema,
                })
            }
            // EXPAND explodes a collection column (t05 `expand` is the source of truth).
            PipeOp::Expand(field) => {
                let name = field.last().cloned().unwrap_or_default();
                let schema = input.schema().expand(&name)?;
                Ok(PlanSource::Expand {
                    input: Box::new(input),
                    schema,
                })
            }
            PipeOp::Decode(codec) | PipeOp::Encode(codec) => Ok(PlanSource::Codec {
                input: Box::new(input),
                fmt: codec.fmt.clone(),
            }),
            // Set operations: unify the two sides' schemas column-wise (blueprint §4).
            PipeOp::Union(p) | PipeOp::Except(p) | PipeOp::Intersect(p) => {
                let rhs = self.fold_query(p, env)?;
                let schema = Schema::unify(input.schema(), rhs.schema())?;
                Ok(PlanSource::SetOp {
                    lhs: Box::new(input),
                    rhs: Box::new(rhs),
                    schema,
                })
            }
            PipeOp::Join(join) => {
                let rhs = self.eval_source(&join.source, env)?;
                let mut cols = input.schema().columns.clone();
                cols.extend(rhs.schema().columns.clone());
                let schema = Schema::new(cols);
                Ok(PlanSource::Join {
                    lhs: Box::new(input),
                    rhs: Box::new(rhs),
                    schema,
                })
            }
            // A `CALL` in a read pipeline is a procedure node; resolution already vetted
            // it, and the query relation is schema-preserving for the fold's purpose
            // (the call's effect, if any, is materialised on the write side).
            PipeOp::Call(_) => Ok(PlanSource::Shape {
                input: Box::new(input),
            }),
            // Schema-preserving shaping ops.
            PipeOp::Limit(_)
            | PipeOp::Distinct
            | PipeOp::OrderBy(_)
            | PipeOp::GroupBy(_)
            | PipeOp::As(_) => Ok(PlanSource::Shape {
                input: Box::new(input),
            }),
            // TRANSFORM (blueprint §15, decision W): schema-transforming. Resolve the definition,
            // check every declared INPUT column is present in the incoming relation (surplus
            // incoming columns are ignored; a missing declared input is a plan-time structured
            // error), and fold to the definition's OUTPUT schema so downstream stages type-check
            // against it. Row EXECUTION stays refused in the engine until the execution ticket.
            PipeOp::Transform(t) => {
                let def = self.mounts.transform_defs().get(&t.name).ok_or_else(|| {
                    EvalError::TransformNotExecutable {
                        name: t.name.clone(),
                    }
                })?;
                let incoming = input.schema();
                if let Some(missing) = def
                    .input
                    .columns
                    .iter()
                    .find(|c| incoming.column(&c.name).is_none())
                {
                    return Err(EvalError::TransformInputMissing {
                        name: t.name.clone(),
                        column: missing.name.clone(),
                    });
                }
                Ok(PlanSource::Transform {
                    input: Box::new(input),
                    schema: def.output.clone(),
                })
            }
            // SWITCH (blueprint §18) is terminal-only: it routes rows to effect arms, so it can
            // never fold as a read stage. Reaching it here means mid-pipe or a read-only context
            // (subquery, JOIN source, set-op branch, LET binding, effect body) — a structured
            // error, never a silent passthrough.
            PipeOp::Switch(_) => Err(EvalError::SwitchNotTerminal),
            // FOLLOW (blueprint §13) belongs to the declared-view evaluator (which splits it
            // out of the body before this evaluator ever runs) — reaching it here means a
            // general pipeline used it, where it has no meaning. Structured, never silent.
            PipeOp::Follow(_) => Err(EvalError::FollowOutsideDeclaredBody),
            // OF (blueprint §5.6): a general, any-position, plan-time type ASSERTION. Schema-identity
            // — it never coerces. It checks the relation's computed schema against the asserted type
            // and, on a structural mismatch, is a plan-time structured error naming the differing
            // columns. Where a named type carries a refinement, the structural half is checked here
            // and the predicate half rides to the next materialising boundary (§5.4's honest split):
            // a bare mid-pipe read assertion enforces structure only, and does not pretend a static
            // proof over rows that do not yet exist.
            PipeOp::Of(oref) => {
                self.check_of_assertion(oref, input.schema())?;
                Ok(PlanSource::Shape {
                    input: Box::new(input),
                })
            }
        }
    }

    /// Check a `|> of <type>` assertion (blueprint §5.6) against the relation's computed schema. The
    /// asserted type is either an inline structural literal (self-describing, no catalog) or a named
    /// declared type resolved from the plan-time type-def registry. STRUCTURAL only: column-name-set
    /// equality plus, for columns known on both sides, type equality — an `unknown` on either side is
    /// conservatively skipped (the honest gap meter). `of` never coerces; a mismatch names the
    /// differing columns.
    fn check_of_assertion(&self, oref: &OfRef, actual: &Schema) -> Result<(), EvalError> {
        let (ty_label, asserted) = match &oref.target {
            OfTarget::Inline(cols) => (
                "(inline)".to_string(),
                Schema::new(
                    cols.iter()
                        .map(|c| {
                            Column::new(
                                c.name.clone(),
                                ColumnType::parse(&c.ty).unwrap_or(ColumnType::Unknown),
                                c.nullable,
                            )
                        })
                        .collect(),
                ),
            ),
            OfTarget::Named(name) => {
                let def = self
                    .mounts
                    .declared_types()
                    .get(name)
                    .ok_or_else(|| EvalError::OfTypeUnresolved { name: name.clone() })?;
                (name.clone(), def.schema.clone())
            }
        };
        structural_diff(&ty_label, &asserted, actual)
    }

    // ---- Write side: construct effect-plan nodes ----

    /// Evaluate an effect statement (`INSERT`/`UPSERT`/`UPDATE`/`REMOVE`) into a [`Plan`]
    /// (blueprint §7). The verb goes through the canonical
    /// [`write_verb_for`](crate::write_verb_for) ∘ [`kind_for_verb`](qfs_plan::kind_for_verb)
    /// pipeline (no `_` arm). The effect node depends on any input relation (an
    /// `INSERT … FROM <query>` body), `REMOVE` is flagged inherently irreversible, and the
    /// optional `RETURNING` projection schema is attached.
    fn eval_write(&self, effect: &EffectStmt, env: &Env) -> Result<Plan, EvalError> {
        // Plan-time type check of an `UPDATE … SET … WHERE` / `REMOVE … WHERE` filter (decision T,
        // ticket t75): the filter predicate is checked against the target's described schema before
        // any effect node is built, so a mismatched key comparison fails at plan time, never at
        // commit. Skipped for an unrouted target (no schema to check against — late-bound).
        if self.stdlib.is_some() {
            if let EffectBody::SetWhere {
                filter: Some(filter),
                ..
            } = &effect.body
            {
                if let Ok(schema) = self.effect_input_schema(effect, env) {
                    self.typecheck_predicate(filter, &schema)?;
                }
            }
        }

        let full = render_path(
            &effect
                .target
                .segments
                .iter()
                .map(|s| s.name.clone())
                .collect::<Vec<_>>(),
        );
        // The host-realm path canon (decision P / owner ruling 2026-07-16): peel a
        // `/hosts/local/<svc>/…` target to its service path so the effect node's canonical VFS
        // path speaks the mount's own namespace; a non-local host and the retired bare spelling
        // of a host-realm-only mount both fail closed here with the structured pointer.
        let full = self
            .mounts
            .canonicalize_host_path(&full)
            .map_err(EvalError::HostScope)?;
        let routed = self.mounts.resolve_path(&full);
        let (driver, vfs) = match &routed {
            Some((driver, sub)) => (driver.id(), format!("/{}/{}", driver.id().as_str(), sub)),
            // An unrouted target is a path/mount concern; we still build a plan against
            // the literal path so the verb/irreversible semantics are testable without a
            // mount (deferred path resolution, ticket scope).
            None => (DriverId::new(first_segment(&full)), full.clone()),
        };
        let target = Target::new(driver, VfsPath::new(vfs));

        // The canonical verb pipeline (t06 O2 → t09 mirror): EffectVerb → WriteVerb →
        // EffectKind, each an exhaustive no-`_`-arm match.
        let kind = kind_for_verb(write_verb_for(effect.verb));

        let mut builder = PlanBuilder::new();

        // If the body is a sub-pipeline, evaluate it to a relation first and emit a Read
        // dependency the write hangs off (the `INSERT … FROM <query>` case).
        let dep: Option<NodeId> = match &effect.body {
            EffectBody::Pipeline(p) => {
                let rel = self.fold_query(p, env)?;
                let read =
                    EffectNode::new(builder.next_id(), EffectKind::Read, source_target(&rel))
                        .with_affected(Affected::Unknown);
                Some(builder.push(read))
            }
            EffectBody::Values(_) | EffectBody::SetWhere { .. } => None,
        };

        // The literal `VALUES` row payload the effect WRITES — lowered here into the effect node's
        // `args` (a `RowBatch`) so the applier actually receives the rows. Column names come from
        // the explicit `VALUES (a,b)` list when present, else the target node's described columns
        // in order (the applier maps the row's named columns onto the backend columns). WITHOUT
        // this, an `INSERT … VALUES` reaches the applier with an empty payload — a silent no-op.
        let values_args: Option<RowBatch> = match &effect.body {
            EffectBody::Values(v) => Some(self.values_row_batch(effect, v, env)?),
            EffectBody::SetWhere { set, .. } => Some(setwhere_row_batch(set)?),
            EffectBody::Pipeline(_) => None,
        };

        // The `WHERE`-selector (blueprint §7): *which* existing rows/nodes the effect addresses,
        // carried on its OWN channel — the ONE place a filter lives. `args` is now purely the
        // SET/VALUES payload (what to WRITE); the selector is purely the match (what to write it
        // TO). Keeping both in one flat batch was the retired two-convention state: it had to de-dup
        // a `WHERE` key that shared a `SET` column, which silently DROPPED the selector on a
        // same-column `SET name='X' WHERE name='Y'` (the gdrive folder-rename bug). Built from the
        // conjoined `col == const` `WHERE` leaves; `None` when the statement carries no `WHERE`.
        let selector_args: Option<RowBatch> = match &effect.body {
            EffectBody::SetWhere {
                filter: Some(f), ..
            } => Some(where_selector_batch(f, &full)?),
            _ => None,
        };

        // **Driver-specific write lowering** (blueprint §7): if the routed driver supplies its own encoded
        // effect plan for this `(path, verb, row)` write — the git case, whose applier consumes a
        // `blob→tree→commit→ref→reflog` plan the generic single node cannot express — use it. Only
        // for VALUES/SET writes (a `FROM`-pipeline write has no literal row to hand the driver); a
        // driver that declines returns `None` and the generic node below is used (sql/slack/github/…).
        if let (Some((driver, _)), Some(args)) = (routed.as_ref(), values_args.as_ref()) {
            if let Some(result) = driver.plan_write(
                &Path::new(full.clone()),
                driver_verb(effect.verb),
                args,
                selector_args.as_ref(),
            ) {
                return result.map_err(|e| EvalError::DriverWrite {
                    detail: format!("{e:?}"),
                });
            }
        }

        // The write node itself. `REMOVE` is inherently irreversible (set in `new`). The row
        // payload is attached FIRST, then the honest affected estimate LAST: `INSERT … VALUES` is
        // `Exact(n)`, while a filter-driven `UPDATE`/`REMOVE` stays `Unknown` (the synthetic
        // key+set row is NOT the affected-row count) — so `with_affected` must win over the
        // `with_args` row-count refinement.
        let write_id = builder.next_id();
        let mut write = EffectNode::new(write_id, kind, target);
        if let Some(args) = values_args {
            write = write.with_args(args);
        }
        if let Some(selector) = selector_args {
            write = write.with_selector(selector);
        }
        write = write.with_affected(write_affected(effect));
        // A routed driver may declare this specific `(path, verb)` write irreversible — e.g. a
        // declared MAP marked `IRREVERSIBLE` onto an external POST. `EffectNode::irreversible`
        // OR-combines, so this never clears the inherent `REMOVE` flag; it only ADDS the declared
        // bit, so the plan's irreversible gate (PREVIEW surfacing + `--commit-irreversible`) fires
        // exactly as it does for a `REMOVE` or an irreversible `CALL`.
        if let Some((driver, _)) = routed.as_ref() {
            write = write.irreversible(
                driver.write_irreversible(&Path::new(full.clone()), driver_verb(effect.verb)),
            );
        }
        builder.push(write);
        if let Some(parent) = dep {
            builder.depends_on(write_id, parent);
        }

        let mut plan = builder.build();

        // The RETURNING projection schema, computed against the effect's input schema.
        if let Some(returning) = &effect.returning {
            let input_schema = self.effect_input_schema(effect, env)?;
            let schema = self.project_schema(&input_schema, returning, false)?;
            plan = plan.returning(schema);
        }

        Ok(plan)
    }

    /// Evaluate a pipeline whose terminal op is `|> call driver.proc(args)`. Resolution (already
    /// run at the gate) classifies the procedure:
    ///
    /// - a **result-returning** procedure (declares a result schema) makes the terminal CALL a
    ///   READ — the whole pipeline folds to a relation (the CALL is a schema-preserving
    ///   read-through) whose rows the agent consumes;
    /// - an **effect** procedure lowers to a single [`EffectKind::Call`] [`Plan`] node whose
    ///   [`Target`] is the source pipeline's base path — the applier resolves the acted-on entity
    ///   from that path live (as a `REMOVE … WHERE` does), so no read dependency is emitted. The
    ///   node's `args` carry the call's arguments as one row (each column named by its `name =>`
    ///   label, or by the declared parameter for a positional arg) for the driver's `decode_call`,
    ///   and the per-procedure irreversible flag rides on the node (so `mail.send` is gated while
    ///   `drive.copy` is not).
    fn eval_terminal_call(
        &self,
        pipeline: &Pipeline,
        call: &CallRef,
        env: &Env,
    ) -> Result<EvalValue, EvalError> {
        let lowering = self.resolver.resolve_call_lowering(call)?;
        if lowering.returns_rows {
            // A row-returning proc's CALL is a read: fold the whole pipeline as a relation.
            return Ok(EvalValue::Relation(self.fold_query(pipeline, env)?));
        }

        // Fold the source pipeline WITHOUT the terminal call to recover the target it acts on
        // (the base scan's driver + path). The call's target is that source path, which the
        // applier re-resolves to the acted-on entity.
        let source = Pipeline {
            source: pipeline.source.clone(),
            ops: pipeline.ops[..pipeline.ops.len() - 1].to_vec(),
        };
        let rel = self.fold_query(&source, env)?;
        let target = source_target(&rel);
        let args = self.call_row_batch(call, &lowering.params)?;

        // **Plan-time CALL validation** (blueprint §3): the routed driver may reject a CALL that
        // cannot resolve a concrete entity — here, at plan time — so `PREVIEW` and `COMMIT` agree.
        // `PREVIEW` never decodes the effect, so an apply-time-only refusal would let a preview
        // claim an effect the commit then rejects (e.g. `mail.send` needs an addressed draft or
        // `to` recipients, never a byteless create-then-send). A driver that declines returns `None`.
        if let Some((driver, sub)) = self.mounts.resolve_path(target.path.as_str()) {
            let vfs = format!("/{}/{}", driver.id().as_str(), sub);
            if let Some(result) =
                driver.plan_call(&Path::new(vfs), &lowering.resolved.qualified, &args)
            {
                result.map_err(|e| EvalError::DriverWrite {
                    detail: format!("{e:?}"),
                })?;
            }
        }

        let mut builder = PlanBuilder::new();
        // A lone `Call` node (no read dependency), mirroring a filter-driven `REMOVE`. The row
        // count a call touches is not a relation row count, so the estimate stays `Unknown`.
        let node = EffectNode::new(
            builder.next_id(),
            EffectKind::Call(ProcId::new(&lowering.resolved.qualified)),
            target,
        )
        .with_args(args)
        .with_affected(Affected::Unknown)
        .irreversible(lowering.resolved.irreversible);
        builder.push(node);
        Ok(EvalValue::Plan(builder.build()))
    }

    /// Evaluate a pipeline whose terminal op is `|> switch <col> { … }` (blueprint §18): validate
    /// the arm-list shape and the discriminant column, lower EVERY arm to its effect plan over the
    /// routed partition, and sequence the arms in declaration order. PREVIEW is model-free — the
    /// taken arm is unknowable — so the statement's declared effect set is the **union** of every
    /// arm's effects (§18-C). Row routing happens at the exec commit boundary: the source
    /// materializes once (the model having run once), rows partition by the discriminant, and an
    /// arm with an empty partition is pruned — previewed-but-not-fired, spending nothing.
    fn eval_terminal_switch(
        &self,
        pipeline: &Pipeline,
        stage: &SwitchStage,
        env: &Env,
    ) -> Result<EvalValue, EvalError> {
        validate_switch_shape(stage)?;
        // Fold the source (the pipeline minus the switch) once; the discriminant must be one of
        // its columns. A late-bound (empty) fold schema cannot refute the discriminant — the
        // check binds only against concrete columns (the same tolerance the transform INPUT
        // check applies below an EXPAND).
        let source = Pipeline {
            source: pipeline.source.clone(),
            ops: pipeline.ops[..pipeline.ops.len() - 1].to_vec(),
        };
        let rel = self.fold_query(&source, env)?;
        let schema = rel.schema();
        if !schema.columns.is_empty() && schema.column(&stage.discriminant).is_none() {
            return Err(EvalError::SwitchDiscriminantUnknown {
                column: stage.discriminant.clone(),
                available: schema
                    .column_names()
                    .iter()
                    .map(ToString::to_string)
                    .collect(),
            });
        }
        // Lower each arm and sequence in declaration order (§18-C: the commit-point order is
        // the declaration order). Each arm's PlanBuilder restarts ids at 0, so the members are
        // disjointified exactly like TRANSACTION members.
        let mut union: Option<Plan> = None;
        let mut base = 0u32;
        for arm in &stage.arms {
            let label = arm.label.as_deref().unwrap_or("else");
            let plan = self.switch_arm_plan(&source, arm, label, env)?;
            let (plan, count) = relabel_plan(plan, base);
            base += count;
            union = Some(match union {
                None => plan,
                Some(u) => u.then(plan),
            });
        }
        match union {
            Some(plan) => Ok(EvalValue::Plan(plan)),
            // Unreachable through the grammar (an arm list is non-empty); stay total.
            None => Err(EvalError::SwitchShape {
                detail: "a switch needs at least one arm".to_string(),
            }),
        }
    }

    /// Lower one switch arm to its effect plan over the routed partition (blueprint §18). The
    /// arm's continuation ops are restricted to the row-local vocabulary this slice routes at the
    /// commit boundary; the terminal is an `INSERT`/`UPSERT INTO` write (the `INSERT … FROM
    /// <query>` plan shape) or an effect `CALL` (the terminal-call plan shape) — an all-pure arm
    /// is deferred (§18 records it).
    fn switch_arm_plan(
        &self,
        source: &Pipeline,
        arm: &SwitchArm,
        label: &str,
        env: &Env,
    ) -> Result<Plan, EvalError> {
        for (i, op) in arm.ops.iter().enumerate() {
            let terminal = i + 1 == arm.ops.len() && arm.write.is_none();
            let unsupported = match op {
                PipeOp::Join(_) => Some("join"),
                PipeOp::Union(_) => Some("union"),
                PipeOp::Except(_) => Some("except"),
                PipeOp::Intersect(_) => Some("intersect"),
                PipeOp::Expand(_) => Some("expand"),
                PipeOp::Decode(_) => Some("decode"),
                PipeOp::Encode(_) => Some("encode"),
                PipeOp::Transform(_) => Some("transform"),
                PipeOp::Switch(_) => Some("switch"),
                PipeOp::Follow(_) => Some("follow"),
                PipeOp::Call(_) if !terminal => Some("call (non-terminal)"),
                _ => None,
            };
            if let Some(op) = unsupported {
                return Err(EvalError::SwitchArmOpUnsupported {
                    label: label.to_string(),
                    op: op.to_string(),
                });
            }
        }
        // The arm's full pipeline: the shared source followed by the arm's continuation. At the
        // commit boundary the source materializes ONCE and each arm's continuation re-evaluates
        // over its routed partition only — this synthetic pipeline is the plan/typing view.
        let arm_pipeline = || Pipeline {
            source: source.source.clone(),
            ops: source.ops.iter().chain(&arm.ops).cloned().collect(),
        };
        match (&arm.write, arm.ops.last()) {
            // `… => <ops> |> INSERT/UPSERT INTO <path>`: the routed+piped partition is the
            // written relation — exactly the `INSERT … FROM <query>` plan shape, so the write
            // capability gate, driver irreversibility, and RETURNING all apply unchanged.
            (Some(w), _) => {
                let effect = EffectStmt {
                    verb: w.verb,
                    target: w.target.clone(),
                    body: EffectBody::Pipeline(Box::new(arm_pipeline())),
                    returning: w.returning.clone(),
                };
                self.eval_write(&effect, env)
            }
            // `… => <ops> |> CALL driver.proc(…)`: the terminal effect CALL, exactly the
            // top-level terminal-call plan shape (args, plan-time driver validation, the
            // per-procedure irreversible flag).
            (None, Some(PipeOp::Call(call))) => {
                match self.eval_terminal_call(&arm_pipeline(), call, env)? {
                    EvalValue::Plan(plan) => Ok(plan),
                    // A row-returning procedure keeps the arm pure — not an effect arm.
                    EvalValue::Relation(_) => Err(EvalError::SwitchArmNotEffect {
                        label: label.to_string(),
                    }),
                }
            }
            // A pure arm (no write, no terminal effect CALL): deferred this slice (§18).
            (None, _) => Err(EvalError::SwitchArmNotEffect {
                label: label.to_string(),
            }),
        }
    }

    /// Lower a `CALL`'s arguments into the effect node's row payload: one row whose columns are
    /// the call arguments, each named by its `name =>` label or — for a positional arg — by the
    /// declared parameter at that position (arity was validated at the resolve gate). The driver's
    /// `decode_call` reads these columns by name. A non-constant argument is a structured error
    /// (a `CALL` takes literal arguments), never a silently-dropped value.
    fn call_row_batch(&self, call: &CallRef, params: &[String]) -> Result<RowBatch, EvalError> {
        let mut columns = Vec::with_capacity(call.args.len());
        let mut cells = Vec::with_capacity(call.args.len());
        for (i, arg) in call.args.iter().enumerate() {
            let name = match &arg.name {
                Some(name) => name.clone(),
                // A positional arg names the declared parameter at its position. The resolve gate
                // bounded the arg count by the param count, so a miss here would be an invariant
                // break — surfaced structurally, never a panic.
                None => params
                    .get(i)
                    .cloned()
                    .ok_or_else(|| EvalError::NonLiteralValues {
                        detail: format!(
                            "CALL {}.{} positional argument {i} has no declared parameter",
                            call.driver, call.action
                        ),
                    })?,
            };
            columns.push(Column::new(name, ColumnType::Unknown, true));
            cells.push(literal_value(&arg.value)?);
        }
        let schema = Schema::new(columns);
        Ok(RowBatch::new(schema, vec![Row::new(cells)]))
    }

    /// Lower an `INSERT … VALUES` body into the effect's row payload [`RowBatch`]. Each cell is a
    /// constant evaluated to a [`Value`]; the column names come from the explicit `VALUES (a,b)`
    /// list when present, else the target node's described columns (truncated to the row width) so
    /// the applier maps each named cell onto the right backend column. A non-constant cell is a
    /// structured [`EvalError::NonLiteralValues`] (use `INSERT … FROM <query>` for computed rows),
    /// never a silently-dropped value.
    fn values_row_batch(
        &self,
        effect: &EffectStmt,
        values: &Values,
        env: &Env,
    ) -> Result<RowBatch, EvalError> {
        let width = values.rows.first().map_or(0, Vec::len);
        // Column names: explicit list, else the target's described columns in order.
        let columns: Vec<Column> = match &values.columns {
            Some(cols) => cols
                .iter()
                .map(|name| Column::new(name.clone(), ColumnType::Unknown, true))
                .collect(),
            None => {
                // Prefer the target's described columns (so a routed table names each cell for the
                // applier); fall back to positional `col{i}` when the target is unrouted or narrower
                // than the row — so a PREVIEW works with no mount (mirroring `values_schema`), never
                // erroring just because the row payload is being built.
                let described = self
                    .effect_input_schema(effect, env)
                    .map(|s| s.columns)
                    .unwrap_or_default();
                (0..width)
                    .map(|i| {
                        described.get(i).cloned().unwrap_or_else(|| {
                            Column::new(format!("col{i}"), ColumnType::Unknown, true)
                        })
                    })
                    .collect()
            }
        };
        let schema = Schema::new(columns);
        let mut rows = Vec::with_capacity(values.rows.len());
        for row in &values.rows {
            let mut cells = Vec::with_capacity(row.len());
            for expr in row {
                cells.push(literal_value(expr)?);
            }
            rows.push(Row::new(cells));
        }
        Ok(RowBatch::new(schema, rows))
    }

    /// The schema the effect reads/writes against — the sub-pipeline's output schema for a
    /// `FROM`-bodied effect, otherwise the target node's described schema. Used to type a
    /// `RETURNING` projection.
    fn effect_input_schema(&self, effect: &EffectStmt, env: &Env) -> Result<Schema, EvalError> {
        if let EffectBody::Pipeline(p) = &effect.body {
            return Ok(self.fold_query(p, env)?.schema().clone());
        }
        let full = render_path(
            &effect
                .target
                .segments
                .iter()
                .map(|s| s.name.clone())
                .collect::<Vec<_>>(),
        );
        match self.mounts.resolve_path(&full) {
            Some((driver, sub)) => {
                let vfs = format!("/{}/{}", driver.id().as_str(), sub);
                describe_schema(driver.as_ref(), &vfs)
            }
            None => Err(EvalError::UnroutedPath { path: full }),
        }
    }

    /// Project a schema by a list of [`Projection`]s (blueprint §4 `SELECT`/`AGGREGATE`). `*`
    /// keeps the input schema; a bare `col`/`col AS a` resolves the real column type (t05
    /// `project`); a `fn(...)` expression is **typed against the function registry** (t08):
    /// the built-in's declared return type becomes the projected column type, and an
    /// unknown function / aggregate-misuse is a structured [`EvalError::Fn`]. Without a
    /// wired registry, a computed expression stays late-bound (`Unknown`, t07 behaviour).
    ///
    /// `under_aggregate` is `true` for an `AGGREGATE` projection (so a top-level aggregate
    /// `fn` is legal there and rejected in a `SELECT`).
    fn project_schema(
        &self,
        input: &Schema,
        projs: &[Projection],
        under_aggregate: bool,
    ) -> Result<Schema, EvalError> {
        // A bare `*` (alone) preserves the whole schema.
        if projs.iter().any(|p| matches!(p, Projection::Star)) && projs.len() == 1 {
            return Ok(input.clone());
        }
        let mut out = Vec::with_capacity(projs.len());
        for p in projs {
            match p {
                Projection::Star => out.extend(input.columns.clone()),
                Projection::Expr { expr, alias } => match (alias, expr) {
                    // `col` / `col AS a` → resolve the real column type (source of truth).
                    (alias, qfs_parser::Expr::Col(name)) => {
                        let col = input.project(std::slice::from_ref(name))?;
                        let mut c = col.columns.into_iter().next().unwrap_or_else(|| {
                            Column::new(name.clone(), ColumnType::Unknown, true)
                        });
                        if let Some(a) = alias {
                            c.name = a.clone();
                        }
                        out.push(c);
                    }
                    // A `fn(...)` projection → type it against the function registry (t08).
                    // The built-in's return type drives the column; an unknown/mis-contexted
                    // function is a structured error (not a silent `Unknown`).
                    (alias, qfs_parser::Expr::Fn(fnref)) => {
                        let ty = self.type_of_fn(&fnref.name, fnref.args.len(), under_aggregate)?;
                        let name = alias
                            .clone()
                            .unwrap_or_else(|| format!("expr{}", out.len()));
                        out.push(Column::new(name, ty, true));
                    }
                    // Any other computed/aliased expression → an Unknown-typed column under
                    // its alias (or a synthesised name); pure expr typing stays late-bound.
                    (Some(a), _) => out.push(Column::new(a.clone(), ColumnType::Unknown, true)),
                    (None, _) => {
                        out.push(Column::new(
                            format!("expr{}", out.len()),
                            ColumnType::Unknown,
                            true,
                        ));
                    }
                },
            }
        }
        Ok(Schema::new(out))
    }
}

/// The lexical binding environment for `LET` (M6, ticket t60): a bound name → its evaluated
/// relation [`PlanSource`]. A bare-identifier source (`FROM <name>`) substitutes the stored
/// relation rather than re-reading a mount, so a `LET`-bound relation is folded **once** and
/// reused. Cheap to extend by value ([`Env::with`]) so each `LET` body gets its own immutable
/// child env — shadowing is a plain re-insert and the parent env is never mutated. A bound
/// value is a relation description (a selector), never a secret, so the purity floor holds.
#[derive(Debug, Clone, Default)]
struct Env {
    bindings: std::collections::HashMap<String, PlanSource>,
}

impl Env {
    /// A child env with `name` bound to `rel` (shadowing any same-named outer binding).
    fn with(&self, name: &str, rel: PlanSource) -> Self {
        let mut bindings = self.bindings.clone();
        bindings.insert(name.to_string(), rel);
        Self { bindings }
    }

    /// The relation bound to `name`, if it is in scope.
    fn get(&self, name: &str) -> Option<&PlanSource> {
        self.bindings.get(name)
    }
}

// ---- Free helpers (pure) ----

/// The `Target` of a relation source (its scanned driver/path), or a synthetic empty
/// target for a non-scan relation (e.g. `VALUES`) — used to anchor a `Read` dependency.
fn source_target(rel: &PlanSource) -> Target {
    match rel {
        PlanSource::Scan { driver, path, .. } => Target::new(driver.clone(), path.clone()),
        PlanSource::Filter { input }
        | PlanSource::Project { input, .. }
        | PlanSource::Extend { input, .. }
        | PlanSource::Shape { input }
        | PlanSource::Expand { input, .. }
        | PlanSource::Codec { input, .. }
        | PlanSource::Transform { input, .. } => source_target(input),
        PlanSource::SetOp { lhs, .. } | PlanSource::Join { lhs, .. } => source_target(lhs),
        PlanSource::Values { .. } => Target::new(DriverId::new(""), VfsPath::new("")),
    }
}

/// Shift every [`NodeId`] in `plan` up by `base`, returning the relabelled plan and the number of
/// ids it consumed (its node count) so the next member can continue the contiguous range. Used to
/// combine independently-built member plans inside a `TRANSACTION` block (each member's
/// [`PlanBuilder`] restarts ids at 0, so they must be disjointified before sequencing).
fn relabel_plan(mut plan: Plan, base: u32) -> (Plan, u32) {
    let count = plan.nodes.len() as u32;
    for node in &mut plan.nodes {
        node.id = NodeId(node.id.index() + base);
    }
    for (parent, child) in &mut plan.deps {
        *parent = NodeId(parent.index() + base);
        *child = NodeId(child.index() + base);
    }
    (plan, count)
}

/// The terminal statement a program leads into, descending through `LET` bindings and
/// `PREVIEW`/`COMMIT` wrappers (the same walk `qfs-exec` routes on).
fn terminal_of(stmt: &Statement) -> &Statement {
    match stmt {
        Statement::Let { body, .. } => terminal_of(body),
        Statement::Plan(PlanWrap { inner, .. }) => terminal_of(inner),
        other => other,
    }
}

/// Collect every `|> transform <name>` stage in a statement, in evaluation order — the
/// WHOLE-TREE walk (blueprint §15): mid-pipe, subquery source, `JOIN` source, set-op branch,
/// `LET` binding and body, and an effect body pipeline all count.
fn collect_transform_names(stmt: &Statement) -> Vec<String> {
    let mut names = Vec::new();
    collect_stmt_transforms(stmt, &mut names);
    names
}

fn collect_stmt_transforms(stmt: &Statement, out: &mut Vec<String>) {
    match stmt {
        Statement::Query(p) => collect_pipeline_transforms(p, out),
        Statement::Effect(e) => {
            if let EffectBody::Pipeline(p) = &e.body {
                collect_pipeline_transforms(p, out);
            }
        }
        Statement::Plan(PlanWrap { inner, .. }) => collect_stmt_transforms(inner, out),
        Statement::Let { value, body, .. } => {
            collect_stmt_transforms(value, out);
            collect_stmt_transforms(body, out);
        }
        Statement::Transaction { body, .. } => {
            for s in body {
                collect_stmt_transforms(s, out);
            }
        }
        Statement::Ddl(_) => {}
    }
}

fn collect_pipeline_transforms(p: &Pipeline, out: &mut Vec<String>) {
    collect_source_transforms(&p.source, out);
    for op in &p.ops {
        collect_op_transforms(op, out);
    }
}

fn collect_op_transforms(op: &PipeOp, out: &mut Vec<String>) {
    match op {
        PipeOp::Transform(t) => out.push(t.name.clone()),
        PipeOp::Join(j) => collect_source_transforms(&j.source, out),
        PipeOp::Union(sub) | PipeOp::Except(sub) | PipeOp::Intersect(sub) => {
            collect_pipeline_transforms(sub, out);
        }
        // A switch arm's continuation counts too (blueprint §18; arms reject transforms this
        // slice, but the walk stays whole-tree so a future relaxation cannot silently skip
        // the consent node).
        PipeOp::Switch(s) => {
            for arm in &s.arms {
                for op in &arm.ops {
                    collect_op_transforms(op, out);
                }
            }
        }
        _ => {}
    }
}

fn collect_source_transforms(source: &Source, out: &mut Vec<String>) {
    if let Source::Subquery(sub) = source {
        collect_pipeline_transforms(sub, out);
    }
}

/// Validate a switch arm list's shape (blueprint §18): exactly one `else` arm, written last,
/// and no duplicate labels. The open-discriminant exhaustiveness rule makes `else` mandatory
/// this slice — label-coverage over a closed refined enum (owner call 3's other half) awaits
/// refinement-carrying schemas and is recorded as deferred in §18.
fn validate_switch_shape(stage: &SwitchStage) -> Result<(), EvalError> {
    let else_count = stage.arms.iter().filter(|a| a.label.is_none()).count();
    if else_count == 0 {
        return Err(EvalError::SwitchShape {
            detail: "a switch requires a trailing `else` arm (open-discriminant \
                     exhaustiveness, blueprint §18-C)"
                .to_string(),
        });
    }
    if else_count > 1 {
        return Err(EvalError::SwitchShape {
            detail: "a switch admits exactly one `else` arm".to_string(),
        });
    }
    if stage.arms.last().is_some_and(|a| a.label.is_some()) {
        return Err(EvalError::SwitchShape {
            detail: "the `else` arm must be written last".to_string(),
        });
    }
    let mut seen = std::collections::BTreeSet::new();
    for arm in &stage.arms {
        if let Some(label) = &arm.label {
            if !seen.insert(label.as_str()) {
                return Err(EvalError::SwitchShape {
                    detail: format!("duplicate switch arm label '{label}'"),
                });
            }
        }
    }
    Ok(())
}

/// Render a segment list into a `/seg/seg` mount path string for the router.
fn render_path(segments: &[Name]) -> String {
    let mut s = String::new();
    for seg in segments {
        s.push('/');
        s.push_str(seg);
    }
    if s.is_empty() {
        s.push('/');
    }
    s
}

/// The plan-time STRUCTURAL comparison behind a `|> of <type>` assertion (blueprint §5.6). Column-
/// name-set equality plus per-column type equality where both sides are concretely known — a column
/// left `unknown` on either side is conservatively skipped (the honest gap meter, so late-bound
/// `extend`/`set` columns never spuriously fail an assertion). Returns [`EvalError::OfAssertionFailed`]
/// naming the differing columns; `of` never coerces.
fn structural_diff(ty: &str, asserted: &Schema, actual: &Schema) -> Result<(), EvalError> {
    let mut missing = Vec::new();
    let mut mismatched = Vec::new();
    for col in &asserted.columns {
        match actual.column(&col.name) {
            None => missing.push(col.name.clone()),
            Some(got) => {
                if col.ty != ColumnType::Unknown
                    && got.ty != ColumnType::Unknown
                    && col.ty != got.ty
                {
                    mismatched.push((
                        col.name.clone(),
                        col.ty.type_token().to_string(),
                        got.ty.type_token().to_string(),
                    ));
                }
            }
        }
    }
    let unexpected: Vec<String> = actual
        .columns
        .iter()
        .filter(|c| asserted.column(&c.name).is_none())
        .map(|c| c.name.clone())
        .collect();
    if missing.is_empty() && unexpected.is_empty() && mismatched.is_empty() {
        Ok(())
    } else {
        Err(EvalError::OfAssertionFailed {
            ty: ty.to_string(),
            missing,
            unexpected,
            mismatched,
        })
    }
}

/// The first path segment (the conventional driver namespace) of a `/seg/seg` path.
fn first_segment(path: &str) -> String {
    path.trim_start_matches('/')
        .split('/')
        .next()
        .unwrap_or("")
        .to_string()
}

/// Describe a node's schema via the driver's **pure** `describe` (no I/O). An
/// undescribable node degrades to an empty schema rather than erroring — the path routed,
/// so the relation exists; its columns are simply late-bound (blueprint §4).
fn describe_schema(driver: &dyn Driver, vfs: &str) -> Result<Schema, EvalError> {
    match driver.describe(&Path::new(vfs.to_string())) {
        Ok(desc) => Ok(desc.schema),
        // A driver that cannot describe a node yields a late-bound (empty) schema; this
        // is not a hard error (the path resolved). Keeps the evaluator total.
        Err(_) => Ok(Schema::empty()),
    }
}

/// Infer the schema of an inline `VALUES` relation from its first row (blueprint §4). Explicit
/// column names are honoured; otherwise positional `col0, col1, …` names are synthesised.
/// Lower an `UPDATE … SET … WHERE …` / `REMOVE … WHERE …` body into the effect's row payload. The
/// row carries the `SET` columns (the new values) plus the equality-key columns extracted from the
/// `WHERE` (`col == <const>` leaves) — exactly what a key-addressed applier (e.g. the SQL driver)
/// splits into `SET <non-key>` + `WHERE <key>`. A bare `REMOVE … WHERE id == 1` yields a one-column
/// `[id]` row; a non-equality filter contributes no key column (the applier then rejects an
/// un-keyed whole-table write, honestly). Constants only — a non-literal `SET`/`WHERE` value is a
/// structured [`EvalError::NonLiteralValues`].
fn setwhere_row_batch(set: &[qfs_parser::Assignment]) -> Result<RowBatch, EvalError> {
    // No SET at all (a `REMOVE … WHERE …`) means the effect writes NOTHING: yield a genuinely empty
    // batch rather than one empty row, so "a REMOVE carries empty args" is literally true and an
    // applier cannot mistake a payload-shaped hole for a payload.
    if set.is_empty() {
        return Ok(RowBatch::default());
    }
    let mut cols: Vec<Column> = Vec::new();
    let mut vals: Vec<Value> = Vec::new();
    for assign in set {
        cols.push(Column::new(assign.name.clone(), ColumnType::Unknown, true));
        vals.push(literal_value(&assign.value)?);
    }
    Ok(RowBatch::new(Schema::new(cols), vec![Row::new(vals)]))
}

/// Build the `WHERE`-selector batch (blueprint §7) from a filter predicate: a single row of every
/// conjoined `col == const` leaf. This is the **only** channel a filter travels on — `args` carries
/// the SET/VALUES payload and nothing else — so a `WHERE` key that shares a `SET` column (the
/// same-column `SET name='X' WHERE name='Y'` case) is carried faithfully rather than de-duped away.
/// Returns `None` when the predicate carries no addressable equality key (a pure `OR`/range/non-const
/// filter); an applier that needs a key then has none, and says so.
fn where_selector_batch(filter: &Expr, path: &str) -> Result<RowBatch, EvalError> {
    let Some(leaves) = collect_eq_constants(filter) else {
        // Any part of the filter the selector cannot represent (a `>`/`!=` comparison, an `OR`,
        // a non-constant side) MUST refuse the whole statement: the appliers resolve a filtered
        // `UPDATE`/`REMOVE` from the selector channel alone, so a partially-captured filter is
        // an under-constrained (over-deleting) irreversible write, not a narrower one.
        return Err(EvalError::WriteFilterUnsupported {
            path: path.to_string(),
            detail: "an UPDATE/REMOVE `WHERE` must be a conjunction (`and`) of \
                     `column == <constant>` equalities — other comparison forms cannot be \
                     carried to the applier and would widen the write"
                .to_string(),
        });
    };
    let mut cols: Vec<Column> = Vec::with_capacity(leaves.len());
    let mut vals: Vec<Value> = Vec::with_capacity(leaves.len());
    for (name, value) in leaves {
        if let Some(idx) = cols.iter().position(|c| c.name == name) {
            // A duplicate WHERE key on the same column: an identical binding is redundant (keep
            // the first); a CONTRADICTORY one (`name=='a' AND name=='b'`) matches nothing the
            // one-equality-per-column selector can express — refuse rather than drop a binding
            // and address the wrong rows.
            if vals[idx] != value {
                return Err(EvalError::WriteFilterUnsupported {
                    path: path.to_string(),
                    detail: format!(
                        "the `WHERE` binds column `{name}` to two different constants — a \
                         selector carries one equality per column"
                    ),
                });
            }
            continue;
        }
        cols.push(Column::new(name, ColumnType::Unknown, true));
        vals.push(value);
    }
    Ok(RowBatch::new(Schema::new(cols), vec![Row::new(vals)]))
}

/// Collect `col == <const>` equality leaves from a `WHERE` predicate (recursing through `AND`),
/// returning each as `(column, value)` — or `None` when ANY leaf is not such an equality
/// (a non-equality comparison, an `OR`, a non-constant side). All-or-nothing on purpose
/// (ticket 20260717102000): the selector is the ONLY filter channel an applier sees, so a
/// silently-skipped leaf under-constrains an irreversible write; the caller fails closed instead.
fn collect_eq_constants(expr: &Expr) -> Option<Vec<(String, Value)>> {
    use qfs_parser::Op;
    match expr {
        Expr::Binary {
            op: Op::And,
            lhs,
            rhs,
        } => {
            let mut out = collect_eq_constants(lhs)?;
            out.extend(collect_eq_constants(rhs)?);
            Some(out)
        }
        Expr::Binary {
            op: Op::Eq,
            lhs,
            rhs,
        } => match (lhs.as_ref(), rhs.as_ref()) {
            (Expr::Col(col), Expr::Lit(lit)) | (Expr::Lit(lit), Expr::Col(col)) => {
                Some(vec![(col.clone(), literal_to_value(lit))])
            }
            // The lexer surfaces the bare keyword constants `true`/`false`/`null` as
            // identifiers (`Expr::Col`) — in a WHERE equality they are the constant, exactly
            // as `literal_value` treats them in a VALUES/SET cell.
            (Expr::Col(col), Expr::Col(kw)) if keyword_const(kw).is_some() => {
                Some(vec![(col.clone(), keyword_const(kw)?)])
            }
            (Expr::Col(kw), Expr::Col(col)) if keyword_const(kw).is_some() => {
                Some(vec![(col.clone(), keyword_const(kw)?)])
            }
            _ => None,
        },
        _ => None,
    }
}

/// The constant a bare keyword identifier denotes (`true`/`false`/`null`), or `None` for a real
/// column reference — the same convention [`literal_value`] applies to VALUES/SET cells.
fn keyword_const(name: &str) -> Option<Value> {
    match name.to_ascii_lowercase().as_str() {
        "true" => Some(Value::Bool(true)),
        "false" => Some(Value::Bool(false)),
        "null" => Some(Value::Null),
        _ => None,
    }
}

/// Map a parser [`EffectVerb`] to the driver-contract [`Verb`] — the form
/// [`Driver::plan_write`](qfs_driver::Driver::plan_write) matches on (the write verbs only).
fn driver_verb(verb: EffectVerb) -> Verb {
    match verb {
        EffectVerb::Insert => Verb::Insert,
        EffectVerb::Upsert => Verb::Upsert,
        EffectVerb::Update => Verb::Update,
        EffectVerb::Remove => Verb::Remove,
    }
}

/// Evaluate one `VALUES` cell expression to a constant [`Value`]. VALUES cells are constants by
/// construction (blueprint §4); a non-constant form (column ref, `fn(..)`, arithmetic) is rejected as a
/// structured [`EvalError::NonLiteralValues`] rather than silently coerced — computed rows belong
/// in an `INSERT … FROM <query>`.
fn literal_value(expr: &Expr) -> Result<Value, EvalError> {
    match expr {
        Expr::Lit(lit) => Ok(literal_to_value(lit)),
        // The lexer surfaces the bare keyword constants `true`/`false`/`null` as identifiers
        // (`Expr::Col`); in a VALUES/SET cell they are the boolean / null literal, not a column
        // reference (a real column ref in a constant cell is rejected below).
        Expr::Col(name) => match name.to_ascii_lowercase().as_str() {
            "true" => Ok(Value::Bool(true)),
            "false" => Ok(Value::Bool(false)),
            "null" => Ok(Value::Null),
            _ => Err(EvalError::NonLiteralValues {
                detail: format!("VALUES expects a constant, got column reference `{name}`"),
            }),
        },
        // t92 composite constructors are constant in a VALUES/SET cell iff every element is
        // constant (the inline-literal attachment case). A non-constant element (a column ref)
        // makes the whole cell non-constant — computed rows belong in an `INSERT … FROM <query>`.
        Expr::Array(elems) => Ok(Value::Array(
            elems
                .iter()
                .map(literal_value)
                .collect::<Result<Vec<Value>, _>>()?,
        )),
        Expr::Struct(fields) => Ok(Value::Struct(Fields::new(
            fields
                .iter()
                .map(|(name, e)| Ok((name.clone(), literal_value(e)?)))
                .collect::<Result<Vec<(Name, Value)>, EvalError>>()?,
        ))),
        other => Err(EvalError::NonLiteralValues {
            detail: format!("VALUES expects a constant, got {other:?}"),
        }),
    }
}

/// Map a parser [`Literal`] to the canonical [`Value`]. `Size` lowers to its byte magnitude; a
/// `Typed` introducer (`DATE '…'`) keeps its raw inner text (the backend parses it) — both are
/// the honest, lossless lowering for the effect payload.
fn literal_to_value(lit: &Literal) -> Value {
    match lit {
        Literal::Str(s) => Value::Text(s.clone()),
        Literal::Int(n) => Value::Int(*n),
        Literal::Float(f) => Value::Float(*f),
        Literal::Bool(b) => Value::Bool(*b),
        Literal::Null => Value::Null,
        Literal::Size { value, .. } => Value::Int(*value as i64),
        Literal::Typed { raw, .. } => Value::Text(raw.clone()),
        // t92: hex bytes lower to `Value::Bytes`. `[ … ]` arrays and `{ … }` structs are now
        // the expression forms `Expr::Array`/`Expr::Struct`, constant-folded by `literal_value`.
        Literal::Bytes(b) => Value::Bytes(b.clone()),
    }
}

fn values_schema(values: &Values) -> Schema {
    let width = values.rows.first().map_or(0, Vec::len);
    let mut cols = Vec::with_capacity(width);
    for i in 0..width {
        let name = values
            .columns
            .as_ref()
            .and_then(|c| c.get(i).cloned())
            .unwrap_or_else(|| format!("col{i}"));
        // Literal column types are late-bound here (pure value typing is the runtime's
        // job); the schema carries the names + Unknown types so projection resolves.
        cols.push(Column::new(name, ColumnType::Unknown, true));
    }
    Schema::new(cols)
}

/// The honest affected estimate for an effect (blueprint §8): an `INSERT … VALUES` of `n`
/// literal rows is `Exact(n)`; a filter-driven `UPDATE`/`REMOVE` is `Unknown` until apply.
fn write_affected(effect: &EffectStmt) -> Affected {
    match &effect.body {
        EffectBody::Values(v) => Affected::Exact(v.rows.len() as u64),
        EffectBody::Pipeline(_) | EffectBody::SetWhere { .. } => {
            if matches!(effect.verb, EffectVerb::Remove) {
                // A REMOVE over a set has an unknown count until apply.
                Affected::Unknown
            } else {
                Affected::Unknown
            }
        }
    }
}

/// Convenience: re-export the canonical AST verb → effect-kind translation so callers can
/// check the mapping without reaching into both `qfs-core` and `qfs-plan`.
#[must_use]
pub fn effect_kind_for(verb: EffectVerb) -> EffectKind {
    kind_for_verb(write_verb_for(verb))
}

/// A `CALL`-effect's [`ProcId`] from a resolved qualified name (`driver.proc`). Exposed so
/// E2 can build `Call` nodes consistently with the evaluator's identity scheme.
#[must_use]
pub fn call_proc_id(qualified: &str) -> ProcId {
    ProcId::new(qualified)
}

#[cfg(test)]
mod tests;
