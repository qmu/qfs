//! The owned, vendor-free qfs AST (blueprint §2.2 pipe-SQL, §3 closed core, §4 data
//! model). This is the **full** grammar surface (t04), promoted from the E0 spike
//! subset: every downstream subsystem (effect-plan, runtime, drivers, server DDL)
//! consumes these sum types.
//!
//! ## Closed core, structurally enforced (blueprint §3)
//! The closed-core thesis — "new backend = zero new keywords" — is enforced *by the
//! shape of these enums*: [`Statement`] and [`PipeOp`] have **no** per-driver,
//! per-action variant, and they are NOT `#[non_exhaustive]` here precisely so a
//! governance test (`grammar`/`lib` tests) can lock their variant set. Everything a
//! driver contributes flows through exactly three **string-named** open seams:
//! [`PathExpr`] (the path/mount registry), [`CallRef`]/[`FnRef`] (the
//! function/procedure registry), and [`Codec`] (the codec registry). A driver can
//! never add an AST node; it can only supply a name inside one of these.
//!
//! ## Owned DTOs / no vendor leak (blueprint §11)
//! Nothing here depends on winnow or any driver/vendor crate. Spans are the
//! `qfs_lang::Span` byte-range primitive (shared with the lexer); literals are owned
//! `std` types. `serde::Serialize` powers `-json` AST dumps and the golden tests.
//!
//! ## Purity (blueprint §3 purity invariant)
//! The AST is **data**: it describes a statement, it does not execute one. `INSERT`
//! vs `UPSERT` is preserved as a distinct [`EffectVerb`] so the runtime can pick a
//! retry-safe verb (blueprint §7); `CALL` is a plan-constructing reference node, never an
//! effect.

use qfs_lang::Span;
use serde::{Deserialize, Serialize};

/// Serialize a `qfs_lang::Span` as a `[start, end]` byte-range pair.
///
/// `qfs_lang::Span` is intentionally `serde`-free (the lexer crate stays zero-dep,
/// B7), so the AST supplies its own projection rather than adding serde to
/// `qfs-lang`. This keeps the span legible in `-json` AST dumps and golden tests
/// without leaking a serde dependency into the closed-core crate.
fn serialize_span<S>(span: &Span, ser: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    use serde::ser::SerializeTuple;
    let mut t = ser.serialize_tuple(2)?;
    t.serialize_element(&span.start)?;
    t.serialize_element(&span.end)?;
    t.end()
}

/// Deserialize a `qfs_lang::Span` from a `[start, end]` byte-range pair (the inverse
/// of [`serialize_span`]). The AST owns this projection because `qfs_lang::Span` is
/// serde-free (the lexer crate stays zero-dep). This makes the owned AST a fully
/// round-trippable serializable value: the server-DDL deferred body (t31
/// `StatementSpec`/`PlanSpec`) is rehydrated from its serialized form WITHOUT
/// re-running the parser — so the runtime cannot hit a parse error at fire time.
fn deserialize_span<'de, D>(de: D) -> Result<Span, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let [start, end] = <[u32; 2]>::deserialize(de)?;
    Ok(Span::new(start, end))
}

/// An identifier name (a path segment, a driver/action name, a column, a codec
/// format). Always a raw string — names are *registry* concerns resolved in a later
/// semantic phase (E2), never grammar (blueprint §3).
pub type Ident = String;

