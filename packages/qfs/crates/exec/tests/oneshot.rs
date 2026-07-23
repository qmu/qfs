//! End-to-end tests for the t29 execution layer: the SELECT read-path executor (the t20
//! carry-over closure) and the one-shot CLI orchestration (PREVIEW/COMMIT gate, exit codes,
//! renderers). All drivers are in-memory fakes — **no live creds, no network**.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;

use qfs_core::{AppliedEffect, ApplyError, EffectNode, PlanApplier};
use qfs_core::{
    Archetype, Capabilities, CfsError, Column, ColumnType, DriverId, Engine, NodeDesc, Path,
    PushdownProfile, Row, RowBatch, Schema, Value,
};
use qfs_exec::{
    block_on_read, parse, run_oneshot, ExecCtx, OutputFormat, ReadDriver, ReadRegistry, StmtSource,
    Streams,
};
use qfs_pushdown::ScanNode;

// ---- An in-memory fake driver: introspective (describe/pushdown) + read (scan) ----

/// A fake mail-like source. `PushdownProfile::None` so a `WHERE`/`LIMIT` stays in the local
/// residual — the scan deliberately **over-returns** every row, and the engine's residual
/// re-filter restores correctness (the t20 property, end-to-end).
struct FakeMail {
    mount: String,
    rows: Vec<Row>,
    procs: Vec<qfs_core::ProcSig>,
}

fn mail_schema() -> Schema {
    Schema::new(vec![
        Column::new("id", ColumnType::Int, false),
        Column::new("subject", ColumnType::Text, true),
    ])
}

impl FakeMail {
    fn new() -> Self {
        Self {
            mount: "/mail".to_string(),
            rows: vec![
                Row::new(vec![Value::Int(1), Value::Text("hello".into())]),
                Row::new(vec![Value::Int(2), Value::Text("spam".into())]),
                Row::new(vec![Value::Int(3), Value::Text("world".into())]),
            ],
            // Two effect procedures for the terminal-`|> CALL` lowering tests: a reversible
            // `archive` and an irreversible `send` (both effect procs — no result schema).
            procs: vec![
                qfs_core::ProcSig::new("archive"),
                qfs_core::ProcSig::new("send").irreversible(true),
            ],
        }
    }
}

#[derive(Default)]
struct NoopApplier;
impl PlanApplier for NoopApplier {
    fn apply(&mut self, node: &EffectNode) -> Result<AppliedEffect, ApplyError> {
        Ok(AppliedEffect::new(node.id, 0))
    }
}

impl qfs_core::Driver for FakeMail {
    fn mount(&self) -> &str {
        &self.mount
    }
    fn describe(&self, _path: &Path) -> Result<NodeDesc, CfsError> {
        // 番地の鍵の宣言: rows are selected by `id`, so `/mail/inbox/@2` is a row address.
        Ok(NodeDesc::new(Archetype::RelationalTable, mail_schema()).child_key(["id"]))
    }
    fn capabilities(&self, _path: &Path) -> Capabilities {
        // All verbs so the effect-path tests (INSERT/REMOVE) pass the capability gate.
        Capabilities::none().select().insert().update().remove()
    }
    fn procedures(&self) -> &[qfs_core::ProcSig] {
        &self.procs
    }
    fn pushdown(&self) -> &PushdownProfile {
        // None: WHERE/LIMIT are local residuals — the engine must re-filter the over-returned
        // scan rows.
        &PushdownProfile::None
    }
    fn applier(&self) -> &dyn PlanApplier {
        // A leaked Box keeps a 'static reference without per-call allocation churn in the test.
        Box::leak(Box::new(NoopApplier))
    }
}

#[async_trait::async_trait]
impl ReadDriver for FakeMail {
    async fn scan(
        &self,
        _scan: &ScanNode,
        _ctx: &qfs_core::RequestContext,
    ) -> Result<RowBatch, CfsError> {
        // Honestly over-return: hand back ALL rows regardless of the pushed WHERE/LIMIT (this
        // source pushes nothing). The executor's residual must trim to the real result.
        Ok(RowBatch::new(mail_schema(), self.rows.clone()))
    }
}

fn engine_with_mail() -> Engine {
    let mut engine = Engine::new();
    engine.mounts.register(Arc::new(FakeMail::new())).unwrap();
    engine
}

fn reads_with_mail() -> ReadRegistry {
    ReadRegistry::new().with(DriverId::new("mail"), Arc::new(FakeMail::new()))
}

// ---- The headline read-path acceptance (the t20 carry-over closure) ----

#[test]
fn headline_read_returns_rows_through_real_executor() {
    // `/mail/inbox |> LIMIT 1` returns {"rows":[…]} end-to-end: parse -> resolve -> plan
    // -> scan -> residual -> rows. The fake over-returns 3 rows; LIMIT 1 trims to 1.
    let engine = engine_with_mail();
    let reads = reads_with_mail();
    let stmt = parse("/mail/inbox |> LIMIT 1").unwrap();
    let rows = block_on_read(
        &stmt,
        &engine.mounts,
        &reads,
        &qfs_core::RequestContext::anonymous(),
    )
    .unwrap();
    assert_eq!(
        rows.len(),
        1,
        "LIMIT 1 residual trims the over-returned rows"
    );
    assert_eq!(rows.columns(), vec!["id", "subject"]);
}

#[test]
fn selection_address_reads_exactly_the_selected_row() {
    // 番地の`@選択` end-to-end (plan.md, settled 2026-07-18): `/mail/inbox/@2` IS the row
    // address — parse → the one lowering site (`where id == 2` from the driver's DECLARED
    // child key) → scan → residual. The fake pushes nothing and over-returns all 3 rows,
    // so a green here proves the lowered predicate really filters at RUNTIME, not just in
    // the plan. (Watched red before `FakeMail::describe` declared the key: the address
    // refused with `selection_no_child_key`.)
    let engine = engine_with_mail();
    let reads = reads_with_mail();
    let stmt = parse("/mail/inbox/@2").unwrap();
    let rows = block_on_read(
        &stmt,
        &engine.mounts,
        &reads,
        &qfs_core::RequestContext::anonymous(),
    )
    .unwrap();
    assert_eq!(rows.len(), 1, "the address selects exactly one row");
    assert_eq!(rows.rows[0].values[0], Value::Int(2));
    assert_eq!(rows.rows[0].values[1], Value::Text("spam".into()));
}

#[test]
fn selection_address_answers_describe() {
    // 閉包の原理 (plan.md): every 番地 answers describe — the ROW address included.
    // `describe /mail/inbox/@2` resolves to the row-node view: the base node's shape
    // (archetype + columns), the FULL row address echoed, and no further `@` child claimed.
    let engine = engine_with_mail();
    let mut out: Vec<u8> = Vec::new();
    let mut err: Vec<u8> = Vec::new();
    let code = {
        let mut streams = Streams {
            out: &mut out,
            err: &mut err,
        };
        qfs_exec::run_describe(
            "/mail/inbox/@2",
            &engine.mounts,
            OutputFormat::Json,
            &mut streams,
        )
        .code()
    };
    let out = String::from_utf8(out).unwrap();
    assert_eq!(
        code,
        0,
        "the row address must describe; stderr: {}",
        String::from_utf8(err).unwrap()
    );
    assert!(
        out.contains("\"path\":\"/mail/inbox/@2\""),
        "the report names the FULL row address, got: {out}"
    );
    assert!(
        out.contains("\"child_address\":{\"kind\":\"none\"}"),
        "a selected row claims no further `@` child today, got: {out}"
    );

    // The refusals stay structured: a keyless arity mismatch is an error, not a guess.
    let mut out2: Vec<u8> = Vec::new();
    let mut err2: Vec<u8> = Vec::new();
    let code2 = {
        let mut streams = Streams {
            out: &mut out2,
            err: &mut err2,
        };
        qfs_exec::run_describe(
            "/mail/inbox/@1,2",
            &engine.mounts,
            OutputFormat::Json,
            &mut streams,
        )
        .code()
    };
    assert_ne!(code2, 0, "arity mismatch must refuse");
    assert!(
        String::from_utf8(err2).unwrap().contains("selection_arity"),
        "structured code"
    );
}

#[test]
fn residual_where_refilters_over_returned_rows() {
    // The t20 property end-to-end: a None-pushdown source returns all 3 rows; the residual
    // WHERE id > 1 re-filters to ids 2 and 3.
    let engine = engine_with_mail();
    let reads = reads_with_mail();
    let stmt = parse("/mail/inbox |> WHERE id > 1").unwrap();
    let rows = block_on_read(
        &stmt,
        &engine.mounts,
        &reads,
        &qfs_core::RequestContext::anonymous(),
    )
    .unwrap();
    assert_eq!(rows.len(), 2);
    let ids: Vec<i64> = rows
        .rows
        .iter()
        .map(|r| match r.values[0] {
            Value::Int(i) => i,
            _ => -1,
        })
        .collect();
    assert_eq!(ids, vec![2, 3]);
}

#[test]
fn headline_json_envelope_is_rows_object() {
    // The stable §14 result envelope: {schema:[{name,type}], rows:[{col:val}], meta:{…}}.
    let engine = engine_with_mail();
    let reads = reads_with_mail();
    let stmt = parse("/mail/inbox |> LIMIT 1").unwrap();
    let rows = block_on_read(
        &stmt,
        &engine.mounts,
        &reads,
        &qfs_core::RequestContext::anonymous(),
    )
    .unwrap();
    let json = serde_json::to_value(&rows).unwrap();
    // rows: unchanged top-level object-per-row (agent-native; back-compatible with t29 consumers).
    assert!(json["rows"].is_array());
    assert_eq!(json["rows"].as_array().unwrap().len(), 1);
    assert_eq!(json["rows"][0]["id"], 1);
    assert_eq!(json["rows"][0]["subject"], "hello");
    // schema: always present, in column order, carrying the §5 type token.
    assert_eq!(
        json["schema"],
        serde_json::json!([
            {"name": "id", "type": "int"},
            {"name": "subject", "type": "text"},
        ])
    );
    // meta: honest execution fact — a pure read has no bound and no affected count.
    assert_eq!(json["meta"]["row_count"], 1);
    assert_eq!(json["meta"]["truncated"], false);
    assert!(json["meta"]["limit"].is_null());
    assert!(json["meta"]["offset"].is_null());
    assert!(json["meta"]["affected"].is_null());
}

#[test]
fn envelope_bytes_column_is_base64() {
    // A `bytes` value renders as base64 (blueprint §14 hard break from the byte-array shape),
    // schema-discoverable via the `bytes` type token.
    let schema = Schema::new(vec![Column::new("blob", ColumnType::Bytes, false)]);
    let batch = RowBatch::new(
        schema,
        vec![Row::new(vec![Value::Bytes(b"hello".to_vec())])],
    );
    let rows = qfs_exec::RowSet::from_batch(batch);
    let json = serde_json::to_value(&rows).unwrap();
    assert_eq!(json["schema"][0]["type"], "bytes");
    // base64("hello") = "aGVsbG8=".
    assert_eq!(json["rows"][0]["blob"], "aGVsbG8=");
}

// ---- One-shot orchestration: exit codes + renderers ----

fn run(src: &str, fmt: OutputFormat, commit: bool) -> (i32, String, String) {
    run_with_ack(src, fmt, commit, false)
}

