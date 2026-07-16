//! `qfs plan` / `qfs apply` — the provisioning reconcile surface (blueprint §16, Decision X).
//!
//! This is the binary composition root for the pure [`qfs_provision`] core: it owns the two
//! things the core cannot — the **current-config fetch** and the **commit I/O**.
//!
//! - The `/sys` half is read/written **directly** (the CLI has System/Project DB access): the
//!   current `/sys` config comes from the same tables `qfs dump` reads, and the reconcile commit
//!   runs through the binary-owned [`crate::sys::SystemDbBackend`].
//! - The `/server` half rides the **running daemon's statement bridge** (§16 "The face, named"):
//!   `POST /api/run` with `mode: "read"` fetches each `/server/<collection>` as the §14 result
//!   envelope, and `POST /api/commit` applies the batch's CREATE≡INSERT twin statements
//!   **statement-by-statement in plan order** (the boot-replay shape) — through the same single
//!   `McpEngine` path MCP and the dashboard drive, gated by the same three controls (face gate,
//!   default-deny policy grant on the `server` driver, irreversible ack). A document that touches
//!   `/server` therefore requires a live daemon; with none reachable it is a structured
//!   **host-not-serving** refusal — never an empty current state (which would read as "destroy
//!   every binding"). A `/sys`-only document never engages `/server` at all (store-scoping by
//!   document content), so it plans + applies with no daemon.
//!
//! A partial `/server` apply is per-statement (§7 cross-source honesty): the error names the
//! statement that refused/failed, everything before it is committed, and a re-plan converges —
//! the reconcile loop is its own recovery tool.
//!
//! After a committed `/server` batch the **daemon** re-emits its post-commit `ServerState` to its
//! boot-config path ([`reemit_boot_config`], atomic temp-then-rename, called by the serve
//! composition's commit path) so an applied reconcile survives a restart.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::Path;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use qfs_cmd::{ApplyAction, PlanAction};
use qfs_core::preview;
use qfs_driver_sys::{SysApplier, SysBackend};
use qfs_provision::{
    build_plan, diff, load, server_op_statement, ConfigState, DdlEventHead, EndpointDef,
    GenerationStamp, JobDef, PathBindingRow, PolicyDef, ReconcileApplier, ReconcilePlan,
    ServerState, StatementSource, SysDriverRow, SysPolicyRow, SysState, TransformRow, TriggerDef,
    ViewDef, WebhookDef,
};

/// The bridge connect/read timeout — a loopback round-trip resolves in milliseconds; this is the
/// ceiling before we call the host not-serving / the bridge unresponsive.
const BRIDGE_TIMEOUT: Duration = Duration::from_secs(10);

/// A structured, secret-free failure of a plan/apply. Each maps to a CLI exit code + stderr line.
#[derive(Debug)]
pub enum ProvisionError {
    /// The document could not be read.
    ReadDocument {
        /// The path that failed.
        path: String,
        /// The OS error text.
        detail: String,
    },
    /// The document failed to load into a desired [`ConfigState`].
    Load(qfs_provision::LoadError),
    /// No config home resolved (HOME / XDG unset) — the `/sys` store is unreachable.
    NoConfigHome,
    /// A System/Project DB read failed.
    Backend(String),
    /// The document touches `/server` but no daemon is reachable (§16 transport ruling): a
    /// structured refusal, never an empty current `/server` state.
    HostNotServing {
        /// The loopback address probed.
        addr: String,
    },
    /// The document's generation stamp has moved from the live one and `--allow-stale-base` was
    /// not passed (a base fetched-then-edited while the deployment changed under it).
    StaleBase,
    /// The plan contains an authoritative destroy and `--commit-irreversible` was not passed.
    NeedsIrreversibleAck {
        /// How many destroys the plan carries.
        destroys: usize,
    },
    /// The reconcile commit failed partway (secret-free reason). `CommitReport.applied` lets it
    /// be re-run (idempotent).
    Commit(String),
}

impl std::fmt::Display for ProvisionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ReadDocument { path, detail } => write!(f, "reading {path}: {detail}"),
            Self::Load(e) => write!(f, "{e}"),
            Self::NoConfigHome => write!(
                f,
                "cannot determine the system database path (set HOME or XDG_CONFIG_HOME)"
            ),
            Self::Backend(d) => write!(f, "{d}"),
            Self::HostNotServing { addr } => write!(
                f,
                "the document configures /server but no qfs daemon is serving at {addr}: \
                 start `qfs serve <config>` first (refusing to treat an unreachable host as an \
                 empty /server state)"
            ),
            Self::StaleBase => write!(
                f,
                "the document's generation stamp does not match the live configuration (the base \
                 moved since it was fetched); re-fetch, or pass --allow-stale-base to override"
            ),
            Self::NeedsIrreversibleAck { destroys } => write!(
                f,
                "the plan contains {destroys} destroy(s); re-run with --commit-irreversible to \
                 apply (a destroy is irreversible)"
            ),
            Self::Commit(d) => write!(f, "apply failed: {d}"),
        }
    }
}

impl From<qfs_provision::LoadError> for ProvisionError {
    fn from(e: qfs_provision::LoadError) -> Self {
        Self::Load(e)
    }
}

/// Run the injected `qfs plan` command. Exit codes (the Terraform `-detailed-exitcode`
/// convention): `0` no changes, `2` changes pending, `1` error.
#[must_use]
pub fn run_plan(action: &PlanAction) -> i32 {
    let addr = resolve_bind_addr();
    match plan_document(&read_document(&action.document), addr) {
        Ok(report) => {
            report.render(action.json);
            if report.plan.is_empty() {
                0
            } else {
                2
            }
        }
        Err(ProvisionError::ReadDocument { path, detail }) => {
            eprintln!("qfs: error: reading {path}: {detail}");
            1
        }
        Err(e) => {
            eprintln!("qfs: error: {e}");
            1
        }
    }
}

/// Run the injected `qfs apply` command. Exit `0` on success (including a no-op), `1` on any
/// refusal or failure.
#[must_use]
pub fn run_apply(action: &ApplyAction) -> i32 {
    let addr = resolve_bind_addr();
    match apply_document(&read_document(&action.document), action, addr) {
        Ok(report) => {
            report.render(action.json);
            0
        }
        Err(e) => {
            eprintln!("qfs: error: {e}");
            1
        }
    }
}

// ---------------------------------------------------------------------------
// The plan / apply core (I/O-owning, but driven by pure qfs-provision)
// ---------------------------------------------------------------------------

/// The result of a `plan`: the diff plus the base-moved flag and the effect preview.
#[derive(Debug)]
pub struct PlanReport {
    /// The reconcile plan (add/change/destroy).
    pub plan: ReconcilePlan,
    /// Whether the document's generation stamp differs from the live one.
    pub base_moved: bool,
}

impl PlanReport {
    fn render(&self, json: bool) {
        if json {
            println!(
                "{{\"add\":{},\"change\":{},\"destroy\":{},\"base_moved\":{}}}",
                self.plan.add_count(),
                self.plan.change_count(),
                self.plan.destroy_count(),
                self.base_moved,
            );
            return;
        }
        if let Ok(built) = build_plan(&self.plan) {
            println!("{}", preview(&built));
        }
        if self.base_moved {
            println!("  (!) base moved: the document's generation stamp differs from the live one");
        }
        println!(
            "Plan: {} to add, {} to change, {} to destroy.",
            self.plan.add_count(),
            self.plan.change_count(),
            self.plan.destroy_count(),
        );
        if self.plan.is_empty() {
            println!("No changes. The live configuration matches the document.");
        }
    }
}

