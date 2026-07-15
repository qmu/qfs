//! **In-server CREATE JOB firing** (ticket 20260711121535) — the cron fire-path leaf that
//! deliberately reverses the t65 "qfs is not a scheduler" decision.
//!
//! The substrate already existed: `CREATE JOB … EVERY … DO …` desugars to `/server/jobs` rows
//! (`qfs_server::JobDef`), `BindingKind::Cron` is a reserved reconcile label, and the watchtower's
//! injected [`Committer`](crate::Committer) already does the gate → policy → commit chain. The
//! missing piece is *when*: this module decides which jobs are due at a given instant and fires each
//! through the same committer, producing a secret-free run record per firing.
//!
//! **Purity / clock discipline.** The decision — [`interval_secs`] / [`is_due`] — is pure over
//! primitives (an `every` string, an `Option<last_run>`, a `now`), so it is wasm-clean and driven by
//! an INJECTED time in tests; no wall-clock lives here. The daemon leaf (a `tokio::time` interval
//! task in the binary) is the only place `SystemTime::now()` feeds this — the same confinement the
//! watchtower bus already uses.
//!
//! **Ruled semantics** (each covered by a hermetic test):
//! - *cadence*: the restricted `<n><unit>` grammar (`m`/`h`/`d`) the config already accepts; an
//!   unparseable cadence never fires (fail-closed, not an every-tick storm).
//! - *missed-fire*: **skip, not catch-up** — a job whose interval elapsed many times while the daemon
//!   was down fires **once** on the next tick, then `last_run` advances to `now` (at-most-one catch-up).
//! - *overlap*: **skip-if-running** — a job already in flight is not re-fired.
//! - *timezone*: **UTC only** — all times are epoch seconds; there is no local-time interpretation.
//! - *at-least-once + idempotency*: inherited from the committer + the ledger (a fire that fails to
//!   commit is not stamped, so it re-fires; the committer's dedup makes a repeat a no-op).

/// The cadence of an `EVERY <interval>` clause in seconds, or `None` for an unparseable cadence.
/// The grammar is the restricted `<n><unit>` form the config store already accepts (unit ∈
/// `m`/`h`/`d`); anything else returns `None` so the caller fails closed (never an every-tick storm).
#[must_use]
pub fn interval_secs(every: &str) -> Option<i64> {
    let every = every.trim();
    let unit = every.chars().last()?;
    if !unit.is_ascii_alphabetic() {
        return None;
    }
    let digits = &every[..every.len() - unit.len_utf8()];
    let num: i64 = digits.trim().parse().ok()?;
    if num <= 0 {
        return None;
    }
    let unit_secs = match unit.to_ascii_lowercase() {
        'm' => 60,
        'h' => 3_600,
        'd' => 86_400,
        _ => return None,
    };
    Some(num * unit_secs)
}

/// Whether a job with cadence `every` and high-water mark `last_run` is due to fire at `now` (all
/// epoch seconds, UTC). A never-run job (`last_run == None`) is due immediately; otherwise it is due
/// once `now` has reached `last_run + interval`. Missed intervals collapse to a single fire — the
/// caller advances `last_run` to `now`, so a long outage yields at most one catch-up fire, not a
/// storm. An unparseable cadence is never due.
#[must_use]
pub fn is_due(every: &str, last_run: Option<i64>, now: i64) -> bool {
    let Some(interval) = interval_secs(every) else {
        return false;
    };
    match last_run {
        None => true,
        Some(last) => now >= last.saturating_add(interval),
    }
}

#[cfg(feature = "native")]
pub use native::*;

#[cfg(feature = "native")]
mod native {
    use std::collections::BTreeSet;

    use qfs_server::JobDef;

    use crate::commit::{Committer, FireError};