fn run_with_ack(
    src: &str,
    fmt: OutputFormat,
    commit: bool,
    irreversible_ack: bool,
) -> (i32, String, String) {
    run_full(
        src,
        fmt,
        commit,
        irreversible_ack,
        qfs_core::SafetyMode::default(),
    )
}

fn run_full(
    src: &str,
    fmt: OutputFormat,
    commit: bool,
    irreversible_ack: bool,
    safety_mode: qfs_core::SafetyMode,
) -> (i32, String, String) {
    let engine = engine_with_mail();
    let reads = reads_with_mail();
    let ctx = ExecCtx {
        engine: &engine,
        reads: &reads,
        world_apply: None,
        safety_mode,
        transform: None,
    };
    let source = StmtSource::Expr(src.to_string());
    let mut out: Vec<u8> = Vec::new();
    let mut err: Vec<u8> = Vec::new();
    let code = {
        let mut streams = Streams {
            out: &mut out,
            err: &mut err,
        };
        run_oneshot(&source, &ctx, fmt, commit, irreversible_ack, &mut streams).code()
    };
    (
        code,
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
    )
}

#[test]
fn oneshot_read_json_exit_zero_with_rows() {
    let (code, out, err) = run("/mail/inbox |> LIMIT 1", OutputFormat::Json, false);
    assert_eq!(code, 0);
    assert!(err.is_empty(), "data goes to stdout, not stderr");
    let v: serde_json::Value = serde_json::from_str(out.trim()).unwrap();
    assert_eq!(v["rows"].as_array().unwrap().len(), 1);
}

#[test]
fn oneshot_read_table_renders_columns() {
    let (code, out, _err) = run("/mail/inbox", OutputFormat::Table, false);
    assert_eq!(code, 0);
    assert!(out.contains("id"));
    assert!(out.contains("subject"));
    assert!(out.contains("hello"));
    assert!(out.contains("3 row(s)"));
}

#[test]
fn oneshot_parse_error_exit_two_with_kind_parse() {
    let (code, _out, err) = run("this is not pipe sql", OutputFormat::Json, false);
    assert_eq!(code, 2);
    let v: serde_json::Value = serde_json::from_str(err.trim()).unwrap();
    assert_eq!(v["error"]["kind"], "parse");
}

#[test]
fn oneshot_relative_path_usage_exit_two() {
    let (code, _out, err) = run("mail/inbox |> LIMIT 1", OutputFormat::Json, false);
    assert_eq!(code, 2);
    let v: serde_json::Value = serde_json::from_str(err.trim()).unwrap();
    assert_eq!(v["error"]["kind"], "usage");
    assert_eq!(v["error"]["path"], "mail/inbox");
}

#[test]
fn oneshot_unknown_source_capability_exit_three() {
    // /nope has no mounted driver → planner rejects with unknown_source → exit 3.
    let (code, _out, err) = run("/nope/x |> LIMIT 1", OutputFormat::Json, false);
    assert_eq!(code, 3);
    let v: serde_json::Value = serde_json::from_str(err.trim()).unwrap();
    assert_eq!(v["error"]["kind"], "capability");
}

// ---- PREVIEW / COMMIT gate ----

#[test]
fn oneshot_effect_preview_exit_zero_with_counts() {
    // A non-destructive INSERT previews at exit 0 with the plan + per-target counts.
    let (code, out, _err) = run(
        "INSERT INTO /mail/inbox VALUES (9, 'x')",
        OutputFormat::Json,
        false,
    );
    assert_eq!(code, 0);
    let v: serde_json::Value = serde_json::from_str(out.trim()).unwrap();
    assert_eq!(v["committed"], false);
    assert!(v["preview"]["rows"].is_array());
}

#[test]
fn oneshot_destructive_set_without_commit_exit_four() {
    // REMOVE over a set (no single-row bound) is irreversible + set-wide → exit 4 without
    // --commit. The PREVIEW is still rendered so the operator sees the affected counts.
    let (code, out, _err) = run("REMOVE /mail/inbox", OutputFormat::Json, false);
    assert_eq!(code, 4);
    let v: serde_json::Value = serde_json::from_str(out.trim()).unwrap();
    assert_eq!(v["committed"], false);
    assert!(!v["preview"]["irreversible"].as_array().unwrap().is_empty());
}

#[test]
fn oneshot_commit_applies_against_in_memory_engine() {
    // With --commit AND the irreversible ack the plan applies via the in-memory engine; the
    // summary marks committed. (`REMOVE` is irreversible, so the t37 one-shot gate requires the
    // ack — see `oneshot_irreversible_commit_blocked_without_ack` for the fail-closed default.)
    let (code, out, _err) = run_with_ack("REMOVE /mail/inbox", OutputFormat::Json, true, true);
    assert_eq!(code, 0);
    let v: serde_json::Value = serde_json::from_str(out.trim()).unwrap();
    assert_eq!(v["committed"], true);
}

#[test]
fn trailing_commit_keyword_applies_without_flag() {
    // A trailing COMMIT wrapper applies even without --commit (the CLI adds zero keywords). The
    // irreversible ack is still required for the irreversible REMOVE (t37): passed here.
    let (code, out, _err) =
        run_with_ack("COMMIT REMOVE /mail/inbox", OutputFormat::Json, false, true);
    assert_eq!(code, 0);
    let v: serde_json::Value = serde_json::from_str(out.trim()).unwrap();
    assert_eq!(v["committed"], true);
}

// ---- t37: the irreversible-effect one-shot gate (blueprint §7/§8) ----

#[test]
fn oneshot_irreversible_commit_blocked_without_ack() {
    // `qfs run … --commit` of an irreversible REMOVE in the NON-INTERACTIVE one-shot fails closed
    // (exit 4, commit_required) without `--commit-irreversible`. Zero effects apply; the PREVIEW
    // is still rendered so the operator sees what WOULD have applied. This is the t37 fail-closed
    // default that protects unattended one-shots from a silent destructive apply.
    let (code, out, err) = run_with_ack("REMOVE /mail/inbox", OutputFormat::Json, true, false);
    assert_eq!(
        code, 4,
        "irreversible --commit without ack must fail closed"
    );
    // The PREVIEW (not a commit) is on stdout: not committed, with the irreversible effect listed.
    let v: serde_json::Value = serde_json::from_str(out.trim()).unwrap();
    assert_eq!(v["committed"], false);
    assert!(!v["preview"]["irreversible"].as_array().unwrap().is_empty());
    // The structured error names the irreversible-ack requirement.
    let e: serde_json::Value = serde_json::from_str(err.trim()).unwrap();
    assert_eq!(e["error"]["code"], "irreversible_ack_required");
}

#[test]
fn oneshot_reversible_commit_needs_no_ack() {
    // A REVERSIBLE single-row INSERT commits with no ack — the gate governs only irreversible
    // effects, so the common case is unaffected.
    let (code, out, _err) = run_with_ack(
        "INSERT INTO /mail/inbox VALUES (9, 'x')",
        OutputFormat::Json,
        true,
        false,
    );
    assert_eq!(code, 0);
    let v: serde_json::Value = serde_json::from_str(out.trim()).unwrap();
    assert_eq!(v["committed"], true);
}

// ---- Golden Plan/PlanPreview for a representative effect statement (preview, no commit) ----

#[test]
fn golden_insert_plan_preview_json() {
    let engine = engine_with_mail();
    let stmt = parse("INSERT INTO /mail/inbox VALUES (9, 'x')").unwrap();
    let plan = qfs_exec::build_plan(&stmt, &engine).unwrap();
    let preview = qfs_exec::plan_preview(&plan);
    let v = serde_json::to_value(&preview).unwrap();
    // The plan has exactly one INSERT effect, not committed, not irreversible.
    assert_eq!(v["committed"], false);
    let rows = v["preview"]["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["verb"], "INSERT");
    assert_eq!(v["preview"]["is_pure"], false);
}

// ---- t75: the static type checker is ACTIVE on the PRODUCTION plan path ----

#[test]
fn build_plan_rejects_mismatched_set_where_at_plan_time() {
    // The headline t75 guarantee proven through the REAL production plan builder
    // (`qfs_exec::build_plan`, which `qfs run`, the server, and MCP all funnel through), NOT a
    // `with_stdlib` unit fixture. `subject` is a `Text` column (FakeMail's describe schema), so
    // `REMOVE /mail/inbox WHERE subject == 1` compares `Text` to an `Int` literal — an
    // incomparable-types error that must surface at PLAN TIME, before any effect node is applied.
    let engine = engine_with_mail();
    let stmt = parse("REMOVE /mail/inbox WHERE subject == 1").unwrap();
    let err = qfs_exec::build_plan(&stmt, &engine)
        .expect_err("a mismatched WHERE comparison must fail at plan time");
    assert_eq!(err.code, "incomparable_types");
    assert_eq!(err.kind, qfs_exec::ErrorKind::Usage);

    // The same REMOVE with a correctly-typed key column (`id` is `Int`) plans cleanly — the
    // checker accepts the well-typed program.
    let ok = parse("REMOVE /mail/inbox WHERE id == 1").unwrap();
    let plan = qfs_exec::build_plan(&ok, &engine).expect("a well-typed plan builds");
    assert!(!plan.nodes().is_empty(), "REMOVE yields an effect plan");
}

#[test]
fn oneshot_commit_of_mistyped_where_fails_before_apply() {
    // End-to-end through the one-shot COMMIT path: a `--commit` of a type-failing destructive
    // effect must fail at plan time (exit 2, usage) and apply ZERO effects — the type error is
    // caught before the applier is ever reached, so `committed` never becomes true.
    let (code, _out, err) = run_with_ack(
        "COMMIT REMOVE /mail/inbox WHERE subject == 1",
        OutputFormat::Json,
        true,
        true,
    );
    assert_eq!(code, 2, "a plan-time type error is a usage-class failure");
    let e: serde_json::Value = serde_json::from_str(err.trim()).unwrap();
    assert_eq!(e["error"]["code"], "incomparable_types");
}

// ---- t59: the selectable safety modes govern the REAL one-shot commit path ----

#[test]
fn t59_approve_everything_refuses_a_reversible_commit_autonomous_applies() {
    use qfs_core::SafetyMode;
    // Baseline: under the default Autonomous-in-policy mode, a reversible single-row INSERT commits
    // with no ack (exit 0, committed via the in-memory engine).
    let (code, out, _err) = run_full(
        "INSERT INTO /mail/inbox VALUES (9, 'x')",
        OutputFormat::Json,
        true,
        false,
        SafetyMode::AutonomousInPolicy,
    );
    assert_eq!(code, 0, "autonomous auto-commits a reversible write");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(out.trim()).unwrap()["committed"],
        true
    );

    // Same statement under Approve-everything: the most restrictive preset HOLDS even this
    // reversible write — fail closed (exit 4, approval_required), ZERO effects applied.
    let (code, out, err) = run_full(
        "INSERT INTO /mail/inbox VALUES (9, 'x')",
        OutputFormat::Json,
        true,
        false,
        SafetyMode::ApproveEverything,
    );
    assert_eq!(
        code, 4,
        "approve-everything refuses a write autonomous would apply"
    );
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(out.trim()).unwrap()["committed"],
        false
    );
    let e: serde_json::Value = serde_json::from_str(err.trim()).unwrap();
    assert_eq!(e["error"]["kind"], "commit_required");
    assert_eq!(e["error"]["code"], "approval_required");
}

// ---- Terminal `|> CALL` routes to the effect path, not the read path (regression) ----

