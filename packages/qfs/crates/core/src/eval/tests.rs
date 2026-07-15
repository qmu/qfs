//! Unit tests for the pure evaluator (t07): query pipeline → `PlanSource` with threaded
//! schema, each effect verb → the correct `EffectKind` node, irreversible flagging,
//! `RETURNING` schema, `PREVIEW` of an evaluated plan, and structured errors for
//! ill-typed / unresolvable input. No execution, no I/O, no credentials.

use super::*;
use crate::registry::MountRegistry;
use qfs_driver::{
    AliasFn, Archetype, Capabilities, CfsError, NodeDesc, Param, Path, ProcSig, PushdownProfile,
    Verb, VersionSupport,
};
use qfs_parser::parse_statement;
use qfs_plan::{preview, AppliedEffect, ApplyError, EffectNode, PlanApplier};
use qfs_types::{Column, ColumnType, Schema};
use std::sync::Arc;

/// A panicking applier: if the evaluator ever performed I/O (it must not), committing
/// through this would blow up. Building/previewing a plan never calls it — that is the
/// purity proof at the test boundary.
#[derive(Default)]
struct PanicApplier;
impl PlanApplier for PanicApplier {
    fn apply(&mut self, _node: &EffectNode) -> Result<AppliedEffect, ApplyError> {
        panic!("evaluator must not perform I/O: applier invoked during pure evaluation");
    }
}

/// An in-memory test driver with a typed `describe` schema, declared procedures, prelude
/// aliases, and per-node capabilities. No I/O, no creds.
struct TestDriver {
    mount: &'static str,
    /// An optional CANONICAL id() distinct from the mount — models a driver CONNECTed under a
    /// user-chosen (multi-segment) defined path while keeping its canonical plan identity (t100030).
    id: Option<&'static str>,
    procs: Vec<ProcSig>,
    prelude: Vec<AliasFn>,
    caps: Capabilities,
    schema: Schema,
    pushdown: PushdownProfile,
    applier: PanicApplier,
    /// When set, the driver declares its writes irreversible (models a declared MAP marked
    /// IRREVERSIBLE) so the planner ORs the bit onto the effect node.
    irreversible_writes: bool,
    /// When set, the driver's `plan_call` rejects every CALL at plan time (models the Gmail
    /// `mail.send` guard) — proving the evaluator consults `plan_call` before building the node.
    reject_calls: bool,
}

impl TestDriver {
    fn new(mount: &'static str) -> Self {
        Self {
            mount,
            id: None,
            procs: Vec::new(),
            prelude: Vec::new(),
            caps: Capabilities::none(),
            schema: Schema::empty(),
            pushdown: PushdownProfile::None,
            applier: PanicApplier,
            irreversible_writes: false,
            reject_calls: false,
        }
    }
    /// Make `plan_call` reject every CALL at plan time (the Gmail `mail.send` guard shape).
    fn with_call_rejection(mut self) -> Self {
        self.reject_calls = true;
        self
    }
    /// Pin a canonical id() that differs from the mount (the `/work/orders` → `postgres` case).
    fn with_id(mut self, id: &'static str) -> Self {
        self.id = Some(id);
        self
    }
    fn with_procs(mut self, procs: Vec<ProcSig>) -> Self {
        self.procs = procs;
        self
    }
    fn with_prelude(mut self, prelude: Vec<AliasFn>) -> Self {
        self.prelude = prelude;
        self
    }
    fn with_caps(mut self, caps: Capabilities) -> Self {
        self.caps = caps;
        self
    }
    fn with_schema(mut self, schema: Schema) -> Self {
        self.schema = schema;
        self
    }
    fn with_irreversible_writes(mut self) -> Self {
        self.irreversible_writes = true;
        self
    }
}

impl Driver for TestDriver {
    fn mount(&self) -> &str {
        self.mount
    }
    fn id(&self) -> DriverId {
        // Canonical id when pinned (a defined path decouples id() from the mount); else the default
        // derives it from the mount (strip the leading `/`).
        match self.id {
            Some(id) => DriverId::new(id),
            None => DriverId::new(self.mount.strip_prefix('/').unwrap_or(self.mount)),
        }
    }
    fn describe(&self, _p: &Path) -> Result<NodeDesc, CfsError> {
        Ok(NodeDesc::new(
            Archetype::RelationalTable,
            self.schema.clone(),
        ))
    }
    fn capabilities(&self, _p: &Path) -> Capabilities {
        self.caps
    }
    fn procedures(&self) -> &[ProcSig] {
        &self.procs
    }
    fn pushdown(&self) -> &PushdownProfile {
        &self.pushdown
    }
    fn prelude(&self) -> &[AliasFn] {
        &self.prelude
    }
    fn version_support(&self, _p: &Path) -> VersionSupport {
        VersionSupport::None
    }
    fn write_irreversible(&self, _p: &Path, _verb: Verb) -> bool {
        self.irreversible_writes
    }
    fn plan_call(
        &self,
        path: &Path,
        _proc: &str,
        _args: &qfs_types::RowBatch,
    ) -> Option<Result<(), CfsError>> {
        self.reject_calls.then(|| {
            Err(CfsError::InvalidPath {
                path: path.as_str().to_string(),
                reason: "test driver rejects this CALL at plan time",
            })
        })
    }
    fn applier(&self) -> &dyn PlanApplier {
        &self.applier
    }
}

/// `/mail` — append-log: select+insert, ships `SEND` → `mail.send` (irreversible), a
/// two-column schema. `/db` — relational: full CRUD, a three-column schema.
fn seeded_registry() -> MountRegistry {
    let mut reg = MountRegistry::new();
    reg.register(Arc::new(
        TestDriver::new("/mail")
            .with_procs(vec![ProcSig::new("send")
                .with_params(vec![Param::new("to", ColumnType::Text)])
                .irreversible(true)])
            .with_prelude(vec![AliasFn::new("SEND", "mail.send")])
            .with_caps(Capabilities::none().select().insert())
            .with_schema(Schema::new(vec![
                Column::new("id", ColumnType::Int, false),
                Column::new("subject", ColumnType::Text, true),
            ])),
    ))
    .unwrap();
    reg.register(Arc::new(
        TestDriver::new("/db")
            .with_caps(
                Capabilities::none()
                    .select()
                    .insert()
                    .upsert()
                    .update()
                    .remove(),
            )
            .with_schema(Schema::new(vec![
                Column::new("id", ColumnType::Int, false),
                Column::new("name", ColumnType::Text, true),
                Column::new("active", ColumnType::Bool, false),
            ])),
    ))
    .unwrap();
    reg
}

fn eval(src: &str) -> Result<EvalValue, EvalError> {
    let reg = seeded_registry();
    let stmt = parse_statement(src).expect("parse");
    Evaluator::new(&reg).eval(&stmt)
}

// ---- §15 TRANSFORM schema fold (decision W) ----

/// A seeded registry with one resolved transform definition installed.
fn registry_with_transform(name: &str, input: Schema, output: Schema) -> MountRegistry {
    let mut reg = seeded_registry();
    let mut defs = qfs_types::TransformDefs::new();
    defs.insert(
        name.to_string(),
        qfs_types::ResolvedTransform::new(input, output).unwrap(),
    );
    reg.set_transform_defs(defs);
    reg
}

