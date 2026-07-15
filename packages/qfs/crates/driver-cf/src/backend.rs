//! [`CfBackend`] — the thin, **mockable** Cloudflare transport seam (blueprint §11 no-heavy-SDK,
//! boundary B3), plus the owned DTOs it trades in, the real [`HttpApiBackend`] (REST over a local
//! [`HttpExchange`] seam — the `qfs-google-auth` precedent, so this crate does not depend on
//! `qfs-driver-http` and stays an independent runtime leaf), and [`MockCfBackend`] (in-memory
//! fixtures for tests — no live Cloudflare, no network).
//!
//! The trait trades **only** in owned, vendor-free DTOs; Cloudflare JSON and `worker::*` env
//! bindings never cross it. The D1 leg ships the **already-rendered** `(sql, params)` from the
//! reused t17 sqlite emitter with `params` as a **structured bound array** — never interpolated
//! into the SQL — the headline injection-safety obligation an HTTP backend carries (the t17
//! Architect flagged this). The D1 `/batch` endpoint maps one [`CfBackend::d1_batch`] to one
//! atomic transaction.
//!
//! ## Token discipline (blueprint §8)
//! The Cloudflare API token is a [`qfs_secrets::Secret`] resolved at construction and written
//! into the `Authorization: Bearer` header. It is **never** logged (the [`HttpRequest`] `Debug`
//! redacts the header via the shared `qfs-http-core` authority), never stored in a DTO, never in
//! a [`CfError`].
//!
//! ## Dual transport (named park)
//! The wasm `WorkersBindingBackend` (native `worker` env bindings) is **parked**: there is no
//! live wasm CI lane yet, and the DTOs + this seam are wasm-clean so the binding impl drops in
//! later behind the identical trait, producing identical DTOs (the ticket's conformance goal).

use std::sync::{Arc, Mutex};

use qfs_http_core::{HttpMethod, HttpRequest, HttpResponse};
use qfs_secrets::Secret;
use qfs_sql_core::Param;
use qfs_types::{Row, Value};

use crate::error::CfError;

/// A Cloudflare D1 database UUID. Distinct from path names and KV namespace ids so discovery
/// cannot accidentally route one resource kind through another endpoint.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct D1DatabaseUuid(String);

impl D1DatabaseUuid {
    /// Construct a D1 database UUID value.
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Borrow the UUID as the Cloudflare API path segment.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A Cloudflare KV namespace id. Separate from D1 UUIDs despite the shared string representation.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct KvNamespaceId(String);

impl KvNamespaceId {
    /// Construct a KV namespace id value.
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Borrow the id as the Cloudflare API path segment.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A Cloudflare Queue name. Queues are addressed by name in the current REST surface.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct QueueName(String);

impl QueueName {
    /// Construct a queue name value.
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Borrow the queue name as the Cloudflare API path segment.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// One discovered D1 database: qfs registers it under `name`, but Cloudflare API calls use `uuid`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct D1DatabaseResource {
    /// Human-facing D1 database name, used in `/cf/d1/<name>/...`.
    pub name: String,
    /// Cloudflare's D1 `uuid` field, used for D1 API calls.
    pub uuid: D1DatabaseUuid,
}

/// One discovered KV namespace: qfs registers it under `title`, but API calls use `id`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KvNamespaceResource {
    /// Human-facing namespace title, used in `/cf/kv/<title>`.
    pub title: String,
    /// Cloudflare's KV namespace `id` field, used for KV API calls.
    pub id: KvNamespaceId,
}

/// One discovered Cloudflare Queue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueResource {
    /// The Cloudflare `queue_name` field, used in `/cf/queue/<queue_name>` and API calls.
    pub queue_name: QueueName,
}

/// One Cloudflare account visible to the API token. The id is the non-secret account locator qfs
/// persists in `path_binding.at_locator`; the name is for operator disambiguation only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountResource {
    /// The Cloudflare account id.
    pub id: String,
    /// The human-facing Cloudflare account name.
    pub name: String,
}

/// A Cloudflare Artifacts repository key. The namespace and repository name are distinct from the
/// opaque repo id so path addressing cannot be mixed with API identity.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ArtifactRepoKey {
    /// The Artifacts namespace.
    pub namespace: String,
    /// The repository name inside the namespace.
    pub name: String,
}

impl ArtifactRepoKey {
    /// Construct an Artifacts repo key.
    #[must_use]
    pub fn new(namespace: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            namespace: namespace.into(),
            name: name.into(),
        }
    }
}

/// One Artifacts repository row. Owned and vendor-free; the plaintext repo token is deliberately
/// not a field and never appears in the table surface.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct ArtifactRepo {
    /// The Artifacts namespace.
    pub namespace: String,
    /// The repository name.
    pub name: String,
    /// Cloudflare's opaque repository id.
    pub id: String,
    /// The Git remote URL returned by Artifacts.
    pub remote_url: String,
    /// Creation timestamp, when returned by the API.
    pub created_at: Option<String>,
    /// Last update timestamp, when returned by the API.
    pub updated_at: Option<String>,
    /// Last push timestamp, when returned by the API.
    pub last_push_at: Option<String>,
    /// Optional repository description.
    pub description: Option<String>,
    /// Default branch name.
    pub default_branch: Option<String>,
    /// Source/fork/import marker, if Cloudflare returns one.
    pub source: Option<String>,
    /// Whether the repository is read-only.
    pub read_only: bool,
}

impl ArtifactRepo {
    /// The `(namespace, name)` key for this repo.
    #[must_use]
    pub fn key(&self) -> ArtifactRepoKey {
        ArtifactRepoKey::new(self.namespace.clone(), self.name.clone())
    }

    /// Project this repository onto the fixed `/cf/artifacts` relational schema. Token-free by
    /// construction.
    #[must_use]
    pub fn to_row(&self) -> Row {
        Row::new(vec![
            Value::Text(self.namespace.clone()),
            Value::Text(self.name.clone()),
            Value::Text(self.id.clone()),
            Value::Text(self.remote_url.clone()),
            opt_text(self.created_at.as_ref()),
            opt_text(self.updated_at.as_ref()),
            opt_text(self.last_push_at.as_ref()),
            opt_text(self.description.as_ref()),
            opt_text(self.default_branch.as_ref()),
            opt_text(self.source.as_ref()),
            Value::Bool(self.read_only),
        ])
    }
}

fn opt_text(value: Option<&String>) -> Value {
    value.map(|v| Value::Text(v.clone())).unwrap_or(Value::Null)
}

/// The non-secret fields qfs can send when creating an Artifacts repo. The API may return a repo
/// token, but that token is carried only as [`Secret`] in [`CreatedArtifactRepo`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct CreateArtifactRepoRequest {
    /// The repository name.
    pub name: String,
    /// Optional description.
    pub description: Option<String>,
    /// Optional default branch.
    pub default_branch: Option<String>,
    /// Optional read-only flag.
    pub read_only: Option<bool>,
}

/// The result of creating an Artifacts repo: a non-secret repo row plus the plaintext token that
/// must be sealed before the operation is considered successful.
pub struct CreatedArtifactRepo {
    /// The created repository row.
    pub repo: ArtifactRepo,
    /// The repo-scoped Git token returned by Cloudflare. Never cloneable, never formatted.
    pub token: Secret,
}

