//! The `/server` **read facet** for the serve composition (blueprint §16 "The face, named" —
//! the read leg of the statement bridge).
//!
//! Mounts the introspective `/server` driver ([`qfs_http::ServerDriver`], pure describe +
//! capabilities) into the serve engine and registers this [`ReadDriver`] facet so
//! `/server/endpoints` (and every sibling collection) resolves + scans through the statement
//! bridge like any path. The scan reads a **snapshot** of the live [`ServerState`] (clone under
//! the read guard — never held across an await) and projects each collection through the
//! canonical [`qfs_core::server_node_schema`] column order, so `DESCRIBE`, the stored rows, and
//! the scan output cannot drift.
//!
//! Registered by the **serve composition only** ([`crate::serve`]): the CLI's offline run engine
//! never mounts `/server`, which is what keeps the reconcile CLI's host-not-serving refusal
//! honest (an unrouted `/server` read outside a daemon is a structured unknown-mount, never an
//! empty current state).

use std::sync::{Arc, RwLock};

use qfs_core::{server_node_schema, CfsError, Engine, Row, RowBatch, ServerNode, Value};
use qfs_exec::{ReadDriver, ReadRegistry};
use qfs_provision::ServerState;
use qfs_pushdown::ScanNode;

/// The `/server` scan facet: a [`ReadDriver`] over the shared live [`ServerState`] lock.
pub struct ServerReadFacet {
    state: Arc<RwLock<ServerState>>,
}

impl ServerReadFacet {
    /// Build the facet over the shared live state handle.
    #[must_use]
    pub fn new(state: Arc<RwLock<ServerState>>) -> Self {
        Self { state }
    }

    /// A read snapshot (clone) of the live state — the guard is never held across an await.
    fn snapshot(&self) -> ServerState {
        self.state.read().map(|g| g.clone()).unwrap_or_default()
    }
}

/// Register the `/server` read facet into the serve composition: the introspective driver into
/// the engine's mounts (describe + capabilities + planning) and the scan facet into the read
/// registry. Serve-side ONLY (see the module docs).
pub fn register_server_face(
    engine: &mut Engine,
    reads: &mut ReadRegistry,
    state: &Arc<RwLock<ServerState>>,
) {
    if let Err(e) = engine
        .mounts
        .register(Arc::new(qfs_http::ServerDriver::new(Arc::clone(state))))
    {
        tracing::warn!(target: "qfs::serve", error = %e, "could not mount the /server read facet");
    }
    reads.register(
        qfs_core::DriverId::new("server"),
        Arc::new(ServerReadFacet::new(Arc::clone(state))),
    );
}

#[async_trait::async_trait]
impl ReadDriver for ServerReadFacet {
    async fn scan(&self, scan: &ScanNode) -> Result<RowBatch, CfsError> {
        // The READ-ONLY per-job run history wins over its `/server/jobs` prefix (the same
        // precedence the driver's describe/capabilities apply): project the recorded firings
        // through the canonical `job_runs_schema` column order. An unknown job name is an empty
        // history, not an error — the collection exists as soon as the job row does.
        if let Some(job) = qfs_http::job_runs_path_job(&scan.path) {
            let snapshot = self.snapshot();
            let rows = snapshot
                .job_runs
                .get(job)
                .map(|runs| runs.iter().map(job_run_row).collect())
                .unwrap_or_default();
            return Ok(RowBatch::new(qfs_core::job_runs_schema(), rows));
        }
        let segment = scan
            .path
            .strip_prefix("/server/")
            .or_else(|| scan.path.strip_prefix("server/"))
            .map(|rest| rest.split('/').next().unwrap_or(rest))
            .unwrap_or_default();
        let node = ServerNode::from_segment(segment).ok_or_else(|| CfsError::UnsupportedVerb {
            path: scan.path.clone(),
            verb: "SELECT",
            supported: Vec::new(),
        })?;
        let snapshot = self.snapshot();
        Ok(RowBatch::new(
            server_node_schema(node),
            collection_rows(&snapshot, node),
        ))
    }
}

