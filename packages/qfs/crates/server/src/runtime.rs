//! The server **runtime** (blueprint §7/§10): `qfs serve <config.qfs>`.
//!
//! [`Runtime::boot`] reads a `.qfs` config file, parses it to statements, lowers each to a
//! `/server` write [`Plan`], and **`COMMIT`s each through the same applier seam a live write
//! uses** — there is no privileged config loader. [`Runtime::run`] blocks on `ctrl_c` and
//! drains the audit ledger on exit.
//!
//! ## Boot is replay (the hard part, blueprint §10)
//! Booting a config file is replaying `INSERT INTO /server/...` statements. The frozen
//! `CREATE …` DDL is sugar over those writes ([`crate::lower`]). The **only** way
//! [`ServerState`] changes is an [`EffectKind::ServerConfigWrite`](qfs_core::EffectKind::ServerConfigWrite)
//! applied at `COMMIT` by [`ServerConfigApplier`] — so a hot reconfigure (a later
//! `/server` write from a CLI / endpoint / trigger) takes the identical path as boot.
//!
//! ## Real COMMIT-applies-state (t28/t29 carry-over closed for /server)
//! The shell/CLI COMMIT routes through an in-memory `RecordingApplier` (records calls,
//! mutates nothing). For `/server` writes this runtime drives [`ServerConfigApplier`]
//! instead — a real [`PlanApplier`] that takes the `RwLock` write guard and applies the op
//! to [`ServerState`] via [`apply_server_write`](crate::driver::apply_server_write). So boot
//! and hot-reconfigure produce a real `ServerState`, not a recording stub.

use std::path::Path as FsPath;
use std::sync::{Arc, RwLock};

use qfs_core::{commit, AppliedEffect, ApplyError, EffectKind, EffectNode, PlanApplier};
use qfs_parser::parse_statement;

use crate::audit::{AuditEntry, AuditSink};
use crate::binding::Binding;
use crate::driver::{apply_server_write, ConfigChange};
use crate::error::ServerError;
use crate::lower::lower_statement;
use crate::state::ServerState;

/// The real `/server` COMMIT applier (blueprint §7): a [`PlanApplier`] that takes the shared
/// [`ServerState`] write guard and applies each [`EffectKind::ServerConfigWrite`] to it —
/// the **only** way `ServerState` changes. Collects the per-apply [`ConfigChange`]s so the
/// runtime can write the audit ledger and reconcile bindings after the commit. Holds the
/// `RwLock` only for the brief apply (a short critical section, never across an `.await`).
pub struct ServerConfigApplier<'s> {
    state: &'s Arc<RwLock<ServerState>>,
    /// The changes applied this commit, in apply order (drained by the caller).
    changes: Vec<ConfigChange>,
}

impl<'s> ServerConfigApplier<'s> {
    /// Construct an applier over the shared state handle.
    #[must_use]
    pub fn new(state: &'s Arc<RwLock<ServerState>>) -> Self {
        Self {
            state,
            changes: Vec::new(),
        }
    }

    /// The changes this applier recorded (consumes the applier).
    #[must_use]
    pub fn into_changes(self) -> Vec<ConfigChange> {
        self.changes
    }
}

impl PlanApplier for ServerConfigApplier<'_> {
    fn apply(&mut self, node: &EffectNode) -> Result<AppliedEffect, ApplyError> {
        match &node.kind {
            EffectKind::ServerConfigWrite { node: snode, op } => {
                // The single ACID mutation: take the write guard for the brief apply.
                let mut guard = self
                    .state
                    .write()
                    .map_err(|_| ApplyError::new(node.id, "server state lock poisoned"))?;
                let change = apply_server_write(&mut guard, *snode, *op, &node.args)
                    .map_err(|reason| ApplyError::new(node.id, reason))?;
                drop(guard);
                self.changes.push(change);
                Ok(AppliedEffect::new(node.id, 1))
            }
            // The /server applier only services /server writes; a foreign node is a wiring
            // bug (the runtime routes only ServerConfigWrite plans here).
            other => Err(ApplyError::new(
                node.id,
                format!(
                    "server applier received non-/server effect {}",
                    other.label()
                ),
            )),
        }
    }
}

