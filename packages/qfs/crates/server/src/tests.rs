//! Internal tests for the server runtime + `/server` self-config driver (t30).
//!
//! Acceptance coverage:
//! - boot a fixture `.qfs` → deterministic [`ServerState`] snapshot; re-apply is a no-op;
//! - CREATE-vs-INSERT **plan-node equivalence** (sugar equivalence, golden on the plan);
//! - unsupported-verb **plan-time rejection** (structured error, no panic, no COMMIT);
//! - `DESCRIBE /server/triggers` returns the trigger schema with no live backend;
//! - [`NullBinding`]/[`CountingBinding`] `reconcile` invoked once per committed mutation;
//! - **purity**: building a `/server` write plan mutates nothing until COMMIT;
//! - the run loop's audit drain on shutdown.

use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use qfs_core::{
    commit, preview, Column, ColumnType, Driver, EffectKind, Path, Plan, Row, RowBatch, Schema,
    ServerNode, ServerWriteOp, StatementSpec,
};
use qfs_parser::parse_statement;

use super::*;
use crate::driver::ServerDriver;
use crate::lower::lower_statement;
use crate::runtime::ServerConfigApplier;

/// The in-worktree boot fixture path (no system paths).
fn fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/server_boot.qfs")
}

/// Lower a single statement source to its `/server` write plan (helper).
fn lower(src: &str) -> Plan {
    let stmt = parse_statement(src).expect("parse");
    lower_statement(&stmt)
        .expect("lower")
        .expect("a /server write")
}

// ---- boot + snapshot + idempotency ----------------------------------------

#[test]
fn boot_fixture_reaches_deterministic_state() {
    let mut rt = Runtime::new().with_binding(Box::new(NullBinding));
    rt.boot(&fixture_path()).expect("boot succeeds");

    let state = rt.snapshot();
    // The fixture defines: 2 jobs (nightly via DDL + weekly via INSERT), 1 endpoint,
    // 1 trigger, 2 views (recent_view + cached_view), 1 policy, 1 webhook.
    assert_eq!(state.jobs.len(), 2, "nightly + weekly jobs");
    assert!(state.jobs.contains_key("nightly"));
    assert!(state.jobs.contains_key("weekly"));
    assert_eq!(state.endpoints.len(), 1);
    assert_eq!(state.triggers.len(), 1);
    assert_eq!(
        state.views.len(),
        2,
        "view + materialized view share /server/views"
    );
    assert_eq!(state.policies.len(), 1);
    assert_eq!(state.webhooks.len(), 1);

    // The materialized view is flagged; the plain view is not.
    assert!(state.views.get("cached_view").unwrap().materialized);
    assert!(!state.views.get("recent_view").unwrap().materialized);

    // Freshness as data (blueprint §14 contract 2): a freshly-created materialized view reports
    // `null` freshness — a never-refreshed view is honest, never a fabricated timestamp.
    assert!(
        state.views.get("cached_view").unwrap().last_run.is_none(),
        "a never-refreshed materialized view reports null freshness"
    );

    // The DDL job desugared its EVERY/DO clauses onto the canonical fields.
    let nightly = state.jobs.get("nightly").unwrap();
    assert_eq!(nightly.every, "1h");
    assert!(!nightly.plan.as_str().is_empty(), "DO plan source recorded");
    // last_run is None on a fresh INSERT (replay-safe; the external runner records it).
    assert!(nightly.last_run.is_none());
}

#[test]
fn materialized_view_last_run_round_trips_readable() {
    use qfs_core::{Column, ColumnType, Row, RowBatch, Schema, Value};
    // A stored view config row a refresh has stamped with a LAST_RUN high-water mark reads back
    // with its freshness intact — the "freshness as data" round-trip (blueprint §14 contract 2).
    let mut state = crate::state::ServerState::default();
    let schema = Schema::new(vec![
        Column::new("name", ColumnType::Text, false),
        Column::new("query", ColumnType::Text, true),
        Column::new("materialized", ColumnType::Bool, false),
        Column::new("last_run", ColumnType::Timestamp, true),
    ]);
    let row = Row::new(vec![
        Value::Text("cached".into()),
        Value::Text("/mail/inbox".into()),
        Value::Bool(true),
        Value::Timestamp(1_751_600_000_000),
    ]);
    let batch = RowBatch::new(schema, vec![row]);
    crate::driver::apply_server_write(&mut state, ServerNode::Views, ServerWriteOp::Upsert, &batch)
        .expect("apply view row");
    assert_eq!(
        state.views.get("cached").unwrap().last_run,
        Some(1_751_600_000_000),
        "a materialized view's recorded freshness reads back as data"
    );
}