/// A `Text` value, or `Null` for an empty string (absent optional — round-trips through the
/// nullable schema columns exactly as the config writers leave them).
fn text_or_null(s: &str) -> Value {
    if s.is_empty() {
        Value::Null
    } else {
        Value::Text(s.to_string())
    }
}

/// An optional handle column (`policy`), `Null` when unattached.
fn opt_text(v: Option<&String>) -> Value {
    v.map_or(Value::Null, |s| Value::Text(s.clone()))
}

/// Project one recorded firing into a `/server/jobs/<name>/runs` row, in the canonical
/// [`qfs_core::job_runs_schema`] column order.
fn job_run_row(r: &qfs_provision::JobRunRecord) -> Row {
    Row::new(vec![
        Value::Timestamp(r.scheduled_at),
        Value::Text(r.outcome.clone()),
        text_or_null(&r.detail),
        Value::Int(r.affected),
        // blueprint §19 axis B/D: the firing principal (secret-free identity), Null for a
        // principal-less ordinary fire.
        text_or_null(&r.principal),
    ])
}

/// Project one collection of the snapshot into rows, in the canonical schema column order
/// (the same [`qfs_core::server_node_schema`] the driver's `DESCRIBE` serves).
fn collection_rows(state: &ServerState, node: ServerNode) -> Vec<Row> {
    match node {
        ServerNode::Endpoints => state
            .endpoints
            .values()
            .map(|d| {
                Row::new(vec![
                    Value::Text(d.name.clone()),
                    text_or_null(&d.method),
                    text_or_null(&d.route),
                    text_or_null(d.query.as_str()),
                    opt_text(d.policy.as_ref()),
                ])
            })
            .collect(),
        ServerNode::Triggers => state
            .triggers
            .values()
            .map(|d| {
                Row::new(vec![
                    Value::Text(d.name.clone()),
                    text_or_null(&d.on),
                    text_or_null(d.predicate.as_str()),
                    text_or_null(d.plan.as_str()),
                    opt_text(d.policy.as_ref()),
                ])
            })
            .collect(),
        ServerNode::Jobs => state
            .jobs
            .values()
            .map(|d| {
                Row::new(vec![
                    Value::Text(d.name.clone()),
                    text_or_null(&d.every),
                    text_or_null(d.plan.as_str()),
                    d.last_run.map_or(Value::Null, Value::Timestamp),
                    opt_text(d.policy.as_ref()),
                ])
            })
            .collect(),
        ServerNode::Views => state
            .views
            .values()
            .map(|d| {
                Row::new(vec![
                    Value::Text(d.name.clone()),
                    text_or_null(d.query.as_str()),
                    Value::Bool(d.materialized),
                    d.last_run.map_or(Value::Null, Value::Timestamp),
                ])
            })
            .collect(),
        ServerNode::Policies => state
            .policies
            .values()
            .map(|d| {
                Row::new(vec![
                    Value::Text(d.name.clone()),
                    text_or_null(&d.handler),
                    Value::Array(d.allow.iter().cloned().map(Value::Text).collect()),
                ])
            })
            .collect(),
        ServerNode::Webhooks => state
            .webhooks
            .values()
            .map(|d| {
                Row::new(vec![
                    Value::Text(d.name.clone()),
                    text_or_null(&d.route),
                    text_or_null(&d.secret),
                ])
            })
            .collect(),
        ServerNode::Agents => state
            .agents
            .values()
            // blueprint §19: the agent read-back is credential-free — name + query-function plan
            // (axis C) + policy handle, in the `server_node_schema(Agents)` column order.
            .map(|d| {
                Row::new(vec![
                    Value::Text(d.name.clone()),
                    text_or_null(d.plan.as_str()),
                    opt_text(d.policy.as_ref()),
                ])
            })
            .collect(),
    }
}