/// The top-level statement sum type (blueprint §3). **Closed core**: exactly these six
/// forms. Not `#[non_exhaustive]` — the governance test locks this variant set so a
/// later ticket cannot smuggle in a per-driver statement form. The fifth and sixth forms,
/// [`Statement::Let`] (ticket t60) and [`Statement::Transaction`] (ticket t62), are the
/// **deliberate** M6 functional-core additions: each is gated by exactly the same governance
/// tripwire as the keyword freeze (the variant-count lock in `tests`), updated in step so the
/// addition is reviewed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Statement {
    /// `FROM <source> |> op |> op …` — a pure read pipeline.
    Query(Pipeline),
    /// `INSERT/UPSERT INTO … | UPDATE … | REMOVE …` — an effect statement.
    Effect(EffectStmt),
    /// `CREATE ENDPOINT|TRIGGER|JOB|VIEW|… ` — server DDL sugar (blueprint §10).
    Ddl(ServerDdl),
    /// `PREVIEW <stmt>` / `COMMIT <stmt>` — a plan wrapper (blueprint §7).
    Plan(PlanWrap),
    /// `LET <name> = <pipeline>` — a relation binding (M6 functional core, ticket t60).
    ///
    /// The program model the roadmap settled on (§1.2): a program is a sequence of
    /// statements with **no terminator**, and a `LET` names an intermediate relation that
    /// stays in scope for everything after it. This is encoded as a **let-in nesting**: the
    /// `body` is the rest of the program (the next `LET`, or the final statement that uses
    /// the binding). Scoping is therefore lexical and conservative — a binding is visible to
    /// its `body` only, shadowing is allowed (an inner `LET` of the same name wins for its
    /// own `body`), and there are no recursive/forward references (`value` is resolved
    /// without `name` in scope). `value` is restricted by the grammar to a **relation**
    /// (a `Statement::Query` pipeline), never an effect — so a `LET` can never smuggle a
    /// write into a pure context (the safety floor holds trivially).
    Let {
        /// The bound name, referenced later as a bare-identifier [`Source::Name`].
        name: Ident,
        /// The bound relation — always a [`Statement::Query`] (grammar-enforced).
        value: Box<Statement>,
        /// The rest of the program, with `name` in scope.
        body: Box<Statement>,
    },
    /// `TRANSACTION { <effect> ; <effect> ; … }` — a reversible-only, all-or-nothing block
    /// (M6 transactional core, ticket t62, decision G).
    ///
    /// The block groups effect statements into ONE atomic unit with a defined **commit-point
    /// ordering** (source order): the effects apply all-or-nothing via the existing `qfs-txn`
    /// envelope (single transactional source → ACID `BEGIN…COMMIT`/rollback; cross-source →
    /// reverse-order saga compensation). Because a transaction promises rollback, every effect
    /// inside **must be reversible** — an irreversible effect (a `REMOVE`, an irreversible `CALL`)
    /// is a hard **eval-time error** (`EvalError::IrreversibleInTransaction`), not the milder
    /// "needs an ack" of the outside-transaction case. The grammar restricts `body` to **effect**
    /// statements only (no read pipeline, no nested `TRANSACTION`, no `LET`) so the block stays a
    /// thin wrapper over existing [`EffectStmt`]s and adds NO new effect kind. Kept conservative
    /// this slice (no nesting) so a later relaxation is non-breaking.
    Transaction {
        /// The effect statements in the block, in source (commit-point) order. Each is a
        /// [`Statement::Effect`] (grammar-enforced).
        body: Vec<Statement>,
        /// Source span of the `TRANSACTION { … }` block.
        #[serde(
            serialize_with = "serialize_span",
            deserialize_with = "deserialize_span"
        )]
        span: Span,
    },
}

/// A `PREVIEW`/`COMMIT` wrapper around an inner statement (blueprint §3 plan keywords).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlanWrap {
    /// `true` for `COMMIT`, `false` for `PREVIEW`.
    pub commit: bool,
    /// The wrapped statement.
    pub inner: Box<Statement>,
    /// Source span of the `PREVIEW`/`COMMIT` keyword.
    #[serde(
        serialize_with = "serialize_span",
        deserialize_with = "deserialize_span"
    )]
    pub span: Span,
}

/// A read pipeline: a source followed by zero or more `|>`-separated ops.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Pipeline {
    /// What the pipeline reads from.
    pub source: Source,
    /// The chain of pipe operations.
    pub ops: Vec<PipeOp>,
}

/// The source of a pipeline (blueprint §2.2). Either a `/driver/...` path, an inline
/// `VALUES` block, or a parenthesised sub-pipeline.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Source {
    /// `/driver/seg/seg` (the open path registry).
    Path(PathExpr),
    /// `FROM VALUES (..),(..)` — an inline literal relation.
    Values(Values),
    /// `FROM ( <pipeline> )` — a sub-query.
    Subquery(Box<Pipeline>),
    /// `FROM <name>` — a bare identifier naming a `LET`-bound relation (M6, ticket t60).
    /// Unresolved here (the parser validates shape only); a name with no matching binding
    /// in scope is a structured resolve/eval error, never a silent empty relation.
    Name(Ident),
}