/// The result of an `apply`: how many effects landed.
#[derive(Debug)]
pub struct ApplyReport {
    applied: usize,
    add: usize,
    change: usize,
    destroy: usize,
}

impl ApplyReport {
    fn render(&self, json: bool) {
        if json {
            println!(
                "{{\"applied\":{},\"add\":{},\"change\":{},\"destroy\":{}}}",
                self.applied, self.add, self.change, self.destroy
            );
            return;
        }
        if self.applied == 0 {
            println!("qfs: apply: no changes. The live configuration already matches.");
        } else {
            println!(
                "qfs: apply committed: {} effect(s) — {} added, {} changed, {} destroyed.",
                self.applied, self.add, self.change, self.destroy,
            );
        }
    }
}

/// Compute the reconcile plan of `document` vs the live configuration. Pure w.r.t. the live
/// state (reads only). `addr` is the daemon probe target for the `/server` half.
///
/// # Errors
/// [`ProvisionError`] on a load, fetch, or host-not-serving condition.
pub fn plan_document(
    document: &Result<String, ProvisionError>,
    addr: SocketAddr,
) -> Result<PlanReport, ProvisionError> {
    let document = document.as_ref().map_err(clone_err)?;
    let desired = load(document)?;
    let (current, live_stamp) = fetch_current(&desired, addr)?;
    let plan = diff(&current, &desired);
    let base_moved = base_moved(document, &live_stamp);
    Ok(PlanReport { plan, base_moved })
}

/// Apply `document` to the live configuration through the dispatching applier.
///
/// # Errors
/// [`ProvisionError`] on load/fetch/host-not-serving, a stale base without the override, a
/// destroy without the ack, or a commit failure.
pub fn apply_document(
    document: &Result<String, ProvisionError>,
    action: &ApplyAction,
    addr: SocketAddr,
) -> Result<ApplyReport, ProvisionError> {
    let document = document.as_ref().map_err(clone_err)?;
    let desired = load(document)?;
    let (current, live_stamp) = fetch_current(&desired, addr)?;

    // Gate 1 (stale base): three independent controls; this one is distinct from the ack.
    if base_moved(document, &live_stamp) && !action.allow_stale_base {
        return Err(ProvisionError::StaleBase);
    }

    let plan = diff(&current, &desired);

    // Gate 2 (irreversible): any authoritative destroy requires the ack.
    if plan.has_destroy() && !action.commit_irreversible {
        return Err(ProvisionError::NeedsIrreversibleAck {
            destroys: plan.destroy_count(),
        });
    }

    let (add, change, destroy) = (plan.add_count(), plan.change_count(), plan.destroy_count());
    if plan.is_empty() {
        return Ok(ApplyReport {
            applied: 0,
            add,
            change,
            destroy,
        });
    }

    // The two store halves apply through their two transports, in the plan's deterministic
    // order (§16): /sys first through the local dispatching applier, then /server
    // statement-by-statement through the daemon's statement bridge.
    let (sys_plan, server_plan) = plan.split_stores();
    let mut applied = 0usize;

    if !sys_plan.is_empty() {
        let batch = build_plan(&sys_plan).map_err(ProvisionError::Commit)?;
        let backend: Arc<dyn SysBackend> = Arc::new(
            crate::sys::SystemDbBackend::open_default().ok_or(ProvisionError::NoConfigHome)?,
        );
        // The /server half never rides this batch (split above); a fresh ServerState satisfies
        // the ReconcileApplier's handle. The binary is the terminal leaf that injects the
        // concrete SysApplier over the real backend.
        let server = Arc::new(RwLock::new(ServerState::new()));
        let mut applier = ReconcileApplier::new(&server, SysApplier::new(backend));
        let report = qfs_core::commit(&batch, &mut applier, |_| {});
        if let Some(err) = report.failed {
            return Err(ProvisionError::Commit(err.reason));
        }
        applied += report.applied.len();
    }

    // The /server half: the CREATE≡INSERT twin statements, one bridge commit each, in plan
    // order (the boot-replay shape). A refusal/failure is per-statement — everything before it
    // is committed, and a re-plan converges (§7 idempotency as recovery).
    for op in server_plan.ops() {
        let statement = server_op_statement(op).map_err(ProvisionError::Commit)?;
        // The explicit irreversible ack rides ONLY on a destroy (the CLI gate above already
        // required the flag when any destroy is present).
        let ack = op.op == qfs_core::ServerWriteOp::Remove && action.commit_irreversible;
        bridge_commit(addr, &statement, ack)?;
        applied += 1;
    }

    Ok(ApplyReport {
        applied,
        add,
        change,
        destroy,
    })
}

/// Fetch the live current [`ConfigState`] + generation stamp. The `/sys` half is read directly;
/// the `/server` half is engaged **only** when `desired` declares `/server` rows (store-scoping),
/// and is then read from the daemon's statement bridge — no daemon ⇒ a structured
/// host-not-serving refusal, never an empty current state.
fn fetch_current(
    desired: &ConfigState,
    addr: SocketAddr,
) -> Result<(ConfigState, GenerationStamp), ProvisionError> {
    let (sys, stamp) = fetch_sys_state()?;
    let server = if desired.server.row_count() > 0 {
        fetch_server_state(addr)?
    } else {
        // /sys-only reconcile: /server is out of scope — never fetched, never diffed, never
        // destroyed (a document manages the stores it names).
        ServerState::new()
    };
    Ok((ConfigState { server, sys }, stamp))
}

/// Whether the document carries a generation stamp that differs from the live one. A document
/// with no stamp (hand-written) is never "moved" — there is nothing to compare.
fn base_moved(document: &str, live: &GenerationStamp) -> bool {
    match GenerationStamp::parse_from_document(document) {
        Some(doc_stamp) => &doc_stamp != live,
        None => false,
    }
}

// ---------------------------------------------------------------------------
// The statement-bridge transport (§16 "The face, named"): the reconcile CLI is the bridge's
// third client — POST /api/run (mode: read) for the current /server state, POST /api/commit
// for the apply, one HTTP/1.1 request per connection (the listener's serving model).
// ---------------------------------------------------------------------------

/// Read the daemon's current `/server` configuration through the statement bridge: one
/// `mode: "read"` run per collection, decoded from the §14 result envelope into the
/// [`ServerState`] shape the differ expects. A connect failure is the host-not-serving refusal.
fn fetch_server_state(addr: SocketAddr) -> Result<ServerState, ProvisionError> {
    let mut state = ServerState::new();
    for segment in [
        "endpoints",
        "triggers",
        "jobs",
        "views",
        "policies",
        "webhooks",
    ] {
        let body = serde_json::json!({
            "statement": format!("/server/{segment}"),
            "mode": "read",
        });
        let envelope = bridge_post(addr, "/api/run", &body)?;
        // An error body (or a body with no `rows`) is a fetch FAILURE — never an empty current
        // state (which would read as "destroy every binding").
        if let Some(err) = envelope.get("error") {
            return Err(ProvisionError::Backend(format!(
                "reading /server/{segment} through the statement bridge failed: {err}"
            )));
        }
        let Some(rows) = envelope.get("rows").and_then(|r| r.as_array()) else {
            return Err(ProvisionError::Backend(format!(
                "the statement bridge returned no rows envelope for /server/{segment}"
            )));
        };
        decode_server_rows(&mut state, segment, rows);
    }
    Ok(state)
}

