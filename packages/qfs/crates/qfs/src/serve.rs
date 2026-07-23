//! The `qfs serve` composition root (t32): the binary wires the HTTP serving binding.
//!
//! The HTTP binding (`qfs-http`) is a LEAF that consumes `qfs-server` (the registry + reconcile
//! seam) AND the `qfs-exec` read executor — putting its composition HERE (the terminal binary,
//! the HTTP sibling of the t28 shell composition root) keeps `qfs-cmd` off both crates and lets
//! tokio dead-end in the terminal sink. qfs-cmd dispatches `serve` to this launcher via the
//! injected [`qfs_cmd::ServeLauncher`]; this builds the engine + read registry, runs
//! [`qfs_http::serve_config`] on a tokio runtime, and returns the process exit code.

use std::path::Path;
use std::sync::Arc;

use qfs_core::{CodecRegistry, Engine};
use qfs_exec::ReadRegistry;

/// The default bounded in-memory result-row cap for `qfs serve`.
const SERVE_MAX_ROWS: usize = 10_000;

/// Boot + run `qfs serve <config>` with the HTTP binding wired. Returns the process exit code
/// (`0` clean shutdown, `1` on a boot / bind / runtime error). Never panics.
#[must_use]
pub fn run_serve(config: &Path) -> i32 {
    // The serve-side engine: codecs registered (json/csv response encoding) + an empty mount
    // registry the real driver crates register into (E4/E7 wiring). At t32 the read drivers a
    // boot config references are registered by the deployment; an unregistered source surfaces
    // as a structured 422 at request time, never a panic.
    let mut engine = Engine::new();
    engine.codecs = CodecRegistry::with_builtins();
    // t36: register the always-available, credential-free built-in read sources (the `/status`
    // daemon-liveness table) so a liveness ENDPOINT serves a real JSON body over loopback before
    // any deployment driver is wired. The deployment registers its real E4 drivers on top.
    let mut reads = ReadRegistry::new();
    crate::serve_builtins::register_builtins(&mut engine, &mut reads);

    // Mission `a-request-resolves-to-a-principal` (item 8): register the credential-free,
    // always-available `/sys` read facet on the serve face, so an ENDPOINT `AS /sys/whoami`
    // resolves over HTTP exactly as it does in one-shot `qfs run` (before this, `AS /sys/whoami`
    // was refused at registration with `UnroutedPath` and `GET /whoami` 404'd). The describe/plan
    // facet (`SysDriver`) is mounted unconditionally; the read facet is the real System-DB backend
    // when one resolves, else the whoami-only `AnonymousSysBackend` so the not-signed-in answer
    // stays a first-class row even pre-init. `/sys/whoami` is resolved from the REQUEST PRINCIPAL
    // (never the backend), so it needs no connected account.
    if let Err(e) = engine
        .mounts
        .register(Arc::new(qfs_driver_sys::SysDriver::new()))
    {
        tracing::warn!(target: "qfs::serve", error = %e, "could not register the /sys mount");
    }
    let sys_backend: Arc<dyn qfs_driver_sys::SysBackend> =
        match crate::sys::SystemDbBackend::open_default() {
            Some(backend) => Arc::new(backend),
            None => Arc::new(crate::sys::AnonymousSysBackend::new()),
        };
    reads.register(
        qfs_core::DriverId::new("sys"),
        Arc::new(crate::sys::SysReadDriver::new(sys_backend)),
    );

    // Claude sessions (mission claude-code-sessions-…, live-app gate): the `/claude` facade is a
    // LOCAL, credential-free surface like `/status`, so serve mounts its pure introspective face
    // unconditionally and wires the live read facet only when the operator opted in
    // (QFS_CLAUDE_SESSIONS names the real `~/.claude` store) — an ENDPOINT over
    // `/hosts/local/claude/sessions` then serves one row per live session; unconfigured it stays
    // the structured unregistered-source error (fail-closed). Path canon (owner ruling
    // 2026-07-16): the surface is canonical under the hosts realm; bare `/claude/...` is retired.
    if let Err(e) = engine
        .mounts
        .register(Arc::new(qfs_driver_claude::ClaudeDriver::new()))
    {
        tracing::warn!(target: "qfs::serve", error = %e, "could not register the /claude mount");
    }
    engine.mounts.require_host_realm("/claude");
    if let Some(source) = crate::claude::ClaudeStoreSource::open_default() {
        reads.register(
            qfs_core::DriverId::new("claude"),
            Arc::new(crate::claude::ClaudeReadDriver::new(Arc::new(source))),
        );
    }

    // Blueprint §16 "The face, named": the shared LIVE ServerState lock is created FIRST so the
    // three legs share one truth — the /server read facet (mounted into THIS engine + registry),
    // the statement-bridge commit path (ServeMcpEngine's live seam below), and the Runtime the
    // boot replay mutates (serve_config_shared / Runtime::with_shared). Serve-side only: the
    // CLI's offline engine never mounts /server (which keeps host-not-serving honest).
    let server_state = Arc::new(std::sync::RwLock::new(qfs_provision::ServerState::new()));
    let (reconf_handle, reconf_rx) = qfs_http::reconfigure_channel(Arc::clone(&server_state));
    crate::server_face::register_server_face(&mut engine, &mut reads, &server_state);

    // The `/collections/<view>` read-by-path surface (mission
    // `a-file-collection-is-a-declared-set-over-any-blob-source`): a registered collection view
    // (`CREATE VIEW <name> AS /local/... |> decode md.<relation>`, a `/server/views` row) is
    // reachable by path — a live SELECT + DESCRIBE reach the declared `documents`/`links` set the
    // way the compiled `/markdown` mount did, applying the root-relative strip (Ruling 3). Wired over
    // the LIVE ServerState (resolved lazily per request, so a view registered by boot replay/reconcile
    // is reachable with no restart) and the daemon's `/local` working tree.
    reads = crate::collection_mount::register_collection_mounts(
        &mut engine,
        reads,
        Arc::new(crate::collection_mount::ViewSource::Live(Arc::clone(
            &server_state,
        ))),
        qfs_driver_local::Sandbox::new(crate::collection_mount::serve_local_root()),
    );

    let engine = Arc::new(engine);
    let reads = Arc::new(reads);

    // The bind address: loopback by default (blueprint §8 trusted bind), overridable via env.
    let addr = match resolve_bind_addr() {
        Ok(addr) => addr,
        Err(e) => {
            eprintln!("qfs serve: invalid bind address: {e}");
            return 1;
        }
    };

    // t36: formalize the EC2/Linux daemon under the `RuntimeHost` seam. We boot the config into a
    // ServerState (the same in-memory parse→lower→COMMIT path serve uses), derive the
    // host-agnostic BindingSet, build the TokioHost (fsync'd durable store + on-disk audit ledger
    // under a project-local state dir), and attach the causes through the trait — REUSING (not
    // rebuilding) the qfs-http/qfs-cron/qfs-watchtower composition wired below. A host-setup
    // failure is non-fatal: the legacy in-memory composition still serves (the durable ledger is
    // an additive observability layer at t36; without a host the cron sweeper cannot run — its
    // `last_run` durability and run ledger live on the host — so jobs stay external-scheduler-only
    // in that degraded mode).
    let daemon_host = match attach_daemon_host(config) {
        Ok(host) => Some(Arc::new(host)),
        Err(e) => {
            tracing::warn!(target: "qfs::serve", error = %e, "daemon host setup degraded; serving without the on-disk ledger or the cron sweeper");
            None
        }
    };

    // Mission `a-request-resolves-to-a-principal` (item 8): resolve the request principal from the
    // `qfs_session` cookie on the serve face. The binary OWNS the System-DB session store, so it
    // builds the resolver closure here and injects it into the HTTP binding (the SAME closure-seam
    // the watchtower `Fallback` uses) — `qfs-http` gains no session dependency. Fail closed: when no
    // session store opens, no resolver is wired and every request stays anonymous; a cookie that
    // resolves to no LIVE session (absent/malformed/unknown/expired) also stays anonymous. This is
    // the consumption side of the OAuth mint face that already issues the cookie.
    let principal_resolver: Option<qfs_http::PrincipalResolver> =
        match crate::session::open_session_store() {
            Ok(store) => {
                let store: Arc<dyn qfs_session::SessionStore> = Arc::new(store);
                tracing::debug!(target: "qfs::serve", "session store ready; serve resolves the qfs_session cookie to a principal");
                Some(Arc::new(move |req: &qfs_http::HttpRequest| {
                    resolve_principal_from_cookie(req, store.as_ref())
                }))
            }
            Err(e) => {
                tracing::debug!(target: "qfs::serve", error = %e, "session store unavailable; serve resolves every request anonymous");
                None
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
            eprintln!("qfs serve: cannot start runtime: {e}");
            return 1;
        }
    };

    // t65 REVERSED (2026-07-11, ticket 20260711121535): the daemon owns the "when" again. The
    // cron sweeper (crate::sweeper) is spawned below beside the watchtower dispatch loop: a
    // `tokio::time` interval drives the pure `qfs_watchtower::cron::fire_due` decision with the
    // LIVE committer (policy gate + irreversible guard + the real applier), stamping `last_run`
    // durably through the daemon host and recording each firing on the `/server/jobs/<name>/runs`
    // read-back + the audit ledger. The external-scheduler path (`qfs job run` / CF Cron Triggers)
    // remains available — the daemon simply ticks its own jobs too.

    // t34 watchtower composition: build the watchtower binding + shared bus + the webhook ingest
    // fallback the HTTP listener routes `/hooks/...` to. The binding is reconciled by the runtime
    // from /server/{webhooks,triggers}; the dispatch loop drains the bus and fires matching
    // triggers through the injected committer (PREVIEW path), acking only after a successful
    // commit (at-least-once). The watchtower's own fire-audit sink is drained on shutdown.
    let secrets: Arc<dyn qfs_secrets::Secrets> = Arc::new(qfs_secrets::InMemoryStore::new());
    let (wt_binding, wt_rx, wt_bus, wt_fallback, wt_policies) =
        crate::watchtower::build_watchtower(Arc::clone(&secrets));
    let wt_triggers = wt_binding.triggers_handle();
    let wt_audit = Arc::new(qfs_watchtower::AuditSink::new());

    // t48 OAuth-AS composition: load/generate the active ES256 signing key over the System DB
    // (unlocked via QFS_PASSPHRASE) and pre-render the three public discovery documents
    // (`/.well-known/oauth-protected-resource`, `/.well-known/oauth-authorization-server`,
    // `/jwks.json`) for the listener's issuer. As of t49 it also serves the live auth-code + PKCE
    // flow + token endpoint; as of t50 it yields the bearer-validation material that gates `/mcp`.
    // Without a passphrase / System DB the routes are simply not served. The handler is composed into
    // the SAME Fallback seam as `POST /mcp`.
    let oauth_routes = crate::oauth::boot_oauth(addr);

    // t47/t50 MCP composition: build the MCP serving binding (qfs-mcp, the MCP sibling of
    // qfs-http/qfs-cron/qfs-watchtower) over the injected ServeMcpEngine, and compose its pure
    // `POST /mcp` handler into the qfs-http listener via the SAME Fallback seam the watchtower
    // webhook ingest uses — so qfs-mcp serves no HTTP itself and needs no tokio. t50: when the OAuth
    // AS booted, the endpoint is GUARDED by a bearer-validating authorizer (verify the
    // `Authorization: Bearer <jwt>` access token against the JWKS + iss/aud/exp; a missing/invalid/
    // expired token is a 401 with a `WWW-Authenticate: Bearer` challenge pointing at the PRM). When
    // the AS is NOT configured (no passphrase / System DB) the endpoint falls back to the inert
    // allow-all, localhost-only posture (no tokens can be minted OR verified without the AS). The
    // combined fallback tries the OAuth routes, then the MCP path, then the watchtower ingest, then 404.
    let mcp_engine: Arc<dyn qfs_mcp::McpEngine> = Arc::new(
        crate::mcp::ServeMcpEngine::new(Arc::clone(&engine), Arc::clone(&reads))
            // The §16 live /server seam: the bridge's write leg commits into the shared state,
            // notifies the runtime (audit + reconcile), and re-emits the boot config.
            .with_live_server(crate::mcp::LiveServer {
                handle: reconf_handle,
                config_path: config.to_path_buf(),
            }),
    );
    // t51: the dashboard bridge drives the SAME injected engine — clone the handle BEFORE the MCP
    // binding takes ownership of `mcp_engine` below (so both faces share one statement path).
    let dashboard_engine = Arc::clone(&mcp_engine);
    let mcp_binding = Arc::new(match &oauth_routes {
        Some(routes) => {
            let v = routes.mcp_verification();
            let authorizer: Arc<dyn qfs_mcp::McpAuthorizer> =
                Arc::new(crate::mcp::BearerAuthorizer::new(
                    v.jwks.clone(),
                    v.issuer.clone(),
                    v.audience.clone(),
                    &v.prm_url,
                ));
            tracing::info!(target: "qfs::serve", issuer = %v.issuer, audience = %v.audience, "t50 MCP endpoint is bearer-gated (Authorization: Bearer required; 401 + WWW-Authenticate otherwise)");
            qfs_mcp::McpBinding::with_authorizer(mcp_engine, authorizer)
        }
        None => {
            tracing::warn!(target: "qfs::serve", "t50 MCP endpoint UNAUTHENTICATED (no OAuth AS: set QFS_PASSPHRASE + System DB to enable bearer gating); relying on the localhost-only bind");
            qfs_mcp::McpBinding::new(mcp_engine)
        }
    });

    // t51 dashboard shell composition: the embedded SPA + its thin `describe`/`preview` JSON bridge
    // ride the SAME Fallback seam as `POST /mcp`, driving the SAME injected `McpEngine` (so the
    // dashboard and MCP faces share ONE statement-execution path — no second executor). The shell is
    // preview/read only this slice (no commit path; commit cards are t52) and served loopback-only
    // (no session gate yet — t46/t50 wiring is a documented follow-up in `crate::dashboard`).
    // Blueprint §16, the one hardening the amendment adds (fail-closed): a NON-loopback bind with
    // no booted bearer material (no OAuth AS) never serves the commit bridge — a network-reachable,
    // unauthenticated commit face would otherwise let any peer reconfigure the daemon. Logged once
    // at boot; the refusal itself is a structured 403 on every `/api/commit` request.
    let commit_locked = crate::dashboard::commit_bridge_locked(&addr, oauth_routes.is_some());
    if commit_locked {
        tracing::warn!(
            target: "qfs::serve",
            %addr,
            "commit bridge DISABLED: non-loopback bind without bearer material (boot the OAuth AS \
             or bind loopback to enable /api/commit)"
        );
    }

    let combined_fallback: qfs_http::Fallback = {
        let mcp = Arc::clone(&mcp_binding);
        let wt = wt_fallback;
        let oauth = oauth_routes.clone();
        let dashboard = dashboard_engine;
        Arc::new(move |req: &qfs_http::HttpRequest| {
            // The three read-only OAuth discovery routes win first (public, cacheable, no creds).
            if let Some(routes) = &oauth {
                if let Some(resp) = routes.handle(req) {
                    return Some(resp);
                }
            }
            if req.path == qfs_mcp::MCP_PATH {
                return Some(crate::mcp::serve_mcp_request(&mcp, req));
            }
            // The §16 fail-closed rule intercepts the commit bridge BEFORE the dashboard handler:
            // a non-loopback, bearer-less daemon refuses every commit structurally.
            if commit_locked && req.path == crate::dashboard::API_COMMIT {
                return Some(crate::dashboard::commit_bridge_locked_response());
            }
            // The dashboard shell + bridge (`GET /`, `GET /assets/*`, `POST /api/*`). Returns None
            // for any non-dashboard path, so the watchtower webhook ingest still gets its chance.
            if let Some(resp) = crate::dashboard::serve_dashboard(dashboard.as_ref(), req) {
                return Some(resp);
            }
            wt(req)
        })
    };

    let result = rt.block_on(async move {
        // Spawn the watchtower dispatch loop (drains the bus, fires matching triggers, acks on
        // success). It runs until the bus is dropped (shutdown).
        let dispatch = crate::watchtower::spawn_dispatch_loop(
            wt_rx,
            wt_bus,
            wt_triggers,
            Arc::clone(&wt_audit),
            clone_engine(&engine),
            Arc::clone(&wt_policies),
        );
        // Spawn the cron sweeper (blueprint §10): the real-clock interval feeding the pure
        // firing decision through the LIVE committer, over the SAME shared ServerState the
        // boot replay + reconfigure channel mutate (a job created over the network schedules
        // without a restart). Needs the daemon host (durable last_run + run ledger).
        let sweeper = daemon_host.as_ref().map(|host| {
            let committer: Arc<dyn qfs_watchtower::Committer> = Arc::new(
                crate::sweeper::LiveCronCommitter::new(clone_engine(&engine), wt_policies),
            );
            crate::sweeper::spawn_sweeper(Arc::clone(&server_state), committer, Arc::clone(host))
        });
        // t65: the cron binding is gone (the scheduler daemon is retired). The watchtower binding
        // is boxed here as the one `qfs_watchtower::Binding` the runtime reconciles. `/server/jobs`
        // rows still reconcile into the registry (the JOB DEFINITION surface is intact); they are
        // simply no longer fired in-process — an external scheduler invokes `qfs job run`.
        let bindings: Vec<Box<dyn qfs_watchtower::Binding>> = vec![Box::new(wt_binding)];
        // §16: the Runtime shares the SAME live ServerState the read facet scans and the bridge
        // commits into, and its run loop services the reconfigure channel (audit + reconcile
        // after every network /server commit).
        let served = qfs_http::serve_config_shared(
            config,
            engine,
            reads,
            addr,
            SERVE_MAX_ROWS,
            bindings,
            Some(combined_fallback),
            principal_resolver,
            qfs_http::Runtime::with_shared(Arc::clone(&server_state), reconf_rx),
        )
        .await;
        dispatch.abort();
        if let Some(sweeper) = sweeper {
            sweeper.abort();
        }
        served
    });

    match result {
        Ok(()) => 0,
        Err(e) => {
            // The error is already secret-free (boot / bind / runtime); surface it on stderr.
            eprintln!("qfs serve: {e}");
            1
        }
    }
}