/// The shared **hot-reconfigure handle** (blueprint §16, "The face, named" — the write leg): the
/// seam the daemon's network commit path drives after it applies `ServerConfigWrite` effects into
/// the live [`ServerState`] lock. [`ReconfigureHandle::notify`] hands the applied
/// [`ConfigChange`]s to the running [`Runtime`], which records the audit entries and runs the
/// same `reconcile_all()` a boot / `apply_source` mutation triggers — so a network `/server`
/// commit converges the live causes exactly like an operator one (§10: no privileged path, and
/// no unreconciled one either).
#[derive(Clone)]
pub struct ReconfigureHandle {
    state: Arc<RwLock<ServerState>>,
    tx: tokio::sync::mpsc::UnboundedSender<Vec<ConfigChange>>,
}

impl ReconfigureHandle {
    /// The shared live [`ServerState`] lock (what a `ServerConfigApplier` mutates at COMMIT).
    #[must_use]
    pub fn state(&self) -> &Arc<RwLock<ServerState>> {
        &self.state
    }

    /// Hand the applied changes to the running [`Runtime`] (audit + `reconcile_all()`).
    /// Best-effort: if the runtime already shut down, the notification is dropped (the state
    /// mutation itself has committed; there are no live causes left to converge).
    pub fn notify(&self, changes: Vec<ConfigChange>) {
        if self.tx.send(changes).is_err() {
            tracing::debug!(target: "qfs::server", "reconfigure notify after runtime shutdown (dropped)");
        }
    }
}

/// The receiving end of [`reconfigure_channel`] — passed into [`Runtime::with_shared`] so the
/// supervised run loop can service network-commit notifications.
pub struct ReconfigureRx(tokio::sync::mpsc::UnboundedReceiver<Vec<ConfigChange>>);

/// Build the shared-state + reconfigure pair for a daemon composition: the caller creates the
/// live [`ServerState`] lock FIRST (so the `/server` read facet and the statement-bridge commit
/// path can share it), then hands the same state + the receiver to [`Runtime::with_shared`].
#[must_use]
pub fn reconfigure_channel(state: Arc<RwLock<ServerState>>) -> (ReconfigureHandle, ReconfigureRx) {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    (ReconfigureHandle { state, tx }, ReconfigureRx(rx))
}

/// The long-lived server runtime (blueprint §10). Owns the shared [`ServerState`] (source of
/// truth), the registered [`Binding`]s (the cause seam E7 fills), and the [`AuditSink`].
pub struct Runtime {
    state: Arc<RwLock<ServerState>>,
    bindings: Vec<Box<dyn Binding>>,
    audit: Arc<AuditSink>,
    /// The network-commit reconfigure notifications (blueprint §16 write leg); `None` for a
    /// runtime with no statement-bridge composition (tests, plain boot).
    reconfigure: Option<ReconfigureRx>,
    /// The **pre-armed** shutdown listener ([`ShutdownSignal`]), installed by the composition
    /// root BEFORE boot so a signal delivered while the daemon is still booting is latched
    /// rather than met at its default disposition. `None` means "arm it at [`Runtime::run`]",
    /// the pre-boot-arming behaviour every non-daemon caller keeps.
    shutdown: Option<ShutdownSignal>,
}

impl Runtime {
    /// A fresh runtime with an empty [`ServerState`] and no bindings. Boot mutates the
    /// state through the COMMIT path; `with_binding` registers cause seams.
    #[must_use]
    pub fn new() -> Self {
        Self {
            state: Arc::new(RwLock::new(ServerState::new())),
            bindings: Vec::new(),
            audit: Arc::new(AuditSink::new()),
            reconfigure: None,
            shutdown: None,
        }
    }

    /// A runtime over a caller-owned shared [`ServerState`] lock + the reconfigure receiver
    /// (blueprint §16 write leg): the daemon composition creates the state first (shared with the
    /// `/server` read facet and the statement-bridge commit path), and the run loop services
    /// [`ReconfigureHandle::notify`] messages — audit + `reconcile_all()` — alongside the
    /// supervised shutdown wait.
    #[must_use]
    pub fn with_shared(state: Arc<RwLock<ServerState>>, reconfigure: ReconfigureRx) -> Self {
        Self {
            state,
            bindings: Vec::new(),
            audit: Arc::new(AuditSink::new()),
            reconfigure: Some(reconfigure),
            shutdown: None,
        }
    }