#[test]
fn materialized_view_refresh_executes_query_caches_rows_and_stamps_last_run() {
    let mut rt = Runtime::new();
    rt.apply_source(
        "test",
        1,
        "CREATE MATERIALIZED VIEW cached AS /local/tmp |> SELECT name",
    )
    .expect("create materialized view");

    let report = rt
        .refresh_materialized_view("cached", 1_751_700_000_000, |query| {
            assert!(
                StatementSpec::from_canonical(query).is_ok(),
                "refresh receives a rehydratable canonical stored query"
            );
            Ok(RowBatch::new(
                Schema::new(vec![Column::new("name", ColumnType::Text, false)]),
                vec![Row::new(vec![qfs_core::Value::Text("one.txt".into())])],
            ))
        })
        .expect("refresh succeeds");

    assert_eq!(report.rows, 1);
    assert_eq!(report.last_run, 1_751_700_000_000);
    let state = rt.snapshot();
    let view = state.views.get("cached").expect("view exists");
    assert_eq!(view.last_run, Some(1_751_700_000_000));
    let cached = view.cache_json.as_deref().expect("cache JSON is stored");
    assert!(
        cached.contains("one.txt"),
        "the materialized row snapshot is cached"
    );
}

#[test]
fn materialized_view_refresh_failure_does_not_stamp_freshness() {
    let mut rt = Runtime::new();
    rt.apply_source("test", 1, "CREATE MATERIALIZED VIEW cached AS /local/tmp")
        .expect("create materialized view");

    let err = rt
        .refresh_materialized_view("cached", 1_751_700_000_000, |_query| {
            Err("source unavailable".to_string())
        })
        .expect_err("refresh fails");

    assert_eq!(err.code(), "view_refresh");
    let state = rt.snapshot();
    let view = state.views.get("cached").expect("view exists");
    assert_eq!(view.last_run, None);
    assert_eq!(view.cache_json, None);
}

#[test]
fn boot_snapshot_serializes_deterministically() {
    let mut rt = Runtime::new();
    rt.boot(&fixture_path()).expect("boot");
    let a = serde_json::to_string(&rt.snapshot()).expect("serialize");

    // A second runtime booting the same file yields a byte-identical snapshot (BTreeMap
    // ordering + canonical row shape => deterministic, golden-stable serialization).
    let mut rt2 = Runtime::new();
    rt2.boot(&fixture_path()).expect("boot 2");
    let b = serde_json::to_string(&rt2.snapshot()).expect("serialize 2");
    assert_eq!(a, b, "snapshot is deterministic across boots");
}

#[test]
fn re_applying_the_same_config_is_idempotent() {
    // Boot once, snapshot; boot the SAME file again into the same runtime (UPSERT replay)
    // and assert the state is unchanged — boot is replay-safe (blueprint §7 idempotency).
    let mut rt = Runtime::new();
    rt.boot(&fixture_path()).expect("first boot");
    let first = rt.snapshot();
    rt.boot(&fixture_path()).expect("second boot (replay)");
    let second = rt.snapshot();
    assert_eq!(
        first, second,
        "re-applying the config is a no-op (UPSERT converges)"
    );
}

// ---- CREATE-vs-INSERT sugar equivalence (golden on the plan) ---------------