/// Seals the repo-scoped Artifacts token into the qfs vault (or a mock vault in tests). This seam
/// exists outside [`CfBackend`] so the Cloudflare transport never learns where qfs stores secrets.
pub trait ArtifactTokenSealer: Send + Sync {
    /// Fail closed before the remote create call if this process cannot persist a returned token.
    ///
    /// # Errors
    /// [`CfError`] when no writable secret store is available.
    fn ensure_can_seal(&self) -> Result<(), CfError>;

    /// Seal the plaintext repo token under the deterministic repo key.
    ///
    /// # Errors
    /// [`CfError`] when sealing fails. Implementations must not include the token value in errors.
    fn seal(&self, key: &ArtifactRepoKey, token: Secret) -> Result<(), CfError>;
}

/// A test/describe sealer that records no token and always succeeds. Production wiring uses a
/// vault-backed sealer instead.
#[derive(Debug, Default)]
pub struct NoopArtifactTokenSealer;

impl ArtifactTokenSealer for NoopArtifactTokenSealer {
    fn ensure_can_seal(&self) -> Result<(), CfError> {
        Ok(())
    }

    fn seal(&self, _key: &ArtifactRepoKey, _token: Secret) -> Result<(), CfError> {
        Ok(())
    }
}

/// A transport failure before an HTTP status was received — secret-free (built from the request
/// shape only). The error half of the local [`HttpExchange`] seam (mirrors the t19 auth base's
/// `TransportError`); kept local so this crate does NOT depend on `qfs-driver-http` (a runtime
/// leaf) and stays an independent runtime leaf.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("transport error for {method} {url}: {reason}")]
pub struct TransportError {
    /// The HTTP method (uppercase token).
    pub method: String,
    /// The request URL (secret-free).
    pub url: String,
    /// A secret-free reason (the transport's class, never a header value).
    pub reason: String,
}

/// The thin **synchronous** transport seam the [`HttpApiBackend`] sends owned [`HttpRequest`]s
/// over (the `qfs-google-auth` `HttpExchange` precedent). A non-2xx status is **not** an error —
/// it rides in the [`HttpResponse`] so the backend classifies it (e.g. a 404 KV miss). The
/// production binary adapts an `Arc<dyn qfs_driver_http::HttpClient>` to this with a trivial DTO
/// copy; `reqwest` stays confined in `qfs-driver-http` and never crosses this boundary (blueprint §11).
///
/// `Send + Sync` so an `Arc<dyn HttpExchange>` can be shared across the runtime bridge's blocking
/// apply threads.
pub trait HttpExchange: Send + Sync {
    /// Execute one request synchronously.
    ///
    /// # Errors
    /// [`TransportError`] if the wire exchange fails before a status is received.
    fn exchange(&self, req: &HttpRequest) -> Result<HttpResponse, TransportError>;
}

impl HttpExchange for Arc<dyn HttpExchange> {
    fn exchange(&self, req: &HttpRequest) -> Result<HttpResponse, TransportError> {
        (**self).exchange(req)
    }
}

/// An in-memory mock transport (tests / CI / wasm): records every request and answers from a FIFO
/// queue of scripted responses — so a test asserts the exact request shape the backend built
/// (method, URL, headers, body) **without any socket**. Mirrors the t19 `MockExchange`.
#[derive(Default)]
pub struct MockExchange {
    responses: Mutex<std::collections::VecDeque<Result<HttpResponse, TransportError>>>,
    recorded: Mutex<Vec<HttpRequest>>,
}

impl MockExchange {
    /// An empty mock (every `exchange` after the queue drains returns a terminal transport error).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Queue one scripted success response (consumed FIFO).
    #[must_use]
    pub fn with_response(self, resp: HttpResponse) -> Self {
        if let Ok(mut q) = self.responses.lock() {
            q.push_back(Ok(resp));
        }
        self
    }

    /// The requests this mock received, in order — what a test asserts against.
    #[must_use]
    pub fn recorded(&self) -> Vec<HttpRequest> {
        self.recorded.lock().map(|r| r.clone()).unwrap_or_default()
    }
}

impl HttpExchange for MockExchange {
    fn exchange(&self, req: &HttpRequest) -> Result<HttpResponse, TransportError> {
        if let Ok(mut r) = self.recorded.lock() {
            r.push(req.clone());
        }
        let next = self.responses.lock().ok().and_then(|mut q| q.pop_front());
        next.unwrap_or_else(|| {
            Err(TransportError {
                method: req.method.as_str().to_string(),
                url: req.url.clone(),
                reason: "mock exhausted: no scripted response".to_string(),
            })
        })
    }
}

/// One KV entry — the owned DTO a `kv_get`/`kv_list` yields and a `kv_put` carries. Owned,
/// vendor-free. `value` is the stored bytes; `metadata` is the small JSON-ish blob Cloudflare KV
/// stores alongside a value; `expiration_ttl` is the optional TTL (seconds) a put requests.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct KvEntry {
    /// The entry key.
    pub key: String,
    /// The stored value bytes (the body of a `kv_get` / the body to `kv_put`).
    pub value: Vec<u8>,
    /// The optional metadata string Cloudflare KV stores next to the value (JSON text).
    pub metadata: Option<String>,
    /// The optional TTL (seconds) a `kv_put` requests; `None` for a non-expiring write.
    pub expiration_ttl: Option<u64>,
}

impl KvEntry {
    /// Construct a KV entry with just a key + value (no metadata/TTL).
    #[must_use]
    pub fn new(key: impl Into<String>, value: Vec<u8>) -> Self {
        Self {
            key: key.into(),
            value,
            metadata: None,
            expiration_ttl: None,
        }
    }

    /// Builder: attach a metadata string.
    #[must_use]
    pub fn with_metadata(mut self, metadata: impl Into<String>) -> Self {
        self.metadata = Some(metadata.into());
        self
    }

    /// Builder: attach a TTL (seconds).
    #[must_use]
    pub fn with_ttl(mut self, ttl: u64) -> Self {
        self.expiration_ttl = Some(ttl);
        self
    }

    /// Project this entry onto the degenerate `(key, value)` table row (the KV-as-table view).
    /// `value` is surfaced as `Text` when it is valid UTF-8, else `Bytes`.
    #[must_use]
    pub fn to_kv_row(&self) -> Row {
        let value = match String::from_utf8(self.value.clone()) {
            Ok(text) => Value::Text(text),
            Err(_) => Value::Bytes(self.value.clone()),
        };
        Row::new(vec![Value::Text(self.key.clone()), value])
    }
}

/// A queue message id — the owned handle a `queue_send` returns (the Cloudflare-assigned id).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct MsgId(pub String);

impl MsgId {
    /// Construct a message id.
    #[must_use]
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// The id as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// One pulled queue message — the owned DTO a `queue_pull` (tail) yields. Owned, vendor-free.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct QueueMsg {
    /// The Cloudflare-assigned message id.
    pub id: String,
    /// The message body bytes.
    pub body: Vec<u8>,
    /// How many delivery attempts this message has had (at-least-once delivery — blueprint §7).
    pub attempts: u32,
}

impl QueueMsg {
    /// Construct a queue message.
    #[must_use]
    pub fn new(id: impl Into<String>, body: Vec<u8>, attempts: u32) -> Self {
        Self {
            id: id.into(),
            body,
            attempts,
        }
    }