/// One pipe operation following `|>` (blueprint §3 query/transform + codec + call).
///
/// **Closed core**: exactly one variant per closed-core query/transform keyword,
/// plus the three registry seams ([`PipeOp::Decode`]/[`PipeOp::Encode`] = codec
/// registry, [`PipeOp::Call`] = procedure registry). There is deliberately **no**
/// per-action variant (no `Send`, no `Merge`): those are pure registry functions
/// that desugar to `CALL` (blueprint §3). The governance test locks this variant set.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PipeOp {
    /// `WHERE <expr>`
    Where(Expr),
    /// `SELECT <proj>, …`
    Select(Vec<Projection>),
    /// `EXTEND <name> = <expr>, …` (add columns, keep the rest).
    Extend(Vec<Assignment>),
    /// `SET <name> = <expr>, …` (overwrite columns in place).
    Set(Vec<Assignment>),
    /// `AGGREGATE <expr> [AS <name>], …` (the aggregate projections).
    Aggregate(Vec<Projection>),
    /// `GROUP BY <expr>, …`
    GroupBy(Vec<Expr>),
    /// `ORDER BY <expr> [ASC|DESC], …` — modelled as expr + descending flag.
    OrderBy(Vec<OrderKey>),
    /// `LIMIT <n>`
    Limit(i64),
    /// `DISTINCT`
    Distinct,
    /// `JOIN <source> ON <expr>`
    Join(JoinOp),
    /// `UNION <pipeline>`
    Union(Box<Pipeline>),
    /// `EXCEPT <pipeline>`
    Except(Box<Pipeline>),
    /// `INTERSECT <pipeline>`
    Intersect(Box<Pipeline>),
    /// `AS <alias>` (name the current relation).
    As(Ident),
    /// `EXPAND <field>` (explode a nested collection into rows, blueprint §4).
    Expand(PathRef),
    /// `DECODE <fmt>` (codec registry seam, blueprint §4).
    Decode(Codec),
    /// `ENCODE <fmt>` (codec registry seam, blueprint §4).
    Encode(Codec),
    /// `CALL driver.action(args)` (procedure registry seam, blueprint §3).
    Call(CallRef),
    /// `TRANSFORM <name>` — the model-calling pipe stage (blueprint §15, decision W).
    ///
    /// A **contextual-identifier** stage (`transform` is *not* a frozen keyword — the
    /// keyword set stays 39) naming a declared `CREATE TRANSFORM` definition, resolved
    /// later. Unlike the pass-through codec stages it is schema-transforming, and its
    /// model call is an impure effect performed by an injected applier, never the pure
    /// engine. The governance test locks this variant into the closed-core set.
    Transform(TransformRef),
    /// `SWITCH <col> { '<label>' => <arm>, …, else => <arm> }` — the model-routing pipe stage
    /// (blueprint §18). A **contextual-identifier** stage (`switch`/`else` are *not* frozen
    /// keywords — the keyword set stays 39). The governance test locks this variant into the
    /// closed-core set (the second additive pipe stage).
    Switch(SwitchStage),
    /// `FOLLOW <field>` — the declared-driver second-fetch stage (blueprint §13, ticket
    /// 20260711121526): take the named field of the (single) delivered row as the URL of a
    /// second GET and deliver its raw bytes as a one-row `content` batch. ONLY meaningful
    /// inside a declared view body — every other context refuses it structurally at lowering.
    /// A **contextual-identifier** stage like `transform`/`switch` (`follow` is *not* a frozen
    /// keyword — the keyword set stays 39). The governance test locks this variant into the
    /// closed-core set (the third additive stage — a deliberate, reviewed change-control event,
    /// like the two before it).
    Follow(FollowRef),
    /// `OF <name>` or `OF (<col> <type>, …)` — the general, any-position, plan-time-checked type
    /// assertion (blueprint §5.6). A **contextual-identifier** stage (`of` is *not* a frozen keyword —
    /// the keyword set stays 39; `of` is already `word("OF")` in the DDL). Admission criterion (2)
    /// of §5.3a: it **asserts/names the relation type** — a plan-time schema rewrite in the degenerate
    /// sense that it computes `Relation<S> → Relation<S>` (schema-*identity*) while *checking* `S`
    /// against the asserted type. It performs **no effect and no row transformation**: unlike
    /// `select`/`extend` it never coerces — a structural mismatch is a plan-time structured error
    /// naming the differing columns; where the asserted type carries a `WHERE` refinement the
    /// structural half is plan-checked and the predicate half is membership at the next boundary rows
    /// exist (§5.4's honest split, restated at the use site). The governance test locks this variant
    /// into the closed-core set (the 20th `PipeOp`, a deliberate, reviewed change-control event).
    Of(OfRef),
}

/// An `of <type>` use-site assertion (blueprint §5.6): a general, any-position, plan-time-checked
/// type assertion. The target is either a declared type **NAME** (`of customer`, resolved against
/// the type catalog at plan time) or an **inline anonymous** structural type literal (`of (priority
/// text, reason text)` — the §5.2 column-list production). A `/type/…` PATH in target position is
/// the §5.7 category error and does not parse (`of_op` reads a `type_name`/column-list, so a
/// `Token::Path` there fails), exactly like `transform /path`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OfRef {
    /// The asserted type: a declared type name or an inline structural literal.
    pub target: OfTarget,
    /// Source span of the `of …` stage.
    #[serde(
        serialize_with = "serialize_span",
        deserialize_with = "deserialize_span"
    )]
    pub span: Span,
}

/// The asserted type of an [`OfRef`] (blueprint §5.6).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum OfTarget {
    /// `of customer` — a declared type NAME, canonicalized to its `/type/<name>` catalog path (§5.5),
    /// resolved against the type-def registry at plan time.
    Named(String),
    /// `of (priority text, reason text)` — an anonymous structural type literal (§5.2), checked
    /// against the computed schema with no catalog lookup.
    Inline(Vec<OfColumn>),
}

/// One `<name> <type> [PRIMARY KEY | UNIQUE | NOT NULL]*` column of an inline `of (…)` structural
/// type literal — the AST twin of the `CREATE TABLE`/`CREATE TYPE` column definition (§5.2's one
/// production), carried in the pipe AST so an inline assertion is self-describing without a catalog.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OfColumn {
    /// The column name.
    pub name: String,
    /// The canonical §5 type token (a base column type, or a declared type's `/type/…` catalog path
    /// when a name is used in column position — resolution stays out of the parser, exactly as
    /// `CREATE TABLE` columns are stored).
    pub ty: String,
    /// `false` when the column carries `NOT NULL`.
    pub nullable: bool,
    /// `true` when the column carries `PRIMARY KEY`.
    pub primary_key: bool,
    /// `true` when the column carries `UNIQUE`.
    pub unique: bool,
}

