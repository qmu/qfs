//! Config-driven REST instances (blueprint §6/§11): the owned, vendor-free DTOs a `/rest/<api>`
//! mount is configured from — [`RestApiConfig`], [`AuthStrategy`], [`Pagination`],
//! [`ResourceMap`]. Auth, headers, base URL, and pagination are **config, not grammar**: an
//! agent reads/writes an arbitrary JSON API with zero new keywords and a small config block.
//!
//! ## Secret discipline (blueprint §8)
//! Auth is a [`SecretRef`] **indirection** — a `(driver, account)` selector — never the token
//! itself. The token is resolved through the injected [`qfs_secrets::Secrets`] handle at
//! *commit* time, never stored in this config or rendered in its `Debug`. No variant here can
//! hold key material, so the whole config is safe to `Debug`, serialize, and log.

use qfs_secrets::{ConnectionId, CredentialKey, DriverId};
use serde::{Deserialize, Serialize};

/// The codec format a `/rest/<api>` response body is decoded with (default `json`). An owned
/// id resolved against the t15 codec registry at commit time — this crate holds no
/// format-specific code.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodecId(pub String);

impl CodecId {
    /// Construct a codec id (e.g. `json`, `jsonl`).
    #[must_use]
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// The codec id as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for CodecId {
    fn default() -> Self {
        Self::new("json")
    }
}

/// A **secret reference** — a `(driver, account)` selector the auth header value is resolved
/// from at commit time (blueprint §8). It is the *only* auth coordinate that lives in config; the
/// live token never does. Resolves through [`qfs_secrets::Secrets::get`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecretRef {
    /// The secrets-store driver namespace the credential belongs to (e.g. `github`, `slack`).
    pub driver: String,
    /// The named account within that driver (e.g. `work`). Defaults to `default` if omitted.
    #[serde(default = "default_account")]
    pub account: String,
}

fn default_account() -> String {
    "default".to_string()
}

impl SecretRef {
    /// Construct a secret reference from a driver namespace + account name.
    #[must_use]
    pub fn new(driver: impl Into<String>, account: impl Into<String>) -> Self {
        Self {
            driver: driver.into(),
            account: account.into(),
        }
    }

    /// Build the [`CredentialKey`] this reference resolves to, validating the account name.
    ///
    /// # Errors
    /// Returns the secret-free error code if the account name is invalid (empty / reserved
    /// char) — surfaced as [`crate::HttpError::Auth`] so no token text is ever fabricated.
    pub fn credential_key(&self) -> Result<CredentialKey, &'static str> {
        let account = ConnectionId::new(self.account.clone()).map_err(|_| "invalid_account")?;
        Ok(CredentialKey::new(
            DriverId::new(self.driver.clone()),
            account,
        ))
    }
}

/// How a request authenticates (blueprint §11 — a **closed** sum type of capabilities). Every
/// variant references a secret by [`SecretRef`]; the live token is resolved at commit time and
/// injected into a header, never held here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[non_exhaustive]
pub enum AuthStrategy {
    /// No authentication (a public API or `http.get` probe).
    None,
    /// `Authorization: Bearer <token>` — the token resolved from `secret_ref`.
    Bearer {
        /// The secret reference the bearer token is resolved from.
        secret_ref: SecretRef,
    },
    /// A custom header `<name>: <token>` (e.g. `X-Api-Key`) — the value resolved from
    /// `secret_ref`. The `name` is config (safe to log); the value never is.
    Header {
        /// The header name to inject (e.g. `X-Api-Key`).
        name: String,
        /// The secret reference the header value is resolved from.
        secret_ref: SecretRef,
    },
    /// `Authorization: Bearer <token>` where the token is an EXISTING account provider's live
    /// credential (`AUTH ACCOUNT 'google'`) — resolved from that provider's account/vault machinery
    /// at wire time (running an OAuth refresh where the provider needs it). Wire-identical to
    /// [`AuthStrategy::Bearer`]; the difference is the coordinate — `secret_ref` names the shared
    /// PROVIDER account (`(provider, account)`), not the declared driver's own connection secret, so
    /// an OAuth service is expressible in the credential-free declared model. `provider` is config
    /// (safe to log); the resolved token never is.
    Account {
        /// The account provider whose stored credential backs the bearer (e.g. `google`, `github`).
        provider: String,
        /// The account coordinate (`(provider, account)`) the live token is resolved from.
        secret_ref: SecretRef,
    },
}

