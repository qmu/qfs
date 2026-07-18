//! The `qfs serve` **cron sweeper composition** (blueprint §10, the 2026-07-11 t65 reversal): the
//! daemon leaf that drives the pure `qfs_watchtower::cron` firing decision on a real clock.
//!
//! The division of labour is the watchtower pattern verbatim:
//!   * the DECISION (`is_due` / `fire_due`) is pure over an injected `now` and lives in
//!     `qfs-watchtower` (wasm-clean, hermetically tested there);
//!   * the GATES (t35 policy default-deny + t37 `IrreversibleGuard`, `RunMode::Server`) run inside
//!     the injected [`Committer`] — scheduling bypasses neither;
//!   * this module owns the impure leaf: [`LiveCronCommitter`] (the gates + the REAL
//!     `crate::commit::apply_plan`, the same runtime interpreter + live registry `qfs job run
//!     --commit` uses), [`sweep_once`] (one sweep: hydrate durable `last_run` marks → fire →
//!     record runs + stamps), and [`spawn_sweeper`] (the `tokio::time` interval that feeds
//!     `SystemTime::now()` in — the ONLY wall-clock read, exactly like the watchtower bus).
//!
//! **Ruled semantics** (blueprint §10): missed-fire collapses to one catch-up (`fire_due`);
//! overlap = skip-if-running — held STRUCTURALLY here because sweeps run sequentially (the loop
//! awaits each sweep before the next tick can start); UTC only (epoch seconds end to end).
//!
//! **What survives a restart**: each committed fire's `last_run` is stamped into the shared
//! [`ServerState`] (the `/server/jobs` read-back) AND persisted through the daemon's fsync'd
//! [`DurableStore`] (`cron/<job>/last_run`); [`sweep_once`] re-hydrates the durable mark into the
//! state on every sweep, so a rebooted daemon (whose config replay reset `last_run`) does not
//! re-fire early. Every attempted firing appends a [`JobRunRecord`] to the state's
//! `/server/jobs/<name>/runs` history and a line to the on-disk audit ledger.

use std::collections::BTreeSet;
use std::sync::{Arc, RwLock};

use qfs_core::Engine;
use qfs_host::{AuditLedger, DurableStore, RuntimeHost, StateBytes, StateKey};
use qfs_provision::{JobRunRecord, ServerState};
use qfs_watchtower::cron::{fire_due, fire_due_agents, CronOutcome, CronRun};
use qfs_watchtower::{Committer, FireError, FireOutcome, PolicyTableHandle};

/// The daemon sweep cadence. The `EVERY` grammar bottoms out at minutes (`m`), so a 30s sweep
/// bounds firing latency at half the smallest expressible interval.
pub const SWEEP_INTERVAL_SECS: u64 = 30;

/// The live [`Committer`] the sweeper injects into `fire_due`: the SAME gate chain as
/// `qfs_watchtower::RecordingCommitter` (build → t35 policy default-deny → t37 irreversible
/// guard, `RunMode::Server`, no ack), but the commit runs through [`crate::commit::apply_plan`]
/// — the real runtime interpreter over the live driver registry, so a fired JOB audits (t76) and
/// emits telemetry (t77) exactly like `qfs job run --commit`.
pub struct LiveCronCommitter {
    engine: Engine,
    policies: PolicyTableHandle,
}

impl LiveCronCommitter {
    /// Build over a clone of the serve engine's registries + the live `/server/policies` handle
    /// (the same pair the watchtower dispatch committer gets).
    #[must_use]
    pub fn new(engine: Engine, policies: PolicyTableHandle) -> Self {
        Self { engine, policies }
    }

    /// Snapshot the live policy table (clones the inner `Arc`; the guard drops at once).
    fn policy_snapshot(&self) -> qfs_host::PolicyTable {
        self.policies
            .read()
            .map(|g| (**g).clone())
            .unwrap_or_default()
    }
}

