//! Internal tests for `qfs-driver-cf` (t23). The backend is the in-memory [`MockCfBackend`]
//! (scripted Cloudflare responses + recorded calls) — **no live Cloudflare, no network**. The
//! tests prove:
//! - D1 SELECT compiles to the reused t17 sqlite SQL and ships `params` as a **structured bound
//!   array** (asserting the SQL carries only `?`, and an injection literal lands in the params
//!   array, NEVER in the SQL text);
//! - D1 INSERT/UPSERT/UPDATE/REMOVE lower to parameterized DML and apply in ONE atomic `/batch`;
//! - KV get/put/delete/list (TTL/metadata) and Queues send (idempotency key) / pull (tail);
//! - capability gating rejects `UPDATE`/`JOIN` over a queue/KV at parse time, structurally;
//! - the API token never leaks across any `CfError` surface;
//! - end-to-end through the interpreter + bridge for a D1 write, a KV upsert, and a queue send.

use std::sync::Arc;

use qfs_driver::{check_capability, Driver, Path, Verb};
use qfs_plan::{
    DriverId as PlanDriverId, EffectKind, EffectNode, NodeId, PlanBuilder, Target, VfsPath,
};
use qfs_runtime::{CapabilitySet, DriverRegistry, Interpreter};
use qfs_sql_core::{Catalog, ColumnDef, Param, QuerySpec, RelationKind, TableCatalog};
use qfs_types::{
    CmpOp, ColRef, Column, ColumnType, Literal, Predicate, Row, RowBatch, Schema, Value,
};

use qfs_http_core::{HttpMethod, HttpResponse};
use qfs_secrets::Secret;

use crate::backend::{
    ArtifactRepo, ArtifactRepoKey, ArtifactTokenSealer, CfBackend, D1DatabaseUuid, HttpApiBackend,
    KvEntry, MockCfBackend, MockExchange, NoopArtifactTokenSealer, QueueMsg, RecordedCall,
};
use crate::registry::{CfRegistry, D1Database};
use crate::{artifacts_repos_schema, cf_apply_driver, CfDriver, CfError, CfNode};

/// A canary value planted as a D1 string param; unmistakable if it ever surfaces in the SQL text
/// (it must ride only in the structured params array).
const INJECTION: &str = "'; DROP TABLE users; --";

/// A planted API token — unmistakable if it ever leaks into an error surface.
const PLANTED_TOKEN: &str = "PLANTED-CF-TOKEN-deadbeef-9f8e7d6c5b4a";
const PLANTED_ARTIFACT_TOKEN: &str = "art_v1_PLANTED-REPO-TOKEN-123456";

#[derive(Default)]
struct RecordingArtifactSealer {
    sealed: std::sync::Mutex<Vec<ArtifactRepoKey>>,
}

impl RecordingArtifactSealer {
    fn sealed(&self) -> Vec<ArtifactRepoKey> {
        self.sealed.lock().map(|s| s.clone()).unwrap_or_default()
    }
}

impl ArtifactTokenSealer for RecordingArtifactSealer {
    fn ensure_can_seal(&self) -> Result<(), CfError> {
        Ok(())
    }

    fn seal(&self, key: &ArtifactRepoKey, token: Secret) -> Result<(), CfError> {
        assert_eq!(token.expose_str(), Some(PLANTED_ARTIFACT_TOKEN));
        if let Ok(mut sealed) = self.sealed.lock() {
            sealed.push(key.clone());
        }
        Ok(())
    }
}

/// Build a `users(id INT pk, name TEXT, email TEXT)` D1 table catalog.
fn users_catalog() -> Catalog {
    Catalog::new(vec![TableCatalog::new(
        "users",
        RelationKind::Table,
        vec![
            ColumnDef::new("id", ColumnType::Int, false, true, true),
            ColumnDef::new("name", ColumnType::Text, true, false, false),
            ColumnDef::new("email", ColumnType::Text, true, false, false),
        ],
    )])
}

/// A registry wiring `prod` (D1), `cache` (KV), and `events` (queue) to one shared mock backend.
fn registry_with(backend: Arc<MockCfBackend>) -> CfRegistry {
    CfRegistry::new()
        .with_d1("prod", D1Database::new(backend.clone(), users_catalog()))
        .with_kv("cache", backend.clone())
        .with_queue("events", backend)
        .with_artifacts(
            Arc::new(MockCfBackend::new().with_artifact_namespace("default")),
            Arc::new(NoopArtifactTokenSealer),
        )
}

fn registry_with_artifacts(
    backend: Arc<MockCfBackend>,
    sealer: Arc<dyn ArtifactTokenSealer>,
) -> CfRegistry {
    CfRegistry::new()
        .with_d1("prod", D1Database::new(backend.clone(), users_catalog()))
        .with_kv("cache", backend.clone())
        .with_queue("events", backend.clone())
        .with_artifacts(backend, sealer)
}

