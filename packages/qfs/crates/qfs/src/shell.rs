//! The interactive-shell command boundary (ticket t28): the REPL driver + the concrete
//! local-FS read facet, hosted in the **`qfs` binary crate**. **All shell LOGIC lives in
//! `qfs_exec::shell`** (resolve / desugar / eval_line / Completer) — this module owns only the
//! glue a real terminal needs: the line reader, the history file, the prompt redraw, and
//! rendering an [`Outcome`] to stdout. Keeping the logic in `qfs-exec` respects the t01 C4 guard
//! (qfs-cmd stays logic-free) and the t29 CO-t29-4 topology (qfs-exec is the integration layer).
//!
//! ## Why the read adapter lives in the BINARY (not qfs-cmd)
//! `ls`/`cat`/`cd`-probe require a real [`qfs_exec::ReadDriver`] for the local mount. The local
//! driver (`qfs-driver-local`) cannot implement that trait itself (the CO-t29-4 guard lets only
//! qfs-cmd depend on qfs-exec), and qfs-exec cannot depend on the driver crate (the same guard
//! confines its deps). qfs-cmd cannot host the adapter either: `qfs-driver-local` is a
//! `qfs-runtime` consumer, so a `qfs-cmd → qfs-driver-local` edge would make qfs-cmd a non-leaf
//! runtime consumer and (correctly) trip the runtime-leaf-confinement guard. The **binary** is
//! the one place that is BOTH an allowlisted runtime consumer AND a leaf (nothing depends on it),
//! so tokio dead-ends here. The adapter [`LocalReadDriver`] — which drives the driver's pure
//! `scan_rows` through qfs-exec's async `ReadDriver` — therefore lives in the binary, which
//! injects the wired shell into `qfs-cmd` via its `ShellLauncher`. This closes part of CO-t29-1
//! for the local driver.
//!
//! ## Line editor footprint decision (recorded)
//! The ticket suggested `rustyline`/`reedline`. Neither is present in the offline cargo cache
//! (`cargo add rustyline --dry-run --offline` → "could not be found in registry index"), and the
//! disk is ~97% full, so adding a heavy editor dep is both impossible offline and against the
//! team's dependency-light precedent (ADR-0002/0003). We therefore ship a **minimal std stdin
//! line-reader** (a `read_line` loop with a best-effort in-memory + on-disk history list). The
//! [`Completer`] API is fully implemented and unit-tested; it is simply not bound to inline
//! TAB editing (which needs raw-mode terminal control a heavy editor would provide). The shell
//! core stays terminal-free and golden-testable regardless.

use std::collections::BTreeMap;
use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::sync::Arc;

use qfs_core::{CfsError, DriverId, Engine, RowBatch};
use qfs_driver_cf::CfDriver;
use qfs_driver_local::{scan_rows, LocalError, LocalFsDriver, Sandbox};
use qfs_exec::shell::{Outcome, Session, VfsPath};
use qfs_exec::{ReadDriver, ReadRegistry};
use qfs_pushdown::ScanNode;

/// The local mount prefix the read facet scans under (mirrors the driver's internal mount).
const LOCAL_MOUNT: &str = "/local";

/// The concrete local-FS read facet: adapts `qfs_driver_local::scan_rows` (pure, synchronous) to
/// qfs-exec's async [`ReadDriver`] seam. Owns the sandbox so the scan stays confined to the
/// mount root. No vendor type crosses the seam — only the owned [`ScanNode`] in and [`RowBatch`]
/// out.
pub struct LocalReadDriver {
    sandbox: Sandbox,
}

impl LocalReadDriver {
    /// Build the read adapter confined to `root`.
    #[must_use]
    pub fn new(sandbox: Sandbox) -> Self {
        Self { sandbox }
    }
}

#[async_trait::async_trait]
impl ReadDriver for LocalReadDriver {
    async fn scan(&self, scan: &ScanNode) -> Result<RowBatch, CfsError> {
        // The ScanNode now carries the full addressed VFS path (t28 pushdown threading), so the
        // scan navigates to the exact node — `ls /local/sub` lists `sub`, not the mount root.
        // An empty path (a synthetic source) falls back to the mount root.
        let vfs = if scan.path.is_empty() {
            LOCAL_MOUNT.to_string()
        } else {
            scan.path.clone()
        };
        let project = scan.pushed.project.as_deref();
        scan_rows(&self.sandbox, &vfs, project).map_err(|e| local_to_qfs(&e))
    }
}

/// Map a local-FS scan failure into the workspace [`CfsError`] the read seam speaks. A
/// sandbox escape is a malformed path at the boundary; the rest reduce to a structured,
/// secret-free invalid-path error (the executor maps these to its own kind/exit code).
fn local_to_qfs(err: &LocalError) -> CfsError {
    match err {
        LocalError::OutsideSandbox(p) => CfsError::InvalidPath {
            path: p.clone(),
            reason: "outside_sandbox",
        },
        LocalError::NotFound(p) | LocalError::Io { path: p, .. } => CfsError::InvalidPath {
            path: p.clone(),
            reason: "read_failed",
        },
        other => CfsError::InvalidPath {
            path: String::new(),
            reason: other.code(),
        },
    }
}

/// The interactive-shell entrypoint the binary injects into `qfs_cmd::run` as its
/// [`ShellLauncher`](qfs_cmd::ShellLauncher). Builds the engine + read registry with a local-FS
/// mount over the process working directory (the `/local` sandbox boundary) plus every CONNECTed
/// cloud/sql/git/declared surface and `/sys` (via [`register_cloud_and_sys_mounts`], the same wiring
/// the one-shot `qfs run` path uses), starts the session at `/local`, and runs the REPL over real
/// stdin/stdout. Returns the process exit code (always 0 — a clean EOF or a best-effort I/O error
/// both end the session without a panic).
#[must_use]
pub fn run_interactive_shell() -> i32 {
    use std::io::BufReader;
    // The local mount root is the process cwd (a sandbox boundary). A missing cwd falls back to
    // `.`, which the sandbox canonicalises; the shell never escapes it.
    let root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let (mut engine, reads) = local_engine_and_reads(root);
    // Parity with the one-shot `qfs run` path: mount every CONNECTed cloud/sql/git/declared surface
    // and `/sys`, so a documented `cp /local/x /drive/y` resolves `/drive` to a canonical write node
    // (not an unrouted literal that historically committed a zero-byte Drive upload). `/local` stays
    // rooted at the process cwd — the shell's sandbox boundary — while the cloud mounts join it.
    let reads = register_cloud_and_sys_mounts(&mut engine, reads);
    let start = VfsPath::root("local");

    let stdin = std::io::stdin();
    let mut input = BufReader::new(stdin.lock());
    let mut out = std::io::stdout();
    if let Err(e) = run_repl(&engine, &reads, start, &mut input, &mut out) {
        // A broken stdout pipe is not a domain error; surface it without a panic.
        let _ = writeln!(std::io::stderr(), "shell io error: {e}");
    }
    0
}

/// Build a read registry with the local mount's read facet registered under the `local` driver
/// id, plus the engine with the local driver mounted at `/local` over `root`.
#[must_use]
fn local_engine_and_reads(root: PathBuf) -> (Engine, ReadRegistry) {
    let mut engine = Engine::new();
    // The introspective + (unused-here) apply facets: the shell only reads, but registering the
    // driver gives the planner its describe schema + pushdown profile + the namespace archetype
    // the `cd` gate checks.
    let _ = engine
        .mounts
        .register(Arc::new(LocalFsDriver::new(root.clone())));
    let reads = ReadRegistry::new().with(
        DriverId::new("local"),
        Arc::new(LocalReadDriver::new(Sandbox::new(root))),
    );
    (engine, reads)
}