#[test]
fn transform_folds_to_output_schema_so_downstream_stages_typecheck() {
    // A mid-pipe transform exposes its OUTPUT schema to the stages after it: `WHERE label`/
    // `SELECT label, score` type-check against the definition's OUTPUT, not the source's columns.
    // A transform-bearing statement is EFFECT-BEARING (§15: it spends tokens), so `eval` now
    // yields a Plan — the observable proof of the fold is that OUTPUT refs downstream SUCCEED
    // while a SOURCE column ref downstream FAILS (the source columns are gone after the stage).
    let reg = registry_with_transform(
        "classify",
        Schema::new(vec![Column::new("name", ColumnType::Text, true)]),
        Schema::new(vec![
            Column::new("label", ColumnType::Text, true),
            Column::new("score", ColumnType::Float, true),
        ]),
    );
    // OUTPUT columns downstream type-check ⇒ Ok (a Plan with the transform consent node).
    let ok = parse_statement(
        "/db/users |> transform classify |> WHERE label == 'x' |> SELECT label, score",
    )
    .unwrap();
    let v = Evaluator::new(&reg).eval(&ok).unwrap();
    let plan = v
        .as_plan()
        .expect("a transform statement is effect-bearing");
    assert!(
        plan.nodes()
            .iter()
            .any(|n| matches!(&n.kind, qfs_plan::EffectKind::Call(p) if p.as_str() == "transform.classify")),
        "the plan carries the transform consent node"
    );
    // A downstream PROJECTION of a SOURCE column (`active`) is gone after the stage ⇒ a type
    // error, proving the relation IS the OUTPUT (not the source's columns).
    let bad = parse_statement("/db/users |> transform classify |> SELECT active").unwrap();
    assert!(Evaluator::new(&reg).eval(&bad).is_err());
}

#[test]
fn transform_ignores_surplus_incoming_columns() {
    // /db/users has id/name/active; the definition declares only `name` — surplus columns are fine.
    let reg = registry_with_transform(
        "classify",
        Schema::new(vec![Column::new("name", ColumnType::Text, true)]),
        Schema::new(vec![Column::new("label", ColumnType::Text, true)]),
    );
    let stmt = parse_statement("/db/users |> transform classify").unwrap();
    // Effect-bearing now: a Plan, not a relation. The point is it PLANS (no error).
    assert!(Evaluator::new(&reg)
        .eval(&stmt)
        .unwrap()
        .as_plan()
        .is_some());
}

#[test]
fn transform_statement_is_effect_bearing_with_an_irreversible_consent_node() {
    // §15 routing: a `|> transform` read is reclassified as effect-bearing — the model call spends
    // tokens and is non-deterministic. The consent node is irreversible (gated at COMMIT) and
    // carries the spend-legibility metadata (provider/model/mode) from the definition, never a
    // secret. PREVIEW shows it without any model call.
    let reg = registry_with_transform(
        "classify",
        Schema::new(vec![Column::new("name", ColumnType::Text, true)]),
        Schema::new(vec![Column::new("label", ColumnType::Text, true)]),
    );
    let stmt = parse_statement("/db/users |> transform classify").unwrap();
    let value = Evaluator::new(&reg).eval(&stmt).unwrap();
    let plan = value.as_plan().unwrap();
    let consent = plan
        .nodes()
        .iter()
        .find(|n| matches!(&n.kind, qfs_plan::EffectKind::Call(p) if p.as_str() == "transform.classify"))
        .expect("consent node present");
    assert!(consent.irreversible, "a model call is irreversible");
    // The args carry the spend-legibility row (no secret column).
    assert!(consent.args.schema.column("model").is_some());
    assert!(consent.args.schema.column("mode").is_some());
    assert!(consent.args.schema.column("secret").is_none());
    assert!(consent.args.schema.column("secret_ref").is_none());
}

#[test]
fn transform_missing_declared_input_column_is_a_plan_time_error() {
    // The definition needs `body`, but /db/users has no `body` column ⇒ a structured plan error.
    let reg = registry_with_transform(
        "needs_body",
        Schema::new(vec![Column::new("body", ColumnType::Text, true)]),
        Schema::new(vec![Column::new("label", ColumnType::Text, true)]),
    );
    let stmt = parse_statement("/db/users |> transform needs_body").unwrap();
    let err = Evaluator::new(&reg).eval(&stmt).unwrap_err();
    assert_eq!(err.code(), "transform_input_missing");
}

#[test]
fn an_unresolved_transform_is_a_structured_error() {
    // No definitions installed (the plain `eval` registry) ⇒ an honest "not executable", never a
    // silent passthrough.
    let err = eval("/db/users |> transform missing").unwrap_err();
    assert_eq!(err.code(), "transform_not_executable");
}

// ---- §5.6 `of <type>` use-site assertion (structural, plan time) ----

/// A seeded registry with one resolved declared type installed under `/type/<name>`.
fn registry_with_declared_type(name: &str, body_json: &str) -> MountRegistry {
    let mut reg = seeded_registry();
    let resolved =
        crate::ddl::types::resolve_type_def(body_json, |_| None).expect("type body resolves");
    let mut defs = crate::DeclaredTypeDefs::new();
    defs.insert(format!("/type/{name}"), resolved);
    reg.set_declared_types(defs);
    reg
}

#[test]
fn of_inline_matching_schema_passes_as_identity() {
    // `/db/users` is id:int, name:text, active:bool. An inline `of (…)` that names exactly those
    // columns and types is a plan-time PASS and schema-identity — the relation flows unchanged, so
    // a downstream projection of every column still type-checks.
    let stmt = parse_statement(
        "/db/users |> of (id int, name text, active bool) |> SELECT id, name, active",
    )
    .unwrap();
    assert!(eval("/db/users").is_ok());
    assert!(Evaluator::new(&seeded_registry()).eval(&stmt).is_ok());
}

#[test]
fn of_inline_unexpected_column_is_a_structured_error() {
    // Asserting a SUBSET (missing `active`) is a mismatch: `active` is an unexpected relation column
    // the asserted type does not declare. `of` never coerces — it is exact structural equality.
    let stmt = parse_statement("/db/users |> of (id int, name text)").unwrap();
    let err = Evaluator::new(&seeded_registry()).eval(&stmt).unwrap_err();
    assert_eq!(err.code(), "of_assertion_failed");
}

#[test]
fn of_inline_missing_column_is_a_structured_error() {
    // Asserting a SUPERSET (an `extra` column /db/users lacks) is a mismatch naming the missing col.
    let stmt =
        parse_statement("/db/users |> of (id int, name text, active bool, extra text)").unwrap();
    let err = Evaluator::new(&seeded_registry()).eval(&stmt).unwrap_err();
    assert_eq!(err.code(), "of_assertion_failed");
}

#[test]
fn of_inline_type_mismatch_is_a_structured_error() {
    // Same column set, wrong concrete type (`id` asserted `text`, the relation carries `int`) ⇒ a
    // structural mismatch. Both sides are concretely known, so it is NOT conservatively skipped.
    let stmt = parse_statement("/db/users |> of (id text, name text, active bool)").unwrap();
    let err = Evaluator::new(&seeded_registry()).eval(&stmt).unwrap_err();
    assert_eq!(err.code(), "of_assertion_failed");
}

#[test]
fn of_named_unresolved_is_a_structured_error() {
    // No declared types installed ⇒ a named `of` is an honest "unresolved type", never a silent pass
    // (the twin of `transform_not_executable`).
    let err = eval("/db/users |> of customer").unwrap_err();
    assert_eq!(err.code(), "of_type_unresolved");
}