    /// Adopt a [`ShutdownSignal`] the composition root armed **before boot** (builder form).
    ///
    /// The daemon entrypoint installs the listener as its first act — see
    /// [`ShutdownSignal::install`] — and hands it here, so a `SIGINT`/`SIGTERM` arriving while
    /// the config is still replaying is latched by the already-installed handler and consumed
    /// by [`Runtime::run`] the instant boot finishes. Without it `run` arms the listener
    /// itself, which is what every non-daemon caller (tests, `qfs_server::serve`) still does.
    #[must_use]
    pub fn with_shutdown(mut self, shutdown: ShutdownSignal) -> Self {
        self.shutdown = Some(shutdown);
        self
    }

    /// Register a [`Binding`] (builder form). Bindings reconcile to the registry after every
    /// committed `/server` mutation.
    #[must_use]
    pub fn with_binding(mut self, binding: Box<dyn Binding>) -> Self {
        self.bindings.push(binding);
        self
    }

    /// The shared state handle (so a binding or test can snapshot it).
    #[must_use]
    pub fn state(&self) -> &Arc<RwLock<ServerState>> {
        &self.state
    }

    /// The audit ledger handle.
    #[must_use]
    pub fn audit(&self) -> &Arc<AuditSink> {
        &self.audit
    }

    /// A read snapshot of the current [`ServerState`] (clones it, so a binding never holds
    /// the read guard across an `.await`).
    #[must_use]
    pub fn snapshot(&self) -> ServerState {
        self.state.read().map(|g| g.clone()).unwrap_or_default()
    }

    /// Refresh one materialized view by executing its stored query through an injected read
    /// function. The executor runs outside the state lock; only after it succeeds do we cache the
    /// row snapshot and stamp `last_run`, so failed refreshes never fabricate freshness.
    ///
    /// # Errors
    /// [`ServerError::ViewRefresh`] when the view is missing, is not materialized, the state lock is
    /// poisoned, the query executor fails, or the cache snapshot cannot serialize.
    pub fn refresh_materialized_view<F>(
        &mut self,
        name: &str,
        now_epoch_ms: i64,
        execute_query: F,
    ) -> Result<RefreshReport, ServerError>
    where
        F: FnOnce(&str) -> Result<qfs_core::RowBatch, String>,
    {
        let query = {
            let guard = self.state.read().map_err(|_| ServerError::ViewRefresh {
                name: name.to_string(),
                reason: "server state lock poisoned".to_string(),
            })?;
            let view = guard
                .views
                .get(name)
                .ok_or_else(|| ServerError::ViewRefresh {
                    name: name.to_string(),
                    reason: "no such view".to_string(),
                })?;
            if !view.materialized {
                return Err(ServerError::ViewRefresh {
                    name: name.to_string(),
                    reason: "view is not materialized".to_string(),
                });
            }
            view.query.as_str().to_string()
        };

        let batch = execute_query(&query).map_err(|reason| ServerError::ViewRefresh {
            name: name.to_string(),
            reason,
        })?;
        let row_count = batch.rows.len();
        let cache_json = serde_json::to_string(&batch).map_err(|e| ServerError::ViewRefresh {
            name: name.to_string(),
            reason: format!("cannot serialize materialized cache: {e}"),
        })?;

        {
            let mut guard = self.state.write().map_err(|_| ServerError::ViewRefresh {
                name: name.to_string(),
                reason: "server state lock poisoned".to_string(),
            })?;
            let view = guard
                .views
                .get_mut(name)
                .ok_or_else(|| ServerError::ViewRefresh {
                    name: name.to_string(),
                    reason: "no such view".to_string(),
                })?;
            if !view.materialized {
                return Err(ServerError::ViewRefresh {
                    name: name.to_string(),
                    reason: "view is not materialized".to_string(),
                });
            }
            view.cache_json = Some(cache_json);
            view.last_run = Some(now_epoch_ms);
        }

        self.reconcile_all()?;
        Ok(RefreshReport {
            name: name.to_string(),
            last_run: now_epoch_ms,
            rows: row_count,
        })
    }