/// The `(Engine, ReadRegistry, SafetyMode)` for the one-shot `qfs run` path (injected into qfs-cmd
/// as the run-context provider). Registers the local-FS driver — its introspective + pushdown facet
/// in the engine's mounts (so `/local/<p>` resolves + plans) and its read facet in the registry
/// (so the scan executes) — rooted at `/`, mirroring the commit driver's mapping, and resolves the
/// active selectable **safety mode** (t59) that governs the one-shot commit gate. qfs-cmd stays
/// off qfs-driver-local; the binary (the leaf) owns this adapter, like the shell + commit
/// composition. Other drivers join here as their read facets land.
#[must_use]
pub fn run_engine_and_reads() -> (Engine, ReadRegistry, qfs_core::SafetyMode) {
    // The active safety mode (t59): the persisted /sys/settings choice, else the env config, else
    // the safe default — resolved once for this run-context.
    let safety_mode = crate::sys::resolve_active_safety_mode();
    let (mut engine, reads) = local_engine_and_reads(PathBuf::from("/"));
    let reads = register_cloud_and_sys_mounts(&mut engine, reads);
    (engine, reads, safety_mode)
}

/// The full `qfs run` run-context: the `(Engine, ReadRegistry, SafetyMode)` plus the §15 COMMIT
/// transform executor (blueprint §15, decision W). The executor holds the stored transform
/// definitions and the live [`LiveModelProvider`](crate::transform_providers::LiveModelProvider):
/// a `|> transform` COMMIT dispatches to the impl named by the definition's `provider` column
/// (`anthropic`/`openai`/`google`) over the confined `reqwest` transport, resolving the def's
/// `secret_ref` (`env:`/`vault:`) lazily at the call. An unknown provider still fails closed with a
/// structured "no model provider configured" error. PREVIEW never touches it, so spend legibility
/// stays model-free. When no System DB resolves there are no definitions and the executor is omitted
/// entirely.
#[must_use]
pub fn run_context() -> (
    Engine,
    ReadRegistry,
    qfs_core::SafetyMode,
    Option<Arc<dyn qfs_exec::TransformExecutor>>,
) {
    let (engine, reads, safety_mode) = run_engine_and_reads();
    let transform: Option<Arc<dyn qfs_exec::TransformExecutor>> =
        crate::transform::TransformDbBackend::open_default().map(|backend| {
            let defs = backend.load_full_defs();
            // The live provider dispatcher over the ONE confined reqwest transport (the binary is
            // the allowlisted reqwest leaf). Unknown providers fall through to a fail-closed
            // Unconfigured error inside the dispatcher.
            let http = Arc::new(qfs_driver_http::ReqwestClient::new(
                TRANSFORM_HTTP_TIMEOUT_SECS,
            ));
            let provider = Arc::new(crate::transform_providers::LiveModelProvider::new(http));
            Arc::new(
                crate::transform::BinaryTransformExecutor::new(provider, defs)
                    .with_vault_resolver(transform_vault_resolver()),
            ) as Arc<dyn qfs_exec::TransformExecutor>
        });
    (engine, reads, safety_mode, transform)
}

/// The per-request timeout (seconds) for a live model-provider call — a genuinely hung provider
/// fails closed as a transport timeout rather than wedging the commit thread (blueprint §7). Model
/// calls can be slow, so this is more generous than the generic REST default.
const TRANSFORM_HTTP_TIMEOUT_SECS: u64 = 120;

/// The `vault:<driver>/<connection>` secret resolver for a transform definition's `secret_ref`. The
/// executor strips the `vault:` scheme and hands the path portion here; we re-form the reference and
/// read it from the same envelope-encrypted store `qfs account add` writes (opened lazily, so a run
/// with no `vault:` def never touches the store). A locked/absent store fails closed with a
/// secret-free reason. The resolved value is handed straight to the provider call, never logged.
fn transform_vault_resolver() -> crate::transform::VaultResolver {
    Arc::new(|path: &str| {
        let vault: Arc<dyn qfs_secrets::Secrets> = crate::connection::open_store_for_commit()
            .map(|s| Arc::new(s) as Arc<dyn qfs_secrets::Secrets>)
            .ok_or_else(|| {
                "the credential vault is not available (locked or absent)".to_string()
            })?;
        let reference = format!("vault:{path}");
        let secret = crate::secret_ref::resolve_secret_ref(&reference, vault.as_ref())
            .map_err(|e| e.to_string())?;
        secret
            .expose_str()
            .map(str::to_string)
            .ok_or_else(|| "the vault secret is not valid UTF-8".to_string())
    })
}