#[test]
fn of_named_matching_declared_type_passes() {
    // A declared type whose structural columns match `/db/users` resolves from the plan-time
    // registry and its assertion passes as schema-identity.
    let body = r#"{"columns":[
        {"name":"id","type":"int","nullable":false,"primary_key":false,"unique":false},
        {"name":"name","type":"text","nullable":true,"primary_key":false,"unique":false},
        {"name":"active","type":"bool","nullable":false,"primary_key":false,"unique":false}
    ],"where":null}"#;
    let reg = registry_with_declared_type("dbrow", body);
    let stmt = parse_statement("/db/users |> of dbrow |> SELECT id").unwrap();
    assert!(Evaluator::new(&reg).eval(&stmt).is_ok());
}

#[test]
fn of_named_mismatching_declared_type_is_a_structured_error() {
    // A declared type that declares a column `/db/users` does not carry ⇒ a structural mismatch,
    // proving the named path runs the SAME structural check as the inline path.
    let body = r#"{"columns":[
        {"name":"id","type":"int","nullable":false,"primary_key":false,"unique":false},
        {"name":"email","type":"text","nullable":true,"primary_key":false,"unique":false}
    ],"where":null}"#;
    let reg = registry_with_declared_type("wrongshape", body);
    let stmt = parse_statement("/db/users |> of wrongshape").unwrap();
    let err = Evaluator::new(&reg).eval(&stmt).unwrap_err();
    assert_eq!(err.code(), "of_assertion_failed");
}

// ---- §18 SWITCH routing ----

/// A registry with a `triage` transform (`name → route`) for switch tests: /db has
/// select+insert+upsert+update+remove, /mail has select+insert plus the irreversible
/// `mail.send` proc.
fn switch_registry() -> MountRegistry {
    registry_with_transform(
        "triage",
        Schema::new(vec![Column::new("name", ColumnType::Text, true)]),
        Schema::new(vec![
            Column::new("id", ColumnType::Int, false),
            Column::new("route", ColumnType::Text, false),
        ]),
    )
}

fn switch_eval(src: &str) -> Result<EvalValue, EvalError> {
    let reg = switch_registry();
    let stmt = parse_statement(src).expect("parse");
    Evaluator::new(&reg).eval(&stmt)
}

#[test]
fn switch_statement_plans_the_union_of_every_arm() {
    // §18-C(3): PREVIEW is model-free, so the statement's declared effect set is the UNION of
    // every arm's effects — one write node per write arm (plus its Read marker) and one Call
    // node for the CALL arm, sequenced in declaration order, all present BEFORE any model runs.
    let value = switch_eval(
        "/db/users |> transform triage \
           |> switch route { \
                'urgent' => INSERT INTO /mail/outbox, \
                'note'   => select id |> UPSERT INTO /db/notes, \
                else     => CALL mail.send(to => 'ops@example.com') \
              }",
    )
    .unwrap();
    let plan = value
        .as_plan()
        .expect("a switch statement is effect-bearing");
    let kinds: Vec<&qfs_plan::EffectKind> = plan
        .nodes()
        .iter()
        .filter(|n| !matches!(n.kind, qfs_plan::EffectKind::Read))
        .map(|n| &n.kind)
        .collect();
    // Consent node (the transform) + Insert + Upsert + Call(mail.send), in declaration order.
    assert_eq!(kinds.len(), 4, "consent + three arm effects: {kinds:?}");
    assert!(
        matches!(kinds[0], qfs_plan::EffectKind::Call(p) if p.as_str() == "transform.triage"),
        "the §15 consent node still rides in front"
    );
    assert!(matches!(kinds[1], qfs_plan::EffectKind::Insert));
    assert!(matches!(kinds[2], qfs_plan::EffectKind::Upsert));
    assert!(
        matches!(kinds[3], qfs_plan::EffectKind::Call(p) if p.as_str() == "mail.send"),
        "the else arm's terminal CALL is previewed too"
    );
    // The CALL arm's irreversibility rides the union: the whole plan is gated.
    assert!(plan.is_irreversible(), "mail.send is irreversible");
}

#[test]
fn switch_without_transform_still_plans_the_arm_union() {
    // The switch stage itself needs no model stage upstream — any text column routes.
    let value = switch_eval(
        "/db/users |> switch name { 'a' => INSERT INTO /db/a, else => INSERT INTO /db/b }",
    )
    .unwrap();
    let plan = value.as_plan().expect("effect-bearing");
    let writes = plan
        .nodes()
        .iter()
        .filter(|n| matches!(n.kind, qfs_plan::EffectKind::Insert))
        .count();
    assert_eq!(writes, 2, "both arms' writes are previewed");
}

#[test]
fn switch_requires_a_trailing_else_arm() {
    let err = switch_eval("/db/users |> switch name { 'a' => INSERT INTO /db/a }").unwrap_err();
    assert_eq!(err.code(), "switch_shape");
    let err = switch_eval(
        "/db/users |> switch name { else => INSERT INTO /db/b, 'a' => INSERT INTO /db/a }",
    )
    .unwrap_err();
    assert_eq!(err.code(), "switch_shape");
}

#[test]
fn switch_duplicate_label_is_a_shape_error() {
    let err = switch_eval(
        "/db/users |> switch name { 'a' => INSERT INTO /db/a, 'a' => INSERT INTO /db/b, \
         else => INSERT INTO /db/c }",
    )
    .unwrap_err();
    assert_eq!(err.code(), "switch_shape");
}

#[test]
fn switch_unknown_discriminant_names_the_available_columns() {
    let err = switch_eval(
        "/db/users |> switch missing { 'a' => INSERT INTO /db/a, else => INSERT INTO /db/b }",
    )
    .unwrap_err();
    let EvalError::SwitchDiscriminantUnknown { column, available } = err else {
        panic!("expected SwitchDiscriminantUnknown, got {err:?}")
    };
    assert_eq!(column, "missing");
    assert!(available.contains(&"name".to_string()));
}

#[test]
fn switch_mid_pipe_is_a_structured_error() {
    let err = switch_eval("/db/users |> switch name { else => INSERT INTO /db/b } |> LIMIT 1")
        .unwrap_err();
    assert_eq!(err.code(), "switch_not_terminal");
}

#[test]
fn switch_pure_arm_is_rejected_this_slice() {
    // §18 typing rule: this slice implements effect-routing only; an arm with no write and no
    // terminal effect CALL is a structured refusal naming the arm.
    let err =
        switch_eval("/db/users |> switch name { 'a' => select id, else => INSERT INTO /db/b }")
            .unwrap_err();
    let EvalError::SwitchArmNotEffect { label } = err else {
        panic!("expected SwitchArmNotEffect, got {err:?}")
    };
    assert_eq!(label, "a");
}

#[test]
fn switch_arm_vocabulary_is_row_local() {
    // A JOIN inside an arm is outside the routed row-local vocabulary (§18 slice).
    let err = switch_eval(
        "/db/users |> switch name { \
           'a' => JOIN /db/other ON id == id |> INSERT INTO /db/a, \
           else => INSERT INTO /db/b }",
    )
    .unwrap_err();
    let EvalError::SwitchArmOpUnsupported { label, op } = err else {
        panic!("expected SwitchArmOpUnsupported, got {err:?}")
    };
    assert_eq!(label, "a");
    assert_eq!(op, "join");
}

#[test]
fn switch_arm_write_is_capability_gated_at_resolve() {
    // /mail supports select+insert only — an UPSERT arm into /mail is denied at the resolve
    // gate, before any plan exists (§6 posture), exactly like a top-level UPSERT.
    let err = switch_eval(
        "/db/users |> switch name { 'a' => UPSERT INTO /mail/outbox, \
         else => INSERT INTO /db/b }",
    )
    .unwrap_err();
    assert!(
        matches!(
            err,
            EvalError::Resolve(ResolveError::UnsupportedVerb { .. })
        ),
        "expected a resolve-stage capability denial, got {err:?}"
    );
}

