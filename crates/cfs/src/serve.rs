//! The `cfs serve` composition root (t32): the binary wires the HTTP serving binding.
//!
//! The HTTP binding (`cfs-http`) is a LEAF that consumes `cfs-server` (the registry + reconcile
//! seam) AND the `cfs-exec` read executor — putting its composition HERE (the terminal binary,
//! the HTTP sibling of the t28 shell composition root) keeps `cfs-cmd` off both crates and lets
//! tokio dead-end in the terminal sink. cfs-cmd dispatches `serve` to this launcher via the
//! injected [`cfs_cmd::ServeLauncher`]; this builds the engine + read registry, runs
//! [`cfs_http::serve_config`] on a tokio runtime, and returns the process exit code.

use std::path::Path;
use std::sync::Arc;

use cfs_core::{CodecRegistry, Engine};
use cfs_exec::ReadRegistry;

/// The default bounded in-memory result-row cap for `cfs serve`.
const SERVE_MAX_ROWS: usize = 10_000;

/// Boot + run `cfs serve <config>` with the HTTP binding wired. Returns the process exit code
/// (`0` clean shutdown, `1` on a boot / bind / runtime error). Never panics.
#[must_use]
pub fn run_serve(config: &Path) -> i32 {
    // The serve-side engine: codecs registered (json/csv response encoding) + an empty mount
    // registry the real driver crates register into (E4/E7 wiring). At t32 the read drivers a
    // boot config references are registered by the deployment; an unregistered source surfaces
    // as a structured 422 at request time, never a panic.
    let mut engine = Engine::new();
    engine.codecs = CodecRegistry::with_builtins();
    let engine = Arc::new(engine);
    let reads = Arc::new(ReadRegistry::new());

    // The bind address: loopback by default (RFD §10 trusted bind), overridable via env.
    let addr = match resolve_bind_addr() {
        Ok(addr) => addr,
        Err(e) => {
            eprintln!("cfs serve: invalid bind address: {e}");
            return 1;
        }
    };

    // The serve composition is async (listener + supervised ctrl_c wait). Build the runtime at
    // this leaf boundary so tokio dead-ends in the binary.
    let rt = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("cfs serve: cannot start runtime: {e}");
            return 1;
        }
    };

    // t33 cron composition: build the cron binding (reconciled by the runtime from /server/jobs)
    // and spawn the native scheduler daemon over a binary-local JobStore + committer. The daemon
    // reads the binding's live JOB set; its tokio loop dead-ends in this terminal binary.
    let (cron_binding, jobs_handle, cron_policies) = cfs_cron::build_cron_binding();
    let cron_store = crate::cron::LedgerJobStore::new(jobs_handle);
    // t35: the cron committer enforces the JOB's bound POLICY against the built plan before any
    // apply (default-deny / atomic abort) and emits one FiredPlanRecord per fire to this sink.
    let cron_audit = Arc::new(cfs_cron::AuditSink::new());
    let cron_committer = crate::cron::PreviewCommitter::with_policy(
        clone_engine(&engine),
        cron_policies,
        Arc::clone(&cron_audit),
    );

    // t34 watchtower composition: build the watchtower binding + shared bus + the webhook ingest
    // fallback the HTTP listener routes `/hooks/...` to. The binding is reconciled by the runtime
    // from /server/{webhooks,triggers}; the dispatch loop drains the bus and fires matching
    // triggers through the injected committer (PREVIEW path), acking only after a successful
    // commit (at-least-once). The watchtower's own fire-audit sink is drained on shutdown.
    let secrets: Arc<dyn cfs_secrets::Secrets> = Arc::new(cfs_secrets::InMemoryStore::new());
    let (wt_binding, wt_rx, wt_bus, wt_fallback, wt_policies) =
        crate::watchtower::build_watchtower(Arc::clone(&secrets));
    let wt_triggers = wt_binding.triggers_handle();
    let wt_audit = Arc::new(cfs_watchtower::AuditSink::new());

    let result = rt.block_on(async move {
        let scheduler = cfs_cron::Scheduler::new(cron_store, cfs_cron::SystemClock, cron_committer);
        // Spawn the daemon loop; it runs until the process exits (the serve future drives the
        // supervised ctrl_c wait + audit drain). A panic in the daemon never aborts serve.
        let daemon = tokio::spawn(async move {
            cfs_cron::run_daemon(
                scheduler,
                cfs_cron::DaemonConfig::default(),
                std::future::pending::<()>(),
            )
            .await;
        });
        // Spawn the watchtower dispatch loop (drains the bus, fires matching triggers, acks on
        // success). It runs until the bus is dropped (shutdown).
        let dispatch = crate::watchtower::spawn_dispatch_loop(
            wt_rx,
            wt_bus,
            wt_triggers,
            Arc::clone(&wt_audit),
            clone_engine(&engine),
            wt_policies,
        );
        // `cron_binding` is ALREADY a `Box<dyn Binding>` (from build_cron_binding); the watchtower
        // binding is boxed here. Both are `cfs_server::Binding` (cfs-watchtower re-exports the same
        // trait), so they share the one binding vector the runtime reconciles.
        let bindings: Vec<Box<dyn cfs_watchtower::Binding>> =
            vec![Box::new(wt_binding), cron_binding];
        let served = cfs_http::serve_config_full(
            config,
            engine,
            reads,
            addr,
            SERVE_MAX_ROWS,
            bindings,
            Some(wt_fallback),
        )
        .await;
        daemon.abort();
        dispatch.abort();
        served
    });

    // t35: drain the cron fired-plan audit ledger on shutdown (secret-free summaries — driver +
    // path + verb/rule only). The watchtower's own fire-audit sink drains via its dispatch loop.
    let drained = cron_audit.drain();
    if drained > 0 {
        tracing::info!(target: "cfs::serve", fired = drained, "drained cron fired-plan audit ledger");
    }

    match result {
        Ok(()) => 0,
        Err(e) => {
            // The error is already secret-free (boot / bind / runtime); surface it on stderr.
            eprintln!("cfs serve: {e}");
            1
        }
    }
}

/// Clone the serve engine's registries into a fresh `Engine` for the cron committer (so a DO body
/// resolves against the same mounts/codecs the deployment registered). `Engine` is not `Clone`;
/// we rebuild it from the shared registries.
fn clone_engine(engine: &std::sync::Arc<Engine>) -> Engine {
    let mut fresh = Engine::new();
    fresh.mounts = engine.mounts.clone();
    // CodecRegistry is not Clone; rebuild the same builtin set the serve engine uses.
    fresh.codecs = CodecRegistry::with_builtins();
    fresh
}

/// Resolve the HTTP bind address: `CFS_HTTP_ADDR` if set, else the loopback default.
fn resolve_bind_addr() -> Result<std::net::SocketAddr, String> {
    let raw =
        std::env::var("CFS_HTTP_ADDR").unwrap_or_else(|_| cfs_http::DEFAULT_BIND_ADDR.to_string());
    raw.parse().map_err(|e| format!("{raw}: {e}"))
}