    /// Project this message onto the queue tail row `(id, body, attempts)`.
    #[must_use]
    pub fn to_queue_row(&self) -> Row {
        let body = match String::from_utf8(self.body.clone()) {
            Ok(text) => Value::Text(text),
            Err(_) => Value::Bytes(self.body.clone()),
        };
        Row::new(vec![
            Value::Text(self.id.clone()),
            body,
            Value::Int(i64::from(self.attempts)),
        ])
    }
}

/// The Cloudflare transport seam every backend implements (blueprint §11). The driver's D1 SQL
/// compile/emit (reused from t17), the KV blob verbs, and the queue append/tail are written once
/// against this trait; the REST [`HttpApiBackend`] and the parked wasm binding backend are
/// interchangeable impls producing **identical** owned DTOs. No `reqwest`/`worker` type crosses
/// this boundary.
///
/// `Send + Sync` so a backend can be shared across the runtime bridge's blocking apply threads.
pub trait CfBackend: Send + Sync {
    /// List the Cloudflare accounts visible to the configured token.
    ///
    /// # Errors
    /// [`CfError`] on a non-2xx status, decode failure, or transport failure.
    fn list_accounts(&self) -> Result<Vec<AccountResource>, CfError>;

    /// Discover D1 databases for the configured account.
    ///
    /// # Errors
    /// [`CfError`] on a non-2xx status, decode failure, or transport failure.
    fn list_d1_databases(&self) -> Result<Vec<D1DatabaseResource>, CfError>;

    /// Discover KV namespaces for the configured account.
    ///
    /// # Errors
    /// [`CfError`] on a non-2xx status, decode failure, or transport failure.
    fn list_kv_namespaces(&self) -> Result<Vec<KvNamespaceResource>, CfError>;

    /// Discover Queues for the configured account.
    ///
    /// # Errors
    /// [`CfError`] on a non-2xx status, decode failure, or transport failure.
    fn list_queues(&self) -> Result<Vec<QueueResource>, CfError>;

    /// Discover Artifacts namespaces for the configured account. A successful empty list still
    /// proves the Artifacts control-plane route is reachable for the token.
    ///
    /// # Errors
    /// [`CfError`] on a non-2xx status, decode failure, or transport failure.
    fn list_artifact_namespaces(&self) -> Result<Vec<String>, CfError>;

    /// List repositories inside one Artifacts namespace.
    ///
    /// # Errors
    /// [`CfError`] on a non-2xx status, decode failure, or transport failure.
    fn list_artifact_repos(&self, namespace: &str) -> Result<Vec<ArtifactRepo>, CfError>;

    /// Read one Artifacts repository by `(namespace, name)`.
    ///
    /// # Errors
    /// [`CfError`] on a non-2xx status, decode failure, or transport failure.
    fn get_artifact_repo(
        &self,
        namespace: &str,
        name: &str,
    ) -> Result<Option<ArtifactRepo>, CfError>;

    /// Create an Artifacts repository, returning the repo row plus the plaintext Git token that
    /// the applier must pass directly to [`ArtifactTokenSealer`].
    ///
    /// # Errors
    /// [`CfError`] on a non-2xx status, decode failure, or transport failure.
    fn create_artifact_repo(
        &self,
        namespace: &str,
        request: &CreateArtifactRepoRequest,
    ) -> Result<CreatedArtifactRepo, CfError>;

    /// Delete an Artifacts repository by `(namespace, name)`.
    ///
    /// # Errors
    /// [`CfError`] on a non-2xx status, decode failure, or transport failure.
    fn delete_artifact_repo(&self, namespace: &str, name: &str) -> Result<(), CfError>;

    /// Execute a **parameterized** D1 query and return owned rows. `sql` is the t17
    /// sqlite-rendered statement (only `?` placeholders + quoted identifiers); `params` is the
    /// **structured bound array** the backend binds positionally — it is **never** interpolated
    /// into `sql` (the injection-safety invariant). Used for SELECT.
    ///
    /// # Errors
    /// [`CfError`] on a non-2xx status, a decode failure, or a transport failure.
    fn d1_query(&self, db: &str, sql: &str, params: &[Param]) -> Result<Vec<Row>, CfError>;

    /// Apply a batch of parameterized D1 statements inside **one atomic transaction** via the
    /// Cloudflare D1 `/batch` endpoint (the D1 ACID story — D1 has no interactive BEGIN/COMMIT,
    /// so a batch IS the transaction). Returns the total affected row count.
    ///
    /// # Errors
    /// [`CfError`] (the batch rolled back) on any statement failure.
    fn d1_batch(&self, db: &str, statements: &[(String, Vec<Param>)]) -> Result<u64, CfError>;

    /// Fetch a single KV entry by key (`None` if absent).
    ///
    /// # Errors
    /// [`CfError`] on a non-2xx (other than 404) status or a transport failure.
    fn kv_get(&self, ns: &str, key: &str) -> Result<Option<KvEntry>, CfError>;

    /// Create-or-replace a KV entry (the retry-safe `UPSERT`/`cp` write).
    ///
    /// # Errors
    /// [`CfError`] on a non-2xx status or a transport failure.
    fn kv_put(&self, ns: &str, entry: &KvEntry) -> Result<(), CfError>;

    /// Delete a KV entry by key (idempotent — deleting an absent key is success).
    ///
    /// # Errors
    /// [`CfError`] on a non-2xx status or a transport failure.
    fn kv_delete(&self, ns: &str, key: &str) -> Result<(), CfError>;

    /// List keys in a namespace, optionally filtered by `prefix`, capped at `limit`.
    ///
    /// # Errors
    /// [`CfError`] on a non-2xx status or a transport failure.
    fn kv_list(
        &self,
        ns: &str,
        prefix: Option<&str>,
        limit: Option<u32>,
    ) -> Result<Vec<String>, CfError>;

    /// Send (append) one message to a queue. `idempotency_key` makes at-least-once retries
    /// safe — Cloudflare de-dupes a resend carrying the same key, so no double-append (blueprint §7).
    /// Returns the assigned message id.
    ///
    /// # Errors
    /// [`CfError`] on a non-2xx status or a transport failure.
    fn queue_send(&self, queue: &str, body: &[u8], idempotency_key: &str)
        -> Result<MsgId, CfError>;

    /// Pull (tail) up to `max` recent messages from a queue (consumer pull). Bounded-tail only —
    /// Queues is not random-access, so there is no WHERE/offset (capabilities advertise exactly
    /// that).
    ///
    /// # Errors
    /// [`CfError`] on a non-2xx status or a transport failure.
    fn queue_pull(&self, queue: &str, max: u32) -> Result<Vec<QueueMsg>, CfError>;
}

/// The Cloudflare REST API base. Real account routing (`/accounts/<id>/...`) is config the
/// caller injects via [`HttpApiBackend::new`]; the op paths below are appended to it.
const API_BASE: &str = "https://api.cloudflare.com/client/v4";

/// The real Cloudflare REST backend: builds owned [`HttpRequest`]s and sends them over the local
/// [`HttpExchange`] seam (the production binary adapts a confined `qfs_driver_http::HttpClient` to
/// it; reqwest stays inside that crate). The API token is a [`qfs_secrets::Secret`] written into
/// the `Authorization: Bearer` header at request-build time and **never** logged.
pub struct HttpApiBackend {
    exchange: Arc<dyn HttpExchange>,
    account_id: String,
    token: Secret,
}

