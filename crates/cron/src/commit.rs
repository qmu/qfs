//! [`Committer`] — the **injected commit seam** (RFD §3/§6 purity invariant). The scheduler
//! constructs no effects and runs nothing itself; it hands the rewritten DO `Statement` (with
//! `LAST_RUN()` resolved) to a `Committer` and asks it to commit under the JOB's policy. The REAL
//! committer (the runtime `Interpreter`/applier with live drivers) is provided by the composition
//! root (the `cfs` binary) — this keeps `cfs-cron` off `cfs-runtime` and keeps the scheduler pure.
//!
//! Tests inject a [`RecordingCommitter`]: it builds the `Plan` via `cfs_exec::build_plan` (resolve
//! then plan construction, no I/O) and commits over a `cfs_core::RecordingApplier` — the same
//! no-creds-no-network PREVIEW path the rest of the workspace exercises. The plan fingerprint
//! (`last_plan_hash`) is derived from the plan's deterministic preview projection (counts/hashes
//! only — never the DO payload).

use cfs_parser::Statement;

use crate::store::PolicyRef;

/// The current epoch second (the receipt clock the t35 fired-plan audit record stamps).
#[cfg(feature = "native")]
fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// The outcome of committing a JOB's DO plan: a secret-free fingerprint + the applied count. NO
/// plan payload, NO secrets (RFD §10) — only what the audit ledger may keep.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitOutcome {
    /// The committed plan fingerprint (a stable hash of the plan structure) — `last_plan_hash`.
    pub plan_hash: String,
    /// How many effects the commit applied (the safe-to-log count).
    pub affected: u64,
}

/// A structured, secret-free commit error.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum CommitError {
    /// The DO body could not be lowered to a plan (resolve / capability / plan construction).
    #[error("plan build failed: {0}")]
    Build(String),
    /// The fired plan was DENIED by the JOB's bound POLICY (t35). Atomic abort: ZERO effects
    /// applied (the apply is never reached). The reason is secret-free (verb/driver/rule index).
    #[error("policy denied: {0}")]
    PolicyDenied(String),
    /// A leg of the plan failed to apply at commit (the reason is already secret-free).
    #[error("commit failed: {0}")]
    Apply(String),
}

/// The injected commit seam. The scheduler calls `commit` with the **already-rewritten** DO
/// statement (`LAST_RUN()` resolved to its boundary) and the JOB's policy; the implementor builds
/// the plan and runs the real (or recording) applier under that policy, never widening scope.
pub trait Committer {
    /// Commit the DO `stmt` under `policy`. Returns a secret-free [`CommitOutcome`].
    ///
    /// # Errors
    /// [`CommitError`] on a build or apply failure (a failed run leaves `last_run_at` unmoved).
    fn commit(&self, stmt: &Statement, policy: &PolicyRef) -> Result<CommitOutcome, CommitError>;
}

/// A no-creds, no-network test committer (the PREVIEW path). Builds the plan via
/// `cfs_exec::build_plan` over an injected engine and commits over a `RecordingApplier`;
/// optionally configured to FAIL (to exercise the failed-run / re-cover / circuit-breaker paths).
/// The engine is injected so a test can register a fake mount the DO body resolves against; the
/// default is an empty engine (sufficient for a `VALUES`-bodied effect with no source resolve).
///
/// Gated behind `native` because it consumes `cfs-exec` (the evaluator pulls tokio, no-wasm). The
/// pure wasm core ships only the [`Committer`] trait; a wasm consumer provides its own committer.
#[cfg(feature = "native")]
pub struct RecordingCommitter {
    /// When set, every commit fails with this reason (the forced-failure path).
    fail_reason: Option<String>,
    /// The engine the plan builds against (mounts the DO body resolves to).
    engine: cfs_core::Engine,
    /// The live `/server/policies` table the bound policy ref resolves against (t35). When
    /// `None` (the legacy/test path), enforcement still runs against a default-deny policy when
    /// the JOB names one, but with an empty table the ref dangles ⇒ default-deny (fail-closed).
    policies: Option<std::sync::Arc<std::sync::RwLock<std::sync::Arc<cfs_server::PolicyTable>>>>,
    /// The fired-plan audit sink (t35): exactly one [`cfs_server::FiredPlanRecord`] per
    /// evaluated plan (allow + deny). `None` in the bare test path (no audit assertion).
    audit: Option<std::sync::Arc<cfs_server::AuditSink>>,
}