/// Clone the serve engine's registries into a fresh `Engine` for the watchtower committer (so a
/// fired-trigger plan resolves against the same mounts/codecs the deployment registered). `Engine`
/// is not `Clone`; we rebuild it from the shared registries.
fn clone_engine(engine: &std::sync::Arc<Engine>) -> Engine {
    let mut fresh = Engine::new();
    fresh.mounts = engine.mounts.clone();
    // CodecRegistry is not Clone; rebuild the same builtin set the serve engine uses.
    fresh.codecs = CodecRegistry::with_builtins();
    fresh
}

/// t36: boot the config into a `ServerState`, derive the host-agnostic `BindingSet`, build the
/// daemon [`crate::host::TokioHost`] over a project-local state dir, and attach the causes through
/// the [`qfs_host::RuntimeHost`] seam (recording the attachment to the on-disk audit ledger). This
/// is the formalization of the existing serve composition under the trait — it boots the config
/// once more (idempotent, in-memory) only to PROJECT the binding set; the listener/interval/bus
/// are wired by the legacy composition below, unchanged. Returns the host: the caller keeps it
/// alive for the daemon's lifetime (the cron sweeper persists `last_run` through its durable
/// store and appends run records to its ledger).
fn attach_daemon_host(config: &Path) -> Result<crate::host::TokioHost, String> {
    use qfs_host::RuntimeHost;

    // The state dir: `QFS_STATE_DIR` if set, else a worktree-local `.qfs-state` (NEVER a system
    // path — system-safety: this is a regular project, the daemon writes only project-local state).
    let state_dir = std::env::var("QFS_STATE_DIR").unwrap_or_else(|_| ".qfs-state".to_string());

    // Boot the config into a BindingSet via qfs-host (the qfs-server coupling lives behind its
    // `host-daemon` feature, so the binary stays the thin entrypoint the dep-direction guard pins).
    let bindings = qfs_host::bindings_from_config(config)?;
    tracing::info!(target: "qfs::serve", summary = %bindings.summary(), "t36 host binding set derived");

    let host = crate::host::TokioHost::open(&state_dir).map_err(|e| format!("host: {e}"))?;
    // Attach the causes through the trait (the futures are non-suspending ledger writes; drive them
    // with qfs-host's tiny block_on so this stays off the tokio runtime built below).
    qfs_host::block_on(host.serve_endpoints(&bindings)).map_err(|e| format!("endpoints: {e}"))?;
    qfs_host::block_on(host.schedule_jobs(&bindings)).map_err(|e| format!("jobs: {e}"))?;
    qfs_host::block_on(host.consume_events(&bindings)).map_err(|e| format!("events: {e}"))?;
    let _ = host.now();
    tracing::info!(target: "qfs::serve", ledger = %host.ledger().path().display(), "t36 daemon host attached (on-disk audit ledger)");
    Ok(host)
}