impl HttpApiBackend {
    /// Build a REST backend over `exchange`, routing to Cloudflare account `account_id`, bearing
    /// the resolved API `token` (a [`Secret`]). The token is held only to inject the bearer
    /// header; it is never rendered.
    #[must_use]
    pub fn new(
        exchange: Arc<dyn HttpExchange>,
        account_id: impl Into<String>,
        token: Secret,
    ) -> Self {
        Self {
            exchange,
            account_id: account_id.into(),
            token,
        }
    }

    /// Build a REST backend for token-only calls such as `GET /accounts`, before qfs knows which
    /// account id to persist. Account-scoped methods must not be called on this value.
    #[must_use]
    pub fn for_token(exchange: Arc<dyn HttpExchange>, token: Secret) -> Self {
        Self {
            exchange,
            account_id: String::new(),
            token,
        }
    }

    /// Build a request with the bearer + content-type headers attached. The token is exposed
    /// **only** here, at request-build time, into a header the redacting `HttpRequest` `Debug`
    /// hides — it never reaches a log line or a `CfError`.
    fn authed(&self, method: HttpMethod, url: String) -> HttpRequest {
        let bearer = format!("Bearer {}", self.token.expose_str().unwrap_or_default());
        HttpRequest::new(method, url)
            .header("Authorization", bearer)
            .header("Content-Type", "application/json")
    }

    /// Send a request, mapping a transport failure to a secret-free [`CfError::Transport`] and a
    /// non-2xx status to [`CfError::Api`] under `op`.
    fn send(&self, op: &'static str, req: &HttpRequest) -> Result<HttpResponse, CfError> {
        let resp = self
            .exchange
            .exchange(req)
            .map_err(|_| CfError::Transport {
                reason: "cloudflare request could not be completed".to_string(),
            })?;
        if resp.is_success() {
            Ok(resp)
        } else {
            Err(CfError::Api {
                op,
                status: resp.status,
            })
        }
    }

    /// Parse a response body as JSON, mapping a failure to a secret-free decode error.
    fn parse_json(op: &'static str, resp: &HttpResponse) -> Result<serde_json::Value, CfError> {
        serde_json::from_slice(&resp.body).map_err(|_| CfError::Decode {
            op,
            reason: "response body was not valid JSON".to_string(),
        })
    }

    /// The D1 database REST path for a `(db)` under the configured account.
    fn d1_path(&self, db: &str, suffix: &str) -> String {
        format!(
            "{API_BASE}/accounts/{}/d1/database/{db}{suffix}",
            self.account_id
        )
    }

    /// The D1 database collection REST path under the configured account.
    fn d1_collection_path(&self) -> String {
        format!("{API_BASE}/accounts/{}/d1/database", self.account_id)
    }

    /// The KV namespace collection REST path under the configured account.
    fn kv_collection_path(&self) -> String {
        format!(
            "{API_BASE}/accounts/{}/storage/kv/namespaces",
            self.account_id
        )
    }

    /// The Queue collection REST path under the configured account.
    fn queue_collection_path(&self) -> String {
        format!("{API_BASE}/accounts/{}/queues", self.account_id)
    }

    /// The Artifacts namespace collection REST path under the configured account.
    fn artifact_namespaces_path(&self) -> String {
        format!(
            "{API_BASE}/accounts/{}/artifacts/namespaces",
            self.account_id
        )
    }

    /// The Artifacts namespace REST path under the configured account.
    fn artifact_namespace_path(&self, namespace: &str, suffix: &str) -> String {
        format!(
            "{API_BASE}/accounts/{}/artifacts/namespaces/{namespace}{suffix}",
            self.account_id
        )
    }

    /// The token-scoped Cloudflare accounts collection path.
    fn accounts_path() -> String {
        format!("{API_BASE}/accounts")
    }

    /// The KV namespace REST path.
    fn kv_path(&self, ns: &str, suffix: &str) -> String {
        format!(
            "{API_BASE}/accounts/{}/storage/kv/namespaces/{ns}{suffix}",
            self.account_id
        )
    }

    /// The Queues REST path.
    fn queue_path(&self, queue: &str, suffix: &str) -> String {
        format!(
            "{API_BASE}/accounts/{}/queues/{queue}{suffix}",
            self.account_id
        )
    }
}

/// Render a bound [`Param`] into the JSON scalar the D1 REST `params` array carries. This is the
/// **structured bound array** form — the value rides in JSON `params`, NEVER in the SQL text, so
/// a `'; DROP TABLE t; --` literal is inert data (the injection-safety invariant).
#[must_use]
pub fn param_to_json(param: &Param) -> serde_json::Value {
    match param {
        Param::Null => serde_json::Value::Null,
        Param::Bool(b) => serde_json::Value::Bool(*b),
        Param::Int(n) => serde_json::Value::from(*n),
        Param::Float(f) => serde_json::Value::from(*f),
        Param::Text(t) => serde_json::Value::String(t.clone()),
        // D1 binds bytes as base64-ish text via the JSON array; we surface the lossy UTF-8 view
        // for a text-y blob, else a JSON array of byte ints — still a single bound array element,
        // never interpolated.
        Param::Bytes(b) => match std::str::from_utf8(b) {
            Ok(s) => serde_json::Value::String(s.to_string()),
            Err(_) => serde_json::Value::Array(
                b.iter()
                    .map(|byte| serde_json::Value::from(*byte))
                    .collect(),
            ),
        },
    }
}

/// Decode the D1 `result[0].results` rows (an array of JSON objects) into owned [`Row`]s. The
/// column order follows the first object's key order (D1 returns named columns); a `null`
/// becomes [`Value::Null`]. Vendor-free — no D1 type crosses.
fn decode_d1_rows(json: &serde_json::Value) -> Vec<Row> {
    let Some(results) = json
        .get("result")
        .and_then(|r| r.as_array())
        .and_then(|arr| arr.first())
        .and_then(|first| first.get("results"))
        .and_then(|r| r.as_array())
    else {
        return Vec::new();
    };
    results.iter().filter_map(json_object_to_row).collect()
}

/// Project one D1 JSON object into a [`Row`]. D1 returns named JSON objects, not arrays; when the
/// query aliases its SELECT list as `c0`, `c1`, ... use that numeric order so object-key ordering
/// cannot scramble the row.
fn json_object_to_row(obj: &serde_json::Value) -> Option<Row> {
    let map = obj.as_object()?;
    let mut indexed = Vec::new();
    for (key, value) in map {
        let Some(raw) = key.strip_prefix('c') else {
            indexed.clear();
            break;
        };
        let Ok(idx) = raw.parse::<usize>() else {
            indexed.clear();
            break;
        };
        indexed.push((idx, json_to_value(value)));
    }
    if indexed.len() == map.len() && !indexed.is_empty() {
        indexed.sort_by_key(|(idx, _)| *idx);
        return Some(Row::new(
            indexed.into_iter().map(|(_, value)| value).collect(),
        ));
    }
    Some(Row::new(map.values().map(json_to_value).collect()))
}

/// Translate a D1 JSON scalar into the owned [`Value`].
fn json_to_value(v: &serde_json::Value) -> Value {
    match v {
        serde_json::Value::Null => Value::Null,
        serde_json::Value::Bool(b) => Value::Bool(*b),
        serde_json::Value::Number(n) => n
            .as_i64()
            .map(Value::Int)
            .or_else(|| n.as_f64().map(Value::Float))
            .unwrap_or(Value::Null),
        serde_json::Value::String(s) => Value::Text(s.clone()),
        other => Value::Text(other.to_string()),
    }
}

