//! The `qfs describe` composition root (ticket t39): builds the **describe-only** driver
//! [`MountRegistry`] that the `qfs describe <path>` subcommand consults, and injects it into
//! `qfs-cmd` via the [`qfs_cmd::DescribeProvider`].
//!
//! ## DESCRIBE is PURE — so the registry is cred-free
//! `DESCRIBE` reads only the **introspective** half of the [`qfs_core::Driver`] contract
//! (`describe` / `capabilities` / `procedures` / `prelude` / `pushdown`) — it never reaches
//! `Driver::applier`, so no credential is resolved, no socket is opened, no I/O happens (blueprint §3
//! purity invariant). Each driver is therefore constructed with its **public, cred-free mock
//! client** (`Mock*Client` — explicitly "no socket, no credentials") or a registry carrying a
//! representative bucket on a cred-free `MockObjectBackend` (s3/r2), which the introspective
//! half reads for capabilities but never *applies*.
//!
//! ## Why the binary owns this
//! qfs-cmd must stay off the concrete `qfs-driver-*` crates (the dep_direction guard). The binary
//! is the allowlisted leaf that may carry those edges, so the registry is built here and injected
//! — exactly like the t28 shell launcher and the t32 serve launcher.
//!
//! ## Coverage (the LIGHT facet of the CO-t29-1 driver-registration carry-over)
//! Registered cred-free (no backend registration needed for describe): **local, fs, mail, drive,
//! github, slack, ga, s3, r2**. The t68 `/fs` driver describes over an EMPTY (deny-all) root
//! allowlist — its pure introspective half names no host path. **sql / git / cf** require a
//! registered connection-catalog / repo
//! / D1-catalog for describe to resolve a concrete node (a *registration* requirement, not a
//! credential one), so their describe is covered by the `qfs-skill` golden corpus instead — where
//! the harness builds the registry with a fixture catalog. This is the documented fallback.
//!
//! ## Driver TYPES + CONNECT-ed paths (t100040, the defined-path model)
//! This registry serves two purposes and so registers two things: the available driver **types**
//! (the cred-free catalogue above — the source of `docs/drivers.md` and the "what can I CONNECT?"
//! reference, always present) AND the developer's **CONNECT-ed defined paths** (each `path_binding`
//! row, mounted at its user path via [`register_defined_paths`]). A fresh binary with no bindings
//! shows only the type catalogue, so the generated docs are unaffected. The PLANNING registry
//! (`shell.rs`) registers ONLY the minimal system set + the bindings — nothing third-party is
//! pre-mounted for *use*; describe is the broader catalogue-plus-connections surface.

use std::sync::Arc;

use qfs_core::MountRegistry;

/// Build the describe-only [`MountRegistry`]. Every driver is constructed cred-free; only the
/// introspective (pure) half is ever invoked by `qfs describe`. Registration failures are
/// impossible here (distinct mounts), but a duplicate would be dropped silently rather than
/// panicking — the registry stays a best-effort describe surface.
#[must_use]
/// A cred-free Cloudflare registry carrying ONE representative D1 database / KV namespace / queue,
/// so `qfs describe /cf/d1/db` (and the t40 driver catalogue) surface `/cf`'s real verbs over the
/// public in-memory [`MockCfBackend`](qfs_driver_cf::MockCfBackend) — the same "representative
/// resource" shape the objstore describe uses for `/s3/bucket`. Never *applied* (describe reads only
/// the introspective half), so no credential and no I/O ever happens.
pub(crate) fn cred_free_cf_registry() -> qfs_driver_cf::CfRegistry {
    use qfs_driver_cf::{Catalog, CfRegistry, D1Database, MockCfBackend, NoopArtifactTokenSealer};
    CfRegistry::new()
        .with_d1(
            "db",
            D1Database::new(Arc::new(MockCfBackend::new()), Catalog::new(Vec::new())),
        )
        .with_kv("ns", Arc::new(MockCfBackend::new()))
        .with_queue("q", Arc::new(MockCfBackend::new()))
        .with_artifacts(
            Arc::new(MockCfBackend::new().with_artifact_namespace("default")),
            Arc::new(NoopArtifactTokenSealer),
        )
}