/// Resolve the acting principal from a request's `qfs_session` cookie via the injected session
/// store (mission item 8, the consumption side of the OAuth mint face). Reads the `Cookie` header,
/// looks the session up ([`qfs_session::authenticate`] — which hashes the token before the lookup),
/// and returns the bound user as a [`qfs_core::RequestContext`]. An absent/malformed/unknown/expired
/// session — and any store read error — resolves to the anonymous actor (fail closed): a cookie
/// that cannot be VERIFIED grants nothing, and "not signed in" stays a first-class answer.
fn resolve_principal_from_cookie(
    req: &qfs_http::HttpRequest,
    store: &dyn qfs_session::SessionStore,
) -> qfs_core::RequestContext {
    let cookie = req.headers.get("cookie").map(String::as_str);
    match qfs_session::authenticate(cookie, store) {
        Ok(Some(user_id)) => qfs_core::RequestContext::for_user(user_id.to_string()),
        _ => qfs_core::RequestContext::anonymous(),
    }
}

/// Resolve the HTTP bind address: `QFS_HTTP_ADDR` if set, else the loopback default.
fn resolve_bind_addr() -> Result<std::net::SocketAddr, String> {
    let raw =
        std::env::var("QFS_HTTP_ADDR").unwrap_or_else(|_| qfs_http::DEFAULT_BIND_ADDR.to_string());
    raw.parse().map_err(|e| format!("{raw}: {e}"))
}

