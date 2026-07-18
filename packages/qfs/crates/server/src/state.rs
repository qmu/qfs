//! The server's own configuration as **owned, vendor-free data** (blueprint §10/§11).
//!
//! [`ServerState`] is the single source of truth for the running server: its endpoints,
//! triggers, jobs, views, policies, and webhooks. It is **not special** — it is the
//! state of the `/server/...` driver, mutated only by an
//! [`EffectKind::ServerConfigWrite`](qfs_core::EffectKind::ServerConfigWrite) under
//! `COMMIT` (see [`crate::driver`] / [`crate::runtime`]).
//!
//! ## Least-privilege & secrets (blueprint §8)
//! Every DTO references policies / credentials **by handle**, never an inline token. The
//! `Debug` impls are derived (the fields are handles + routes + plan ids, not secrets),
//! but [`ServerState`] is **never logged verbatim** — the runtime logs *counts* and
//! *names*, never the whole registry, so a future credential-bearing field cannot leak by
//! an incidental `{:?}`. `POLICY` rows are stored now but **enforced in t34**; until then
//! no handler runs constrained by them (documented gap).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// An opaque reference to a parsed statement / effect-plan body (blueprint §10). The runtime
/// stores the config row's plan as its **source text** — an owned, vendor-free string the
/// binding (E7) re-parses and lowers when it fires. Keeping it as text (not a live
/// `qfs_plan::Plan`) keeps [`ServerState`] `Serialize`/`Deserialize` and snapshot-stable
/// (a `Plan` carries `NodeId`s that are not a stable serialized identity).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct StatementSource(pub String);

impl StatementSource {
    /// Construct a statement source from owned text.
    #[must_use]
    pub fn new(src: impl Into<String>) -> Self {
        Self(src.into())
    }

    /// The raw statement text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// An HTTP endpoint definition (`CREATE ENDPOINT name ON 'METHOD /route' AS <query>`).
/// The t31 HTTP binding turns this into an axum route; here it is pure data.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct EndpointDef {
    /// The handler name (the config row key).
    pub name: String,
    /// The HTTP method (`GET`/`POST`/…), uppercased; empty if unspecified.
    pub method: String,
    /// The route path, e.g. `/recent`.
    pub route: String,
    /// The backing query the endpoint serves (`AS <query>`), as source text.
    pub query: StatementSource,
    /// The optional read-only-policy handle (a `/server/policies` row name) the t32 HTTP
    /// binding consults to decide whether a write-lowering query is permitted. `None`
    /// (the t31 default) means the endpoint is read-only by default — a write effect is
    /// refused. The full POLICY engine is t34; this is the registration-time gate handle.
    #[serde(default)]
    pub policy: Option<String>,
}

/// An event-trigger definition (`CREATE TRIGGER name ON <event> DO <plan>`). The t33
/// trigger poller fires `plan` when `on` matches; here it is pure data.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct TriggerDef {
    /// The trigger name (the config row key).
    pub name: String,
    /// The event this trigger fires on (raw, e.g. `inbox`); empty if unspecified.
    pub on: String,
    /// The optional `WHERE <pred>` guard (t34, CO-t31-4), as the canonical StatementSpec source
    /// (a query wrapping the predicate). Empty when the trigger declares no guard — the watchtower
    /// dispatcher treats an empty predicate as "always fire". Rehydrated (no re-parse) + evaluated
    /// over `NEW.*` at fire time.
    #[serde(default)]
    pub predicate: StatementSource,
    /// The effect-plan to run when the trigger fires (`DO <plan>`), as source text.
    pub plan: StatementSource,
    /// The attached `POLICY <name>` handle (t35): the `/server/policies` row the fired plan
    /// commits under (least privilege). `None` = no policy attached ⇒ fail-closed default-deny
    /// at fire time. Resolved against the live policy table when the trigger fires.
    #[serde(default)]
    pub policy: Option<String>,
}

/// A cron-job definition (`CREATE JOB name EVERY <interval> DO <plan>`). The t32 scheduler
/// fires `plan` every `every`; `last_run` is its persisted high-water mark (recorded by
/// t32, `None` until first fire).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct JobDef {
    /// The job name (the config row key).
    pub name: String,
    /// The cron interval, raw text (e.g. `1h`); empty if unspecified.
    pub every: String,
    /// The effect-plan to run on each fire (`DO <plan>`), as source text.
    pub plan: StatementSource,
    /// The last successful fire time as an epoch second, recorded by the t32 scheduler.
    /// `None` until the first fire (boot is replay-safe — re-applying a config preserves
    /// this only if the row carries it; a fresh INSERT leaves it `None`).
    pub last_run: Option<i64>,
    /// The attached `POLICY <name>` handle (t35): the `/server/policies` row the fired JOB plan
    /// commits under. `None` = no policy ⇒ fail-closed default-deny at fire time.
    #[serde(default)]
    pub policy: Option<String>,
}