impl AuthStrategy {
    /// The [`SecretRef`] this strategy resolves, if any (`None` for [`AuthStrategy::None`]).
    #[must_use]
    pub fn secret_ref(&self) -> Option<&SecretRef> {
        match self {
            AuthStrategy::None => None,
            AuthStrategy::Bearer { secret_ref }
            | AuthStrategy::Header { secret_ref, .. }
            | AuthStrategy::Account { secret_ref, .. } => Some(secret_ref),
        }
    }
}

/// How a paginated `SELECT` follows pages (blueprint §6 — a **closed** sum type). The plan stays
/// pure: a single `HttpEffect` carries this policy and the *interpreter* drives the follow
/// loop at the edge (the genuinely-hard-part note in the ticket). `max_pages` bounds runaway
/// fetches on every strategy.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[non_exhaustive]
pub enum Pagination {
    /// Single page only — no following.
    #[default]
    None,
    /// Cursor/next-token: read `next_field` out of the JSON body; if present and non-null,
    /// re-request with it set as the `param` query parameter. Bounded by `max_pages`.
    Cursor {
        /// The response-body field holding the next cursor (e.g. `next_cursor`).
        next_field: String,
        /// The query parameter the cursor is sent back as (e.g. `cursor`).
        param: String,
        /// The hard ceiling on pages followed (runaway guard).
        max_pages: u32,
    },
    /// RFC 5988 `Link` header with `rel="next"` — follow the `next` URL verbatim. Bounded.
    LinkHeader {
        /// The hard ceiling on pages followed (runaway guard).
        max_pages: u32,
    },
}

impl Pagination {
    /// The page cap for this strategy (`1` for [`Pagination::None`]). The follow loop never
    /// fetches more than this many pages (blueprint §7 — bound runaway fetches).
    #[must_use]
    pub const fn max_pages(&self) -> u32 {
        match self {
            Pagination::None => 1,
            Pagination::Cursor { max_pages, .. } | Pagination::LinkHeader { max_pages } => {
                *max_pages
            }
        }
    }

    /// A short, stable label for the `PREVIEW` pagination note (`none`/`cursor`/`link-header`).
    #[must_use]
    pub const fn label(&self) -> &'static str {
        match self {
            Pagination::None => "none",
            Pagination::Cursor { .. } => "cursor",
            Pagination::LinkHeader { .. } => "link-header",
        }
    }
}

/// One resource within a `/rest/<api>` mount: the leading path segment that names it, the
/// universal verbs it supports, and the id field used to address a single object. Maps a path
/// segment to `{supported verbs, id field}` (blueprint §6 path→resource mapping).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceMap {
    /// The path segment that names this resource (e.g. `things`, `issues`).
    pub segment: String,
    /// The universal verbs this resource supports (`select`/`insert`/`upsert`/`remove`). A
    /// verb absent here is rejected at the parse-time capability gate (blueprint §6).
    pub verbs: Vec<RestVerb>,
    /// The response field that uniquely addresses one object (e.g. `id`) — used to build the
    /// per-object URL for `UPSERT`/`REMOVE`. Optional for read-only collections.
    #[serde(default)]
    pub id_field: Option<String>,
    /// The subset of `verbs` whose write is **irreversible** (blueprint §7/§8). A `REMOVE` is
    /// inherently irreversible regardless, but a **declared MAP** can mark an `INSERT`/`UPSERT`
    /// onto an external POST (e.g. a Slack message send) irreversible, so the planner sets
    /// [`crate::RestDriver`]'s [`Driver::write_irreversible`](qfs_driver::Driver::write_irreversible)
    /// and `PREVIEW`/`COMMIT` gate it. Empty = every write reversible (the compiled-driver default).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub irreversible_verbs: Vec<RestVerb>,
}

/// Whether a resource segment is a declared **parameter token** (`{ws}`, `{account}`) rather than
/// a literal path segment — the wildcard arm of [`RestApiConfig::resource_for_segment`] and the
/// placeholder arm of [`NodeMap`]'s template match.
fn is_param_token(segment: &str) -> bool {
    segment.len() > 2 && segment.starts_with('{') && segment.ends_with('}')
}