#[cfg(test)]
mod tests {
    //! Hermetic proof of mission `a-request-resolves-to-a-principal` item 8 (no TCP, no network, no
    //! credentials): the serve face registers the `/sys/whoami` facet and the request principal
    //! resolves from the `qfs_session` cookie. Mirrors the un-shipped pieces the in-container live
    //! round (ticket 20260719101204) could not prove: `AS /sys/whoami` was refused at registration
    //! with `UnroutedPath` and the handler ignored the cookie.
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;
    use qfs_session::{SessionStore, UserId};
    use qfs_store::{MemorySource, SqliteSessionStore, SystemDb};

    /// The serve-side engine + read registry with the credential-free `/sys` facet wired exactly as
    /// [`run_serve`] wires it (describe/plan `SysDriver` mount + the whoami-only `AnonymousSysBackend`
    /// read facet — the always-available, no-System-DB path). Codecs are registered so the handler
    /// can encode the JSON envelope.
    fn serve_engine_and_reads() -> (Arc<Engine>, Arc<ReadRegistry>) {
        let mut engine = Engine::new();
        engine.codecs = CodecRegistry::with_builtins();
        let mut reads = ReadRegistry::new();
        crate::serve_builtins::register_builtins(&mut engine, &mut reads);
        engine
            .mounts
            .register(Arc::new(qfs_driver_sys::SysDriver::new()))
            .unwrap();
        reads.register(
            qfs_core::DriverId::new("sys"),
            Arc::new(crate::sys::SysReadDriver::new(Arc::new(
                crate::sys::AnonymousSysBackend::new(),
            ))),
        );
        (Arc::new(engine), Arc::new(reads))
    }