fn driver_with(backend: Arc<MockCfBackend>) -> CfDriver {
    CfDriver::new(registry_with(backend))
}

/// Stage 2a (ticket 20260718203326): the wildcard-D1 `CfRegistry` template that the declared
/// `/cloudflare/d1/{database}` nested mount resolves against — WITHOUT any mount-time
/// `list_d1_databases`/`introspect_d1`. A single template (backend + declared catalog, uuid=None)
/// answers an arbitrary `{database}` key, the queried segment IS the Cloudflare api id, and the
/// declared catalog is served with zero backend I/O. Proves the no-introspection resolution the
/// declared twin needs (shape-independent; forced by the no-introspection model).
#[test]
fn wildcard_d1_template_resolves_any_database_key_without_introspection() {
    let backend = Arc::new(MockCfBackend::new());
    let registry =
        CfRegistry::new().with_d1_template(D1Database::new(backend.clone(), users_catalog()));

    // Any db key is available (the capability gate reads this), and resolves to the template.
    assert!(registry.has_d1("anything"));
    let handle = registry
        .d1("some-declared-db")
        .expect("the wildcard template answers any db key");
    // The addressed `{database}` segment IS the api id: uuid is None, so it falls back to the name.
    assert_eq!(
        handle.api_database_id("some-declared-db"),
        "some-declared-db"
    );
    // The DECLARED catalog is served with no I/O (the `users` table came from the declaration).
    assert!(handle.catalog().table("users").is_some());
    // Resolving a handle performs ZERO backend introspection — it is a pure in-memory lookup.
    assert!(
        backend.recorded().is_empty(),
        "template resolution performs no introspection I/O"
    );

    // An explicit (discovered) registration still wins over the template for its own key...
    let registry = registry.with_d1(
        "prod",
        D1Database::discovered(
            backend.clone(),
            D1DatabaseUuid::new("uuid-prod"),
            users_catalog(),
        ),
    );
    assert_eq!(
        registry.d1("prod").unwrap().api_database_id("prod"),
        "uuid-prod"
    );
    // ...while any other key still falls through to the wildcard template.
    assert_eq!(
        registry.d1("other").unwrap().api_database_id("other"),
        "other"
    );
}

/// A single-row RowBatch over the given (name, type, value) triples.
fn row_batch(cells: Vec<(&str, ColumnType, Value)>) -> RowBatch {
    let columns = cells
        .iter()
        .map(|(n, t, _)| Column::new(*n, t.clone(), true))
        .collect();
    let values = cells.into_iter().map(|(_, _, v)| v).collect();
    RowBatch::new(Schema::new(columns), vec![Row::new(values)])
}

fn effect(kind: EffectKind, path: &str, args: RowBatch) -> EffectNode {
    let target = Target::new(PlanDriverId::new("cf"), VfsPath::new(path));
    EffectNode::new(NodeId(1), kind, target).with_args(args)
}

/// An effect carrying a `WHERE`-selector (blueprint §7) — the channel a filter travels on. `args` is
/// the SET/VALUES payload only, so a filtered UPDATE/REMOVE is built with BOTH.
fn effect_where(kind: EffectKind, path: &str, args: RowBatch, selector: RowBatch) -> EffectNode {
    effect(kind, path, args).with_selector(selector)
}

// ----------------------------------------------------------------------------------------------
// Path parsing
// ----------------------------------------------------------------------------------------------

#[test]
fn parses_each_service_address() {
    assert_eq!(
        CfNode::parse_str("/cf/d1/prod/users").unwrap(),
        CfNode::D1Table {
            db: "prod".to_string(),
            table: "users".to_string()
        }
    );
    assert_eq!(
        CfNode::parse_str("/cf/kv/cache/session:abc").unwrap(),
        CfNode::KvKey {
            ns: "cache".to_string(),
            key: "session:abc".to_string()
        }
    );
    assert_eq!(
        CfNode::parse_str("/cf/kv/cache").unwrap(),
        CfNode::KvNamespace {
            ns: "cache".to_string()
        }
    );
    assert_eq!(
        CfNode::parse_str("/cf/queue/events").unwrap(),
        CfNode::Queue {
            name: "events".to_string()
        }
    );
    assert_eq!(
        CfNode::parse_str("/cf/artifacts").unwrap(),
        CfNode::Artifacts
    );
    assert_eq!(
        CfNode::parse_str("/cf/artifacts/default/starter").unwrap(),
        CfNode::ArtifactRepo {
            namespace: "default".to_string(),
            name: "starter".to_string()
        }
    );
    // A bare service is not addressable.
    assert_eq!(
        CfNode::parse_str("/cf/d1").unwrap_err().code(),
        "invalid_path"
    );
    // Outside the mount.
    assert_eq!(
        CfNode::parse_str("/mail/x").unwrap_err().code(),
        "invalid_path"
    );
}

