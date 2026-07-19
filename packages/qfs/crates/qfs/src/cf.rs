//! Cloudflare live-driver composition for `/cf`.
//!
//! The driver crate owns the vendor-free D1/KV/Queues semantics. This binary module owns only the
//! live wiring: resolve the API token from the qfs vault, read the Cloudflare account id from the
//! connect-created mount binding, adapt the shared reqwest transport, discover resources, and build
//! a `CfRegistry` with D1 catalogs introspected once up front.

use std::sync::Arc;

use qfs_driver_cf::{
    ArtifactRepoKey, ArtifactTokenSealer, CfBackend, CfDriver, CfRegistry, D1Database,
    HttpApiBackend,
};
use qfs_driver_sql::{Catalog, ColumnDef, Dialect, Param, RelationKind, TableCatalog};
use qfs_secrets::{ConnectionId, CredentialKey, DriverId, Secret, Secrets};
use qfs_types::Value;

/// The non-secret Cloudflare account id carried by the connect binding's `at_locator`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CloudflareAccountId(String);

impl CloudflareAccountId {
    fn from_mount(mount: &crate::cloud_mounts::CloudMount) -> Option<Self> {
        let raw = mount.at_locator.as_deref()?.trim();
        if raw.is_empty() {
            tracing::warn!(
                target: "qfs::cf",
                path = %mount.path,
                "skipping Cloudflare mount; reconnect it with `qfs connect <path> --driver cf --account <label>` (or add `--at <cloudflare-account-id>` for a multi-account token)"
            );
            return None;
        }
        Some(Self(raw.to_string()))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

/// One account visible to a Cloudflare token during connect-time account id discovery.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct VisibleCloudflareAccount {
    pub(crate) id: CloudflareAccountId,
    pub(crate) name: String,
}

/// The explicit result of resolving `qfs connect /cf --account <label>` without `--at`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CloudflareAccountResolution {
    Resolved(CloudflareAccountId),
    NoneVisible,
    Ambiguous(Vec<VisibleCloudflareAccount>),
}

/// Resolve the non-secret Cloudflare account id that should be persisted in a `/cf` binding when
/// the operator omitted `--at`.
pub(crate) fn resolve_cf_account_id_for_connect(
    connection: &str,
) -> Result<CloudflareAccountResolution, String> {
    let token = resolve_cf_token_for_connect(connection)?;
    let backend = HttpApiBackend::for_token(crate::transport::cf_exchange(), token);
    resolve_cf_account_id_from_backend(&backend).map_err(|e| {
        format!("discovering Cloudflare accounts visible to account `{connection}`: {e}")
    })
}

pub(crate) fn resolve_cf_account_id_from_backend(
    backend: &dyn CfBackend,
) -> Result<CloudflareAccountResolution, qfs_driver_cf::CfError> {
    let accounts: Vec<VisibleCloudflareAccount> = backend
        .list_accounts()?
        .into_iter()
        .map(|account| VisibleCloudflareAccount {
            id: CloudflareAccountId(account.id),
            name: account.name,
        })
        .collect();
    Ok(match accounts.as_slice() {
        [] => CloudflareAccountResolution::NoneVisible,
        [single] => CloudflareAccountResolution::Resolved(single.id.clone()),
        _ => CloudflareAccountResolution::Ambiguous(accounts),
    })
}

fn resolve_cf_token_for_connect(connection: &str) -> Result<Secret, String> {
    let Some((store, cred)) = crate::commit::networked_credential("cf", connection) else {
        return Err(format!(
            "cannot resolve Cloudflare account `{connection}` -- run `qfs account add cf \
             {connection}` first"
        ));
    };
    if !crate::commit::cloud_bind_allowed("cf", cred.connection.as_str()) {
        return Err(format!(
            "Cloudflare account `{connection}` is not authorized for this operator -- run \
             `qfs account add cf {connection}` after `qfs init`"
        ));
    }
    store.get(&cred).map_err(|e| {
        format!(
            "cannot read Cloudflare token for account `{}`: {}",
            cred.connection.as_str(),
            e
        )
    })
}

