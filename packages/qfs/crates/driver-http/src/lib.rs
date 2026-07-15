//! `qfs-driver-http` — the **generic HTTP / REST driver** (blueprint §6, t18): the escape
//! hatch for any service qfs does not model natively. It implements the t13
//! [`qfs_driver::Driver`] contract, mounted at `/rest/<api>/...`, and registers the ad-hoc
//! [`http.get`](http_get_args) table-valued function for one-off probes.
//!
//! ## The path is the type — no HTTP-verb keywords (blueprint §3)
//! The REST driver maps the **universal CRUD verbs** onto HTTP methods *internally*:
//! `SELECT→GET`, `INSERT→POST`, `UPSERT→PUT`, `REMOVE→DELETE`. There are **no** HTTP-verb
//! keywords in the DSL; auth, headers, base URL, and pagination are **config**
//! ([`RestApiConfig`]), not grammar. Combined with the t15 codec registry (`DECODE json`), an
//! agent reads/writes an arbitrary JSON API with zero new keywords and a small config block.
//!
//! ## The reusable REST seam (the base t24/t25 layer on)
//! The request/response machinery — owned [`HttpRequest`]/[`HttpResponse`] DTOs, the thin
//! [`HttpClient`] trait, auth injection from a [`qfs_secrets::Secret`], status→error
//! classification, codec decode, and pagination following — is **API-agnostic**. A specific
//! API (GitHub t24, Slack t25) supplies a [`RestApiConfig`] and reuses all of it; it does not
//! re-implement an HTTP path. `reqwest`/`url` types are **confined to this crate** and never
//! cross the [`HttpClient`] boundary (blueprint §11).
//!
//! ## Surface
//! - [`RestDriver`] — the introspective `Driver`: `mount()` = `/rest`, per-resource archetype
//!   (relational table) + open schema (JSON is dynamic), per-resource capabilities (the
//!   declared verbs), and the synchronous [`RestApplier`] via `applier()`.
//! - [`RestApplier`] — the apply leg + the reusable REST machinery; also the
//!   [`qfs_runtime::SharedApplier`] the bridge drives.
//! - [`rest_apply_driver`] — wraps the applier in a [`qfs_runtime::PlanApplierBridge`] ready to
//!   `register` into a `DriverRegistry`, so a plan over `/rest/<api>` executes end-to-end.
//! - [`http_get_args`] / [`http_get_node`] — build the `http.get(url, headers=>{...})` TVF
//!   effect (a pure read producing rows via the codec registry).
//!
//! ## Secrets (blueprint §8)
//! Auth is a [`SecretRef`](config::SecretRef) indirection resolved through the injected
//! [`qfs_secrets::Secrets`] handle at commit time; the live token is read via `Secret::expose`
//! **only** at request-build time, written into a header, and **never** logged — the request's
//! `Debug` and the structured request log redact every sensitive header (a redaction test
//! asserts this).

#![forbid(unsafe_code)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

mod applier;
pub mod client;
pub mod config;
mod effect;
mod error;
pub mod multipart;
pub mod request;

use std::sync::Arc;

use qfs_codec::{value_to_json, Codec, Row, RowBatch, Value};
use qfs_driver::{Archetype, Capabilities, Driver, NodeDesc, Path, ProcSig, PushdownProfile, Verb};
use qfs_plan::{EffectKind, EffectNode, NodeId, PlanApplier, Target, VfsPath};
use qfs_runtime::PlanApplierBridge;
use qfs_secrets::Secrets;
use qfs_types::{Column, ColumnType, DriverId, Schema};

pub use applier::RestApplier;
pub use client::{redirect_allowed, HttpClient, MockHttpClient, ReqwestClient};
pub use config::{
    AuthStrategy, CodecId, Pagination, ResourceMap, RestApiConfig, RestVerb, SecretRef,
};
pub use effect::{HttpEffect, BODY_COL, HEADER_COL_PREFIX, URL_COL};
pub use error::HttpError;
pub use multipart::{encode_multipart, http_multipart_args, MultipartBody};
pub use request::{HttpMethod, HttpRequest, HttpResponse, SENSITIVE_HEADERS};

/// The mount point the REST driver answers for. Per-`<api>` instances live under
/// `/rest/<api>/...`; the driver id is `rest`.
pub const MOUNT: &str = "/rest";