impl LiveCronCommitter {
    /// The shared gate→irreversible→apply chain, evaluated under the firing principal's decision
    /// context (blueprint §19 axis B/D): `principal = Some(agent)` gates under the AGENT subject
    /// (`DecisionContext::for_agent`) — an agent fire commits by the agent's grants; `None` is the
    /// operator/anonymous context (an ordinary `/server/jobs` fire, unchanged). Everything else —
    /// the IrreversibleGuard (`RunMode::Server`, no ack), the real applier — is identical.
    fn commit_inner(
        &self,
        stmt: &qfs_watchtower::Statement,
        policy: Option<&str>,
        principal: Option<&str>,
    ) -> Result<FireOutcome, FireError> {
        let plan = qfs_exec::build_plan(stmt, &self.engine)
            .map_err(|e| FireError::Build(e.to_string()))?;

        // Policy gate: resolve the bound policy against the live table and run the pure enforcer
        // BEFORE any apply, UNDER THE FIRING PRINCIPAL'S CONTEXT. No policy / a dangling ref ⇒
        // fail-closed default-deny; atomic abort on deny (zero effects).
        let table = self.policy_snapshot();
        let resolved = qfs_host::resolve_policy(policy, &table);
        let gate = match principal {
            Some(agent) => {
                let ctx = qfs_host::DecisionContext::for_agent(agent);
                qfs_host::gate_plan_with_context(&resolved, &plan, &ctx)
            }
            None => qfs_host::gate_plan(&resolved, &plan),
        };
        let effects = gate.effects.clone();
        if let qfs_host::PolicyDecision::Deny {
            verb, driver, rule, ..
        } = &gate.decision
        {
            return Err(FireError::PolicyDenied {
                reason: gate.deny_reason().unwrap_or_default(),
                verb: verb.label().to_string(),
                driver: driver.clone(),
                rule: *rule,
                effects,
            });
        }

        // Irreversible gate: a scheduled fire is UNATTENDED (`RunMode::Server`), so an irreversible
        // REMOVE / declared-irreversible CALL is refused fail-closed — the ruled property
        // (blueprint §19): an agent never fires an irreversible plan unattended.
        if let Err(needs) = qfs_core::IrreversibleGuard::require_ack(
            &plan,
            qfs_core::RunMode::Server,
            qfs_core::Ack::Absent,
        ) {
            return Err(FireError::IrreversibleBlocked(needs.reason().to_string()));
        }

        let preview = qfs_core::preview(&plan);
        let plan_summary = qfs_crypto_core::sha256_hex(format!("{preview:?}").as_bytes());
        crate::commit::apply_plan(&plan).map_err(|e| FireError::Apply(e.to_string()))?;
        Ok(FireOutcome {
            plan_summary,
            affected: effects.len() as u64,
            effects,
        })
    }
}

impl Committer for LiveCronCommitter {
    fn commit(
        &self,
        trigger: &str,
        stmt: &qfs_watchtower::Statement,
        policy: Option<&str>,
    ) -> Result<FireOutcome, FireError> {
        let _ = trigger;
        self.commit_inner(stmt, policy, None)
    }

    fn commit_for_principal(
        &self,
        trigger: &str,
        stmt: &qfs_watchtower::Statement,
        policy: Option<&str>,
        principal: Option<&str>,
    ) -> Result<FireOutcome, FireError> {
        let _ = trigger;
        self.commit_inner(stmt, policy, principal)
    }
}