/// A `FOLLOW <field>` reference (blueprint §13): the delivered-row field whose text value is
/// the URL of the second GET (e.g. a `download_url` the service minted). Evaluated by the
/// declared-view evaluator; the follow request carries NO driver credentials (the URL is
/// self-authorizing), so no secret can leave the driver's declared host.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FollowRef {
    /// The delivered-row field carrying the follow URL.
    pub field: Ident,
    /// Source span of the `follow <field>` stage.
    #[serde(
        serialize_with = "serialize_span",
        deserialize_with = "deserialize_span"
    )]
    pub span: Span,
}

/// A `switch <col> { … }` routing stage (blueprint §18). It calls no model and performs no
/// effect of its own: it partitions the incoming relation by the discriminant column's value and
/// dispatches each partition to a declared arm. The *choice* is model-made — the discriminant is
/// typically a `transform` OUTPUT column — but every arm's effect exists in the plan before any
/// model runs, so PREVIEW shows the full declared effect **union** (§18-C). Forced-local like
/// `transform`; **terminal** — it must be the last op in its pipeline (this slice).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SwitchStage {
    /// The discriminant column whose per-row value selects the arm.
    pub discriminant: Ident,
    /// The arms in declaration order. Exactly one is the `else` default (`label: None`), written
    /// last; the labeled arms carry unique string labels.
    pub arms: Vec<SwitchArm>,
    /// Source span of the `switch … { … }` stage.
    #[serde(
        serialize_with = "serialize_span",
        deserialize_with = "deserialize_span"
    )]
    pub span: Span,
}

/// One arm of a [`SwitchStage`]: a label (or `else`) and the pipeline continuation its routed
/// partition runs. The continuation is a bare pipeline over the routed rows (`'urgent' => <arm>`
/// is notation for `(rows) => rows |> <arm>`, §18-B): zero or more leading pipe ops (`select …`,
/// possibly ending in a terminal `CALL`), optionally terminated by a write ([`ArmWrite`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SwitchArm {
    /// The literal label this arm matches, or `None` for the `else` default arm.
    pub label: Option<String>,
    /// The arm's leading pipe-op continuation over its routed partition (may be empty; may end in
    /// a terminal `PipeOp::Call` to an effect procedure).
    pub ops: Vec<PipeOp>,
    /// The arm's terminal write consuming the routed+piped rows, or `None` for a pure /
    /// CALL-terminal arm.
    pub write: Option<ArmWrite>,
    /// Source span of the arm.
    #[serde(
        serialize_with = "serialize_span",
        deserialize_with = "deserialize_span"
    )]
    pub span: Span,
}

/// A [`SwitchArm`]'s terminal write: `INSERT`/`UPSERT INTO <target>` over the routed rows. The
/// rows come from the arm's piped partition (there is no `VALUES`/`FROM` body — that is what the
/// partition *is*), so this captures only the verb, target, and optional `RETURNING`. `UPDATE`/
/// `REMOVE` are self-contained (`SET`/`WHERE`) and are not expressible as an arm terminal this
/// slice — route to them via a terminal `CALL` instead.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArmWrite {
    /// `INSERT` or `UPSERT` (the piped-row writes).
    pub verb: EffectVerb,
    /// The path the routed rows are written to.
    pub target: PathExpr,
    /// An optional `RETURNING <expr>, …` projection.
    pub returning: Option<Vec<Projection>>,
    /// Source span of the arm write clause.
    #[serde(
        serialize_with = "serialize_span",
        deserialize_with = "deserialize_span"
    )]
    pub span: Span,
}

/// A `JOIN <source> ON <expr>` operation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JoinOp {
    /// The joined relation.
    pub source: Source,
    /// The `ON` predicate.
    pub on: Expr,
}

/// One `ORDER BY` sort key.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OrderKey {
    /// The sort expression.
    pub expr: Expr,
    /// `true` for `DESC`, `false` for the `ASC` default.
    pub descending: bool,
}

/// One `SELECT`/`AGGREGATE` projection: an expression with an optional `AS` alias,
/// or a bare `*`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Projection {
    /// `*` — project everything.
    Star,
    /// `<expr> [AS <alias>]`
    Expr {
        /// The projected expression.
        expr: Expr,
        /// An optional `AS <alias>`.
        alias: Option<Ident>,
    },
}

/// One `EXTEND`/`SET` assignment: `<name> = <expr>`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Assignment {
    /// The target column name.
    pub name: Ident,
    /// The value expression.
    pub value: Expr,
}