/// Submit one reconcile statement through the gated commit bridge. Maps the bridge's structured
/// outcomes: an `applied` commit is success; a policy refusal / held approval / engine error is a
/// per-statement [`ProvisionError::Commit`] naming the statement (a re-plan converges, §7).
fn bridge_commit(addr: SocketAddr, statement: &str, ack: bool) -> Result<(), ProvisionError> {
    let body = serde_json::json!({ "statement": statement, "ack": ack });
    let resp = bridge_post(addr, "/api/commit", &body)?;
    if resp.get("applied").and_then(serde_json::Value::as_bool) == Some(true) {
        return Ok(());
    }
    let detail = if let Some(refused) = resp.get("refused").and_then(serde_json::Value::as_str) {
        let reason = resp
            .get("reason")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("(no reason)");
        format!("{refused}: {reason}")
    } else if let Some(err) = resp.get("error") {
        let code = err
            .get("code")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("error");
        let message = err
            .get("message")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("(no message)");
        format!("{code}: {message}")
    } else {
        "unexpected bridge response".to_string()
    };
    Err(ProvisionError::Commit(format!(
        "statement `{statement}` was not applied — {detail}"
    )))
}

/// One `POST` to the daemon's statement bridge: a minimal HTTP/1.1 exchange over a loopback
/// [`TcpStream`] (one request per connection — the listener's model), returning the parsed JSON
/// body. A connect failure maps to [`ProvisionError::HostNotServing`] (§16: an unreachable host
/// is a refusal, never an empty state).
fn bridge_post(
    addr: SocketAddr,
    path: &str,
    body: &serde_json::Value,
) -> Result<serde_json::Value, ProvisionError> {
    let mut stream = TcpStream::connect_timeout(&addr, BRIDGE_TIMEOUT).map_err(|_| {
        ProvisionError::HostNotServing {
            addr: addr.to_string(),
        }
    })?;
    stream.set_read_timeout(Some(BRIDGE_TIMEOUT)).ok();
    stream.set_write_timeout(Some(BRIDGE_TIMEOUT)).ok();
    let payload = body.to_string();
    let request = format!(
        "POST {path} HTTP/1.1\r\nHost: {addr}\r\nContent-Type: application/json\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{payload}",
        payload.len()
    );
    stream
        .write_all(request.as_bytes())
        .map_err(|e| ProvisionError::Backend(format!("writing to the statement bridge: {e}")))?;
    let mut raw = Vec::new();
    stream
        .read_to_end(&mut raw)
        .map_err(|e| ProvisionError::Backend(format!("reading from the statement bridge: {e}")))?;
    let text = String::from_utf8_lossy(&raw);
    let body_start = text.find("\r\n\r\n").map(|i| i + 4).unwrap_or(text.len());
    let json_body = &text[body_start..];
    serde_json::from_str(json_body).map_err(|e| {
        ProvisionError::Backend(format!(
            "the statement bridge returned a non-JSON body ({e}); is this a qfs daemon?"
        ))
    })
}

// -- §14 envelope → ServerState decode (config projection; runtime fields ignored) ----------

/// A row object's text column, `""` when null/absent (the empty-body convention).
fn row_text(row: &serde_json::Value, col: &str) -> String {
    row.get(col)
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_string()
}

/// A row object's optional text column (`None` when null/absent/empty).
fn row_opt(row: &serde_json::Value, col: &str) -> Option<String> {
    let s = row_text(row, col);
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

/// Decode one collection's envelope rows into `state`. Runtime fields (`last_run`, cache) are
/// deliberately NOT decoded — the reconcile compares config projections only (blueprint §16).
fn decode_server_rows(state: &mut ServerState, segment: &str, rows: &[serde_json::Value]) {
    for row in rows {
        let name = row_text(row, "name");
        if name.is_empty() {
            continue;
        }
        match segment {
            "endpoints" => {
                state.endpoints.insert(
                    name.clone(),
                    EndpointDef {
                        name,
                        method: row_text(row, "method"),
                        route: row_text(row, "route"),
                        query: StatementSource::new(row_text(row, "query")),
                        policy: row_opt(row, "policy"),
                    },
                );
            }
            "triggers" => {
                state.triggers.insert(
                    name.clone(),
                    TriggerDef {
                        name,
                        on: row_text(row, "on"),
                        predicate: StatementSource::new(row_text(row, "predicate")),
                        plan: StatementSource::new(row_text(row, "plan")),
                        policy: row_opt(row, "policy"),
                    },
                );
            }
            "jobs" => {
                state.jobs.insert(
                    name.clone(),
                    JobDef {
                        name,
                        every: row_text(row, "every"),
                        plan: StatementSource::new(row_text(row, "plan")),
                        policy: row_opt(row, "policy"),
                        last_run: None,
                    },
                );
            }
            "views" => {
                state.views.insert(
                    name.clone(),
                    ViewDef {
                        name,
                        query: StatementSource::new(row_text(row, "query")),
                        materialized: row
                            .get("materialized")
                            .and_then(serde_json::Value::as_bool)
                            .unwrap_or(false),
                        last_run: None,
                        cache_json: None,
                    },
                );
            }
            "policies" => {
                let allow = row
                    .get("allow")
                    .and_then(|v| v.as_array())
                    .map(|items| {
                        items
                            .iter()
                            .filter_map(|v| v.as_str().map(str::to_string))
                            .collect()
                    })
                    .unwrap_or_default();
                state.policies.insert(
                    name.clone(),
                    PolicyDef {
                        name,
                        handler: row_text(row, "handler"),
                        allow,
                    },
                );
            }
            "webhooks" => {
                state.webhooks.insert(
                    name.clone(),
                    WebhookDef {
                        name,
                        route: row_text(row, "route"),
                        secret: row_text(row, "secret"),
                    },
                );
            }
            _ => {}
        }
    }
}

/// Resolve the daemon bind address the CLI probes (`QFS_HTTP_ADDR`, else the loopback default).
fn resolve_bind_addr() -> SocketAddr {
    let raw =
        std::env::var("QFS_HTTP_ADDR").unwrap_or_else(|_| qfs_http::DEFAULT_BIND_ADDR.to_string());
    raw.parse().unwrap_or_else(|_| {
        qfs_http::DEFAULT_BIND_ADDR
            .parse()
            .unwrap_or_else(|_| SocketAddr::from(([127, 0, 0, 1], 8787)))
    })
}

/// Read the document file (or `-` for stdin), producing a [`ProvisionError`] on failure. Kept as
/// a `Result` so the pure planners can borrow it without re-reading.
fn read_document(path: &str) -> Result<String, ProvisionError> {
    if path == "-" {
        let mut buf = String::new();
        std::io::Read::read_to_string(&mut std::io::stdin(), &mut buf).map_err(|e| {
            ProvisionError::ReadDocument {
                path: "<stdin>".to_string(),
                detail: e.to_string(),
            }
        })?;
        return Ok(buf);
    }
    std::fs::read_to_string(path).map_err(|e| ProvisionError::ReadDocument {
        path: path.to_string(),
        detail: e.to_string(),
    })
}

/// Clone a borrowed [`ProvisionError`] into an owned one (the read result is shared by ref).
fn clone_err(e: &ProvisionError) -> ProvisionError {
    ProvisionError::Backend(e.to_string())
}

// ---------------------------------------------------------------------------
// /sys current-state fetch (direct DB reads — the material `qfs dump` emits)
// ---------------------------------------------------------------------------

/// Read the current `/sys` config projection + the live generation stamp from the System/Project
/// DBs. Mirrors `qfs dump`'s record set minus the exclusions: secretish settings are filtered
/// (never fetched), billing / `sys_ddl_events` are outside the universe entirely.
///
/// # Errors
/// [`ProvisionError::NoConfigHome`] when no DB path resolves, [`ProvisionError::Backend`] on I/O.
pub fn fetch_sys_state() -> Result<(SysState, GenerationStamp), ProvisionError> {
    let system = crate::store::open_system_db()
        .map_err(|e| ProvisionError::Backend(format!("opening the system database: {e}")))?
        .ok_or(ProvisionError::NoConfigHome)?;
    let sys_migrations = qfs_store::applied_migrations(system.db())
        .map_err(|e| ProvisionError::Backend(format!("reading migrations: {e}")))?
        .len();
    let conn = system.db().conn();

    let project = crate::store::open_project_db()
        .map_err(|e| ProvisionError::Backend(format!("opening the project database: {e}")))?;
    let project_migrations = project
        .as_ref()
        .map(|p| qfs_store::applied_migrations(p.db()).map(|m| m.len()))
        .transpose()
        .map_err(|e| ProvisionError::Backend(format!("reading project migrations: {e}")))?;

    let mut sys = SysState::default();
    read_drivers(conn, &mut sys)?;
    read_policies(conn, &mut sys)?;
    read_settings(conn, &mut sys)?;
    read_transforms(conn, &mut sys)?;
    // The binding registry lives in the System DB (re-homed by 20260716143641); the Project DB
    // stays open above only for its migration count in the generation stamp.
    read_bindings(conn, &mut sys)?;

    let ddl_event_head = conn
        .query_row(
            "SELECT seq, hash FROM sys_ddl_events ORDER BY seq DESC LIMIT 1",
            [],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)),
        )
        .ok()
        .map(|(seq, hash)| DdlEventHead { seq, hash });

    let stamp = GenerationStamp {
        system_migrations: sys_migrations,
        project_migrations,
        ddl_event_head,
    };
    Ok((sys, stamp))
}