    /// **Boot** from a `.qfs` config file (blueprint §10). Reads the file, splits it into
    /// statements, parses + lowers each, and `COMMIT`s it through [`ServerConfigApplier`]
    /// (the same path a live write takes). After every committed mutation, every registered
    /// binding reconciles to the new state and an audit entry is recorded. Fails fast with a
    /// line-located error on any rejected statement.
    ///
    /// # Errors
    /// [`ServerError`] (read / parse / lower / unsupported-verb / commit) — line-located.
    pub fn boot(&mut self, cfg: &FsPath) -> Result<(), ServerError> {
        let text = std::fs::read_to_string(cfg).map_err(|e| ServerError::Read {
            path: cfg.display().to_string(),
            source: e,
        })?;
        // One splitter for every `.qfs` surface (qfs-core): the lexer decides what a comment, a
        // string and a path token are, so a `--`/`#`/`;` inside a path or locator is content.
        // A document that does not tokenize applies nothing rather than its prefix.
        let stmts =
            qfs_core::ddl::document::split_document(&text).map_err(|e| ServerError::Parse {
                line: e.line,
                code: e.code,
                message: e.message,
            })?;
        for (line, src) in stmts {
            self.apply_source("boot", line, &src)?;
        }
        // A final reconcile so a binding converges even if the config was empty.
        self.reconcile_all()?;
        let summary = self.snapshot().summary();
        tracing::info!(target: "qfs::server", config = %cfg.display(), %summary, "boot complete");
        Ok(())
    }

    /// Parse → lower → COMMIT one server-config statement (the boot + hot-reconfigure unit).
    /// Records the audit ledger entry and reconciles bindings for each applied change.
    ///
    /// # Errors
    /// [`ServerError`] — line-located — on parse / lower / unsupported-verb / commit failure.
    pub fn apply_source(&mut self, who: &str, line: usize, src: &str) -> Result<(), ServerError> {
        let stmt = parse_statement(src).map_err(|e| ServerError::Parse {
            line,
            code: e.code.as_str().to_string(),
            message: e.message.clone(),
        })?;

        let plan =
            match lower_statement(&stmt).map_err(|detail| ServerError::Lower { line, detail })? {
                Some(plan) => plan,
                None => {
                    return Err(ServerError::NotServerConfig {
                        line,
                        detail: "statement does not write /server".to_string(),
                    })
                }
            };

        // COMMIT through the REAL applier (mutates ServerState), not a recording stub.
        let mut applier = ServerConfigApplier::new(&self.state);
        let report = commit(&plan, &mut applier, |_| {});
        if let Some(err) = report.failed {
            return Err(ServerError::Commit {
                line,
                reason: err.reason,
            });
        }
        let changes = applier.into_changes();

        // Audit + reconcile after every committed mutation (blueprint §7/§10).
        for change in &changes {
            self.audit.record(AuditEntry::from_change(who, change));
            self.reconcile_all()?;
        }
        Ok(())
    }

    /// Reconcile every registered binding to a read snapshot of the current state. The
    /// snapshot is cloned so no binding holds the read guard across an `.await` (blueprint §7
    /// concurrency rule).
    fn reconcile_all(&mut self) -> Result<(), ServerError> {
        let snapshot = self.snapshot();
        for binding in &mut self.bindings {
            binding.reconcile(&snapshot)?;
        }
        Ok(())
    }