/// An effect statement (blueprint §3 effects). `INSERT`/`UPSERT` are kept distinct via
/// [`EffectVerb`] so the runtime can choose a retry-safe verb (blueprint §7).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EffectStmt {
    /// Which effect verb (`INSERT`/`UPSERT`/`UPDATE`/`REMOVE`).
    pub verb: EffectVerb,
    /// The target path the effect writes to.
    pub target: PathExpr,
    /// The data being written (`VALUES`, a sub-pipeline, or `SET`/`WHERE` clauses).
    pub body: EffectBody,
    /// An optional `RETURNING <expr>, …` projection.
    pub returning: Option<Vec<Projection>>,
}

/// The effect verb. `Insert` and `Upsert` are distinct (idempotency, blueprint §7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EffectVerb {
    /// `INSERT INTO`
    Insert,
    /// `UPSERT INTO`
    Upsert,
    /// `UPDATE`
    Update,
    /// `REMOVE`
    Remove,
}

/// The data portion of an effect statement.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum EffectBody {
    /// `VALUES (..),(..)` — inline literal rows.
    Values(Values),
    /// A sub-pipeline source (`INSERT INTO x FROM y |> …`).
    Pipeline(Box<Pipeline>),
    /// `UPDATE … SET a = b [WHERE …]` — column assignments + optional filter.
    SetWhere {
        /// The `SET` assignments (empty for a bare `REMOVE`).
        set: Vec<Assignment>,
        /// An optional `WHERE` filter.
        filter: Option<Expr>,
    },
}

/// An inline `VALUES` relation: an optional column list plus one or more rows.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Values {
    /// Optional explicit column names: `VALUES (a, b) (1, 2)`.
    pub columns: Option<Vec<Ident>>,
    /// The literal rows; each row is a list of expressions.
    pub rows: Vec<Vec<Expr>>,
}

/// A server-DDL statement (blueprint §10). Each form is **sugar** that desugars downstream
/// to `INSERT INTO /server/...`; the [`ServerDdl::target`] records that path. The
/// parser only validates shape — desugaring lives in a later epic.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ServerDdl {
    /// Which DDL kind (`ENDPOINT`/`TRIGGER`/`JOB`/`VIEW`/…).
    pub kind: DdlKind,
    /// The handler/object name.
    pub name: Ident,
    /// The `/server/...` path this DDL desugars to (blueprint §10).
    pub target: Vec<Ident>,
    /// The optional `DO <plan>` clause (the effect-plan body).
    pub do_plan: Option<Box<Statement>>,
    /// The optional `AS <query>` clause (the backing query for `ENDPOINT`/`VIEW`).
    pub as_query: Option<Box<Statement>>,
    /// The optional `WHERE <pred>` clause (the trigger guard for `TRIGGER`, t34). `WHERE` is a
    /// frozen keyword, so wiring `CREATE TRIGGER … ON <event> WHERE <pred> DO <plan>` adds NO new
    /// keyword — the clause is captured here as a `Statement::Query` wrapping the predicate over an
    /// empty `VALUES` source (so it round-trips through the downstream `StatementSpec`).
    pub where_pred: Option<Box<Statement>>,
    /// The optional `EVERY <interval>` clause (cron interval for `JOB`).
    pub every: Option<String>,
    /// The optional `ON <event>` clause (trigger event / route).
    pub on: Option<String>,
    /// The `ALLOW`/`DENY` rule clauses of a `CREATE POLICY` form (t35). Empty for every
    /// non-POLICY DDL. `ALLOW`/`DENY` are **not** frozen keywords (blueprint §3 freeze) — they are
    /// parsed as contextual UPPERCASE identifiers within the POLICY form (the t31 `AT` lesson,
    /// see the grammar), so wiring `CREATE POLICY … ALLOW … DENY …` adds NO new keyword.
    #[serde(default)]
    pub policy_rules: Vec<PolicyRuleAst>,
    /// The optional `POLICY <name>` **attachment** clause (t35): the `/server/policies` row a
    /// binding (`ENDPOINT`/`TRIGGER`/`JOB`/…) commits its fired plan under (least privilege).
    /// `POLICY` IS a frozen keyword, so this adds none. `None` = no policy attached (fail-closed
    /// default-deny at fire time). Distinct from `policy_rules`, which are the rule body of a
    /// `CREATE POLICY` statement itself.
    #[serde(default)]
    pub policy: Option<String>,
    /// The `CREATE CONNECTION` clauses (`DRIVER`/`AT`/`SECRET`), present only for the `Connection`
    /// kind. **Boxed** so the rare connection form doesn't widen every parsed `Statement` (the
    /// `large_enum_variant` discipline). `None` for every non-CONNECTION DDL.
    #[serde(default)]
    pub connection: Option<Box<ConnectionDeclAst>>,
}

