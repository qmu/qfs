//! Cloudflare live-driver composition for `/cf`.
//!
//! The driver crate owns the vendor-free D1/KV/Queues/Artifacts semantics. This binary module owns
//! only the live wiring: resolve the API token from the qfs vault, read the Cloudflare account id
//! from the connect-created mount binding, and adapt the shared reqwest transport.
//!
//! ## §13 self-hosting ratchet — the compiled `/cf` is now a MINIMAL fallback
//! D1, KV (get/put/list), and Queue *push* moved onto the committed `cloudflare.qfs` declaration
//! (the declared `/cloudflare` mount + the declared `/cloudflare/d1` twin), so the compiled `/cf`
//! no longer *discovers* or serves them (ticket 20260718203326, blueprint §13). What stays compiled
//! is only what plain declared REST cannot express:
//!
//! - **Queue PULL** — Cloudflare pull is a POST-to-read; a declared VIEW is always a GET and a
//!   declared MAP is a write effect, so there is no declared read-over-POST primitive to consume a
//!   queue. It rides the compiled queue handle (which also serves push, on the `/cf` path).
//! - **Artifacts** — Cloudflare Artifacts is a git-repo surface, not a REST resource the declared
//!   view/map shape covers, so it too stays on the compiled driver.
//!
//! Everything the declared twin covers is GONE from compiled discovery: no `list_d1_databases`, no
//! `introspect_d1`, no `list_kv_namespaces` at mount time.

use std::sync::Arc;

use qfs_driver_cf::{
    ArtifactRepoKey, ArtifactTokenSealer, CfBackend, CfDriver, CfRegistry, D1Database,
    HttpApiBackend,
};
use qfs_driver_sql::Catalog;
use qfs_secrets::{ConnectionId, CredentialKey, DriverId, Secret, Secrets};

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

/// Build the MINIMAL compiled `/cf` driver — the §13-ratchet fallback for the two surfaces plain
/// declared REST cannot express: **queue PULL** (a POST-to-read the declared view/map shape has no
/// primitive for) and **Artifacts** (a git-repo surface, not a REST resource). D1, KV, and queue
/// *push* moved onto the committed `cloudflare.qfs` declaration, so this NO LONGER discovers them:
/// there is no `list_d1_databases`/`introspect_d1`/`list_kv_namespaces` here. The queue handle it
/// registers also serves push over the `/cf` path (one handle serves both directions), but the
/// declared `/cloudflare` mount is the reviewable way push is reached.
pub(crate) fn driver_from_backend_with_artifact_sealer(
    backend: Arc<dyn CfBackend>,
    artifact_sealer: Arc<dyn ArtifactTokenSealer>,
) -> Option<CfDriver> {
    let mut registry = CfRegistry::new();

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
            "skipping Cloudflare mount; no Queue or Artifacts resources were discovered (D1/KV moved to the declared /cloudflare mount)"
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

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use qfs_driver_cf::{ArtifactRepoKey, ArtifactTokenSealer, MockCfBackend, RecordedCall};
    use qfs_secrets::{InMemoryStore, Secret, Secrets};
    use qfs_types::{Row, Value};

    use super::driver_from_backend;

    #[test]
    fn minimal_compiled_fallback_registers_only_queue_pull_and_artifacts() {
        // §13 ratchet (ticket 20260718203326): D1, KV, and queue *push* moved onto the committed
        // `cloudflare.qfs` declaration, so the compiled `/cf` no longer DISCOVERS or serves them.
        // What stays compiled is only what plain declared REST cannot express — queue PULL (a
        // POST-to-read) and Artifacts (a git-repo surface). So compiled discovery issues NO
        // `list_d1_databases`/`list_kv_namespaces`, registers no D1/KV, and builds only the
        // queue + artifacts surface.
        let backend = Arc::new(
            MockCfBackend::new()
                .with_d1_database("prod", qfs_driver_cf::D1DatabaseUuid::new("d1-uuid"))
                .with_kv_namespace("cache", qfs_driver_cf::KvNamespaceId::new("kv-id"))
                .with_queue(qfs_driver_cf::QueueName::new("events")),
        );
        let driver = driver_from_backend(backend.clone()).expect("discovered driver");

        // D1 and KV are NO LONGER served by the compiled driver (they are declared on /cloudflare).
        assert!(!driver.registry().has_d1("prod"));
        assert!(!driver.registry().has_kv("cache"));
        // Queue (for PULL) and Artifacts are the minimal compiled fallback.
        assert!(driver.registry().has_queue("events"));
        assert!(driver.registry().has_artifacts());
        driver.queue_tail("events", 5).unwrap();

        // The recorded calls prove ZERO D1/KV discovery: only queue + artifacts discovery, then the
        // queue pull. No `D1Discovery`, no `KvDiscovery`, no `introspect_d1` column pragmas.
        let calls = backend.recorded();
        assert!(
            !calls
                .iter()
                .any(|c| matches!(c, RecordedCall::D1Discovery | RecordedCall::KvDiscovery)),
            "compiled discovery must not probe D1 or KV: {calls:?}"
        );
        assert!(matches!(calls[0], RecordedCall::QueueDiscovery));
        assert!(matches!(calls[1], RecordedCall::ArtifactNamespaceDiscovery));
        assert!(matches!(calls[2], RecordedCall::QueuePull { ref queue, .. } if queue == "events"));
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
    // The declared `/cloudflare/d1` twin (ticket 20260718203326).
    //
    // The Stage-4 equivalence twin (declared ≡ compiled `/cf` D1, over `MockCfBackend`) fired the
    // §13 ratchet — it was GREEN at `f1bd5f3`, which authorized deleting the compiled D1 discovery
    // in Stage 5. With compiled D1 introspection now retired, there is no compiled counterpart to
    // compare against, so the equivalence test retired with it (git history holds the proof). The
    // declared D1 path stands on its own coverage below:
    //   * `declared_d1_driver_serves_the_declared_catalog_without_introspection` (above) — the
    //     wildcard template serves the committed catalog for ANY key with ZERO backend I/O;
    //   * `declared_d1_read_over_injected_mock_exchange_does_no_network` (below) — the read/apply
    //     facets build their live backend through `declared_d1_backend`, which calls `cf_exchange()`
    //     internally; the test seam injects a socket-free `MockExchange` through that exact builder,
    //     so the declared read runs with NO network and addresses the confined Cloudflare host.
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