/// The generic HTTP/REST driver (blueprint §6). Owns one [`RestApiConfig`] instance + the
/// synchronous [`RestApplier`] the contract returns from `applier()`. Construct with
/// [`RestDriver::new`], injecting the HTTP client, the response codec, and the secrets handle
/// (auth is injected at construction, never on the contract surface — blueprint §6).
pub struct RestDriver {
    applier: RestApplier,
    pushdown: PushdownProfile,
    procs: Vec<ProcSig>,
}

impl RestDriver {
    /// Build a REST driver for `config`, decoding responses with `codec`, sending through
    /// `client`, and resolving auth through `secrets`. The codec is the resolved response
    /// codec (the caller looks `config.default_codec` up against the t15 codec registry).
    #[must_use]
    pub fn new(
        config: RestApiConfig,
        codec: Arc<dyn Codec>,
        client: Arc<dyn HttpClient>,
        secrets: Arc<dyn Secrets>,
    ) -> Self {
        let config = Arc::new(config);
        Self {
            applier: RestApplier::new(Arc::clone(&config), codec, client, secrets),
            // A REST API can natively filter/paginate via query params (a thin passthrough);
            // full WHERE/ORDER lowering is deferred to E3, so the driver declares only the
            // limit (pagination cap) it actually pushes today.
            pushdown: PushdownProfile::Partial {
                where_: false,
                project: false,
                limit: true,
                order: false,
                join: false,
                aggregate: false,
                distinct: false,
                group_by: false,
            },
            procs: Vec::new(),
        }
    }

    /// Borrow the synchronous applier (e.g. to drive a `qfs_plan::commit` directly, or to
    /// build the runtime bridge).
    #[must_use]
    pub fn rest_applier(&self) -> &RestApplier {
        &self.applier
    }

    /// The capability set for a `/rest/<api>/<resource>/...` node: exactly the verbs the
    /// resource declares ([`ResourceMap::verbs`]). A path that names no configured resource
    /// gets the empty set, so every verb is rejected at the parse-time gate.
    fn caps_for(&self, path: &Path) -> Capabilities {
        let Some(segment) = applier::resource_segment_of(path.as_str()) else {
            return Capabilities::none();
        };
        let Some(resource) = self.applier.config().resource_for_segment(&segment) else {
            return Capabilities::none();
        };
        let mut caps = Capabilities::none();
        for verb in &resource.verbs {
            caps = caps.with(rest_verb_to_verb(*verb));
        }
        caps
    }
}

impl Driver for RestDriver {
    fn mount(&self) -> &str {
        MOUNT
    }

    fn describe(&self, path: &Path) -> Result<NodeDesc, qfs_driver::CfsError> {
        // A REST resource is a relational table whose JSON rows are weakly typed: describe
        // returns an OPEN struct archetype (a single `json` column) rather than inventing
        // column types (blueprint §4 — irregular JSON stays a struct/json column). Pure: no I/O.
        let _ = applier::resource_segment_of(path.as_str());
        Ok(NodeDesc::new(
            Archetype::RelationalTable,
            Schema::new(vec![Column::new("value", ColumnType::Json, true)]),
        ))
    }

    fn capabilities(&self, path: &Path) -> Capabilities {
        self.caps_for(path)
    }

    fn procedures(&self) -> &[ProcSig] {
        &self.procs
    }

    fn pushdown(&self) -> &PushdownProfile {
        &self.pushdown
    }

    /// A write is irreversible iff the resource the path names marks that verb irreversible
    /// (blueprint §7/§8) — a declared MAP's `IRREVERSIBLE` flag, lifted onto the resource config.
    /// The planner ORs this onto the effect node, so `PREVIEW`/`COMMIT` gate it like a `REMOVE`.
    fn write_irreversible(&self, path: &Path, verb: Verb) -> bool {
        let Some(rv) = verb_to_rest_verb(verb) else {
            return false;
        };
        let Some(segment) = applier::resource_segment_of(path.as_str()) else {
            return false;
        };
        self.applier
            .config()
            .resource_for_segment(&segment)
            .is_some_and(|r| r.is_irreversible(rv))
    }

    fn applier(&self) -> &dyn PlanApplier {
        &self.applier
    }
}

/// Map a configured [`RestVerb`] onto the universal [`Verb`] the capability gate speaks.
fn rest_verb_to_verb(v: RestVerb) -> Verb {
    match v {
        RestVerb::Select => Verb::Select,
        RestVerb::Insert => Verb::Insert,
        RestVerb::Upsert => Verb::Upsert,
        RestVerb::Remove => Verb::Remove,
    }
}