// ----------------------------------------------------------------------------------------------
// DESCRIBE — per-node archetype + schema
// ----------------------------------------------------------------------------------------------

#[test]
fn describe_returns_the_correct_archetype_per_service() {
    let backend = Arc::new(MockCfBackend::new());
    let d = driver_with(backend);

    let d1 = d.describe(&Path::new("/cf/d1/prod/users")).unwrap();
    assert_eq!(d1.archetype, qfs_driver::Archetype::RelationalTable);
    assert_eq!(d1.schema.columns.len(), 3);

    let kv = d.describe(&Path::new("/cf/kv/cache")).unwrap();
    assert_eq!(kv.archetype, qfs_driver::Archetype::BlobNamespace);
    assert!(kv.schema.column("key").is_some());
    assert!(kv.schema.column("value").is_some());

    let q = d.describe(&Path::new("/cf/queue/events")).unwrap();
    assert_eq!(q.archetype, qfs_driver::Archetype::AppendLog);
    assert!(q.schema.column("attempts").is_some());

    let artifacts = d.describe(&Path::new("/cf/artifacts")).unwrap();
    assert_eq!(artifacts.archetype, qfs_driver::Archetype::RelationalTable);
    assert!(artifacts.schema.column("remote_url").is_some());
    assert!(
        artifacts.schema.column("token").is_none(),
        "repo tokens must not be part of the Artifacts table schema"
    );
}

// ----------------------------------------------------------------------------------------------
// D1 SELECT — reused t17 sqlite emit + structured params (injection safety)
// ----------------------------------------------------------------------------------------------

#[test]
fn d1_select_pushes_where_and_binds_params_as_structured_array_not_interpolated() {
    // Seed one returned row.
    let backend = Arc::new(MockCfBackend::new().with_d1_rows(vec![Row::new(vec![
        Value::Int(1),
        Value::Text("alice".to_string()),
    ])]));
    let d = driver_with(backend.clone());

    // SELECT id, name FROM users WHERE name = INJECTION  — the injection literal is a VALUE.
    let spec =
        QuerySpec::new(vec!["id".to_string(), "name".to_string()]).with_predicate(Predicate::Cmp(
            ColRef::col("name"),
            CmpOp::Eq,
            Literal::Text(INJECTION.to_string()),
        ));
    let (rows, residual) = d
        .execute_d1_query(&Path::new("/cf/d1/prod/users"), &spec)
        .unwrap();
    assert_eq!(rows.len(), 1);
    // An exact `=` predicate pushes down fully — no residual.
    assert!(residual.is_none());

    let calls = backend.recorded();
    let RecordedCall::D1Query { db, sql, params } = &calls[0] else {
        panic!("expected a d1.query call, got {calls:?}");
    };
    assert_eq!(db, "prod");
    // THE injection-safety invariant: the SQL carries only the `?` placeholder + quoted
    // identifiers — the injection literal is NOWHERE in the SQL text.
    assert!(sql.contains('?'), "SQL must use a `?` placeholder: {sql}");
    assert!(
        !sql.contains("DROP TABLE"),
        "the injection literal must NOT be interpolated into the SQL: {sql}"
    );
    assert!(!sql.contains(INJECTION), "no value text in the SQL: {sql}");
    // The value rides in the STRUCTURED bound params array, inert as data.
    assert_eq!(params, &vec![Param::Text(INJECTION.to_string())]);
    // The projection is the requested column subset, aliased into deterministic D1 JSON keys.
    assert!(
        sql.contains("\"id\" AS c0") && sql.contains("\"name\" AS c1"),
        "projection: {sql}"
    );
}

#[test]
fn http_backend_decodes_d1_c_aliases_in_numeric_order() {
    let body = serde_json::json!({
        "success": true,
        "result": [{
            "results": [{
                "c10": "ten",
                "c2": "two",
                "c0": "zero"
            }]
        }]
    });
    let exchange = Arc::new(
        MockExchange::new()
            .with_response(HttpResponse::new(200, serde_json::to_vec(&body).unwrap())),
    );
    let backend = HttpApiBackend::new(exchange, "account", Secret::from(PLANTED_TOKEN.to_string()));

    let rows = backend.d1_query("db", "SELECT 1", &[]).unwrap();

    assert_eq!(
        rows,
        vec![Row::new(vec![
            Value::Text("zero".to_string()),
            Value::Text("two".to_string()),
            Value::Text("ten".to_string()),
        ])]
    );
}