/// One sweep over the live job table at `now` (UTC epoch seconds, injected — hermetic tests drive
/// the exact instant): hydrate durable `last_run` marks into the snapshot + state, run the pure
/// `fire_due` through the committer, then persist what happened (run history + `last_run` stamps
/// under one write guard, the durable mark, the ledger line). Returns the run records.
pub fn sweep_once(
    state: &Arc<RwLock<ServerState>>,
    committer: &dyn Committer,
    durable: &dyn DurableStore,
    ledger: Option<&AuditLedger>,
    now: i64,
) -> Vec<CronRun> {
    // 1. Snapshot the jobs and hydrate each `last_run` from the durable store (restart safety:
    // the config replay leaves `last_run` wherever the config document had it — usually unset —
    // while the durable mark survived the restart; the newer of the two wins).
    let mut jobs: Vec<qfs_provision::JobDef> = state
        .read()
        .map(|g| g.jobs.values().cloned().collect())
        .unwrap_or_default();
    let mut hydrated: Vec<(String, i64)> = Vec::new();
    for job in &mut jobs {
        if let Some(mark) = read_last_run(durable, &job.name) {
            if job.last_run.is_none_or(|l| l < mark) {
                job.last_run = Some(mark);
                hydrated.push((job.name.clone(), mark));
            }
        }
    }
    if !hydrated.is_empty() {
        if let Ok(mut guard) = state.write() {
            for (name, mark) in hydrated {
                if let Some(job) = guard.jobs.get_mut(&name) {
                    if job.last_run.is_none_or(|l| l < mark) {
                        job.last_run = Some(mark);
                    }
                }
            }
        }
    }

    // 2. The pure decision + gated fire. The in-flight set is empty by construction: sweeps run
    // sequentially (the daemon loop awaits each sweep), so a job can never overlap itself — the
    // ruled skip-if-running holds structurally.
    let runs = fire_due(&jobs, now, &BTreeSet::new(), committer);

    // 3. Persist: run history + `last_run` stamps under ONE write guard (a `/server` reader sees
    // the run and its stamp together), then the durable mark and the audit line per run.
    if let Ok(mut guard) = state.write() {
        for run in &runs {
            guard.record_job_run(&run.job, run_record(run));
            if let Some(stamp) = run.stamp_last_run {
                if let Some(job) = guard.jobs.get_mut(&run.job) {
                    job.last_run = Some(stamp);
                }
            }
        }
    }
    for run in &runs {
        if let Some(stamp) = run.stamp_last_run {
            write_last_run(durable, &run.job, stamp);
        }
        if let Some(ledger) = ledger {
            let _ = ledger.append(&ledger_line(run));
        }
    }

    // 4. The AGENT cadence pass (blueprint §19 axis D): agents with a launch cadence ride the SAME
    // sweep — no new scheduler. Hydrate each agent's durable `last_run`, run the pure
    // `fire_due_agents` (which threads the agent as the firing principal so the committer gates under
    // the agent subject), then persist to the agent's OWN run history + `last_run` + durable mark +
    // ledger. Returned appended to the job runs so the daemon loop logs both.
    let mut agents: Vec<qfs_provision::AgentDef> = state
        .read()
        .map(|g| g.agents.values().cloned().collect())
        .unwrap_or_default();
    let mut agent_hydrated: Vec<(String, i64)> = Vec::new();
    for agent in &mut agents {
        if let Some(mark) = read_agent_last_run(durable, &agent.name) {
            if agent.last_run.is_none_or(|l| l < mark) {
                agent.last_run = Some(mark);
                agent_hydrated.push((agent.name.clone(), mark));
            }
        }
    }
    if !agent_hydrated.is_empty() {
        if let Ok(mut guard) = state.write() {
            for (name, mark) in agent_hydrated {
                if let Some(agent) = guard.agents.get_mut(&name) {
                    if agent.last_run.is_none_or(|l| l < mark) {
                        agent.last_run = Some(mark);
                    }
                }
            }
        }
    }

    let agent_runs = fire_due_agents(&agents, now, &BTreeSet::new(), committer);

    if let Ok(mut guard) = state.write() {
        for run in &agent_runs {
            guard.record_agent_run(&run.job, run_record(run));
            if let Some(stamp) = run.stamp_last_run {
                if let Some(agent) = guard.agents.get_mut(&run.job) {
                    agent.last_run = Some(stamp);
                }
            }
        }
    }
    for run in &agent_runs {
        if let Some(stamp) = run.stamp_last_run {
            write_agent_last_run(durable, &run.job, stamp);
        }
        if let Some(ledger) = ledger {
            let _ = ledger.append(&ledger_line(run));
        }
    }

    let mut all = runs;
    all.extend(agent_runs);
    all
}

/// Spawn the daemon sweep loop: a `tokio::time` interval feeding `SystemTime::now()` into
/// [`sweep_once`] every [`SWEEP_INTERVAL_SECS`]. Each sweep runs on the blocking pool
/// ([`crate::commit::apply_plan`] builds its own current-thread runtime, which must never nest
/// inside an async worker) and is awaited before the next tick fires — sweeps are sequential, so
/// the ruled overlap-skip holds structurally. Runs until aborted at shutdown.
pub fn spawn_sweeper(
    state: Arc<RwLock<ServerState>>,
    committer: Arc<dyn Committer>,
    host: Arc<crate::host::TokioHost>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval =
            tokio::time::interval(std::time::Duration::from_secs(SWEEP_INTERVAL_SECS));
        // A sweep that outruns its tick (a slow commit) skips the missed ticks instead of
        // bursting to catch up — the missed-fire semantics already collapse to one fire.
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            let state = Arc::clone(&state);
            let committer = Arc::clone(&committer);
            let host = Arc::clone(&host);
            let swept = tokio::task::spawn_blocking(move || {
                sweep_once(
                    &state,
                    committer.as_ref(),
                    host.durable(),
                    Some(host.ledger()),
                    now_secs(),
                )
            })
            .await;
            if let Ok(runs) = swept {
                for run in &runs {
                    tracing::info!(
                        target: "qfs::cron",
                        job = %run.job,
                        outcome = ?run.outcome,
                        scheduled_at = run.scheduled_at,
                        "cron sweep firing"
                    );
                }
            }
        }
    })
}

/// The durable key carrying a job's persisted `last_run` high-water mark.
fn last_run_key(job: &str) -> StateKey {
    StateKey::new(format!("cron/{job}/last_run"))
}

/// Read a job's durable `last_run` mark (epoch seconds), `None` if unset/unreadable.
fn read_last_run(durable: &dyn DurableStore, job: &str) -> Option<i64> {
    let key = last_run_key(job);
    let bytes = qfs_host::block_on(durable.get(&key)).ok()??;
    String::from_utf8(bytes.0).ok()?.trim().parse().ok()
}