    /// **Run**: block until a shutdown signal (`SIGINT`/ctrl_c **or** `SIGTERM`), then drain the
    /// audit ledger and return. The cause bindings (HTTP/cron/webhook) attach in E7; at t30 the run
    /// loop is the supervised wait + graceful drain. Consumes the runtime.
    ///
    /// ## SIGTERM is a first-class graceful-shutdown trigger (t36)
    /// A `qfs serve` daemon is stopped by systemd with `KillSignal=SIGTERM` (see
    /// `deploy/qfs.service`); the operator's interactive ctrl_c is `SIGINT`. EITHER triggers the
    /// SAME graceful path — the supervised wait resolves, in-flight plans stop being accepted, and
    /// the audit ledger drains — so a `systemctl stop` is a clean drain, not an uncaught SIGTERM
    /// (exit 143 with no drain). The SIGTERM listener is unix-only (`tokio::signal::unix`);
    /// non-unix targets fall back to `ctrl_c` alone (`SIGTERM` does not exist there).
    ///
    /// ## …and it holds while the daemon is still BOOTING
    /// The contract above once held only from the moment this method started waiting: the
    /// listener was installed here, so a signal delivered during boot met its DEFAULT
    /// disposition and killed the process un-drained (raw wait status 2 / 15). A composition
    /// root closes that window by arming a [`ShutdownSignal`] before boot and handing it in
    /// through [`Runtime::with_shutdown`]; the already-installed handler LATCHES a mid-boot
    /// signal, and the first thing this method does is consume it — so the graceful path runs
    /// as soon as the runtime is whole, with `drain()` meaning exactly what it always meant.
    /// Shutdown therefore never precedes boot completion: a `systemctl stop` racing a slow boot
    /// waits out the remaining boot rather than losing the ledger. A boot that FAILS still
    /// fails (this method is never reached), so a latched signal can never report a clean
    /// shutdown of a daemon that never started.
    ///
    /// # Errors
    /// [`ServerError`] only if the shutdown wiring fails (rare); a normal signal returns `Ok(())`
    /// after draining.
    pub async fn run(mut self) -> Result<(), ServerError> {
        // The pre-armed listener when the composition root installed one before boot, else armed
        // here (the historical behaviour, and the right one for a caller with nothing to boot).
        let mut shutdown = self.shutdown.take().unwrap_or_else(ShutdownSignal::install);
        tracing::info!(target: "qfs::server", summary = %self.snapshot().summary(), "server running; press ctrl_c (SIGINT) or send SIGTERM to stop");
        // Consume a signal that ALREADY arrived while the daemon was booting: one non-blocking
        // poll of the pre-armed listener (the `biased` select tries it first, then the
        // always-ready arm). `Signal::recv` is cancel-safe, so a poll that finds nothing costs
        // the wait below nothing.
        let arrived_during_boot = {
            let pending = shutdown.wait();
            tokio::pin!(pending);
            tokio::select! {
                biased;
                () = &mut pending => true,
                () = std::future::ready(()) => false,
            }
        };
        if arrived_during_boot {
            tracing::info!(target: "qfs::server", "shutdown signal arrived during boot; draining without entering the run loop");
        } else {
            // Block on the shutdown signal, servicing statement-bridge reconfigure notifications
            // (blueprint §16 write leg) while waiting. A failure to install a handler degrades to
            // an immediate shutdown (drain + return) rather than a panic.
            let shutdown = shutdown.wait();
            tokio::pin!(shutdown);
            loop {
                // The reconfigure arm: a network /server commit's applied changes. `None` receiver
                // (no bridge composition) or a closed channel both degrade to a never-ready arm so
                // the shutdown wait is undisturbed.
                let reconfigured = async {
                    match &mut self.reconfigure {
                        Some(ReconfigureRx(rx)) => rx.recv().await,
                        None => std::future::pending().await,
                    }
                };
                tokio::select! {
                    () = &mut shutdown => break,
                    changes = reconfigured => {
                        match changes {
                            Some(changes) => {
                                if let Err(e) = self.handle_reconfigure(&changes) {
                                    tracing::warn!(target: "qfs::server", error = %e, "bridge reconfigure reconcile failed");
                                }
                            }
                            // Every sender dropped: stop polling this arm.
                            None => self.reconfigure = None,
                        }
                    }
                }
            }
        }
        let drained = self.audit.drain();
        tracing::info!(target: "qfs::server", entries = drained, "shutdown: audit ledger drained");
        Ok(())
    }

    /// Service one statement-bridge reconfigure notification (blueprint §16 write leg): record
    /// the audit entry for each applied change, then `reconcile_all()` so the live causes (HTTP
    /// router, watchtower) converge from the new snapshot — the exact post-commit sequence
    /// [`Runtime::apply_source`] runs for an operator mutation.
    ///
    /// # Errors
    /// [`ServerError`] if a binding fails to reconcile.
    pub fn handle_reconfigure(&mut self, changes: &[ConfigChange]) -> Result<(), ServerError> {
        for change in changes {
            self.audit.record(AuditEntry::from_change("bridge", change));
        }
        self.reconcile_all()
    }
}

/// The secret-free receipt from a successful materialized-view refresh.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefreshReport {
    /// The refreshed `/server/views` row key.
    pub name: String,
    /// The successful refresh timestamp stamped into the row.
    pub last_run: i64,
    /// Number of rows cached from the stored query.
    pub rows: usize,
}

/// The graceful-shutdown listener — `SIGINT` (the operator's ctrl_c) **and**, on unix,
/// `SIGTERM` (systemd's `KillSignal`) — held as a VALUE so the moment it is *installed* is
/// separate from the moment it is *awaited*.
///
/// That separation is the whole point. A handler installed at the start of the supervised wait
/// leaves the entire boot uncovered: a signal arriving there meets its default disposition and
/// terminates the process un-drained (measured 2026-08-18 — every `qfs serve` signalled before
/// the run loop died with raw wait status 2/15 and an empty ledger, which is the flake that
/// reddened `main` on PR #74's merge commit). Installing it FIRST, from
/// [`ShutdownSignal::install`], makes the kernel latch such a signal in the already-registered
/// handler; [`Runtime::run`] then consumes it as its first act.
///
/// A handler that cannot be installed is never fatal: the listener degrades — to the other
/// signal alone, or, if neither installs, to an immediate shutdown — so the daemon drains
/// rather than panicking.
pub struct ShutdownSignal {
    #[cfg(unix)]
    arm: UnixArm,
}

