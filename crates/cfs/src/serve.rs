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
    let (cron_binding, jobs_handle) = cfs_cron::build_cron_binding();
    let cron_store = crate::cron::LedgerJobStore::new(jobs_handle);
    let cron_committer = crate::cron::PreviewCommitter::new(clone_engine(&engine));

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
        let served = cfs_http::serve_config_with(
            config,
            engine,
            reads,
            addr,
            SERVE_MAX_ROWS,
            vec![cron_binding],
        )
        .await;
        daemon.abort();
        served
    });

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
