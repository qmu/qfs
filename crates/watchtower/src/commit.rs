//! [`Committer`] — the **injected commit seam** (RFD §3/§6 purity invariant), and [`PolicyGate`]
//! — the policy hook every fired plan passes through (the engine is t35; this calls the hook).
//!
//! The dispatcher constructs no effects and runs nothing itself: it builds the `NEW.*`-bound
//! handler `Statement`, asks the [`PolicyGate`] whether the fire is permitted, then hands the
//! statement to a [`Committer`]. The REAL committer (the runtime interpreter with live drivers) is
//! provided by the composition root (the `cfs` binary) — this keeps `cfs-watchtower` off
//! `cfs-runtime` and the dispatch logic pure (the exact cfs-cron pattern).

use cfs_parser::Statement;

/// The outcome of committing a fired handler plan: a secret-free summary. NO plan payload, NO
/// secrets (RFD §10) — only what the audit ledger may keep.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FireOutcome {
    /// A stable, secret-free fingerprint of the committed plan (counts/hashes only).
    pub plan_summary: String,
    /// How many effects the commit applied (the safe-to-log count).
    pub affected: u64,
}

/// A structured, secret-free fire error.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum FireError {
    /// The handler body could not be lowered to a plan (resolve / capability / plan construction).
    #[error("plan build failed: {0}")]
    Build(String),
    /// The policy gate refused the fire (a future POLICY engine, t35, denies the capability).
    #[error("policy denied: {0}")]
    PolicyDenied(String),
    /// A leg of the plan failed to apply at commit (the reason is already secret-free).
    #[error("commit failed: {0}")]
    Apply(String),
}

/// The policy gate hook (RFD §10). Every fired plan passes through it BEFORE commit so an
/// unconstrained handler cannot run once the POLICY engine (t35) lands. t34 ships the seam (the
/// dispatcher calls it) but NOT the engine: the default [`AllowAllGate`] permits everything, and a
/// future engine replaces it without touching dispatch.
pub trait PolicyGate: Send + Sync {
    /// Decide whether firing `stmt` under the named `trigger` is permitted. `Ok(())` permits;
    /// `Err(reason)` denies (the dispatcher records zero effect + a denial, never the plan).
    ///
    /// # Errors
    /// A secret-free denial reason if the fire is not permitted.
    fn check(&self, trigger: &str, stmt: &Statement) -> Result<(), String>;
}

/// The t34 default gate: permits every fire (the POLICY engine is t35). Pure, no state.
#[derive(Debug, Default, Clone, Copy)]
pub struct AllowAllGate;

impl PolicyGate for AllowAllGate {
    fn check(&self, _trigger: &str, _stmt: &Statement) -> Result<(), String> {
        Ok(())
    }
}

/// The injected commit seam. The dispatcher calls `commit` with the **already-`NEW.*`-bound**
/// handler statement; the implementor builds the plan and runs the real (or recording) applier,
/// never widening scope.
pub trait Committer: Send + Sync {
    /// Commit the bound handler `stmt`. Returns a secret-free [`FireOutcome`].
    ///
    /// # Errors
    /// [`FireError`] on a build or apply failure (a failed commit is NOT acked, so the event is
    /// redelivered — at-least-once).
    fn commit(&self, stmt: &Statement) -> Result<FireOutcome, FireError>;
}

/// A no-creds, no-network test committer (the PREVIEW path), gated behind `native` because it
/// consumes `cfs-exec` (the evaluator pulls tokio, no-wasm). Builds the plan via
/// `cfs_exec::build_plan` over an injected engine and commits over a `RecordingApplier`. Counts
/// the effects applied across ALL commits (so the idempotency golden can assert "one net effect
/// across two deliveries"). The pure wasm core ships only the [`Committer`] trait; a wasm consumer
/// provides its own committer.
#[cfg(feature = "native")]
pub struct RecordingCommitter {
    engine: cfs_core::Engine,
    /// When set, every commit fails with this reason (the forced-failure / retry path).
    fail_reason: Option<String>,
    /// The cumulative count of effects applied across all commits (for the idempotency golden).
    applied: std::sync::atomic::AtomicU64,
}

#[cfg(feature = "native")]
impl RecordingCommitter {
    /// A recording committer over an empty engine (sufficient for a `VALUES`-bodied effect).
    #[must_use]
    pub fn new() -> Self {
        Self {
            engine: cfs_core::Engine::new(),
            fail_reason: None,
            applied: std::sync::atomic::AtomicU64::new(0),
        }
    }

    /// A recording committer over a pre-built engine (with the test's fake mounts registered).
    #[must_use]
    pub fn with_engine(engine: cfs_core::Engine) -> Self {
        Self {
            engine,
            fail_reason: None,
            applied: std::sync::atomic::AtomicU64::new(0),
        }
    }

    /// A recording committer that fails every commit (the retry / dead-letter tests).
    #[must_use]
    pub fn failing(reason: impl Into<String>) -> Self {
        Self {
            engine: cfs_core::Engine::new(),
            fail_reason: Some(reason.into()),
            applied: std::sync::atomic::AtomicU64::new(0),
        }
    }

    /// The cumulative effects applied across all commits so far (test/observability aid).
    #[must_use]
    pub fn total_applied(&self) -> u64 {
        self.applied.load(std::sync::atomic::Ordering::SeqCst)
    }
}

#[cfg(feature = "native")]
impl Default for RecordingCommitter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(feature = "native")]
impl Committer for RecordingCommitter {
    fn commit(&self, stmt: &Statement) -> Result<FireOutcome, FireError> {
        if let Some(reason) = &self.fail_reason {
            return Err(FireError::Apply(reason.clone()));
        }
        let plan = cfs_exec::build_plan(stmt, &self.engine)
            .map_err(|e| FireError::Build(e.to_string()))?;
        let preview = cfs_core::preview(&plan);
        let plan_summary = cfs_crypto_core::sha256_hex(format!("{preview:?}").as_bytes());
        // Commit over a recording applier (no creds, no network — the PREVIEW path).
        let mut applier = cfs_core::RecordingApplier::new();
        let report = cfs_core::commit(&plan, &mut applier, |_| {});
        if let Some(err) = report.failed {
            return Err(FireError::Apply(err.to_string()));
        }
        let affected = report.applied.len() as u64;
        self.applied
            .fetch_add(affected, std::sync::atomic::Ordering::SeqCst);
        Ok(FireOutcome {
            plan_summary,
            affected,
        })
    }
}