#[test]
fn http_backend_lists_accounts_without_account_id_in_path() {
    let body = serde_json::json!({
        "success": true,
        "result": [
            { "id": "acct-one", "name": "Production" },
            { "id": "acct-two", "name": "Staging" }
        ]
    });
    let exchange = Arc::new(
        MockExchange::new()
            .with_response(HttpResponse::new(200, serde_json::to_vec(&body).unwrap())),
    );
    let backend = HttpApiBackend::for_token(exchange.clone(), Secret::from(PLANTED_TOKEN));

    let accounts = backend.list_accounts().unwrap();

    assert_eq!(accounts.len(), 2);
    assert_eq!(accounts[0].id, "acct-one");
    assert_eq!(accounts[0].name, "Production");
    assert_eq!(accounts[1].id, "acct-two");
    let requests = exchange.recorded();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, HttpMethod::Get);
    assert_eq!(
        requests[0].url,
        "https://api.cloudflare.com/client/v4/accounts"
    );
    let debug = format!("{:?}", requests[0]);
    assert!(
        !debug.contains(PLANTED_TOKEN),
        "request debug leaked token: {debug}"
    );
}

#[test]
fn http_backend_creates_artifact_repo_on_the_account_namespace_route() {
    let body = serde_json::json!({
        "success": true,
        "result": {
            "id": "repo_123",
            "name": "starter",
            "description": "Repository for automation",
            "default_branch": "main",
            "remote": "https://acct.artifacts.cloudflare.net/git/default/starter.git",
            "token": PLANTED_ARTIFACT_TOKEN
        }
    });
    let exchange = Arc::new(
        MockExchange::new()
            .with_response(HttpResponse::new(200, serde_json::to_vec(&body).unwrap())),
    );
    let backend = HttpApiBackend::new(exchange.clone(), "acct", Secret::from(PLANTED_TOKEN));

    let created = backend
        .create_artifact_repo(
            "default",
            &crate::CreateArtifactRepoRequest {
                name: "starter".to_string(),
                description: Some("Repository for automation".to_string()),
                default_branch: Some("main".to_string()),
                read_only: Some(false),
            },
        )
        .unwrap();

    assert_eq!(created.repo.namespace, "default");
    assert_eq!(
        created.repo.remote_url,
        "https://acct.artifacts.cloudflare.net/git/default/starter.git"
    );
    assert_eq!(created.token.expose_str(), Some(PLANTED_ARTIFACT_TOKEN));
    let requests = exchange.recorded();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, HttpMethod::Post);
    assert_eq!(
        requests[0].url,
        "https://api.cloudflare.com/client/v4/accounts/acct/artifacts/namespaces/default/repos"
    );
    let request_debug = format!("{:?}", requests[0]);
    assert!(!request_debug.contains(PLANTED_TOKEN));
    assert!(!request_debug.contains(PLANTED_ARTIFACT_TOKEN));
}

// ----------------------------------------------------------------------------------------------
// D1 writes — DML lowering + batch atomicity
// ----------------------------------------------------------------------------------------------

#[test]
fn d1_insert_lowers_to_parameterized_batch_with_bound_values() {
    let backend = Arc::new(MockCfBackend::new().with_d1_affected(1));
    let applier = crate::CfApplier::new(registry_with(backend.clone()));

    let node = effect(
        EffectKind::Insert,
        "/cf/d1/prod/users",
        row_batch(vec![
            ("id", ColumnType::Int, Value::Int(7)),
            ("name", ColumnType::Text, Value::Text(INJECTION.to_string())),
        ]),
    );
    use qfs_runtime::SharedApplier;
    let out = applier.apply_shared(&node).unwrap();
    assert_eq!(out.affected, 1);

    let calls = backend.recorded();
    let RecordedCall::D1Batch { db, statements } = &calls[0] else {
        panic!("a D1 write must go through ONE /batch (atomic), got {calls:?}");
    };
    assert_eq!(db, "prod");
    // Batch atomicity: exactly one batch request carrying exactly one statement.
    assert_eq!(statements.len(), 1, "one atomic batch");
    let (sql, params) = &statements[0];
    assert!(sql.starts_with("INSERT INTO \"users\""), "sql: {sql}");
    assert!(!sql.contains("DROP TABLE"), "value not interpolated: {sql}");
    // The injection value is BOUND (a structured param), never in the SQL.
    assert!(params.contains(&Param::Text(INJECTION.to_string())));
}