/// The clauses of a `CREATE CONNECTION <name> DRIVER <driver> [AT '<loc>'] [SECRET '<ref>']`
/// declaration — the in-language replacement for the `QFS_SQL_*` / `QFS_GIT_*` env-var alias
/// convention. A shape-only AST node (the driver→path-family map + secret resolution live
/// downstream); `CONNECTION`/`DRIVER`/`SECRET` are contextual idents, so this adds NO frozen keyword.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConnectionDeclAst {
    /// `DRIVER <driver>` — the driver kind (`sqlite`/`postgres`/`mysql`/`git`/`gmail`/…) that
    /// decides which path family the connection mounts under. `None` only in the permissive parse;
    /// a real declaration requires it (validated downstream).
    pub driver: Option<String>,
    /// `AT '<locator>'` — the non-secret location (a file path / URL / bucket / base URL). `None`
    /// when the driver's locator is implicit (e.g. Gmail).
    pub at_locator: Option<String>,
    /// `SECRET '<ref>'` — a secret **reference** (`env:<VAR>` / `vault:<path>`), never an inline
    /// value (a literal is rejected downstream). `None` when the connection needs no secret.
    pub secret_ref: Option<String>,
}

/// One `ALLOW`/`DENY` rule clause inside a `CREATE POLICY` form (t35). A shape-only AST node —
/// the verb-set / driver-glob semantics live in `qfs-server::policy`. Owned, vendor-free.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PolicyRuleAst {
    /// Whether this is an `ALLOW` (`true`) or `DENY` (`false`) rule.
    pub allow: bool,
    /// The verb tokens (`SELECT`/`INSERT`/`UPSERT`/`UPDATE`/`REMOVE`/`CALL`), uppercased — or
    /// a single `ALL` token captured here as the literal `"ALL"`.
    pub verbs: Vec<String>,
    /// Whether the verb list was the bare `ALL` token (vs an explicit list).
    pub all_token: bool,
    /// The optional `ON <driver-glob>` scope (e.g. `mail`, `s3/*`); `None` = every driver.
    pub driver: Option<String>,
    /// The optional `FOR <subject>` actor clause (t57): the user/role/group this rule is for.
    /// `None` = the unscoped `FOR`-less rule (applies to every actor). A shape-only AST node;
    /// the `Subject` semantics live in `qfs-server::policy`. Adds NO keyword —
    /// `FOR`/`user`/`role`/`group` are contextual UPPERCASE idents (the t31 `AT` lesson).
    #[serde(default)]
    pub subject: Option<PolicySubjectAst>,
    /// The optional `AT <path-glob>` realm-scoped path clause (t57): a realm-qualified glob like
    /// `/members/alice/**`. Captured as raw text; the realm/segment semantics live in
    /// `qfs-server::policy`. `None` = every path. `AT` is a contextual ident (no new keyword).
    #[serde(default)]
    pub scope: Option<String>,
    /// The optional `WHERE <expr>` conditional grant (t57). `WHERE` IS a frozen keyword, so this
    /// adds none. The expression is an ORDINARY call (`member_of('/directories/...')`, the
    /// "functions are values" [`Expr::Fn`] seam) — NOT new grammar vocabulary. `None` = no
    /// condition (the grant always applies).
    #[serde(default)]
    pub condition: Option<Expr>,
}

/// One `FOR <kind> <name>` actor clause inside a `CREATE POLICY` rule (t57). A shape-only AST
/// node — the `Subject` semantics live in `qfs-server::policy`. `kind` is the
/// contextual word `user`/`role`/`group`; `name` is the bare principal/role/group identifier.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PolicySubjectAst {
    /// The subject kind word (`user`/`role`/`group`), as written (case-insensitive downstream).
    pub kind: String,
    /// The subject name (a bare identifier: a user id, role label, or group name).
    pub name: String,
}

/// The kind of a server-DDL statement (blueprint §10). Frozen, driver-agnostic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DdlKind {
    /// `CREATE ENDPOINT`
    Endpoint,
    /// `CREATE TRIGGER`
    Trigger,
    /// `CREATE JOB`
    Job,
    /// `CREATE VIEW`
    View,
    /// `CREATE MATERIALIZED VIEW`
    MaterializedView,
    /// `CREATE WEBHOOK`
    Webhook,
    /// `CREATE POLICY`
    Policy,
    /// `CREATE CONNECTION` — an in-language connection declaration (the env-var alias replacement).
    /// `CONNECTION` is parsed as a contextual UPPERCASE ident (the `AT` lesson, like `MATERIALIZED`)
    /// so it adds NO frozen keyword.
    Connection,
}

