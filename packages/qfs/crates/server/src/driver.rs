//! The `/server/...` self-config **driver** (blueprint §10: the server IS a driver).
//!
//! [`ServerDriver`] implements the t13 [`Driver`] contract over `/server/{endpoints,
//! triggers,jobs,views,policies,webhooks}`. Its **introspective** half is pure data:
//! [`Driver::describe`] returns the per-node config schema (so `DESCRIBE /server/triggers`
//! works with no live backend) and [`Driver::capabilities`] advertises the verbs each node
//! accepts. Writes to `/server/...` lower to **pure**
//! [`EffectKind::ServerConfigWrite`](qfs_core::EffectKind::ServerConfigWrite) plan nodes
//! (the purity invariant: building the plan mutates nothing); the interpreter mutates
//! [`ServerState`] **only at COMMIT**, via [`apply_server_write`].
//!
//! The owned `ServerState` lives behind an `Arc<RwLock<…>>` so a single `/server` write is
//! one ACID mutation (blueprint §7) and (future) inbound bindings share the same source of truth.

use std::sync::Arc;
use std::sync::RwLock;

use qfs_core::{
    Archetype, Capabilities, CfsError, Driver, NodeDesc, Path, PlanApplier, PushdownProfile,
    Schema, ServerNode, ServerWriteOp, Value,
};

use crate::state::{
    AgentDef, EndpointDef, JobDef, PolicyDef, ServerState, StatementSource, TriggerDef, ViewDef,
    WebhookDef,
};

/// The reserved mount point for the server-as-a-driver (blueprint §10).
pub const SERVER_MOUNT: &str = "/server";

/// The `/server/...` self-config driver. Holds the shared [`ServerState`] (the source of
/// truth) behind an `Arc<RwLock<…>>`. The introspective methods read **nothing** from the
/// lock (they return static schema/capability data), so `DESCRIBE`/capability gating never
/// contend with a live write.
pub struct ServerDriver {
    state: Arc<RwLock<ServerState>>,
    pushdown: PushdownProfile,
    procs: Vec<qfs_core::ProcSig>,
}

impl ServerDriver {
    /// Construct a driver over a shared [`ServerState`] handle.
    #[must_use]
    pub fn new(state: Arc<RwLock<ServerState>>) -> Self {
        Self {
            state,
            // The config registry filters/projects in-engine (it is a small in-memory map);
            // it pushes nothing down. Honest declaration (blueprint §7).
            pushdown: PushdownProfile::None,
            procs: Vec::new(),
        }
    }

    /// The shared state handle (so the runtime can snapshot it for bindings + the audit).
    #[must_use]
    pub fn state(&self) -> &Arc<RwLock<ServerState>> {
        &self.state
    }

    /// Resolve a `/server/...` path to its [`ServerNode`], if the path names a known
    /// collection. `/server/triggers` and `/server/triggers/<name>` both resolve to
    /// [`ServerNode::Triggers`]. Returns `None` for `/server` itself or an unknown segment.
    #[must_use]
    pub fn node_for_path(path: &Path) -> Option<ServerNode> {
        let raw = path.as_str();
        let rest = raw
            .strip_prefix("/server/")
            .or_else(|| raw.strip_prefix("server/"))?;
        let segment = rest.split('/').next().unwrap_or(rest);
        ServerNode::from_segment(segment)
    }
}

/// The job name when `path` addresses the READ-ONLY per-job run history
/// `/server/jobs/<name>/runs` (blueprint §10), `None` for every other path. Checked BEFORE
/// [`ServerDriver::node_for_path`] wherever both apply — the runs sub-collection is runtime
/// telemetry (select-only), never the writable jobs config node its prefix would resolve to.
#[must_use]
pub fn job_runs_path_job(path: &str) -> Option<&str> {
    let rest = path
        .strip_prefix("/server/")
        .or_else(|| path.strip_prefix("server/"))?;
    let mut segs = rest.split('/');
    if segs.next() != Some("jobs") {
        return None;
    }
    let name = segs.next().filter(|s| !s.is_empty())?;
    (segs.next() == Some("runs") && segs.next().is_none()).then_some(name)
}

/// The agent name when `path` addresses the READ-ONLY per-agent cadence-fire history
/// `/server/agents/<name>/runs` (blueprint §19 axis D), `None` otherwise. The agent's own
/// run-history read-back, select-only telemetry beside `/server/jobs/<name>/runs`.
#[must_use]
pub fn agent_runs_path_agent(path: &str) -> Option<&str> {
    let rest = path
        .strip_prefix("/server/")
        .or_else(|| path.strip_prefix("server/"))?;
    let mut segs = rest.split('/');
    if segs.next() != Some("agents") {
        return None;
    }
    let name = segs.next().filter(|s| !s.is_empty())?;
    (segs.next() == Some("runs") && segs.next().is_none()).then_some(name)
}