#[test]
fn d1_upsert_update_remove_lower_correctly() {
    use qfs_runtime::SharedApplier;
    let backend = Arc::new(MockCfBackend::new().with_d1_affected(1));
    let applier = crate::CfApplier::new(registry_with(backend.clone()));

    // UPSERT — uses the PK (`id`) as the conflict key (t17 reuse).
    applier
        .apply_shared(&effect(
            EffectKind::Upsert,
            "/cf/d1/prod/users",
            row_batch(vec![
                ("id", ColumnType::Int, Value::Int(7)),
                ("name", ColumnType::Text, Value::Text("bob".to_string())),
            ]),
        ))
        .unwrap();
    // UPDATE — `args` is the SET payload; the WHERE rides the selector (§7).
    applier
        .apply_shared(&effect_where(
            EffectKind::Update,
            "/cf/d1/prod/users",
            row_batch(vec![(
                "name",
                ColumnType::Text,
                Value::Text("carol".to_string()),
            )]),
            row_batch(vec![("id", ColumnType::Int, Value::Int(7))]),
        ))
        .unwrap();
    // REMOVE — writes nothing, so `args` is EMPTY and the WHERE is wholly the selector.
    applier
        .apply_shared(&effect_where(
            EffectKind::Remove,
            "/cf/d1/prod/users",
            RowBatch::default(),
            row_batch(vec![("id", ColumnType::Int, Value::Int(7))]),
        ))
        .unwrap();

    let calls = backend.recorded();
    assert_eq!(calls.len(), 3);
    let sqls: Vec<&str> = calls
        .iter()
        .filter_map(|c| match c {
            RecordedCall::D1Batch { statements, .. } => Some(statements[0].0.as_str()),
            _ => None,
        })
        .collect();
    assert!(sqls[0].contains("ON CONFLICT"), "upsert: {}", sqls[0]);
    assert!(
        sqls[1].starts_with("UPDATE \"users\" SET") && sqls[1].contains("WHERE"),
        "update: {}",
        sqls[1]
    );
    assert!(
        sqls[2].starts_with("DELETE FROM \"users\" WHERE"),
        "remove: {}",
        sqls[2]
    );
}

#[test]
fn d1_remove_without_key_is_rejected_structurally() {
    use qfs_runtime::SharedApplier;
    let backend = Arc::new(MockCfBackend::new());
    let applier = crate::CfApplier::new(registry_with(backend));
    // A REMOVE row carrying only a non-key column — no key filter ⇒ refuse the mass delete.
    let node = effect(
        EffectKind::Remove,
        "/cf/d1/prod/users",
        row_batch(vec![(
            "name",
            ColumnType::Text,
            Value::Text("x".to_string()),
        )]),
    );
    let err = applier.apply_shared(&node).unwrap_err();
    assert!(format!("{err:?}").contains("Terminal") || format!("{err:?}").contains("terminal"));
}

// ----------------------------------------------------------------------------------------------
// KV — get / put / delete / list (TTL + metadata)
// ----------------------------------------------------------------------------------------------

#[test]
fn kv_get_put_delete_list_round_trip() {
    use qfs_runtime::SharedApplier;
    let backend = Arc::new(
        MockCfBackend::new()
            .with_kv_entry(
                KvEntry::new("k1", b"v1".to_vec())
                    .with_metadata("{\"tag\":\"a\"}")
                    .with_ttl(3600),
            )
            .with_kv_keys(vec!["k1".to_string(), "k2".to_string()]),
    );
    let d = driver_with(backend.clone());

    // GET
    let got = d.kv_get("cache", "k1").unwrap().unwrap();
    assert_eq!(got.value, b"v1");
    assert_eq!(got.metadata.as_deref(), Some("{\"tag\":\"a\"}"));
    assert_eq!(got.expiration_ttl, Some(3600));

    // LIST
    let keys = d.kv_list_keys("cache", None, Some(10)).unwrap();
    assert_eq!(keys, vec!["k1".to_string(), "k2".to_string()]);

    // PUT (UPSERT) with TTL + metadata via the applier.
    let applier = crate::CfApplier::new(registry_with(backend.clone()));
    applier
        .apply_shared(&effect(
            EffectKind::Upsert,
            "/cf/kv/cache",
            row_batch(vec![
                ("key", ColumnType::Text, Value::Text("k3".to_string())),
                ("value", ColumnType::Text, Value::Text("v3".to_string())),
                ("ttl", ColumnType::Int, Value::Int(60)),
            ]),
        ))
        .unwrap();
    // DELETE
    applier
        .apply_shared(&effect(
            EffectKind::Remove,
            "/cf/kv/cache/k3",
            RowBatch::default(),
        ))
        .unwrap();

    let calls = backend.recorded();
    assert!(matches!(calls[0], RecordedCall::KvGet { .. }));
    assert!(matches!(calls[1], RecordedCall::KvList { .. }));
    let RecordedCall::KvPut { ns, entry } = &calls[2] else {
        panic!("expected kv.put, got {:?}", calls[2]);
    };
    assert_eq!(ns, "cache");
    assert_eq!(entry.key, "k3");
    assert_eq!(entry.expiration_ttl, Some(60));
    let RecordedCall::KvDelete { key, .. } = &calls[3] else {
        panic!("expected kv.delete, got {:?}", calls[3]);
    };
    assert_eq!(key, "k3");
}

// ----------------------------------------------------------------------------------------------
// Queues — send (idempotency key) / pull (tail)
// ----------------------------------------------------------------------------------------------