/// An expression (blueprint §3 operators, frozen). The boolean structure (`AND`/`OR`/
/// `NOT`) and comparison/predicate forms are all closed core; the only open seam is
/// [`Expr::Fn`] (the function registry) and column/path references.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Expr {
    /// A literal value.
    Lit(Literal),
    /// A bare column reference (a single identifier).
    Col(Ident),
    /// A struct-navigation path `a.b.c` (blueprint §4 path access, no flattening).
    Path(Vec<Ident>),
    /// A registry function call `fn(args)` (the function registry seam, blueprint §3).
    Fn(FnRef),
    /// A binary operation `<lhs> <op> <rhs>` (comparison / logical).
    Binary {
        /// The operator.
        op: Op,
        /// Left operand.
        lhs: Box<Expr>,
        /// Right operand.
        rhs: Box<Expr>,
    },
    /// A unary operation (`NOT <expr>`).
    Unary {
        /// The unary operator.
        op: Op,
        /// The operand.
        expr: Box<Expr>,
    },
    /// `<expr> IN (<list>)`.
    In {
        /// The tested expression.
        expr: Box<Expr>,
        /// The candidate set.
        set: Vec<Expr>,
    },
    /// `<expr> BETWEEN <low> AND <high>`.
    Between {
        /// The tested expression.
        expr: Box<Expr>,
        /// Lower bound (inclusive).
        low: Box<Expr>,
        /// Upper bound (inclusive).
        high: Box<Expr>,
    },
    /// `<expr> LIKE <pattern>`.
    Like {
        /// The tested expression.
        expr: Box<Expr>,
        /// The LIKE pattern.
        pattern: Box<Expr>,
    },
    /// `<expr> <op> ANY (<set>)` — the quantified comparison (blueprint §3 `ANY`).
    AnyOp {
        /// The comparison operator applied against the set.
        op: Op,
        /// The tested expression.
        expr: Box<Expr>,
        /// The candidate set.
        set: Vec<Expr>,
    },
    /// A lambda literal `(p, …) => <expr>` — a first-class **value** (M6 ticket t61,
    /// roadmap §1.2, decision H "functions are values").
    ///
    /// **No keyword added.** A lambda rides the *expression* grammar — it is a new
    /// [`Expr`] variant, not a new [`Statement`]/[`PipeOp`] form and not a new reserved
    /// word — so the frozen closed core (the `qfs-lang` keyword/operator freeze) is
    /// **untouched**. It reuses the existing `=>` arrow token (already used by named call
    /// args); the parenthesised parameter list is what distinguishes a lambda from a
    /// named-arg or a parenthesised sub-expression. A *named* function is just a
    /// `LET`-bound lambda (no `DEF`): `LET normalize = (addr) => …`.
    ///
    /// The body is a single sub-expression evaluated under the params bound — a lambda is
    /// a **pure** transformation over values/rows (blueprint §3 purity), it performs no I/O and
    /// constructs no effect node, so a `LET`-bound lambda or a `map`/`filter`/`reduce`
    /// over a relation stays in the read/transform half (the safety floor is untouched).
    Lambda {
        /// The parameter list (possibly empty), each with an optional type annotation.
        params: Vec<Param>,
        /// The body expression, evaluated with the params in scope.
        body: Box<Expr>,
    },
    /// An array constructor `[ e1, e2, … ]` (t92, generalised): each element is a full
    /// sub-expression (a column reference, a literal, a nested array/struct), so the array
    /// is built per row on the read path, not only from constants. An all-constant array is
    /// constant-folded to a [`Value::Array`](qfs_types::Value::Array); an empty array is `[]`.
    Array(Vec<Expr>),
    /// A struct constructor `{ name: value, … }` (t92, generalised): named fields whose
    /// values are full sub-expressions in insertion order (field order preserved). Lowers to
    /// [`Value::Struct`](qfs_types::Value::Struct). The per-row constructor is what feeds a
    /// Gmail draft's `attachments` column from Drive columns (`{filename: name, bytes: content}`).
    Struct(Vec<(String, Expr)>),
}

/// One lambda parameter: a name with an optional type annotation (`addr: text`).
///
/// The annotation is **parsed-and-retained** (`Option<TypeAnn>`) and **enforced** by the
/// plan-time static type checker (blueprint §5.3, decision T / t75 — now the type-system
/// chapter's enforcement arm; the checker types lambda parameters and bodies in
/// `qfs-core::typeck`). A bare `(addr) => …` parameter carries `ty: None` (the honest
/// spelling of "late-bound").
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Param {
    /// The parameter name, bound in the lambda body.
    pub name: Ident,
    /// The optional type annotation (`: text`), retained for the type checker.
    pub ty: Option<TypeAnn>,
}

/// A retained lambda parameter type annotation (`text`, `bool`, `int`, …).
///
/// Stored as the raw annotation text (parse-and-retain, decision S/T). The **one canonical
/// vocabulary** is the `ColumnType` grammar (blueprint §5.2 — `text`/`int`/`bool`/`bytes`/
/// `array<…>`/…), plus the single non-column word `Resource`. The `string`/`i64`/`Row`
/// spellings are **retired** by the type-system chapter and rejected in `typeck`; scalar
/// misspellings are still retained as text so an unrecognised token is a plan-time error, not
/// a silent late-bind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TypeAnn {
    /// The raw type name as written (e.g. `text`, `Row`).
    pub name: Ident,
}

/// The frozen operator set (blueprint §3). No operator can be added without editing this
/// enum (and the keyword/operator freeze tests in `qfs-lang`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Op {
    /// `==`
    Eq,
    /// `<>`
    Ne,
    /// `<`
    Lt,
    /// `>`
    Gt,
    /// `<=`
    Le,
    /// `>=`
    Ge,
    /// `AND`
    And,
    /// `OR`
    Or,
    /// `NOT`
    Not,
    /// `LIKE`
    Like,
    /// `~` (regex match)
    Match,
    /// `+`
    Add,
    /// `-`
    Sub,
    /// `*`
    Mul,
    /// `/`
    Div,
}