/// The capability set every `/server/...` config node advertises: a relational table
/// supporting `SELECT/INSERT/UPSERT/UPDATE/REMOVE` (no blob verbs). Single source of truth
/// shared by [`Driver::capabilities`] and the plan-time verb gate.
#[must_use]
pub fn server_node_capabilities() -> Capabilities {
    Capabilities::none()
        .select()
        .insert()
        .upsert()
        .update()
        .remove()
}

/// The typed [`Schema`] of a `/server/...` config node — what `DESCRIBE /server/<node>`
/// returns. Pure data; no live backend. The schema is owned by **closed core** (t31:
/// [`qfs_core::server_node_schema`]) so `DESCRIBE`, the stored rows, and the DDL desugar all
/// read one source of truth and cannot drift; this re-exports it.
#[must_use]
pub fn server_node_schema(node: ServerNode) -> Schema {
    qfs_core::server_node_schema(node)
}

impl Driver for ServerDriver {
    fn mount(&self) -> &str {
        SERVER_MOUNT
    }

    fn describe(&self, path: &Path) -> Result<NodeDesc, CfsError> {
        // Pure: returns static schema data; never touches the RwLock or any I/O.
        // A runs sub-collection (jobs or agents, blueprint §19 axis D) wins over its config-node
        // prefix (select-only telemetry) and shares the canonical `job_runs_schema`.
        if job_runs_path_job(path.as_str()).is_some()
            || agent_runs_path_agent(path.as_str()).is_some()
        {
            return Ok(NodeDesc::new(
                Archetype::RelationalTable,
                qfs_core::job_runs_schema(),
            ));
        }
        let node = Self::node_for_path(path).ok_or_else(|| CfsError::UnsupportedVerb {
            path: path.as_str().to_string(),
            verb: "DESCRIBE",
            supported: Vec::new(),
        })?;
        Ok(NodeDesc::new(
            Archetype::RelationalTable,
            server_node_schema(node),
        ))
    }

    fn capabilities(&self, path: &Path) -> Capabilities {
        // A run history (jobs or agents) is structurally read-only: SELECT and nothing else — a
        // write to `/server/{jobs,agents}/<name>/runs` must never fall through to the writable
        // config node its prefix would resolve to.
        if job_runs_path_job(path.as_str()).is_some()
            || agent_runs_path_agent(path.as_str()).is_some()
        {
            return Capabilities::none().select();
        }
        // A known /server node is a relational config table; an unknown path denies all.
        match Self::node_for_path(path) {
            Some(_) => server_node_capabilities(),
            None => Capabilities::none(),
        }
    }

    fn procedures(&self) -> &[qfs_core::ProcSig] {
        &self.procs
    }

    fn pushdown(&self) -> &PushdownProfile {
        &self.pushdown
    }

    fn applier(&self) -> &dyn PlanApplier {
        // The /server writes apply through the runtime's `ServerConfigApplier` (a
        // `PlanApplier` that holds the same `Arc<RwLock<ServerState>>` and dispatches to
        // `apply_server_write`). The introspective driver does not own that impure seam —
        // the runtime does — so this contract method is unreachable for `/server` writes in
        // the boot path. We keep a no-op applier to satisfy the trait without I/O.
        &NoopApplier
    }
}

/// A no-op applier for the `Driver::applier()` contract slot. The real `/server` apply path
/// is the runtime's `ServerConfigApplier` (which mutates `ServerState`); this exists only
/// so `ServerDriver` satisfies the introspective `Driver` trait without pretending to own
/// the impure seam. It records nothing and touches no state.
struct NoopApplier;

impl PlanApplier for NoopApplier {
    fn apply(
        &mut self,
        node: &qfs_core::EffectNode,
    ) -> Result<qfs_core::AppliedEffect, qfs_core::ApplyError> {
        // Should not be reached for /server writes (the runtime uses ServerConfigApplier).
        Ok(qfs_core::AppliedEffect::new(node.id, 0))
    }
}

/// The before/after audit record of one applied `/server` mutation (blueprint §7/§8): who/op/
/// node/before-after. Owned, secret-free — `before`/`after` are the affected row's
/// **names**, never the credential-bearing contents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigChange {
    /// The collection mutated.
    pub node: ServerNode,
    /// The write op applied.
    pub op: ServerWriteOp,
    /// The affected row name (the config key), if one was carried.
    pub name: Option<String>,
    /// Whether a row with that name existed before the apply.
    pub existed_before: bool,
    /// Whether a row with that name exists after the apply.
    pub exists_after: bool,
}

