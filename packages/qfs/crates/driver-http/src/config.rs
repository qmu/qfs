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
    /// **Host confinement** (blueprint §13): the hosts this mount may address. `EMPTY = unconfined`
    /// (the default — compiled `/rest` and the `http.get` TVF are not host-pinned). A **declared**
    /// driver populates this with its own `AT` host, so a request to any host outside the set is a
    /// structured [`crate::HttpError::Confinement`] error before dispatch — the anti-exfiltration
    /// guarantee: an LLM-generated script is structurally unable to read one service and write to
    /// another, including after a cursor/link-header follow or an `override_url`.
    #[serde(default)]
    pub allowed_hosts: Vec<String>,
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
            base_url: base_url.into(),
            auth: AuthStrategy::None,
            default_headers: Vec::new(),
            pagination: Pagination::None,
            default_codec: CodecId::default(),
            resources,
            allowed_hosts: Vec::new(),
        }
    }

    /// Builder: set the auth strategy.
    #[must_use]
    pub fn with_auth(mut self, auth: AuthStrategy) -> Self {
        self.auth = auth;
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
    #[must_use]
    pub fn resource_for_segment(&self, segment: &str) -> Option<&ResourceMap> {
        self.resources.iter().find(|r| r.segment == segment)
    }
}