#[test]
fn oneshot_terminal_call_previews_as_an_effect_not_rows() {
    // A pipeline terminating in `|> CALL mail.archive()` is an EFFECT, not a read: the one-shot
    // must PREVIEW the CALL (committed:false), NOT stream the source rows. This is the regression
    // guard for the one-shot routing — without it the `Statement::Query` fell through to the read
    // path and the CALL was silently dropped (drive.copy returned file rows and copied nothing).
    let (code, out, _err) = run(
        "/mail/inbox |> CALL mail.archive()",
        OutputFormat::Json,
        false,
    );
    assert_eq!(code, 0, "a reversible CALL previews at exit 0");
    let v: serde_json::Value = serde_json::from_str(out.trim()).unwrap();
    assert!(
        v["rows"].is_null(),
        "output is a plan preview, not a rows envelope"
    );
    assert_eq!(v["committed"], false);
    let rows = v["preview"]["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["verb"], "CALL mail.archive");
}

#[test]
fn oneshot_terminal_reversible_call_commits() {
    // `--commit` of the reversible CALL applies through the in-memory engine (no ack needed).
    let (code, out, _err) = run_with_ack(
        "/mail/inbox |> CALL mail.archive()",
        OutputFormat::Json,
        true,
        false,
    );
    assert_eq!(code, 0);
    let v: serde_json::Value = serde_json::from_str(out.trim()).unwrap();
    assert_eq!(v["committed"], true);
}

#[test]
fn oneshot_terminal_irreversible_call_is_gated_without_ack() {
    // The irreversible CALL (`mail.send`) carries its per-procedure irreversible flag through the
    // one-shot lowering, so `--commit` without `--commit-irreversible` fails closed (exit 4).
    let (code, out, err) = run_with_ack(
        "/mail/inbox |> CALL mail.send()",
        OutputFormat::Json,
        true,
        false,
    );
    assert_eq!(code, 4, "an irreversible CALL is held without the ack");
    let v: serde_json::Value = serde_json::from_str(out.trim()).unwrap();
    assert_eq!(v["committed"], false);
    assert!(!v["preview"]["irreversible"].as_array().unwrap().is_empty());
    let e: serde_json::Value = serde_json::from_str(err.trim()).unwrap();
    assert_eq!(e["error"]["code"], "irreversible_ack_required");
}

#[test]
fn oneshot_plain_read_without_a_terminal_call_still_returns_rows() {
    // The routing change touches ONLY terminal-CALL queries: a plain read still streams rows.
    let (code, out, _err) = run("/mail/inbox |> LIMIT 1", OutputFormat::Json, false);
    assert_eq!(code, 0);
    let v: serde_json::Value = serde_json::from_str(out.trim()).unwrap();
    assert!(
        v["rows"].is_array(),
        "a plain read is still a rows envelope"
    );
    assert!(v["preview"].is_null());
}

#[test]
fn t59_policy_only_auto_commits_an_irreversible_write_without_the_ack() {
    use qfs_core::SafetyMode;
    // Under Policy-only (unattended CI), an irreversible REMOVE auto-commits with NO ack — the
    // write Autonomous-in-policy would have held. The policy floor still applies (the capability
    // set allows REMOVE here, the CLI one-shot's within-policy posture).
    let (code, out, _err) = run_full(
        "REMOVE /mail/inbox",
        OutputFormat::Json,
        true,
        false,
        SafetyMode::PolicyOnly,
    );
    assert_eq!(
        code, 0,
        "policy-only auto-commits an irreversible write unattended"
    );
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(out.trim()).unwrap()["committed"],
        true
    );
}

// ---- §15 transform execution + routing (blueprint decision W) ----