/// Apply one `/server` config write to [`ServerState`] under the held write guard — the
/// **only** way `ServerState` changes (blueprint §10). Called by the runtime's
/// `ServerConfigApplier` at `COMMIT`, never while building a plan (purity invariant).
///
/// The config DTO rides in the effect node's `args` [`RowBatch`](qfs_core::RowBatch); this
/// decodes the single row into the owned DTO and applies the op. Returns the secret-free
/// [`ConfigChange`] for the audit ledger.
///
/// # Errors
/// A secret-free message if the row is missing / malformed for the target node.
pub fn apply_server_write(
    state: &mut ServerState,
    node: ServerNode,
    op: ServerWriteOp,
    args: &qfs_core::RowBatch,
) -> Result<ConfigChange, String> {
    let name = row_name(args);
    let existed_before = name
        .as_deref()
        .is_some_and(|n| collection_contains(state, node, n));

    match op {
        ServerWriteOp::Remove => {
            let key = name
                .clone()
                .ok_or_else(|| "REMOVE /server requires a `name`".to_string())?;
            remove_row(state, node, &key);
        }
        ServerWriteOp::Insert | ServerWriteOp::Upsert | ServerWriteOp::Update => {
            // INSERT/UPSERT/UPDATE all carry a full config row; the distinction (duplicate
            // rejection vs replace) is the verb semantics. UPSERT (the boot/replay verb) is
            // a stable replace-by-name so re-applying a config converges (idempotency §6).
            if matches!(op, ServerWriteOp::Insert) && existed_before {
                return Err(format!(
                    "INSERT into /server/{} would duplicate `{}` (use UPSERT)",
                    node.segment(),
                    name.as_deref().unwrap_or("")
                ));
            }
            if matches!(op, ServerWriteOp::Update) && !existed_before {
                return Err(format!(
                    "UPDATE /server/{} has no row `{}`",
                    node.segment(),
                    name.as_deref().unwrap_or("")
                ));
            }
            insert_row(state, node, args)?;
        }
    }

    let exists_after = name
        .as_deref()
        .is_some_and(|n| collection_contains(state, node, n));
    Ok(ConfigChange {
        node,
        op,
        name,
        existed_before,
        exists_after,
    })
}

/// The `name` value of the (single) config row in `args`, if present and textual.
fn row_name(args: &qfs_core::RowBatch) -> Option<String> {
    let idx = args.schema.columns.iter().position(|c| c.name == "name")?;
    let row = args.rows.first()?;
    match row.values.get(idx) {
        Some(Value::Text(s)) => Some(s.clone()),
        _ => None,
    }
}

/// Whether the named row exists in the target collection.
fn collection_contains(state: &ServerState, node: ServerNode, name: &str) -> bool {
    match node {
        ServerNode::Endpoints => state.endpoints.contains_key(name),
        ServerNode::Triggers => state.triggers.contains_key(name),
        ServerNode::Jobs => state.jobs.contains_key(name),
        ServerNode::Views => state.views.contains_key(name),
        ServerNode::Policies => state.policies.contains_key(name),
        ServerNode::Webhooks => state.webhooks.contains_key(name),
        ServerNode::Agents => state.agents.contains_key(name),
    }
}

/// Remove the named row from the target collection (idempotent — a missing row is a no-op).
fn remove_row(state: &mut ServerState, node: ServerNode, name: &str) {
    match node {
        ServerNode::Endpoints => {
            state.endpoints.remove(name);
        }
        ServerNode::Triggers => {
            state.triggers.remove(name);
        }
        ServerNode::Jobs => {
            state.jobs.remove(name);
            // The run history lives and dies with its job row (telemetry has no orphan life).
            state.job_runs.remove(name);
        }
        ServerNode::Views => {
            state.views.remove(name);
        }
        ServerNode::Policies => {
            state.policies.remove(name);
        }
        ServerNode::Webhooks => {
            state.webhooks.remove(name);
        }
        ServerNode::Agents => {
            state.agents.remove(name);
            // The run history lives and dies with its agent row (telemetry has no orphan life).
            state.agent_runs.remove(name);
        }
    }
}