/// Augment a base `(engine, reads)` (from [`local_engine_and_reads`]) with every non-`/local` mount
/// the process can reach: the connect-created cloud mounts (ADR 0008 §4), the live `/cf` native
/// mounts, the configured `/sql` and `/git` mounts, `/sys` administration, the Claude session
/// source, and the declared `/rest` drivers. Shared by the one-shot `qfs run` context
/// ([`run_engine_and_reads`]) and the interactive shell ([`run_interactive_shell`]) so both resolve
/// and plan the SAME path surface — the shell differs only in rooting `/local` at the process cwd
/// instead of `/`. Without this the interactive shell mounted only `/local`, so a documented
/// cross-driver `cp /local/x /drive/y` could not resolve `/drive` to build a canonical write node.
fn register_cloud_and_sys_mounts(engine: &mut Engine, reads: ReadRegistry) -> ReadRegistry {
    // t100040 (the CONNECT model): NOTHING third-party is pre-mounted. Only the minimal system set
    // (`/local`, wired by `local_engine_and_reads`, plus `/sys`, `/transform`, `/type` and the
    // local credential-free `/claude` facade below) is always present; every third-party driver
    // (gmail/gdrive/ga/github/slack/s3/r2/cf/rest/fs) is reachable ONLY
    // after a `CONNECT`, mounted at its user path from the project DB `path_binding` registry. The
    // read + apply facets stay keyed by canonical driver id (`commit.rs`, the reads below), so THIS
    // path-keyed planning registry is the gate: an un-CONNECTed path simply does not resolve.
    let cloud_mounts = crate::cloud_mounts::load_cloud_mounts();
    let mut live_cf_drivers: BTreeMap<String, Arc<CfDriver>> = BTreeMap::new();
    for mount in cloud_mounts.iter().filter(|m| m.kind == "cf") {
        let Some(driver) = crate::cf::live_driver_for_mount(mount).map(Arc::new) else {
            continue;
        };
        if let Ok(wrapped) = crate::mount_adapter::MountDriver::new(&mount.path, driver.clone()) {
            let _ = engine.mounts.register(Arc::new(wrapped));
            live_cf_drivers.insert(mount.path.clone(), driver);
        }
    }
    crate::describe::register_defined_paths_where(&mut engine.mounts, |b| {
        b.driver_id.as_deref() != Some("cf") || !live_cf_drivers.contains_key(&b.path)
    });
    // SQL: register the live SQLite-backed mount when configured, so `/sql/<conn>/<table>`
    // statements PLAN against the real introspected catalog (the same registry the commit apply
    // driver uses). Skipped when no `QFS_SQL_*` connection is configured.
    if crate::sql::has_connections() {
        let _ = engine.mounts.register(Arc::new(crate::sql::sql_driver()));
    }
    // Git: register the live git mount when configured, so `/git/<repo>/...` statements PLAN against
    // the real repository's refs and the engine's plan_write seam lowers commit INSERTs.
    if crate::git::has_connections() {
        let _ = engine.mounts.register(Arc::new(crate::git::git_driver()));
    }
    // Sys (t53): register the `/sys/*` administration mount (its PURE describe/capabilities/pushdown
    // facet, so `/sys/users |> …` and `INSERT INTO /sys/policies …` resolve + plan + gate) plus
    // the live read facet (so a `/sys` scan returns real rows). The read source is the binary's
    // injected System-DB backend; when no System DB resolves the mount still plans (describe is
    // cred-free) but a scan over an unwired `/sys` surfaces a structured read error.
    let _ = engine
        .mounts
        .register(Arc::new(qfs_driver_sys::SysDriver::new()));
    let mut reads = reads;
    if let Some(backend) = crate::sys::SystemDbBackend::open_default() {
        reads = reads.with(
            DriverId::new("sys"),
            Arc::new(crate::sys::SysReadDriver::new(std::sync::Arc::new(backend))),
        );
    }
    // §15 transform definitions (decision W): the `/transform` mount (pure describe/plan/gate) plus
    // its live read facet (so `ls /transform` / `SELECT /transform` returns real rows with the
    // DERIVED mode). The read source is the binary's injected System-DB backend; when no System DB
    // resolves the mount still plans (describe is cred-free) but a scan surfaces a structured error.
    // The same open also feeds the plan-time definition registry below — ONE System-DB open.
    let _ = engine
        .mounts
        .register(Arc::new(qfs_driver_transform::TransformDriver::new()));
    let mut transform_defs = qfs_types::TransformDefs::new();
    if let Some(backend) = crate::transform::TransformDbBackend::open_default() {
        transform_defs = backend.load_defs();
        reads = reads.with(
            DriverId::new("transform"),
            Arc::new(crate::transform::TransformReadDriver::new(
                std::sync::Arc::new(backend),
            )),
        );
    }
    // §15 (decision W): install the resolved transform definitions on the registry so the pure
    // planner/evaluator can resolve a `|> transform <name>` stage's OUTPUT schema + mode at plan
    // time (the planner cannot read the System DB itself). Empty when no System DB resolves — a
    // transform then lowers to a structured "unresolved" error rather than silently passing through.
    engine.mounts.set_transform_defs(transform_defs);
    // §5.4/§5.5 declared types: the `/type` CATALOG mount (pure, cred-free describe — so
    // `describe /type/customer` teaches a shape even unwired) plus its live read facet, which makes
    // `ls /type` = SHOW TYPES true in the binary. The rows are the SAME `/sys/drivers` `kind='type'`
    // declarations `of <name>` resolves; the catalog is a READ face only — install/remove stay
    // previewed writes to `/sys/drivers` (the mount exposes no write verb). When no System DB
    // resolves the mount still plans, but a scan surfaces a structured read error.
    let _ = engine
        .mounts
        .register(Arc::new(qfs_driver_type::TypeDriver::new()));
    if let Some(backend) = crate::type_catalog::TypeDbBackend::open_default() {
        reads = reads.with(
            DriverId::new("type"),
            Arc::new(crate::type_catalog::TypeReadDriver::new(
                std::sync::Arc::new(backend),
            )),
        );
    }
    // §5.6: install the resolved declared-type definitions so the pure planner/evaluator can resolve
    // a `|> of <name>` assertion's structural schema + refinement at plan time (the same reason
    // transform defs are pre-loaded — the planner cannot read the System DB). Empty when no System DB
    // resolves, so a named `of` then reports `of_type_unresolved` rather than silently passing.
    engine
        .mounts
        .set_declared_types(crate::declared_driver::load_declared_type_defs());
    // Claude sessions (mission claude-code-sessions-…): register the `/claude` introspective
    // mount UNCONDITIONALLY, like `/sys` and `/transform` — describe is pure and credential-free,
    // and without this registration the planner raised `unknown_source` before the read facet was
    // ever consulted (the pre-mission one-line omission that made the whole driver unreachable).
    // The LIVE read facet stays opt-in (QFS_CLAUDE_SESSIONS names the real `~/.claude` store), so
    // an unconfigured scan fails closed with the read-registry's structured error.
    let _ = engine
        .mounts
        .register(Arc::new(qfs_driver_claude::ClaudeDriver::new()));
    // Path canon (owner ruling 2026-07-16, ticket 20260717010400): the session surface is
    // canonical under the hosts realm — `/hosts/<host>/claude/...`; the bare `/claude/...`
    // spelling is retired and fails with a `retired_path` pointer at the canonical form.
    engine.mounts.require_host_realm("/claude");
    if let Some(source) = crate::claude::ClaudeStoreSource::open_default() {
        reads = reads.with(
            DriverId::new("claude"),
            Arc::new(crate::claude::ClaudeReadDriver::new(std::sync::Arc::new(
                source,
            ))),
        );
    }
    // Markdown collection paths (hermetic, local): every declared `/markdown/<name>` root
    // (a `CONNECT /markdown/<name> TO markdown AT '<dir>'` path_binding row — the declared-
    // drivers convention, no env var) registers BOTH the mount (so the planner resolves) and
    // the read facet (so the scan executes). Registering both is load-bearing: `/claude` above
    // shipped read-facet-only and stayed describable but unqueryable (`unknown_source`) — the
    // claude mission's documented finding. Skipped entirely when nothing is declared
    // (fail-closed, like every mount).
    reads = crate::markdown::register_markdown_mounts(engine, reads);
    // SQL (hermetic, no network): register the live SQLite-backed read facet when a connection is
    // configured, so `FROM /sql/<conn>/<table> |> WHERE … |> SELECT …` executes — the native SELECT
    // pushes the WHERE/ORDER/LIMIT into the database and the residual is re-filtered locally. Skipped
    // (leaving the source unresolvable) when no `QFS_SQL_*` connection resolves, so it fails closed.
    if crate::sql::has_connections() {
        reads = reads.with(
            DriverId::new("sql"),
            Arc::new(crate::read_facets::SqlReadDriver::new(Arc::new(
                crate::sql::sql_driver(),
            ))),
        );
    }
    // Git (hermetic, no network): register the in-house object-reader read facet when a repo is
    // configured, so `FROM /git/<repo>@<ref>/commits` (and refs/tags/reflog/changes/blame + tree
    // listings) executes against the local `.git`. Skipped (source unresolvable) when no `QFS_GIT_*`
    // repo resolves, so it fails closed.
    if crate::git::has_connections() {
        reads = reads.with(
            DriverId::new("git"),
            Arc::new(crate::read_facets::GitReadDriver::new(Arc::new(
                crate::git::git_driver(),
            ))),
        );
    }
    // Cloud mounts (ADR 0008 §4 — mount-bound accounts): every connect-created cloud mount
    // registers its OWN read facet under the mount's segment id, bound to the MOUNT's account —
    // never a process-global selection — and wrapped in a MountReadDriver so the scan's source
    // id + path land back on the wrapped driver's canonical namespace. A mount whose live client
    // cannot bind (no account, no operator app, refused t54 gate, unresolvable credential) gets
    // the honest connect-account facet instead, so a `FROM` over it fails with an ACTIONABLE
    // error (never the internal-sounding `unknown_source`, never a read without authorization).
    for mount in cloud_mounts {
        let Some(remap) = mount.remap() else { continue };
        let cf_driver = live_cf_drivers.get(&mount.path).cloned();
        reads = match cloud_read_facet(&mount, cf_driver) {
            // A live facet speaks the wrapped driver's canonical namespace — remap the scan in.
            Some(facet) => reads.with(
                remap.outer_id(),
                Arc::new(crate::mount_adapter::MountReadDriver::new(remap, facet)),
            ),
            // The quiet bind failed (locked store, missing app/account, refused gate). Register
            // the LAZY facet: it retries the bind AT SCAN TIME — the one moment the query
            // provably reads this mount — where a human at a terminal is prompted for the store
            // passphrase (once per process). Registered UNWRAPPED so its errors echo the scan's
            // own path, like the old honest fallback.
            None => {
                let hint = connect_hint(&mount.kind);
                reads.with(
                    remap.outer_id(),
                    Arc::new(LazyCloudReadDriver::new(mount.clone(), hint)),
                )
            }
        };
    }
    // §13 declared drivers: a connect-created mount whose `driver_id` names a `/sys/drivers`
    // declaration registers a LIVE `RestDriver` read facet (real transport), wrapped in a
    // `/rest/<name>` remap. The reconstructed config carries `allowed_hosts`, so the facet is pinned
    // to its declared host (the confinement guard). Nothing is registered when no declared driver is
    // connected (fail-closed, like every cloud mount).
    let declared_types = crate::declared_driver::load_declared_types();
    for mount in crate::declared_driver::declared_mounts() {
        let path = mount.path;
        let d = mount.driver;
        let Some(remap) = crate::declared_driver::declared_remap(&path, &d.name) else {
            continue;
        };
        let client = crate::declared_driver::declared_http_client(&d);
        let secrets = crate::declared_driver::declared_secrets(
            &d,
            mount.secret_ref.as_deref(),
            mount.account.as_deref(),
            mount.app.as_deref(),
        );
        // The view specs (tier 2): reading a declared mount evaluates the matched view's stored body.
        let views = crate::declared_eval::view_specs(&d, &declared_types);
        let Some(driver) = crate::declared_driver::live_rest_driver(&d, client, secrets) else {
            continue;
        };
        let facet = crate::read_facets::RestReadDriver::new(
            driver.rest_applier().clone(),
            d.name.clone(),
            views,
        );
        reads = reads.with(
            remap.outer_id(),
            Arc::new(crate::mount_adapter::MountReadDriver::new(
                remap,
                Arc::new(facet),
            )),
        );
    }
    // §13 declared D1 nested mounts: the LIVE read facet for a `/cloudflare/d1` twin — a
    // `CfReadDriver` over the declared `CfDriver` (its wildcard-D1 template serves the committed
    // catalog), backed by an `HttpApiBackend` built from the DECLARED inputs: the Cloudflare account
    // id from the mount's `AT` locator + the bearer the mount's auth resolves. No `list_*`/
    // `introspect_d1` at mount time. Fail-closed like every cloud mount: a mount with no account id
    // or no resolvable bearer registers nothing (a scan then fails `unknown_source`, honest).
    for m in crate::declared_driver::declared_sql_mounts() {
        let Some(remap) = crate::declared_driver::declared_d1_remap(&m.prefix) else {
            continue;
        };
        let Some(account_id) = m
            .mount
            .at_locator
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        else {
            continue;
        };
        let Some(token) = crate::declared_driver::declared_auth_bearer(&m.mount) else {
            continue;
        };
        let backend = crate::cf::declared_d1_backend(account_id, token);
        let driver = Arc::new(crate::cf::declared_d1_driver(backend, m.resource.catalog()));
        let facet = crate::read_facets::CfReadDriver::new(driver);
        reads = reads.with(
            remap.outer_id(),
            Arc::new(crate::mount_adapter::MountReadDriver::new(
                remap,
                Arc::new(facet),
            )),
        );
    }

    reads
}