/// Build the **cred-free** driver instance for a canonical driver id — the planning + describe
/// facet of a CONNECT-ed third-party driver (t100040). Mock / empty clients only: the planner and
/// `describe` only ever touch the pure introspective half (`describe`/`capabilities`/`pushdown`),
/// never `applier`, so no credential is resolved and no I/O happens. The driver's `mount()` / `id()`
/// stay CANONICAL (e.g. `/mail`, `mail`); a binding mounts this instance at the user path via
/// `register_alias`, and the `/<id>/<sub>` reconstruction (t100030) keeps the driver's own parser
/// working under the user path.
///
/// Returns `None` for a driver id with no cred-free constructor here — notably `sql`/`git`, whose
/// MULTI-connection planning driver is built from the declared-connection seam (`crate::sql` /
/// `crate::git`), not a single mock instance; those keep their existing config-gated registration.
#[must_use]
pub(crate) fn cred_free_driver(driver_id: &str) -> Option<Arc<dyn qfs_core::Driver>> {
    let driver: Arc<dyn qfs_core::Driver> = match driver_id {
        "gmail" => Arc::new(qfs_driver_gmail::GmailDriver::new(Arc::new(
            qfs_driver_gmail::MockGmailClient::new(),
        ))),
        "gdrive" | "drive" => Arc::new(qfs_driver_gdrive::GDriveDriver::new(Arc::new(
            qfs_driver_gdrive::MockDriveClient::default(),
        ))),
        "google-analytics" | "ga" => Arc::new(qfs_driver_ga::GaDriver::new(Arc::new(
            qfs_driver_ga::MockGaClient::default(),
        ))),
        "github" => Arc::new(qfs_driver_github::GitHubDriver::new(Arc::new(
            qfs_driver_github::MockGitHubClient::default(),
        ))),
        "slack" => Arc::new(qfs_driver_slack::SlackDriver::new(Arc::new(
            qfs_driver_slack::MockSlackClient::default(),
        ))),
        "s3" => Arc::new(qfs_driver_objstore::S3Driver::new(
            crate::objstore::planning_registry(qfs_driver_objstore::Scheme::S3),
        )),
        "r2" => Arc::new(qfs_driver_objstore::R2Driver::new(
            crate::objstore::planning_registry(qfs_driver_objstore::Scheme::R2),
        )),
        "cf" => Arc::new(qfs_driver_cf::CfDriver::new(cred_free_cf_registry())),
        "fs" => Arc::new(qfs_driver_fs::FsDriver::new(qfs_driver_fs::FsRoots::new())),
        "claude" => Arc::new(qfs_driver_claude::ClaudeDriver::new()),
        "rest" | "http" => {
            let json = qfs_core::CodecRegistry::with_builtins()
                .resolve("json")
                .ok()?;
            Arc::new(qfs_driver_http::RestDriver::new(
                qfs_driver_http::RestApiConfig::new("http://localhost", Vec::new()),
                json,
                Arc::new(qfs_driver_http::MockHttpClient::new()),
                Arc::new(qfs_secrets::InMemoryStore::new()),
            ))
        }
        _ => return None,
    };
    Some(driver)
}

/// Load the persisted defined-path bindings (best-effort, cred-free): the `path_binding` rows the
/// registration loop mounts. Returns an empty list when no System DB resolves (a fresh binary has
/// no third-party mounts — nothing is pre-mounted, exactly the CONNECT model; re-homed by
/// 20260716143641).
fn load_bindings() -> Vec<crate::path_binding::PathBindingRow> {
    match crate::store::open_system_db() {
        Ok(Some(sys)) => {
            let conn = sys.into_db().into_connection();
            crate::path_binding::db_list_bindings(&conn).unwrap_or_default()
        }
        _ => Vec::new(),
    }
}

/// Register every CONNECT-ed defined path from the project DB `path_binding` registry into `reg`
/// (t100040). A FULL connect mounts the cred-free driver for its `driver_id` at the user path,
/// wrapped in a [`MountDriver`](crate::mount_adapter::MountDriver) so the mount's leading
/// segment IS the driver's plan identity (ADR 0008 §4 — per-mount `driver.id()`, so N mounts of
/// one kind plan as N distinct sources); an ALIAS mounts the SAME (already wrapped) driver its
/// target resolves to, reusing the target's identity. NOTHING is pre-mounted — a third-party
/// path resolves ONLY after a CONNECT. Fail-open per binding (an unknown driver id, a malformed
/// path, or a dangling alias is skipped, never a panic) so one bad row cannot sink the registry.
pub(crate) fn register_defined_paths(reg: &mut MountRegistry) {
    register_defined_paths_where(reg, |_| true);
}