/// One **declared node** within a `/rest/<api>` mount: the mount-relative path *template* that
/// addresses it and the verbs that node — not its leading segment — declares (blueprint §13).
///
/// [`ResourceMap`] answers per leading segment, which is the right grain for a compiled `/rest`
/// mount (`things`, `issues`) and too coarse for a declared one: a declaration's nodes commonly
/// share a leading `{tenant}` parameter, so the segment's verb union is every verb the driver
/// declares anywhere. This table is the per-node answer, consulted first by
/// [`crate::RestDriver`]'s capability gate. A mount that declares none (every compiled config)
/// keeps the segment answer unchanged.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeMap {
    /// The node's path template **relative to the api segment** — `{ws}/files/{file}` for a node
    /// declared at `/slack/{ws}/files/{file}`, i.e. the same coordinate space the inbound
    /// `/rest/<api>/<resource…>` path reduces to. `{param}` segments match any one segment.
    pub path: String,
    /// The universal verbs this node declares (a view contributes `SELECT`; a map its own verb).
    pub verbs: Vec<RestVerb>,
    /// The subset of `verbs` whose write is **irreversible** — a declared `MAP … IRREVERSIBLE`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub irreversible_verbs: Vec<RestVerb>,
}

impl NodeMap {
    /// Construct a node map for the mount-relative template `path` declaring `verbs`.
    #[must_use]
    pub fn new(path: impl Into<String>, verbs: Vec<RestVerb>) -> Self {
        Self {
            path: path.into(),
            verbs,
            irreversible_verbs: Vec::new(),
        }
    }

    /// Builder: mark `verbs` (a subset of this node's verbs) as irreversible writes.
    #[must_use]
    pub fn with_irreversible_verbs(mut self, verbs: Vec<RestVerb>) -> Self {
        self.irreversible_verbs = verbs;
        self
    }

    /// Whether this node's template addresses `resource_path` — segment count equal, every
    /// literal segment equal, every `{param}` segment matching whatever stands there.
    #[must_use]
    pub fn matches(&self, resource_path: &str) -> bool {
        let template: Vec<&str> = self.path.trim_matches('/').split('/').collect();
        let concrete: Vec<&str> = resource_path.trim_matches('/').split('/').collect();
        template.len() == concrete.len()
            && !template.iter().any(|s| s.is_empty())
            && template
                .iter()
                .zip(concrete.iter())
                .all(|(t, c)| is_param_token(t) || t == c)
    }
}

impl ResourceMap {
    /// Construct a resource map for `segment` supporting `verbs`.
    #[must_use]
    pub fn new(segment: impl Into<String>, verbs: Vec<RestVerb>) -> Self {
        Self {
            segment: segment.into(),
            verbs,
            id_field: None,
            irreversible_verbs: Vec::new(),
        }
    }

    /// Builder: set the id field used to address a single object.
    #[must_use]
    pub fn with_id_field(mut self, field: impl Into<String>) -> Self {
        self.id_field = Some(field.into());
        self
    }

    /// Builder: mark `verbs` (a subset of the resource's supported verbs) as irreversible writes.
    #[must_use]
    pub fn with_irreversible_verbs(mut self, verbs: Vec<RestVerb>) -> Self {
        self.irreversible_verbs = verbs;
        self
    }

    /// Whether this resource declares support for `verb`.
    #[must_use]
    pub fn supports(&self, verb: RestVerb) -> bool {
        self.verbs.contains(&verb)
    }

    /// Whether a write with `verb` on this resource is irreversible (blueprint §7/§8).
    #[must_use]
    pub fn is_irreversible(&self, verb: RestVerb) -> bool {
        self.irreversible_verbs.contains(&verb)
    }
}

/// The subset of universal verbs the REST driver maps onto HTTP methods (blueprint §3). `UPDATE`
/// (PATCH) is deliberately out of scope (ticket); a new backend adds **zero** variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum RestVerb {
    /// `SELECT` → `GET`.
    Select,
    /// `INSERT` → `POST`.
    Insert,
    /// `UPSERT` → `PUT`.
    Upsert,
    /// `REMOVE` → `DELETE`.
    Remove,
}

