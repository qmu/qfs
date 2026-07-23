//! `registry` — the per-mount Cloudflare resource registry (blueprint §6). The engine builds it
//! from the configured Cloudflare resources; the driver looks a handle up by the path's
//! `<db>`/`<ns>`/`<name>` segment.
//!
//! ## D1 catalog (reused from t17)
//! A [`D1Database`] pairs a [`CfBackend`](crate::backend::CfBackend) with its cached
//! [`Catalog`](qfs_sql_core::Catalog) — the **same** owned catalog DTO the t17 SQL driver
//! introspects, so `DESCRIBE`/`capabilities`/the SQL compiler all read it without I/O (the
//! catalog was introspected once at handle construction). The D1 backend's catalog is supplied at
//! construction (D1's schema is known to the engine config / introspected via `PRAGMA` over the
//! same SQLite dialect).
//!
//! KV namespaces and queues carry no catalog — their schema is the fixed degenerate `(key,
//! value)` / `(id, body, attempts)` shape — so the registry tracks only their declared names.

use std::collections::HashMap;
use std::sync::Arc;

use qfs_sql_core::{Catalog, TableCatalog};

use crate::backend::{ArtifactTokenSealer, CfBackend, D1DatabaseUuid, KvNamespaceId, QueueName};
use crate::error::CfError;

/// One live Cloudflare backend handle: the shared [`CfBackend`] plus, for D1, the cached
/// [`Catalog`]. Cheaply cloneable (the backend is behind an `Arc`).
#[derive(Clone)]
pub struct D1Database {
    backend: Arc<dyn CfBackend>,
    uuid: Option<D1DatabaseUuid>,
    catalog: Catalog,
}

impl D1Database {
    /// Build a D1 database handle from a backend + an already-introspected [`Catalog`] (the
    /// engine config supplies the schema; D1 introspection rides the same sqlite `PRAGMA` path as
    /// t17 and is the engine's concern, not this driver's I/O).
    #[must_use]
    pub fn new(backend: Arc<dyn CfBackend>, catalog: Catalog) -> Self {
        Self {
            backend,
            uuid: None,
            catalog,
        }
    }

    /// Build a D1 handle from a discovered Cloudflare UUID + an already-introspected catalog.
    #[must_use]
    pub fn discovered(backend: Arc<dyn CfBackend>, uuid: D1DatabaseUuid, catalog: Catalog) -> Self {
        Self {
            backend,
            uuid: Some(uuid),
            catalog,
        }
    }

    /// The shared backend (the read/commit I/O path).
    #[must_use]
    pub fn backend(&self) -> &Arc<dyn CfBackend> {
        &self.backend
    }

    /// The Cloudflare API database id. Falls back to the qfs path name for hand-built test
    /// registries that predate discovery.
    #[must_use]
    pub fn api_database_id<'a>(&'a self, path_name: &'a str) -> &'a str {
        self.uuid.as_ref().map_or(path_name, D1DatabaseUuid::as_str)
    }

    /// The cached catalog (no I/O).
    #[must_use]
    pub fn catalog(&self) -> &Catalog {
        &self.catalog
    }

    /// Look up a table's catalog by name.
    ///
    /// # Errors
    /// [`CfError::MalformedEffect`] if the table is absent from the catalog.
    pub fn table(&self, name: &str, path: &str) -> Result<&TableCatalog, CfError> {
        self.catalog
            .table(name)
            .ok_or_else(|| CfError::MalformedEffect {
                verb: "EFFECT",
                path: path.to_string(),
                reason: format!("no such D1 table `{name}` in the database catalog"),
            })
    }
}

/// A registered KV namespace handle: qfs path title plus the Cloudflare API namespace id.
#[derive(Clone)]
pub struct KvNamespace {
    backend: Arc<dyn CfBackend>,
    id: Option<KvNamespaceId>,
}

impl KvNamespace {
    /// Build a fallback KV handle whose path title is also the API id.
    #[must_use]
    pub fn new(backend: Arc<dyn CfBackend>) -> Self {
        Self { backend, id: None }
    }

    /// Build a discovered KV handle with an explicit Cloudflare namespace id.
    #[must_use]
    pub fn discovered(backend: Arc<dyn CfBackend>, id: KvNamespaceId) -> Self {
        Self {
            backend,
            id: Some(id),
        }
    }

    /// The shared backend.
    #[must_use]
    pub fn backend(&self) -> &Arc<dyn CfBackend> {
        &self.backend
    }