#[test]
fn switch_in_a_read_only_context_is_a_structured_error() {
    // A switch inside an effect body's source pipeline folds through fold_op → terminal-only.
    let err =
        switch_eval("INSERT INTO /db/copy /db/users |> switch name { else => INSERT INTO /db/b }")
            .unwrap_err();
    assert_eq!(err.code(), "switch_not_terminal");
}

/// True if `value`'s plan carries a `transform.*` model-call / consent effect node — the seam a
/// `|> transform` stage emits. A pure relation (no plan) carries no effect node at all.
fn carries_transform_node(value: &EvalValue) -> bool {
    match value.as_plan() {
        Some(plan) => plan.nodes().iter().any(|n| {
            matches!(&n.kind, qfs_plan::EffectKind::Call(p) if p.as_str().starts_with("transform."))
        }),
        None => false,
    }
}

#[test]
fn transform_is_the_only_model_call_seam() {
    // §15 GOVERNANCE LOCK: the `|> transform` stage is the ONLY seam that plans a model call. A
    // spread of non-transform statements — a read, a write, an effect CALL, codec DECODE/ENCODE,
    // and DDL (CREATE TABLE / CREATE TYPE) — must each carry NEITHER a `collect_transform_names`
    // hit NOR a `transform.*` consent/model-call effect node; the single transform-bearing
    // statement is the only one that does. This test fails the instant a future change routes a
    // model call through any non-transform stage. Pure: plan SHAPE only — the `PanicApplier`
    // guarding `/db` and `/mail` proves no I/O and no model/network call occurs.
    let reg = registry_with_transform(
        "triage",
        Schema::new(vec![Column::new("name", ColumnType::Text, true)]),
        Schema::new(vec![Column::new("label", ColumnType::Text, true)]),
    );

    // Each parses, resolves, and evaluates in the seeded registry. The DDL forms desugar to an
    // (unrouted) catalog INSERT — still a plain effect plan, still no model call.
    let non_transform = [
        "/db/users",                                         // a read
        "INSERT INTO /db/users VALUES (1, 'a', true)",       // a write
        "/mail/inbox |> CALL mail.send(to => 'a@b.c')",      // an effect CALL
        "/db/users |> ENCODE csv",                           // a codec ENCODE
        "/mail/inbox |> DECODE json",                        // a codec DECODE
        "CREATE TABLE /sql/main/orders (id int, note text)", // DDL: relational definition
        "CREATE TYPE email (value text)",                    // DDL: declared type
    ];
    for src in non_transform {
        let stmt = parse_statement(src).unwrap_or_else(|e| panic!("parse `{src}`: {e:?}"));
        // 1. The whole-tree AST walk finds NO transform stage.
        assert!(
            collect_transform_names(&stmt).is_empty(),
            "`{src}` must carry no transform stage"
        );
        // 2. The built plan carries NO `transform.*` model-call / consent effect node.
        let value = Evaluator::new(&reg)
            .eval(&stmt)
            .unwrap_or_else(|e| panic!("eval `{src}`: {e:?}"));
        assert!(
            !carries_transform_node(&value),
            "`{src}` must carry no transform effect node"
        );
    }

    // The ONE transform-bearing statement carries EXACTLY the seam: a collected name AND the
    // consent/model-call node.
    let stmt = parse_statement("/db/users |> transform triage").unwrap();
    assert_eq!(
        collect_transform_names(&stmt),
        vec!["triage".to_string()],
        "the transform stage is collected"
    );
    let value = Evaluator::new(&reg).eval(&stmt).unwrap();
    assert!(
        carries_transform_node(&value),
        "the transform statement carries the one model-call seam node"
    );
}

// ---- Query pipeline → relation + schema threading ----

#[test]
fn query_pipeline_folds_to_relation_with_threaded_schema() {
    let v = eval("/db/users |> WHERE active == true |> SELECT id, name").unwrap();
    let rel = v.as_relation().expect("a query yields a relation");
    // The fold produced a Project at the top, threading the source schema down.
    assert!(matches!(rel, PlanSource::Project { .. }));
    let schema = rel.schema();
    assert_eq!(
        schema.column_names(),
        vec!["id".to_string(), "name".to_string()]
    );
    // The projected `id` keeps its real type from describe (Int), not Unknown — the
    // schema threaded from the source through the filter into the projection.
    assert_eq!(schema.column("id").unwrap().ty, ColumnType::Int);
}

#[test]
fn select_star_preserves_full_schema() {
    let v = eval("/db/users |> SELECT *").unwrap();
    let schema = v.as_relation().unwrap().schema();
    assert_eq!(
        schema.columns.len(),
        3,
        "SELECT * keeps every source column"
    );
}

#[test]
fn extend_adds_a_column_to_the_threaded_schema() {
    let v = eval("/db/users |> EXTEND label = name").unwrap();
    let schema = v.as_relation().unwrap().schema();
    assert!(schema.column("label").is_some(), "EXTEND added the column");
    assert_eq!(schema.columns.len(), 4);
}

// ---- Each effect verb → correct EffectKind node ----

#[test]
fn each_effect_verb_maps_to_its_kind() {
    // The canonical EffectVerb → WriteVerb → EffectKind pipeline (no `_` arm).
    use qfs_parser::EffectVerb;
    assert_eq!(effect_kind_for(EffectVerb::Insert), EffectKind::Insert);
    assert_eq!(effect_kind_for(EffectVerb::Upsert), EffectKind::Upsert);
    assert_eq!(effect_kind_for(EffectVerb::Update), EffectKind::Update);
    assert_eq!(effect_kind_for(EffectVerb::Remove), EffectKind::Remove);
}

#[test]
fn insert_evaluates_to_an_insert_node() {
    let plan = eval("INSERT INTO /db/users VALUES (1, 'a', true)")
        .unwrap()
        .as_plan()
        .cloned()
        .unwrap();
    assert_eq!(plan.nodes().len(), 1);
    assert_eq!(plan.nodes()[0].kind, EffectKind::Insert);
    // A literal VALUES of one row is an exact affected estimate (honest preview).
    assert_eq!(plan.nodes()[0].est_affected, Affected::Exact(1));
    plan.validate().expect("a single-node plan is a valid DAG");
}

#[test]
fn upsert_and_update_evaluate_to_their_kinds() {
    let upsert = eval("UPSERT INTO /db/users VALUES (1, 'a', true)")
        .unwrap()
        .as_plan()
        .cloned()
        .unwrap();
    assert_eq!(upsert.nodes()[0].kind, EffectKind::Upsert);

    let update = eval("UPDATE /db/users SET name = 'b' WHERE id == 1")
        .unwrap()
        .as_plan()
        .cloned()
        .unwrap();
    assert_eq!(update.nodes()[0].kind, EffectKind::Update);
}

#[test]
fn update_lowers_the_where_into_the_selector_channel() {
    // Blueprint §7 / ticket 20260713195008: the `WHERE` is carried on the effect's `selector`
    // channel distinct from the SET payload, so a **same-column** `SET name … WHERE name …` survives
    // to the applier — `args` de-dups the WHERE key that shares the SET column, but `selector` keeps
    // it with its own value.
    let plan = eval("UPDATE /db/users SET name = 'new' WHERE name == 'old'")
        .unwrap()
        .as_plan()
        .cloned()
        .unwrap();
    let node = &plan.nodes()[0];
    let sel = node
        .selector
        .as_ref()
        .expect("a WHERE lowers into the selector channel");
    assert_eq!(sel.schema.columns.len(), 1, "one WHERE key");
    assert_eq!(sel.schema.columns[0].name, "name");
    assert_eq!(
        sel.rows[0].values[0],
        qfs_types::Value::Text("old".into()),
        "the selector keeps the WHERE value, distinct from the SET value"
    );
    // The SET payload on `args` keeps the NEW value on the same column.
    let set_idx = node
        .args
        .schema
        .columns
        .iter()
        .position(|c| c.name == "name")
        .expect("SET name is on args");
    assert_eq!(
        node.args.rows[0].values[set_idx],
        qfs_types::Value::Text("new".into())
    );
}