/// Persist a job's `last_run` mark durably (fsync'd; last-writer-wins — the sweep loop is the
/// only writer). A write failure is logged, never fatal: the in-state stamp still holds for this
/// process, and the worst restart cost is one early catch-up fire.
fn write_last_run(durable: &dyn DurableStore, job: &str, stamp: i64) {
    let key = last_run_key(job);
    if let Err(e) =
        qfs_host::block_on(durable.put(&key, StateBytes(stamp.to_string().into_bytes())))
    {
        tracing::warn!(target: "qfs::cron", job = %job, error = %e, "durable last_run write failed");
    }
}

/// The durable key carrying an AGENT's persisted cadence `last_run` mark (blueprint §19 axis D).
/// A separate `cron/agent/<name>/` namespace so an agent and a job of the same name never collide.
fn agent_last_run_key(agent: &str) -> StateKey {
    StateKey::new(format!("cron/agent/{agent}/last_run"))
}

/// Read an agent's durable cadence `last_run` mark (epoch seconds), `None` if unset/unreadable.
fn read_agent_last_run(durable: &dyn DurableStore, agent: &str) -> Option<i64> {
    let key = agent_last_run_key(agent);
    let bytes = qfs_host::block_on(durable.get(&key)).ok()??;
    String::from_utf8(bytes.0).ok()?.trim().parse().ok()
}

/// Persist an agent's cadence `last_run` mark durably (fsync'd). Non-fatal on failure, like the job
/// path — the worst restart cost is one early catch-up fire.
fn write_agent_last_run(durable: &dyn DurableStore, agent: &str, stamp: i64) {
    let key = agent_last_run_key(agent);
    if let Err(e) =
        qfs_host::block_on(durable.put(&key, StateBytes(stamp.to_string().into_bytes())))
    {
        tracing::warn!(target: "qfs::cron", agent = %agent, error = %e, "durable agent last_run write failed");
    }
}

/// Map one firing to its `/server/jobs/<name>/runs` record (secret-free by construction — the
/// outcome reasons come from the committer's secret-free errors).
fn run_record(run: &CronRun) -> JobRunRecord {
    let (outcome, detail, affected) = match &run.outcome {
        CronOutcome::Fired { affected } => ("fired", String::new(), *affected as i64),
        CronOutcome::Denied { reason } => ("denied", reason.clone(), 0),
        CronOutcome::Blocked { reason } => ("blocked", reason.clone(), 0),
        CronOutcome::Failed { reason } => ("failed", reason.clone(), 0),
    };
    JobRunRecord {
        scheduled_at: run.scheduled_at,
        outcome: outcome.to_string(),
        detail,
        affected,
        // blueprint §19 axis B/D: an agent-cadence fire carries `agent:<name>` from the firing
        // DecisionContext; a plain `/server/jobs` fire has no agent principal (empty).
        principal: run.principal.clone().unwrap_or_default(),
    }
}

/// The one-line audit-ledger projection of a firing (secret-free: name + outcome + firing
/// principal). blueprint §19 axis B/D: an agent-fired plan records `principal=agent:<name>`; a
/// principal-less ordinary job fire omits the field.
fn ledger_line(run: &CronRun) -> String {
    let record = run_record(run);
    let principal = if record.principal.is_empty() {
        String::new()
    } else {
        format!(" principal={}", record.principal)
    };
    format!(
        "cron fire job={} outcome={} affected={} at={}{principal}",
        run.job, record.outcome, record.affected, run.scheduled_at
    )
}