/// Read the total affected count out of a D1 batch/query response (`result[].meta.changes`),
/// summed across statements.
fn decode_d1_affected(json: &serde_json::Value) -> u64 {
    json.get("result")
        .and_then(|r| r.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|stmt| {
                    stmt.get("meta")
                        .and_then(|m| m.get("changes"))
                        .and_then(serde_json::Value::as_u64)
                })
                .sum()
        })
        .unwrap_or(0)
}

fn artifact_repo_from_json(namespace: &str, repo: &serde_json::Value) -> Option<ArtifactRepo> {
    let name = text_field(repo, "name")?;
    let id = text_field(repo, "id").unwrap_or_else(|| name.clone());
    let remote_url = text_field(repo, "remote").or_else(|| text_field(repo, "remote_url"))?;
    Some(ArtifactRepo {
        namespace: namespace.to_string(),
        name,
        id,
        remote_url,
        created_at: text_field(repo, "created_at"),
        updated_at: text_field(repo, "updated_at"),
        last_push_at: text_field(repo, "last_push_at"),
        description: text_field(repo, "description"),
        default_branch: text_field(repo, "default_branch"),
        source: text_field(repo, "source"),
        read_only: repo
            .get("read_only")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false),
    })
}

fn text_field(value: &serde_json::Value, field: &str) -> Option<String> {
    value
        .get(field)
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
}