#[test]
fn the_where_lives_only_on_the_selector_never_on_args() {
    // Increment 2 (blueprint §7): the lowering is now UNIFORM — `args` is purely the SET/VALUES
    // payload and the `WHERE` lives ONLY on the selector. Increment 1 populated BOTH (the WHERE was
    // appended to `args`, deduped), which is the two-convention state this retires: an applier could
    // read a match key out of `args` and appear to work, until a same-column SET/WHERE silently
    // dropped it. This test is the governance floor — if `args` ever regains a WHERE key, it fails.
    let plan = eval("UPDATE /db/users SET name = 'new' WHERE id == 1")
        .unwrap()
        .as_plan()
        .cloned()
        .unwrap();
    let node = &plan.nodes()[0];
    let arg_cols: Vec<&str> = node
        .args
        .schema
        .columns
        .iter()
        .map(|c| c.name.as_str())
        .collect();
    assert_eq!(
        arg_cols,
        vec!["name"],
        "args must carry the SET payload ONLY — the WHERE key `id` must not appear"
    );
    assert!(
        node.selector_value("id").is_some(),
        "the WHERE key rides the selector"
    );
}

#[test]
fn a_remove_carries_an_empty_args_and_a_populated_selector() {
    // A REMOVE writes nothing, so once the WHERE moves off `args` there is no payload left at all —
    // the selector is the whole story. Every applier's REMOVE path reads it from there.
    let plan = eval("REMOVE /db/users WHERE id == 1")
        .unwrap()
        .as_plan()
        .cloned()
        .unwrap();
    let node = &plan.nodes()[0];
    assert!(
        node.args.schema.columns.is_empty() && node.args.rows.is_empty(),
        "a REMOVE writes nothing: its args must be EMPTY, got {:?}",
        node.args
    );
    assert!(
        node.selector_value("id").is_some(),
        "the REMOVE's match rides the selector"
    );
}

#[test]
fn insert_from_query_emits_a_read_dependency() {
    // `INSERT INTO /db/users /mail/inbox |> SELECT id, subject` should build a Read
    // node (the sub-pipeline) the Insert depends on — the plan DAG with a dependency edge.
    let plan = eval("INSERT INTO /db/users /mail/inbox |> SELECT id, subject")
        .unwrap()
        .as_plan()
        .cloned()
        .unwrap();
    assert_eq!(plan.nodes().len(), 2, "a Read dep + the Insert");
    assert!(plan.nodes().iter().any(|n| n.kind == EffectKind::Read));
    assert!(plan.nodes().iter().any(|n| n.kind == EffectKind::Insert));
    assert_eq!(plan.deps().len(), 1, "the Insert depends on the Read");
    plan.validate().expect("valid DAG with one edge");
}

/// t100030: a driver CONNECTed at a MULTI-SEGMENT defined path (`/work/orders`) with a CANONICAL
/// id() (`postgres`) — the write plan's Target is REBUILT as `/<id>/<sub>` (`/postgres/rows`), the
/// per-driver path the driver's own parser expects, NOT the user mount. This is the reconstruction
/// the `/ga` alias proved for a single segment, now exercised for a multi-segment user path (the
/// route → reconstruct coverage the epic calls for).
#[test]
fn effect_target_reconstructs_canonical_id_under_a_multi_segment_defined_path() {
    let mut reg = MountRegistry::new();
    reg.register(Arc::new(
        TestDriver::new("/work/orders")
            .with_id("postgres")
            .with_caps(Capabilities::none().select().insert())
            .with_schema(Schema::new(vec![Column::new("id", ColumnType::Int, false)])),
    ))
    .unwrap();
    let stmt = parse_statement("INSERT INTO /work/orders/rows VALUES (1)").expect("parse");
    let plan = Evaluator::new(&reg)
        .eval(&stmt)
        .unwrap()
        .as_plan()
        .cloned()
        .unwrap();
    let node = &plan.nodes()[0];
    assert_eq!(node.kind, EffectKind::Insert);
    // Routed by the multi-segment mount, but the driver-facing Target is canonical.
    assert_eq!(node.target.driver, DriverId::new("postgres"));
    assert_eq!(node.target.path.as_str(), "/postgres/rows");
}

// ---- Irreversible flagging ----

#[test]
fn remove_is_flagged_inherently_irreversible() {
    let plan = eval("REMOVE /db/users WHERE id == 1")
        .unwrap()
        .as_plan()
        .cloned()
        .unwrap();
    assert_eq!(plan.nodes()[0].kind, EffectKind::Remove);
    assert!(
        plan.nodes()[0].irreversible,
        "REMOVE is inherently irreversible"
    );
    assert!(plan.is_irreversible());
}

#[test]
fn insert_is_reversible() {
    let plan = eval("INSERT INTO /db/users VALUES (1, 'a', true)")
        .unwrap()
        .as_plan()
        .cloned()
        .unwrap();
    assert!(!plan.is_irreversible(), "INSERT is reversible");
}

#[test]
fn a_driver_declared_irreversible_write_gates_the_plan() {
    // A driver that declares its writes irreversible (a declared MAP marked IRREVERSIBLE onto an
    // external POST) makes even an INSERT plan irreversible, so PREVIEW surfaces it and COMMIT gates
    // it behind `--commit-irreversible` — exactly like a REMOVE.
    let mut reg = MountRegistry::new();
    reg.register(Arc::new(
        TestDriver::new("/wire")
            .with_caps(Capabilities::none().insert())
            .with_irreversible_writes()
            .with_schema(Schema::new(vec![Column::new("id", ColumnType::Int, false)])),
    ))
    .unwrap();
    let stmt = parse_statement("INSERT INTO /wire/post VALUES (1)").expect("parse");
    let plan = Evaluator::new(&reg)
        .eval(&stmt)
        .unwrap()
        .as_plan()
        .cloned()
        .unwrap();
    assert_eq!(plan.nodes()[0].kind, EffectKind::Insert);
    assert!(
        plan.nodes()[0].irreversible,
        "the driver-declared irreversible bit is ORed onto the INSERT node"
    );
    assert!(plan.is_irreversible());
}

// ---- Terminal CALL → Call effect plan (t: pipeline-call lowering) ----

/// A registry with a `/files` mount whose `copy` procedure is REVERSIBLE and takes two
/// declared params — the `drive.copy` shape, used to prove a non-irreversible pipeline CALL
/// lowers and is not gated.
fn call_registry() -> MountRegistry {
    let mut reg = MountRegistry::new();
    reg.register(Arc::new(
        TestDriver::new("/files")
            .with_procs(vec![ProcSig::new("copy").with_params(vec![
                Param::new("parent_id", ColumnType::Text),
                Param::new("name", ColumnType::Text),
            ])])
            .with_caps(Capabilities::none().select())
            .with_schema(Schema::new(vec![Column::new(
                "id",
                ColumnType::Text,
                false,
            )])),
    ))
    .unwrap();
    reg
}