/// One secret-free record of a JOB firing — a row of the READ-ONLY
/// `/server/jobs/<name>/runs` collection (blueprint §10). Runtime telemetry the daemon sweeper
/// appends (mapped from `qfs-watchtower`'s `CronRun`), never configuration: no plan payload, no
/// secrets — only what the audit ledger may keep.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobRunRecord {
    /// The sweep instant the firing was scheduled at (UTC epoch seconds).
    pub scheduled_at: i64,
    /// The firing outcome label: `fired` | `denied` | `blocked` | `failed`.
    pub outcome: String,
    /// The secret-free reason for a non-fired outcome; empty for a committed fire.
    pub detail: String,
    /// Effects applied by a committed fire (0 for denied/blocked/failed — atomic abort).
    pub affected: i64,
    /// The firing **principal** (blueprint §19 axis B), recorded secret-free as an IDENTITY only —
    /// `agent:<name>` for an agent-fired plan, or empty/an operator label for an ordinary
    /// (non-agent) fire. Never credential material. The sweeper (blueprint §19 axis D) threads the
    /// agent identity into this from the firing `DecisionContext`; a plain `/server/jobs` fire
    /// leaves it empty. `#[serde(default)]` so a pre-§19 record rehydrates with no principal.
    #[serde(default)]
    pub principal: String,
}

/// A view definition (`CREATE [MATERIALIZED] VIEW name AS <query>`). A materialized view
/// is the same row with `materialized = true` (t32 refreshes it); here it is pure data.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ViewDef {
    /// The view name (the config row key).
    pub name: String,
    /// The backing query (`AS <query>`), as source text.
    pub query: StatementSource,
    /// Whether this is a `MATERIALIZED VIEW` (cached + refreshed) vs a logical view.
    pub materialized: bool,
    /// The last successful refresh timestamp of a **materialized** view (epoch-ms high-water mark,
    /// the same persisted `LAST_RUN` a job records). `None` = never refreshed — the honest
    /// "freshness as data" primitive (blueprint §14 contract 2): a client reads it to compute
    /// staleness, and a never-run view reports `null`, never a fabricated timestamp. A logical
    /// (non-materialized) view is always `None` (it re-runs on read; there is nothing to stale).
    #[serde(default)]
    pub last_run: Option<i64>,
    /// The last successful materialized result snapshot, serialized as a [`qfs_core::RowBatch`]
    /// JSON value. Internal cache only: it is not part of the `/server/views` relational schema, so
    /// config reads still expose freshness through `last_run` without dumping cached row payloads.
    #[serde(default)]
    pub cache_json: Option<String>,
}

/// A least-privilege policy definition (`CREATE POLICY name`). Stored now; **enforced in
/// t34** (the capability-gating engine). `allow` lists scope handles, never credentials.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct PolicyDef {
    /// The policy name (the config row key).
    pub name: String,
    /// The handler / target this policy governs (raw name), empty if unspecified.
    pub handler: String,
    /// The allowed capability scope **handles** (e.g. `mail.read`) — never tokens (§10).
    pub allow: Vec<String>,
}

/// An inbound-webhook definition (`CREATE WEBHOOK name ON '/route'`). The t33 ingestion
/// binding registers the route; here it is pure data.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct WebhookDef {
    /// The webhook name (the config row key).
    pub name: String,
    /// The inbound route, e.g. `/hooks/x`; empty if unspecified.
    pub route: String,
    /// The optional signing-secret HANDLE (t34, blueprint §8) — a `qfs-secrets` account id the
    /// watchtower resolves BY HANDLE to verify the inbound HMAC signature. NEVER an inline token,
    /// NEVER logged. Empty for an unsigned (test/internal) webhook (ingest accepts without a
    /// signature check — a documented less-secure mode, signed is the production path).
    #[serde(default)]
    pub secret: String,
}

