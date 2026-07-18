//! The **config projection** of a `/server` row — the diff/emit unit (blueprint §16, Decision X).
//!
//! A [`ProjRow`] is the name→value column map of exactly the fields that make up a config
//! binding's *identity as code*: never the runtime freshness fields ([`ViewDef::last_run`] /
//! [`ViewDef::cache_json`], [`JobDef::last_run`]). Two bindings that differ **only** in those
//! runtime fields produce the **same** [`ProjRow`], so a materialized-view refresh between fetch
//! and apply is not drift (blueprint §16: runtime fields never emit, never diff, are preserved).
//!
//! The projection is the single seam shared by the emitter (renders each column), the diff
//! engine (equality + the downstream write payload), and the loader's round-trip law: the column
//! values are exactly what [`qfs_core::binding_config_row`] stores, so re-loading an emitted
//! document reproduces the identical projection (CREATE ≡ INSERT).

use std::collections::BTreeMap;

use qfs_core::{
    config_row_batch, Column, ColumnType, ConfigRow, Row, RowBatch, Schema, ServerNode, Value,
};
use qfs_server::{
    AgentDef, EndpointDef, JobDef, PolicyDef, ServerState, TriggerDef, ViewDef, WebhookDef,
};

/// One `/server` row projected to its config columns (runtime fields excluded). The map is a
/// [`BTreeMap`] so column iteration — and thus the emitted `VALUES (...)` order and the diff
/// equality — is deterministic.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ProjRow {
    cols: BTreeMap<String, Value>,
}

impl ProjRow {
    /// Set a typed column value.
    pub(crate) fn set(&mut self, key: &str, value: Value) {
        self.cols.insert(key.to_string(), value);
    }

    /// Set a text column value (always emitted, even when empty — mirrors the canonical
    /// [`qfs_core::binding_config_row`], which always writes the always-present columns).
    pub(crate) fn set_text(&mut self, key: &str, text: &str) {
        self.set(key, Value::Text(text.to_string()));
    }

    /// Set a text column only when non-empty (the optional columns: a policy handle, a trigger
    /// guard, a webhook secret). An absent optional column round-trips through the schema `Null`
    /// exactly as [`qfs_core::binding_config_row`] leaves it, so equality stays exact.
    fn set_text_opt(&mut self, key: &str, text: &str) {
        if !text.is_empty() {
            self.set_text(key, text);
        }
    }

    /// The set columns, in deterministic (name) order.
    pub fn columns(&self) -> impl Iterator<Item = (&String, &Value)> {
        self.cols.iter()
    }

    /// Whether the projection carries any column (a name is always present in practice).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.cols.is_empty()
    }

    /// The canonical [`ConfigRow`] this projection maps to (the seam `config_row_batch` reads).
    fn to_config_row(&self) -> ConfigRow {
        let mut row = ConfigRow::default();
        for (key, value) in &self.cols {
            row.set(key, value.clone());
        }
        row
    }

    /// Build the schema-ordered [`RowBatch`] payload for a write to `node` (the batch the plan
    /// builder turns into one `ServerConfigWrite` plan node). Absent columns fill with `Null`.
    ///
    /// # Errors
    /// A secret-free string if the projection carries a column the node's schema does not declare
    /// (unreachable for the built-in converters, which only set declared columns).
    pub fn to_row_batch(&self, node: ServerNode) -> Result<RowBatch, String> {
        config_row_batch(node, &self.to_config_row()).map_err(|e| e.to_string())
    }

    /// Build a name-addressed [`RowBatch`] carrying exactly the set columns (the `/sys` write
    /// payload — the [`qfs_driver_sys::SysBackend`](qfs_driver_sys::SysBackend) reads columns by
    /// NAME, so no fixed schema order is required and absent optionals are simply absent).
    /// Column types derive from the values (text / bool / int; all nullable).
    #[must_use]
    pub fn to_named_batch(&self) -> RowBatch {
        let mut cols = Vec::with_capacity(self.cols.len());
        let mut values = Vec::with_capacity(self.cols.len());
        for (name, value) in &self.cols {
            let ty = match value {
                Value::Bool(_) => ColumnType::Bool,
                Value::Int(_) => ColumnType::Int,
                _ => ColumnType::Text,
            };
            cols.push(Column::new(name.clone(), ty, true));
            values.push(value.clone());
        }
        RowBatch::new(Schema::new(cols), vec![Row::new(values)])
    }
}

/// A name-only projection (the payload a `REMOVE` needs — `apply_server_write` reads only `name`).
#[must_use]
pub fn name_only(name: &str) -> ProjRow {
    let mut row = ProjRow::default();
    row.set_text("name", name);
    row
}

/// The config projection of an endpoint row (`name`/`method`/`route`/`query`; `policy` optional).
#[must_use]
pub fn endpoint_proj(def: &EndpointDef) -> ProjRow {
    let mut row = ProjRow::default();
    row.set_text("name", &def.name);
    row.set_text("method", &def.method);
    row.set_text("route", &def.route);
    row.set_text("query", def.query.as_str());
    if let Some(policy) = &def.policy {
        row.set_text_opt("policy", policy);
    }
    row
}