#[test]
fn create_job_and_insert_into_server_jobs_yield_identical_plans() {
    // The acceptance assertion: `CREATE JOB … EVERY … DO …` and the equivalent
    // `INSERT INTO /server/jobs …` must lower to IDENTICAL ServerConfigWrite plan nodes
    // (sugar equivalence). The desugar maps the DDL clauses (name/EVERY/DO) 1:1 onto the
    // INSERT columns (name/every/plan), so feeding the explicit write the SAME field values
    // the desugar produces yields a byte-identical plan node.
    // A job with no DO body (the `plan` field desugars to an empty string), so the equivalent
    // explicit write carries a literal `''` — an exact, round-trip-clean equivalence on the
    // load-bearing mapping (name/EVERY → node/op/args) without re-embedding rendered AST text.
    let create = lower("CREATE JOB nightly EVERY '1h'");

    // CREATE desugars to UPSERT (declarative make-exist), so the equivalent explicit write is
    // UPSERT with the same name/every/plan field values.
    let insert = lower("UPSERT INTO /server/jobs VALUES (name, every, plan) ('nightly', '1h', '')");

    // The plans are equal: same single ServerConfigWrite node, same target, same args.
    assert_eq!(
        create.nodes(),
        insert.nodes(),
        "CREATE JOB and UPSERT INTO /server/jobs must produce identical plan nodes"
    );
    // And the JSON PREVIEW projection (the golden the AI sees) matches too.
    assert_eq!(
        serde_json::to_string(&preview(&create)).unwrap(),
        serde_json::to_string(&preview(&insert)).unwrap(),
        "the PREVIEW golden is identical for the sugar and the explicit write"
    );

    // The single node is a ServerConfigWrite{Jobs, Upsert}, reversible, exact-1.
    let node = &create.nodes()[0];
    assert_eq!(
        node.kind,
        EffectKind::ServerConfigWrite {
            node: ServerNode::Jobs,
            op: ServerWriteOp::Upsert
        }
    );
    assert!(!node.irreversible, "config writes are reversible");
}

#[test]
fn materialized_view_sugar_routes_to_views_collection() {
    let plan = lower("CREATE MATERIALIZED VIEW cached AS /mail/inbox");
    let node = &plan.nodes()[0];
    assert_eq!(
        node.kind,
        EffectKind::ServerConfigWrite {
            node: ServerNode::Views,
            op: ServerWriteOp::Upsert
        }
    );
}

// ---- unsupported-verb plan-time rejection ----------------------------------

#[test]
fn unsupported_verb_is_rejected_at_plan_time_with_structured_error() {
    // The /server config nodes are relational tables: SELECT/INSERT/UPSERT/UPDATE/REMOVE.
    // They do NOT support blob verbs (LS/CP/MV/RM). The capability gate rejects an
    // unsupported verb at PLAN time with a structured error — no panic, no COMMIT.
    use qfs_core::{check_capability, Verb};
    let state = Arc::new(RwLock::new(ServerState::new()));
    let driver = ServerDriver::new(state.clone());

    let path = Path::new("/server/triggers");
    let err = check_capability(&driver, &path, Verb::Cp).expect_err("CP must be rejected");
    match err {
        qfs_core::CfsError::UnsupportedVerb {
            path: p,
            verb,
            supported,
        } => {
            assert_eq!(p, "/server/triggers");
            assert_eq!(verb, "CP");
            // The supported set is the relational verb set (machine-readable for AI).
            assert!(supported.contains(&"SELECT"));
            assert!(supported.contains(&"INSERT"));
            assert!(supported.contains(&"UPSERT"));
            assert!(supported.contains(&"REMOVE"));
            assert!(!supported.contains(&"CP"));
        }
        other => panic!("expected UnsupportedVerb, got {other:?}"),
    }

    // The state was never mutated by the rejected plan attempt.
    assert_eq!(state.read().unwrap().row_count(), 0);
}

// ---- DESCRIBE /server/triggers ---------------------------------------------