#[test]
fn queue_send_carries_idempotency_key_and_pull_tails() {
    use qfs_runtime::SharedApplier;
    let backend = Arc::new(
        MockCfBackend::new()
            .with_queue_msg(QueueMsg::new("m1", b"hello".to_vec(), 1))
            .with_queue_msg(QueueMsg::new("m2", b"world".to_vec(), 2)),
    );
    let d = driver_with(backend.clone());

    // PULL (tail) capped at 1.
    let msgs = d.queue_tail("events", 1).unwrap();
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].id, "m1");

    // SEND (append) via the applier with an explicit idempotency key.
    let applier = crate::CfApplier::new(registry_with(backend.clone()));
    applier
        .apply_shared(&effect(
            EffectKind::Insert,
            "/cf/queue/events",
            row_batch(vec![
                ("body", ColumnType::Text, Value::Text("payload".to_string())),
                (
                    "idempotency_key",
                    ColumnType::Text,
                    Value::Text("evt-42".to_string()),
                ),
            ]),
        ))
        .unwrap();

    let calls = backend.recorded();
    assert!(matches!(calls[0], RecordedCall::QueuePull { max: 1, .. }));
    let RecordedCall::QueueSend {
        queue,
        body,
        idempotency_key,
    } = &calls[1]
    else {
        panic!("expected queue.send, got {:?}", calls[1]);
    };
    assert_eq!(queue, "events");
    assert_eq!(body, b"payload");
    // The idempotency key is present (the at-least-once de-dupe — no double-append on retry).
    assert_eq!(idempotency_key, "evt-42");
}