/// The full configuration for one `/rest/<api>` mount (blueprint §6). An owned DTO with **no vendor
/// type and no secret material** — `base_url` is a string (validated at request-build time),
/// `auth` is a [`SecretRef`] indirection, and `resources` declare the path→verb mapping.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RestApiConfig {
    /// The base URL every resource path is joined onto (e.g. `https://api.example.com/v1`).
    pub base_url: String,
    /// How requests authenticate (token resolved from a [`SecretRef`] at commit time).
    #[serde(default = "default_auth")]
    pub auth: AuthStrategy,
    /// Static/templated headers added to every request (e.g. `Accept: application/json`).
    #[serde(default)]
    pub default_headers: Vec<(String, String)>,
    /// The pagination strategy for `SELECT` (default: single page).
    #[serde(default)]
    pub pagination: Pagination,
    /// The codec the response body is decoded with (default `json`).
    #[serde(default)]
    pub default_codec: CodecId,
    /// The resources (path segments) this mount exposes and the verbs each supports.
    pub resources: Vec<ResourceMap>,
    /// **Declared nodes** (blueprint §13): the per-node capability table a declared driver lifts
    /// from its own view/map declarations, keyed by each node's path template rather than by its
    /// leading segment. Empty (every compiled `/rest` mount) leaves the capability answer on
    /// `resources` exactly as before.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub nodes: Vec<NodeMap>,
    /// **Host confinement** (blueprint §13): the hosts this mount may address. `EMPTY = unconfined`
    /// (the default — compiled `/rest` and the `http.get` TVF are not host-pinned). A **declared**
    /// driver populates this with its own `AT` host, so a request to any host outside the set is a
    /// structured [`crate::HttpError::Confinement`] error before dispatch — the anti-exfiltration
    /// guarantee: an LLM-generated script is structurally unable to read one service and write to
    /// another, including after a cursor/link-header follow or an `override_url`.
    #[serde(default)]
    pub allowed_hosts: Vec<String>,
    /// **Declared pushdown** (blueprint §13.1 G2): whether this mount's declaration carries a
    /// `PUSHDOWN (…)` map, so a `WHERE` may be pushed to the wire as query parameters. The
    /// declaration data itself lives with the view (the binary's declared read facet lowers it); this
    /// flag is only what the driver ADVERTISES in its [`qfs_driver::PushdownProfile`], so the planner
    /// hands the predicate to the facet instead of filtering it locally. `false` (the default, and
    /// every compiled `/rest` mount) keeps the pre-G2 behaviour: `WHERE` stays a local residual.
    #[serde(default)]
    pub declared_where_pushdown: bool,
}

fn default_auth() -> AuthStrategy {
    AuthStrategy::None
}

impl RestApiConfig {
    /// Construct a minimal config: a base URL + the resources, with no auth, no extra
    /// headers, no pagination, and the default `json` codec. Use the builders to add detail.
    #[must_use]
    pub fn new(base_url: impl Into<String>, resources: Vec<ResourceMap>) -> Self {
        Self {
            declared_where_pushdown: false,
            base_url: base_url.into(),
            auth: AuthStrategy::None,
            default_headers: Vec::new(),
            pagination: Pagination::None,
            default_codec: CodecId::default(),
            resources,
            nodes: Vec::new(),
            allowed_hosts: Vec::new(),
        }
    }

    /// Builder: attach the per-node declared capability table (blueprint §13). Passing an empty
    /// list keeps the mount on the leading-segment answer, which is what every compiled config does.
    #[must_use]
    pub fn with_nodes(mut self, nodes: Vec<NodeMap>) -> Self {
        self.nodes = nodes;
        self
    }

    /// Builder: set the auth strategy.
    #[must_use]
    pub fn with_auth(mut self, auth: AuthStrategy) -> Self {
        self.auth = auth;
        self
    }

    /// Builder: advertise that this mount's declaration carries a §13.1 G2 `PUSHDOWN (…)` map, so
    /// the driver's profile claims `WHERE` and the planner hands the predicate to the read facet
    /// (which lowers it through the declared map and re-filters the truthful residual locally).
    #[must_use]
    pub fn with_declared_where_pushdown(mut self, declared: bool) -> Self {
        self.declared_where_pushdown = declared;
        self
    }