/// Build the live Cloudflare driver for one connect-created cloud mount. Returns `None` when the
/// mount lacks an account id, the vault credential cannot resolve, consent/bind gates refuse it, or
/// discovery finds no registrable resources.
#[must_use]
pub(crate) fn live_driver_for_mount(mount: &crate::cloud_mounts::CloudMount) -> Option<CfDriver> {
    let account_id = CloudflareAccountId::from_mount(mount)?;
    let connection = mount.account.as_deref().unwrap_or("default");
    let token = resolve_cf_token(connection)?;
    let backend: Arc<dyn CfBackend> = Arc::new(HttpApiBackend::new(
        crate::transport::cf_exchange(),
        account_id.as_str(),
        token,
    ));
    let sealer = artifact_token_sealer();
    driver_from_backend_with_artifact_sealer(backend, sealer)
}

fn resolve_cf_token(connection: &str) -> Option<Secret> {
    resolve_cf_store_and_token(connection).map(|(_, token)| token)
}

fn resolve_cf_store_and_token(connection: &str) -> Option<(Arc<dyn Secrets>, Secret)> {
    let (store, cred) = crate::commit::networked_credential("cf", connection)?;
    if !crate::commit::cloud_bind_allowed("cf", cred.connection.as_str()) {
        return None;
    }
    let token = store.get(&cred).ok()?;
    Some((store, token))
}

fn artifact_token_sealer() -> Arc<dyn ArtifactTokenSealer> {
    match crate::connection::open_store_for_commit() {
        Some(store) => Arc::new(VaultArtifactTokenSealer::new(Arc::new(store))),
        None => Arc::new(RejectingArtifactTokenSealer),
    }
}

#[cfg(test)]
pub(crate) fn driver_from_backend(backend: Arc<dyn CfBackend>) -> Option<CfDriver> {
    driver_from_backend_with_artifact_sealer(
        backend,
        Arc::new(qfs_driver_cf::NoopArtifactTokenSealer),
    )
}

pub(crate) fn driver_from_backend_with_artifact_sealer(
    backend: Arc<dyn CfBackend>,
    artifact_sealer: Arc<dyn ArtifactTokenSealer>,
) -> Option<CfDriver> {
    let mut registry = CfRegistry::new();

    match backend.list_d1_databases() {
        Ok(databases) => {
            for db in databases {
                let api_id = db.uuid.as_str().to_string();
                let catalog = match introspect_d1(backend.as_ref(), &api_id) {
                    Ok(catalog) => catalog,
                    Err(e) => {
                        tracing::warn!(
                            target: "qfs::cf",
                            database = %db.name,
                            uuid = %api_id,
                            error = %e,
                            "skipping Cloudflare D1 database; catalog introspection failed"
                        );
                        continue;
                    }
                };
                registry = registry.with_d1(
                    db.name,
                    D1Database::discovered(backend.clone(), db.uuid, catalog),
                );
            }
        }
        Err(e) => {
            tracing::warn!(
                target: "qfs::cf",
                error = %e,
                "skipping Cloudflare D1 registration; resource discovery failed"
            );
        }
    }

    match backend.list_kv_namespaces() {
        Ok(namespaces) => {
            for ns in namespaces {
                registry = registry.with_kv_id(ns.title, ns.id, backend.clone());
            }
        }
        Err(e) => {
            tracing::warn!(
                target: "qfs::cf",
                error = %e,
                "skipping Cloudflare KV registration; resource discovery failed"
            );
        }
    }

    match backend.list_queues() {
        Ok(queues) => {
            for queue in queues {
                registry = registry.with_queue_name(queue.queue_name, backend.clone());
            }
        }
        Err(e) => {
            tracing::warn!(
                target: "qfs::cf",
                error = %e,
                "skipping Cloudflare Queue registration; resource discovery failed"
            );
        }
    }

    match backend.list_artifact_namespaces() {
        Ok(_) => {
            registry = registry.with_artifacts(backend.clone(), artifact_sealer);
        }
        Err(e) => {
            tracing::warn!(
                target: "qfs::cf",
                error = %e,
                "skipping Cloudflare Artifacts registration; resource discovery failed"
            );
        }
    }

    if registry.is_empty() {
        tracing::warn!(
            target: "qfs::cf",
            "skipping Cloudflare mount; no D1, KV, Queue, or Artifacts resources were discovered"
        );
        return None;
    }
    Some(CfDriver::new(registry))
}