#[test]
fn terminal_call_lowers_to_a_call_effect_node() {
    // A pipeline terminating in `|> CALL` is an EFFECT: it evaluates to a plan with one Call
    // node (no read dependency), NOT a read relation. This is the fix for the silent drop.
    let plan = eval("/mail/inbox |> CALL mail.send(to => 'a@b.c')")
        .unwrap()
        .as_plan()
        .cloned()
        .expect("a terminal CALL yields an effect plan, not a relation");
    assert_eq!(
        plan.nodes().len(),
        1,
        "a lone Call node, no read dependency"
    );
    let node = &plan.nodes()[0];
    assert_eq!(node.kind, EffectKind::Call(ProcId::new("mail.send")));
    // The call acts on the source path; the applier re-resolves the entity from it.
    assert_eq!(node.target.driver, DriverId::new("mail"));
    assert_eq!(node.target.path.as_str(), "/mail/inbox");
    // A call's affected count is not a relation row count — the estimate stays honest.
    assert_eq!(node.est_affected, Affected::Unknown);
    // The named arg is lowered into the node's row payload, keyed by its name for the driver.
    assert_eq!(node.args.schema.column_names(), vec!["to".to_string()]);
    assert_eq!(node.args.rows.len(), 1);
    assert_eq!(
        node.args.rows[0].values[0],
        Value::Text("a@b.c".to_string())
    );
}

#[test]
fn terminal_call_target_carries_the_full_addressed_source_path() {
    // The lowering the Gmail send-by-id fix depends on: `/mail/drafts/<id> |> call mail.send` must
    // build a Call node whose TARGET PATH is the full addressed source (`/mail/drafts/d42`), so the
    // driver's `decode_call` can recover the draft id from the path (a CALL's args are its literal
    // arguments, never upstream rows — the path is the only per-entity channel). No args needed.
    let plan = eval("/mail/drafts/d42 |> CALL mail.send")
        .unwrap()
        .as_plan()
        .cloned()
        .expect("a terminal CALL yields an effect plan");
    let node = &plan.nodes()[0];
    assert_eq!(node.kind, EffectKind::Call(ProcId::new("mail.send")));
    assert_eq!(node.target.path.as_str(), "/mail/drafts/d42");
}

#[test]
fn terminal_call_consults_the_driver_plan_call_guard_at_plan_time() {
    // A driver may reject a CALL at PLAN time via `plan_call` (the Gmail `mail.send` guard) — so a
    // PREVIEW (which never decodes the effect) refuses exactly what a COMMIT would. Prove the
    // evaluator consults it: a rejecting driver makes `eval` fail while building the plan.
    let mut reg = MountRegistry::new();
    reg.register(Arc::new(
        TestDriver::new("/mail")
            .with_procs(vec![ProcSig::new("send")
                .with_params(vec![Param::new("to", ColumnType::Text)])
                .irreversible(true)])
            .with_caps(Capabilities::none().select().insert())
            .with_schema(Schema::new(vec![Column::new("id", ColumnType::Int, false)]))
            .with_call_rejection(),
    ))
    .unwrap();
    let stmt = parse_statement("/mail/drafts |> CALL mail.send(to => 'x')").expect("parse");
    let err = Evaluator::new(&reg).eval(&stmt).unwrap_err();
    assert!(
        matches!(err, EvalError::DriverWrite { .. }),
        "a plan_call rejection surfaces at plan time: {err:?}"
    );
}

#[test]
fn terminal_call_carries_the_per_procedure_irreversible_flag() {
    // mail.send is declared irreversible → the node is flagged and the plan gates.
    let plan = eval("/mail/inbox |> CALL mail.send(to => 'a@b.c')")
        .unwrap()
        .as_plan()
        .cloned()
        .unwrap();
    assert!(plan.nodes()[0].irreversible, "mail.send is irreversible");
    assert!(plan.is_irreversible());

    // files.copy is NOT declared irreversible → the same lowering leaves it reversible.
    let reg = call_registry();
    let stmt =
        parse_statement("/files/report.pdf |> CALL files.copy(parent_id => 'p', name => 'n')")
            .expect("parse");
    let copy = Evaluator::new(&reg)
        .eval(&stmt)
        .unwrap()
        .as_plan()
        .cloned()
        .unwrap();
    assert_eq!(
        copy.nodes()[0].kind,
        EffectKind::Call(ProcId::new("files.copy"))
    );
    assert!(!copy.nodes()[0].irreversible, "files.copy is reversible");
    assert!(!copy.is_irreversible());
    // Both named args lower into the payload, keyed for the driver's column-keyed decode.
    assert_eq!(
        copy.nodes()[0].args.schema.column_names(),
        vec!["parent_id".to_string(), "name".to_string()]
    );
    assert_eq!(
        copy.nodes()[0].args.rows[0].values,
        vec![Value::Text("p".to_string()), Value::Text("n".to_string())]
    );
}

#[test]
fn positional_call_arg_is_named_by_its_declared_parameter() {
    // A positional argument must reach the applier under its declared parameter name (the
    // applier decodes by column name), so `mail.send('a@b.c')` names the cell `to`.
    let plan = eval("/mail/inbox |> CALL mail.send('a@b.c')")
        .unwrap()
        .as_plan()
        .cloned()
        .unwrap();
    let args = &plan.nodes()[0].args;
    assert_eq!(args.schema.column_names(), vec!["to".to_string()]);
    assert_eq!(args.rows[0].values[0], Value::Text("a@b.c".to_string()));
}

#[test]
fn terminal_call_previews_as_a_call_effect() {
    // The preview now shows the CALL honestly (the bug returned file rows with no preview).
    let plan = eval("/mail/inbox |> CALL mail.send(to => 'a@b.c')")
        .unwrap()
        .as_plan()
        .cloned()
        .unwrap();
    let pv = preview(&plan);
    assert!(!pv.is_pure, "a CALL plan is not pure");
    assert_eq!(pv.rows.len(), 1);
    // The preview verb carries the qualified proc name (preview renders `CALL <proc>`).
    assert_eq!(pv.rows[0].verb, "CALL mail.send");
    assert_eq!(
        pv.irreversible.len(),
        1,
        "mail.send is flagged irreversible"
    );
    assert!(pv.to_string().contains("CALL mail.send"));
}

#[test]
fn a_call_that_is_not_the_terminal_op_stays_a_read_relation() {
    // Only the TAIL call is an effect; a plain read pipeline (no terminal CALL) still folds
    // to a relation — the regression guard that this change does not turn reads into effects.
    let v = eval("/db/users |> SELECT id").unwrap();
    assert!(
        v.as_relation().is_some(),
        "a pipeline with no terminal CALL is still a relation"
    );
}

// ---- RETURNING schema ----

#[test]
fn returning_projection_schema_is_computed() {
    let plan = eval("INSERT INTO /db/users VALUES (1, 'a', true) RETURNING id, name")
        .unwrap()
        .as_plan()
        .cloned()
        .unwrap();
    let returning = plan.returning.as_ref().expect("a RETURNING schema");
    assert_eq!(
        returning.column_names(),
        vec!["id".to_string(), "name".to_string()]
    );
    // RETURNING types resolve against the target's described schema (id stays Int).
    assert_eq!(returning.column("id").unwrap().ty, ColumnType::Int);
}

// ---- PREVIEW of an evaluated plan ----

#[test]
fn preview_of_evaluated_plan_is_secret_free_and_ordered() {
    let plan = eval("REMOVE /db/users WHERE id == 1")
        .unwrap()
        .as_plan()
        .cloned()
        .unwrap();
    let pv = preview(&plan);
    assert!(!pv.is_pure, "a REMOVE plan is not pure");
    assert_eq!(pv.rows.len(), 1);
    assert_eq!(pv.rows[0].verb, "REMOVE");
    assert_eq!(
        pv.irreversible.len(),
        1,
        "the REMOVE is flagged irreversible"
    );
    // Rendered preview is deterministic, human-readable, secret-free.
    let rendered = pv.to_string();
    assert!(rendered.contains("REMOVE"));
    assert!(rendered.contains("irreversible"));
}