/// The actionable hint when a cloud mount cannot bind because the encrypted store itself cannot
/// be unlocked (no keychain slot, no `QFS_PASSPHRASE`, no terminal to prompt) — distinct from
/// the per-kind connect hints, which would misleadingly claim "no usable account" for an
/// account that exists behind a locked vault.
pub(crate) const LOCKED_STORE_HINT: &str =
    "the encrypted credential store is locked — export QFS_PASSPHRASE (`read -rs \
     QFS_PASSPHRASE && export QFS_PASSPHRASE`) or run on a terminal so qfs can prompt, then \
     retry";

/// A cloud mount whose live client could not bind **quietly** at registry build. The registry
/// build must never prompt (it runs for every `qfs run`, including credential-free previews and
/// purely local reads), so the bind is deferred to `scan` — the moment the executing query
/// provably reads THIS mount. There, [`crate::connection::ensure_store_unlocked_for_scan`]
/// prompts a human at a terminal for the store passphrase (once per process, cached) and the
/// live facet is built and delegated to; bound at most once ([`std::sync::OnceLock`]). With no
/// terminal, or with the store unlocked but the app/account genuinely absent, the scan fails
/// with the matching actionable hint (locked-store vs connect-account) — never a read without
/// authorization, never a misleading message.
struct LazyCloudReadDriver {
    mount: crate::cloud_mounts::CloudMount,
    connect_hint: &'static str,
    /// Set only on a SUCCESSFUL bind — a failed attempt is retried on the next scan (an
    /// interactive shell session can fix the cause — authorize the account, export the
    /// passphrase — and re-run without restarting).
    bound: std::sync::OnceLock<Arc<dyn ReadDriver>>,
}

impl LazyCloudReadDriver {
    fn new(mount: crate::cloud_mounts::CloudMount, connect_hint: &'static str) -> Self {
        Self {
            mount,
            connect_hint,
            bound: std::sync::OnceLock::new(),
        }
    }

    /// One bind attempt: unlock the store (prompting if a terminal allows), rebuild the live
    /// facet, and wrap it onto this mount's namespace. Runs on a plain OS thread — the prompt
    /// blocks on `/dev/tty` and the facet build constructs the blocking transport, neither of
    /// which may run on the async executor (the t203030 nested-runtime class).
    fn bind(&self) -> Option<Arc<dyn ReadDriver>> {
        std::thread::scope(|s| {
            s.spawn(|| {
                if !crate::connection::ensure_store_unlocked_for_scan() {
                    return None;
                }
                let facet = cloud_read_facet(&self.mount, None)?;
                let remap = self.mount.remap()?;
                let wrapped: Arc<dyn ReadDriver> =
                    Arc::new(crate::mount_adapter::MountReadDriver::new(remap, facet));
                Some(wrapped)
            })
            .join()
        })
        .unwrap_or(None)
    }
}

#[async_trait::async_trait]
impl ReadDriver for LazyCloudReadDriver {
    async fn scan(&self, scan: &ScanNode) -> Result<RowBatch, CfsError> {
        let facet = match self.bound.get() {
            Some(facet) => facet.clone(),
            None => match self.bind() {
                Some(facet) => self.bound.get_or_init(|| facet).clone(),
                None => {
                    // Choose the honest hint: a store that still cannot be unlocked is the
                    // cause (locked vault), else the app/account is genuinely not configured.
                    let reason = if crate::connection::open_store_for_commit().is_none() {
                        LOCKED_STORE_HINT
                    } else {
                        self.connect_hint
                    };
                    return Err(CfsError::InvalidPath {
                        path: scan.path.clone(),
                        reason,
                    });
                }
            },
        };
        facet.scan(scan).await
    }
}