fn read_drivers(conn: &rusqlite::Connection, sys: &mut SysState) -> Result<(), ProvisionError> {
    let mut stmt = conn
        .prepare(
            "SELECT kind, name, base_url, auth, pagination, of_type, verb, body, irreversible \
             FROM sys_drivers ORDER BY name",
        )
        .map_err(be)?;
    let rows = stmt
        .query_map([], |r| {
            Ok(SysDriverRow {
                kind: r.get(0)?,
                name: r.get(1)?,
                base_url: r.get(2)?,
                auth: r.get(3)?,
                pagination: r.get(4)?,
                of_type: r.get(5)?,
                verb: r.get(6)?,
                body: r.get(7)?,
                irreversible: r.get::<_, i64>(8)? != 0,
            })
        })
        .map_err(be)?;
    for row in rows {
        let row = row.map_err(be)?;
        sys.drivers.insert(row.name.clone(), row);
    }
    Ok(())
}

fn read_transforms(conn: &rusqlite::Connection, sys: &mut SysState) -> Result<(), ProvisionError> {
    // §15: the current transform definitions (the `/transform` collection). Without this fetch a
    // `qfs plan`/`apply` would see current-transforms as empty and authoritatively DESTROY every
    // real definition — so the reconcile MUST read them. The derived mode is not read (it is not a
    // stored/projected column).
    let mut stmt = conn
        .prepare(
            "SELECT name, input, output, provider, model, effort, secret_ref \
             FROM sys_transforms ORDER BY name",
        )
        .map_err(be)?;
    let rows = stmt
        .query_map([], |r| {
            Ok(TransformRow {
                name: r.get(0)?,
                input: r.get(1)?,
                output: r.get(2)?,
                provider: r.get(3)?,
                model: r.get(4)?,
                effort: r.get(5)?,
                secret_ref: r.get(6)?,
            })
        })
        .map_err(be)?;
    for row in rows {
        let row = row.map_err(be)?;
        sys.transforms.insert(row.name.clone(), row);
    }
    Ok(())
}

fn read_policies(conn: &rusqlite::Connection, sys: &mut SysState) -> Result<(), ProvisionError> {
    let mut stmt = conn
        .prepare("SELECT name, allow, target FROM sys_policies ORDER BY name")
        .map_err(be)?;
    let rows = stmt
        .query_map([], |r| {
            Ok(SysPolicyRow {
                name: r.get(0)?,
                allow: r.get(1)?,
                target: r.get(2)?,
            })
        })
        .map_err(be)?;
    for row in rows {
        let row = row.map_err(be)?;
        sys.policies.insert(row.name.clone(), row);
    }
    Ok(())
}

fn read_settings(conn: &rusqlite::Connection, sys: &mut SysState) -> Result<(), ProvisionError> {
    let mut stmt = conn
        .prepare("SELECT key, value FROM sys_settings ORDER BY key")
        .map_err(be)?;
    let rows = stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
        .map_err(be)?;
    for row in rows {
        let (key, value) = row.map_err(be)?;
        // Secretish settings are excluded from the provisioning universe (blueprint §16, amended):
        // never fetched, so never diffed, so never destroyed by absence.
        if qfs_core::secretish_setting_key(&key) {
            continue;
        }
        sys.settings.insert(key, value);
    }
    Ok(())
}

fn read_bindings(conn: &rusqlite::Connection, sys: &mut SysState) -> Result<(), ProvisionError> {
    let mut stmt = conn
        .prepare(
            "SELECT path, driver_id, at_locator, secret_ref, alias_of, host, account, app \
             FROM path_binding ORDER BY path",
        )
        .map_err(be)?;
    let rows = stmt
        .query_map([], |r| {
            let host: Option<String> = r.get(5)?;
            Ok(PathBindingRow {
                path: r.get(0)?,
                driver: r.get(1)?,
                at: r.get(2)?,
                secret_ref: r.get(3)?,
                alias_of: r.get(4)?,
                // The implicit embedded `local` host is normalised to absent so a binding
                // declared without a HOST clause round-trips (a doc omitting host == host=local).
                host: host.filter(|h| h != "local"),
                account: r.get(6)?,
                app: r.get(7)?,
            })
        })
        .map_err(be)?;
    for row in rows {
        let row = row.map_err(be)?;
        sys.bindings.insert(row.path.clone(), row);
    }
    Ok(())
}

fn be(e: rusqlite::Error) -> ProvisionError {
    ProvisionError::Backend(e.to_string())
}

// ---------------------------------------------------------------------------
// Daemon boot-config re-emission (durability after a committed /server batch)
// ---------------------------------------------------------------------------