#[test]
fn describe_server_triggers_returns_the_trigger_schema_with_no_backend() {
    let state = Arc::new(RwLock::new(ServerState::new()));
    let driver = ServerDriver::new(state);
    let desc = driver
        .describe(&Path::new("/server/triggers"))
        .expect("describe triggers");
    assert_eq!(desc.archetype, qfs_core::Archetype::RelationalTable);
    let names: Vec<&str> = desc
        .schema
        .columns
        .iter()
        .map(|c| c.name.as_str())
        .collect();
    // t34 (CO-t31-4): the trigger schema gains the optional `predicate` (the `WHERE <pred>`
    // guard's canonical spec), between `on` and `plan`. t35 appends the optional `policy`
    // attachment handle (the fired-plan least-privilege ref).
    assert_eq!(names, vec!["name", "on", "predicate", "plan", "policy"]);
}

#[test]
fn describe_unknown_server_node_is_structured_error() {
    let state = Arc::new(RwLock::new(ServerState::new()));
    let driver = ServerDriver::new(state);
    let err = driver
        .describe(&Path::new("/server/nope"))
        .expect_err("unknown node");
    assert_eq!(err.code(), "unsupported_verb");
}

// ---- /server/jobs/<name>/runs (the READ-ONLY run history) -------------------

#[test]
fn job_runs_path_resolves_only_the_exact_runs_shape() {
    use crate::driver::job_runs_path_job;
    assert_eq!(
        job_runs_path_job("/server/jobs/nightly/runs"),
        Some("nightly")
    );
    assert_eq!(
        job_runs_path_job("server/jobs/nightly/runs"),
        Some("nightly")
    );
    // Everything else falls through to the ordinary node resolution.
    assert_eq!(job_runs_path_job("/server/jobs"), None);
    assert_eq!(job_runs_path_job("/server/jobs/nightly"), None);
    assert_eq!(job_runs_path_job("/server/jobs/nightly/runs/extra"), None);
    assert_eq!(job_runs_path_job("/server/triggers/x/runs"), None);
    assert_eq!(job_runs_path_job("/server/jobs//runs"), None);
}

#[test]
fn describe_job_runs_returns_the_runs_schema_not_the_jobs_schema() {
    let state = Arc::new(RwLock::new(ServerState::new()));
    let driver = ServerDriver::new(state);
    let desc = driver
        .describe(&Path::new("/server/jobs/nightly/runs"))
        .expect("describe runs");
    assert_eq!(desc.archetype, qfs_core::Archetype::RelationalTable);
    let names: Vec<&str> = desc
        .schema
        .columns
        .iter()
        .map(|c| c.name.as_str())
        .collect();
    assert_eq!(
        names,
        // blueprint §19 axis B/D adds the secret-free firing `principal` column.
        vec!["scheduled_at", "outcome", "detail", "affected", "principal"],
        "the runs sub-collection must NOT fall through to its /server/jobs prefix"
    );
}

#[test]
fn job_runs_capabilities_are_select_only() {
    let state = Arc::new(RwLock::new(ServerState::new()));
    let driver = ServerDriver::new(state);
    let caps = driver.capabilities(&Path::new("/server/jobs/nightly/runs"));
    let labels = caps.supported_labels();
    assert!(labels.contains(&"SELECT"), "the run history is readable");
    for verb in ["INSERT", "UPSERT", "UPDATE", "REMOVE"] {
        assert!(
            !labels.contains(&verb),
            "run history is runtime telemetry — {verb} must be structurally refused"
        );
    }
}

#[test]
fn removing_a_job_drops_its_run_history() {
    let mut state = ServerState::new();
    state.jobs.insert(
        "nightly".into(),
        JobDef {
            name: "nightly".into(),
            every: "1h".into(),
            plan: StatementSource("insert into /log/x values ('hi')".into()),
            last_run: None,
            policy: None,
        },
    );
    state.record_job_run(
        "nightly",
        JobRunRecord {
            scheduled_at: 1,
            outcome: "fired".into(),
            detail: String::new(),
            affected: 1,
            principal: String::new(),
        },
    );
    assert_eq!(state.job_runs["nightly"].len(), 1);

    let args = qfs_core::RowBatch::new(
        qfs_core::Schema::new(vec![qfs_core::Column::new(
            "name",
            qfs_core::ColumnType::Text,
            false,
        )]),
        vec![qfs_core::Row::new(vec![qfs_core::Value::Text(
            "nightly".into(),
        )])],
    );
    apply_server_write(&mut state, ServerNode::Jobs, ServerWriteOp::Remove, &args)
        .expect("remove job");
    assert!(state.jobs.is_empty());
    assert!(
        state.job_runs.is_empty(),
        "telemetry lives and dies with its job row"
    );
}