// ---------------------------------------------------------------------------
// §13 declared D1 twin — the `/cloudflare/d1` nested mount served from a committed
// `CREATE SQL … TABLES(…)` declaration, NOT compiled introspection (ticket 20260718203326).
// ---------------------------------------------------------------------------

/// Build a **declared** Cloudflare D1 driver: a [`CfDriver`] whose registry is a single wildcard-D1
/// template over `backend`, serving the declared `catalog` for ANY `{database}` key with NO
/// mount-time `list_d1_databases`/`introspect_d1` (the declared twin of the compiled `/cf` D1
/// surface, blueprint §13). The addressed `{database}` segment is used AS the Cloudflare D1 api id
/// ([`D1Database::api_database_id`] falls back to the path name when the uuid is `None`). The
/// `backend` carries whatever auth it was built with — this function is credential-free by
/// construction (a `MockCfBackend` for the pure DESCRIBE mount, the live [`HttpApiBackend`] for the
/// read/apply facets).
#[must_use]
pub(crate) fn declared_d1_driver(backend: Arc<dyn CfBackend>, catalog: Catalog) -> CfDriver {
    CfDriver::new(CfRegistry::new().with_d1_template(D1Database::new(backend, catalog)))
}

/// The live wire backend a declared D1 mount serves over: the shared `reqwest` Cloudflare transport,
/// the Cloudflare account id from the mount's `AT` locator, and the resolved bearer — the SAME
/// [`HttpApiBackend`] the compiled `/cf` uses, built from DECLARED inputs instead of compiled
/// discovery. Its D1 URL/req/resp shape already matches the declared `query_endpoint`.
#[must_use]
pub(crate) fn declared_d1_backend(account_id: &str, token: Secret) -> Arc<dyn CfBackend> {
    Arc::new(HttpApiBackend::new(
        declared_d1_exchange(),
        account_id,
        token,
    ))
}

/// The wire seam [`declared_d1_backend`] builds its [`HttpApiBackend`] over. In production this is
/// always the shared `reqwest` transport [`crate::transport::cf_exchange`]; the `#[cfg(test)]`
/// override below lets the conformance twin inject a socket-free [`qfs_driver_cf::MockExchange`]
/// through the *exact same* read/apply-facet backend builder, so the twin drives the declared D1
/// facets with ZERO network. Production behaviour is unchanged — the override branch does not exist
/// in a non-test build.
#[must_use]
fn declared_d1_exchange() -> Arc<dyn qfs_driver_cf::HttpExchange> {
    #[cfg(test)]
    if let Some(exchange) = tests::declared_d1_exchange_override() {
        return exchange;
    }
    crate::transport::cf_exchange()
}

struct VaultArtifactTokenSealer {
    store: Arc<dyn Secrets>,
}

impl VaultArtifactTokenSealer {
    fn new(store: Arc<dyn Secrets>) -> Self {
        Self { store }
    }
}

impl ArtifactTokenSealer for VaultArtifactTokenSealer {
    fn ensure_can_seal(&self) -> Result<(), qfs_driver_cf::CfError> {
        Ok(())
    }

    fn seal(&self, key: &ArtifactRepoKey, token: Secret) -> Result<(), qfs_driver_cf::CfError> {
        let connection = artifact_token_connection_id(key)?;
        let credential = CredentialKey::new(DriverId::new("cf-artifact"), connection);
        self.store
            .put(&credential, token)
            .map_err(|e| qfs_driver_cf::CfError::Auth { code: e.code() })
    }
}

struct RejectingArtifactTokenSealer;

impl ArtifactTokenSealer for RejectingArtifactTokenSealer {
    fn ensure_can_seal(&self) -> Result<(), qfs_driver_cf::CfError> {
        Err(qfs_driver_cf::CfError::Auth {
            code: "secret_locked",
        })
    }

    fn seal(&self, _key: &ArtifactRepoKey, _token: Secret) -> Result<(), qfs_driver_cf::CfError> {
        Err(qfs_driver_cf::CfError::Auth {
            code: "secret_locked",
        })
    }
}