/// An agent-principal definition (`CREATE AGENT name [POLICY p]`, blueprint §19). An agent is a
/// new user principal (a first-class policy subject), NOT a process. This ticket lands the naming +
/// registry row only: its name (the `Subject::Agent` identity) and its optional attached POLICY
/// handle (least privilege, axis E). Query functions (axis C) and launch cadence (axis D) build on
/// this row in later tickets. Credential-free by construction.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct AgentDef {
    /// The agent name (the config row key, and the `Subject::Agent` identity).
    pub name: String,
    /// The agent's **query function** (blueprint §19 axis C): a named saved plan — the `DO <plan>`
    /// body shape WITHOUT a cadence — as canonical [`qfs_core::PlanSpec`] source. Empty for a
    /// function-less agent. Rehydrated (no re-parse) + built + gated under the agent's own subject
    /// and committed through the shipped preview/commit pipeline by `qfs agent run`.
    #[serde(default)]
    pub plan: StatementSource,
    /// The attached `POLICY <name>` handle the agent's fired plans commit under. `None` = no
    /// policy attached ⇒ fail-closed default-deny at fire time. A handle, never a credential.
    #[serde(default)]
    pub policy: Option<String>,
}

/// The running server's whole configuration — the source of truth (blueprint §7/§10). Each
/// collection is a name-keyed [`BTreeMap`] so the serialized snapshot is **deterministic**
/// (golden-testable) and `UPSERT` is a stable replace-by-name. Owned data only; no vendor
/// types, no secrets. Mutated exclusively by [`crate::driver::apply_server_write`] under
/// `COMMIT` (the purity invariant — building a `/server` write plan mutates nothing).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ServerState {
    /// `/server/endpoints` — name → endpoint.
    pub endpoints: BTreeMap<String, EndpointDef>,
    /// `/server/triggers` — name → trigger.
    pub triggers: BTreeMap<String, TriggerDef>,
    /// `/server/jobs` — name → job.
    pub jobs: BTreeMap<String, JobDef>,
    /// `/server/views` — name → view.
    pub views: BTreeMap<String, ViewDef>,
    /// `/server/policies` — name → policy.
    pub policies: BTreeMap<String, PolicyDef>,
    /// `/server/webhooks` — name → webhook.
    pub webhooks: BTreeMap<String, WebhookDef>,
    /// `/server/agents` — name → agent principal (blueprint §19).
    #[serde(default)]
    pub agents: BTreeMap<String, AgentDef>,
    /// `/server/jobs/<name>/runs` — per-job firing history (READ-ONLY runtime telemetry, not
    /// configuration: only the daemon sweeper appends via [`ServerState::record_job_run`], a
    /// replace-by-name of the job row never touches it, and removing the job drops it). Kept
    /// beside the config collections so the one shared lock serves the read facet; capped by the
    /// recorder ([`JOB_RUN_HISTORY_CAP`]) so a long-lived daemon stays bounded.
    #[serde(default)]
    pub job_runs: BTreeMap<String, Vec<JobRunRecord>>,
}

/// The per-job run-history cap: [`ServerState::record_job_run`] keeps only the newest this many
/// records (a denied job re-fires every sweep — the ruled "not stamped" semantics — so an
/// unbounded history would grow by the tick).
pub const JOB_RUN_HISTORY_CAP: usize = 50;

impl ServerState {
    /// An empty server configuration (the boot starting point).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The total number of config rows across every collection — the safe-to-log summary
    /// the runtime emits instead of the registry itself (blueprint §8: never log verbatim).
    #[must_use]
    pub fn row_count(&self) -> usize {
        self.endpoints.len()
            + self.triggers.len()
            + self.jobs.len()
            + self.views.len()
            + self.policies.len()
            + self.webhooks.len()
            + self.agents.len()
    }

    /// A one-line, secret-free summary (counts per collection) — the audit/log projection
    /// of the registry. Never includes a row's contents.
    #[must_use]
    pub fn summary(&self) -> String {
        format!(
            "endpoints={} triggers={} jobs={} views={} policies={} webhooks={} agents={}",
            self.endpoints.len(),
            self.triggers.len(),
            self.jobs.len(),
            self.views.len(),
            self.policies.len(),
            self.webhooks.len(),
            self.agents.len(),
        )
    }

    /// Append one run record to a job's `/server/jobs/<name>/runs` history, newest last,
    /// dropping the oldest records past [`JOB_RUN_HISTORY_CAP`]. The ONLY writer of `job_runs`
    /// (the daemon sweeper calls this under the state's write guard).
    pub fn record_job_run(&mut self, job: &str, record: JobRunRecord) {
        let runs = self.job_runs.entry(job.to_string()).or_default();
        runs.push(record);
        if runs.len() > JOB_RUN_HISTORY_CAP {
            let drop = runs.len() - JOB_RUN_HISTORY_CAP;
            runs.drain(..drop);
        }
    }
}