mod transform_e2e {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A deterministic mock transform executor: counts its calls and emits one `label` row per
    /// input row. Proves the PREVIEW-zero-calls property (it is only ever reached at COMMIT) and
    /// the committed-read row plumbing — no model, no network.
    struct MockExec {
        calls: Arc<AtomicUsize>,
    }
    impl qfs_engine::TransformExecutor for MockExec {
        fn execute(
            &self,
            _call: &qfs_engine::TransformCall<'_>,
            input: RowBatch,
        ) -> Result<RowBatch, String> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            let rows = input
                .rows
                .iter()
                .map(|_| Row::new(vec![Value::Text("L".into())]))
                .collect();
            Ok(RowBatch::new(
                Schema::new(vec![Column::new("label", ColumnType::Text, true)]),
                rows,
            ))
        }
    }

    /// A mock returning an undeclared column — the untrusted-output case the engine must reject.
    struct BadExec;
    impl qfs_engine::TransformExecutor for BadExec {
        fn execute(
            &self,
            _call: &qfs_engine::TransformCall<'_>,
            _input: RowBatch,
        ) -> Result<RowBatch, String> {
            Ok(RowBatch::new(
                Schema::new(vec![Column::new("wrong", ColumnType::Text, true)]),
                vec![Row::new(vec![Value::Text("x".into())])],
            ))
        }
    }

    /// A `classify` definition over the mail schema: INPUT (subject text) ⇒ row-wise,
    /// OUTPUT (label text). Installed on the engine so a `|> transform classify` stage plans.
    fn engine_with_transform() -> Engine {
        let mut engine = engine_with_mail();
        let mut defs = qfs_core::TransformDefs::new();
        defs.insert(
            "classify".to_string(),
            qfs_core::ResolvedTransform::new(
                Schema::new(vec![Column::new("subject", ColumnType::Text, true)]),
                Schema::new(vec![Column::new("label", ColumnType::Text, true)]),
            )
            .unwrap()
            .with_model_meta("claude", "claude-sonnet-5", Some("medium".into())),
        );
        engine.mounts.set_transform_defs(defs);
        engine
    }

    fn run_transform(
        src: &str,
        commit: bool,
        ack: bool,
        exec: Option<Arc<dyn qfs_engine::TransformExecutor>>,
    ) -> (i32, String, String) {
        let engine = engine_with_transform();
        let reads = reads_with_mail();
        let ctx = ExecCtx {
            engine: &engine,
            reads: &reads,
            world_apply: None,
            safety_mode: qfs_core::SafetyMode::default(),
            transform: exec,
        };
        let source = StmtSource::Expr(src.to_string());
        let (mut out, mut err): (Vec<u8>, Vec<u8>) = (Vec::new(), Vec::new());
        let code = {
            let mut streams = Streams {
                out: &mut out,
                err: &mut err,
            };
            run_oneshot(&source, &ctx, OutputFormat::Json, commit, ack, &mut streams).code()
        };
        (
            code,
            String::from_utf8(out).unwrap(),
            String::from_utf8(err).unwrap(),
        )
    }

    #[test]
    fn preview_of_a_transform_calls_no_model() {
        // AC: PREVIEW (no --commit) calls no model — the mock records ZERO invocations. The
        // statement is effect-bearing (§15), so it is held on the commit-required class.
        let calls = Arc::new(AtomicUsize::new(0));
        let exec = Arc::new(MockExec {
            calls: calls.clone(),
        });
        let (code, _out, err) = run_transform(
            "/mail/inbox |> transform classify",
            false,
            false,
            Some(exec),
        );
        assert_eq!(code, 4, "held: a model call requires explicit commit");
        assert_eq!(calls.load(Ordering::Relaxed), 0, "PREVIEW calls no model");
        assert!(err.contains("commit"), "{err}");
    }

    #[test]
    fn commit_without_the_irreversible_ack_is_rejected_and_calls_no_model() {
        // AC: COMMIT without the irreversible ack is rejected (the model call is irreversible).
        let calls = Arc::new(AtomicUsize::new(0));
        let exec = Arc::new(MockExec {
            calls: calls.clone(),
        });
        let (code, _out, err) =
            run_transform("/mail/inbox |> transform classify", true, false, Some(exec));
        assert_eq!(code, 4, "held: irreversible ack required");
        assert_eq!(
            calls.load(Ordering::Relaxed),
            0,
            "a held commit calls no model"
        );
        assert!(err.contains("irreversible"), "{err}");
    }

    #[test]
    fn commit_with_the_ack_runs_the_model_and_renders_the_committed_read_envelope() {
        // AC: with the ack, the mock-backed run executes; a committed read renders rows +
        // meta.affected (§14). The engine hands the whole upstream relation to the executor in one
        // call (per-row model chunking lives in the binary executor); the mock emits one OUTPUT
        // row per input row ⇒ three OUTPUT rows over three mail rows.
        let calls = Arc::new(AtomicUsize::new(0));
        let exec = Arc::new(MockExec {
            calls: calls.clone(),
        });
        let (code, out, _err) =
            run_transform("/mail/inbox |> transform classify", true, true, Some(exec));
        assert_eq!(code, 0, "committed");
        assert_eq!(
            calls.load(Ordering::Relaxed),
            1,
            "the engine calls the executor once"
        );
        let v: serde_json::Value = serde_json::from_str(out.trim()).unwrap();
        assert_eq!(
            v["rows"].as_array().unwrap().len(),
            3,
            "the OUTPUT rows are rendered"
        );
        assert_eq!(v["rows"][0]["label"], "L");
        // The §14 committed-read envelope: the rows document's meta.affected is non-null.
        assert_eq!(v["meta"]["affected"], 3);
    }

    #[test]
    fn an_output_membership_violation_fails_the_commit() {
        // The model's output is untrusted: a returned column the definition never declared is a
        // structured commit failure, never silently accepted.
        let (code, _out, err) = run_transform(
            "/mail/inbox |> transform classify",
            true,
            true,
            Some(Arc::new(BadExec)),
        );
        assert_eq!(code, 5, "commit failed");
        assert!(err.contains("transform_output_mismatch"), "{err}");
    }

    #[test]
    fn a_commit_without_an_injected_executor_fails_closed() {
        // No executor injected (the fail-closed default until a live provider is wired): the
        // engine refuses the model stage rather than returning silent rows.
        let (code, _out, err) =
            run_transform("/mail/inbox |> transform classify", true, true, None);
        assert_eq!(code, 5, "commit failed: no executor");
        assert!(err.contains("transform_no_executor"), "{err}");
    }

    #[test]
    fn a_transform_in_a_subquery_source_is_classified_effect_bearing() {
        // Whole-tree routing (§15): a `|> transform` in a SUBQUERY source classifies the whole
        // statement as effect-bearing — it is held on the commit-required class, never read
        // directly. Zero model calls at PREVIEW.
        let calls = Arc::new(AtomicUsize::new(0));
        let exec = Arc::new(MockExec {
            calls: calls.clone(),
        });
        let (code, _out, err) = run_transform(
            "(/mail/inbox |> transform classify) |> WHERE label == 'L'",
            false,
            false,
            Some(exec),
        );
        assert_eq!(
            code, 4,
            "a nested transform routes through preview/commit: {err}"
        );
        assert_eq!(calls.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn a_plain_read_without_a_transform_is_unaffected() {
        // A read with NO transform keeps its exact prior behaviour: exit 0, rows rendered, and the
        // §14 envelope's `meta.affected` is NULL (no effects ran) — the committed-read signal.
        let (code, out, _err) = run_transform("/mail/inbox |> LIMIT 1", false, false, None);
        assert_eq!(code, 0);
        let v: serde_json::Value = serde_json::from_str(out.trim()).unwrap();
        assert_eq!(v["rows"].as_array().unwrap().len(), 1);
        assert!(
            v["meta"]["affected"].is_null(),
            "a plain read ran no effects"
        );
    }

    // ---- T7: transformation chains (`… |> transform a |> transform b`) ----

    use std::sync::Mutex;

    /// A two-stage chain executor routing on the def name: `extract` (subject → summary) then
    /// `summarize` (summary → digest). It records the CALL ORDER and asserts stage b receives exactly
    /// stage a's OUTPUT column (`summary`) — the schema-checked handoff, proven at runtime.
    struct ChainExec {
        order: Arc<Mutex<Vec<String>>>,
        fail_second: bool,
    }
    impl qfs_engine::TransformExecutor for ChainExec {
        fn execute(
            &self,
            call: &qfs_engine::TransformCall<'_>,
            input: RowBatch,
        ) -> Result<RowBatch, String> {
            self.order.lock().unwrap().push(call.name.to_string());
            match call.name {
                "extract" => Ok(RowBatch::new(
                    Schema::new(vec![Column::new("summary", ColumnType::Text, true)]),
                    input
                        .rows
                        .iter()
                        .map(|_| Row::new(vec![Value::Text("S".into())]))
                        .collect(),
                )),
                "summarize" => {
                    // Stage b must receive stage a's OUTPUT column `summary`, never the source columns.
                    assert!(
                        input.schema.column("summary").is_some(),
                        "stage b did not receive stage a's OUTPUT rows"
                    );
                    if self.fail_second {
                        return Err("mid-chain provider failure".into());
                    }
                    Ok(RowBatch::new(
                        Schema::new(vec![Column::new("digest", ColumnType::Text, true)]),
                        input
                            .rows
                            .iter()
                            .map(|_| Row::new(vec![Value::Text("D".into())]))
                            .collect(),
                    ))
                }
                other => Err(format!("unexpected stage {other}")),
            }
        }
    }

    fn chain_defs() -> qfs_core::TransformDefs {
        let mut defs = qfs_core::TransformDefs::new();
        defs.insert(
            "extract".to_string(),
            qfs_core::ResolvedTransform::new(
                Schema::new(vec![Column::new("subject", ColumnType::Text, true)]),
                Schema::new(vec![Column::new("summary", ColumnType::Text, true)]),
            )
            .unwrap()
            .with_model_meta("anthropic", "claude-sonnet-5", None),
        );
        defs.insert(
            "summarize".to_string(),
            qfs_core::ResolvedTransform::new(
                Schema::new(vec![Column::new("summary", ColumnType::Text, true)]),
                Schema::new(vec![Column::new("digest", ColumnType::Text, true)]),
            )
            .unwrap()
            .with_model_meta("anthropic", "claude-sonnet-5", None),
        );
        defs
    }

    fn engine_with_chain() -> Engine {
        let mut engine = engine_with_mail();
        engine.mounts.set_transform_defs(chain_defs());
        engine
    }

    /// A write applier that rejects a `Read` effect exactly like the real Gmail/GitHub/Slack driver
    /// appliers ("READ is not serviced"). A `world_apply` closure routes every plan node through it,
    /// reproducing the real interpreter's per-node dispatch (the in-memory `apply_commit` fallback
    /// used by other tests accepts everything, so it cannot surface this — round-6 defect).
    struct RejectReadApplier;
    impl PlanApplier for RejectReadApplier {
        fn apply(&mut self, node: &EffectNode) -> Result<AppliedEffect, ApplyError> {
            if matches!(node.kind, qfs_core::EffectKind::Read) {
                return Err(ApplyError::new(
                    node.id,
                    "READ is not serviced by the Gmail driver".to_string(),
                ));
            }
            Ok(AppliedEffect::new(node.id, 0))
        }
    }

    fn run_chain(
        src: &str,
        commit: bool,
        ack: bool,
        exec: Option<Arc<dyn qfs_engine::TransformExecutor>>,
    ) -> (i32, String, String) {
        let engine = engine_with_chain();
        let reads = reads_with_mail();
        let ctx = ExecCtx {
            engine: &engine,
            reads: &reads,
            world_apply: None,
            safety_mode: qfs_core::SafetyMode::default(),
            transform: exec,
        };
        let source = StmtSource::Expr(src.to_string());
        let (mut out, mut err): (Vec<u8>, Vec<u8>) = (Vec::new(), Vec::new());
        let code = {
            let mut streams = Streams {
                out: &mut out,
                err: &mut err,
            };
            run_oneshot(&source, &ctx, OutputFormat::Json, commit, ack, &mut streams).code()
        };
        (
            code,
            String::from_utf8(out).unwrap(),
            String::from_utf8(err).unwrap(),
        )
    }

    #[test]
    fn a_two_stage_chain_executes_in_order_with_a_schema_checked_handoff() {
        let order = Arc::new(Mutex::new(Vec::new()));
        let exec = Arc::new(ChainExec {
            order: order.clone(),
            fail_second: false,
        });
        let (code, out, err) = run_chain(
            "/mail/inbox |> select subject |> transform extract |> transform summarize",
            true,
            true,
            Some(exec),
        );
        assert_eq!(code, 0, "the chain commits: {err}");
        // Stage order is extract THEN summarize (once per input row — 3 mail rows → 3 + 3 calls).
        let seen = order.lock().unwrap().clone();
        let first_summarize = seen.iter().position(|s| s == "summarize").unwrap();
        assert!(
            seen[..first_summarize].iter().all(|s| s == "extract"),
            "every extract call precedes the first summarize: {seen:?}"
        );
        // The relation downstream is the LAST stage's OUTPUT (`digest`).
        let v: serde_json::Value = serde_json::from_str(out.trim()).unwrap();
        assert_eq!(v["schema"][0]["name"], "digest");
        assert!(v["rows"]
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["digest"] == "D"));
    }

    #[test]
    fn a_chain_preview_calls_no_model_for_either_stage() {
        let order = Arc::new(Mutex::new(Vec::new()));
        let exec = Arc::new(ChainExec {
            order: order.clone(),
            fail_second: false,
        });
        // PREVIEW (no commit): both stages are model-free — the chain is held on commit-required.
        let (code, _out, _err) = run_chain(
            "/mail/inbox |> select subject |> transform extract |> transform summarize",
            false,
            false,
            Some(exec),
        );
        assert_eq!(code, 4, "an effect-bearing chain is held at PREVIEW");
        assert!(
            order.lock().unwrap().is_empty(),
            "PREVIEW must call NO model for either chain stage"
        );
    }

    #[test]
    fn a_mid_chain_failure_aborts_the_whole_statement() {
        let order = Arc::new(Mutex::new(Vec::new()));
        let exec = Arc::new(ChainExec {
            order: order.clone(),
            fail_second: true,
        });
        let (code, _out, err) = run_chain(
            "/mail/inbox |> select subject |> transform extract |> transform summarize",
            true,
            true,
            Some(exec),
        );
        assert_ne!(code, 0, "a mid-chain provider failure fails the commit");
        assert!(
            err.contains("mid-chain") || err.to_lowercase().contains("transform"),
            "the failure surfaces the mid-chain error: {err}"
        );
    }

    #[test]
    fn read_terminal_transform_commits_though_the_source_applier_rejects_read() {
        // Round-6 defect (ticket 20260713123000): a read-terminal transform chain over Gmail failed
        // at commit — "READ is not serviced by the Gmail driver" — because the source READ was
        // dispatched to the driver's WRITE applier. The `world_apply` here rejects READ exactly like
        // the real interpreter would route it to the Gmail applier; the chain must STILL commit,
        // because the read engine already materialised the source and the read node is stripped
        // before the consent-ledger apply. (Without the fix, the source READ reaches the applier and
        // the commit fails — proven by temporarily disabling `strip_source_reads`.)
        let engine = engine_with_chain();
        let reads = reads_with_mail();
        let world = |plan: &qfs_core::Plan| -> Result<(), qfs_exec::ExecError> {
            let mut applier = RejectReadApplier;
            for node in &plan.nodes {
                applier.apply(node).map_err(|e| {
                    qfs_exec::ExecError::new(
                        qfs_exec::ErrorKind::CommitFailed,
                        "commit_failed",
                        e.to_string(),
                    )
                })?;
            }
            Ok(())
        };
        let order = Arc::new(Mutex::new(Vec::new()));
        let exec: Arc<dyn qfs_engine::TransformExecutor> = Arc::new(ChainExec {
            order,
            fail_second: false,
        });
        let ctx = ExecCtx {
            engine: &engine,
            reads: &reads,
            world_apply: Some(&world),
            safety_mode: qfs_core::SafetyMode::default(),
            transform: Some(exec),
        };
        let source = StmtSource::Expr(
            "/mail/inbox |> select subject |> transform extract |> transform summarize".to_string(),
        );
        let (mut out, mut err): (Vec<u8>, Vec<u8>) = (Vec::new(), Vec::new());
        let code = {
            let mut streams = Streams {
                out: &mut out,
                err: &mut err,
            };
            run_oneshot(&source, &ctx, OutputFormat::Json, true, true, &mut streams).code()
        };
        let err = String::from_utf8(err).unwrap();
        assert_eq!(
            code, 0,
            "the read-terminal chain commits though the source applier rejects READ: {err}"
        );
        let v: serde_json::Value = serde_json::from_str(String::from_utf8(out).unwrap().trim())
            .expect("committed-read envelope");
        assert_eq!(
            v["schema"][0]["name"], "digest",
            "the chain returns the last stage's rows"
        );
    }
}

// ---- T4: cross-service Gmail-attachment → Drive-folder transfer (materialize_pipeline_source) ----
//
// One statement reads an attachment's bytes from `/mail/<acct>/<msg>/<att>` and upserts them into a
// specific `/drive/<acct>/<folder>` — the mirror of the shipped Drive→Gmail attach-and-send pipe.
// These tests drive `materialize_pipeline_source` (the cross-driver read→write channel) END TO END
// through the commit gate: the first exec-level commit test of that machinery. A `world_apply`
// closure captures the applied write so we can assert the attachment's bytes reached the Drive
// upload byte-for-byte, with the destination folder as the write target.
mod attachment_to_drive {
    use super::*;
    use std::cell::RefCell;

    /// The attachment bytes the fake Gmail source delivers — asserted byte-identical at the write.
    const ATTACHMENT_BYTES: &[u8] = b"%PDF-1.7 fake invoice bytes";

    /// Mirror of the live gmail driver's `attachment_read_schema` (filename/mime/size/content) — the
    /// single-row shape a `/mail/<msg>/<att>` read returns, lined up column-for-column with the Drive
    /// upload row shape.
    fn attachment_schema() -> Schema {
        Schema::new(vec![
            Column::new("filename", ColumnType::Text, false),
            Column::new("mime", ColumnType::Text, false),
            Column::new("size", ColumnType::Int, false),
            Column::new("content", ColumnType::Bytes, true),
        ])
    }

    /// The Drive upload row shape (`name`/`mime_type`/`bytes`) a folder node accepts on INSERT.
    fn drive_upload_schema() -> Schema {
        Schema::new(vec![
            Column::new("name", ColumnType::Text, true),
            Column::new("mime_type", ColumnType::Text, true),
            Column::new("bytes", ColumnType::Bytes, true),
        ])
    }

    /// A fake Gmail exposing `/mail/<acct>/<msg>/<att>` as a single-row attachment source. Its
    /// `describe` advertises the SAME columns the scan returns — the gap the real driver's new
    /// describe arm closes — so the cross-service SELECT resolves `filename`/`mime`/`content` at plan
    /// time.
    struct FakeAttachmentMail;