#[test]
fn preview_of_pure_query_has_no_effects() {
    // PREVIEW of a pure read evaluates to a relation; there is no plan to apply, so a
    // caller building a plan for it (the empty plan) previews as pure.
    let v = eval("PREVIEW /db/users |> SELECT id").unwrap();
    // A PREVIEW wrapper over a query is transparent: still a relation.
    assert!(v.as_relation().is_some());
}

// ---- Structured errors for ill-typed / unresolvable input ----

#[test]
fn unknown_column_in_projection_is_structured() {
    let err = eval("/db/users |> SELECT nope").unwrap_err();
    assert_eq!(err.code(), "unknown_column");
    assert!(matches!(
        err,
        EvalError::Type(qfs_types::TypeError::UnknownColumn { .. })
    ));
}

#[test]
fn capability_denied_verb_never_reaches_a_plan() {
    // /mail allows only select+insert; UPDATE is denied at resolve time — the evaluator
    // surfaces the structured resolve error, no plan is built.
    let err = eval("UPDATE /mail/inbox SET subject = 'x' WHERE id == 1").unwrap_err();
    assert_eq!(err.code(), "unsupported_verb");
    assert!(matches!(
        err,
        EvalError::Resolve(ResolveError::UnsupportedVerb { .. })
    ));
}

#[test]
fn unknown_procedure_is_structured() {
    let err = eval("/mail/inbox |> CALL mail.nuke()").unwrap_err();
    assert_eq!(err.code(), "unknown_procedure");
    assert!(matches!(
        err,
        EvalError::Resolve(ResolveError::UnknownProcedure { .. })
    ));
}

#[test]
fn ambiguous_alias_is_structured() {
    // Two drivers ship SEND, the receiver (/git) ships neither → ambiguous.
    let mut reg = MountRegistry::new();
    reg.register(Arc::new(
        TestDriver::new("/mail")
            .with_procs(vec![ProcSig::new("send")])
            .with_prelude(vec![AliasFn::new("SEND", "mail.send")]),
    ))
    .unwrap();
    reg.register(Arc::new(
        TestDriver::new("/sms")
            .with_procs(vec![ProcSig::new("send")])
            .with_prelude(vec![AliasFn::new("SEND", "sms.send")]),
    ))
    .unwrap();
    reg.register(Arc::new(
        TestDriver::new("/git")
            .with_procs(vec![ProcSig::new("merge")])
            .with_caps(Capabilities::none().select()),
    ))
    .unwrap();
    let stmt = parse_statement("/git/repo |> WHERE SEND()").unwrap();
    let err = Evaluator::new(&reg).eval(&stmt).unwrap_err();
    assert_eq!(err.code(), "ambiguous_alias");
}

// ---- SEND alias in a predicate (golden) ----

#[test]
fn send_alias_in_predicate_folds_to_a_schema_preserving_relation() {
    // `/mail/inbox |> WHERE SEND()` resolves (the receiver ships SEND, desugaring to
    // mail.send — proven at the resolve layer) and folds to a schema-preserving relation
    // over the scan, identical to the bare scan's schema. (A *terminal* `|> CALL mail.send`
    // is an EFFECT, not a relation — covered by `terminal_call_lowers_to_a_call_effect_node`.)
    let aliased = eval("/mail/inbox |> WHERE SEND()").unwrap();
    let scan = eval("/mail/inbox").unwrap();
    assert_eq!(
        aliased.as_relation().unwrap().schema().column_names(),
        scan.as_relation().unwrap().schema().column_names(),
        "WHERE SEND() is a schema-preserving filter over the scan"
    );
}

// ---- Purity proof ----

#[test]
fn evaluation_performs_no_io() {
    // The seeded drivers hand back a PanicApplier; evaluating + previewing a write plan
    // never touches it. If the evaluator did I/O, the panic applier would fire.
    let plan = eval("INSERT INTO /db/users VALUES (1, 'a', true)")
        .unwrap()
        .as_plan()
        .cloned()
        .unwrap();
    let _ = preview(&plan);
    // No commit / apply is invoked anywhere in pure evaluation.
}

// ---- Error code distinctness ----

#[test]
fn error_codes_are_stable() {
    assert_eq!(
        EvalError::UnroutedPath { path: "/x".into() }.code(),
        "unrouted_path"
    );
    assert_eq!(
        EvalError::Type(qfs_types::TypeError::UnknownColumn {
            name: "c".into(),
            available: vec![],
        })
        .code(),
        "unknown_column"
    );
    assert_eq!(
        EvalError::Fn(crate::FnError::UnknownFunction { name: "x".into() }).code(),
        "unknown_function"
    );
}

// ---- t08: function-registry-wired evaluation (`Evaluator::with_stdlib`) ----

/// Evaluate with the function registry wired so `fn(...)` projections are typed.
fn eval_with_stdlib(src: &str) -> Result<EvalValue, EvalError> {
    let reg = seeded_registry();
    let stdlib = crate::StdlibRegistry::with_core();
    let stmt = parse_statement(src).expect("parse");
    Evaluator::with_stdlib(&reg, &stdlib).eval(&stmt)
}

/// A `SELECT fn(col)` projection carries the **built-in's declared return type** (t08),
/// not the late-bound `Unknown` of t07 — `UPPER` → `Text`, `LENGTH` → `Int`.
#[test]
fn select_over_builtin_carries_the_functions_return_type() {
    let v = eval_with_stdlib("/db/users |> SELECT UPPER(name) AS u, LENGTH(name) AS n").unwrap();
    let schema = v.as_relation().unwrap().schema();
    assert_eq!(schema.column("u").unwrap().ty, ColumnType::Text);
    assert_eq!(schema.column("n").unwrap().ty, ColumnType::Int);
}

/// An unknown `fn(...)` in a projection is a structured [`EvalError::Fn`] (not a silent
/// `Unknown` column) once the registry is wired.
#[test]
fn select_over_unknown_function_is_a_structured_error() {
    let err = eval_with_stdlib("/db/users |> SELECT NOPE(name) AS x").unwrap_err();
    assert_eq!(err.code(), "unknown_function");
}

/// Aggregate-vs-scalar dispatch (blueprint §3): an aggregate (`SUM`) in a plain `SELECT` is a
/// typed error, while the same `SUM` under `AGGREGATE` types to its `Float` return.
#[test]
fn aggregate_dispatch_is_context_sensitive() {
    // SUM in a SELECT → aggregate-outside-aggregate, a typed error.
    let err = eval_with_stdlib("/db/users |> SELECT SUM(id) AS s").unwrap_err();
    assert_eq!(err.code(), "aggregate_outside_aggregate");
    // SUM under AGGREGATE → typed to Float, no error.
    let v = eval_with_stdlib("/db/users |> AGGREGATE SUM(id) AS s").unwrap();
    let schema = v.as_relation().unwrap().schema();
    assert_eq!(schema.column("s").unwrap().ty, ColumnType::Float);
}

/// Without the registry wired, a `fn(...)` projection stays late-bound (`Unknown`) — the
/// t07 behaviour is preserved for `Evaluator::new`.
#[test]
fn unwired_evaluator_keeps_functions_late_bound() {
    let v = eval("/db/users |> SELECT UPPER(name) AS u").unwrap();
    let schema = v.as_relation().unwrap().schema();
    assert_eq!(schema.column("u").unwrap().ty, ColumnType::Unknown);
}