/// Register defined paths matching `include`. The run planner uses this to let Cloudflare mounts
/// install their live discovered catalog instead of the generic representative driver.
pub(crate) fn register_defined_paths_where(
    reg: &mut MountRegistry,
    include: impl Fn(&crate::path_binding::PathBindingRow) -> bool,
) {
    let bindings = load_bindings();
    // §13 two-source registry: the declared drivers (`/sys/drivers` rows) join the compiled set.
    let declared = crate::declared_driver::load_declared_drivers();
    report_shadowed_declared(&declared);
    // Full connects first, so an alias's target mount already exists when the alias is processed.
    for b in bindings
        .iter()
        .filter(|b| b.alias_of.is_none())
        .filter(|b| include(b))
    {
        let Some(id) = b.driver_id.as_deref() else {
            continue;
        };
        // §13 two-source registry, compiled wins: a COMPILED driver mounts via the single-segment
        // remap ([`MountDriver::new`]); only a name the compiled set does not know falls through to
        // a DECLARED driver, mounted via the `/rest/<name>` remap so its capabilities resolve.
        if let Some(driver) = cred_free_driver(id) {
            if let Ok(wrapped) = crate::mount_adapter::MountDriver::new(&b.path, driver) {
                let _ = reg.register(Arc::new(wrapped));
            }
        } else if let Some(d) = declared.iter().find(|d| d.name == id) {
            if let Some(wrapped) = crate::declared_driver::declared_describe_mount(&b.path, d) {
                let _ = reg.register(Arc::new(wrapped));
            }
        }
    }
    for b in bindings
        .iter()
        .filter(|b| b.alias_of.is_some())
        .filter(|b| include(b))
    {
        if let Some(target) = &b.alias_of {
            if let Some((driver, _)) = reg.resolve_path(target) {
                let _ = reg.register_alias(&b.path, driver);
            }
        }
    }
    // §13 declared D1 nested mounts: a connected declared driver carrying a `CREATE SQL … TABLES(…)`
    // resource gets a SECOND, nested mount (`/cloudflare/d1`, slash-bearing id `cloudflare/d1`) served
    // by the declared `CfDriver` twin — the D1 relational surface from the committed catalog, not a
    // mount-time `introspect_d1`. This is the PLAN/DESCRIBE mount (pure, network-free): the twin is
    // built over a cred-free `MockCfBackend` + the declared catalog, so `describe`/capabilities/the
    // pushdown planner all read the declared catalog with zero I/O (the live backend lives only in
    // the read/apply facets). Registered unconditionally — the nested `/cloudflare/d1` prefix has no
    // compiled counterpart, so the `include` filter (which only skips already-live compiled `/cf`
    // mounts) never applies. Fail-closed: nothing declared, nothing registered.
    for m in crate::declared_driver::declared_sql_mounts() {
        let Some(remap) = crate::declared_driver::declared_d1_remap(&m.prefix) else {
            continue;
        };
        let backend = Arc::new(qfs_driver_cf::MockCfBackend::new());
        let driver: Arc<dyn qfs_core::Driver> =
            Arc::new(crate::cf::declared_d1_driver(backend, m.resource.catalog()));
        let wrapped = crate::mount_adapter::MountDriver::with_remap(remap, driver);
        let _ = reg.register(Arc::new(wrapped));
    }
}

/// Report (never silently shadow) each declared driver whose name collides with a compiled driver:
/// the compiled one wins, so the declaration is inert. Returns the shadowed names (also logged), so a
/// collision is observable per §13's "reported, never silently shadowed" rule.
fn report_shadowed_declared(declared: &[crate::declared_driver::DeclaredDriver]) -> Vec<String> {
    let shadowed: Vec<String> = declared
        .iter()
        .map(|d| d.name.clone())
        .filter(|name| cred_free_driver(name).is_some())
        .collect();
    for name in &shadowed {
        tracing::warn!(
            driver = %name,
            "a declared driver is shadowed by a compiled driver of the same name (compiled wins)"
        );
    }
    shadowed
}