    impl qfs_core::Driver for FakeAttachmentMail {
        fn mount(&self) -> &str {
            "/mail"
        }
        fn describe(&self, _path: &Path) -> Result<NodeDesc, CfsError> {
            Ok(NodeDesc::new(Archetype::AppendLog, attachment_schema()))
        }
        fn capabilities(&self, _path: &Path) -> Capabilities {
            Capabilities::none().select()
        }
        fn procedures(&self) -> &[qfs_core::ProcSig] {
            &[]
        }
        fn pushdown(&self) -> &PushdownProfile {
            &PushdownProfile::None
        }
        fn applier(&self) -> &dyn PlanApplier {
            Box::leak(Box::new(NoopApplier))
        }
    }

    #[async_trait::async_trait]
    impl ReadDriver for FakeAttachmentMail {
        async fn scan(
            &self,
            _scan: &ScanNode,
            _ctx: &qfs_core::RequestContext,
        ) -> Result<RowBatch, CfsError> {
            let row = Row::new(vec![
                Value::Text("invoice.pdf".into()),
                Value::Text("application/pdf".into()),
                Value::Int(ATTACHMENT_BYTES.len() as i64),
                Value::Bytes(ATTACHMENT_BYTES.to_vec()),
            ]);
            Ok(RowBatch::new(attachment_schema(), vec![row]))
        }
    }

    /// A fake Drive whose folder nodes accept an upload INSERT (`name`/`mime_type`/`bytes`). A path
    /// segment naming `readonly` is a NON-insertable destination — the wrong-destination case that
    /// must fail at the capability gate (PREVIEW), before any commit.
    struct FakeDriveFolder;

    impl qfs_core::Driver for FakeDriveFolder {
        fn mount(&self) -> &str {
            "/drive"
        }
        fn describe(&self, _path: &Path) -> Result<NodeDesc, CfsError> {
            Ok(NodeDesc::new(
                Archetype::RelationalTable,
                drive_upload_schema(),
            ))
        }
        fn capabilities(&self, path: &Path) -> Capabilities {
            if path.as_str().contains("readonly") {
                Capabilities::none().select()
            } else {
                Capabilities::none().select().insert()
            }
        }
        fn procedures(&self) -> &[qfs_core::ProcSig] {
            &[]
        }
        fn pushdown(&self) -> &PushdownProfile {
            &PushdownProfile::None
        }
        fn applier(&self) -> &dyn PlanApplier {
            Box::leak(Box::new(NoopApplier))
        }
    }

    fn engine_mail_and_drive() -> Engine {
        let mut engine = Engine::new();
        engine
            .mounts
            .register(Arc::new(FakeAttachmentMail))
            .unwrap();
        engine.mounts.register(Arc::new(FakeDriveFolder)).unwrap();
        engine
    }

    fn reads_mail() -> ReadRegistry {
        ReadRegistry::new().with(DriverId::new("mail"), Arc::new(FakeAttachmentMail))
    }

    /// Run one statement to COMMIT, capturing every applied (non-Read) write's `(target path, args)`
    /// through the injected `world_apply` — the real commit path's substitute in tests.
    fn run_capturing(src: &str) -> (i32, String, String, Vec<(String, RowBatch)>) {
        let engine = engine_mail_and_drive();
        let reads = reads_mail();
        let captured: RefCell<Vec<(String, RowBatch)>> = RefCell::new(Vec::new());
        {
            let world = |plan: &qfs_core::Plan| -> Result<(), qfs_exec::ExecError> {
                for node in &plan.nodes {
                    if !matches!(
                        node.kind,
                        qfs_core::EffectKind::Read | qfs_core::EffectKind::List
                    ) {
                        captured
                            .borrow_mut()
                            .push((node.target.path.as_str().to_string(), node.args.clone()));
                    }
                }
                Ok(())
            };
            let ctx = ExecCtx {
                engine: &engine,
                reads: &reads,
                world_apply: Some(&world),
                safety_mode: qfs_core::SafetyMode::default(),
                transform: None,
            };
            let source = StmtSource::Expr(src.to_string());
            let (mut out, mut err): (Vec<u8>, Vec<u8>) = (Vec::new(), Vec::new());
            let code = {
                let mut streams = Streams {
                    out: &mut out,
                    err: &mut err,
                };
                run_oneshot(&source, &ctx, OutputFormat::Json, true, false, &mut streams).code()
            };
            (
                code,
                String::from_utf8(out).unwrap(),
                String::from_utf8(err).unwrap(),
                captured.into_inner(),
            )
        }
    }

    #[test]
    fn attachment_bytes_transfer_into_the_named_drive_folder_in_one_statement() {
        // The mission's Gmail→Drive transfer: one statement, byte-identical content, folder targeted.
        let (code, out, err, applied) = run_capturing(
            "/mail/you/m1/att1 \
             |> select filename as name, mime as mime_type, content as bytes \
             |> insert into /drive/you/Reports",
        );
        assert_eq!(code, 0, "the one-statement transfer commits: {err}");
        assert_eq!(applied.len(), 1, "exactly one write applied");
        let (path, batch) = &applied[0];
        assert_eq!(
            path, "/drive/you/Reports",
            "the destination folder is the write target"
        );
        assert_eq!(
            batch.rows.len(),
            1,
            "the attachment row materialized into the write"
        );
        let idx = |n: &str| {
            batch
                .schema
                .columns
                .iter()
                .position(|c| c.name == n)
                .unwrap_or_else(|| panic!("write args missing column {n}"))
        };
        assert_eq!(
            batch.rows[0].values[idx("name")],
            Value::Text("invoice.pdf".into())
        );
        assert_eq!(
            batch.rows[0].values[idx("mime_type")],
            Value::Text("application/pdf".into())
        );
        assert_eq!(
            batch.rows[0].values[idx("bytes")],
            Value::Bytes(ATTACHMENT_BYTES.to_vec()),
            "byte-identical attachment content reaches the Drive upload"
        );
        let v: serde_json::Value = serde_json::from_str(out.trim()).unwrap();
        assert_eq!(v["committed"], true);
    }

    #[test]
    fn a_non_insertable_destination_fails_before_commit() {
        // A wrong destination (here: a node without the INSERT capability) fails at the capability
        // gate — a structured error at PREVIEW, before any write is applied.
        let (code, _out, err, applied) = run_capturing(
            "/mail/you/m1/att1 \
             |> select filename as name, mime as mime_type, content as bytes \
             |> insert into /drive/you/readonly",
        );
        assert_eq!(
            code, 3,
            "an un-insertable destination is a capability error: {err}"
        );
        assert!(applied.is_empty(), "no write applied to a bad destination");
        let e: serde_json::Value = serde_json::from_str(err.trim()).unwrap();
        assert_eq!(e["error"]["kind"], "capability");
    }
}

// ---- T6: PDF bytes → transform (Extraction) → Google Drive in one statement ----
//
// The mission's Extraction flagship (ticket 20260711121530): read a document's bytes, run them
// through a single-`bytes`-input transform (derive_mode → Extraction), and upsert the extracted
// text into a Drive folder — one statement, materialized at commit. A mock transform executor
// stands in for the model (the provider document-encoding leg is proven in transform_providers),
// and a world_apply closure captures the Drive write so we assert the extracted text lands there.
mod pdf_extraction_to_drive {
    use super::*;
    use std::cell::RefCell;

    const PDF_BYTES: &[u8] = b"%PDF-1.7 fake invoice document bytes";
    const EXTRACTED: &str = "Invoice #42 — total $1,000";

    /// A fake `/local` document source that mirrors the REAL `LocalRow::content_schema` a single-file
    /// read produces: the listing columns PLUS a nullable `content` (Bytes) column. This is the
    /// honest seam (ticket 20260713120000) — the recipe must `|> select content` to narrow to the
    /// single-bytes Extraction input, and that projection must type-check against this real describe
    /// schema (the previous single-`blob` fake hid the plan/runtime divergence).
    struct FakePdfLocal;
    /// The single-`content` (Bytes) shape a `|> select content` yields — the Extraction input.
    fn content_col_schema() -> Schema {
        Schema::new(vec![Column::new("content", ColumnType::Bytes, true)])
    }
    /// The real `/local` single-file read schema: listing columns + the nullable `content` column.
    fn local_content_schema() -> Schema {
        Schema::new(vec![
            Column::new("name", ColumnType::Text, false),
            Column::new("path", ColumnType::Text, false),
            Column::new("size", ColumnType::Int, false),
            Column::new("modified", ColumnType::Timestamp, false),
            Column::new("is_dir", ColumnType::Bool, false),
            Column::new("mode", ColumnType::Int, false),
            Column::new("content", ColumnType::Bytes, true),
        ])
    }
    impl qfs_core::Driver for FakePdfLocal {
        fn mount(&self) -> &str {
            "/local"
        }
        fn describe(&self, _p: &Path) -> Result<NodeDesc, CfsError> {
            Ok(NodeDesc::new(
                Archetype::BlobNamespace,
                local_content_schema(),
            ))
        }
        fn capabilities(&self, _p: &Path) -> Capabilities {
            Capabilities::none().select()
        }
        fn procedures(&self) -> &[qfs_core::ProcSig] {
            &[]
        }
        fn pushdown(&self) -> &PushdownProfile {
            &PushdownProfile::None
        }
        fn applier(&self) -> &dyn PlanApplier {
            Box::leak(Box::new(NoopApplier))
        }
    }
    #[async_trait::async_trait]
    impl ReadDriver for FakePdfLocal {
        async fn scan(
            &self,
            _s: &ScanNode,
            _ctx: &qfs_core::RequestContext,
        ) -> Result<RowBatch, CfsError> {
            // A single-file read: the listing row for report.pdf plus its raw bytes under `content`.
            Ok(RowBatch::new(
                local_content_schema(),
                vec![Row::new(vec![
                    Value::Text("report.pdf".into()),
                    Value::Text("/local/report.pdf".into()),
                    Value::Int(PDF_BYTES.len() as i64),
                    Value::Timestamp(0),
                    Value::Bool(false),
                    Value::Int(0),
                    Value::Bytes(PDF_BYTES.to_vec()),
                ])],
            ))
        }
    }

    /// A fake Drive folder accepting the extracted-text upload (`name`/`mime_type`/`bytes`).
    struct FakeDriveFolder;
    fn drive_schema() -> Schema {
        Schema::new(vec![
            Column::new("name", ColumnType::Text, true),
            Column::new("mime_type", ColumnType::Text, true),
            Column::new("bytes", ColumnType::Bytes, true),
        ])
    }
    impl qfs_core::Driver for FakeDriveFolder {
        fn mount(&self) -> &str {
            "/drive"
        }
        fn describe(&self, _p: &Path) -> Result<NodeDesc, CfsError> {
            Ok(NodeDesc::new(Archetype::RelationalTable, drive_schema()))
        }
        fn capabilities(&self, _p: &Path) -> Capabilities {
            Capabilities::none().select().insert().upsert()
        }
        fn procedures(&self) -> &[qfs_core::ProcSig] {
            &[]
        }
        fn pushdown(&self) -> &PushdownProfile {
            &PushdownProfile::None
        }
        fn applier(&self) -> &dyn PlanApplier {
            Box::leak(Box::new(NoopApplier))
        }
    }

