//! [`CronBinding`] (t33): the [`cfs_server::Binding`] (kind `Cron`) that reconciles the
//! `/server/jobs` registry into the scheduler's live JOB set.
//!
//! ## Reconcile = converge the JOB set (the t30 rule)
//! `reconcile(&state)` is **synchronous** (the CO-t30-1 contract): handed an owned
//! [`cfs_server::ServerState`] snapshot, it rehydrates each [`cfs_server::JobDef`] into a
//! [`JobBinding`] (parsing the raw `EVERY` interval into a [`Schedule`], rehydrating the canonical
//! `DO` body via [`cfs_core::PlanSpec::from_canonical`] — NO re-parse) and atomically swaps the
//! shared binding set the daemon's tick reads. A malformed JOB row (bad interval / unrehydratable
//! body) is REFUSED at reconcile (skipped + logged with the name only, never the payload), so one
//! bad row never tears down the whole set.
//!
//! ## The store/clock/committer are wired by the binary
//! `CronBinding` owns only the reconciled JOB SET (the registry projection). The durable
//! [`crate::store::JobStore`] (run state / lease / ledger over `/server`), the [`crate::clock`],
//! and the real [`crate::commit::Committer`] (the runtime applier) are composed by the `cfs`
//! binary's serve root and read this shared set — keeping `cfs-cron` off `cfs-runtime`.

use std::sync::{Arc, RwLock};

use cfs_core::PlanSpec;
use cfs_server::{Binding, BindingKind, JobDef, ServerError, ServerState};

use crate::policy::MissedPolicy;
use crate::schedule::Schedule;
use crate::store::{JobBinding, PolicyRef};

/// The shared, atomically-swappable JOB-set handle the daemon's `JobStore` reads (the registry
/// projection the binding reconciles into). An alias so the composition root and the binding agree
/// on one type and clippy's complex-type lint stays quiet.
pub type JobSetHandle = Arc<RwLock<Arc<Vec<JobBinding>>>>;

/// The cron binding. Holds the atomically swappable set of rehydrated, enabled JOB bindings the
/// daemon's `tick` reads. Constructed by the `cfs` binary's serve composition root.
#[derive(Debug, Default)]
pub struct CronBinding {
    /// The live JOB set (the registry projection); swapped atomically on reconcile.
    jobs: JobSetHandle,
}

impl CronBinding {
    /// A cron binding with an empty JOB set (boot reconciles it to the registry).
    #[must_use]
    pub fn new() -> Self {
        Self {
            jobs: Arc::new(RwLock::new(Arc::new(Vec::new()))),
        }
    }

    /// A shared handle to the live JOB set, for the daemon's JobStore `load_enabled`. Reading it
    /// clones the inner `Arc<Vec<_>>` under a momentary guard (never across an `.await`).
    #[must_use]
    pub fn jobs_handle(&self) -> JobSetHandle {
        Arc::clone(&self.jobs)
    }

    /// Snapshot the current live JOB set (clones the `Arc`; the guard is dropped immediately).
    #[must_use]
    pub fn current_jobs(&self) -> Arc<Vec<JobBinding>> {
        self.jobs
            .read()
            .map(|g| Arc::clone(&g))
            .unwrap_or_else(|_| Arc::new(Vec::new()))
    }
}

impl Binding for CronBinding {
    fn kind(&self) -> BindingKind {
        BindingKind::Cron
    }

    fn reconcile(&mut self, state: &ServerState) -> Result<(), ServerError> {
        let mut jobs = Vec::with_capacity(state.jobs.len());
        for def in state.jobs.values() {
            match rehydrate_job(def) {
                Ok(binding) => jobs.push(binding),
                Err(reason) => {
                    // Refused / malformed: do NOT register the JOB. Log the name + class only
                    // (no DO body, no interval payload beyond the class — RFD §10).
                    tracing::warn!(
                        target: "cfs::cron",
                        job = %def.name,
                        reason = %reason,
                        "job not registered (rehydrate refusal)"
                    );
                }
            }
        }
        let count = jobs.len();
        if let Ok(mut guard) = self.jobs.write() {
            *guard = Arc::new(jobs);
        } else {
            return Err(ServerError::Reconcile {
                kind: BindingKind::Cron.label().to_string(),
                reason: "cron job set lock poisoned".to_string(),
            });
        }
        tracing::info!(
            target: "cfs::cron",
            jobs = count,
            registered = state.jobs.len(),
            "cron job set reconciled"
        );
        Ok(())
    }
}

/// Build a [`CronBinding`] as a boxed [`cfs_server::Binding`] for the runtime to reconcile, plus
/// its shared JOB-set handle for the daemon's `JobStore`. A composition-root convenience so the
/// `cfs` binary never names `cfs_server` directly (keeping its dep-allowlist unchanged).
#[must_use]
pub fn build_cron_binding() -> (Box<dyn Binding>, JobSetHandle) {
    let binding = CronBinding::new();
    let handle = binding.jobs_handle();
    (Box::new(binding), handle)
}

/// Rehydrate one [`JobDef`] into a [`JobBinding`]: parse the `EVERY` interval into a [`Schedule`],
/// rehydrate the canonical `DO` body (no re-parse), default the missed policy to `Coalesce`. A
/// `last_run`-bearing row carries no schedule semantics here (the durable store owns run state).
fn rehydrate_job(def: &JobDef) -> Result<JobBinding, String> {
    let schedule = parse_interval(&def.every)?;
    let plan = PlanSpec::from_canonical(def.plan.as_str())
        .map_err(|e| format!("DO body not rehydratable: {e}"))?;
    Ok(JobBinding {
        name: def.name.clone(),
        schedule,
        plan,
        // POLICY threading is a sibling-ticket concern; the row carries no policy handle yet, so
        // the binding commits under the default (no widening). When the JOB row gains a policy
        // handle, map it here.
        policy: PolicyRef::default(),
        missed: MissedPolicy::default(),
        enabled: true,
    })
}

/// Parse the raw `EVERY <interval>` text into a [`Schedule`]. Supports a small set of duration
/// suffixes (`s`/`m`/`h`/`d`) and a bare-seconds integer; an unsuffixed crontab-looking 5-field
/// string is parsed as a cron expression. A malformed interval is a structured error string the
/// reconcile logs (name only).
fn parse_interval(raw: &str) -> Result<Schedule, String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err("empty EVERY interval".to_string());
    }
    // A 5-field whitespace-separated string is a cron expression.
    if raw.split_whitespace().count() == 5 {
        return Schedule::cron(raw).map_err(|e| e.to_string());
    }
    // Otherwise a duration: <number><unit?> (unit defaults to seconds).
    let (num_part, mult) = split_unit(raw);
    let n: i64 = num_part
        .parse()
        .map_err(|_| format!("bad EVERY interval: {raw:?}"))?;
    Schedule::every(n * mult).map_err(|e| e.to_string())
}

/// Split a duration string into its numeric part and a seconds multiplier (default 1).
fn split_unit(raw: &str) -> (&str, i64) {
    let bytes = raw.as_bytes();
    match bytes.last() {
        Some(b's' | b'S') => (&raw[..raw.len() - 1], 1),
        Some(b'm' | b'M') => (&raw[..raw.len() - 1], 60),
        Some(b'h' | b'H') => (&raw[..raw.len() - 1], 3600),
        Some(b'd' | b'D') => (&raw[..raw.len() - 1], 86_400),
        _ => (raw, 1),
    }
}