#[test]
fn record_job_run_caps_the_history_newest_kept() {
    let mut state = ServerState::new();
    for i in 0..(crate::JOB_RUN_HISTORY_CAP as i64 + 10) {
        state.record_job_run(
            "busy",
            JobRunRecord {
                scheduled_at: i,
                outcome: "denied".into(),
                detail: "default-deny".into(),
                affected: 0,
                principal: String::new(),
            },
        );
    }
    let runs = &state.job_runs["busy"];
    assert_eq!(runs.len(), crate::JOB_RUN_HISTORY_CAP, "bounded history");
    assert_eq!(
        runs.first().map(|r| r.scheduled_at),
        Some(10),
        "the oldest records are dropped"
    );
    assert_eq!(
        runs.last().map(|r| r.scheduled_at),
        Some(crate::JOB_RUN_HISTORY_CAP as i64 + 9),
        "the newest record is kept"
    );
}

// ---- binding reconcile-once-per-mutation -----------------------------------

#[test]
fn null_binding_reconcile_invoked_once_per_committed_mutation() {
    // The acceptance assertion via a counting double: each committed /server mutation
    // triggers exactly one reconcile. The fixture has 8 statements => 8 mutations, plus the
    // final end-of-boot reconcile => 9 reconcile calls. We assert the per-mutation count by
    // applying statements one at a time.
    // Apply two mutations directly; each should reconcile every binding exactly once.
    let probe = CountingProbe::default();
    let mut rt = Runtime::new().with_binding(Box::new(probe.binding()));

    rt.apply_source("test", 1, "CREATE JOB a EVERY '1h' DO REMOVE /tmp")
        .expect("apply 1");
    assert_eq!(
        probe.reconciles(),
        1,
        "one reconcile after the first mutation"
    );

    rt.apply_source("test", 2, "CREATE JOB b EVERY '2h' DO REMOVE /tmp")
        .expect("apply 2");
    assert_eq!(
        probe.reconciles(),
        2,
        "one more reconcile after the second mutation"
    );

    // A statement that produces no mutation should not reconcile (none here; covered by the
    // not-server-config rejection test instead).
    assert_eq!(rt.snapshot().jobs.len(), 2);
}

#[test]
fn counting_binding_sees_post_mutation_snapshot() {
    // The reconcile snapshot reflects the JUST-applied mutation (read snapshot is taken
    // AFTER the commit), so the binding converges to the new registry.
    let probe = CountingProbe::default();
    let mut rt = Runtime::new().with_binding(Box::new(probe.binding()));
    rt.apply_source("test", 1, "CREATE JOB a EVERY '1h' DO REMOVE /tmp")
        .expect("apply");
    // Snapshot row_count seen by reconcile is 1 (the job just inserted).
    assert_eq!(probe.last_row_count(), Some(1));
}

// ---- purity: building a /server write plan mutates nothing -----------------

#[test]
fn building_a_server_write_plan_mutates_nothing_until_commit() {
    let state = Arc::new(RwLock::new(ServerState::new()));
    assert_eq!(state.read().unwrap().row_count(), 0);

    // Build (lower) a /server write plan — pure, no state handle even involved.
    let plan = lower("CREATE JOB nightly EVERY '1h' DO REMOVE /tmp");
    assert_eq!(plan.nodes().len(), 1);
    // The state is still empty: lowering performed no mutation.
    assert_eq!(state.read().unwrap().row_count(), 0);

    // Only COMMIT (driving the ServerConfigApplier) mutates the state.
    let mut applier = ServerConfigApplier::new(&state);
    let report = commit(&plan, &mut applier, |_| {});
    assert!(report.failed.is_none());
    assert_eq!(
        state.read().unwrap().jobs.len(),
        1,
        "COMMIT mutated the state"
    );
}