fn artifact_token_connection_id(
    key: &ArtifactRepoKey,
) -> Result<ConnectionId, qfs_driver_cf::CfError> {
    ConnectionId::new(format!(
        "repo-{}-{}",
        hex_bytes(key.namespace.as_bytes()),
        hex_bytes(key.name.as_bytes())
    ))
    .map_err(|_| qfs_driver_cf::CfError::MalformedEffect {
        verb: "UPSERT",
        path: "/cf/artifacts".to_string(),
        reason: "could not encode the Artifacts repo token key".to_string(),
    })
}

fn hex_bytes(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(char::from(HEX[usize::from(byte >> 4)]));
        out.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    out
}

fn introspect_d1(backend: &dyn CfBackend, db: &str) -> Result<Catalog, String> {
    let rels = backend
        .d1_query(
            db,
            "SELECT name AS c0, type AS c1 FROM sqlite_master \
             WHERE type IN ('table','view') AND name NOT LIKE 'sqlite_%' ORDER BY name",
            &[],
        )
        .map_err(|e| e.to_string())?;
    let mut tables = Vec::new();
    for rel in rels {
        let Some(name) = text_at(&rel.values, 0) else {
            continue;
        };
        if name.starts_with("_cf_") {
            continue;
        }
        let kind = text_at(&rel.values, 1).unwrap_or("table");
        let columns = introspect_d1_columns(backend, db, name)?;
        let relkind = if kind.eq_ignore_ascii_case("view") {
            RelationKind::View
        } else {
            RelationKind::Table
        };
        tables.push(TableCatalog::new(name.to_string(), relkind, columns));
    }
    Ok(Catalog::new(tables))
}

fn introspect_d1_columns(
    backend: &dyn CfBackend,
    db: &str,
    table: &str,
) -> Result<Vec<ColumnDef>, String> {
    let rows = backend
        .d1_query(
            db,
            "SELECT name AS c0, type AS c1, [notnull] AS c2, pk AS c3 \
             FROM pragma_table_info(?) ORDER BY cid",
            &[Param::Text(table.to_string())],
        )
        .map_err(|e| e.to_string())?;
    let mut cols = Vec::new();
    for row in rows {
        let Some(name) = text_at(&row.values, 0) else {
            continue;
        };
        let ty = text_at(&row.values, 1).unwrap_or("text");
        let notnull = int_at(&row.values, 2).unwrap_or(0) != 0;
        let pk = int_at(&row.values, 3).unwrap_or(0) != 0;
        cols.push(ColumnDef::new(
            name.to_string(),
            Dialect::Sqlite.map_type(ty),
            !notnull,
            pk,
            pk,
        ));
    }
    Ok(cols)
}

fn text_at(values: &[Value], idx: usize) -> Option<&str> {
    match values.get(idx) {
        Some(Value::Text(s)) => Some(s.as_str()),
        _ => None,
    }
}