    /// What happened when a due job fired — a secret-free run record. The daemon sweeper maps each
    /// into a `/server/jobs/<name>/runs` row (the relational read-back) AND an audit-ledger line,
    /// so a denied/failed firing is visible both ways.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum CronOutcome {
        /// The plan committed: `affected` effects applied under the bound policy.
        Fired {
            /// Effects applied by the fired plan.
            affected: u64,
        },
        /// The bound POLICY denied the plan — ZERO effects applied (default-deny for a policy-less job).
        Denied {
            /// Secret-free denial reason.
            reason: String,
        },
        /// The plan carried an irreversible effect and the server fires UNATTENDED — refused, fail-closed.
        Blocked {
            /// Secret-free block reason.
            reason: String,
        },
        /// The plan failed to build or apply (re-fires next tick — not stamped).
        Failed {
            /// Secret-free failure reason.
            reason: String,
        },
    }

    impl CronOutcome {
        /// Whether the firing succeeded — the caller stamps `last_run` only on success (a build/apply
        /// failure is NOT stamped, so at-least-once re-fires it).
        #[must_use]
        pub fn committed(&self) -> bool {
            matches!(self, CronOutcome::Fired { .. })
        }
    }

    /// One firing's secret-free run record: which job, the scheduled instant (UTC epoch seconds), and
    /// the outcome. `stamp_last_run` carries the new high-water mark the caller persists on success.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct CronRun {
        /// The job name.
        pub job: String,
        /// The instant the sweep fired at (UTC epoch seconds) — the plan's `NOW()` anchor.
        pub scheduled_at: i64,
        /// The firing outcome.
        pub outcome: CronOutcome,
        /// The `last_run` to persist (`Some(now)` on a committed fire; `None` leaves it unchanged so a
        /// failed fire re-fires next tick).
        pub stamp_last_run: Option<i64>,
    }

    /// Fire every job that is due at `now` and is NOT already in flight (`overlap = skip-if-running`),
    /// through the injected [`Committer`] (which runs the policy gate + irreversible guard + commit).
    /// Returns one [`CronRun`] per FIRED job (an in-flight or not-yet-due job produces no record). The
    /// caller persists each run's `stamp_last_run` and appends the record to the run history.
    ///
    /// This constructs no effects itself and holds no clock: `now` is injected, so a test drives the
    /// exact firing instant deterministically.
    #[must_use]
    pub fn fire_due(
        jobs: &[JobDef],
        now: i64,
        in_flight: &BTreeSet<String>,
        committer: &dyn Committer,
    ) -> Vec<CronRun> {
        let mut runs = Vec::new();
        for job in jobs {
            if in_flight.contains(&job.name) {
                continue; // overlap: skip-if-running
            }
            if !super::is_due(&job.every, job.last_run, now) {
                continue;
            }
            runs.push(fire_one(job, now, committer));
        }
        runs
    }

    /// Fire a single due job through the committer and map the result to a run record. The job's
    /// stored `DO` body is the canonical serialized `PlanSpec` (`ServerState` stays serializable);
    /// it is rehydrated here via serde with no re-parse, matching the writer. An un-rehydratable
    /// stored body is a build failure (re-fires, not silently dropped).
    fn fire_one(job: &JobDef, now: i64, committer: &dyn Committer) -> CronRun {
        // The stored `DO` body is the canonical serialized `PlanSpec` — the SAME format the
        // `/server/jobs` config row holds (`PlanSpec::canonical()` at install) and the trigger
        // dispatcher + `qfs job run` already read back. Rehydrate it via serde with NO re-parse
        // (the plan was parsed + validated at install). Reading it as statement text was the
        // round-9 defect: the writer stores AST-JSON, so the lexer saw `{` and failed every sweep.
        let spec = match qfs_core::PlanSpec::from_canonical(job.plan.0.as_str()) {
            Ok(s) => s,
            Err(e) => {
                return CronRun {
                    job: job.name.clone(),
                    scheduled_at: now,
                    outcome: CronOutcome::Failed {
                        reason: format!("stored JOB body did not rehydrate: {e}"),
                    },
                    stamp_last_run: None,
                };
            }
        };
        let outcome = match committer.commit(&job.name, spec.statement(), job.policy.as_deref()) {
            Ok(o) => CronOutcome::Fired {
                affected: o.affected,
            },
            Err(FireError::PolicyDenied { reason, .. }) => CronOutcome::Denied { reason },
            Err(FireError::IrreversibleBlocked(reason)) => CronOutcome::Blocked { reason },
            Err(FireError::Build(reason)) => CronOutcome::Failed { reason },
            Err(FireError::Apply(reason)) => CronOutcome::Failed { reason },
        };
        let stamp_last_run = outcome.committed().then_some(now);
        CronRun {
            job: job.name.clone(),
            scheduled_at: now,
            outcome,
            stamp_last_run,
        }
    }
}