#[test]
fn insert_then_remove_round_trips() {
    let state = Arc::new(RwLock::new(ServerState::new()));
    let insert = lower("UPSERT INTO /server/webhooks VALUES (name, route) ('h', '/hooks/x')");
    let mut a = ServerConfigApplier::new(&state);
    assert!(commit(&insert, &mut a, |_| {}).failed.is_none());
    assert_eq!(state.read().unwrap().webhooks.len(), 1);

    let remove = lower("REMOVE /server/webhooks/h");
    let mut b = ServerConfigApplier::new(&state);
    assert!(commit(&remove, &mut b, |_| {}).failed.is_none());
    assert_eq!(
        state.read().unwrap().webhooks.len(),
        0,
        "REMOVE undid the insert"
    );
}

// ---- audit ledger ----------------------------------------------------------

#[test]
fn boot_records_one_audit_entry_per_mutation_and_drain_flushes() {
    let mut rt = Runtime::new();
    rt.boot(&fixture_path()).expect("boot");
    // 8 statements => 8 committed mutations => 8 audit entries.
    assert_eq!(rt.audit().len(), 8, "one audit entry per /server mutation");
    // The entries are secret-free (names + ops only).
    for entry in rt.audit().snapshot() {
        let s = entry.summary();
        assert!(s.contains("/server/"), "entry names its node: {s}");
    }
    // Drain flushes and reports the count.
    assert_eq!(rt.audit().drain(), 8);
}

// ---- run loop shutdown (audit drain) ---------------------------------------

#[test]
fn run_loop_drains_audit_on_ctrl_c() {
    // The run loop blocks on ctrl_c then drains. We cannot raise SIGINT portably in a unit
    // test, so we exercise the drain directly (the run loop's shutdown step), proving the
    // audit ledger is flushed on exit. The full ctrl_c path is exercised by `qfs serve` E2E.
    let mut rt = Runtime::new();
    rt.boot(&fixture_path()).expect("boot");
    let audit = rt.audit().clone();
    assert!(!audit.is_empty());
    let drained = audit.drain();
    assert_eq!(drained, 8, "shutdown drains every recorded entry");
}

// ---- not-server-config rejection -------------------------------------------

#[test]
fn boot_rejects_a_non_server_statement_with_a_line_located_error() {
    let mut rt = Runtime::new();
    // A pure read pipeline is not a /server config write — boot rejects it, line-located.
    let err = rt
        .apply_source("test", 7, "/mail/inbox |> LIMIT 5")
        .expect_err("non-server statement rejected");
    match err {
        ServerError::NotServerConfig { line, .. } => assert_eq!(line, 7),
        other => panic!("expected NotServerConfig, got {other:?}"),
    }
    assert_eq!(err.code(), "not_server_config");
    // Nothing was committed.
    assert_eq!(rt.snapshot().row_count(), 0);
}

#[test]
fn insert_duplicate_into_server_is_rejected_use_upsert() {
    // INSERT (not UPSERT) rejects a duplicate at COMMIT — the verb distinction is honored.
    let state = Arc::new(RwLock::new(ServerState::new()));
    let first = lower("INSERT INTO /server/jobs VALUES (name, every) ('j', '1h')");
    let mut a = ServerConfigApplier::new(&state);
    assert!(commit(&first, &mut a, |_| {}).failed.is_none());

    let dup = lower("INSERT INTO /server/jobs VALUES (name, every) ('j', '2h')");
    let mut b = ServerConfigApplier::new(&state);
    let report = commit(&dup, &mut b, |_| {});
    assert!(
        report.failed.is_some(),
        "duplicate INSERT fails (use UPSERT)"
    );
    // The original row is unchanged.
    assert_eq!(state.read().unwrap().jobs.get("j").unwrap().every, "1h");
}

// ---- counting-probe test harness -------------------------------------------