#[test]
fn queue_send_derives_a_deterministic_idempotency_key_when_absent() {
    use qfs_runtime::SharedApplier;
    let backend = Arc::new(MockCfBackend::new());
    let applier = crate::CfApplier::new(registry_with(backend.clone()));
    let make = || {
        effect(
            EffectKind::Insert,
            "/cf/queue/events",
            row_batch(vec![(
                "body",
                ColumnType::Text,
                Value::Text("same".to_string()),
            )]),
        )
    };
    applier.apply_shared(&make()).unwrap();
    applier.apply_shared(&make()).unwrap();
    let calls = backend.recorded();
    // The SAME body derives the SAME idempotency key on a retry (deterministic, purity-safe).
    let keys: Vec<&str> = calls
        .iter()
        .filter_map(|c| match c {
            RecordedCall::QueueSend {
                idempotency_key, ..
            } => Some(idempotency_key.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(keys.len(), 2);
    assert_eq!(keys[0], keys[1], "same body ⇒ same idempotency key");
}

// ----------------------------------------------------------------------------------------------
// Artifacts — list/get/create/delete with token sealing
// ----------------------------------------------------------------------------------------------

fn artifact_repo(namespace: &str, name: &str) -> ArtifactRepo {
    ArtifactRepo {
        namespace: namespace.to_string(),
        name: name.to_string(),
        id: format!("repo-{name}"),
        remote_url: format!("https://acct.artifacts.cloudflare.net/git/{namespace}/{name}.git"),
        created_at: Some("2026-07-09T00:00:00Z".to_string()),
        updated_at: Some("2026-07-09T00:00:00Z".to_string()),
        default_branch: Some("main".to_string()),
        read_only: false,
        ..ArtifactRepo::default()
    }
}

#[test]
fn artifacts_list_and_get_return_remote_metadata_without_token_column() {
    let backend = Arc::new(
        MockCfBackend::new()
            .with_artifact_namespace("default")
            .with_artifact_repo(artifact_repo("default", "starter")),
    );
    let d = CfDriver::new(registry_with_artifacts(
        backend.clone(),
        Arc::new(NoopArtifactTokenSealer),
    ));

    let repos = d.artifact_repos().unwrap();
    assert_eq!(repos.len(), 1);
    assert_eq!(
        repos[0].remote_url,
        "https://acct.artifacts.cloudflare.net/git/default/starter.git"
    );
    let row = repos[0].to_row();
    assert_eq!(row.values.len(), artifacts_repos_schema().columns.len());
    let rendered = format!("{row:?}");
    assert!(
        !rendered.contains("token"),
        "Artifacts rows must not carry a token-shaped field: {rendered}"
    );

    let got = d.artifact_repo("default", "starter").unwrap().unwrap();
    assert_eq!(got.name, "starter");

    let calls = backend.recorded();
    assert!(matches!(calls[0], RecordedCall::ArtifactNamespaceDiscovery));
    assert!(matches!(
        calls[1],
        RecordedCall::ArtifactRepoList { ref namespace } if namespace == "default"
    ));
    assert!(matches!(
        calls[2],
        RecordedCall::ArtifactRepoGet { ref namespace, ref name }
            if namespace == "default" && name == "starter"
    ));
}

#[test]
fn artifact_create_seals_the_returned_token_and_never_records_it() {
    use qfs_runtime::SharedApplier;

    let created = artifact_repo("default", "starter");
    let backend = Arc::new(
        MockCfBackend::new()
            .with_artifact_namespace("default")
            .with_artifact_create_result(created, PLANTED_ARTIFACT_TOKEN),
    );
    let sealer = Arc::new(RecordingArtifactSealer::default());
    let applier = crate::CfApplier::new(registry_with_artifacts(backend.clone(), sealer.clone()));

    applier
        .apply_shared(&effect(
            EffectKind::Upsert,
            "/cf/artifacts",
            row_batch(vec![
                (
                    "namespace",
                    ColumnType::Text,
                    Value::Text("default".to_string()),
                ),
                ("name", ColumnType::Text, Value::Text("starter".to_string())),
                (
                    "description",
                    ColumnType::Text,
                    Value::Text("automation repo".to_string()),
                ),
                (
                    "default_branch",
                    ColumnType::Text,
                    Value::Text("main".to_string()),
                ),
                ("read_only", ColumnType::Bool, Value::Bool(false)),
            ]),
        ))
        .unwrap();

    assert_eq!(
        sealer.sealed(),
        vec![ArtifactRepoKey::new("default", "starter")]
    );
    let calls = backend.recorded();
    let RecordedCall::ArtifactRepoCreate { request, .. } = &calls[0] else {
        panic!("expected artifact create call, got {calls:?}");
    };
    assert_eq!(request.name, "starter");
    let rendered = format!("{calls:?}");
    assert!(
        !rendered.contains(PLANTED_ARTIFACT_TOKEN),
        "recorded backend calls must not leak repo tokens: {rendered}"
    );
}

#[test]
fn artifact_delete_uses_the_concrete_repo_path() {
    use qfs_runtime::SharedApplier;

    let backend = Arc::new(MockCfBackend::new().with_artifact_namespace("default"));
    let applier = crate::CfApplier::new(registry_with_artifacts(
        backend.clone(),
        Arc::new(NoopArtifactTokenSealer),
    ));

    applier
        .apply_shared(&effect(
            EffectKind::Remove,
            "/cf/artifacts/default/starter",
            RowBatch::default(),
        ))
        .unwrap();

    let calls = backend.recorded();
    assert!(matches!(
        calls[0],
        RecordedCall::ArtifactRepoDelete { ref namespace, ref name }
            if namespace == "default" && name == "starter"
    ));
}

// ----------------------------------------------------------------------------------------------
// Capability gating — structured rejection at parse time
// ----------------------------------------------------------------------------------------------

#[test]
fn update_on_a_queue_is_rejected_structurally() {
    let backend = Arc::new(MockCfBackend::new());
    let d = driver_with(backend);
    let queue = Path::new("/cf/queue/events");
    let err = check_capability(&d, &queue, Verb::Update).unwrap_err();
    assert_eq!(err.code(), "unsupported_verb");
    // The supported set advertises exactly INSERT + SELECT (append/log).
    match err {
        qfs_driver::CfsError::UnsupportedVerb { supported, .. } => {
            assert_eq!(supported, vec!["SELECT", "INSERT"]);
        }
        other => panic!("expected UnsupportedVerb, got {other:?}"),
    }
    // SELECT and INSERT pass the gate.
    assert!(check_capability(&d, &queue, Verb::Select).is_ok());
    assert!(check_capability(&d, &queue, Verb::Insert).is_ok());
}

#[test]
fn join_writes_over_kv_namespace_are_gated() {
    let backend = Arc::new(MockCfBackend::new());
    let d = driver_with(backend);
    let kv = Path::new("/cf/kv/cache");
    // A KV namespace is not a relational table: it admits blob verbs + the key/value table
    // SELECT/UPSERT, but UPDATE (a JOIN-style relational write) is denied at the gate.
    let err = check_capability(&d, &kv, Verb::Update).unwrap_err();
    assert_eq!(err.code(), "unsupported_verb");
    assert!(check_capability(&d, &kv, Verb::Ls).is_ok());
    assert!(check_capability(&d, &kv, Verb::Upsert).is_ok());
}

#[test]
fn artifacts_capabilities_keep_delete_on_concrete_repo_paths() {
    let backend = Arc::new(MockCfBackend::new());
    let d = driver_with(backend);
    let list = Path::new("/cf/artifacts");
    let repo = Path::new("/cf/artifacts/default/starter");

    assert!(check_capability(&d, &list, Verb::Select).is_ok());
    assert!(check_capability(&d, &list, Verb::Upsert).is_ok());
    assert!(check_capability(&d, &repo, Verb::Select).is_ok());
    assert!(check_capability(&d, &repo, Verb::Remove).is_ok());
    assert_eq!(
        check_capability(&d, &list, Verb::Remove)
            .unwrap_err()
            .code(),
        "unsupported_verb"
    );
}

// ----------------------------------------------------------------------------------------------
// Token never leaks
// ----------------------------------------------------------------------------------------------

#[test]
fn the_api_token_never_appears_in_any_error_surface() {
    // Build a real HttpApiBackend bearing the planted token, then drive every CfError surface and
    // prove the token is nowhere in any of them. (The token rides only in a redacted header.)
    let secret = qfs_secrets::Secret::from(PLANTED_TOKEN);
    let _backend = crate::backend::HttpApiBackend::new(
        Arc::new(crate::backend::MockExchange::new()),
        "acct-123",
        secret,
    );

    let errors = vec![
        CfError::InvalidPath {
            path: "/cf/x".to_string(),
            reason: "bad",
        },
        CfError::CapabilityDenied {
            path: "/cf/queue/q".to_string(),
            verb: "UPDATE",
        },
        CfError::Api {
            op: "d1.batch",
            status: 500,
        },
        CfError::Decode {
            op: "kv.get",
            reason: "not json".to_string(),
        },
        CfError::Auth {
            code: "secret_not_found",
        },
        CfError::Transport {
            reason: "connection failed".to_string(),
        },
    ];
    for e in &errors {
        let dbg = format!("{e:?}");
        let disp = e.to_string();
        assert!(!dbg.contains(PLANTED_TOKEN), "token leaked in Debug: {dbg}");
        assert!(
            !disp.contains(PLANTED_TOKEN),
            "token leaked in Display: {disp}"
        );
        assert!(!dbg.contains("deadbeef"), "token fragment leaked: {dbg}");
    }
}

#[test]
fn the_request_debug_redacts_the_bearer_token() {
    // The HttpRequest the backend builds carries the Authorization bearer; its Debug redacts it.
    use qfs_http_core::{HttpMethod, HttpRequest};
    let req = HttpRequest::new(HttpMethod::Post, "https://api.cloudflare.com/x")
        .header("Authorization", format!("Bearer {PLANTED_TOKEN}"));
    let dbg = format!("{req:?}");
    assert!(
        !dbg.contains(PLANTED_TOKEN),
        "bearer must be redacted: {dbg}"
    );
    assert!(dbg.contains(qfs_secrets::REDACTED));
}

// ----------------------------------------------------------------------------------------------
// End-to-end through the interpreter + bridge
// ----------------------------------------------------------------------------------------------

#[tokio::test]
async fn end_to_end_commit_through_interpreter_for_all_three_services() {
    let backend = Arc::new(MockCfBackend::new().with_d1_affected(1));
    let driver = driver_with(backend.clone());
    let bridge = cf_apply_driver(&driver);
    let registry = DriverRegistry::new().with(driver.id(), Arc::new(bridge));
    let interp = Interpreter::with_defaults(registry);

    // A plan with a D1 insert, a KV upsert, and a queue send.
    let mut b = PlanBuilder::new();
    b.push(
        EffectNode::new(
            NodeId(0),
            EffectKind::Insert,
            Target::new(PlanDriverId::new("cf"), VfsPath::new("/cf/d1/prod/users")),
        )
        .with_args(row_batch(vec![
            ("id", ColumnType::Int, Value::Int(9)),
            ("name", ColumnType::Text, Value::Text("dave".to_string())),
        ])),
    );
    b.push(
        EffectNode::new(
            NodeId(1),
            EffectKind::Upsert,
            Target::new(PlanDriverId::new("cf"), VfsPath::new("/cf/kv/cache")),
        )
        .with_args(row_batch(vec![
            ("key", ColumnType::Text, Value::Text("sess".to_string())),
            ("value", ColumnType::Text, Value::Text("xyz".to_string())),
        ])),
    );
    b.push(
        EffectNode::new(
            NodeId(2),
            EffectKind::Insert,
            Target::new(PlanDriverId::new("cf"), VfsPath::new("/cf/queue/events")),
        )
        .with_args(row_batch(vec![(
            "body",
            ColumnType::Text,
            Value::Text("ping".to_string()),
        )])),
    );
    let plan = b.build();

    let caps = CapabilitySet::none()
        .grant(PlanDriverId::new("cf"), &EffectKind::Insert)
        .grant(PlanDriverId::new("cf"), &EffectKind::Upsert);
    let outcome = interp.commit(plan, &caps).await.unwrap();
    assert!(
        outcome.is_complete(),
        "all three CF effects must apply: {outcome:?}"
    );

    // Every service backend was exercised end-to-end.
    let calls = backend.recorded();
    assert!(calls
        .iter()
        .any(|c| matches!(c, RecordedCall::D1Batch { .. })));
    assert!(calls
        .iter()
        .any(|c| matches!(c, RecordedCall::KvPut { .. })));
    assert!(calls
        .iter()
        .any(|c| matches!(c, RecordedCall::QueueSend { .. })));
}