// ---- t75: plan-time static primitive type checking ------------------------

/// A well-typed `WHERE` comparison type-checks at plan time and folds to a filtered relation.
#[test]
fn well_typed_where_checks_and_folds() {
    let v = eval_with_stdlib("/db/users |> WHERE id == 1 |> SELECT id").unwrap();
    assert!(v.as_relation().is_some(), "a well-typed pipeline folds");
}

/// A mismatched comparison (`name` is `Text`, compared to an `Int` literal) is a structured
/// **plan-time** error — surfaced from the pure fold, before any effect/relation is built.
#[test]
fn mismatched_where_comparison_is_a_plan_time_error() {
    let err = eval_with_stdlib("/db/users |> WHERE name == 1").unwrap_err();
    assert_eq!(err.code(), "incomparable_types");
    assert!(matches!(
        err,
        EvalError::Type(qfs_types::TypeError::IncomparableTypes { .. })
    ));
}

/// A built-in handed a statically-wrong argument type (`UPPER` of the `Int` column `id`) is a
/// structured plan-time error, not a runtime surprise.
#[test]
fn builtin_bad_arg_type_in_where_is_a_plan_time_error() {
    let err = eval_with_stdlib("/db/users |> WHERE UPPER(id) == 'x'").unwrap_err();
    assert_eq!(err.code(), "fn_type");
}

/// A type-failing plan **never reaches commit**: `COMMIT REMOVE … WHERE name == 1` compares a
/// `Text` key column to an `Int`, so evaluation fails at plan time and the `PanicApplier` (which
/// would fire on any I/O) is never reached — zero effects applied.
#[test]
fn type_failing_plan_never_reaches_commit() {
    let err = eval_with_stdlib("COMMIT REMOVE /db/users WHERE name == 1").unwrap_err();
    assert_eq!(err.code(), "incomparable_types");
    // The same REMOVE with a correctly-typed key plans (and would commit) cleanly.
    let plan = eval_with_stdlib("REMOVE /db/users WHERE id == 1")
        .unwrap()
        .as_plan()
        .cloned()
        .unwrap();
    assert_eq!(plan.nodes()[0].kind, EffectKind::Remove);
}

// ---- LET binding evaluation (M6, ticket t60) ------------------------------

/// A `LET`-bound name substitutes its folded relation in the body: the bound scan's real
/// schema threads through the body's projection (the `id` column keeps its `Int` type — an
/// empty/late-bound schema would not), proving the binding is the *same* relation, reused.
#[test]
fn let_substitutes_bound_relation_with_its_schema() {
    let v = eval(
        "LET u = /db/users\n\
         u |> SELECT id, name",
    )
    .unwrap();
    let rel = v
        .as_relation()
        .expect("a LET program ending in a query is a relation");
    assert!(matches!(rel, PlanSource::Project { .. }));
    let schema = rel.schema();
    assert_eq!(
        schema.column_names(),
        vec!["id".to_string(), "name".to_string()]
    );
    assert_eq!(
        schema.column("id").unwrap().ty,
        ColumnType::Int,
        "the bound relation's real schema threaded into the body"
    );
}

/// A `LET`-bound relation flows into a set operation in the body (referenced as a source on
/// both sides) — the binding is reusable, not single-use.
#[test]
fn let_bound_relation_is_reusable() {
    let v = eval(
        "LET u = /db/users\n\
         u |> UNION u",
    )
    .unwrap();
    assert!(matches!(v.as_relation().unwrap(), PlanSource::SetOp { .. }));
}

/// An unbound bare-identifier source fails evaluation with the structured `UnknownBinding`
/// resolve error (resolution runs first) — never a silent empty relation.
#[test]
fn unbound_name_fails_evaluation() {
    let err = eval("ghost |> SELECT id").unwrap_err();
    assert_eq!(err.code(), "unknown_binding");
}

/// Shadowing: the innermost binding wins. The body sees `/db/users` (3 columns), not the
/// outer `/mail/inbox` (2 columns), so `SELECT *` yields the inner relation's schema.
#[test]
fn shadowing_uses_the_innermost_binding() {
    let v = eval(
        "LET x = /mail/inbox\n\
         LET x = /db/users\n\
         x |> SELECT *",
    )
    .unwrap();
    let schema = v.as_relation().unwrap().schema();
    assert_eq!(
        schema.columns.len(),
        3,
        "the inner /db/users binding (3 cols) shadows the outer /mail/inbox (2 cols)"
    );
}

// ---- TRANSACTION block (M6, ticket t62): reversible-only + commit-point ordering ----

/// A transaction of two reversible effects lowers to ONE plan whose nodes carry a deterministic
/// commit-point ordering — the block's source order, recovered by `topo_order`.
#[test]
fn transaction_of_reversible_effects_plans_with_source_order() {
    let plan = eval(
        "TRANSACTION { \
           UPSERT INTO /db/users VALUES (1, 'a', true); \
           INSERT INTO /db/users VALUES (2, 'b', false) \
         }",
    )
    .unwrap()
    .as_plan()
    .cloned()
    .unwrap();
    assert_eq!(plan.nodes().len(), 2, "two effect nodes, one per member");
    assert!(
        !plan.is_irreversible(),
        "an all-UPSERT/INSERT block is reversible"
    );
    plan.validate()
        .expect("a sequenced transaction is a valid DAG");
    // Commit-point ordering: the topo walk yields the members in source order (the first
    // UPSERT before the second INSERT), because `then` made the second depend on the first.
    let order = qfs_plan::topo_order(&plan).expect("acyclic");
    let kinds: Vec<&qfs_plan::EffectKind> = order
        .iter()
        .map(|id| &plan.node(*id).unwrap().kind)
        .collect();
    assert_eq!(
        kinds,
        vec![&qfs_plan::EffectKind::Upsert, &qfs_plan::EffectKind::Insert],
        "commit-point order is the block's source order"
    );
    assert_eq!(
        plan.deps().len(),
        1,
        "the second effect depends on the first"
    );
}

/// An irreversible effect (`REMOVE`) inside a transaction is rejected at EVAL time with the
/// structured `IrreversibleInTransaction` error — before anything touches the world (the
/// `PanicApplier` is never reached, proving zero effects applied / full purity).
#[test]
fn irreversible_remove_in_transaction_is_rejected_at_plan_time() {
    let err = eval(
        "TRANSACTION { \
           UPSERT INTO /db/users VALUES (1, 'a', true); \
           REMOVE /db/users WHERE id == 2 \
         }",
    )
    .unwrap_err();
    assert_eq!(err.code(), "irreversible_in_transaction");
    let EvalError::IrreversibleInTransaction { effect } = err else {
        panic!("expected IrreversibleInTransaction, got {err:?}")
    };
    assert_eq!(
        effect, "REMOVE",
        "the offending effect is named for recovery"
    );
}

/// The reversible-only guard fires on the per-node `irreversible` flag too, not only the inherent
/// `REMOVE` classification — but note the canonical irreversible `CALL mail.send` lives OUTSIDE a
/// transaction (it is not an effect statement). A transaction of only reversible writes commits
/// atomically; the same writes plus an irreversible follow-up keep that follow-up out of the block.
#[test]
fn empty_transaction_is_a_reversible_empty_plan() {
    let plan = eval("TRANSACTION { }").unwrap().as_plan().cloned().unwrap();
    assert!(plan.nodes().is_empty(), "an empty block has no effects");
    assert!(!plan.is_irreversible());
}