#[cfg(feature = "native")]
impl Default for RecordingCommitter {
    fn default() -> Self {
        Self {
            fail_reason: None,
            engine: cfs_core::Engine::new(),
            policies: None,
            audit: None,
        }
    }
}

#[cfg(feature = "native")]
impl RecordingCommitter {
    /// A recording committer that always succeeds (empty engine).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// A recording committer over a pre-built engine (with the test's fake mounts registered).
    #[must_use]
    pub fn with_engine(engine: cfs_core::Engine) -> Self {
        Self {
            fail_reason: None,
            engine,
            policies: None,
            audit: None,
        }
    }

    /// A recording committer that fails every commit (forced-failure tests).
    #[must_use]
    pub fn failing(reason: impl Into<String>) -> Self {
        Self {
            fail_reason: Some(reason.into()),
            engine: cfs_core::Engine::new(),
            policies: None,
            audit: None,
        }
    }

    /// Attach the live `/server/policies` table the bound policy ref resolves against (t35).
    #[must_use]
    pub fn with_policies(
        mut self,
        policies: std::sync::Arc<std::sync::RwLock<std::sync::Arc<cfs_server::PolicyTable>>>,
    ) -> Self {
        self.policies = Some(policies);
        self
    }

    /// Attach the fired-plan audit sink (t35): one [`cfs_server::FiredPlanRecord`] per fire.
    #[must_use]
    pub fn with_audit(mut self, audit: std::sync::Arc<cfs_server::AuditSink>) -> Self {
        self.audit = Some(audit);
        self
    }

    /// Snapshot the live policy table (clones the inner `Arc`; the guard is dropped at once).
    fn policy_snapshot(&self) -> cfs_server::PolicyTable {
        match &self.policies {
            Some(h) => h.read().map(|g| (**g).clone()).unwrap_or_default(),
            None => cfs_server::PolicyTable::new(),
        }
    }
}

#[cfg(feature = "native")]
impl Committer for RecordingCommitter {
    fn commit(&self, stmt: &Statement, policy: &PolicyRef) -> Result<CommitOutcome, CommitError> {
        if let Some(reason) = &self.fail_reason {
            return Err(CommitError::Apply(reason.clone()));
        }
        // Build the plan from the rewritten DO body (resolve + plan construction, no I/O).
        let plan = cfs_exec::build_plan(stmt, &self.engine)
            .map_err(|e| CommitError::Build(e.to_string()))?;

        // t35 policy gate (RFD §10): resolve the JOB's bound policy ref against the live
        // `/server/policies` table and run the PURE enforcer over the built plan BEFORE any
        // apply. A handler with no policy / a dangling ref ⇒ fail-closed default-deny. Emit ONE
        // FiredPlanRecord (allow + deny). On deny, RETURN before the apply (atomic abort: ZERO
        // effects). When no policy is attached AND the plan is a pure read / has no write
        // effects, evaluate returns Allow (a read JOB is permitted).
        let table = self.policy_snapshot();
        let bound = if policy.policy.is_empty() {
            None
        } else {
            Some(policy.policy.as_str())
        };
        let resolved = cfs_server::resolve_policy(bound, &table);
        let outcome = cfs_server::gate_plan(&resolved, &plan);
        if let Some(audit) = &self.audit {
            audit.record_fired(outcome.record(
                format!("job-commit policy={}", policy.policy),
                policy.policy.clone(),
                now_secs(),
            ));
        }
        if !outcome.is_allow() {
            return Err(CommitError::PolicyDenied(
                outcome.deny_reason().unwrap_or_default(),
            ));
        }

        // Fingerprint the plan structure deterministically (counts/hashes only — never payload).
        // The preview projection is a stable, secret-free description; hash its debug form.
        let preview = cfs_core::preview(&plan);
        let plan_hash = cfs_crypto_core::sha256_hex(format!("{preview:?}").as_bytes());

        // Commit over a recording applier (no creds, no network — the PREVIEW path).
        let mut applier = cfs_core::RecordingApplier::new();
        let mut affected: u64 = 0;
        let report = cfs_core::commit(&plan, &mut applier, |e| affected += e.affected);
        if let Some(err) = report.failed {
            return Err(CommitError::Apply(err.to_string()));
        }
        Ok(CommitOutcome {
            plan_hash,
            affected,
        })
    }
}