    /// The Extraction transform executor: one bytes blob in → one extracted-text row out
    /// (`name`/`mime_type`/`bytes`), the Drive upload shape. Stands in for the model; asserts it
    /// received the PDF bytes intact.
    struct ExtractExec;
    impl qfs_engine::TransformExecutor for ExtractExec {
        fn execute(
            &self,
            _call: &qfs_engine::TransformCall<'_>,
            input: RowBatch,
        ) -> Result<RowBatch, String> {
            // The single input row carries the PDF bytes (the Extraction input column).
            let got = input
                .rows
                .first()
                .and_then(|r| {
                    r.values.iter().find_map(|v| match v {
                        Value::Bytes(b) => Some(b.clone()),
                        _ => None,
                    })
                })
                .ok_or("extraction input had no bytes")?;
            if got != PDF_BYTES {
                return Err("the PDF bytes did not reach the transform intact".into());
            }
            Ok(RowBatch::new(
                drive_schema(),
                vec![Row::new(vec![
                    Value::Text("invoice.txt".into()),
                    Value::Text("text/plain".into()),
                    Value::Text(EXTRACTED.into()),
                ])],
            ))
        }
    }

    fn engine_pdf_to_drive() -> Engine {
        let mut engine = Engine::new();
        engine.mounts.register(Arc::new(FakePdfLocal)).unwrap();
        engine.mounts.register(Arc::new(FakeDriveFolder)).unwrap();
        let mut defs = qfs_core::TransformDefs::new();
        // INPUT (blob bytes) ⇒ derive_mode = Extraction; OUTPUT is the Drive upload shape.
        defs.insert(
            "extract".to_string(),
            qfs_core::ResolvedTransform::new(content_col_schema(), drive_schema())
                .unwrap()
                .with_model_meta("anthropic", "claude-sonnet-5", Some("medium".into())),
        );
        engine.mounts.set_transform_defs(defs);
        engine
    }

    #[test]
    fn pdf_bytes_extract_and_upsert_into_a_drive_folder_in_one_statement() {
        let engine = engine_pdf_to_drive();
        let reads = ReadRegistry::new().with(DriverId::new("local"), Arc::new(FakePdfLocal));
        let captured: RefCell<Vec<(String, RowBatch)>> = RefCell::new(Vec::new());
        let (code, out, err) = {
            let world = |plan: &qfs_core::Plan| -> Result<(), qfs_exec::ExecError> {
                for node in &plan.nodes {
                    if !matches!(
                        node.kind,
                        qfs_core::EffectKind::Read | qfs_core::EffectKind::List
                    ) && !node.args.rows.is_empty()
                    {
                        captured
                            .borrow_mut()
                            .push((node.target.path.as_str().to_string(), node.args.clone()));
                    }
                }
                Ok(())
            };
            let ctx = ExecCtx {
                engine: &engine,
                reads: &reads,
                world_apply: Some(&world),
                safety_mode: qfs_core::SafetyMode::default(),
                transform: Some(Arc::new(ExtractExec)),
            };
            // The honest recipe against the REAL /local schema: `select content` narrows the
            // multi-column single-file read to the single-bytes Extraction input BEFORE transform.
            // This projection type-checks only because describe() now advertises `content`
            // (ticket 20260713120000) — it previously failed `UnknownColumn` at plan time.
            let source = StmtSource::Expr(
                "/local/report.pdf |> select content |> transform extract \
                 |> upsert into /drive/you/Extracted"
                    .to_string(),
            );
            let (mut o, mut e): (Vec<u8>, Vec<u8>) = (Vec::new(), Vec::new());
            let code = {
                let mut streams = Streams {
                    out: &mut o,
                    err: &mut e,
                };
                // commit + ack: a transform commit needs the model-consent ack.
                run_oneshot(&source, &ctx, OutputFormat::Json, true, true, &mut streams).code()
            };
            (
                code,
                String::from_utf8(o).unwrap(),
                String::from_utf8(e).unwrap(),
            )
        };
        assert_eq!(code, 0, "the PDF→text→Drive pipeline commits: {err}");
        let applied = captured.into_inner();
        // The Drive upsert is the write that carries the extracted rows (the transform-consent node
        // also applies, so we assert on the Drive target specifically).
        let (_path, batch) = applied
            .iter()
            .find(|(p, _)| p == "/drive/you/Extracted")
            .expect("the Drive upsert was applied");
        let idx = |n: &str| {
            batch
                .schema
                .columns
                .iter()
                .position(|c| c.name == n)
                .unwrap_or_else(|| panic!("Drive args missing {n}"))
        };
        assert_eq!(
            batch.rows[0].values[idx("name")],
            Value::Text("invoice.txt".into())
        );
        assert_eq!(
            batch.rows[0].values[idx("bytes")],
            Value::Text(EXTRACTED.into()),
            "the extracted OUTPUT text lands as the Drive upload payload"
        );
        let v: serde_json::Value = serde_json::from_str(out.trim()).unwrap();
        assert_eq!(v["committed"], true);
    }
}

// ---- T3: cross-service reply-with-attachment — a Drive file into a Gmail thread reply ----
//
// The mission's third file-handling flow (ticket 20260711121528): one statement REPLIES to a Gmail
// thread with a file whose bytes come from Google Drive. The reply is expressed as an append into
// the parent message's `replies` collection (`INSERT INTO /mail/<label>/<msg>/replies`) so its body
// is an `EffectBody::Pipeline` that flows through `materialize_pipeline_source` — the leg a `CALL
// mail.reply`'s literal args cannot express. The Drive row is packed into the Gmail attachments
// struct vocabulary (`array_agg(struct{filename, mime, bytes})`) and the parent id rides the path.
mod drive_to_gmail_reply {
    use super::*;
    use std::cell::RefCell;

    /// The Drive file bytes the fake source delivers — asserted byte-identical inside the reply's
    /// materialized attachments array.
    const DRIVE_BYTES: &[u8] = b"%PDF-1.7 quarterly report bytes";

    /// The Drive content-read shape (`name`/`mime_type`/`size`/`content`) a `/drive/<file>` read
    /// returns — lined up so `select {filename: name, mime: mime_type, bytes: content}` resolves.
    fn drive_content_schema() -> Schema {
        Schema::new(vec![
            Column::new("name", ColumnType::Text, false),
            Column::new("mime_type", ColumnType::Text, false),
            Column::new("size", ColumnType::Int, false),
            Column::new("content", ColumnType::Bytes, true),
        ])
    }

    /// The reply-append WRITE shape (`body`/`to`/`cc`/`subject`/`attachments`) the Gmail `replies`
    /// node advertises — a mirror of the real driver's `schema::reply_write_schema`.
    fn reply_write_schema() -> Schema {
        let attachment_struct = Schema::new(vec![
            Column::new("filename", ColumnType::Text, false),
            Column::new("mime", ColumnType::Text, false),
            Column::new("bytes", ColumnType::Bytes, false),
        ]);
        Schema::new(vec![
            Column::new("body", ColumnType::Text, false),
            Column::new("to", ColumnType::Text, true),
            Column::new("cc", ColumnType::Text, true),
            Column::new("subject", ColumnType::Text, true),
            Column::new(
                "attachments",
                ColumnType::Array(Box::new(ColumnType::Struct(attachment_struct))),
                true,
            ),
        ])
    }

    /// A fake Drive exposing `/drive/<acct>/<file>` as a single-row content source (bytes in
    /// `content`). Its `describe` advertises the SAME columns the scan returns so the cross-service
    /// SELECT resolves `name`/`mime_type`/`content` at plan time.
    struct FakeDriveFile;

    impl qfs_core::Driver for FakeDriveFile {
        fn mount(&self) -> &str {
            "/drive"
        }
        fn describe(&self, _path: &Path) -> Result<NodeDesc, CfsError> {
            Ok(NodeDesc::new(Archetype::AppendLog, drive_content_schema()))
        }
        fn capabilities(&self, _path: &Path) -> Capabilities {
            Capabilities::none().select()
        }
        fn procedures(&self) -> &[qfs_core::ProcSig] {
            &[]
        }
        fn pushdown(&self) -> &PushdownProfile {
            &PushdownProfile::None
        }
        fn applier(&self) -> &dyn PlanApplier {
            Box::leak(Box::new(NoopApplier))
        }
    }

    #[async_trait::async_trait]
    impl ReadDriver for FakeDriveFile {
        async fn scan(
            &self,
            _scan: &ScanNode,
            _ctx: &qfs_core::RequestContext,
        ) -> Result<RowBatch, CfsError> {
            let row = Row::new(vec![
                Value::Text("report.pdf".into()),
                Value::Text("application/pdf".into()),
                Value::Int(DRIVE_BYTES.len() as i64),
                Value::Bytes(DRIVE_BYTES.to_vec()),
            ]);
            Ok(RowBatch::new(drive_content_schema(), vec![row]))
        }
    }

    /// A fake Gmail whose `/mail/<label>/<msg>/replies` node accepts a reply INSERT (the reply
    /// write columns). A path NOT ending in `replies` is read-only here — only the reply append-log
    /// is insertable, mirroring the real driver's capability map.
    struct FakeGmailReplies;

    impl qfs_core::Driver for FakeGmailReplies {
        fn mount(&self) -> &str {
            "/mail"
        }
        fn describe(&self, _path: &Path) -> Result<NodeDesc, CfsError> {
            Ok(NodeDesc::new(Archetype::AppendLog, reply_write_schema()))
        }
        fn capabilities(&self, path: &Path) -> Capabilities {
            if path.as_str().ends_with("/replies") {
                Capabilities::none().select().insert()
            } else {
                Capabilities::none().select()
            }
        }
        fn procedures(&self) -> &[qfs_core::ProcSig] {
            &[]
        }
        fn pushdown(&self) -> &PushdownProfile {
            &PushdownProfile::None
        }
        fn applier(&self) -> &dyn PlanApplier {
            Box::leak(Box::new(NoopApplier))
        }
    }

    fn engine_drive_and_mail() -> Engine {
        let mut engine = Engine::new();
        engine.mounts.register(Arc::new(FakeDriveFile)).unwrap();
        engine.mounts.register(Arc::new(FakeGmailReplies)).unwrap();
        engine
    }

    fn reads_drive() -> ReadRegistry {
        ReadRegistry::new().with(DriverId::new("drive"), Arc::new(FakeDriveFile))
    }

    fn run_capturing(src: &str) -> (i32, String, String, Vec<(String, RowBatch)>) {
        let engine = engine_drive_and_mail();
        let reads = reads_drive();
        let captured: RefCell<Vec<(String, RowBatch)>> = RefCell::new(Vec::new());
        {
            let world = |plan: &qfs_core::Plan| -> Result<(), qfs_exec::ExecError> {
                for node in &plan.nodes {
                    if !matches!(
                        node.kind,
                        qfs_core::EffectKind::Read | qfs_core::EffectKind::List
                    ) {
                        captured
                            .borrow_mut()
                            .push((node.target.path.as_str().to_string(), node.args.clone()));
                    }
                }
                Ok(())
            };
            let ctx = ExecCtx {
                engine: &engine,
                reads: &reads,
                world_apply: Some(&world),
                safety_mode: qfs_core::SafetyMode::default(),
                transform: None,
            };
            let source = StmtSource::Expr(src.to_string());
            let (mut out, mut err): (Vec<u8>, Vec<u8>) = (Vec::new(), Vec::new());
            let code = {
                let mut streams = Streams {
                    out: &mut out,
                    err: &mut err,
                };
                run_oneshot(&source, &ctx, OutputFormat::Json, true, false, &mut streams).code()
            };
            (
                code,
                String::from_utf8(out).unwrap(),
                String::from_utf8(err).unwrap(),
                captured.into_inner(),
            )
        }
    }