    /// The `GET /whoami AS /sys/whoami` endpoint, stored as the canonical `StatementSpec` exactly as
    /// the DDL desugar does.
    fn whoami_endpoint() -> qfs_http::EndpointDef {
        let stmt = qfs_exec::parse("/sys/whoami").expect("/sys/whoami parses");
        let spec = qfs_core::StatementSpec::from_statement(stmt);
        qfs_http::EndpointDef {
            name: "whoami".to_string(),
            method: "GET".to_string(),
            route: "/whoami".to_string(),
            query: qfs_http::StatementSource::new(spec.canonical()),
            policy: None,
        }
    }

    /// Dispatch a `GET /whoami` through the SHIPPED serve pipeline (compile → router → handler),
    /// with the given injected resolver and `Cookie` header. Returns the encoded response.
    fn dispatch_whoami(
        engine: &Arc<Engine>,
        reads: &Arc<ReadRegistry>,
        resolver: Option<qfs_http::PrincipalResolver>,
        cookie: Option<&str>,
    ) -> qfs_http::HttpResponse {
        // The endpoint MUST compile — this is the exact `UnroutedPath` check the live round tripped.
        let route = qfs_http::compile_endpoint(&whoami_endpoint(), engine, None)
            .expect("AS /sys/whoami registers over the serve face (no UnroutedPath)");
        let router = qfs_http::Router::from_routes(vec![route]);
        let binding = {
            let b = qfs_http::HttpBinding::new(Arc::clone(engine), Arc::clone(reads), 10_000);
            match resolver {
                Some(r) => b.with_principal_resolver(r),
                None => b,
            }
        };
        let ctx = binding.ctx();
        let mut req = qfs_http::HttpRequest::new(qfs_http::Method::Get, "/whoami");
        if let Some(c) = cookie {
            req = req.with_header("cookie", c);
        }
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            match router.match_request(&req.method, &req.path) {
                Some((route, path_params)) => {
                    qfs_http::dispatch(route, path_params, &req, &ctx).await
                }
                None => panic!("/whoami must match the compiled route"),
            }
        })
    }

    /// A session store over a fresh in-memory System DB holding one user (a session FKs to `users`),
    /// plus the issued `Cookie` header value (`qfs_session=<token>`) for that user.
    fn store_user_and_cookie() -> (Arc<dyn SessionStore>, UserId, String) {
        let sys = SystemDb::open(&MemorySource).unwrap();
        let conn = sys.into_db().into_connection();
        conn.execute("INSERT INTO users (primary_email) VALUES ('a@b.com')", [])
            .unwrap();
        let uid = UserId(conn.last_insert_rowid());
        let store = SqliteSessionStore::new(conn);
        let (_session, set_cookie) = crate::session::issue_session(&store, uid, false).unwrap();
        // The browser echoes the `name=value` pair back on the next request.
        let cookie = set_cookie.split(';').next().unwrap().to_string();
        (Arc::new(store), uid, cookie)
    }

    /// Build the production resolver closure over a store (the same one `run_serve` injects).
    fn resolver_over(store: Arc<dyn SessionStore>) -> qfs_http::PrincipalResolver {
        Arc::new(move |req: &qfs_http::HttpRequest| {
            resolve_principal_from_cookie(req, store.as_ref())
        })
    }

    // ----- Quality Gate 1: the /sys facet registers + serves whoami over the serve face -----

    #[test]
    fn serve_registers_sys_whoami_and_returns_the_not_signed_in_row() {
        let (engine, reads) = serve_engine_and_reads();
        // Registration proof: the endpoint compiles (no `UnroutedPath`).
        assert!(
            qfs_http::compile_endpoint(&whoami_endpoint(), &engine, None).is_ok(),
            "AS /sys/whoami must register over the serve face (no UnroutedPath)"
        );
        // GET with no resolver → the anonymous first-class row, 200 (not 404).
        let resp = dispatch_whoami(&engine, &reads, None, None);
        assert_eq!(resp.status, 200, "GET /whoami is 200, not 404");
        let body = resp.body_text();
        assert!(body.contains("\"signed_in\":false"), "body: {body}");
        assert!(body.contains("\"user\":null"), "body: {body}");
    }

    // ----- Quality Gate 2: the qfs_session cookie resolves to its principal (fail closed) -----

    #[test]
    fn valid_session_cookie_resolves_to_its_user() {
        let (store, uid, cookie) = store_user_and_cookie();
        let req = qfs_http::HttpRequest::new(qfs_http::Method::Get, "/whoami")
            .with_header("cookie", cookie);
        let ctx = resolve_principal_from_cookie(&req, store.as_ref());
        assert_eq!(ctx.user(), Some(uid.to_string().as_str()));
    }

    #[test]
    fn absent_or_invalid_session_cookie_resolves_anonymous() {
        let (store, _uid, _cookie) = store_user_and_cookie();
        // No cookie at all.
        let none = qfs_http::HttpRequest::new(qfs_http::Method::Get, "/whoami");
        assert_eq!(
            resolve_principal_from_cookie(&none, store.as_ref()).user(),
            None
        );
        // A well-formed cookie whose token was never issued (unknown session).
        let unknown = qfs_http::HttpRequest::new(qfs_http::Method::Get, "/whoami")
            .with_header("cookie", "qfs_session=never-issued-token");
        assert_eq!(
            resolve_principal_from_cookie(&unknown, store.as_ref()).user(),
            None
        );
        // A malformed cookie header (no qfs_session value).
        let malformed = qfs_http::HttpRequest::new(qfs_http::Method::Get, "/whoami")
            .with_header("cookie", "theme=dark");
        assert_eq!(
            resolve_principal_from_cookie(&malformed, store.as_ref()).user(),
            None
        );
    }

    #[test]
    fn whoami_over_serve_reflects_the_session_resolved_principal_end_to_end() {
        let (engine, reads) = serve_engine_and_reads();
        let (store, uid, cookie) = store_user_and_cookie();
        let resolver = resolver_over(Arc::clone(&store));

        // A request CARRYING the valid cookie → signed_in=true, user=<uid>.
        let signed_in =
            dispatch_whoami(&engine, &reads, Some(Arc::clone(&resolver)), Some(&cookie));
        assert_eq!(signed_in.status, 200);
        let body = signed_in.body_text();
        assert!(body.contains("\"signed_in\":true"), "body: {body}");
        assert!(
            body.contains(&format!("\"user\":\"{uid}\"")),
            "body: {body} (expected user {uid})"
        );

        // The SAME wired serve, no cookie → anonymous first-class row (fail closed).
        let anon = dispatch_whoami(&engine, &reads, Some(resolver), None);
        let body = anon.body_text();
        assert!(body.contains("\"signed_in\":false"), "body: {body}");
        assert!(body.contains("\"user\":null"), "body: {body}");
    }
}