fn int_at(values: &[Value], idx: usize) -> Option<i64> {
    match values.get(idx) {
        Some(Value::Int(n)) => Some(*n),
        Some(Value::Bool(b)) => Some(i64::from(*b)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use qfs_driver_cf::{ArtifactRepoKey, ArtifactTokenSealer, MockCfBackend, RecordedCall};
    use qfs_secrets::{InMemoryStore, Secret, Secrets};
    use qfs_types::{Row, Value};

    use super::{driver_from_backend, introspect_d1};

    #[test]
    fn d1_introspection_skips_cloudflare_internal_tables() {
        let backend = MockCfBackend::new()
            .with_d1_rows(vec![
                Row::new(vec![
                    Value::Text("_cf_KV".to_string()),
                    Value::Text("table".to_string()),
                ]),
                Row::new(vec![
                    Value::Text("artifacts".to_string()),
                    Value::Text("table".to_string()),
                ]),
            ])
            .with_d1_rows(vec![Row::new(vec![
                Value::Text("id".to_string()),
                Value::Text("TEXT".to_string()),
                Value::Int(1),
                Value::Int(1),
            ])]);

        let catalog = introspect_d1(&backend, "db").expect("catalog introspection");

        assert!(catalog.table("artifacts").is_some());
        assert!(catalog.table("_cf_KV").is_none());

        let calls = backend.recorded();
        assert_eq!(calls.len(), 2);
        let RecordedCall::D1Query { params, .. } = &calls[1] else {
            panic!("expected D1 column query");
        };
        assert_eq!(params.len(), 1);
        assert_eq!(format!("{:?}", params[0]), "Text(\"artifacts\")");
    }

    #[test]
    fn resource_discovery_registers_human_names_with_cloudflare_ids() {
        let backend = Arc::new(
            MockCfBackend::new()
                .with_d1_database("prod", qfs_driver_cf::D1DatabaseUuid::new("d1-uuid"))
                .with_kv_namespace("cache", qfs_driver_cf::KvNamespaceId::new("kv-id"))
                .with_queue(qfs_driver_cf::QueueName::new("events"))
                .with_d1_rows(vec![Row::new(vec![
                    Value::Text("users".to_string()),
                    Value::Text("table".to_string()),
                ])])
                .with_d1_rows(vec![Row::new(vec![
                    Value::Text("id".to_string()),
                    Value::Text("TEXT".to_string()),
                    Value::Int(1),
                    Value::Int(1),
                ])]),
        );
        let driver = driver_from_backend(backend.clone()).expect("discovered driver");

        assert!(driver.registry().has_d1("prod"));
        assert!(driver.registry().has_kv("cache"));
        assert!(driver.registry().has_queue("events"));
        assert!(driver.registry().has_artifacts());
        driver.kv_list_keys("cache", None, Some(10)).unwrap();
        driver.queue_tail("events", 5).unwrap();

        let calls = backend.recorded();
        assert!(matches!(calls[0], RecordedCall::D1Discovery));
        assert!(matches!(calls[1], RecordedCall::D1Query { ref db, .. } if db == "d1-uuid"));
        assert!(matches!(calls[2], RecordedCall::D1Query { ref db, .. } if db == "d1-uuid"));
        assert!(matches!(calls[3], RecordedCall::KvDiscovery));
        assert!(matches!(calls[4], RecordedCall::QueueDiscovery));
        assert!(matches!(calls[5], RecordedCall::ArtifactNamespaceDiscovery));
        assert!(matches!(calls[6], RecordedCall::KvList { ref ns, .. } if ns == "kv-id"));
        assert!(matches!(calls[7], RecordedCall::QueuePull { ref queue, .. } if queue == "events"));
    }

    #[test]
    fn cf_account_secret_resolves_from_the_qfs_vault() {
        use qfs_identity::IdentityStore as _;
        use qfs_secrets::{ConnectionId, CredentialKey, DriverId, Secret, Secrets};

        let _home = crate::testenv::HomeGuard::with_passphrase("cf-vault-test");
        crate::identity::open_identity_store()
            .unwrap()
            .create_user("op@example.com")
            .unwrap();
        let conn = crate::connection::open_system_conn().unwrap();
        crate::secret_store::db_record_consent(&conn, "cf", "mycf", "op@example.com", "").unwrap();

        let store = crate::connection::open_store().unwrap();
        let key = CredentialKey::new(
            DriverId("cf".to_string()),
            ConnectionId::new("mycf").unwrap(),
        );
        store
            .put(&key, Secret::from(qfs_secrets::generate_dek().to_vec()))
            .unwrap();

        assert!(super::resolve_cf_token("mycf").is_some());
        assert!(super::resolve_cf_token("missing").is_none());
    }

    #[test]
    fn cf_account_declared_with_a_secret_reference_resolves_at_bind_time() {
        // 20260718203325: `CREATE ACCOUNT cf 'mycf' SECRET 'env:CF_TOKEN'` records the reference on
        // the consent row; the credential resolves LAZILY at bind time from the env — NO
        // `qfs account add` (no sealed vault row). An unset env fails closed, secret-free.
        use qfs_identity::IdentityStore as _;

        // `HomeGuard` already holds `ENV_LOCK` for the whole test body (its `build` calls
        // `env_guard()`); acquiring the same non-reentrant lock again here would deadlock.
        let _home = crate::testenv::HomeGuard::with_passphrase("cf-secret-ref-test");
        crate::identity::open_identity_store()
            .unwrap()
            .create_user("op@example.com")
            .unwrap();

        // Declare the account with a bind-time SECRET reference — the desugar path of
        // `CREATE ACCOUNT cf 'mycf' SECRET 'env:CF_TOKEN'`. No token is sealed in the vault.
        crate::account::declare_account("cf", "mycf", None, Some("env:CF_TOKEN")).unwrap();

        let var = "CF_TOKEN";
        std::env::set_var(var, "cf-bearer-from-env");
        assert!(
            super::resolve_cf_token("mycf").is_some(),
            "the declared env: reference resolves the token at bind time, with no sealed vault row"
        );

        std::env::remove_var(var);
        assert!(
            super::resolve_cf_token("mycf").is_none(),
            "an unresolvable reference fails closed (no credential, no leak)"
        );
    }

    #[test]
    fn declared_d1_driver_serves_the_declared_catalog_without_introspection() {
        // Stage 2a-ii (ticket 20260718203326): the declared→CfDriver composition. A single
        // wildcard-D1 template over the backend serves the DECLARED catalog for ANY `{database}`
        // key, with ZERO mount-time `list_d1_databases`/`introspect_d1` — the no-introspection
        // model the declared `/cloudflare/d1/{database}` twin needs (the D1 relational surface from
        // the committed `CREATE SQL … TABLES(…)` row, not compiled discovery).
        use qfs_driver_cf::MockCfBackend;
        use qfs_driver_sql::{Catalog, ColumnDef, Dialect, RelationKind, TableCatalog};

        let backend = Arc::new(MockCfBackend::new());
        let catalog = Catalog::new(vec![TableCatalog::new(
            "users".to_string(),
            RelationKind::Table,
            vec![ColumnDef::new(
                "id".to_string(),
                Dialect::Sqlite.map_type("text"),
                false,
                true,
                true,
            )],
        )]);
        let driver = super::declared_d1_driver(backend.clone(), catalog);

        // The wildcard template answers ANY database key — no discovery.
        assert!(driver.registry().has_d1("prod"));
        assert!(driver.registry().has_d1("anything-else"));
        let handle = driver
            .registry()
            .d1("prod")
            .expect("template answers any key");
        assert!(handle.catalog().table("users").is_some());
        // The addressed `{database}` segment IS the api id (uuid None → path-name fallback).
        assert_eq!(handle.api_database_id("prod"), "prod");
        // ZERO backend I/O: the declared catalog replaced mount-time introspection.
        assert!(
            backend.recorded().is_empty(),
            "the declared D1 driver performs no introspection at build"
        );
    }

    #[test]
    fn artifact_token_sealer_writes_a_separate_repo_scoped_secret() {
        let store = Arc::new(InMemoryStore::new());
        let sealer = super::VaultArtifactTokenSealer::new(store.clone());
        let key = ArtifactRepoKey::new("default", "starter");

        sealer
            .seal(&key, Secret::from("repo-token-secret"))
            .expect("seal repo token");

        let connection = super::artifact_token_connection_id(&key).unwrap();
        let stored = store
            .get(&qfs_secrets::CredentialKey::new(
                qfs_secrets::DriverId::new("cf-artifact"),
                connection,
            ))
            .unwrap();
        assert_eq!(stored.expose_str(), Some("repo-token-secret"));
    }

    // -----------------------------------------------------------------------------------------
    // Stage 4 (ticket 20260718203326) — the §13 conformance twin.
    //
    // The declared `/cloudflare/d1` twin must serve the SAME behaviour the compiled `/cf` D1 path
    // serves, over `MockCfBackend`/`MockExchange`, BEFORE the compiled discovery/registration is
    // deleted (the self-hosting ratchet forbids deleting first). Two proofs:
    //   1. equivalence: given the same D1 data, the declared twin returns the same rows, the same
    //      residual, the same outward schema, and emits the same wire query as compiled — while
    //      doing ZERO introspection;
    //   2. the wire seam: the read/apply facets build their live backend through
    //      `declared_d1_backend`, which calls `cf_exchange()` internally. The test seam below
    //      injects a socket-free `MockExchange` through that exact builder, so the declared read
    //      runs with NO network.
    // -----------------------------------------------------------------------------------------

    thread_local! {
        static DECLARED_D1_EXCHANGE: std::cell::RefCell<Option<Arc<dyn qfs_driver_cf::HttpExchange>>> =
            const { std::cell::RefCell::new(None) };
    }

    /// The `#[cfg(test)]` override [`super::declared_d1_exchange`] consults: `Some` only while an
    /// [`inject_declared_d1_exchange`] guard is live on this thread, so production (which never sets
    /// it) always falls through to the real `cf_exchange()`.
    pub(super) fn declared_d1_exchange_override() -> Option<Arc<dyn qfs_driver_cf::HttpExchange>> {
        DECLARED_D1_EXCHANGE.with(|c| c.borrow().clone())
    }

    /// Drop-clears the thread-local so an injected exchange never leaks to another test on the same
    /// (reused) cargo test thread.
    struct D1ExchangeGuard;
    impl Drop for D1ExchangeGuard {
        fn drop(&mut self) {
            DECLARED_D1_EXCHANGE.with(|c| *c.borrow_mut() = None);
        }
    }

    #[must_use]
    fn inject_declared_d1_exchange(
        exchange: Arc<dyn qfs_driver_cf::HttpExchange>,
    ) -> D1ExchangeGuard {
        DECLARED_D1_EXCHANGE.with(|c| *c.borrow_mut() = Some(exchange));
        D1ExchangeGuard
    }

    #[test]
    fn declared_d1_twin_matches_compiled_cf_rows_schema_and_wire_query() {
        use qfs_core::Path;
        use qfs_driver_cf::{D1DatabaseUuid, RecordedCall};
        use qfs_driver_sql::QuerySpec;

        // The single data row every D1 path returns for `SELECT id, name FROM users`.
        let data_row = || Row::new(vec![Value::Int(1), Value::Text("alice".to_string())]);

        // COMPILED path: discovery + introspection (the table listing, then the column pragma) then
        // the data query. The uuid IS `prod`, so the compiled data query addresses the same api db
        // id the declared twin derives from the path-name fallback.
        let compiled_backend = Arc::new(
            MockCfBackend::new()
                .with_d1_database("prod", D1DatabaseUuid::new("prod"))
                .with_d1_rows(vec![Row::new(vec![
                    Value::Text("users".to_string()),
                    Value::Text("table".to_string()),
                ])])
                .with_d1_rows(vec![
                    Row::new(vec![
                        Value::Text("id".to_string()),
                        Value::Text("INTEGER".to_string()),
                        Value::Int(1),
                        Value::Int(1),
                    ]),
                    Row::new(vec![
                        Value::Text("name".to_string()),
                        Value::Text("TEXT".to_string()),
                        Value::Int(0),
                        Value::Int(0),
                    ]),
                ])
                .with_d1_rows(vec![data_row()]),
        );
        let compiled = driver_from_backend(compiled_backend.clone()).expect("compiled cf driver");

        // The declared twin serves the SAME catalog the compiled driver introspected — but WITHOUT
        // introspecting: it is handed the catalog straight from the committed `CREATE SQL` row.
        let catalog = compiled
            .registry()
            .d1("prod")
            .expect("compiled introspected the users catalog")
            .catalog()
            .clone();
        let declared_backend = Arc::new(MockCfBackend::new().with_d1_rows(vec![data_row()]));
        let declared = super::declared_d1_driver(declared_backend.clone(), catalog);

        let spec = QuerySpec::new(vec!["id".to_string(), "name".to_string()]);
        let path = Path::new("/cf/d1/prod/users");
        let (compiled_rows, compiled_residual) = compiled.execute_d1_query(&path, &spec).unwrap();
        let (declared_rows, declared_residual) = declared.execute_d1_query(&path, &spec).unwrap();

        // CONFORMANCE — identical rows and residual.
        assert_eq!(compiled_rows, declared_rows);
        assert_eq!(compiled_residual, declared_residual);

        // CONFORMANCE — identical outward schema (columns/types/pk).
        let schema_of = |d: &qfs_driver_cf::CfDriver| {
            d.registry()
                .d1("prod")
                .unwrap()
                .table("users", "/cf/d1/prod/users")
                .unwrap()
                .describe_schema()
        };
        assert_eq!(schema_of(&compiled), schema_of(&declared));

        // CONFORMANCE — the declared twin emits the SAME wire D1 query (SQL text + bound params).
        let last_d1 = |calls: Vec<RecordedCall>| {
            calls
                .into_iter()
                .rev()
                .find_map(|c| match c {
                    RecordedCall::D1Query { sql, params, .. } => Some((sql, params)),
                    _ => None,
                })
                .expect("a recorded d1 query")
        };
        let (compiled_sql, compiled_params) = last_d1(compiled_backend.recorded());
        let (declared_sql, declared_params) = last_d1(declared_backend.recorded());
        assert_eq!(
            compiled_sql, declared_sql,
            "the declared twin emits the compiled wire SQL"
        );
        assert_eq!(compiled_params, declared_params);

        // The declared twin did ZERO discovery/introspection — its only backend call is the read.
        let declared_calls = declared_backend.recorded();
        assert_eq!(
            declared_calls.len(),
            1,
            "no discovery/introspection, just the data read: {declared_calls:?}"
        );
        assert!(matches!(declared_calls[0], RecordedCall::D1Query { .. }));
    }

    #[test]
    fn declared_d1_read_over_injected_mock_exchange_does_no_network() {
        use qfs_core::Path;
        use qfs_driver_cf::MockExchange;
        use qfs_driver_sql::{Catalog, ColumnDef, Dialect, QuerySpec, RelationKind, TableCatalog};
        use qfs_http_core::HttpResponse;

        // A socket-free wire: a MockExchange scripted with the Cloudflare D1 query JSON envelope.
        let body = serde_json::json!({
            "success": true,
            "result": [{ "results": [{ "c0": 1, "c1": "alice" }] }]
        });
        let exchange = Arc::new(
            MockExchange::new()
                .with_response(HttpResponse::new(200, serde_json::to_vec(&body).unwrap())),
        );
        // Inject it through the SAME builder the read/apply facets (shell.rs / commit.rs) use.
        let _guard = inject_declared_d1_exchange(exchange.clone());
        let backend = super::declared_d1_backend("acct-id", Secret::from("cf-bearer"));

        let catalog = Catalog::new(vec![TableCatalog::new(
            "users".to_string(),
            RelationKind::Table,
            vec![
                ColumnDef::new(
                    "id".to_string(),
                    Dialect::Sqlite.map_type("integer"),
                    false,
                    true,
                    true,
                ),
                ColumnDef::new(
                    "name".to_string(),
                    Dialect::Sqlite.map_type("text"),
                    true,
                    false,
                    false,
                ),
            ],
        )]);
        let driver = super::declared_d1_driver(backend, catalog);

        // `execute_d1_query` is the exact call `read_facets::cf_scan` issues for a D1 SELECT.
        let spec = QuerySpec::new(vec!["id".to_string(), "name".to_string()]);
        let (rows, _residual) = driver
            .execute_d1_query(&Path::new("/cf/d1/prod/users"), &spec)
            .expect("the declared read runs over the injected mock exchange");
        assert_eq!(
            rows,
            vec![Row::new(vec![
                Value::Int(1),
                Value::Text("alice".to_string())
            ])]
        );

        // The request went to the injected mock (NO socket) and addressed the confined Cloudflare
        // host for the api db id taken from the path (`prod`) — the no-introspection resolution.
        let reqs = exchange.recorded();
        assert_eq!(reqs.len(), 1, "exactly one wire call: the D1 query");
        assert!(
            reqs[0].url.contains("api.cloudflare.com"),
            "confined to the cloudflare host: {}",
            reqs[0].url
        );
        assert!(
            reqs[0].url.contains("/d1/database/prod/query"),
            "addresses the path-derived api db id: {}",
            reqs[0].url
        );
    }

    #[test]
    fn declared_d1_exchange_seam_falls_through_to_production_when_unset() {
        // With no guard live, the override is absent — production always uses the real transport.
        assert!(
            declared_d1_exchange_override().is_none(),
            "the exchange seam is inert unless a test explicitly injects a mock"
        );
    }
}