/// Build the live read facet for one cloud mount, bound to the mount's account — or `None`
/// (fail closed) when the mount cannot bind. Mirrors `crate::commit::cloud_apply_driver` so the
/// read and apply funnels can never disagree about which account a mount binds.
fn cloud_read_facet(
    mount: &crate::cloud_mounts::CloudMount,
    cf_driver: Option<Arc<CfDriver>>,
) -> Option<Arc<dyn ReadDriver>> {
    let connection = mount.account.as_deref().unwrap_or("default");
    match mount.kind.as_str() {
        "gmail" => {
            let stack = crate::commit::google_stack_for_mount(mount)?;
            let client: Arc<dyn qfs_driver_gmail::GmailClient> = Arc::new(
                qfs_driver_gmail::GoogleApiGmailClient::new(stack.api.clone()),
            );
            Some(Arc::new(crate::read_facets::GmailReadDriver::new(client)))
        }
        "gdrive" | "drive" => {
            let stack = crate::commit::google_stack_for_mount(mount)?;
            let client: Arc<dyn qfs_driver_gdrive::GDriveClient> = Arc::new(
                qfs_driver_gdrive::GoogleApiDriveClient::new(stack.api.clone()),
            );
            Some(Arc::new(crate::read_facets::DriveReadDriver::new(client)))
        }
        "ga" | "google-analytics" => {
            let stack = crate::commit::google_stack_for_mount(mount)?;
            let client: Arc<dyn qfs_driver_ga::GaClient> =
                Arc::new(qfs_driver_ga::GoogleApiGaClient::new(stack.api.clone()));
            let driver = Arc::new(qfs_driver_ga::GaDriver::new(client));
            Some(Arc::new(crate::read_facets::GaReadDriver::new(driver)))
        }
        "github" => {
            let client = crate::clients::live_github_client(connection)?;
            Some(Arc::new(crate::read_facets::GitHubReadDriver::new(client)))
        }
        "slack" => {
            let client = crate::clients::live_slack_client(connection)?;
            Some(Arc::new(crate::read_facets::SlackReadDriver::new(client)))
        }
        "s3" => {
            let driver =
                crate::commit::live_obj_read_driver(qfs_driver_objstore::Scheme::S3, connection)?;
            Some(Arc::new(crate::read_facets::ObjReadDriver::new(Arc::new(
                driver,
            ))))
        }
        "r2" => {
            let driver =
                crate::commit::live_obj_read_driver(qfs_driver_objstore::Scheme::R2, connection)?;
            Some(Arc::new(crate::read_facets::ObjReadDriver::new(Arc::new(
                driver,
            ))))
        }
        "cf" => {
            let driver =
                cf_driver.or_else(|| crate::cf::live_driver_for_mount(mount).map(Arc::new))?;
            Some(Arc::new(crate::read_facets::CfReadDriver::new(driver)))
        }
        _ => None,
    }
}

/// The actionable, secret-free hint a cloud mount surfaces when its live client cannot bind —
/// the ADR 0008 connect flow (`account add` then `connect`), per kind.
pub(crate) fn connect_hint(kind: &str) -> &'static str {
    match kind {
        "gmail" => {
            "this mail mount has no usable Google account — run `qfs app add google <app>`, \
             `qfs account add google <email> --app <app>`, then `qfs connect <path> --driver gmail --account <email>`"
        }
        "gdrive" | "drive" => {
            "this Drive mount has no usable Google account — run `qfs app add google <app>`, \
             `qfs account add google <email> --app <app>`, then `qfs connect <path> --driver gdrive --account <email>`"
        }
        "ga" | "google-analytics" => {
            "this Analytics mount has no usable Google account — run `qfs app add google <app>`, \
             `qfs account add google <email> --app <app>`, then `qfs connect <path> --driver ga --account <email>`"
        }
        "github" => {
            "this GitHub mount has no usable account — run `qfs account add github <label>`, \
             then `qfs connect <path> --driver github --account <label>`"
        }
        "slack" => {
            "this Slack mount has no usable workspace token — run `qfs account add slack \
             <label>`, then `qfs connect <path> --driver slack --account <label>`"
        }
        "s3" => {
            "this S3 mount has no usable credentials — run `qfs account add objstore <label>`, \
             then `qfs connect <path> --driver s3 --account <label>` (S3 reads need a \
             credentialed bucket)"
        }
        "r2" => {
            "this R2 mount has no usable credentials — run `qfs account add objstore <label>`, \
             then `qfs connect <path> --driver r2 --account <label>`"
        }
        "cf" => {
            "this Cloudflare mount has no usable account token or account id — run `qfs account \
             add cf <label>`, then `qfs connect <path> --driver cf --account <label>` (add \
             `--at <cloudflare-account-id>` when the token can see multiple accounts)"
        }
        _ => {
            "this mount's driver has no live read facet yet — see `qfs describe` for its \
             surface"
        }
    }
}

/// Render one [`Outcome`] to `out` (human text). The shell reuses qfs-exec's renderers for the
/// row/plan DTOs so the formatting matches the one-shot path.
fn render(outcome: &Outcome, out: &mut dyn Write) -> std::io::Result<()> {
    use qfs_exec::{Renderer, TableRenderer};
    let r = TableRenderer;
    match outcome {
        Outcome::Listing(rows) => r.rows(rows, out),
        Outcome::Preview(plans) => {
            writeln!(
                out,
                "PREVIEW ({} effect plan(s), nothing applied):",
                plans.len()
            )?;
            for p in plans {
                r.plan(p, out)?;
            }
            writeln!(out, "type COMMIT to apply")
        }
        Outcome::Committed(plans) => {
            writeln!(out, "COMMITTED ({} effect plan(s)):", plans.len())?;
            for p in plans {
                r.plan(p, out)?;
            }
            Ok(())
        }
        // The same report the one-shot `qfs describe` renders, through the same renderer — the
        // in-session form only adds cwd-relative addressing.
        Outcome::Described(report) => r.describe(report, out),
        Outcome::Moved(loc) => writeln!(out, "{loc}"),
        Outcome::Cwd(loc) => writeln!(out, "{loc}"),
        Outcome::Empty => Ok(()),
    }
}

/// Run the REPL against the given input/output streams. Generic over `BufRead`/`Write` so tests
/// feed scripted lines and capture the rendered transcript — no real terminal required.
///
/// A bare `COMMIT` on its own line is the typed confirmation that applies the **previous**
/// previewed effect line (the safety gate). Any other line is evaluated PREVIEW-by-default.
fn run_repl(
    engine: &Engine,
    reads: &ReadRegistry,
    start: VfsPath,
    input: &mut dyn BufRead,
    out: &mut dyn Write,
) -> std::io::Result<()> {
    run_repl_with_history_and_apply(
        engine,
        reads,
        start,
        history_path(),
        Some(&crate::commit::apply_plan),
        input,
        out,
    )
}

/// The history-injectable REPL core (tests pass `None` to stay hermetic — no real history file
/// is touched; the dispatch passes the resolved config path).
#[cfg(test)]
fn run_repl_with_history(
    engine: &Engine,
    reads: &ReadRegistry,
    start: VfsPath,
    history: Option<PathBuf>,
    input: &mut dyn BufRead,
    out: &mut dyn Write,
) -> std::io::Result<()> {
    run_repl_with_history_and_apply(engine, reads, start, history, None, input, out)
}