/// A shared counter a [`Binding`] increments on each reconcile (a test double whose count
/// is readable from outside the runtime that owns the boxed binding).
#[derive(Clone, Default)]
struct CountingProbe {
    inner: Arc<RwLock<(usize, Option<usize>)>>,
}

impl CountingProbe {
    fn binding(&self) -> ProbeBinding {
        ProbeBinding {
            inner: self.inner.clone(),
        }
    }
    fn reconciles(&self) -> usize {
        self.inner.read().unwrap().0
    }
    fn last_row_count(&self) -> Option<usize> {
        self.inner.read().unwrap().1
    }
}

/// A [`Binding`] backed by a shared [`CountingProbe`] counter.
struct ProbeBinding {
    inner: Arc<RwLock<(usize, Option<usize>)>>,
}

impl Binding for ProbeBinding {
    fn kind(&self) -> BindingKind {
        BindingKind::Null
    }
    fn reconcile(&mut self, state: &ServerState) -> Result<(), ServerError> {
        let mut g = self.inner.write().unwrap();
        g.0 += 1;
        g.1 = Some(state.row_count());
        Ok(())
    }
}

#[test]
fn reconfigure_handle_commits_notify_the_runtime_to_audit_and_reconcile() {
    // Blueprint §16 write leg: the statement bridge applies a ServerConfigWrite into the SHARED
    // state lock, then hands the changes to the runtime — which records the audit entries and
    // runs the same reconcile_all() an operator apply_source triggers.
    let state = Arc::new(RwLock::new(ServerState::new()));
    let (handle, rx) = crate::runtime::reconfigure_channel(Arc::clone(&state));
    let mut runtime = Runtime::with_shared(Arc::clone(&state), rx)
        .with_binding(Box::new(CountingBinding::default()));

    // The bridge-side commit: apply into the shared lock through the live applier.
    let plan = lower("CREATE WEBHOOK ingest ON '/hooks/ingest'");
    let mut applier = ServerConfigApplier::new(handle.state());
    let report = commit(&plan, &mut applier, |_| {});
    assert!(report.failed.is_none());
    let changes = applier.into_changes();
    assert_eq!(changes.len(), 1);
    assert_eq!(
        state.read().unwrap().webhooks.len(),
        1,
        "the network commit mutated the SAME lock the runtime owns"
    );

    // The runtime services the notification: one audit entry per change + reconcile_all().
    runtime
        .handle_reconfigure(&changes)
        .expect("reconcile succeeds");
    assert_eq!(runtime.audit().len(), 1, "one audit entry per change");
    // An empty notification still reconciles but records nothing.
    runtime
        .handle_reconfigure(&[])
        .expect("empty is a no-op reconcile");
    assert_eq!(runtime.audit().len(), 1, "no change, no audit entry");
}

// The splitter itself moved to `qfs_core::ddl::document` and is tested there against the full
// comment/string/path table. What belongs HERE is that boot — this crate's `.qfs` consumer —
// actually reaches it: a `--` inside a path used to truncate the statement, swallow its
// terminating `;`, and merge the next statement into it, surfacing as a bogus parse error on the
// wrong line. The provisioning loader has the twin of this test, so the two consumers are pinned
// to the same answer.
#[test]
fn boot_applies_a_statement_whose_path_contains_a_double_dash() {
    let dir = std::env::temp_dir().join("qfs-boot-dashes");
    std::fs::create_dir_all(&dir).expect("temp dir");
    let cfg = dir.join("boot_dashes.qfs");
    std::fs::write(
        &cfg,
        "CREATE POLICY p ALLOW SELECT ON /local/a--b/*;\nCREATE JOB j EVERY '1h';\n",
    )
    .expect("write config");

    let mut rt = Runtime::new();
    rt.boot(&cfg)
        .expect("a `--` inside a path must not break boot");

    let snap = rt.snapshot();
    assert_eq!(snap.policies.len(), 1, "the policy statement applied");
    assert_eq!(
        snap.jobs.len(),
        1,
        "the statement after the `--` path applied too — its `;` was not swallowed"
    );
}