/// A literal value (blueprint §4 data model).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Literal {
    /// A string literal.
    Str(String),
    /// An integer literal.
    Int(i64),
    /// A floating-point literal.
    Float(f64),
    /// A boolean literal.
    Bool(bool),
    /// The null literal.
    Null,
    /// A size literal (`25 MB`): magnitude + canonical unit text.
    Size {
        /// The numeric magnitude.
        value: u64,
        /// The unit text (`B`/`KB`/`MB`/`GB`/`TB`).
        unit: String,
    },
    /// A typed literal (`DATE '…'`): the introducer keyword text + raw inner string.
    Typed {
        /// The introducer (`DATE`/`TIME`/`TIMESTAMP`).
        ty: String,
        /// The raw, unvalidated inner string content.
        raw: String,
    },
    /// A hex bytes literal (`X'48656c6c6f'`): the decoded raw bytes (t92). The only composite
    /// **scalar** literal that remains; `[ … ]` arrays and `{ … }` structs are now the
    /// expression forms [`Expr::Array`]/[`Expr::Struct`] (their elements are sub-expressions).
    Bytes(Vec<u8>),
}

/// A `/driver/seg/seg` path expression — the open path/mount registry seam (blueprint §3,
/// §4). Driver and segments are raw strings; `@version` / `AS OF` are temporal
/// coordinates (blueprint §4).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PathExpr {
    /// The path segments (raw text; first is conventionally the driver).
    pub segments: Vec<PathSegment>,
    /// An optional `AS OF '<ts>'` temporal coordinate (blueprint §4).
    pub as_of: Option<String>,
    /// Source span of the whole path.
    #[serde(
        serialize_with = "serialize_span",
        deserialize_with = "deserialize_span"
    )]
    pub span: Span,
}

/// One segment of a [`PathExpr`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PathSegment {
    /// The raw segment name.
    pub name: Ident,
    /// An optional `@version` ref bound to this segment (blueprint §4), raw text.
    pub version: Option<String>,
    /// Whether the segment carried a glob character.
    pub glob: bool,
}

/// A path reference used in expression position (e.g. the target of `EXPAND`), where
/// the path is dotted struct navigation rather than a `/driver/...` mount path.
pub type PathRef = Vec<Ident>;

/// A `CALL driver.action(args)` reference — the procedure registry seam (blueprint §3).
/// All names are strings resolved later; the parser validates *shape* only.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CallRef {
    /// The driver namespace (e.g. `mail`).
    pub driver: Ident,
    /// The action name (e.g. `send`).
    pub action: Ident,
    /// The named/positional arguments.
    pub args: Vec<NamedArg>,
    /// Source span of the call.
    #[serde(
        serialize_with = "serialize_span",
        deserialize_with = "deserialize_span"
    )]
    pub span: Span,
}

/// One argument to a [`CallRef`]: either positional or `name => value`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NamedArg {
    /// The argument name, if given as `name => value`.
    pub name: Option<Ident>,
    /// The argument value.
    pub value: Expr,
}

/// A `fn(args)` registry function reference — the function registry seam (blueprint §3).
/// The name is a string resolved later (receiver-typed alias resolution is E2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FnRef {
    /// The function name.
    pub name: Ident,
    /// The positional arguments.
    pub args: Vec<Expr>,
    /// Source span of the call.
    #[serde(
        serialize_with = "serialize_span",
        deserialize_with = "deserialize_span"
    )]
    pub span: Span,
}

/// A `DECODE fmt` / `ENCODE fmt` codec reference — the codec registry seam (blueprint §4).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Codec {
    /// The codec format name (`json`/`yaml`/`csv`/…), resolved later.
    pub fmt: Ident,
    /// Source span of the codec format token.
    #[serde(
        serialize_with = "serialize_span",
        deserialize_with = "deserialize_span"
    )]
    pub span: Span,
}

/// A `TRANSFORM <name>` reference — the model-calling pipe stage (blueprint §15).
///
/// `name` is a contextual identifier naming a declared `CREATE TRANSFORM` definition
/// (its input/output schema, provider, model, effort). The name is resolved later
/// against the transform registry; the parser validates shape only.
///
/// **The reference is a bare NAME, never a path (§5.5): paths are data, names are
/// definitions.** `transform triage` names the definition; `transform /transform/triage`
/// is a category error — a `/transform/…` path in selector position — and does not parse
/// (`transform_op` reads a bare `ident`, so a `Token::Path` there fails). `/transform` stays
/// the catalog/shell face (`ls /transform`), addressing the catalog as data, not a definition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TransformRef {
    /// The declared transform definition name, resolved later.
    pub name: Ident,
    /// Source span of the transform name token.
    #[serde(
        serialize_with = "serialize_span",
        deserialize_with = "deserialize_span"
    )]
    pub span: Span,
}