/// The config projection of a trigger row (`name`/`on`/`plan`; `predicate`/`policy` optional).
#[must_use]
pub fn trigger_proj(def: &TriggerDef) -> ProjRow {
    let mut row = ProjRow::default();
    row.set_text("name", &def.name);
    row.set_text("on", &def.on);
    row.set_text_opt("predicate", def.predicate.as_str());
    row.set_text("plan", def.plan.as_str());
    if let Some(policy) = &def.policy {
        row.set_text_opt("policy", policy);
    }
    row
}

/// The config projection of a job row (`name`/`every`/`plan`; `policy` optional). The runtime
/// [`JobDef::last_run`] high-water mark is **excluded** (never drift, never emitted).
#[must_use]
pub fn job_proj(def: &JobDef) -> ProjRow {
    let mut row = ProjRow::default();
    row.set_text("name", &def.name);
    row.set_text("every", &def.every);
    row.set_text("plan", def.plan.as_str());
    if let Some(policy) = &def.policy {
        row.set_text_opt("policy", policy);
    }
    row
}

/// The config projection of a view row (`name`/`query`/`materialized`). The runtime freshness
/// fields ([`ViewDef::last_run`], [`ViewDef::cache_json`]) are **excluded** — a refresh is not
/// drift (blueprint §16, Decision X).
#[must_use]
pub fn view_proj(def: &ViewDef) -> ProjRow {
    let mut row = ProjRow::default();
    row.set_text("name", &def.name);
    row.set_text("query", def.query.as_str());
    row.set("materialized", Value::Bool(def.materialized));
    row
}

/// The config projection of a policy row (`name`/`handler`/`allow`). `allow` is the canonical
/// rule-string array (the same form `CREATE POLICY` desugars into `/server/policies.allow`).
#[must_use]
pub fn policy_proj(def: &PolicyDef) -> ProjRow {
    let mut row = ProjRow::default();
    row.set_text("name", &def.name);
    row.set_text("handler", &def.handler);
    row.set(
        "allow",
        Value::Array(def.allow.iter().cloned().map(Value::Text).collect()),
    );
    row
}

/// The config projection of a webhook row (`name`/`route`; `secret` optional, secret-by-handle).
#[must_use]
pub fn webhook_proj(def: &WebhookDef) -> ProjRow {
    let mut row = ProjRow::default();
    row.set_text("name", &def.name);
    row.set_text("route", &def.route);
    row.set_text_opt("secret", &def.secret);
    row
}

/// The config projection of an agent row (`name`; `policy` optional). Credential-free (blueprint
/// §19 axis A/E) — the agent binding's identity as code is its name and, when attached, its
/// least-privilege POLICY handle.
#[must_use]
pub fn agent_proj(def: &AgentDef) -> ProjRow {
    let mut row = ProjRow::default();
    row.set_text("name", &def.name);
    // blueprint §19 axis D: the launch cadence (empty for a launch-less agent). The runtime
    // `last_run` high-water mark is EXCLUDED — a fire is not drift (the same rule as jobs).
    row.set_text("every", &def.every);
    // blueprint §19 axis C: the query function's canonical plan body (empty for a function-less
    // agent — the same body-column convention as jobs/triggers).
    row.set_text("plan", def.plan.as_str());
    if let Some(policy) = &def.policy {
        row.set_text_opt("policy", policy);
    }
    row
}

/// The name→projection map of one collection, keyed and sorted by row name.
pub(crate) fn collection_projs(state: &ServerState, node: ServerNode) -> BTreeMap<String, ProjRow> {
    match node {
        ServerNode::Endpoints => state
            .endpoints
            .iter()
            .map(|(k, d)| (k.clone(), endpoint_proj(d)))
            .collect(),
        ServerNode::Triggers => state
            .triggers
            .iter()
            .map(|(k, d)| (k.clone(), trigger_proj(d)))
            .collect(),
        ServerNode::Jobs => state
            .jobs
            .iter()
            .map(|(k, d)| (k.clone(), job_proj(d)))
            .collect(),
        ServerNode::Views => state
            .views
            .iter()
            .map(|(k, d)| (k.clone(), view_proj(d)))
            .collect(),
        ServerNode::Policies => state
            .policies
            .iter()
            .map(|(k, d)| (k.clone(), policy_proj(d)))
            .collect(),
        ServerNode::Webhooks => state
            .webhooks
            .iter()
            .map(|(k, d)| (k.clone(), webhook_proj(d)))
            .collect(),
        ServerNode::Agents => state
            .agents
            .iter()
            .map(|(k, d)| (k.clone(), agent_proj(d)))
            .collect(),
    }
}

/// The seven `/server` collections, in the fixed order the emitter and diff engine walk them.
pub(crate) const SERVER_NODES: [ServerNode; 7] = [
    ServerNode::Endpoints,
    ServerNode::Triggers,
    ServerNode::Jobs,
    ServerNode::Views,
    ServerNode::Policies,
    ServerNode::Webhooks,
    ServerNode::Agents,
];