/// The COMPILED, connection-independent describe registry: every cred-free driver TYPE the binary
/// ships — the source of the generated `docs/drivers.md`. It carries NO operator CONNECT-ed,
/// declared, live-`/sql`, or `/git` mounts, so it is a pure function of the binary: deterministic
/// and independent of whatever this machine has connected. `gen-docs` renders from THIS (via
/// [`crate::catalog::driver_catalog`]) so a live-connected declared driver can neither leak into nor
/// de-idempotent the generated catalog (the `rendering_is_idempotent` regression).
pub fn compiled_describe_registry() -> MountRegistry {
    let mut reg = MountRegistry::new();

    // Each driver's describe facet, constructed cred-free (mock client / empty registry). The
    // `register` result is intentionally ignored: distinct mounts never collide, and a describe
    // registry that dropped one entry is still a valid (if smaller) surface — never a panic.
    let drivers: Vec<Arc<dyn qfs_core::Driver>> = vec![
        // Blob: the reference local-FS driver (genuinely cred-free).
        Arc::new(qfs_driver_local::LocalFsDriver::new("/")),
        // Blob: the t68 first-class `/fs` driver. DESCRIBE is PURE — it names no host path and does
        // no I/O — so it describes cred-free over an EMPTY (deny-all) root allowlist; the live roots
        // are injected only on the apply registry (`commit.rs`). This is what makes `/fs` appear in
        // the generated `docs/drivers.md` without exposing any operator-configured directory.
        Arc::new(qfs_driver_fs::FsDriver::new(qfs_driver_fs::FsRoots::new())),
        // Append: Gmail (fixed describe; the MockGmailClient is never called by describe).
        Arc::new(qfs_driver_gmail::GmailDriver::new(Arc::new(
            qfs_driver_gmail::MockGmailClient::new(),
        ))),
        // Blob: Google Drive (fixed describe).
        Arc::new(qfs_driver_gdrive::GDriveDriver::new(Arc::new(
            qfs_driver_gdrive::MockDriveClient::default(),
        ))),
        // Object-graph: GitHub (path-keyed describe; no backend registration needed).
        Arc::new(qfs_driver_github::GitHubDriver::new(Arc::new(
            qfs_driver_github::MockGitHubClient::default(),
        ))),
        // Append/object: Slack (path-keyed describe).
        Arc::new(qfs_driver_slack::SlackDriver::new(Arc::new(
            qfs_driver_slack::MockSlackClient::default(),
        ))),
        // Relational: Google Analytics (path-keyed describe; schema filled at query time).
        Arc::new(qfs_driver_ga::GaDriver::new(Arc::new(
            qfs_driver_ga::MockGaClient::default(),
        ))),
        // Blob: S3 + R2 over a registry carrying ONE representative bucket (`bucket`), built on
        // the public, cred-free `MockObjectBackend` (in-memory fixtures — no creds, no socket, no
        // network). Per-node capabilities are gated on a *registered* bucket (a registration
        // requirement, not a credential one), so registering this one representative bucket lets
        // `qfs describe /s3/bucket/key` — and the t40 driver catalog — surface S3/R2's real blob
        // verbs instead of an empty set. The mock backend is never *applied* (DESCRIBE reads only
        // the introspective half), so no I/O ever happens.
        Arc::new(qfs_driver_objstore::S3Driver::new(
            qfs_driver_objstore::ObjRegistry::new().with_bucket(
                "bucket",
                qfs_driver_objstore::Bucket::new(Arc::new(
                    qfs_driver_objstore::MockObjectBackend::new(),
                )),
            ),
        )),
        Arc::new(qfs_driver_objstore::R2Driver::new(
            qfs_driver_objstore::ObjRegistry::new().with_bucket(
                "bucket",
                qfs_driver_objstore::Bucket::new(Arc::new(
                    qfs_driver_objstore::MockObjectBackend::new(),
                )),
            ),
        )),
        // t53 administration: the `/sys/*` admin surface. DESCRIBE is PURE — SysDriver owns NO
        // backend and NO creds (its read source + applier are injected from the binary), so it
        // describes `/sys/users`, `/sys/audit`, … cred-free, exactly like the other introspective
        // facets. This is what makes `/sys/*` appear in the generated `docs/drivers.md`.
        Arc::new(qfs_driver_sys::SysDriver::new()),
        // t64 AI-sessions (roadmap M7): the `/claude/...` session surface. DESCRIBE is PURE —
        // ClaudeDriver owns NO session source and NO creds (its read source + applier are injected
        // from the binary), so it describes `/claude/sessions` + `.../instructions` cred-free,
        // exactly like the other introspective facets. Decision W: the `/claude` driver is a path
        // façade over session metadata + an append-log, NOT qfs calling an LLM (qfs's model-calling
        // surface is `|> transform`, §15). This is what makes `/claude/*` appear in the generated
        // `docs/drivers.md`.
        Arc::new(qfs_driver_claude::ClaudeDriver::new()),
        // The markdown collection path (マークダウン収集パス, minimal slice): the
        // `/markdown/<name>/{documents,links}` tables. DESCRIBE is PURE — MarkdownDriver owns
        // NO root and NO creds (the declared roots feed only the binary's read facet), so it
        // describes both tables cred-free for any tree name. This is what makes `/markdown`
        // appear in the generated `docs/drivers.md`. The links schema deliberately carries NO
        // relation-type column: the closed relation vocabulary is a later, separate mission
        // layered on `source_section_path`.
        Arc::new(qfs_driver_markdown::MarkdownDriver::new()),
        // §15 transform definitions (decision W): the `/transform` definition registry. DESCRIBE is
        // PURE — TransformDriver owns NO backend and NO creds (its read source + applier are injected
        // from the binary), so it describes `/transform` cred-free, exactly like the other
        // introspective facets. This is what makes `/transform` appear in the generated `docs/drivers.md`.
        Arc::new(qfs_driver_transform::TransformDriver::new()),
        // §5.4/§5.5 declared types: the `/type` catalog — the type namespace's inspection surface
        // (`ls /type` = SHOW TYPES, `DESCRIBE /type/customer` teaches the shape). DESCRIBE is PURE —
        // TypeDriver owns NO backend and NO creds (its System-DB read source is injected from the
        // binary), so it describes `/type` cred-free like the other introspective facets. This is
        // what makes `/type` appear in the generated `docs/drivers.md`. The catalog is READ-ONLY:
        // a type is installed by a previewed write to `/sys/drivers` (`kind='type'`), never through
        // this mount, and referenced by NAME (`of customer`), never by path (§5.5).
        Arc::new(qfs_driver_type::TypeDriver::new()),
        // Cloudflare (/cf) + the generic HTTP/REST (/rest) drivers: their PURE describe surfaces,
        // built cred-free (empty registry / placeholder config), so `qfs describe /cf` and
        // `qfs describe /rest` resolve and the t40 driver catalogue surfaces them — closing the
        // "exist in the code but aren't reachable as paths" gap. Live read/commit + per-resource
        // config (which D1/KV/queues; which REST resource maps) are the follow-up.
        Arc::new(qfs_driver_cf::CfDriver::new(cred_free_cf_registry())),
        // NOTE (t58): the `/directories/...` identity-directory driver is deliberately NOT
        // registered here. `/directories` is a RESERVED SCOPE REALM (decision P / §1.3 —
        // `RESERVED_REALMS`), not a driver-backed mount like `/sys`, so `MountRegistry::register`
        // governance rejects a `/directories` mount (proven by
        // `register_rejects_a_driver_mount_that_shadows_a_realm`). The t58 driver's PURE,
        // credential-free describe surface (`qfs_driver_directory::DirectoryDriver`) and its read
        // seam are instead consumed directly by the live `member_of` resolver in `src/directory.rs`;
        // routing a scope-realm `/directories/<provider>/groups` path THROUGH the driver for `qfs
        // describe` is the documented seam this read-first slice leaves open.
    ];

    for driver in drivers {
        // Ignore a (theoretically impossible) duplicate-mount error: the describe surface is
        // best-effort and must never panic.
        let _ = reg.register(driver);
    }
    // The generic HTTP/REST driver's cred-free describe mount (placeholder config + mock client +
    // empty in-memory secrets — never applied). Its codec is resolved from the builtin set; if that
    // somehow fails, /rest is simply absent rather than panicking (the best-effort describe rule).
    if let Ok(json) = qfs_core::CodecRegistry::with_builtins().resolve("json") {
        let _ = reg.register(Arc::new(qfs_driver_http::RestDriver::new(
            qfs_driver_http::RestApiConfig::new("http://localhost", Vec::new()),
            json,
            Arc::new(qfs_driver_http::MockHttpClient::new()),
            Arc::new(qfs_secrets::InMemoryStore::new()),
        )));
    }
    // Path canon (owner ruling 2026-07-16, ticket 20260717010400): `/claude` is reachable only
    // under the hosts realm — `DESCRIBE /claude/...` fails with the `retired_path` pointer and
    // the t40 catalog renders the canonical `/hosts/<host>/claude/...` address.
    reg.require_host_realm("/claude");
    reg
}

