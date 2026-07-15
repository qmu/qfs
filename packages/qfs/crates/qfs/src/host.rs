//! The `qfs serve` **daemon host** (t36): `TokioHost`, the EC2/Linux [`RuntimeHost`] composed in
//! the terminal binary.
//!
//! This formalizes the EXISTING serve composition behind the [`qfs_host::RuntimeHost`] trait — it
//! does NOT rebuild it. The HTTP listener (`qfs-http`) and the watchtower bus + `/hooks/...` ingest
//! (`qfs-watchtower`) are wired in [`crate::serve`]; the JOB cause's in-server firing was RESTORED
//! (ticket 20260711121535, reversing t65) — the pure sweeper is `qfs_watchtower::cron` and the
//! daemon `tokio::time` interval that drives it is the owner-attended live round, so `schedule_jobs`
//! records the saved-JOB definitions to the ledger for now. The
//! `TokioHost`'s `serve_endpoints`/`schedule_jobs`/`consume_events` are the trait projection of
//! those already-wired causes (they record the cause attachment to the on-disk audit ledger so the
//! daemon's startup is observable). The two NEW daemon primitives the host adds are the fsync'd
//! [`qfs_host::FileDurableStore`] (watcher cursors / `LAST_RUN` that survive a restart) and the
//! on-disk [`qfs_host::AuditLedger`] (the persistent fired-plan record replacing the in-memory
//! drain). Both live under a caller-supplied state dir — never a system path.

use std::path::PathBuf;

use qfs_host::{
    AuditLedger, BindingSet, DurableStore, FileDurableStore, HostError, Mount, NativeStoreHandle,
    RuntimeHost, Timestamp,
};

/// The EC2/Linux daemon host (blueprint §10). Owns the fsync'd durable store + the on-disk audit ledger;
/// its cause-attachment methods formalize the existing `qfs-http`/`qfs-watchtower` serve
/// composition behind the [`RuntimeHost`] trait (it does not rebuild them); the JOB cause is
/// external since t65 (no internal scheduler — `schedule_jobs` only records the definitions).
pub struct TokioHost {
    durable: FileDurableStore,
    ledger: AuditLedger,
}

impl TokioHost {
    /// Build the daemon host rooted at a project-local state dir (the systemd `StateDirectory` in
    /// production, a tempdir in tests — NEVER a system path). Creates the durable store + ledger.
    ///
    /// # Errors
    /// [`HostError`] if the state dir / store / ledger could not be opened.
    pub fn open(state_dir: impl Into<PathBuf>) -> Result<Self, HostError> {
        let state_dir = state_dir.into();
        let durable = FileDurableStore::open(state_dir.join("durable"))?;
        let ledger = AuditLedger::open(&state_dir, "audit.log")?;
        Ok(Self { durable, ledger })
    }

    /// The on-disk audit ledger (the daemon appends every fired plan / config mutation here).
    #[must_use]
    pub fn ledger(&self) -> &AuditLedger {
        &self.ledger
    }
}

impl RuntimeHost for TokioHost {
    fn now(&self) -> Timestamp {
        Timestamp::from_secs(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0),
        )
    }

    async fn serve_endpoints(&self, set: &BindingSet) -> Result<(), HostError> {
        // The qfs-http listener is wired in crate::serve; record the attached endpoint causes.
        self.ledger
            .append(&format!("host attach endpoints={}", set.endpoints.len()))?;
        Ok(())
    }

    async fn schedule_jobs(&self, set: &BindingSet) -> Result<(), HostError> {
        // t65 REVERSED (2026-07-11, ticket 20260711121535): qfs IS a scheduler again — the daemon
        // owns the "when" beside the "what". The pure firing DECISION lives in
        // `qfs_watchtower::cron` (`is_due` / `fire_due` over an injected `now`); the `tokio::time`
        // interval that drives it on a real clock is `crate::sweeper::spawn_sweeper`, wired by
        // `crate::serve` over THIS host (the live `Committer` runs the policy gate + irreversible
        // guard, `last_run` persists through `self.durable()`, each firing lands on the
        // `/server/jobs/<name>/runs` read-back + this ledger). Like `serve_endpoints`, this trait
        // projection records the attached JOB causes so the daemon's startup stays observable.
        // (CF Workers keeps platform Cron Triggers as its "when"; the reversal is daemon-scoped.)
        self.ledger
            .append(&format!("host attach jobs={}", set.jobs.len()))?;
        Ok(())
    }

    async fn consume_events(&self, set: &BindingSet) -> Result<(), HostError> {
        // The qfs-watchtower bus + /hooks ingest is wired in crate::serve; record the causes.
        self.ledger.append(&format!(
            "host attach webhooks={} watchers={}",
            set.webhooks.len(),
            set.watchers.len()
        ))?;
        Ok(())
    }

    fn durable(&self) -> &dyn DurableStore {
        &self.durable
    }

    fn native_store(&self, set: &BindingSet, mount: &Mount) -> Option<NativeStoreHandle> {
        // On the daemon a native store resolves to the driver's existing HTTP client; the handle
        // names the binding (the driver knows how to use it on this platform). None if unbound.
        set.native_stores
            .iter()
            .find(|ns| &ns.mount == mount)
            .map(|ns| NativeStoreHandle {
                mount: ns.mount.clone(),
                binding_name: ns.binding_name(),
            })
    }
}