/// The inverse of [`rest_verb_to_verb`] for the write verbs the REST driver maps
/// (`INSERT`/`UPSERT`/`REMOVE`). Every other universal verb (`SELECT`, `UPDATE`, the blob verbs)
/// has no REST counterpart and returns `None` — those are never resource-declared irreversible here.
fn verb_to_rest_verb(v: Verb) -> Option<RestVerb> {
    match v {
        Verb::Insert => Some(RestVerb::Insert),
        Verb::Upsert => Some(RestVerb::Upsert),
        Verb::Remove => Some(RestVerb::Remove),
        _ => None,
    }
}

/// Wrap a [`RestDriver`]'s synchronous applier in the runtime [`PlanApplierBridge`], yielding
/// the async `ApplyDriver` ready to `register` into a `DriverRegistry` under the driver's id
/// (`rest`). A plan routed to `/rest/<api>` then executes end-to-end through the t10
/// interpreter, which dispatches each effect to this bridge.
#[must_use]
pub fn rest_apply_driver(driver: &RestDriver) -> PlanApplierBridge<RestApplier> {
    PlanApplierBridge::new(Arc::new(driver.rest_applier().clone()))
}

/// Read the rows a `/rest/<api>/<resource>` GET yields — for a §13 declared-driver read facet. Builds
/// a `Read` effect node over `vfs_path` and drives the applier's GET → decode (following pagination,
/// injecting auth, and enforcing host confinement, exactly like a committed read).
///
/// # Errors
/// The structured [`HttpError`] on a transport / auth / decode / confinement failure.
pub fn rest_read_rows(
    applier: &RestApplier,
    vfs_path: &str,
) -> Result<qfs_codec::RowBatch, HttpError> {
    let node = qfs_plan::EffectNode::new(
        qfs_plan::NodeId(0),
        qfs_plan::EffectKind::Read,
        qfs_plan::Target::new(
            qfs_plan::DriverId::new("rest"),
            qfs_plan::VfsPath::new(vfs_path),
        ),
    );
    applier.read(&node)
}

/// Build the [`RowBatch`] args for the `http.get(url, headers=>{...})` table-valued function:
/// a single row carrying the absolute URL under [`URL_COL`] and each header under a
/// `__http_h:<name>` column (see [`effect`]). The evaluator (E1) builds this from the TVF
/// call; the applier reads it back and issues a no-config `GET`.
#[must_use]
pub fn http_get_args(url: impl Into<String>, headers: &[(String, String)]) -> RowBatch {
    let mut columns = vec![Column::new(URL_COL, ColumnType::Text, false)];
    let mut values = vec![Value::Text(url.into())];
    for (name, value) in headers {
        columns.push(Column::new(
            format!("{HEADER_COL_PREFIX}{name}"),
            ColumnType::Text,
            false,
        ));
        values.push(Value::Text(value.clone()));
    }
    RowBatch::new(Schema::new(columns), vec![Row::new(values)])
}

/// Build the single-row [`RowBatch`] args carrying a **pre-encoded wire body** under [`BODY_COL`] —
/// the §13 declared-map write path (`qfs_exec::declared::eval_map_body`) evaluated a `VALUES
/// (<expr>)` map into a wire [`Value`]; this encodes it to the JSON object the applier POSTs
/// verbatim (`HttpEffect::from_node` reads the body back from `__http_body`). The encode is the
/// clean, untagged [`value_to_json`] (a struct → a JSON object), not the codec's row-array encode.
#[must_use]
pub fn http_body_args(body: &Value) -> RowBatch {
    let bytes = serde_json::to_vec(&value_to_json(body)).unwrap_or_default();
    RowBatch::new(
        Schema::new(vec![Column::new(BODY_COL, ColumnType::Bytes, false)]),
        vec![Row::new(vec![Value::Bytes(bytes)])],
    )
}

/// Build the full `http.get` effect node (a pure `Read` producing rows via the codec). The
/// node carries the URL + headers in its args; the applier sends one `GET` and decodes the
/// response. `id` is the plan-local node identity.
#[must_use]
pub fn http_get_node(
    id: NodeId,
    url: impl Into<String>,
    headers: &[(String, String)],
) -> EffectNode {
    let target = Target::new(DriverId::new("rest"), VfsPath::new("/rest/_http/get"));
    EffectNode::new(id, EffectKind::Read, target).with_args(http_get_args(url, headers))
}

#[cfg(test)]
mod tests;