    #[test]
    fn drive_file_materializes_into_a_gmail_thread_reply_in_one_statement() {
        // One statement: read a Drive file, pack it into the Gmail attachments struct, and thread a
        // reply carrying it — byte-identical, addressed at the parent message.
        let (code, out, err, applied) = run_capturing(
            "/drive/you/report.pdf \
             |> select {filename: name, mime: mime_type, bytes: content} as att \
             |> aggregate array_agg(att) as attachments \
             |> extend body = 'See the attached quarterly report.' \
             |> insert into /mail/you/m1/replies",
        );
        assert_eq!(
            code, 0,
            "the one-statement reply-with-attachment commits: {err}"
        );
        assert_eq!(applied.len(), 1, "exactly one write (the reply) applied");
        let (path, batch) = &applied[0];
        assert_eq!(
            path, "/mail/you/m1/replies",
            "the reply threads onto the parent message's replies log"
        );
        assert_eq!(
            batch.rows.len(),
            1,
            "the reply row materialized into the write"
        );
        let idx = |n: &str| {
            batch
                .schema
                .columns
                .iter()
                .position(|c| c.name == n)
                .unwrap_or_else(|| panic!("reply args missing column {n}"))
        };
        assert_eq!(
            batch.rows[0].values[idx("body")],
            Value::Text("See the attached quarterly report.".into())
        );
        // The attachments column is an Array of one Struct carrying the Drive file's bytes intact.
        let Value::Array(atts) = &batch.rows[0].values[idx("attachments")] else {
            panic!("attachments did not materialize as an Array");
        };
        assert_eq!(atts.len(), 1, "exactly the one Drive file was attached");
        let Value::Struct(fields) = &atts[0] else {
            panic!("attachment element is not a Struct");
        };
        let field = |n: &str| {
            fields
                .iter()
                .find(|(k, _)| k.as_str() == n)
                .map(|(_, v)| v)
                .unwrap_or_else(|| panic!("attachment struct missing field {n}"))
        };
        assert_eq!(field("filename"), &Value::Text("report.pdf".into()));
        assert_eq!(field("mime"), &Value::Text("application/pdf".into()));
        assert_eq!(
            field("bytes"),
            &Value::Bytes(DRIVE_BYTES.to_vec()),
            "byte-identical Drive content reaches the Gmail reply attachment"
        );
        let v: serde_json::Value = serde_json::from_str(out.trim()).unwrap();
        assert_eq!(v["committed"], true);
    }
}

// ---- §18 SWITCH routing e2e: preview union → commit routing → untaken-arm prune ----

mod switch_e2e {
    use super::*;
    use std::cell::RefCell;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A deterministic mock router: `subject == "hello"` routes `'urgent'`, everything else
    /// `'misc'`. Counts its calls so the tests can prove PREVIEW calls no model and COMMIT
    /// calls it exactly once (one materialization, N arms).
    struct RouteExec {
        calls: Arc<AtomicUsize>,
    }
    impl qfs_engine::TransformExecutor for RouteExec {
        fn execute(
            &self,
            _call: &qfs_engine::TransformCall<'_>,
            input: RowBatch,
        ) -> Result<RowBatch, String> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            let subject = input
                .schema
                .columns
                .iter()
                .position(|c| c.name == "subject")
                .ok_or("input carries no subject column")?;
            let rows = input
                .rows
                .iter()
                .map(|r| {
                    let route = match &r.values[subject] {
                        Value::Text(s) if s == "hello" => "urgent",
                        _ => "misc",
                    };
                    Row::new(vec![Value::Text(route.into())])
                })
                .collect();
            Ok(RowBatch::new(
                Schema::new(vec![Column::new("route", ColumnType::Text, false)]),
                rows,
            ))
        }
    }

    /// The mail engine plus a `triage` definition (INPUT subject ⇒ OUTPUT route) so
    /// `|> transform triage |> switch route { … }` plans.
    fn engine_with_triage() -> Engine {
        let mut engine = engine_with_mail();
        let mut defs = qfs_core::TransformDefs::new();
        defs.insert(
            "triage".to_string(),
            qfs_core::ResolvedTransform::new(
                Schema::new(vec![Column::new("subject", ColumnType::Text, true)]),
                Schema::new(vec![Column::new("route", ColumnType::Text, false)]),
            )
            .unwrap()
            .with_model_meta("claude", "claude-sonnet-5", Some("medium".into())),
        );
        engine.mounts.set_transform_defs(defs);
        engine
    }

    /// Run one statement with a CAPTURING world applier: every non-Read effect that actually
    /// fires is recorded as `(target path, args)` — the direct observable for "untaken arms'
    /// effects never execute".
    fn run_switch(
        src: &str,
        commit: bool,
        ack: bool,
        exec: Option<Arc<dyn qfs_engine::TransformExecutor>>,
    ) -> (i32, String, String, Vec<(String, RowBatch)>) {
        let engine = engine_with_triage();
        let reads = reads_with_mail();
        let captured: RefCell<Vec<(String, RowBatch)>> = RefCell::new(Vec::new());
        let (code, out, err) = {
            let world = |plan: &qfs_core::Plan| -> Result<(), qfs_exec::ExecError> {
                for node in &plan.nodes {
                    if !matches!(
                        node.kind,
                        qfs_core::EffectKind::Read | qfs_core::EffectKind::List
                    ) {
                        captured
                            .borrow_mut()
                            .push((node.target.path.as_str().to_string(), node.args.clone()));
                    }
                }
                Ok(())
            };
            let ctx = ExecCtx {
                engine: &engine,
                reads: &reads,
                world_apply: Some(&world),
                safety_mode: qfs_core::SafetyMode::default(),
                transform: exec,
            };
            let source = StmtSource::Expr(src.to_string());
            let (mut out, mut err): (Vec<u8>, Vec<u8>) = (Vec::new(), Vec::new());
            let code = {
                let mut streams = Streams {
                    out: &mut out,
                    err: &mut err,
                };
                run_oneshot(&source, &ctx, OutputFormat::Json, commit, ack, &mut streams).code()
            };
            (
                code,
                String::from_utf8(out).unwrap(),
                String::from_utf8(err).unwrap(),
            )
        };
        (code, out, err, captured.into_inner())
    }

    const ROUTED: &str = "/mail/inbox |> transform triage \
        |> switch route { 'urgent' => INSERT INTO /mail/urgent, \
                          else => select route |> INSERT INTO /mail/rest }";

    #[test]
    fn preview_of_a_switch_previews_every_arm_and_calls_no_model() {
        // §18-C(3): PREVIEW shows the arm UNION — both writes — while the model is never
        // called and nothing applies. The transform consent node holds the statement on the
        // commit-required class (exit 4), exactly like any transform-bearing statement.
        let calls = Arc::new(AtomicUsize::new(0));
        let exec = Arc::new(RouteExec {
            calls: calls.clone(),
        });
        let (code, out, _err, applied) = run_switch(ROUTED, false, false, Some(exec));
        assert_eq!(code, 4, "held for commit");
        assert_eq!(calls.load(Ordering::Relaxed), 0, "PREVIEW calls no model");
        assert!(applied.is_empty(), "PREVIEW applies nothing");
        assert!(
            out.contains("/mail/urgent") && out.contains("/mail/rest"),
            "every arm's effect is previewed (the union is the declared effect set): {out}"
        );
    }

    #[test]
    fn commit_routes_each_partition_to_its_arm() {
        // §18-C(1): the source materializes ONCE (one model call), rows partition by the
        // discriminant, and each arm's write receives exactly its partition — 'hello' routes
        // 'urgent' (1 row), 'spam'/'world' route 'misc' → else (2 rows, piped through the
        // arm's `select route` continuation).
        let calls = Arc::new(AtomicUsize::new(0));
        let exec = Arc::new(RouteExec {
            calls: calls.clone(),
        });
        let (code, out, err, applied) = run_switch(ROUTED, true, true, Some(exec));
        assert_eq!(code, 0, "committed: {err}");
        assert_eq!(
            calls.load(Ordering::Relaxed),
            1,
            "one materialization, one model call"
        );
        let urgent = applied
            .iter()
            .find(|(p, _)| p == "/mail/urgent")
            .expect("the urgent arm fired");
        assert_eq!(urgent.1.rows.len(), 1, "exactly the 'hello' row routed");
        let rest = applied
            .iter()
            .find(|(p, _)| p == "/mail/rest")
            .expect("the else arm fired");
        assert_eq!(rest.1.rows.len(), 2, "the two 'misc' rows routed to else");
        assert_eq!(
            rest.1.schema.columns.len(),
            1,
            "the arm's `select route` continuation applied over its partition"
        );
        assert_eq!(rest.1.schema.columns[0].name, "route");
        let v: serde_json::Value = serde_json::from_str(out.trim()).unwrap();
        assert_eq!(v["committed"], true);
    }

    #[test]
    fn an_untaken_arm_is_previewed_but_never_fires() {
        // §18-C(3): every arm is consented at PREVIEW, but an arm whose partition is empty is
        // pruned at commit — its effect never reaches the applier and the committed summary
        // does not claim it.
        let src = "/mail/inbox |> transform triage \
            |> switch route { 'nope' => INSERT INTO /mail/never, \
                              else => INSERT INTO /mail/rest }";
        let calls = Arc::new(AtomicUsize::new(0));
        // PREVIEW first: the untaken arm IS in the union.
        let (code, out, _err, applied) = run_switch(
            src,
            false,
            false,
            Some(Arc::new(RouteExec {
                calls: calls.clone(),
            })),
        );
        assert_eq!(code, 4);
        assert!(applied.is_empty());
        assert!(
            out.contains("/mail/never"),
            "the untaken arm was previewed/consented: {out}"
        );
        // COMMIT: nothing routes 'nope', so the arm never fires.
        let (code, out, err, applied) =
            run_switch(src, true, true, Some(Arc::new(RouteExec { calls })));
        assert_eq!(code, 0, "committed: {err}");
        assert!(
            applied.iter().all(|(p, _)| p != "/mail/never"),
            "the untaken arm's effect never executes: {applied:?}"
        );
        let rest = applied
            .iter()
            .find(|(p, _)| p == "/mail/rest")
            .expect("else took every row");
        assert_eq!(rest.1.rows.len(), 3);
        assert!(
            !out.contains("/mail/never"),
            "the committed summary claims only fired arms: {out}"
        );
    }

    #[test]
    fn a_switch_needs_no_transform_to_route_a_source_column() {
        // The stage routes any text column — here the raw `subject`. No model, no consent
        // node, a reversible plan: the default safety mode commits without the ack.
        let src = "/mail/inbox |> switch subject { 'hello' => INSERT INTO /mail/hello, \
                   else => INSERT INTO /mail/rest }";
        let (code, _out, err, applied) = run_switch(src, true, false, None);
        assert_eq!(code, 0, "committed: {err}");
        let hello = applied
            .iter()
            .find(|(p, _)| p == "/mail/hello")
            .expect("the labeled arm fired");
        assert_eq!(hello.1.rows.len(), 1);
        // The routed row is the REAL matching source row (id 1, subject 'hello').
        assert_eq!(hello.1.rows[0].values[0], Value::Int(1));
        let rest = applied
            .iter()
            .find(|(p, _)| p == "/mail/rest")
            .expect("the else arm fired");
        assert_eq!(rest.1.rows.len(), 2);
    }

    #[test]
    fn a_switch_commit_without_an_executor_fails_closed() {
        // The transform stage inside the source fails closed with no injected executor —
        // the switch materialization refuses rather than routing silent rows.
        let (code, _out, err, applied) = run_switch(ROUTED, true, true, None);
        assert_eq!(code, 5, "commit failed: {err}");
        assert!(err.contains("transform_no_executor"), "{err}");
        assert!(applied.is_empty(), "nothing fired");
    }

    #[test]
    fn commit_bridges_deps_so_arms_apply_in_declaration_order() {
        // §18-C(1) at the APPLIED plan: pruning the Read markers must BRIDGE the dependency
        // chain (consent → urgent-write → else-write), not drop it — the live round showed a
        // dep-free later arm firing before an earlier arm (and before an earlier arm's
        // FAILURE could stop it). Assert the applied plan's deps still chain the arms.
        let engine = engine_with_triage();
        let reads = reads_with_mail();
        let captured: RefCell<Vec<(qfs_core::NodeId, qfs_core::NodeId)>> = RefCell::new(Vec::new());
        let world = |plan: &qfs_core::Plan| -> Result<(), qfs_exec::ExecError> {
            captured.borrow_mut().extend(plan.deps.iter().copied());
            Ok(())
        };
        let ctx = ExecCtx {
            engine: &engine,
            reads: &reads,
            world_apply: Some(&world),
            safety_mode: qfs_core::SafetyMode::default(),
            transform: Some(Arc::new(RouteExec {
                calls: Arc::new(AtomicUsize::new(0)),
            })),
        };
        let source = StmtSource::Expr(ROUTED.to_string());
        let (mut out, mut err): (Vec<u8>, Vec<u8>) = (Vec::new(), Vec::new());
        let code = {
            let mut streams = Streams {
                out: &mut out,
                err: &mut err,
            };
            run_oneshot(&source, &ctx, OutputFormat::Json, true, true, &mut streams).code()
        };
        assert_eq!(code, 0, "{}", String::from_utf8_lossy(&err));
        let deps = captured.into_inner();
        // The eval plan ids: consent #0, urgent read #1 / write #2, else read #3 / write #4.
        // After the bridged prune the applied chain must be #0 → #2 → #4.
        assert!(
            deps.contains(&(qfs_core::NodeId(0), qfs_core::NodeId(2))),
            "consent → urgent write survives the prune: {deps:?}"
        );
        assert!(
            deps.contains(&(qfs_core::NodeId(2), qfs_core::NodeId(4))),
            "urgent write → else write (declaration order) survives the prune: {deps:?}"
        );
    }
}