/// The FULL describe registry `qfs describe` uses: the compiled catalogue PLUS this deployment's
/// live/connected mounts (CONNECT-ed defined paths, declared drivers, live `/sql` + `/git`).
/// gen-docs deliberately does NOT use this — it renders from [`compiled_describe_registry`] so the
/// committed `docs/drivers.md` never depends on what this machine has connected.
pub fn describe_registry() -> MountRegistry {
    let mut reg = compiled_describe_registry();
    // t100040: ALSO surface the developer's CONNECT-ed defined paths, so `qfs describe` shows both
    // the available driver TYPES (the catalogue above, cred-free, the source of `docs/drivers.md`)
    // AND the paths this deployment has actually connected.
    register_defined_paths(&mut reg);
    // The `/sql` describe mount (split-brain fix, ticket 20260705000500): register the LIVE sql
    // driver — introspected over whatever connection resolves (a `QFS_SQL_*` env var, a
    // `connections.qfs` declaration, OR a `qfs connect /sql/<conn>` binding, the canonical source) —
    // so `describe /sql/<conn>` (SHOW TABLES) and `describe /sql/<conn>/<table>` (columns) reflect
    // the live catalog. SQLite introspects from the file (cred-free); pg/mysql introspect on connect
    // (best-effort, an unreachable DB leaves `/sql` unregistered). This converges describe with the
    // runtime driver (`commit::live_registry`), which builds from the SAME `crate::sql::sql_driver()`.
    if crate::sql::has_connections() {
        let _ = reg.register(Arc::new(crate::sql::sql_driver()));
    }
    // The `/git` describe mount (ticket 20260706170000, matching the `/sql` convergence above):
    // register the LIVE git driver — planning repos over whatever connection resolves (a `QFS_GIT_*`
    // env var, a `connections.qfs` DRIVER git declaration, OR a `qfs connect /git/<repo>` binding,
    // the canonical source) — so `describe /git/<repo>/...` resolves for a path_binding-connected
    // repo, not just an env/declared one. This converges describe with the runtime driver
    // (`shell`/`commit`), which build from the SAME `crate::git::git_driver()`. A fresh binary with
    // no git connection registers nothing here, so the generated `docs/drivers.md` is unaffected.
    if crate::git::has_connections() {
        let _ = reg.register(Arc::new(crate::git::git_driver()));
    }
    reg
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The describe registry resolves the acceptance path `/mail/drafts` to its driver, and that
    /// driver's introspective half folds into a populated [`qfs_core::DescribeReport`] — no creds,
    /// no I/O (the mock client is never called).
    #[test]
    fn mail_drafts_describes_cred_free() {
        let reg = describe_registry();
        let (driver, _rest) = reg
            .resolve_path("/mail/drafts")
            .expect("/mail is registered in the describe registry");
        let report = qfs_core::DescribeReport::from_driver(
            driver.as_ref(),
            &qfs_core::Path::new("/mail/drafts"),
        )
        .expect("/mail/drafts is describable");
        assert_eq!(report.archetype, qfs_core::Archetype::AppendLog);
        assert!(!report.columns.is_empty(), "mail describe has columns");
        // The SEND prelude alias is surfaced for the agent (mail.send desugar target).
        assert!(report.aliases.iter().any(|a| a.name == "SEND"));
        // The irreversible mail.send procedure is declared.
        assert!(report
            .procedures
            .iter()
            .any(|p| p.name == "send" && p.irreversible));
        // Drafts supports INSERT + UPSERT (the retry-safe default).
        assert!(report.verbs.insert && report.verbs.upsert);
    }

    /// Every registered mount resolves and describes a representative node without creds — proving
    /// the registry is genuinely cred-free across all eight drivers.
    #[test]
    fn all_registered_mounts_describe_cred_free() {
        let reg = describe_registry();
        let cases = [
            ("/local/x.txt", qfs_core::Archetype::BlobNamespace),
            ("/mail/drafts", qfs_core::Archetype::AppendLog),
            ("/drive/Reports", qfs_core::Archetype::BlobNamespace),
            (
                "/github/o/r/pulls",
                qfs_core::Archetype::ObjectGraphWorkflow,
            ),
            (
                "/slack/ws/#general/messages",
                qfs_core::Archetype::AppendLog,
            ),
            ("/s3/bucket/key", qfs_core::Archetype::BlobNamespace),
            ("/r2/bucket/key", qfs_core::Archetype::BlobNamespace),
        ];
        for (path, want) in cases {
            let (driver, _rest) = reg
                .resolve_path(path)
                .unwrap_or_else(|| panic!("{path} resolves to a registered describe driver"));
            let report =
                qfs_core::DescribeReport::from_driver(driver.as_ref(), &qfs_core::Path::new(path))
                    .unwrap_or_else(|e| panic!("{path} should describe cred-free: {e:?}"));
            assert_eq!(report.archetype, want, "archetype mismatch for {path}");
        }
    }

    /// §13 two-source registry: a declared driver whose name collides with a compiled one loses
    /// (compiled wins) and is REPORTED, never silently shadowed; a declared-only name resolves to the
    /// reconstructed wire driver.
    #[test]
    fn two_source_resolution_lets_compiled_win_and_reports_the_shadow() {
        use crate::declared_driver::{DeclaredDriver, DeclaredMap, DeclaredNode};
        use qfs_core::Driver;
        let decl = |name: &str| DeclaredDriver {
            name: name.to_string(),
            base_url: "https://api.example.io/v1".to_string(),
            auth: r#"{"kind":"none"}"#.to_string(),
            pagination: None,
            views: Vec::<DeclaredNode>::new(),
            maps: Vec::<DeclaredMap>::new(),
        };
        let declared = vec![decl("slack"), decl("chatwork")];

        // `slack` collides with a COMPILED driver → compiled wins (the compiled slack driver, mount
        // `/slack`); the declared `slack` is never mounted.
        assert!(cred_free_driver("slack").is_some(), "slack is compiled");
        assert_eq!(cred_free_driver("slack").unwrap().mount(), "/slack");
        // `chatwork` is declared-only (no compiled) → a declared describe mount resolves at the
        // connect path via the `/rest/<name>` remap.
        assert!(
            cred_free_driver("chatwork").is_none(),
            "chatwork is not compiled"
        );
        let mount = crate::declared_driver::declared_describe_mount("/chatwork", &declared[1])
            .expect("declared chatwork mounts");
        assert_eq!(mount.mount(), "/chatwork");

        // The collision is reported (never silent).
        assert_eq!(
            report_shadowed_declared(&declared),
            vec!["slack".to_string()]
        );
    }

    /// Split-brain fix (ticket 20260705000500; owner path model = `/sql/<conn>`): a `qfs connect
    /// /sql/<conn>` binding — the CANONICAL `path_binding` source — wires BOTH the runtime sql driver
    /// AND the describe mount, and `describe /sql/<conn>/<table>` reflects the live catalog. One
    /// source (the project DB), not two split-brain registries.
    #[test]
    fn qfs_connect_sql_binding_converges_run_and_describe() {
        let _home = crate::testenv::HomeGuard::with_passphrase("sql-split-test");
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("shop.db");
        {
            let c = rusqlite::Connection::open(&db_path).unwrap();
            c.execute("CREATE TABLE items (id INTEGER, name TEXT)", [])
                .unwrap();
        }
        // Seed a `CONNECT /sql/shop TO sqlite AT '<db_path>'` binding in the system DB.
        let proj = crate::store::open_system_db()
            .unwrap()
            .unwrap()
            .into_db()
            .into_connection();
        crate::path_binding::db_upsert_binding(
            &proj,
            "/sql/shop",
            "sqlite",
            db_path.to_str(),
            None,
            Some("local"),
            None,
            None,
        )
        .unwrap();
        drop(proj);

        // Runtime convergence: the persisted binding now wires the sql driver (was invisible before,
        // so `qfs run /sql/shop` reported `no driver registered for sql`).
        assert!(
            crate::sql::has_connections(),
            "the qfs-connect binding wires the runtime sql driver"
        );

        // Describe convergence: `/sql/shop/items` resolves (was `unknown_mount … describe registry`)
        // AND its describe reflects the live catalog columns (the DBMS ticket's gate, now exercised
        // through the mount, not just the golden corpus).
        let reg = describe_registry();
        let (driver, _rest) = reg
            .resolve_path("/sql/shop/items")
            .expect("describe /sql/<conn>/<table> resolves through the sql mount");
        let report = qfs_core::DescribeReport::from_driver(
            driver.as_ref(),
            &qfs_core::Path::new("/sql/shop/items"),
        )
        .expect("describe /sql/shop/items");
        let cols: Vec<&str> = report.columns.iter().map(|c| c.name.as_str()).collect();
        assert!(
            cols.contains(&"id") && cols.contains(&"name"),
            "describe reflects the live catalog columns: {cols:?}"
        );
    }

    #[test]
    fn qfs_connect_gdrive_binding_resolves_under_outer_mount() {
        let driver = cred_free_driver("gdrive").expect("gdrive cred-free driver");
        let driver = crate::mount_adapter::MountDriver::new("/gdrive", driver)
            .expect("gdrive remounts under the user path");
        let report =
            qfs_core::DescribeReport::from_driver(&driver, &qfs_core::Path::new("/gdrive/my"))
                .expect("describe /gdrive/my");
        let cols: Vec<&str> = report.columns.iter().map(|c| c.name.as_str()).collect();
        assert!(
            cols.contains(&"name"),
            "Drive file columns are visible: {cols:?}"
        );
    }

    /// git → `path_binding` convergence (ticket 20260706170000, mirroring the `/sql` convergence
    /// above): a `qfs connect /git/<repo> TO git AT '<path>'` binding — the CANONICAL `path_binding`
    /// source — wires BOTH the runtime git driver (`crate::git::has_connections`) AND the describe
    /// mount, so `describe /git/<repo>/commits` resolves for a path_binding-connected repo. Before
    /// this ticket git rode ONLY the env-var/`connections.qfs` seam, so a persisted `qfs connect`
    /// binding was invisible to run and describe — one source now (the project DB), not two.
    #[test]
    fn qfs_connect_git_binding_converges_run_and_describe() {
        let _home = crate::testenv::HomeGuard::with_passphrase("git-split-test");
        let dir = tempfile::tempdir().unwrap();
        // The AT path need not be a populated repo: the mount is registered per binding regardless
        // (an unreadable repo just yields empty refs, best-effort), which is all resolution needs.
        let repo_path = dir.path().join("app.git");
        std::fs::create_dir_all(&repo_path).unwrap();

        // Seed a `CONNECT /git/app TO git AT '<repo_path>'` binding in the system DB.
        let proj = crate::store::open_system_db()
            .unwrap()
            .unwrap()
            .into_db()
            .into_connection();
        crate::path_binding::db_upsert_binding(
            &proj,
            "/git/app",
            "git",
            repo_path.to_str(),
            None,
            Some("local"),
            None,
            None,
        )
        .unwrap();
        drop(proj);

        // Runtime convergence: the persisted binding now wires the git driver (was invisible before,
        // so `qfs run /git/app/...` reported `unknown source git`). `has_connections` feeds the
        // shell + commit registries, which build from the same `git_driver()`.
        assert!(
            crate::git::has_connections(),
            "the qfs-connect binding wires the runtime git driver"
        );

        // Describe convergence: `/git/app/commits` resolves through the path_binding-wired git mount
        // (a path_binding-only repo was previously absent from the describe registry entirely).
        let reg = describe_registry();
        assert!(
            reg.resolve_path("/git/app/commits").is_some(),
            "describe /git/<repo>/commits resolves through the path_binding-wired git mount"
        );
    }

    /// §13 declared D1 nested mount (ticket 20260718203326, Stage 2b): a connected declared
    /// `cloudflare` driver carrying a `CREATE SQL … TABLES(…)` resource registers a SECOND, nested
    /// describe mount at `/cloudflare/d1` (id `cloudflare/d1`). `describe /cloudflare/d1/<db>/<table>`
    /// resolves through it and reflects the DECLARED catalog — network-free (the plan/describe twin is
    /// built over a `MockCfBackend`, so DESCRIBE reads only the committed catalog, never introspection).
    #[test]
    fn declared_d1_nested_mount_describes_the_declared_catalog_network_free() {
        let _home = crate::testenv::HomeGuard::new();
        const CF_D1_BODY: &str = r#"{"dialect":"sqlite",
            "query_endpoint":"/http/cloudflare/accounts/{account}/d1/database/{database}/query",
            "tables":[
              {"name":"users","columns":[
                {"name":"id","type":"text","nullable":false,"primary_key":true,"unique":false},
                {"name":"email","type":"text","nullable":false,"primary_key":false,"unique":false}]}
            ]}"#;
        {
            let sys = crate::store::open_system_db()
                .unwrap()
                .expect("system db resolves");
            let conn = sys.into_db().into_connection();
            conn.execute(
                "INSERT INTO sys_drivers (kind, name, base_url, auth, verb, body, irreversible) \
                 VALUES ('driver', 'cloudflare', 'https://api.cloudflare.com/client/v4', \
                         '{\"kind\":\"account\",\"provider\":\"cf\"}', NULL, NULL, 0)",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO sys_drivers (kind, name, body, irreversible) \
                 VALUES ('sql', '/cloudflare/d1/{database}', ?1, 0)",
                rusqlite::params![CF_D1_BODY],
            )
            .unwrap();
            crate::path_binding::db_upsert_binding(
                &conn,
                "/cloudflare",
                "cloudflare",
                Some("cf-acct-id"),
                None,
                None,
                Some("mycf"),
                None,
            )
            .unwrap();
        }

        let reg = describe_registry();
        // The nested mount is longest-prefix over the plain `/cloudflare` REST mount.
        let (driver, _rest) = reg
            .resolve_path("/cloudflare/d1/mydb/users")
            .expect("the nested D1 describe mount resolves");
        let report = qfs_core::DescribeReport::from_driver(
            driver.as_ref(),
            &qfs_core::Path::new("/cloudflare/d1/mydb/users"),
        )
        .expect("describe /cloudflare/d1/mydb/users cred-free");
        assert_eq!(report.archetype, qfs_core::Archetype::RelationalTable);
        let cols: Vec<&str> = report.columns.iter().map(|c| c.name.as_str()).collect();
        assert!(
            cols.contains(&"id") && cols.contains(&"email"),
            "the DECLARED D1 catalog columns are described: {cols:?}"
        );
        // A table absent from the declared catalog is not describable (the surface is exactly the
        // committed declaration — no hidden introspection fallback).
        assert!(
            qfs_core::DescribeReport::from_driver(
                driver.as_ref(),
                &qfs_core::Path::new("/cloudflare/d1/mydb/absent"),
            )
            .is_err(),
            "an undeclared D1 table is not describable"
        );
    }
}