/// The current epoch second — the daemon leaf's ONLY wall-clock read (the pure decision takes it
/// injected), mirroring the watchtower bus.
fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;
    use qfs_provision::JobDef;
    use qfs_provision::StatementSource;

    /// A fixed-verdict committer (the same seam the watchtower cron tests use) so a sweep test
    /// drives the persistence half without a live engine.
    struct MockCommitter {
        verdict: fn() -> Result<FireOutcome, FireError>,
    }
    impl Committer for MockCommitter {
        fn commit(
            &self,
            _trigger: &str,
            _stmt: &qfs_watchtower::Statement,
            _policy: Option<&str>,
        ) -> Result<FireOutcome, FireError> {
            (self.verdict)()
        }
    }
    fn allow() -> MockCommitter {
        MockCommitter {
            verdict: || {
                Ok(FireOutcome {
                    plan_summary: "sha-abc".into(),
                    affected: 1,
                    effects: vec!["UPSERT local:/x".into()],
                })
            },
        }
    }
    fn deny() -> MockCommitter {
        MockCommitter {
            verdict: || {
                Err(FireError::PolicyDenied {
                    reason: "default-deny (no matching rule)".into(),
                    verb: "UPSERT".into(),
                    driver: "local".into(),
                    rule: None,
                    effects: vec![],
                })
            },
        }
    }

    /// The `plan` column exactly as the real install path writes it: the canonical serialized
    /// `PlanSpec` (AST-JSON), NOT statement text. Firing reads it back with
    /// `PlanSpec::from_canonical` — so a test that stored raw text (the round-9 drift) would fail
    /// to rehydrate. Every job helper here builds through this to stay on the real stored format.
    fn plan_col(src: &str) -> StatementSource {
        StatementSource(
            qfs_core::PlanSpec::from_statement(qfs_exec::parse(src).unwrap()).canonical(),
        )
    }

    fn job(name: &str, every: &str, last_run: Option<i64>) -> JobDef {
        JobDef {
            name: name.into(),
            every: every.into(),
            plan: plan_col("upsert into /local/tmp/x values ('hi')"),
            last_run,
            policy: Some("p".into()),
        }
    }

    fn state_with(jobs: Vec<JobDef>) -> Arc<RwLock<ServerState>> {
        let mut s = ServerState::new();
        for j in jobs {
            s.jobs.insert(j.name.clone(), j);
        }
        Arc::new(RwLock::new(s))
    }

    fn agent(name: &str, every: &str, last_run: Option<i64>) -> qfs_provision::AgentDef {
        qfs_provision::AgentDef {
            name: name.into(),
            every: every.into(),
            last_run,
            plan: plan_col("upsert into /local/tmp/x values ('hi')"),
            policy: Some("p".into()),
        }
    }

    fn state_with_agents(agents: Vec<qfs_provision::AgentDef>) -> Arc<RwLock<ServerState>> {
        let mut s = ServerState::new();
        for a in agents {
            s.agents.insert(a.name.clone(), a);
        }
        Arc::new(RwLock::new(s))
    }

    fn tempdir_host() -> (tempfile::TempDir, crate::host::TokioHost) {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let host = crate::host::TokioHost::open(dir.path()).expect("host");
        (dir, host)
    }

    /// A committed fire stamps `last_run` (state + durable), appends the run record, and writes
    /// the ledger line — the whole persistence contract of one sweep, hermetically.
    #[test]
    fn sweep_once_fires_a_due_job_and_persists_everything() {
        let (dir, host) = tempdir_host();
        let state = state_with(vec![job("nightly", "1m", None)]);

        let runs = sweep_once(&state, &allow(), host.durable(), Some(host.ledger()), 5_000);
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].outcome, CronOutcome::Fired { affected: 1 });

        let g = state.read().unwrap();
        assert_eq!(
            g.jobs["nightly"].last_run,
            Some(5_000),
            "the committed fire stamps last_run into the shared state"
        );
        let history = &g.job_runs["nightly"];
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].outcome, "fired");
        assert_eq!(history[0].scheduled_at, 5_000);
        assert_eq!(history[0].affected, 1);
        drop(g);

        assert_eq!(
            read_last_run(host.durable(), "nightly"),
            Some(5_000),
            "the durable mark survives (what a restart re-hydrates from)"
        );
        let ledger = std::fs::read_to_string(dir.path().join("audit.log")).expect("ledger");
        assert!(
            ledger.contains("cron fire job=nightly outcome=fired affected=1"),
            "the firing is on the on-disk audit ledger: {ledger}"
        );
    }

    /// Restart safety: the durable mark hydrates a config-reset `last_run`, so a rebooted daemon
    /// does NOT re-fire a job whose interval has not elapsed — and the hydrated mark becomes
    /// visible on the `/server/jobs` read-back.
    #[test]
    fn sweep_once_hydrates_last_run_from_the_durable_store() {
        let (_dir, host) = tempdir_host();
        // The previous daemon stamped 4_990; the restart replayed the config, so state says None.
        write_last_run(host.durable(), "nightly", 4_990);
        let state = state_with(vec![job("nightly", "1m", None)]);

        let runs = sweep_once(&state, &allow(), host.durable(), None, 5_000);
        assert!(
            runs.is_empty(),
            "10s after the durable mark, a 1m job must NOT re-fire on restart"
        );
        assert_eq!(
            state.read().unwrap().jobs["nightly"].last_run,
            Some(4_990),
            "the durable mark is hydrated into the shared state (visible on /server/jobs)"
        );

        // One interval later the job is due again.
        let runs = sweep_once(&state, &allow(), host.durable(), None, 5_051);
        assert_eq!(runs.len(), 1, "due again one interval after the mark");
    }

    /// A denied firing records a visible denied run and stamps NOTHING (the ruled semantics: a
    /// policy-less/denied job re-fires — visibly — until its policy is fixed).
    #[test]
    fn sweep_once_denied_records_a_run_but_stamps_nothing() {
        let (_dir, host) = tempdir_host();
        let state = state_with(vec![job("guarded", "1m", None)]);

        let runs = sweep_once(&state, &deny(), host.durable(), Some(host.ledger()), 9_000);
        assert_eq!(runs.len(), 1);

        let g = state.read().unwrap();
        assert_eq!(
            g.jobs["guarded"].last_run, None,
            "a denied fire is not stamped"
        );
        let history = &g.job_runs["guarded"];
        assert_eq!(history[0].outcome, "denied");
        assert!(history[0].detail.contains("default-deny"));
        assert_eq!(history[0].affected, 0);
        drop(g);
        assert_eq!(
            read_last_run(host.durable(), "guarded"),
            None,
            "no durable mark for a denied fire"
        );
    }

    /// The `/server/jobs/<name>/runs` read facet projects the recorded history through the
    /// canonical `job_runs_schema` (and an unknown job reads as an empty history, not an error).
    #[test]
    fn job_runs_read_facet_projects_the_recorded_history() {
        let (_dir, host) = tempdir_host();
        let state = state_with(vec![job("nightly", "1m", None)]);
        sweep_once(&state, &allow(), host.durable(), None, 5_000);

        let facet = crate::server_face::ServerReadFacet::new(Arc::clone(&state));
        let scan = |path: &str| {
            let node = qfs_pushdown::ScanNode {
                source: qfs_pushdown::SourceId::new("server"),
                path: path.to_string(),
                pushed: qfs_pushdown::PushedQuery::default(),
                schema: qfs_core::job_runs_schema(),
            };
            let rt = tokio::runtime::Builder::new_current_thread()
                .build()
                .expect("rt");
            rt.block_on(qfs_exec::ReadDriver::scan(&facet, &node))
        };

        let batch = scan("/server/jobs/nightly/runs").expect("scan runs");
        assert_eq!(batch.schema, qfs_core::job_runs_schema());
        assert_eq!(batch.rows.len(), 1);
        assert_eq!(
            batch.rows[0].values[0],
            qfs_core::Value::Timestamp(5_000),
            "scheduled_at"
        );
        assert_eq!(
            batch.rows[0].values[1],
            qfs_core::Value::Text("fired".into())
        );
        assert_eq!(
            batch.rows[0].values[2],
            qfs_core::Value::Null,
            "no detail on a fire"
        );
        assert_eq!(batch.rows[0].values[3], qfs_core::Value::Int(1), "affected");

        let empty = scan("/server/jobs/absent/runs").expect("scan absent");
        assert!(empty.rows.is_empty(), "an unknown job is an empty history");
    }

    /// The live committer's gate chain, hermetically: a job with NO resolvable policy is
    /// default-denied (zero effects), and an irreversible plan is blocked unattended — neither
    /// reaches the live applier.
    #[test]
    fn live_committer_gates_deny_and_block_before_any_apply() {
        let (engine, _reads, _safety) = crate::shell::run_engine_and_reads();
        let policies: PolicyTableHandle = Arc::new(std::sync::RwLock::new(Arc::new(
            qfs_host::PolicyTable::new(),
        )));
        let committer = LiveCronCommitter::new(engine, policies);

        // No policy ⇒ fail-closed default-deny.
        let stmt = qfs_exec::parse("upsert into /local/tmp/never.txt values ('x')").unwrap();
        match committer.commit("job", &stmt, None) {
            Err(FireError::PolicyDenied { .. }) => {}
            other => panic!("expected default-deny, got {other:?}"),
        }
        assert!(
            !std::path::Path::new("/tmp/never.txt").exists(),
            "atomic abort: nothing applied"
        );

        // An irreversible REMOVE is blocked unattended even under an allowing policy.
        let dir = tempfile::TempDir::new().expect("tempdir");
        let victim = dir.path().join("keep.txt");
        std::fs::write(&victim, b"stay").unwrap();
        let mut table = qfs_host::PolicyTable::new();
        table.insert(
            "p".into(),
            qfs_provision::PolicyDef {
                name: "p".into(),
                handler: String::new(),
                // The canonical stored rule string (what the CREATE POLICY desugar writes).
                allow: vec!["ALLOW REMOVE".into()],
            },
        );
        let policies: PolicyTableHandle = Arc::new(std::sync::RwLock::new(Arc::new(table)));
        let (engine, _reads, _safety) = crate::shell::run_engine_and_reads();
        let committer = LiveCronCommitter::new(engine, policies);
        let stmt = qfs_exec::parse(&format!("remove /local{}", victim.display())).unwrap();
        match committer.commit("job", &stmt, Some("p")) {
            Err(FireError::IrreversibleBlocked(_)) => {}
            other => panic!("expected the irreversible block, got {other:?}"),
        }
        assert!(victim.exists(), "fail-closed: the REMOVE never applied");
    }

    /// End to end against the real thing: a due job's plan commits through the LIVE applier (the
    /// local filesystem driver) — the file exists afterwards and the fire is stamped + recorded.
    /// This is the hermetic twin of the owner-attended live round (T9 fires over HTTP serve).
    #[test]
    fn sweep_once_with_the_live_committer_applies_a_real_local_write() {
        let (_state_dir, host) = tempdir_host();
        let dir = tempfile::TempDir::new().expect("tempdir");
        let out = dir.path().join("swept.txt");

        let mut table = qfs_host::PolicyTable::new();
        table.insert(
            "p".into(),
            qfs_provision::PolicyDef {
                name: "p".into(),
                handler: String::new(),
                // The canonical stored rule string (what the CREATE POLICY desugar writes).
                allow: vec!["ALLOW UPSERT".into()],
            },
        );
        let policies: PolicyTableHandle = Arc::new(std::sync::RwLock::new(Arc::new(table)));
        let (engine, _reads, _safety) = crate::shell::run_engine_and_reads();
        let committer = LiveCronCommitter::new(engine, policies);

        let state = state_with(vec![JobDef {
            name: "writer".into(),
            every: "1m".into(),
            // Stored in the REAL install format (canonical serialized PlanSpec), so this drives
            // the reader's real rehydrate path — the round-9 defect made this exact fire fail.
            plan: plan_col(&format!(
                "upsert into /local{} values ('scheduled')",
                out.display()
            )),
            last_run: None,
            policy: Some("p".into()),
        }]);

        let runs = sweep_once(
            &state,
            &committer,
            host.durable(),
            Some(host.ledger()),
            7_000,
        );
        assert_eq!(runs.len(), 1);
        assert!(
            matches!(runs[0].outcome, CronOutcome::Fired { .. }),
            "the live fire committed: {:?}",
            runs[0].outcome
        );
        assert_eq!(
            std::fs::read_to_string(&out).expect("the swept file exists"),
            "scheduled",
            "the real local applier wrote the file"
        );
        assert_eq!(state.read().unwrap().jobs["writer"].last_run, Some(7_000));
    }

    // ---- blueprint §19 axis D: agent cadence rides the sweep -------------------------------

    /// A due agent cadence fires through the SAME sweep and lands a `JobRunRecord`-shaped row on the
    /// agent's OWN run history (`/server/agents/<name>/runs`), carrying the `agent:<name>` firing
    /// principal — and stamps the agent's `last_run` (state + durable).
    #[test]
    fn sweep_once_fires_a_due_agent_and_records_to_agent_history() {
        let (dir, host) = tempdir_host();
        let state = state_with_agents(vec![agent("triage", "1m", None)]);

        let runs = sweep_once(&state, &allow(), host.durable(), Some(host.ledger()), 5_000);
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].principal.as_deref(), Some("agent:triage"));

        let g = state.read().unwrap();
        assert_eq!(
            g.agents["triage"].last_run,
            Some(5_000),
            "the committed agent fire stamps its last_run"
        );
        let history = &g.agent_runs["triage"];
        assert_eq!(history.len(), 1, "landed on the agent's OWN run history");
        assert_eq!(history[0].outcome, "fired");
        assert_eq!(
            history[0].principal, "agent:triage",
            "the run record carries the secret-free firing principal"
        );
        assert!(
            !g.job_runs.contains_key("triage"),
            "an agent fire does NOT pollute the /server/jobs run history"
        );
        drop(g);

        assert_eq!(
            read_agent_last_run(host.durable(), "triage"),
            Some(5_000),
            "the durable agent mark survives (restart re-hydrates from it)"
        );
        let ledger = std::fs::read_to_string(dir.path().join("audit.log")).expect("ledger");
        assert!(
            ledger.contains("cron fire job=triage") && ledger.contains("principal=agent:triage"),
            "the agent firing is on the audit ledger under its principal: {ledger}"
        );
    }

    /// Restart safety for an agent cadence: the durable mark hydrates a config-reset `last_run`, so a
    /// rebooted daemon does NOT re-fire an agent whose interval has not elapsed.
    #[test]
    fn sweep_once_hydrates_agent_last_run_from_the_durable_store() {
        let (_dir, host) = tempdir_host();
        write_agent_last_run(host.durable(), "triage", 4_990);
        let state = state_with_agents(vec![agent("triage", "1m", None)]);

        let runs = sweep_once(&state, &allow(), host.durable(), None, 5_000);
        assert!(
            runs.is_empty(),
            "10s after the durable mark, a 1m agent must NOT re-fire on restart"
        );
        assert_eq!(state.read().unwrap().agents["triage"].last_run, Some(4_990));

        // One interval later the agent is due again.
        let runs = sweep_once(&state, &allow(), host.durable(), None, 5_051);
        assert_eq!(runs.len(), 1, "due again one interval after the mark");
    }

    /// The LIVE committer gates an agent fire UNDER THE AGENT SUBJECT: a function on a path the
    /// agent's policy does NOT grant is denied (a visible denied run on the agent history, zero
    /// effects) — the mission's over-reach case, on a timer.
    #[test]
    fn sweep_once_agent_fire_denied_by_agent_subject_records_denial() {
        let (_dir, host) = tempdir_host();
        let dir = tempfile::TempDir::new().expect("tempdir");
        let out = dir.path().join("blocked.txt");
        std::fs::write(&out, b"safe").unwrap();

        // The policy grants UPSERT ON local only to an OPERATOR (FOR user op), NOT the agent — so an
        // agent-subject gate default-denies it.
        let mut table = qfs_host::PolicyTable::new();
        table.insert(
            "p".into(),
            qfs_provision::PolicyDef {
                name: "p".into(),
                handler: String::new(),
                allow: vec!["ALLOW UPSERT ON local FOR user:op".into()],
            },
        );
        let policies: PolicyTableHandle = Arc::new(std::sync::RwLock::new(Arc::new(table)));
        let (engine, _reads, _safety) = crate::shell::run_engine_and_reads();
        let committer = LiveCronCommitter::new(engine, policies);

        let state = state_with_agents(vec![qfs_provision::AgentDef {
            name: "triage".into(),
            every: "1m".into(),
            last_run: None,
            plan: plan_col(&format!("upsert into /local{} values ('x')", out.display())),
            policy: Some("p".into()),
        }]);

        let runs = sweep_once(
            &state,
            &committer,
            host.durable(),
            Some(host.ledger()),
            6_000,
        );
        assert_eq!(runs.len(), 1);
        assert!(
            matches!(runs[0].outcome, CronOutcome::Denied { .. }),
            "the agent is default-denied under its own subject: {:?}",
            runs[0].outcome
        );
        assert_eq!(
            std::fs::read_to_string(&out).unwrap(),
            "safe",
            "atomic abort: the denied agent fire applied nothing"
        );
        let g = state.read().unwrap();
        assert_eq!(g.agent_runs["triage"][0].outcome, "denied");
        assert_eq!(
            g.agents["triage"].last_run, None,
            "a denied fire is not stamped"
        );
    }

    /// The ruled property (blueprint §19), on a timer through the LIVE committer: an irreversible
    /// REMOVE on an agent cadence is refused fail-closed (RunMode::Server, Ack::Absent) — a blocked
    /// run, nothing applied. An agent can NEVER fire an irreversible plan unattended.
    #[test]
    fn sweep_once_agent_irreversible_is_blocked_fail_closed() {
        let (_dir, host) = tempdir_host();
        let dir = tempfile::TempDir::new().expect("tempdir");
        let victim = dir.path().join("keep.txt");
        std::fs::write(&victim, b"stay").unwrap();

        // The agent's policy even ALLOWS REMOVE — the block is the unattended irreversible guard,
        // not a policy denial.
        let mut table = qfs_host::PolicyTable::new();
        table.insert(
            "p".into(),
            qfs_provision::PolicyDef {
                name: "p".into(),
                handler: String::new(),
                allow: vec!["ALLOW REMOVE FOR agent:triage".into()],
            },
        );
        let policies: PolicyTableHandle = Arc::new(std::sync::RwLock::new(Arc::new(table)));
        let (engine, _reads, _safety) = crate::shell::run_engine_and_reads();
        let committer = LiveCronCommitter::new(engine, policies);

        let state = state_with_agents(vec![qfs_provision::AgentDef {
            name: "triage".into(),
            every: "1m".into(),
            last_run: None,
            plan: plan_col(&format!("remove /local{}", victim.display())),
            policy: Some("p".into()),
        }]);

        let runs = sweep_once(
            &state,
            &committer,
            host.durable(),
            Some(host.ledger()),
            8_000,
        );
        assert_eq!(runs.len(), 1);
        assert!(
            matches!(runs[0].outcome, CronOutcome::Blocked { .. }),
            "an irreversible agent fire is blocked unattended: {:?}",
            runs[0].outcome
        );
        assert!(victim.exists(), "fail-closed: the REMOVE never applied");
        assert_eq!(
            state.read().unwrap().agents["triage"].last_run,
            None,
            "a blocked fire is not stamped"
        );
    }
}