fn run_repl_with_history_and_apply(
    engine: &Engine,
    reads: &ReadRegistry,
    start: VfsPath,
    history: Option<PathBuf>,
    apply: Option<&qfs_exec::WorldApply>,
    input: &mut dyn BufRead,
    out: &mut dyn Write,
) -> std::io::Result<()> {
    let mut session = Session::new(start, engine, reads);
    if let Some(apply) = apply {
        session = session.with_world_apply(apply);
    }
    // The pending effect line awaiting a typed COMMIT confirmation (PREVIEW safety gate).
    let mut pending: Option<String> = None;
    // Best-effort persistent history (no creds ever pass through the shell, so the file is
    // safe). A `None` path disables it (used by hermetic tests).
    let mut history = History::open(history);

    write!(out, "{}", session.prompt())?;
    out.flush()?;
    let mut line = String::new();
    while input.read_line(&mut line)? != 0 {
        let trimmed = line.trim_end_matches(['\n', '\r']).to_string();
        line.clear();
        if !trimmed.trim().is_empty() {
            history.push(&trimmed);
        }

        // A bare `COMMIT` confirms the pending previewed effect.
        if trimmed.trim().eq_ignore_ascii_case("COMMIT") {
            if let Some(prev) = pending.take() {
                emit(&mut session, &prev, true, out)?;
            } else {
                writeln!(out, "nothing to commit")?;
            }
        } else {
            // Evaluate PREVIEW-by-default. If it produced an effect preview, remember the line so
            // a following bare COMMIT can apply it (the safety gate).
            match session.eval_line(&trimmed, false) {
                Ok(outcome @ Outcome::Preview(_)) => {
                    render(&outcome, out)?;
                    pending = Some(trimmed.clone());
                }
                Ok(outcome) => {
                    pending = None;
                    render(&outcome, out)?;
                }
                Err(e) => {
                    pending = None;
                    use qfs_exec::Renderer;
                    let _ = qfs_exec::TableRenderer.error(&e, out);
                }
            }
        }

        write!(out, "{}", session.prompt())?;
        out.flush()?;
    }
    writeln!(out)?;
    Ok(())
}

/// Evaluate one line at the given commit level and render it (used by both the normal path and
/// the bare-COMMIT confirmation).
fn emit(
    session: &mut Session,
    line: &str,
    commit: bool,
    out: &mut dyn Write,
) -> std::io::Result<()> {
    match session.eval_line(line, commit) {
        Ok(outcome) => render(&outcome, out),
        Err(e) => {
            use qfs_exec::Renderer;
            let _ = qfs_exec::TableRenderer.error(&e, out);
            Ok(())
        }
    }
}

/// A minimal, best-effort append-only command history file under the qfs config dir. No creds
/// ever pass through the shell, so the file holds nothing sensitive. All file I/O is
/// best-effort — a missing config dir or a write failure silently disables persistence without
/// breaking the REPL. (The minimal std line-reader does not bind up-arrow recall; the file is
/// the durable record an editor upgrade would consume.)
struct History {
    path: Option<PathBuf>,
}

impl History {
    /// Open the history at `path` (best-effort). `None` disables persistence.
    fn open(path: Option<PathBuf>) -> Self {
        Self { path }
    }

    /// Append one line to the persistent history file (best-effort).
    fn push(&mut self, line: &str) {
        if let Some(p) = &self.path {
            if let Some(parent) = p.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(p)
            {
                let _ = writeln!(f, "{line}");
            }
        }
    }
}

/// The qfs config dir for the persistent history file (`$XDG_CONFIG_HOME/qfs` or `~/.config/qfs`).
/// Best-effort: a missing home just disables persistent history.
#[must_use]
fn history_path() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .map(|base| base.join("qfs").join("history"))
}

#[cfg(test)]
mod tests {
    //! Golden REPL tests: feed scripted lines through `run_repl` over an in-memory cursor and a
    //! real temp-dir local mount, asserting the rendered transcript + the PREVIEW/COMMIT safety
    //! gate end-to-end (ticket t28 acceptance). The local mount root is ALWAYS a tempdir — these
    //! tests never touch a system path.
    use super::*;
    use std::io::Cursor;
    use tempfile::TempDir;

    /// A temp-dir local mount with a small fixed tree, and the engine + reads wired to it.
    fn fixture() -> (TempDir, Engine, ReadRegistry) {
        let dir = TempDir::new().expect("tempdir");
        std::fs::write(dir.path().join("a.md"), b"alpha").unwrap();
        std::fs::write(dir.path().join("b.txt"), b"beta").unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        std::fs::write(dir.path().join("sub").join("c.md"), b"gamma").unwrap();
        let (engine, reads) = local_engine_and_reads(dir.path().to_path_buf());
        (dir, engine, reads)
    }

    /// Run a scripted session and return the captured transcript.
    fn run_script(engine: &Engine, reads: &ReadRegistry, script: &str) -> String {
        let mut input = Cursor::new(script.as_bytes().to_vec());
        let mut out: Vec<u8> = Vec::new();
        // `None` history keeps the test hermetic (no real ~/.config/qfs/history write).
        run_repl_with_history(
            engine,
            reads,
            VfsPath::root("local"),
            None,
            &mut input,
            &mut out,
        )
        .expect("repl runs");
        String::from_utf8(out).expect("utf8 transcript")
    }