// ---- SPIKE 2.0: a slash-bearing (nested-mount) DriverId routes through BOTH the read and apply
//      DriverId-keyed funnels (ticket 20260718203326, owner ruling 2026-07-19). ----
//
// This settles the ONE unverified seam that gated the declared `/cf` D1 mount shape (nested mount
// vs composite facet). The mechanical half was already de-risked by code-read: `DriverId` is an
// unvalidated `String` (`qfs_types::schema`) and the plan/describe registry routes by longest-prefix
// path (`core::registry::resolve_service_path`), so the default `id()` = `mount()` minus the leading
// `/` yields a nested mount `/a/b` the slash-bearing id `"a/b"`, distinct from `"a"`. What no test
// exercised is whether that slash-bearing id ALSO flows cleanly through the two `DriverId`-keyed
// runtime funnels — the read funnel (`ReadRegistry.get(id)`, `exec::exec::id_of`) and the apply/write
// funnel (the effect target's `DriverId`, `runtime::interpreter` `drivers.get(id)`) — given every
// driver today registers a SINGLE-segment id. These tests register a nested mount at `/a/b` and prove
// both funnels resolve it correctly, with no single-segment collision. Green ⇒ the declared D1
// surface can be a plain NESTED mount (id `cloudflare/d1`); no composite facet is needed.
mod nested_mount_id_routing_spike {
    use super::*;

    /// A fake source at an arbitrary mount that tags every read row with its OWN mount, so a
    /// returned row is attributable to exactly one driver. SELECT+INSERT so the apply (effect-plan)
    /// path also lowers over it.
    struct TaggedSource {
        mount: String,
    }

    fn tag_schema() -> Schema {
        Schema::new(vec![
            Column::new("id", ColumnType::Int, false),
            Column::new("who", ColumnType::Text, true),
        ])
    }

    impl TaggedSource {
        fn new(mount: &str) -> Self {
            Self {
                mount: mount.to_string(),
            }
        }
    }

    impl qfs_core::Driver for TaggedSource {
        fn mount(&self) -> &str {
            &self.mount
        }
        // The default `id()` (mount minus the leading `/`) is deliberately kept: it is exactly the
        // production derivation under test — `/a/b` → `"a/b"`.
        fn describe(&self, _p: &Path) -> Result<NodeDesc, CfsError> {
            Ok(NodeDesc::new(Archetype::RelationalTable, tag_schema()))
        }
        fn capabilities(&self, _p: &Path) -> Capabilities {
            Capabilities::none().select().insert()
        }
        fn procedures(&self) -> &[qfs_core::ProcSig] {
            &[]
        }
        fn pushdown(&self) -> &PushdownProfile {
            &PushdownProfile::None
        }
        fn applier(&self) -> &dyn PlanApplier {
            Box::leak(Box::new(NoopApplier))
        }
    }

    #[async_trait::async_trait]
    impl ReadDriver for TaggedSource {
        async fn scan(&self, _scan: &ScanNode) -> Result<RowBatch, CfsError> {
            Ok(RowBatch::new(
                tag_schema(),
                vec![Row::new(vec![
                    Value::Int(1),
                    Value::Text(self.mount.as_str().into()),
                ])],
            ))
        }
    }

    /// An engine with two OVERLAPPING mounts: the top `/a` (id `"a"`) and the NESTED `/a/b`
    /// (slash-bearing id `"a/b"`).
    fn engine_ab() -> Engine {
        let mut engine = Engine::new();
        engine
            .mounts
            .register(Arc::new(TaggedSource::new("/a")))
            .unwrap();
        engine
            .mounts
            .register(Arc::new(TaggedSource::new("/a/b")))
            .unwrap();
        engine
    }

    fn reads_ab() -> ReadRegistry {
        ReadRegistry::new()
            .with(DriverId::new("a"), Arc::new(TaggedSource::new("/a")))
            .with(DriverId::new("a/b"), Arc::new(TaggedSource::new("/a/b")))
    }

    /// The `who` (mount tag) of the first returned row — which driver actually served the read.
    fn who_of(rows: &qfs_exec::RowSet) -> String {
        match &rows.rows[0].values[1] {
            Value::Text(s) => s.to_string(),
            v => panic!("expected a text `who`, got {v:?}"),
        }
    }

    #[test]
    fn read_funnel_routes_the_slash_bearing_nested_mount_id() {
        // A SELECT of `/a/b/x` must resolve to the NESTED driver through the REAL read funnel:
        // `plan_query` tags the scan with source `"a/b"` (longest-prefix over the overlapping `/a`),
        // and `ReadRegistry.get("a/b")` resolves the slash-bearing id. The returned row is tagged by
        // the driver that served it, so this is attributable end-to-end.
        let engine = engine_ab();
        let reads = reads_ab();

        let nested = block_on_read(&parse("/a/b/x |> LIMIT 1").unwrap(), &engine.mounts, &reads)
            .expect("nested read resolves");
        assert_eq!(nested.len(), 1);
        assert_eq!(
            who_of(&nested),
            "/a/b",
            "the slash-bearing nested id `a/b` resolves its OWN read driver, not the top `a`"
        );

        // Control: the top mount still routes to the single-segment id `"a"` (no shadowing).
        let top = block_on_read(&parse("/a/x |> LIMIT 1").unwrap(), &engine.mounts, &reads)
            .expect("top read resolves");
        assert_eq!(who_of(&top), "/a", "the top mount still routes to id `a`");
    }

    #[test]
    fn apply_funnel_targets_the_slash_bearing_nested_mount_id() {
        // The full effect lowering (parse → resolve → plan → typeck → build effect plan) must tag
        // the write's target with the slash-bearing id `"a/b"` — the key the runtime apply funnel
        // (`interpreter::drivers.get(id)`) resolves. If any lowering / capability-qualification stage
        // assumed a single-segment id, `build_plan` would misroute or reject here.
        let engine = engine_ab();
        let stmt = parse("INSERT INTO /a/b/tbl VALUES (9, 'x')").unwrap();
        let plan = qfs_exec::build_plan(&stmt, &engine).expect("the nested-mount INSERT plans");
        let node = &plan.nodes()[0];
        assert_eq!(
            node.target.driver,
            DriverId::new("a/b"),
            "the effect target keys the apply funnel on the slash-bearing nested id"
        );
        assert_eq!(node.target.path.as_str(), "/a/b/tbl");
    }

    #[test]
    fn full_commit_path_drives_the_nested_mount_without_choking_on_the_slash() {
        // End-to-end through the REAL one-shot COMMIT path: the whole apply pipeline (capability
        // gate + effect dispatch) must drive an INSERT into the nested `/a/b` mount to a clean
        // commit, applying the write at the nested path. A `world_apply` captures the applied write.
        use std::cell::RefCell;
        let engine = engine_ab();
        let reads = reads_ab();
        let captured: RefCell<Vec<String>> = RefCell::new(Vec::new());
        let (code, err) = {
            let world = |plan: &qfs_core::Plan| -> Result<(), qfs_exec::ExecError> {
                for node in &plan.nodes {
                    if !matches!(
                        node.kind,
                        qfs_core::EffectKind::Read | qfs_core::EffectKind::List
                    ) {
                        captured
                            .borrow_mut()
                            .push(node.target.path.as_str().to_string());
                    }
                }
                Ok(())
            };
            let ctx = ExecCtx {
                engine: &engine,
                reads: &reads,
                world_apply: Some(&world),
                safety_mode: qfs_core::SafetyMode::default(),
                transform: None,
            };
            let source = StmtSource::Expr("INSERT INTO /a/b/tbl VALUES (9, 'x')".to_string());
            let (mut out, mut err): (Vec<u8>, Vec<u8>) = (Vec::new(), Vec::new());
            let code = {
                let mut streams = Streams {
                    out: &mut out,
                    err: &mut err,
                };
                run_oneshot(&source, &ctx, OutputFormat::Json, true, false, &mut streams).code()
            };
            (code, String::from_utf8(err).unwrap())
        };
        assert_eq!(code, 0, "the nested-mount INSERT commits cleanly: {err}");
        let applied = captured.into_inner();
        assert_eq!(
            applied,
            vec!["/a/b/tbl".to_string()],
            "the write applied at the nested path"
        );
    }

    #[test]
    fn both_funnel_registries_resolve_a_slash_bearing_id_with_no_single_segment_collision() {
        // The routing primitive both funnels share: a `HashMap<DriverId, _>` `.get(id)`. Prove a
        // slash-bearing id is a DISTINCT key from its leading segment — the read funnel's
        // `ReadRegistry` here (the apply funnel's `runtime::DriverRegistry` is the identical
        // `HashMap<DriverId, _>` keying, exercised end-to-end by the commit test above).
        let reads =
            ReadRegistry::new().with(DriverId::new("a/b"), Arc::new(TaggedSource::new("/a/b")));
        assert!(
            reads.get(&DriverId::new("a/b")).is_some(),
            "the slash-bearing id resolves"
        );
        assert!(
            reads.get(&DriverId::new("a")).is_none(),
            "a slash-bearing id does NOT collide with its leading single segment"
        );
    }
}