/// Decode the single config row from `args` and insert/replace it by name.
fn insert_row(
    state: &mut ServerState,
    node: ServerNode,
    args: &qfs_core::RowBatch,
) -> Result<(), String> {
    let get = |col: &str| -> Option<&Value> {
        let idx = args.schema.columns.iter().position(|c| c.name == col)?;
        args.rows.first().and_then(|r| r.values.get(idx))
    };
    let text = |col: &str| -> String {
        match get(col) {
            Some(Value::Text(s)) => s.clone(),
            _ => String::new(),
        }
    };
    let name = match get("name") {
        Some(Value::Text(s)) if !s.is_empty() => s.clone(),
        _ => return Err(format!("/server/{} row requires a `name`", node.segment())),
    };

    match node {
        ServerNode::Endpoints => {
            state.endpoints.insert(
                name.clone(),
                EndpointDef {
                    name,
                    method: text("method"),
                    route: text("route"),
                    query: StatementSource::new(text("query")),
                    // An absent / empty `policy` column means read-only-by-default (`None`).
                    policy: match get("policy") {
                        Some(Value::Text(s)) if !s.is_empty() => Some(s.clone()),
                        _ => None,
                    },
                },
            );
        }
        ServerNode::Triggers => {
            state.triggers.insert(
                name.clone(),
                TriggerDef {
                    name,
                    on: text("on"),
                    predicate: StatementSource::new(text("predicate")),
                    plan: StatementSource::new(text("plan")),
                    policy: match get("policy") {
                        Some(Value::Text(s)) if !s.is_empty() => Some(s.clone()),
                        _ => None,
                    },
                },
            );
        }
        ServerNode::Jobs => {
            // Runtime-field preservation (blueprint §16, Decision X): `last_run` is execution
            // state, not configuration. A replace-by-name (UPSERT/UPDATE — the reconcile and
            // boot-replay verbs) whose args carry NO timestamp PRESERVES the existing row's
            // high-water mark — a reconcile never resets freshness. A fresh row stays `None`.
            let last_run = match get("last_run") {
                Some(Value::Timestamp(t)) | Some(Value::Int(t)) => Some(*t),
                _ => state.jobs.get(&name).and_then(|j| j.last_run),
            };
            state.jobs.insert(
                name.clone(),
                JobDef {
                    name,
                    every: text("every"),
                    plan: StatementSource::new(text("plan")),
                    policy: match get("policy") {
                        Some(Value::Text(s)) if !s.is_empty() => Some(s.clone()),
                        _ => None,
                    },
                    last_run,
                },
            );
        }
        ServerNode::Views => {
            let materialized = matches!(get("materialized"), Some(Value::Bool(true)));
            // Freshness as data (blueprint §14): a materialized view carries its last successful
            // refresh time (the persisted `LAST_RUN` high-water mark), read the same way a job's
            // is. Runtime-field preservation (blueprint §16): a replace-by-name whose args carry
            // no timestamp keeps the existing freshness AND the cached snapshot — a reconcile
            // `Update` never resets a materialized cache.
            let existing = state.views.get(&name);
            let last_run = match get("last_run") {
                Some(Value::Timestamp(t)) | Some(Value::Int(t)) => Some(*t),
                _ => existing.and_then(|v| v.last_run),
            };
            let cache_json = existing.and_then(|v| v.cache_json.clone());
            state.views.insert(
                name.clone(),
                ViewDef {
                    name,
                    query: StatementSource::new(text("query")),
                    materialized,
                    last_run,
                    cache_json,
                },
            );
        }
        ServerNode::Policies => {
            let allow = match get("allow") {
                Some(Value::Array(items)) => items
                    .iter()
                    .filter_map(|v| match v {
                        Value::Text(s) => Some(s.clone()),
                        _ => None,
                    })
                    .collect(),
                _ => Vec::new(),
            };
            state.policies.insert(
                name.clone(),
                PolicyDef {
                    name,
                    handler: text("handler"),
                    allow,
                },
            );
        }
        ServerNode::Webhooks => {
            state.webhooks.insert(
                name.clone(),
                WebhookDef {
                    name,
                    route: text("route"),
                    secret: text("secret"),
                },
            );
        }
        ServerNode::Agents => {
            // blueprint §19: credential-free — name, the launch cadence `every` (axis D), the query
            // function `plan` (axis C), and the optional attached POLICY handle (axis E).
            // Runtime-field preservation (blueprint §16): a replace-by-name whose args carry NO
            // `last_run` PRESERVES the existing high-water mark — a reconcile never resets it.
            let last_run = match get("last_run") {
                Some(Value::Timestamp(t)) | Some(Value::Int(t)) => Some(*t),
                _ => state.agents.get(&name).and_then(|a| a.last_run),
            };
            state.agents.insert(
                name.clone(),
                AgentDef {
                    name,
                    every: text("every"),
                    last_run,
                    plan: StatementSource::new(text("plan")),
                    policy: match get("policy") {
                        Some(Value::Text(s)) if !s.is_empty() => Some(s.clone()),
                        _ => None,
                    },
                },
            );
        }
    }
    Ok(())
}