#[cfg(all(test, feature = "native"))]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use std::collections::BTreeSet;

    use qfs_parser::Statement;
    use qfs_server::{JobDef, StatementSource};

    use super::*;
    use crate::commit::{Committer, FireError, FireOutcome};

    /// A fully-controllable fire seam: the committer's verdict is fixed by construction, so a cron
    /// test drives every branch (fired / policy-denied / irreversible-blocked) without a live engine.
    struct MockCommitter {
        verdict: fn() -> Result<FireOutcome, FireError>,
    }
    impl Committer for MockCommitter {
        fn commit(
            &self,
            _trigger: &str,
            _stmt: &Statement,
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
                    effects: vec!["INSERT log:/log".into()],
                })
            },
        }
    }
    fn deny() -> MockCommitter {
        MockCommitter {
            verdict: || {
                Err(FireError::PolicyDenied {
                    reason: "default-deny (no matching rule)".into(),
                    verb: "INSERT".into(),
                    driver: "log".into(),
                    rule: None,
                    effects: vec!["INSERT log:/log".into()],
                })
            },
        }
    }
    fn block() -> MockCommitter {
        MockCommitter {
            verdict: || Err(FireError::IrreversibleBlocked("unattended REMOVE".into())),
        }
    }

    fn job(name: &str, every: &str, last_run: Option<i64>) -> JobDef {
        // The `plan` column holds the canonical serialized `PlanSpec` (AST-JSON), exactly as the
        // install path writes it — NOT statement text. Building it through `PlanSpec::canonical()`
        // keeps the test on the real writer/reader format so the round-9 drift can't pass again.
        let canonical = qfs_core::PlanSpec::from_statement(
            qfs_exec::parse("insert into /log/x values ('hi')").unwrap(),
        )
        .canonical();
        JobDef {
            name: name.into(),
            every: every.into(),
            plan: StatementSource(canonical),
            last_run,
            policy: Some("p".into()),
        }
    }

    #[test]
    fn interval_secs_parses_the_restricted_grammar() {
        assert_eq!(interval_secs("5m"), Some(300));
        assert_eq!(interval_secs("1h"), Some(3_600));
        assert_eq!(interval_secs("2h"), Some(7_200));
        assert_eq!(interval_secs("1d"), Some(86_400));
        // Unparseable / degenerate cadences never fire (fail-closed, not an every-tick storm).
        assert_eq!(interval_secs("0m"), None);
        assert_eq!(interval_secs("* * * * *"), None);
        assert_eq!(interval_secs("banana"), None);
        assert_eq!(interval_secs("5"), None);
    }

    #[test]
    fn is_due_fires_immediately_when_never_run_and_respects_the_interval() {
        assert!(is_due("1h", None, 1_000), "a never-run job is due at once");
        // last_run=1000, interval=3600 → due only at/after 4600.
        assert!(
            !is_due("1h", Some(1_000), 4_599),
            "not yet due one second early"
        );
        assert!(
            is_due("1h", Some(1_000), 4_600),
            "due exactly at the boundary"
        );
        assert!(is_due("1h", Some(1_000), 9_999), "still due later");
        // An unparseable cadence is never due.
        assert!(!is_due("nonsense", None, 10_000));
    }

    #[test]
    fn fire_due_fires_a_due_job_and_stamps_last_run_to_now() {
        let jobs = vec![job("nightly", "1h", Some(1_000))];
        let runs = fire_due(&jobs, 5_000, &BTreeSet::new(), &allow());
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].job, "nightly");
        assert_eq!(runs[0].scheduled_at, 5_000);
        assert_eq!(runs[0].outcome, CronOutcome::Fired { affected: 1 });
        assert_eq!(
            runs[0].stamp_last_run,
            Some(5_000),
            "a committed fire stamps last_run to the fire instant"
        );
    }

    #[test]
    fn fire_due_skips_a_not_yet_due_job() {
        let jobs = vec![job("hourly", "1h", Some(4_000))];
        // now=5000 < 4000+3600 → not due.
        let runs = fire_due(&jobs, 5_000, &BTreeSet::new(), &allow());
        assert!(runs.is_empty(), "a not-yet-due job does not fire");
    }

    #[test]
    fn fire_due_missed_fire_fires_once_not_catch_up() {
        // last_run is DAYS in the past for an hourly job; a naive catch-up would fire dozens of times.
        let jobs = vec![job("hourly", "1h", Some(0))];
        let runs = fire_due(&jobs, 1_000_000, &BTreeSet::new(), &allow());
        assert_eq!(
            runs.len(),
            1,
            "a long outage collapses to a single catch-up fire"
        );
        assert_eq!(
            runs[0].stamp_last_run,
            Some(1_000_000),
            "last_run jumps to now, so the next fire is one interval away (no storm)"
        );
    }

    #[test]
    fn fire_due_skips_an_in_flight_job_overlap() {
        let jobs = vec![job("slow", "1m", Some(0))];
        let mut in_flight = BTreeSet::new();
        in_flight.insert("slow".to_string());
        let runs = fire_due(&jobs, 10_000, &in_flight, &allow());
        assert!(
            runs.is_empty(),
            "overlap policy: an in-flight job is not re-fired"
        );
    }

    #[test]
    fn fire_due_policy_denied_records_a_denied_run_with_no_stamp() {
        let jobs = vec![job("guarded", "1m", None)];
        let runs = fire_due(&jobs, 10_000, &BTreeSet::new(), &deny());
        assert_eq!(runs.len(), 1);
        match &runs[0].outcome {
            CronOutcome::Denied { reason } => assert!(reason.contains("default-deny")),
            other => panic!("expected a denied run, got {other:?}"),
        }
        assert_eq!(
            runs[0].stamp_last_run, None,
            "a denied firing is NOT stamped — a visible denied run, zero effects"
        );
    }

    #[test]
    fn fire_due_irreversible_blocked_records_a_blocked_run() {
        let jobs = vec![job("destructive", "1m", None)];
        let runs = fire_due(&jobs, 10_000, &BTreeSet::new(), &block());
        assert_eq!(runs.len(), 1);
        assert!(matches!(runs[0].outcome, CronOutcome::Blocked { .. }));
        assert_eq!(
            runs[0].stamp_last_run, None,
            "a blocked firing is not stamped"
        );
    }
}