    /// Builder: confine this mount to `host` (blueprint §13). Adding one or more hosts switches the
    /// mount from unconfined to confined — a request to any other host is rejected before dispatch.
    #[must_use]
    pub fn with_allowed_host(mut self, host: impl Into<String>) -> Self {
        self.allowed_hosts.push(host.into());
        self
    }

    /// Builder: set a default header on every request.
    #[must_use]
    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.default_headers.push((name.into(), value.into()));
        self
    }

    /// Builder: set the pagination strategy.
    #[must_use]
    pub fn with_pagination(mut self, pagination: Pagination) -> Self {
        self.pagination = pagination;
        self
    }

    /// Builder: set the default codec.
    #[must_use]
    pub fn with_codec(mut self, codec: CodecId) -> Self {
        self.default_codec = codec;
        self
    }

    /// Resolve the [`ResourceMap`] a `/rest/<api>/<segment>/...` path names, matching the
    /// segment immediately after the api segment. Returns `None` if no resource matches.
    ///
    /// An exact match wins; failing that, a resource whose own segment is a **`{param}` token**
    /// matches any concrete segment. A declared driver aggregates its nodes under their leading
    /// segment, and that segment is a parameter whenever the service is per-tenant — Slack's every
    /// node lives under `/slack/{ws}/…`, so its ONE resource is keyed `{ws}` and no concrete
    /// workspace id could ever match it. Without this arm `capabilities()` was empty for every
    /// such path, which the plan-time verb gate reads as "this node supports nothing": measured
    /// 2026-08-17, `INSERT INTO /slack/<ws>/<channel>/messages` — a write the shipped declaration
    /// maps — was refused `unsupported_verb; supported: []`, and the refusal for a genuinely
    /// unmapped verb could name no declared verb either (ticket `20260816213014`, whose acceptance
    /// asks the refusal to list them). Reads never noticed: the read path does not consult
    /// capabilities.
    ///
    /// The match stays **leading-segment coarse**, deliberately: this layer's contract is that a
    /// driver's nodes collapse under their leading segment and their verbs aggregate there (see
    /// the cloudflare `accounts` case), and the applier still resolves a wire resource this way.
    /// Per-node precision — `REMOVE` mapped on `/slack/{ws}/files/{file}` but not on
    /// `/slack/{ws}/users` — is [`RestApiConfig::verbs_for_path`] over the [`NodeMap`] table, which
    /// the capability gate consults **first** whenever a mount declares one.
    #[must_use]
    pub fn resource_for_segment(&self, segment: &str) -> Option<&ResourceMap> {
        self.resources
            .iter()
            .find(|r| r.segment == segment)
            .or_else(|| self.resources.iter().find(|r| is_param_token(&r.segment)))
    }

    /// Whether this mount carries a per-node declared capability table ([`NodeMap`]) — true for a
    /// declared driver, false for every compiled `/rest` config, which answers per segment.
    #[must_use]
    pub fn declares_nodes(&self) -> bool {
        !self.nodes.is_empty()
    }

    /// The verbs the declared nodes addressing `resource_path` (the `/rest/<api>/…` path with the
    /// `rest` and api segments dropped) themselves declare — the per-node answer this mount's
    /// capability gate reads when [`RestApiConfig::declares_nodes`] holds. Empty means no declared
    /// node addresses that path, which is a refusal rather than a fallback.
    ///
    /// Every matching node contributes, deliberately: the apply seam picks the **first** declared
    /// map matching the path *for the verb being written*, so a verb is performable exactly when
    /// some matching node declares it. A narrower rule (most-literal-segments wins) would refuse
    /// writes the applier would then have performed, and this layer must not invent a precedence
    /// the seams below it do not have.
    #[must_use]
    pub fn verbs_for_path(&self, resource_path: &str) -> Vec<RestVerb> {
        let mut verbs: Vec<RestVerb> = Vec::new();
        for node in self.nodes.iter().filter(|n| n.matches(resource_path)) {
            for v in &node.verbs {
                if !verbs.contains(v) {
                    verbs.push(*v);
                }
            }
        }
        verbs
    }

    /// Whether a declared node addressing `resource_path` marks `verb`'s write irreversible.
    #[must_use]
    pub fn irreversible_for_path(&self, resource_path: &str, verb: RestVerb) -> bool {
        self.nodes
            .iter()
            .filter(|n| n.matches(resource_path))
            .any(|n| n.irreversible_verbs.contains(&verb))
    }
}