    /// The `fixture()` engine PLUS the §5.4 `/type` catalog mount and, when `wire_reads` is set,
    /// its read facet over an in-memory System DB carrying two declared types. `wire_reads = false`
    /// models the unwired deployment (no System DB resolves): the mount still describes, but a scan
    /// has no source — the fail-closed posture.
    fn type_fixture(wire_reads: bool) -> (TempDir, Engine, ReadRegistry) {
        let (dir, mut engine, mut reads) = fixture();
        let _ = engine
            .mounts
            .register(Arc::new(qfs_driver_type::TypeDriver::new()));
        let _ = engine
            .mounts
            .register(Arc::new(qfs_driver_transform::TransformDriver::new()));
        if wire_reads {
            let conn = rusqlite::Connection::open_in_memory().expect("in-memory db");
            conn.execute_batch(
                "CREATE TABLE sys_drivers (
                     id         INTEGER PRIMARY KEY,
                     kind       TEXT NOT NULL,
                     name       TEXT NOT NULL,
                     body       TEXT,
                     created_at TEXT
                 );
                 -- the stored key is the `/type/...` PATH form (what `of <name>` normalises into)
                 INSERT INTO sys_drivers (kind, name, body, created_at) VALUES
                   ('type', '/type/customer', '{\"columns\":[{\"name\":\"id\",\"type\":\"int\"}],\"where\":null}', '2026-07-14T00:00:00Z'),
                   ('type', '/type/chatwork/message', '{\"columns\":[{\"name\":\"body\",\"type\":\"text\"}],\"where\":null}', '2026-07-14T00:00:00Z'),
                   -- a non-type declaration must NOT leak into the type catalog
                   ('driver', 'chatwork_driver', '{}', '2026-07-14T00:00:00Z');",
            )
            .expect("seed sys_drivers");
            reads = reads.with(
                DriverId::new("type"),
                Arc::new(crate::type_catalog::TypeReadDriver::new(Arc::new(
                    crate::type_catalog::TypeDbBackend::new(conn),
                ))),
            );
        }
        (dir, engine, reads)
    }

    #[test]
    fn ls_over_the_type_catalog_is_show_types_end_to_end() {
        // blueprint §5.4/§9: `ls /type` IS SHOW TYPES — the catalog's rows ARE its enumeration, so
        // the entry-kind-typed `ls` lowers to the bare read and the declared names come back inside
        // one session. This is the assertion that makes the blueprint's claim true in the binary.
        let (_d, engine, reads) = type_fixture(true);
        let t = run_script(&engine, &reads, "ls /type\n");
        assert!(t.contains("customer"), "transcript:\n{t}");
        // The listed name is the REFERENCE spelling (§5.5) — paste-able into `of chatwork/message`.
        // The stored key is the path `/type/chatwork/message`; listing THAT would print the one
        // spelling the grammar rejects (`of /type/x` is a parse error).
        assert!(t.contains("chatwork/message"), "transcript:\n{t}");
        assert!(
            !t.contains("/type/chatwork/message"),
            "listed the stored path, not the reference name:\n{t}"
        );
        // Only `kind='type'` rows are the type catalog — a declared DRIVER is not a type.
        assert!(!t.contains("chatwork_driver"), "non-type row leaked:\n{t}");
    }

    #[test]
    fn describe_builtin_renders_type_and_transform_contracts_in_session() {
        // The `describe` builtin is PURE navigation: it folds the driver's introspective half and
        // renders the same report the one-shot `qfs describe` does — without leaving the session.
        // `/type/<name>` teaches a declared type's catalog shape; `/transform/<name>` the registry's.
        let (_d, engine, reads) = type_fixture(true);
        let t = run_script(
            &engine,
            &reads,
            "describe /type/customer\ndescribe /transform/classify\n",
        );
        assert!(t.contains("/type/customer"), "transcript:\n{t}");
        assert!(t.contains("refinement"), "type columns not rendered:\n{t}");
        assert!(t.contains("/transform/classify"), "transcript:\n{t}");
        assert!(t.contains("secret_ref"), "transform columns missing:\n{t}");
    }

    #[test]
    fn describe_builtin_addresses_the_cwd_and_is_relative_to_it() {
        // The in-session `describe` is the one form that can address the cwd (the one-shot CLI is
        // absolute-path-only, no cwd) — a bare `describe` reports where you already are, and a
        // relative argument resolves against it.
        let (_d, engine, reads) = type_fixture(true);
        let t = run_script(&engine, &reads, "cd sub\ndescribe\ndescribe c.md\n");
        assert!(t.contains("/local/sub"), "cwd describe missing:\n{t}");
        assert!(t.contains("/local/sub/c.md"), "relative describe:\n{t}");
    }

    #[test]
    fn describe_over_an_unmounted_path_is_a_structured_error_not_a_panic() {
        let (_d, engine, reads) = type_fixture(true);
        let t = run_script(&engine, &reads, "describe /nope\n");
        assert!(t.contains("no driver is mounted"), "transcript:\n{t}");
    }

    #[test]
    fn the_type_catalog_fails_closed_when_no_system_db_resolves() {
        // The purity split: with no System DB the read facet is never registered, so a SCAN over
        // `/type` fails closed with a structured error — while the cred-free DESCRIBE still plans
        // and still teaches the shape. A listing must never silently render as "no types declared".
        let (_d, engine, reads) = type_fixture(false);
        let t = run_script(&engine, &reads, "ls /type\ndescribe /type\n");
        assert!(
            !t.contains("customer"),
            "unwired catalog must not produce rows:\n{t}"
        );
        assert!(
            t.contains("error"),
            "expected a structured read error:\n{t}"
        );
        // DESCRIBE is cred-free and DB-free — it works regardless.
        assert!(t.contains("/type"), "describe must still teach:\n{t}");
        assert!(t.contains("refinement"), "describe must still teach:\n{t}");
    }

    #[test]
    fn ls_lists_local_directory_end_to_end() {
        let (_d, engine, reads) = fixture();
        let t = run_script(&engine, &reads, "ls\n");
        // The listing renders the local entries (real FS read through the wired ReadDriver).
        assert!(t.contains("a.md"), "transcript:\n{t}");
        assert!(t.contains("b.txt"), "transcript:\n{t}");
        assert!(t.contains("sub"), "transcript:\n{t}");
    }

    #[test]
    fn cd_then_ls_navigates_into_subdir() {
        let (_d, engine, reads) = fixture();
        let t = run_script(&engine, &reads, "cd sub\nls\n");
        // The prompt reflects the new cwd, and ls shows only the subdir's entry.
        assert!(t.contains("local:/sub$"), "prompt not updated:\n{t}");
        assert!(t.contains("c.md"), "transcript:\n{t}");
        assert!(!t.contains("a.md"), "should not list parent entries:\n{t}");
    }

    #[test]
    fn cat_reads_a_file_node() {
        let (_d, engine, reads) = fixture();
        let t = run_script(&engine, &reads, "cat a.md\n");
        assert!(t.contains("a.md"), "transcript:\n{t}");
    }

    #[test]
    fn rm_previews_and_does_not_apply_until_commit() {
        let (d, engine, reads) = fixture();
        // `rm a.md` previews (nothing applied); only a typed COMMIT removes it.
        let t = run_script(&engine, &reads, "rm a.md\n");
        assert!(t.contains("PREVIEW"), "rm must preview by default:\n{t}");
        assert!(t.contains("type COMMIT to apply"), "transcript:\n{t}");
        // The file still exists — nothing was applied.
        assert!(
            d.path().join("a.md").exists(),
            "PREVIEW must not delete the file"
        );
    }

    #[test]
    fn rm_then_commit_reaches_the_committed_plan_stage() {
        // The safety gate is asserted at the PLAN level (t28 acceptance: "asserted by plan
        // assertions, not live effects"): `rm` previews, then a typed COMMIT advances to the
        // committed-plan stage. This hermetic test path does not inject the binary's real
        // world-applier, so COMMIT uses qfs-exec's in-memory RecordingApplier fallback and leaves
        // the tempdir untouched.
        let (_d, engine, reads) = fixture();
        let t = run_script(&engine, &reads, "rm a.md\nCOMMIT\n");
        assert!(t.contains("PREVIEW"), "transcript:\n{t}");
        assert!(
            t.contains("COMMITTED"),
            "COMMIT reaches the committed stage:\n{t}"
        );
    }

    #[test]
    fn cp_previews_a_cross_node_plan() {
        let (d, engine, reads) = fixture();
        let t = run_script(&engine, &reads, "cp a.md a-copy.md\n");
        assert!(t.contains("PREVIEW"), "cp must preview:\n{t}");
        assert!(
            !d.path().join("a-copy.md").exists(),
            "PREVIEW must not create the copy"
        );
    }

    #[test]
    fn cp_from_local_to_a_cloud_mount_plans_and_commits() {
        // Ticket 20260707181404: a documented `cp /local/x /drive/y` must resolve the cloud target
        // (historically the shell mounted only `/local`, so `/drive` fell through to an unrouted
        // literal that committed a zero-byte upload). The interactive shell now mounts the CONNECTed
        // cloud surface (parity with `qfs run`, via `register_cloud_and_sys_mounts`); here we simulate
        // the Drive mount with the cred-free driver (`register_alias`, what `register_defined_paths`
        // does per DB row) and drive a cp of a real temp-dir file end to end. No client/token/network
        // — a real OAuth client only matters at the live COMMIT apply, so COMMIT reaches the committed
        // stage over qfs-exec's in-memory applier fallback.
        let (_d, mut engine, reads) = fixture();
        let drive = crate::describe::cred_free_driver("gdrive").expect("gdrive cred-free driver");
        engine
            .mounts
            .register_alias("/drive", drive)
            .expect("mount the connected drive path");
        let t = run_script(&engine, &reads, "cp a.md /drive/my/a.pdf\nCOMMIT\n");
        assert!(t.contains("PREVIEW"), "cp to /drive must preview:\n{t}");
        assert!(
            t.contains("/drive/my/a.pdf"),
            "the Drive target is planned:\n{t}"
        );
        assert!(
            t.contains("COMMITTED"),
            "COMMIT reaches the committed stage:\n{t}"
        );
    }

    #[test]
    fn raw_statement_runs_through_same_pipeline() {
        let (_d, engine, reads) = fixture();
        // A raw qfs read typed at the prompt produces a listing, same as the one-shot path.
        let t = run_script(&engine, &reads, "/local |> SELECT name\n");
        assert!(t.contains("a.md"), "raw statement listing:\n{t}");
    }

    #[test]
    fn a_connected_mail_path_plans_end_to_end() {
        // t100040: nothing is pre-mounted — a gmail driver is reachable only after a CONNECT, mounted
        // at its USER path. Here we simulate the binding by mounting the cred-free gmail driver at a
        // user path (`/work/mail`) via `register_alias` (what `register_defined_paths` does per DB
        // row), then a write RESOLVES + PLANS end to end (canonical `/mail/drafts` reconstruction,
        // t100030) with no client, no token, no network. A real OAuth client only matters at COMMIT.
        let (_d, mut engine, reads) = fixture();
        let gmail = crate::describe::cred_free_driver("gmail").expect("gmail cred-free driver");
        engine
            .mounts
            .register_alias("/work/mail", gmail)
            .expect("mount the connected path");
        let t = run_script(
            &engine,
            &reads,
            "INSERT INTO /work/mail/drafts VALUES ('alice@example.com', 'Hi', 'Body text')\n",
        );
        assert!(
            t.contains("PREVIEW"),
            "connected /work/mail must plan:\n{t}"
        );
        assert!(
            t.contains("type COMMIT to apply"),
            "the plan reaches the COMMIT gate:\n{t}"
        );
    }

    #[test]
    fn an_unconnected_third_party_path_does_not_resolve() {
        // t100040: the CONNECT model's floor — with no binding, a third-party path is NOT mounted, so
        // a statement over it fails to resolve rather than silently planning against a pre-mounted
        // driver. (Only `/local` — and `/sys` on the one-shot path — are always present.)
        let (_d, engine, reads) = fixture();
        let t = run_script(&engine, &reads, "/mail/drafts |> SELECT subject\n");
        assert!(
            !t.contains("PREVIEW") && !t.contains("subject"),
            "an un-CONNECTed /mail must not resolve:\n{t}"
        );
    }

    #[test]
    fn a_connected_s3_path_plans_end_to_end() {
        // t100040: the objstore driver, likewise, is reachable only after a CONNECT. Mount the
        // cred-free s3 driver at a user path and an UPSERT plans end to end (the per-node capability
        // gate keys off the driver's representative `bucket`; canonical `/s3/bucket/key`
        // reconstruction). The real SigV4 backend only matters at COMMIT.
        let (_d, mut engine, reads) = fixture();
        let s3 = crate::describe::cred_free_driver("s3").expect("s3 cred-free driver");
        engine
            .mounts
            .register_alias("/files", s3)
            .expect("mount the connected path");
        let t = run_script(
            &engine,
            &reads,
            "UPSERT INTO /files/bucket/key VALUES ('blob')\n",
        );
        assert!(t.contains("PREVIEW"), "connected /files must plan:\n{t}");
        assert!(
            t.contains("type COMMIT to apply"),
            "the plan reaches the COMMIT gate:\n{t}"
        );
    }

    #[test]
    fn cf_binding_falls_back_to_representative_mount_when_live_build_fails() {
        let _home = crate::testenv::HomeGuard::with_passphrase("cf-shell-fallback-test");
        let conn = crate::connection::open_system_conn().unwrap();
        crate::path_binding::db_upsert_binding(
            &conn,
            "/cf",
            "cf",
            Some("cloudflare-account-id"),
            None,
            None,
            Some("missing-cf-account"),
            None,
        )
        .unwrap();

        // §13 ratchet (ticket 20260718203326): the compiled `/cf` representative surface is now
        // queue (PULL) + artifacts only — D1/KV moved to the declared /cloudflare mount — so the
        // representative planning mount is exercised over `/cf/queue/q`.
        let (engine, reads, _safety) = run_engine_and_reads();
        assert!(
            engine.mounts.resolve_path("/cf/queue/q").is_some(),
            "failed live cf binding must retain the representative planning mount"
        );
        let t = run_script(&engine, &reads, "/cf/queue/q |> LIMIT 1\n");
        assert!(
            !t.contains("unknown_source"),
            "connected cf fallback must not produce unknown_source:\n{t}"
        );

        let scan = ScanNode {
            source: qfs_pushdown::SourceId::new("cf"),
            path: "/cf/queue/q".to_string(),
            pushed: qfs_pushdown::PushedQuery::default(),
            schema: qfs_core::Schema::new(Vec::new()),
        };
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let driver = reads.get(&DriverId::new("cf")).expect("cf read fallback");
        let err = rt.block_on(driver.scan(&scan)).unwrap_err();
        match err {
            CfsError::InvalidPath { reason, path } => {
                assert_eq!(path, "/cf/queue/q");
                assert!(
                    reason.contains("Cloudflare mount has no usable account token or account id"),
                    "expected Cloudflare connect hint, got: {reason}"
                );
            }
            other => panic!("expected actionable InvalidPath, got {other:?}"),
        }
    }

    #[test]
    fn bare_commit_with_no_pending_is_reported() {
        let (_d, engine, reads) = fixture();
        let t = run_script(&engine, &reads, "COMMIT\n");
        assert!(t.contains("nothing to commit"), "transcript:\n{t}");
    }

    /// The lazy cloud bind runs at SCAN time: with the store quietly unlockable (env passphrase,
    /// fresh home) but no OAuth app/account configured, the scan fails with the per-kind connect
    /// hint — never the locked-store hint (the store opened fine) and never a bare
    /// `unknown_source`. Hermetic: the bind fails before any transport is built.
    #[test]
    fn lazy_cloud_scan_reports_the_connect_hint_after_unlock() {
        let _g = crate::testenv::env_guard();
        let dir = TempDir::new().unwrap();
        let prev_xdg = std::env::var_os("XDG_CONFIG_HOME");
        let prev_pass = std::env::var_os("QFS_PASSPHRASE");
        let prev_id = std::env::var_os(crate::google::GOOGLE_CLIENT_ID_ENV);
        std::env::set_var("XDG_CONFIG_HOME", dir.path());
        std::env::set_var("QFS_PASSPHRASE", "lazy-scan-test-pass");
        std::env::remove_var(crate::google::GOOGLE_CLIENT_ID_ENV);

        let mount = crate::cloud_mounts::CloudMount {
            path: "/mail2".to_string(),
            kind: "gmail".to_string(),
            account: Some("you@example.com".to_string()),
            at_locator: None,
            app: None,
        };
        let driver = LazyCloudReadDriver::new(mount, connect_hint("gmail"));
        let scan = ScanNode {
            source: qfs_pushdown::SourceId::new("test"),
            path: "/mail2/inbox".to_string(),
            pushed: qfs_pushdown::PushedQuery::default(),
            schema: qfs_core::Schema::new(Vec::new()),
        };
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let err = rt.block_on(driver.scan(&scan)).unwrap_err();

        match prev_xdg {
            Some(v) => std::env::set_var("XDG_CONFIG_HOME", v),
            None => std::env::remove_var("XDG_CONFIG_HOME"),
        }
        match prev_pass {
            Some(v) => std::env::set_var("QFS_PASSPHRASE", v),
            None => std::env::remove_var("QFS_PASSPHRASE"),
        }
        if let Some(v) = prev_id {
            std::env::set_var(crate::google::GOOGLE_CLIENT_ID_ENV, v);
        }

        match err {
            CfsError::InvalidPath { reason, path } => {
                assert_eq!(path, "/mail2/inbox", "the error echoes the scan's own path");
                assert!(
                    reason.contains("no usable Google account"),
                    "an unlocked store with no app/account gets the connect hint, got: {reason}"
                );
                assert!(
                    !reason.contains("credential store is locked"),
                    "the locked-store hint must not fire when the store opened: {reason}"
                );
            }
            other => panic!("expected InvalidPath, got {other:?}"),
        }
    }
}