/// Which unix listeners actually installed. Kept as a closed set rather than two `Option`s so
/// the "neither installed" degrade (resolve immediately) cannot be confused with "wait forever".
#[cfg(unix)]
enum UnixArm {
    /// Both listeners live: either signal takes the graceful path.
    Both(tokio::signal::unix::Signal, tokio::signal::unix::Signal),
    /// Only `SIGINT` installed.
    Interrupt(tokio::signal::unix::Signal),
    /// Only `SIGTERM` installed.
    Terminate(tokio::signal::unix::Signal),
    /// Neither installed: degrade to an immediate shutdown (drain + return), never a panic.
    Neither,
}

impl ShutdownSignal {
    /// Install the handlers **now**. Call this as the composition root's first act — before any
    /// boot work — and hand the value to [`Runtime::with_shutdown`]; from this instant a
    /// `SIGINT`/`SIGTERM` is latched by the installed handler instead of killing the process.
    ///
    /// On unix both `SIGINT` and `SIGTERM` are registered eagerly (`tokio::signal::unix`), which
    /// is why `tokio::signal::ctrl_c()` is not used here: it registers only when first awaited,
    /// which is exactly the late arming this type exists to avoid. Non-unix targets have no
    /// `SIGTERM`, so `ctrl_c` remains the only trigger there and is awaited lazily as before.
    ///
    /// Must be called from within a tokio runtime context (the signal driver lives there).
    #[must_use]
    pub fn install() -> Self {
        #[cfg(unix)]
        {
            use tokio::signal::unix::{signal, SignalKind};
            let interrupt = signal(SignalKind::interrupt())
                .inspect_err(|e| {
                    tracing::warn!(target: "qfs::server", error = %e, "SIGINT handler failed");
                })
                .ok();
            let terminate = signal(SignalKind::terminate())
                .inspect_err(|e| {
                    tracing::warn!(target: "qfs::server", error = %e, "SIGTERM handler failed");
                })
                .ok();
            let arm = match (interrupt, terminate) {
                (Some(i), Some(t)) => UnixArm::Both(i, t),
                (Some(i), None) => UnixArm::Interrupt(i),
                (None, Some(t)) => UnixArm::Terminate(t),
                (None, None) => UnixArm::Neither,
            };
            if matches!(arm, UnixArm::Neither) {
                tracing::warn!(target: "qfs::server", "no shutdown handler could be installed; the next signal shuts down immediately");
            } else {
                tracing::info!(target: "qfs::server", "shutdown signal armed (SIGINT/SIGTERM); the graceful path is live from here");
            }
            Self { arm }
        }
        #[cfg(not(unix))]
        {
            tracing::info!(target: "qfs::server", "shutdown signal armed (ctrl_c); the graceful path is live from here");
            Self {}
        }
    }

    /// Resolve when a shutdown signal fires — or immediately, if one already fired since
    /// [`install`](Self::install) or no handler could be installed at all. Cancel-safe: the
    /// listeners live in `self`, so a dropped wait loses nothing.
    async fn wait(&mut self) {
        #[cfg(unix)]
        match &mut self.arm {
            UnixArm::Both(interrupt, terminate) => {
                tokio::select! {
                    _ = interrupt.recv() => tracing::info!(target: "qfs::server", "received SIGINT; shutting down"),
                    _ = terminate.recv() => tracing::info!(target: "qfs::server", "received SIGTERM; shutting down"),
                }
            }
            UnixArm::Interrupt(interrupt) => {
                interrupt.recv().await;
                tracing::info!(target: "qfs::server", "received SIGINT; shutting down");
            }
            UnixArm::Terminate(terminate) => {
                terminate.recv().await;
                tracing::info!(target: "qfs::server", "received SIGTERM; shutting down");
            }
            UnixArm::Neither => {}
        }
        #[cfg(not(unix))]
        {
            // Non-unix (incl. wasm): SIGTERM does not exist; ctrl_c is the only graceful trigger.
            if let Err(e) = tokio::signal::ctrl_c().await {
                tracing::warn!(target: "qfs::server", error = %e, "ctrl_c handler failed; shutting down");
            }
        }
    }
}

impl Default for Runtime {
    fn default() -> Self {
        Self::new()
    }
}