    /// The Cloudflare API namespace id. Falls back to the qfs path title for hand-built tests.
    #[must_use]
    pub fn api_namespace_id<'a>(&'a self, path_title: &'a str) -> &'a str {
        self.id.as_ref().map_or(path_title, KvNamespaceId::as_str)
    }
}

/// A registered Queue handle. The current Cloudflare API addresses queues by queue name.
#[derive(Clone)]
pub struct QueueHandle {
    backend: Arc<dyn CfBackend>,
    name: QueueName,
}

impl QueueHandle {
    /// Build a queue handle.
    #[must_use]
    pub fn new(backend: Arc<dyn CfBackend>, name: QueueName) -> Self {
        Self { backend, name }
    }

    /// The shared backend.
    #[must_use]
    pub fn backend(&self) -> &Arc<dyn CfBackend> {
        &self.backend
    }

    /// The Cloudflare API queue name.
    #[must_use]
    pub fn api_queue_name(&self) -> &str {
        self.name.as_str()
    }
}

/// The account-scoped Artifacts handle. Unlike D1/KV/Queues it is not keyed by one discovered
/// resource segment: `/cf/artifacts` fans out over namespaces in the account, and create/delete
/// address `(namespace, repo)` under the same backend.
#[derive(Clone)]
pub struct ArtifactsHandle {
    backend: Arc<dyn CfBackend>,
    sealer: Arc<dyn ArtifactTokenSealer>,
}

impl ArtifactsHandle {
    /// Build an account-scoped Artifacts handle.
    #[must_use]
    pub fn new(backend: Arc<dyn CfBackend>, sealer: Arc<dyn ArtifactTokenSealer>) -> Self {
        Self { backend, sealer }
    }

    /// The shared backend.
    #[must_use]
    pub fn backend(&self) -> &Arc<dyn CfBackend> {
        &self.backend
    }

    /// The token sealer for create responses.
    #[must_use]
    pub fn sealer(&self) -> &Arc<dyn ArtifactTokenSealer> {
        &self.sealer
    }
}

/// The Cloudflare resource registry, keyed by the service target name. Built by the engine from
/// the configured resources; the driver resolves a handle by the path's selector segment.
///
/// - D1 databases carry a backend + catalog (`d1`).
/// - KV namespaces carry only a backend (their schema is fixed).
/// - Queues carry only a backend (their schema is fixed).
///
/// A single shared backend commonly serves all three (one Cloudflare account); the registry lets
/// each service map a target name to its handle independently so a least-privilege deployment can
/// scope per-resource.
#[derive(Clone, Default)]
pub struct CfRegistry {
    d1: HashMap<String, D1Database>,
    /// A wildcard D1 handle answering ANY database key not explicitly registered in `d1` — the
    /// no-introspection model the declared `/cloudflare/d1/{database}` mount needs.
    d1_template: Option<D1Database>,
    kv: HashMap<String, KvNamespace>,
    queues: HashMap<String, QueueHandle>,
    artifacts: Option<ArtifactsHandle>,
}

impl CfRegistry {
    /// An empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a D1 database under `db`.
    #[must_use]
    pub fn with_d1(mut self, db: impl Into<String>, handle: D1Database) -> Self {
        self.d1.insert(db.into(), handle);
        self
    }

    /// Register a **wildcard D1 template**: a single handle (backend + declared catalog, no
    /// discovered UUID) that answers ANY database key not explicitly registered via
    /// [`with_d1`](Self::with_d1). This is the no-introspection model the declared
    /// `/cloudflare/d1/{database}` mount needs — the addressed `{database}` segment is itself used
    /// as the Cloudflare D1 api id ([`D1Database::api_database_id`] falls back to the path name when
    /// the uuid is `None`), so a declared mount serves an arbitrary database with **no** mount-time
    /// `list_d1_databases`/`introspect_d1`. The declared catalog (from the committed
    /// `CREATE SQL … TABLES(…)` row) supplies the relation schema. An explicit
    /// [`with_d1`](Self::with_d1) registration still wins over the template.
    #[must_use]
    pub fn with_d1_template(mut self, handle: D1Database) -> Self {
        self.d1_template = Some(handle);
        self
    }

    /// Register a KV namespace under `ns` (served by `backend`).
    #[must_use]
    pub fn with_kv(mut self, ns: impl Into<String>, backend: Arc<dyn CfBackend>) -> Self {
        self.kv.insert(ns.into(), KvNamespace::new(backend));
        self
    }