impl CfBackend for HttpApiBackend {
    fn list_accounts(&self) -> Result<Vec<AccountResource>, CfError> {
        let op = "accounts.discovery";
        let req = self.authed(HttpMethod::Get, Self::accounts_path());
        let resp = self.send(op, &req)?;
        let json = Self::parse_json(op, &resp)?;
        Ok(json
            .get("result")
            .and_then(|r| r.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|account| {
                        let id = account.get("id").and_then(|v| v.as_str())?;
                        let name = account.get("name").and_then(|v| v.as_str())?;
                        Some(AccountResource {
                            id: id.to_string(),
                            name: name.to_string(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default())
    }

    fn list_d1_databases(&self) -> Result<Vec<D1DatabaseResource>, CfError> {
        let op = "d1.discovery";
        let req = self.authed(HttpMethod::Get, self.d1_collection_path());
        let resp = self.send(op, &req)?;
        let json = Self::parse_json(op, &resp)?;
        Ok(json
            .get("result")
            .and_then(|r| r.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|db| {
                        let name = db.get("name").and_then(|v| v.as_str())?;
                        let uuid = db.get("uuid").and_then(|v| v.as_str())?;
                        Some(D1DatabaseResource {
                            name: name.to_string(),
                            uuid: D1DatabaseUuid::new(uuid),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default())
    }

    fn list_kv_namespaces(&self) -> Result<Vec<KvNamespaceResource>, CfError> {
        let op = "kv.discovery";
        let req = self.authed(HttpMethod::Get, self.kv_collection_path());
        let resp = self.send(op, &req)?;
        let json = Self::parse_json(op, &resp)?;
        Ok(json
            .get("result")
            .and_then(|r| r.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|ns| {
                        let title = ns.get("title").and_then(|v| v.as_str())?;
                        let id = ns.get("id").and_then(|v| v.as_str())?;
                        Some(KvNamespaceResource {
                            title: title.to_string(),
                            id: KvNamespaceId::new(id),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default())
    }

    fn list_queues(&self) -> Result<Vec<QueueResource>, CfError> {
        let op = "queue.discovery";
        let req = self.authed(HttpMethod::Get, self.queue_collection_path());
        let resp = self.send(op, &req)?;
        let json = Self::parse_json(op, &resp)?;
        Ok(json
            .get("result")
            .and_then(|r| r.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|queue| {
                        let name = queue.get("queue_name").and_then(|v| v.as_str())?;
                        Some(QueueResource {
                            queue_name: QueueName::new(name),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default())
    }

    fn list_artifact_namespaces(&self) -> Result<Vec<String>, CfError> {
        let op = "artifacts.namespaces";
        let req = self.authed(HttpMethod::Get, self.artifact_namespaces_path());
        let resp = self.send(op, &req)?;
        let json = Self::parse_json(op, &resp)?;
        Ok(json
            .get("result")
            .and_then(|r| r.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|namespace| {
                        namespace
                            .as_str()
                            .map(str::to_string)
                            .or_else(|| text_field(namespace, "name"))
                    })
                    .collect()
            })
            .unwrap_or_default())
    }

    fn list_artifact_repos(&self, namespace: &str) -> Result<Vec<ArtifactRepo>, CfError> {
        let op = "artifacts.repos.list";
        let req = self.authed(
            HttpMethod::Get,
            self.artifact_namespace_path(namespace, "/repos"),
        );
        let resp = self.send(op, &req)?;
        let json = Self::parse_json(op, &resp)?;
        Ok(json
            .get("result")
            .and_then(|r| r.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|repo| artifact_repo_from_json(namespace, repo))
                    .collect()
            })
            .unwrap_or_default())
    }

    fn get_artifact_repo(
        &self,
        namespace: &str,
        name: &str,
    ) -> Result<Option<ArtifactRepo>, CfError> {
        let op = "artifacts.repos.get";
        let req = self.authed(
            HttpMethod::Get,
            self.artifact_namespace_path(namespace, &format!("/repos/{name}")),
        );
        let resp = self
            .exchange
            .exchange(&req)
            .map_err(|_| CfError::Transport {
                reason: "cloudflare artifacts get could not be completed".to_string(),
            })?;
        if resp.status == 404 {
            return Ok(None);
        }
        if !resp.is_success() {
            return Err(CfError::Api {
                op,
                status: resp.status,
            });
        }
        let json = Self::parse_json(op, &resp)?;
        Ok(json
            .get("result")
            .and_then(|repo| artifact_repo_from_json(namespace, repo)))
    }

    fn create_artifact_repo(
        &self,
        namespace: &str,
        request: &CreateArtifactRepoRequest,
    ) -> Result<CreatedArtifactRepo, CfError> {
        let op = "artifacts.repos.create";
        let mut body = serde_json::Map::new();
        body.insert(
            "name".to_string(),
            serde_json::Value::String(request.name.clone()),
        );
        if let Some(description) = &request.description {
            body.insert(
                "description".to_string(),
                serde_json::Value::String(description.clone()),
            );
        }
        if let Some(default_branch) = &request.default_branch {
            body.insert(
                "default_branch".to_string(),
                serde_json::Value::String(default_branch.clone()),
            );
        }
        if let Some(read_only) = request.read_only {
            body.insert("read_only".to_string(), serde_json::Value::Bool(read_only));
        }
        let bytes =
            serde_json::to_vec(&serde_json::Value::Object(body)).map_err(|_| CfError::Decode {
                op,
                reason: "could not encode the artifacts create body".to_string(),
            })?;
        let req = self
            .authed(
                HttpMethod::Post,
                self.artifact_namespace_path(namespace, "/repos"),
            )
            .with_body(bytes);
        let resp = self.send(op, &req)?;
        let json = Self::parse_json(op, &resp)?;
        let result = json.get("result").ok_or_else(|| CfError::Decode {
            op,
            reason: "artifacts create response had no result".to_string(),
        })?;
        let repo = artifact_repo_from_json(namespace, result).ok_or_else(|| CfError::Decode {
            op,
            reason: "artifacts create response did not include a repo remote".to_string(),
        })?;
        let token = text_field(result, "token").ok_or_else(|| CfError::Decode {
            op,
            reason: "artifacts create response did not include a repo token".to_string(),
        })?;
        Ok(CreatedArtifactRepo {
            repo,
            token: Secret::from(token),
        })
    }

    fn delete_artifact_repo(&self, namespace: &str, name: &str) -> Result<(), CfError> {
        let op = "artifacts.repos.delete";
        let req = self.authed(
            HttpMethod::Delete,
            self.artifact_namespace_path(namespace, &format!("/repos/{name}")),
        );
        let resp = self
            .exchange
            .exchange(&req)
            .map_err(|_| CfError::Transport {
                reason: "cloudflare artifacts delete could not be completed".to_string(),
            })?;
        if resp.is_success() || resp.status == 404 {
            Ok(())
        } else {
            Err(CfError::Api {
                op,
                status: resp.status,
            })
        }
    }

    fn d1_query(&self, db: &str, sql: &str, params: &[Param]) -> Result<Vec<Row>, CfError> {
        let op = "d1.query";
        // The bound values ride in `params` as a STRUCTURED JSON ARRAY; `sql` carries only `?`
        // placeholders + quoted identifiers (rendered by the reused t17 sqlite emitter). No value
        // is ever interpolated into `sql` — the injection-safety invariant.
        let body = serde_json::json!({
            "sql": sql,
            "params": params.iter().map(param_to_json).collect::<Vec<_>>(),
        });
        let bytes = serde_json::to_vec(&body).map_err(|_| CfError::Decode {
            op,
            reason: "could not encode the d1 query body".to_string(),
        })?;
        let req = self
            .authed(HttpMethod::Post, self.d1_path(db, "/query"))
            .with_body(bytes);
        let resp = self.send(op, &req)?;
        let json = Self::parse_json(op, &resp)?;
        Ok(decode_d1_rows(&json))
    }

    fn d1_batch(&self, db: &str, statements: &[(String, Vec<Param>)]) -> Result<u64, CfError> {
        let op = "d1.batch";
        // One /batch request = one atomic transaction (D1 has no interactive BEGIN/COMMIT). Each
        // statement carries its own bound `params` array — never interpolated.
        let batch: Vec<serde_json::Value> = statements
            .iter()
            .map(|(sql, params)| {
                serde_json::json!({
                    "sql": sql,
                    "params": params.iter().map(param_to_json).collect::<Vec<_>>(),
                })
            })
            .collect();
        let body = serde_json::json!({ "batch": batch });
        let bytes = serde_json::to_vec(&body).map_err(|_| CfError::Decode {
            op,
            reason: "could not encode the d1 batch body".to_string(),
        })?;
        let req = self
            .authed(HttpMethod::Post, self.d1_path(db, "/batch"))
            .with_body(bytes);
        let resp = self.send(op, &req)?;
        let json = Self::parse_json(op, &resp)?;
        Ok(decode_d1_affected(&json))
    }

    fn kv_get(&self, ns: &str, key: &str) -> Result<Option<KvEntry>, CfError> {
        let op = "kv.get";
        let req = self.authed(HttpMethod::Get, self.kv_path(ns, &format!("/values/{key}")));
        let resp = self
            .exchange
            .exchange(&req)
            .map_err(|_| CfError::Transport {
                reason: "cloudflare kv get could not be completed".to_string(),
            })?;
        if resp.status == 404 {
            return Ok(None);
        }
        if !resp.is_success() {
            return Err(CfError::Api {
                op,
                status: resp.status,
            });
        }
        Ok(Some(KvEntry::new(key.to_string(), resp.body)))
    }

    fn kv_put(&self, ns: &str, entry: &KvEntry) -> Result<(), CfError> {
        let op = "kv.put";
        let mut url = self.kv_path(ns, &format!("/values/{}", entry.key));
        if let Some(ttl) = entry.expiration_ttl {
            url.push_str(&format!("?expiration_ttl={ttl}"));
        }
        let req = self
            .authed(HttpMethod::Put, url)
            .with_body(entry.value.clone());
        self.send(op, &req)?;
        Ok(())
    }

    fn kv_delete(&self, ns: &str, key: &str) -> Result<(), CfError> {
        let op = "kv.delete";
        let req = self.authed(
            HttpMethod::Delete,
            self.kv_path(ns, &format!("/values/{key}")),
        );
        let resp = self
            .exchange
            .exchange(&req)
            .map_err(|_| CfError::Transport {
                reason: "cloudflare kv delete could not be completed".to_string(),
            })?;
        // A delete of an absent key is success (idempotent).
        if resp.is_success() || resp.status == 404 {
            Ok(())
        } else {
            Err(CfError::Api {
                op,
                status: resp.status,
            })
        }
    }

    fn kv_list(
        &self,
        ns: &str,
        prefix: Option<&str>,
        limit: Option<u32>,
    ) -> Result<Vec<String>, CfError> {
        let op = "kv.list";
        let mut url = self.kv_path(ns, "/keys");
        let mut query: Vec<String> = Vec::new();
        if let Some(p) = prefix {
            query.push(format!("prefix={p}"));
        }
        if let Some(n) = limit {
            query.push(format!("limit={n}"));
        }
        if !query.is_empty() {
            url.push('?');
            url.push_str(&query.join("&"));
        }
        let req = self.authed(HttpMethod::Get, url);
        let resp = self.send(op, &req)?;
        let json = Self::parse_json(op, &resp)?;
        Ok(json
            .get("result")
            .and_then(|r| r.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|k| k.get("name").and_then(|v| v.as_str()).map(str::to_string))
                    .collect()
            })
            .unwrap_or_default())
    }

    fn queue_send(
        &self,
        queue: &str,
        body: &[u8],
        idempotency_key: &str,
    ) -> Result<MsgId, CfError> {
        let op = "queue.send";
        let body_text = String::from_utf8_lossy(body).to_string();
        let payload = serde_json::json!({
            "body": body_text,
            "idempotency_key": idempotency_key,
        });
        let bytes = serde_json::to_vec(&payload).map_err(|_| CfError::Decode {
            op,
            reason: "could not encode the queue send body".to_string(),
        })?;
        let req = self
            .authed(HttpMethod::Post, self.queue_path(queue, "/messages"))
            .with_body(bytes);
        let resp = self.send(op, &req)?;
        let json = Self::parse_json(op, &resp)?;
        let id = json
            .get("result")
            .and_then(|r| r.get("id"))
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .unwrap_or_else(|| idempotency_key.to_string());
        Ok(MsgId::new(id))
    }

    fn queue_pull(&self, queue: &str, max: u32) -> Result<Vec<QueueMsg>, CfError> {
        let op = "queue.pull";
        let payload = serde_json::json!({ "batch_size": max });
        let bytes = serde_json::to_vec(&payload).map_err(|_| CfError::Decode {
            op,
            reason: "could not encode the queue pull body".to_string(),
        })?;
        let req = self
            .authed(HttpMethod::Post, self.queue_path(queue, "/messages/pull"))
            .with_body(bytes);
        let resp = self.send(op, &req)?;
        let json = Self::parse_json(op, &resp)?;
        Ok(json
            .get("result")
            .and_then(|r| r.get("messages"))
            .and_then(|m| m.as_array())
            .map(|arr| {
                arr.iter()
                    .map(|m| {
                        let id = m
                            .get("id")
                            .and_then(|v| v.as_str())
                            .unwrap_or_default()
                            .to_string();
                        let body = m
                            .get("body")
                            .and_then(|v| v.as_str())
                            .unwrap_or_default()
                            .as_bytes()
                            .to_vec();
                        let attempts = m
                            .get("attempts")
                            .and_then(serde_json::Value::as_u64)
                            .unwrap_or(1) as u32;
                        QueueMsg::new(id, body, attempts)
                    })
                    .collect()
            })
            .unwrap_or_default())
    }
}

/// One recorded Cloudflare backend call (the op + its salient owned arguments) — what a test
/// asserts the driver issued. Secret-free by construction (no token ever enters this seam). The
/// D1 arms carry the rendered `sql` + the structured `params` array so a test asserts both the
/// `?`-only SQL and the bound (never-interpolated) values.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum RecordedCall {
    /// Account discovery.
    AccountDiscovery,
    /// D1 database discovery.
    D1Discovery,
    /// KV namespace discovery.
    KvDiscovery,
    /// Queue discovery.
    QueueDiscovery,
    /// Artifacts namespace discovery.
    ArtifactNamespaceDiscovery,
    /// Artifacts repo list.
    ArtifactRepoList {
        /// The namespace.
        namespace: String,
    },
    /// Artifacts repo get.
    ArtifactRepoGet {
        /// The namespace.
        namespace: String,
        /// The repository name.
        name: String,
    },
    /// Artifacts repo create. The minted token is deliberately not recorded.
    ArtifactRepoCreate {
        /// The namespace.
        namespace: String,
        /// The non-secret create request.
        request: CreateArtifactRepoRequest,
    },
    /// Artifacts repo delete.
    ArtifactRepoDelete {
        /// The namespace.
        namespace: String,
        /// The repository name.
        name: String,
    },
    /// `d1.query` (SELECT) with the rendered SQL + bound params.
    D1Query {
        /// The D1 database.
        db: String,
        /// The rendered SQL (only `?` placeholders).
        sql: String,
        /// The structured bound params array (the values — never interpolated into `sql`).
        params: Vec<Param>,
    },
    /// `d1.batch` — one atomic transaction of N statements (the batch atomicity proof).
    D1Batch {
        /// The D1 database.
        db: String,
        /// Each statement's rendered SQL + bound params.
        statements: Vec<(String, Vec<Param>)>,
    },
    /// `kv.get`.
    KvGet {
        /// The namespace.
        ns: String,
        /// The key.
        key: String,
    },
    /// `kv.put`.
    KvPut {
        /// The namespace.
        ns: String,
        /// The written entry.
        entry: KvEntry,
    },
    /// `kv.delete`.
    KvDelete {
        /// The namespace.
        ns: String,
        /// The deleted key.
        key: String,
    },
    /// `kv.list`.
    KvList {
        /// The namespace.
        ns: String,
        /// The optional key prefix.
        prefix: Option<String>,
        /// The optional cap.
        limit: Option<u32>,
    },
    /// `queue.send` (carries the idempotency key — the at-least-once de-dupe proof).
    QueueSend {
        /// The queue.
        queue: String,
        /// The message body.
        body: Vec<u8>,
        /// The idempotency key.
        idempotency_key: String,
    },
    /// `queue.pull` (tail, capped at `max`).
    QueuePull {
        /// The queue.
        queue: String,
        /// The tail cap.
        max: u32,
    },
}

/// An in-memory mock Cloudflare backend (tests / CI / wasm): answers from pre-seeded fixtures and
/// **records** every call so a test asserts the exact API surface the driver exercised — with
/// **no socket and no credentials**. The recorded D1 calls carry the rendered SQL + the bound
/// params so a test proves the params are a structured array and never interpolated into the SQL.
#[derive(Default)]
pub struct MockCfBackend {
    accounts: Mutex<Vec<AccountResource>>,
    d1_databases: Mutex<Vec<D1DatabaseResource>>,
    kv_namespaces: Mutex<Vec<KvNamespaceResource>>,
    queues: Mutex<Vec<QueueResource>>,
    artifact_namespaces: Mutex<Vec<String>>,
    artifact_repos: Mutex<Vec<ArtifactRepo>>,
    artifact_create_results: Mutex<Vec<(ArtifactRepo, String)>>,
    d1_rows: Mutex<Vec<Vec<Row>>>,
    d1_affected: Mutex<u64>,
    kv_entries: Mutex<Vec<KvEntry>>,
    kv_keys: Mutex<Vec<String>>,
    queue_msgs: Mutex<Vec<QueueMsg>>,
    recorded: Mutex<Vec<RecordedCall>>,
}

impl MockCfBackend {
    /// An empty mock.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Seed a visible Cloudflare account.
    #[must_use]
    pub fn with_account(self, id: impl Into<String>, name: impl Into<String>) -> Self {
        if let Ok(mut a) = self.accounts.lock() {
            a.push(AccountResource {
                id: id.into(),
                name: name.into(),
            });
        }
        self
    }

    /// Seed a discovered D1 database.
    #[must_use]
    pub fn with_d1_database(self, name: impl Into<String>, uuid: D1DatabaseUuid) -> Self {
        if let Ok(mut d) = self.d1_databases.lock() {
            d.push(D1DatabaseResource {
                name: name.into(),
                uuid,
            });
        }
        self
    }

    /// Seed a discovered KV namespace.
    #[must_use]
    pub fn with_kv_namespace(self, title: impl Into<String>, id: KvNamespaceId) -> Self {
        if let Ok(mut k) = self.kv_namespaces.lock() {
            k.push(KvNamespaceResource {
                title: title.into(),
                id,
            });
        }
        self
    }

    /// Seed a discovered Queue.
    #[must_use]
    pub fn with_queue(self, queue_name: QueueName) -> Self {
        if let Ok(mut q) = self.queues.lock() {
            q.push(QueueResource { queue_name });
        }
        self
    }

    /// Seed a discovered Artifacts namespace.
    #[must_use]
    pub fn with_artifact_namespace(self, namespace: impl Into<String>) -> Self {
        if let Ok(mut a) = self.artifact_namespaces.lock() {
            a.push(namespace.into());
        }
        self
    }

    /// Seed an Artifacts repository returned by list/get.
    #[must_use]
    pub fn with_artifact_repo(self, repo: ArtifactRepo) -> Self {
        if let Ok(mut a) = self.artifact_repos.lock() {
            a.push(repo);
        }
        self
    }

    /// Seed the next Artifacts create result. The token is stored only inside the mock and is never
    /// reflected in [`RecordedCall`].
    #[must_use]
    pub fn with_artifact_create_result(self, repo: ArtifactRepo, token: impl Into<String>) -> Self {
        if let Ok(mut a) = self.artifact_create_results.lock() {
            a.push((repo, token.into()));
        }
        self
    }

    /// Queue one D1 SELECT result page (FIFO, consumed by `d1_query`).
    #[must_use]
    pub fn with_d1_rows(self, rows: Vec<Row>) -> Self {
        if let Ok(mut q) = self.d1_rows.lock() {
            q.push(rows);
        }
        self
    }

    /// Seed the affected count a `d1_batch` reports.
    #[must_use]
    pub fn with_d1_affected(self, affected: u64) -> Self {
        if let Ok(mut a) = self.d1_affected.lock() {
            *a = affected;
        }
        self
    }

    /// Seed a KV entry `kv_get` returns (matched by key).
    #[must_use]
    pub fn with_kv_entry(self, entry: KvEntry) -> Self {
        if let Ok(mut e) = self.kv_entries.lock() {
            e.push(entry);
        }
        self
    }

    /// Seed the keys `kv_list` returns.
    #[must_use]
    pub fn with_kv_keys(self, keys: Vec<String>) -> Self {
        if let Ok(mut k) = self.kv_keys.lock() {
            *k = keys;
        }
        self
    }

    /// Seed a queue message `queue_pull` returns.
    #[must_use]
    pub fn with_queue_msg(self, msg: QueueMsg) -> Self {
        if let Ok(mut m) = self.queue_msgs.lock() {
            m.push(msg);
        }
        self
    }

    /// The calls this mock received, in order — what a test asserts against.
    #[must_use]
    pub fn recorded(&self) -> Vec<RecordedCall> {
        self.recorded.lock().map(|r| r.clone()).unwrap_or_default()
    }

    fn record(&self, call: RecordedCall) {
        if let Ok(mut r) = self.recorded.lock() {
            r.push(call);
        }
    }
}

impl CfBackend for MockCfBackend {
    fn list_accounts(&self) -> Result<Vec<AccountResource>, CfError> {
        self.record(RecordedCall::AccountDiscovery);
        Ok(self.accounts.lock().map(|a| a.clone()).unwrap_or_default())
    }

    fn list_d1_databases(&self) -> Result<Vec<D1DatabaseResource>, CfError> {
        self.record(RecordedCall::D1Discovery);
        Ok(self
            .d1_databases
            .lock()
            .map(|d| d.clone())
            .unwrap_or_default())
    }

    fn list_kv_namespaces(&self) -> Result<Vec<KvNamespaceResource>, CfError> {
        self.record(RecordedCall::KvDiscovery);
        Ok(self
            .kv_namespaces
            .lock()
            .map(|k| k.clone())
            .unwrap_or_default())
    }

    fn list_queues(&self) -> Result<Vec<QueueResource>, CfError> {
        self.record(RecordedCall::QueueDiscovery);
        Ok(self.queues.lock().map(|q| q.clone()).unwrap_or_default())
    }

    fn list_artifact_namespaces(&self) -> Result<Vec<String>, CfError> {
        self.record(RecordedCall::ArtifactNamespaceDiscovery);
        Ok(self
            .artifact_namespaces
            .lock()
            .map(|a| a.clone())
            .unwrap_or_default())
    }

    fn list_artifact_repos(&self, namespace: &str) -> Result<Vec<ArtifactRepo>, CfError> {
        self.record(RecordedCall::ArtifactRepoList {
            namespace: namespace.to_string(),
        });
        Ok(self
            .artifact_repos
            .lock()
            .map(|repos| {
                repos
                    .iter()
                    .filter(|repo| repo.namespace == namespace)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default())
    }

    fn get_artifact_repo(
        &self,
        namespace: &str,
        name: &str,
    ) -> Result<Option<ArtifactRepo>, CfError> {
        self.record(RecordedCall::ArtifactRepoGet {
            namespace: namespace.to_string(),
            name: name.to_string(),
        });
        Ok(self.artifact_repos.lock().ok().and_then(|repos| {
            repos
                .iter()
                .find(|repo| repo.namespace == namespace && repo.name == name)
                .cloned()
        }))
    }

    fn create_artifact_repo(
        &self,
        namespace: &str,
        request: &CreateArtifactRepoRequest,
    ) -> Result<CreatedArtifactRepo, CfError> {
        self.record(RecordedCall::ArtifactRepoCreate {
            namespace: namespace.to_string(),
            request: request.clone(),
        });
        let next = self.artifact_create_results.lock().ok().and_then(|mut q| {
            if q.is_empty() {
                None
            } else {
                Some(q.remove(0))
            }
        });
        let (repo, token) = next.unwrap_or_else(|| {
            (
                ArtifactRepo {
                    namespace: namespace.to_string(),
                    name: request.name.clone(),
                    id: format!("repo-{}", request.name),
                    remote_url: format!(
                        "https://account.artifacts.cloudflare.net/git/{namespace}/{}.git",
                        request.name
                    ),
                    default_branch: request.default_branch.clone(),
                    description: request.description.clone(),
                    read_only: request.read_only.unwrap_or(false),
                    ..ArtifactRepo::default()
                },
                "mock-artifact-token".to_string(),
            )
        });
        Ok(CreatedArtifactRepo {
            repo,
            token: Secret::from(token),
        })
    }

    fn delete_artifact_repo(&self, namespace: &str, name: &str) -> Result<(), CfError> {
        self.record(RecordedCall::ArtifactRepoDelete {
            namespace: namespace.to_string(),
            name: name.to_string(),
        });
        Ok(())
    }

    fn d1_query(&self, db: &str, sql: &str, params: &[Param]) -> Result<Vec<Row>, CfError> {
        self.record(RecordedCall::D1Query {
            db: db.to_string(),
            sql: sql.to_string(),
            params: params.to_vec(),
        });
        Ok(self
            .d1_rows
            .lock()
            .ok()
            .and_then(|mut q| {
                if q.is_empty() {
                    None
                } else {
                    Some(q.remove(0))
                }
            })
            .unwrap_or_default())
    }

    fn d1_batch(&self, db: &str, statements: &[(String, Vec<Param>)]) -> Result<u64, CfError> {
        self.record(RecordedCall::D1Batch {
            db: db.to_string(),
            statements: statements.to_vec(),
        });
        Ok(self.d1_affected.lock().map(|a| *a).unwrap_or(0))
    }

    fn kv_get(&self, ns: &str, key: &str) -> Result<Option<KvEntry>, CfError> {
        self.record(RecordedCall::KvGet {
            ns: ns.to_string(),
            key: key.to_string(),
        });
        Ok(self
            .kv_entries
            .lock()
            .ok()
            .and_then(|e| e.iter().find(|x| x.key == key).cloned()))
    }

    fn kv_put(&self, ns: &str, entry: &KvEntry) -> Result<(), CfError> {
        self.record(RecordedCall::KvPut {
            ns: ns.to_string(),
            entry: entry.clone(),
        });
        Ok(())
    }

    fn kv_delete(&self, ns: &str, key: &str) -> Result<(), CfError> {
        self.record(RecordedCall::KvDelete {
            ns: ns.to_string(),
            key: key.to_string(),
        });
        Ok(())
    }

    fn kv_list(
        &self,
        ns: &str,
        prefix: Option<&str>,
        limit: Option<u32>,
    ) -> Result<Vec<String>, CfError> {
        self.record(RecordedCall::KvList {
            ns: ns.to_string(),
            prefix: prefix.map(str::to_string),
            limit,
        });
        Ok(self.kv_keys.lock().map(|k| k.clone()).unwrap_or_default())
    }

    fn queue_send(
        &self,
        queue: &str,
        body: &[u8],
        idempotency_key: &str,
    ) -> Result<MsgId, CfError> {
        self.record(RecordedCall::QueueSend {
            queue: queue.to_string(),
            body: body.to_vec(),
            idempotency_key: idempotency_key.to_string(),
        });
        Ok(MsgId::new(format!("msg-{idempotency_key}")))
    }

    fn queue_pull(&self, queue: &str, max: u32) -> Result<Vec<QueueMsg>, CfError> {
        self.record(RecordedCall::QueuePull {
            queue: queue.to_string(),
            max,
        });
        let msgs = self
            .queue_msgs
            .lock()
            .map(|m| m.clone())
            .unwrap_or_default();
        Ok(msgs.into_iter().take(max as usize).collect())
    }
}