/// Re-emit a post-commit `ServerState` as the canonical document at the daemon's boot-config
/// `path`, **atomically** (write a sibling temp file, then rename over `path`) so a concurrent
/// reader never sees a half-written config and an applied reconcile survives a restart
/// (blueprint §16). The `/sys` half is empty (the daemon owns only `/server`).
///
/// # Errors
/// An [`std::io::Error`] if the temp write or the rename fails.
pub fn reemit_boot_config(
    server: &ServerState,
    stamp: &GenerationStamp,
    path: &Path,
) -> std::io::Result<()> {
    let document = qfs_provision::emit(
        &ConfigState {
            server: server.clone(),
            sys: SysState::default(),
        },
        stamp,
    );
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("config.qfs");
    let tmp = dir.join(format!(".{file_name}.tmp-{}", std::process::id()));
    std::fs::write(&tmp, document)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testenv::HomeGuard;
    use qfs_store::{FileSource, SystemDb};

    /// A loopback address nothing listens on (bind :0, capture the port, drop the listener).
    fn closed_addr() -> SocketAddr {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        drop(l);
        addr
    }

    /// A tempdir OUTSIDE the session TMPDIR (which carries `--`, truncated by the config comment
    /// stripper) — for writing `.qfs` documents to disk.
    fn clean_tempdir() -> tempfile::TempDir {
        tempfile::Builder::new()
            .prefix("qfs-prov")
            .tempdir_in("/tmp")
            .unwrap()
    }

    fn seed_sys(home: &HomeGuard) {
        let sys = SystemDb::open(&FileSource::new(home.system_db_path())).unwrap();
        sys.db()
            .conn()
            .execute(
                "INSERT INTO sys_settings (key, value) VALUES ('safety_mode', 'policy-only')",
                [],
            )
            .unwrap();
        sys.db()
            .conn()
            .execute(
                "INSERT INTO sys_settings (key, value) VALUES ('api_token', 'SECRET-CANARY')",
                [],
            )
            .unwrap();
        sys.db()
            .conn()
            .execute(
                "INSERT INTO sys_policies (name, allow, target) VALUES ('analysts', 'SELECT', '/sql/*')",
                [],
            )
            .unwrap();
        sys.db()
            .conn()
            .execute(
                "INSERT INTO sys_drivers (kind, name, base_url, auth, irreversible) \
                 VALUES ('driver', 'chatwork', 'https://api.chatwork.com/v2', \
                         '{\"kind\":\"header\",\"name\":\"x-chatworktoken\"}', 0)",
                [],
            )
            .unwrap();
        // The binding registry lives in the System DB (re-homed by 20260716143641).
        sys.db()
            .conn()
            .execute(
                "INSERT INTO path_binding (path, driver_id, at_locator, secret_ref, host, account) \
                 VALUES ('/chat', 'chatwork', 'https://api.chatwork.com/v2', 'vault:chatwork/work', \
                         'local', 'work')",
                [],
            )
            .unwrap();
        drop(sys);
    }

    #[test]
    fn fetch_sys_state_reads_the_universe_and_excludes_secretish() {
        let home = HomeGuard::with_passphrase("test-pass");
        seed_sys(&home);
        let (sys, stamp) = fetch_sys_state().unwrap();
        assert_eq!(sys.drivers.len(), 1);
        assert_eq!(sys.policies.len(), 1);
        assert_eq!(sys.bindings.len(), 1);
        // The secretish setting is filtered; only the ordinary one is present.
        assert_eq!(sys.settings.len(), 1);
        assert!(sys.settings.contains_key("safety_mode"));
        assert!(!sys.settings.contains_key("api_token"));
        // The implicit local host normalised to absent.
        assert_eq!(sys.bindings["/chat"].host, None);
        assert_eq!(sys.bindings["/chat"].account.as_deref(), Some("work"));
        assert!(stamp.system_migrations > 0);
    }

    #[test]
    fn plan_sys_only_reports_changes_with_no_daemon() {
        let home = HomeGuard::with_passphrase("test-pass");
        seed_sys(&home);
        // A /sys-only document that adds a setting (drift vs the seeded live state).
        let doc = Ok("UPSERT INTO /sys/settings VALUES (key, value) ('safety_mode', 'policy-only');\n\
             UPSERT INTO /sys/settings VALUES (key, value) ('theme', 'dark');\n\
             INSERT INTO /sys/policies VALUES (name, allow, target) ('analysts', 'SELECT', '/sql/*');\n\
             UPSERT INTO /sys/drivers VALUES (kind, name, base_url, auth) \
               ('driver', 'chatwork', 'https://api.chatwork.com/v2', '{\"kind\":\"header\",\"name\":\"x-chatworktoken\"}');\n\
             UPSERT INTO /sys/paths VALUES (account, at, driver, path, secret_ref) \
               ('work', 'https://api.chatwork.com/v2', 'chatwork', '/chat', 'vault:chatwork/work');"
            .to_string());
        let report = plan_document(&doc, closed_addr()).unwrap();
        // Only the added `theme` setting is a change; everything else matches the seed.
        assert_eq!(report.plan.add_count(), 1);
        assert_eq!(report.plan.change_count(), 0);
        assert_eq!(report.plan.destroy_count(), 0);
        assert!(!report.plan.is_empty());
    }

    #[test]
    fn apply_sys_only_end_to_end_then_second_apply_is_a_noop() {
        let home = HomeGuard::with_passphrase("test-pass");
        seed_sys(&home);
        let doc_text = "UPSERT INTO /sys/settings VALUES (key, value) ('safety_mode', 'policy-only');\n\
             UPSERT INTO /sys/settings VALUES (key, value) ('theme', 'dark');\n\
             INSERT INTO /sys/policies VALUES (name, allow, target) ('analysts', 'SELECT', '/sql/*');\n\
             UPSERT INTO /sys/drivers VALUES (kind, name, base_url, auth) \
               ('driver', 'chatwork', 'https://api.chatwork.com/v2', '{\"kind\":\"header\",\"name\":\"x-chatworktoken\"}');\n\
             UPSERT INTO /sys/paths VALUES (account, at, driver, path, secret_ref) \
               ('work', 'https://api.chatwork.com/v2', 'chatwork', '/chat', 'vault:chatwork/work');"
            .to_string();
        let action = ApplyAction {
            document: "-".to_string(),
            commit_irreversible: false,
            allow_stale_base: false,
            json: false,
        };
        let report = apply_document(&Ok(doc_text.clone()), &action, closed_addr()).unwrap();
        assert!(report.applied >= 1, "at least the new setting applied");

        // The new setting landed.
        let (sys, _) = fetch_sys_state().unwrap();
        assert_eq!(sys.settings.get("theme").map(String::as_str), Some("dark"));

        // A second apply of the same document is a no-op (idempotent).
        let report2 = apply_document(&Ok(doc_text), &action, closed_addr()).unwrap();
        assert_eq!(report2.applied, 0, "second apply is empty");
    }

    #[test]
    fn server_document_with_no_daemon_is_host_not_serving() {
        let home = HomeGuard::with_passphrase("test-pass");
        seed_sys(&home);
        // The document configures /server → the /server current state needs the daemon.
        let doc =
            Ok("CREATE ENDPOINT recent ON 'GET /recent' AS /mail/inbox |> limit 5;".to_string());
        let err = plan_document(&doc, closed_addr()).unwrap_err();
        assert!(
            matches!(err, ProvisionError::HostNotServing { .. }),
            "no daemon ⇒ a host-not-serving refusal, got {err:?}"
        );
        // Apply refuses identically — nothing is written.
        let action = ApplyAction {
            document: "-".to_string(),
            commit_irreversible: true,
            allow_stale_base: true,
            json: false,
        };
        assert!(matches!(
            apply_document(&doc, &action, closed_addr()).unwrap_err(),
            ProvisionError::HostNotServing { .. }
        ));
    }

    // -----------------------------------------------------------------------
    // The in-process bridge daemon (the §16 face, composed exactly as `qfs serve` composes it:
    // the shared live ServerState + the /server read facet + the dashboard statement bridge
    // over a real loopback listener). Hermetic: no subprocess, no credentials.
    // -----------------------------------------------------------------------

    /// A running in-process bridge daemon: its loopback address, the shared live state, and the
    /// shutdown sender (the listener thread joins on drop of the test).
    struct BridgeDaemon {
        addr: SocketAddr,
        state: Arc<RwLock<ServerState>>,
        shutdown: tokio::sync::watch::Sender<bool>,
        thread: Option<std::thread::JoinHandle<()>>,
    }

    impl Drop for BridgeDaemon {
        fn drop(&mut self) {
            let _ = self.shutdown.send(true);
            if let Some(t) = self.thread.take() {
                let _ = t.join();
            }
        }
    }

    /// Spin the bridge daemon over `state`, re-emitting its boot config to `config_path`.
    fn spawn_bridge_daemon(
        state: Arc<RwLock<ServerState>>,
        config_path: std::path::PathBuf,
    ) -> BridgeDaemon {
        use qfs_core::{CodecRegistry, Engine};

        // The serve-composition shape: engine + reads with the /server face mounted, the
        // reconfigure channel over the SAME shared state, the ServeMcpEngine with the live seam.
        let mut engine = Engine::new();
        engine.codecs = CodecRegistry::with_builtins();
        let mut reads = qfs_exec::ReadRegistry::new();
        crate::server_face::register_server_face(&mut engine, &mut reads, &state);
        let engine = Arc::new(engine);
        let reads = Arc::new(reads);
        let (handle, _rx) = qfs_http::reconfigure_channel(Arc::clone(&state));
        let mcp_engine: Arc<dyn qfs_mcp::McpEngine> = Arc::new(
            crate::mcp::ServeMcpEngine::new(Arc::clone(&engine), Arc::clone(&reads))
                .with_live_server(crate::mcp::LiveServer {
                    handle,
                    config_path,
                }),
        );
        let fallback: qfs_http::Fallback = Arc::new(move |req: &qfs_http::HttpRequest| {
            crate::dashboard::serve_dashboard(mcp_engine.as_ref(), req)
        });

        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
        let (addr_tx, addr_rx) = std::sync::mpsc::channel();
        let engine_for_thread = engine;
        let reads_for_thread = reads;
        let thread = std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            rt.block_on(async move {
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                addr_tx.send(listener.local_addr().unwrap()).unwrap();
                let binding =
                    qfs_http::HttpBinding::new(engine_for_thread, reads_for_thread, 10_000);
                let router = binding.router_handle();
                let ctx = binding.ctx();
                let mut rx = shutdown_rx;
                let wait = async move {
                    while rx.changed().await.is_ok() {
                        if *rx.borrow() {
                            break;
                        }
                    }
                };
                qfs_http::serve_on_with(listener, router, ctx, Some(fallback), wait).await;
            });
        });
        let addr = addr_rx.recv().unwrap();
        BridgeDaemon {
            addr,
            state,
            shutdown: shutdown_tx,
            thread: Some(thread),
        }
    }

    /// The canonical spec string a stored body carries (what the emitter/differ compare).
    fn canonical_body(src: &str) -> StatementSource {
        let stmt = qfs_exec::parse(src).unwrap();
        StatementSource::new(qfs_core::StatementSpec::from_statement(stmt).canonical())
    }

    /// The seeded live /server state: the bridge policy grant (`api` — ALLOW ALL ON server),
    /// an endpoint, a job (with a runtime high-water mark), and a webhook.
    fn seed_server_state() -> ServerState {
        let mut s = ServerState::new();
        s.policies.insert(
            "api".to_string(),
            PolicyDef {
                name: "api".to_string(),
                handler: String::new(),
                // Explicit verbs, in the canonical VerbSet order (round-trips byte-identically):
                // the policy engine deliberately does NOT let a broad `ALL` grant the
                // irreversible REMOVE, so the bridge grant names it explicitly.
                allow: vec!["ALLOW SELECT,INSERT,UPSERT,UPDATE,REMOVE,CALL ON server".to_string()],
            },
        );
        s.endpoints.insert(
            "recent".to_string(),
            EndpointDef {
                name: "recent".to_string(),
                method: "GET".to_string(),
                route: "/recent".to_string(),
                query: canonical_body("/status |> LIMIT 1"),
                policy: None,
            },
        );
        s.jobs.insert(
            "nightly".to_string(),
            JobDef {
                name: "nightly".to_string(),
                every: "1h".to_string(),
                plan: canonical_body("/status |> LIMIT 1"),
                policy: None,
                last_run: Some(1_700_000_123),
            },
        );
        s.webhooks.insert(
            "ingest".to_string(),
            WebhookDef {
                name: "ingest".to_string(),
                route: "/hooks/ingest".to_string(),
                secret: String::new(),
            },
        );
        s
    }

    /// The desired document: the seed WITH the webhook removed (destroy), the job interval
    /// changed (change), and a trigger added (add) — over the UNCHANGED live `/sys` state (the
    /// document declares both stores; carrying the live /sys as-is keeps the /sys half a no-op).
    /// Emitted with the LIVE stamp so the base has not moved.
    fn desired_document(live_stamp: &GenerationStamp, sys: SysState) -> (String, ConfigState) {
        let mut desired_server = seed_server_state();
        desired_server.webhooks.remove("ingest");
        desired_server.jobs.get_mut("nightly").unwrap().every = "2h".to_string();
        desired_server.triggers.insert(
            "onmail".to_string(),
            TriggerDef {
                name: "onmail".to_string(),
                on: "inbox".to_string(),
                predicate: StatementSource::new(String::new()),
                plan: canonical_body("/status |> LIMIT 1"),
                policy: None,
            },
        );
        let desired = ConfigState {
            server: desired_server,
            sys,
        };
        (qfs_provision::emit(&desired, live_stamp), desired)
    }

    #[test]
    fn server_reconcile_end_to_end_through_the_statement_bridge() {
        // The §16 e2e: a live daemon with seeded /server bindings; a document that adds one /
        // changes one / removes one plans with honest counts, applies through the bridge (the
        // destroy under the ack), converges the live state, re-emits the boot config, and a
        // second apply is a no-op.
        let home = HomeGuard::with_passphrase("test-pass");
        seed_sys(&home);
        let dir = clean_tempdir();
        let config_path = dir.path().join("config.qfs");
        let state = Arc::new(RwLock::new(seed_server_state()));
        let daemon = spawn_bridge_daemon(Arc::clone(&state), config_path.clone());

        let (live_sys, live_stamp) = fetch_sys_state().unwrap();
        let (doc, desired) = desired_document(&live_stamp, live_sys);

        // Plan: add 1 (trigger), change 1 (job every), destroy 1 (webhook); base unmoved.
        let report = plan_document(&Ok(doc.clone()), daemon.addr).unwrap();
        assert_eq!(report.plan.add_count(), 1);
        assert_eq!(report.plan.change_count(), 1);
        assert_eq!(report.plan.destroy_count(), 1);
        assert!(!report.base_moved);

        // Apply refuses without the ack (the destroy), then applies with it.
        let mut action = ApplyAction {
            document: "-".to_string(),
            commit_irreversible: false,
            allow_stale_base: false,
            json: false,
        };
        assert!(matches!(
            apply_document(&Ok(doc.clone()), &action, daemon.addr).unwrap_err(),
            ProvisionError::NeedsIrreversibleAck { destroys: 1 }
        ));
        action.commit_irreversible = true;
        let applied = apply_document(&Ok(doc.clone()), &action, daemon.addr).unwrap();
        assert_eq!(applied.applied, 3, "one bridge commit per /server op");

        // Convergence, re-read THROUGH the bridge: the live state now matches the document.
        let fetched = fetch_server_state(daemon.addr).unwrap();
        assert!(fetched.triggers.contains_key("onmail"), "add converged");
        assert!(
            !fetched.webhooks.contains_key("ingest"),
            "destroy converged"
        );
        assert_eq!(fetched.jobs["nightly"].every, "2h", "change converged");
        let converged = ConfigState {
            server: fetched,
            sys: SysState::default(),
        };
        let desired_server_only = ConfigState {
            server: desired.server.clone(),
            sys: SysState::default(),
        };
        assert!(
            qfs_provision::diff(&converged, &desired_server_only).is_empty(),
            "the fetched live state matches the desired projection exactly"
        );

        // Runtime-field preservation (§16): the job UPDATE kept the live high-water mark.
        assert_eq!(
            daemon.state.read().unwrap().jobs["nightly"].last_run,
            Some(1_700_000_123),
            "a reconcile Update never resets freshness"
        );

        // Reemit: the daemon wrote its post-commit state to the boot-config path; reloading it
        // yields the converged /server projection (reboot-from-file converges).
        let reemitted = std::fs::read_to_string(&config_path).unwrap();
        let reloaded = load(&reemitted).unwrap();
        assert!(
            qfs_provision::diff(&reloaded, &desired_server_only).is_empty(),
            "the re-emitted boot config reloads to the converged projection"
        );

        // Idempotency: a second apply of the same document is an empty plan (no bridge commits).
        let second = apply_document(&Ok(doc), &action, daemon.addr).unwrap();
        assert_eq!(second.applied, 0, "second apply is a no-op");
    }

    #[test]
    fn mixed_server_and_sys_document_reconciles_both_stores() {
        let home = HomeGuard::with_passphrase("test-pass");
        seed_sys(&home);
        let dir = clean_tempdir();
        let state = Arc::new(RwLock::new(seed_server_state()));
        let daemon = spawn_bridge_daemon(Arc::clone(&state), dir.path().join("config.qfs"));

        // Desired: the CURRENT /server seed (no /server drift) + the CURRENT /sys seed + one
        // NEW /sys setting — a mixed document where only the /sys half changes.
        let (sys, live_stamp) = fetch_sys_state().unwrap();
        let mut desired = ConfigState {
            server: seed_server_state(),
            sys,
        };
        desired
            .sys
            .settings
            .insert("theme".to_string(), "dark".to_string());
        let doc = qfs_provision::emit(&desired, &live_stamp);

        let report = plan_document(&Ok(doc.clone()), daemon.addr).unwrap();
        assert_eq!(report.plan.add_count(), 1, "only the new /sys setting");
        assert_eq!(report.plan.change_count(), 0);
        assert_eq!(report.plan.destroy_count(), 0);

        let action = ApplyAction {
            document: "-".to_string(),
            commit_irreversible: false,
            allow_stale_base: true, // the /sys apply moves the ddl_event head between runs
            json: false,
        };
        let applied = apply_document(&Ok(doc.clone()), &action, daemon.addr).unwrap();
        assert_eq!(applied.applied, 1);
        let (sys_after, _) = fetch_sys_state().unwrap();
        assert_eq!(
            sys_after.settings.get("theme").map(String::as_str),
            Some("dark")
        );
        // The /server half was untouched (no drift; nothing applied there).
        assert!(daemon.state.read().unwrap().webhooks.contains_key("ingest"));
    }

    #[test]
    fn bridge_commit_without_the_policy_grant_is_refused_per_statement() {
        // The §16 policy control: a live state WITHOUT the `api` bridge policy row gates every
        // commit default-deny — the apply surfaces a per-statement policy refusal, and the
        // already-planned add is NOT silently skipped.
        let home = HomeGuard::with_passphrase("test-pass");
        seed_sys(&home);
        let dir = clean_tempdir();
        let mut seed = seed_server_state();
        seed.policies.remove("api");
        let state = Arc::new(RwLock::new(seed));
        let daemon = spawn_bridge_daemon(Arc::clone(&state), dir.path().join("config.qfs"));

        let (_, live_stamp) = fetch_sys_state().unwrap();
        // Desired: current minus the `api` policy, plus one new trigger (an add).
        let mut desired_server = seed_server_state();
        desired_server.policies.remove("api");
        desired_server.triggers.insert(
            "onmail".to_string(),
            TriggerDef {
                name: "onmail".to_string(),
                on: "inbox".to_string(),
                predicate: StatementSource::new(String::new()),
                plan: canonical_body("/status |> LIMIT 1"),
                policy: None,
            },
        );
        let doc = qfs_provision::emit(
            &ConfigState {
                server: desired_server,
                sys: SysState::default(),
            },
            &live_stamp,
        );
        let action = ApplyAction {
            document: "-".to_string(),
            commit_irreversible: true,
            allow_stale_base: false,
            json: false,
        };
        let err = apply_document(&Ok(doc), &action, daemon.addr).unwrap_err();
        let text = err.to_string();
        assert!(
            text.contains("policy_denied"),
            "no bridge policy grant ⇒ a per-statement policy refusal, got: {text}"
        );
        assert!(
            !daemon.state.read().unwrap().triggers.contains_key("onmail"),
            "the refused statement applied nothing"
        );
    }

    #[test]
    fn offline_run_engine_does_not_mount_server() {
        // The read leg is serve-side ONLY: the CLI's offline run engine never mounts /server,
        // so an offline `/server/endpoints` read is a structured unknown-source failure — which
        // is exactly what keeps the reconcile CLI's host-not-serving refusal honest.
        let (engine, _reads, _mode) = crate::shell::run_engine_and_reads();
        let stmt = qfs_exec::parse("/server/endpoints").unwrap();
        assert!(
            qfs_exec::build_plan(&stmt, &engine).is_err(),
            "the offline engine must not route /server"
        );
    }

    #[test]
    fn destroy_requires_the_irreversible_ack() {
        let home = HomeGuard::with_passphrase("test-pass");
        seed_sys(&home);
        // A document that OMITS the seeded `safety_mode` setting ⇒ an authoritative destroy.
        let doc = "UPSERT INTO /sys/settings VALUES (key, value) ('theme', 'dark');\n\
             INSERT INTO /sys/policies VALUES (name, allow, target) ('analysts', 'SELECT', '/sql/*');\n\
             UPSERT INTO /sys/drivers VALUES (kind, name, base_url, auth) \
               ('driver', 'chatwork', 'https://api.chatwork.com/v2', '{\"kind\":\"header\",\"name\":\"x-chatworktoken\"}');\n\
             UPSERT INTO /sys/paths VALUES (account, at, driver, path, secret_ref) \
               ('work', 'https://api.chatwork.com/v2', 'chatwork', '/chat', 'vault:chatwork/work');"
            .to_string();
        let mut action = ApplyAction {
            document: "-".to_string(),
            commit_irreversible: false,
            allow_stale_base: true,
            json: false,
        };
        // Without the ack: refused, distinct from a stale base.
        let err = apply_document(&Ok(doc.clone()), &action, closed_addr()).unwrap_err();
        assert!(matches!(
            err,
            ProvisionError::NeedsIrreversibleAck { destroys: 1 }
        ));
        // With the ack: the destroy applies.
        action.commit_irreversible = true;
        let report = apply_document(&Ok(doc), &action, closed_addr()).unwrap();
        assert_eq!(report.destroy, 1);
        let (sys, _) = fetch_sys_state().unwrap();
        assert!(
            !sys.settings.contains_key("safety_mode"),
            "the setting was destroyed"
        );
    }

    #[test]
    fn stale_base_is_refused_without_the_override_and_proceeds_with_it() {
        let home = HomeGuard::with_passphrase("test-pass");
        seed_sys(&home);
        // A document that is a FULL match of the live state (so the diff is empty), emitted with a
        // stamp that does NOT match the live one — an isolated moved-base condition.
        let stale = GenerationStamp {
            system_migrations: 999,
            project_migrations: Some(999),
            ddl_event_head: Some(DdlEventHead {
                seq: 999,
                hash: "stale".to_string(),
            }),
        };
        let (sys, _live) = fetch_sys_state().unwrap();
        let doc = qfs_provision::emit(
            &ConfigState {
                server: ServerState::new(),
                sys,
            },
            &stale,
        );

        let mut action = ApplyAction {
            document: "-".to_string(),
            commit_irreversible: false,
            allow_stale_base: false,
            json: false,
        };
        assert!(matches!(
            apply_document(&Ok(doc.clone()), &action, closed_addr()).unwrap_err(),
            ProvisionError::StaleBase
        ));
        // plan renders the base-moved flag rather than refusing.
        assert!(
            plan_document(&Ok(doc.clone()), closed_addr())
                .unwrap()
                .base_moved
        );
        // With the override, apply proceeds (this doc matches the seed → a no-op, but no refusal).
        action.allow_stale_base = true;
        assert!(apply_document(&Ok(doc), &action, closed_addr()).is_ok());
    }

    #[test]
    fn reemit_boot_config_writes_atomically_and_round_trips() {
        let dir = clean_tempdir();
        let path = dir.path().join("config.qfs");
        let mut server = ServerState::new();
        server.webhooks.insert(
            "ingest".to_string(),
            qfs_provision::WebhookDef {
                name: "ingest".to_string(),
                route: "/hooks/ingest".to_string(),
                secret: String::new(),
            },
        );
        reemit_boot_config(&server, &GenerationStamp::default(), &path).unwrap();

        // The temp file was renamed away (atomic): only the final config remains.
        let leftover: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| e.file_name().to_string_lossy().contains(".tmp-"))
            .collect();
        assert!(
            leftover.is_empty(),
            "the temp file was renamed, not left behind"
        );

        // The written document reloads to the same /server projection.
        let text = std::fs::read_to_string(&path).unwrap();
        let loaded = load(&text).unwrap();
        assert_eq!(loaded.server.webhooks.len(), 1);
        assert_eq!(loaded.server.webhooks["ingest"].route, "/hooks/ingest");
        assert!(loaded.sys.drivers.is_empty(), "the /sys half is empty");
    }

    // -----------------------------------------------------------------------
    // The dispatching applier (composed here, the terminal leaf that owns the concrete SysApplier)
    // -----------------------------------------------------------------------

    use qfs_core::{commit, RowBatch, Schema, Value};
    use qfs_driver_sys::{SysError, SysNode};
    use qfs_provision::{
        build_plan, diff, ConfigState, EndpointDef, PathBindingRow, StatementSource,
    };
    use std::sync::Mutex;

    /// An in-memory fake `/sys` backend (no DB, no creds): records what the DISPATCHER routed to
    /// it, so the mixed-batch routing is proven without the binary's rusqlite implementation.
    #[derive(Default)]
    struct FakeSysBackend {
        settings: Mutex<Vec<(String, String)>>,
        inserted: Mutex<usize>,
        removed: Mutex<Vec<String>>,
    }

    fn cell_text(row: &RowBatch, col: &str) -> Option<String> {
        let idx = row.schema.columns.iter().position(|c| c.name == col)?;
        match row.rows.first().and_then(|r| r.values.get(idx)) {
            Some(Value::Text(s)) => Some(s.clone()),
            _ => None,
        }
    }

    impl SysBackend for FakeSysBackend {
        fn scan(&self, _node: SysNode) -> Result<RowBatch, SysError> {
            Ok(RowBatch::new(Schema::new(vec![]), vec![]))
        }
        fn insert_policy(&self, _row: &RowBatch) -> Result<u64, SysError> {
            *self.inserted.lock().unwrap() += 1;
            Ok(1)
        }
        fn set_setting(&self, row: &RowBatch) -> Result<u64, SysError> {
            let key = cell_text(row, "key").unwrap_or_default();
            let value = cell_text(row, "value").unwrap_or_default();
            self.settings.lock().unwrap().push((key, value));
            Ok(1)
        }
        fn set_billing(&self, _row: &RowBatch) -> Result<u64, SysError> {
            *self.inserted.lock().unwrap() += 1;
            Ok(1)
        }
        fn upsert_binding(&self, _row: &RowBatch) -> Result<u64, SysError> {
            *self.inserted.lock().unwrap() += 1;
            Ok(1)
        }
        fn remove_binding(&self, path: &str) -> Result<u64, SysError> {
            self.removed.lock().unwrap().push(path.to_string());
            Ok(1)
        }
        fn insert_driver(&self, _row: &RowBatch) -> Result<u64, SysError> {
            *self.inserted.lock().unwrap() += 1;
            Ok(1)
        }
        fn record_account(&self, _row: &RowBatch) -> Result<u64, SysError> {
            *self.inserted.lock().unwrap() += 1;
            Ok(1)
        }
        fn remove_account(&self, provider: &str, account: &str) -> Result<u64, SysError> {
            self.removed
                .lock()
                .unwrap()
                .push(format!("{provider}/{account}"));
            Ok(1)
        }
        fn update_policy(&self, _row: &RowBatch) -> Result<u64, SysError> {
            *self.inserted.lock().unwrap() += 1;
            Ok(1)
        }
        fn remove_policy(&self, name: &str) -> Result<u64, SysError> {
            self.removed.lock().unwrap().push(format!("policy:{name}"));
            Ok(1)
        }
        fn remove_setting(&self, key: &str) -> Result<u64, SysError> {
            self.removed.lock().unwrap().push(format!("setting:{key}"));
            Ok(1)
        }
        fn remove_driver(&self, name: &str) -> Result<u64, SysError> {
            self.removed.lock().unwrap().push(format!("driver:{name}"));
            Ok(1)
        }
    }

    #[test]
    fn dispatching_applier_commits_a_mixed_server_and_sys_batch() {
        // A mixed batch: one /server endpoint + one /sys setting on an empty current state, driven
        // through ONE ReconcileApplier — both legs land, CommitReport reflects both.
        let mut desired = ConfigState::new();
        desired.server.endpoints.insert(
            "recent".to_string(),
            EndpointDef {
                name: "recent".to_string(),
                method: "GET".to_string(),
                route: "/recent".to_string(),
                query: StatementSource::new("/mail |> LIMIT 10"),
                policy: None,
            },
        );
        desired
            .sys
            .settings
            .insert("safety_mode".to_string(), "policy-only".to_string());

        let rp = diff(&ConfigState::new(), &desired);
        assert_eq!(rp.add_count(), 2);
        let batch = build_plan(&rp).unwrap();

        let server = Arc::new(RwLock::new(ServerState::new()));
        let backend = Arc::new(FakeSysBackend::default());
        let mut applier = ReconcileApplier::new(&server, SysApplier::new(backend.clone()));
        let report = commit(&batch, &mut applier, |_| {});
        assert!(report.failed.is_none(), "both legs apply cleanly");
        assert_eq!(report.applied.len(), 2, "one /sys + one /server effect");
        assert_eq!(
            applier.into_changes().len(),
            1,
            "the /server leg recorded its change"
        );

        // Both sides landed.
        assert!(server.read().unwrap().endpoints.contains_key("recent"));
        assert_eq!(
            backend.settings.lock().unwrap().as_slice(),
            &[("safety_mode".to_string(), "policy-only".to_string())]
        );
    }

    #[test]
    fn dispatching_applier_routes_a_sys_binding_remove_as_disconnect() {
        let mut current = ConfigState::new();
        current.sys.bindings.insert(
            "/chat".to_string(),
            PathBindingRow {
                path: "/chat".to_string(),
                driver: Some("chatwork".to_string()),
                ..PathBindingRow::default()
            },
        );
        let rp = diff(&current, &ConfigState::new());
        assert_eq!(rp.destroy_count(), 1);
        let batch = build_plan(&rp).unwrap();

        let server = Arc::new(RwLock::new(ServerState::new()));
        let backend = Arc::new(FakeSysBackend::default());
        let mut applier = ReconcileApplier::new(&server, SysApplier::new(backend.clone()));
        let report = commit(&batch, &mut applier, |_| {});
        assert!(report.failed.is_none());
        assert_eq!(
            backend.removed.lock().unwrap().as_slice(),
            &["/chat".to_string()],
            "the DISCONNECT twin reconstructs the binding path"
        );
    }
}