    /// Register a discovered KV namespace under its human title, carrying its Cloudflare id.
    #[must_use]
    pub fn with_kv_id(
        mut self,
        title: impl Into<String>,
        id: KvNamespaceId,
        backend: Arc<dyn CfBackend>,
    ) -> Self {
        self.kv
            .insert(title.into(), KvNamespace::discovered(backend, id));
        self
    }

    /// Register a queue under `name` (served by `backend`).
    #[must_use]
    pub fn with_queue(mut self, name: impl Into<String>, backend: Arc<dyn CfBackend>) -> Self {
        let name = name.into();
        self.queues.insert(
            name.clone(),
            QueueHandle::new(backend, QueueName::new(name.clone())),
        );
        self
    }

    /// Register a discovered queue.
    #[must_use]
    pub fn with_queue_name(mut self, name: QueueName, backend: Arc<dyn CfBackend>) -> Self {
        self.queues.insert(
            name.as_str().to_string(),
            QueueHandle::new(backend, name.clone()),
        );
        self
    }

    /// Register the account-scoped Artifacts resource.
    #[must_use]
    pub fn with_artifacts(
        mut self,
        backend: Arc<dyn CfBackend>,
        sealer: Arc<dyn ArtifactTokenSealer>,
    ) -> Self {
        self.artifacts = Some(ArtifactsHandle::new(backend, sealer));
        self
    }

    /// Whether no resources are registered.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.d1.is_empty()
            && self.d1_template.is_none()
            && self.kv.is_empty()
            && self.queues.is_empty()
            && self.artifacts.is_none()
    }

    /// Look up a D1 database handle: an explicit [`with_d1`](Self::with_d1) registration first, then
    /// the wildcard [`with_d1_template`](Self::with_d1_template) fallback (which answers any key).
    ///
    /// # Errors
    /// [`CfError::InvalidPath`] if no D1 database is registered under `db` and no template exists.
    pub fn d1(&self, db: &str) -> Result<&D1Database, CfError> {
        self.d1
            .get(db)
            .or(self.d1_template.as_ref())
            .ok_or(CfError::InvalidPath {
                path: format!("/cf/d1/{db}"),
                reason: "no such registered D1 database",
            })
    }

    /// Whether a D1 database is registered (the introspective capability gate uses this without
    /// borrowing the handle). A wildcard template answers for **any** key, so its presence makes
    /// every D1 database key available.
    #[must_use]
    pub fn has_d1(&self, db: &str) -> bool {
        self.d1.contains_key(db) || self.d1_template.is_some()
    }

    /// Look up a KV namespace backend.
    ///
    /// # Errors
    /// [`CfError::InvalidPath`] if no KV namespace is registered under `ns`.
    pub fn kv(&self, ns: &str) -> Result<&KvNamespace, CfError> {
        self.kv.get(ns).ok_or(CfError::InvalidPath {
            path: format!("/cf/kv/{ns}"),
            reason: "no such registered KV namespace",
        })
    }

    /// Whether a KV namespace is registered.
    #[must_use]
    pub fn has_kv(&self, ns: &str) -> bool {
        self.kv.contains_key(ns)
    }

    /// Look up a queue backend.
    ///
    /// # Errors
    /// [`CfError::InvalidPath`] if no queue is registered under `name`.
    pub fn queue(&self, name: &str) -> Result<&QueueHandle, CfError> {
        self.queues.get(name).ok_or(CfError::InvalidPath {
            path: format!("/cf/queue/{name}"),
            reason: "no such registered queue",
        })
    }

    /// Whether a queue is registered.
    #[must_use]
    pub fn has_queue(&self, name: &str) -> bool {
        self.queues.contains_key(name)
    }

    /// Look up the account-scoped Artifacts handle.
    ///
    /// # Errors
    /// [`CfError::InvalidPath`] if Artifacts is not registered for this account.
    pub fn artifacts(&self) -> Result<&ArtifactsHandle, CfError> {
        self.artifacts.as_ref().ok_or(CfError::InvalidPath {
            path: "/cf/artifacts".to_string(),
            reason: "artifacts are not registered for this Cloudflare account",
        })
    }

    /// Whether Artifacts is registered.
    #[must_use]
    pub fn has_artifacts(&self) -> bool {
        self.artifacts.is_some()
    }
}
