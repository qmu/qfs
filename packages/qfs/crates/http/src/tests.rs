//! Internal tests (t32): the request → bind → eval → encode pipeline driven IN-PROCESS (the
//! `oneshot` analogue — NO TCP, NO live network, NO credentials). All read I/O is an in-memory
//! fake [`qfs_exec::ReadDriver`]. Each test builds a [`HttpBinding`], reconciles it from a
//! synthetic [`qfs_server::ServerState`], and dispatches an owned [`HttpRequest`].

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;
use std::sync::Arc;

use qfs_core::{
    Archetype, Capabilities, CfsError, Column, ColumnType, DriverId, Engine, NodeDesc, Path,
    PushdownProfile, RequestContext, Row, RowBatch, Schema, StatementSpec, Value,
};
use qfs_exec::{parse, ReadDriver, ReadRegistry};
use qfs_pushdown::ScanNode;
use qfs_server::{Binding, EndpointDef, ServerState};

use crate::handler::dispatch;
use crate::route::{compile_endpoint, RoutePattern};
use crate::{HttpBinding, HttpRequest, Method};

// ---- an in-memory fake `/mock` source: introspective (describe/pushdown) + read (scan) ----

struct FakeItems {
    rows: Vec<Row>,
}

fn items_schema() -> Schema {
    Schema::new(vec![
        Column::new("id", ColumnType::Int, false),
        Column::new("name", ColumnType::Text, true),
    ])
}

impl FakeItems {
    fn new() -> Self {
        Self {
            rows: vec![
                Row::new(vec![Value::Int(1), Value::Text("alpha".into())]),
                Row::new(vec![Value::Int(2), Value::Text("beta".into())]),
                Row::new(vec![Value::Int(3), Value::Text("gamma".into())]),
            ],
        }
    }
}

impl qfs_core::Driver for FakeItems {
    fn mount(&self) -> &str {
        "/mock"
    }
    fn describe(&self, _path: &Path) -> Result<NodeDesc, CfsError> {
        Ok(NodeDesc::new(Archetype::RelationalTable, items_schema()))
    }
    fn capabilities(&self, _path: &Path) -> Capabilities {
        // The SOURCE supports reads AND writes (a realistic backend). The read-only invariant
        // is enforced by the HTTP layer's policy gate, NOT by withholding source capability —
        // so a write-lowering endpoint genuinely produces a write Plan that the gate refuses.
        Capabilities::none().select().insert().update().remove()
    }
    fn procedures(&self) -> &[qfs_core::ProcSig] {
        &[]
    }
    fn pushdown(&self) -> &PushdownProfile {
        // None: WHERE/LIMIT are local residuals; the scan over-returns and the engine re-filters.
        &PushdownProfile::None
    }
    fn applier(&self) -> &dyn qfs_core::PlanApplier {
        Box::leak(Box::new(NoopApplier))
    }
}

#[derive(Default)]
struct NoopApplier;
impl qfs_core::PlanApplier for NoopApplier {
    fn apply(
        &mut self,
        node: &qfs_core::EffectNode,
    ) -> Result<qfs_core::AppliedEffect, qfs_core::ApplyError> {
        Ok(qfs_core::AppliedEffect::new(node.id, 0))
    }
}

#[async_trait::async_trait]
impl ReadDriver for FakeItems {
    async fn scan(&self, _scan: &ScanNode, _ctx: &RequestContext) -> Result<RowBatch, CfsError> {
        // Honestly over-return ALL rows; the executor's residual trims to the bound result.
        Ok(RowBatch::new(items_schema(), self.rows.clone()))
    }
}

/// A counting fake that records how many times `scan` ran (asserts the live route was hit).
struct CountingItems {
    inner: FakeItems,
    scans: Arc<std::sync::atomic::AtomicUsize>,
}

impl qfs_core::Driver for CountingItems {
    fn mount(&self) -> &str {
        "/mock"
    }
    fn describe(&self, p: &Path) -> Result<NodeDesc, CfsError> {
        self.inner.describe(p)
    }
    fn capabilities(&self, p: &Path) -> Capabilities {
        self.inner.capabilities(p)
    }
    fn procedures(&self) -> &[qfs_core::ProcSig] {
        &[]
    }
    fn pushdown(&self) -> &PushdownProfile {
        self.inner.pushdown()
    }
    fn applier(&self) -> &dyn qfs_core::PlanApplier {
        Box::leak(Box::new(NoopApplier))
    }
}

#[async_trait::async_trait]
impl ReadDriver for CountingItems {
    async fn scan(&self, scan: &ScanNode, ctx: &RequestContext) -> Result<RowBatch, CfsError> {
        self.scans.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.inner.scan(scan, ctx).await
    }
}

// ---- fixtures: engine + reads + an EndpointDef from a query string ----

fn engine_with_mock() -> Arc<Engine> {
    let mut engine = Engine::new();
    engine.mounts.register(Arc::new(FakeItems::new())).unwrap();
    engine.codecs = qfs_core::CodecRegistry::with_builtins();
    Arc::new(engine)
}

fn reads_with_mock() -> Arc<ReadRegistry> {
    Arc::new(ReadRegistry::new().with(DriverId::new("mock"), Arc::new(FakeItems::new())))
}

/// The permissive read policy every fixture endpoint binds to. Reads are default-denied like any
/// other effect, so a fixture that wants to serve rows must be GRANTED `SELECT` — the tests that
/// prove the denial side deliberately bind no policy (or a narrowed one) instead.
const READ_POLICY: &str = "read-any";

/// The `PolicyDef` row for [`READ_POLICY`]: an unscoped `ALLOW SELECT` (every driver, every path,
/// every actor).
fn read_any_policy() -> qfs_server::PolicyDef {
    qfs_server::PolicyDef {
        name: READ_POLICY.to_string(),
        handler: String::new(),
        allow: vec!["ALLOW SELECT".to_string()],
    }
}

/// Build an `EndpointDef` from a method/route and a query SOURCE string, storing the query as
/// the canonical span-normalised `StatementSpec` exactly as t31's DDL desugar does. The endpoint
/// binds [`READ_POLICY`], which [`state_with`] registers.
fn endpoint(name: &str, method: &str, route: &str, query_src: &str) -> EndpointDef {
    let stmt = parse(query_src).expect("endpoint query parses");
    let spec = StatementSpec::from_statement(stmt);
    EndpointDef {
        name: name.to_string(),
        method: method.to_string(),
        route: route.to_string(),
        query: qfs_server::StatementSource::new(spec.canonical()),
        policy: Some(READ_POLICY.to_string()),
    }
}

/// Like [`endpoint`], but binding NO policy — the fail-closed shape (an ungranted read is refused).
fn endpoint_without_policy(name: &str, method: &str, route: &str, query_src: &str) -> EndpointDef {
    EndpointDef {
        policy: None,
        ..endpoint(name, method, route, query_src)
    }
}

fn state_with(endpoints: Vec<EndpointDef>) -> ServerState {
    let mut state = ServerState::new();
    state
        .policies
        .insert(READ_POLICY.to_string(), read_any_policy());
    for ep in endpoints {
        state.endpoints.insert(ep.name.clone(), ep);
    }
    state
}

/// Reconcile a binding from a state and dispatch one request in-process, returning the response.
fn serve_once(binding: &HttpBinding, req: &HttpRequest) -> crate::HttpResponse {
    let router = binding.current_router();
    let ctx = binding.ctx();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async {
        match router.match_request(&req.method, &req.path) {
            Some((route, path_params)) => dispatch(route, path_params, req, &ctx).await,
            None => crate::error::HttpError::NotFound.into_response(),
        }
    })
}

// ---------------------------------------------------------------------------
// Acceptance tests
// ---------------------------------------------------------------------------

#[test]
fn endpoint_registers_and_get_returns_200_json_of_matching_rows() {
    let mut binding = HttpBinding::new(engine_with_mock(), reads_with_mock(), 10_000);
    // The route param `:p_id` is a DISTINCT identifier from the `id` column it filters on, so
    // the bare-identifier param convention is unambiguous (the LHS `id` stays a column, the RHS
    // `p_id` is the param slot the path segment binds into).
    let state = state_with(vec![endpoint(
        "items",
        "GET",
        "/items/:p_id",
        "/mock/items |> WHERE id == p_id",
    )]);
    binding.reconcile(&state).unwrap();

    let resp = serve_once(&binding, &HttpRequest::new(Method::Get, "/items/2"));
    assert_eq!(resp.status, 200, "body: {}", resp.body_text());
    assert_eq!(resp.content_type, "application/json");
    let body = resp.body_text();
    // The WHERE id = 2 residual trims the over-returned 3 rows to the single matching row.
    assert!(body.contains("\"beta\""), "body was: {body}");
    assert!(
        !body.contains("\"alpha\""),
        "should not include id=1: {body}"
    );
    assert!(
        !body.contains("\"gamma\""),
        "should not include id=3: {body}"
    );
}

#[test]
fn write_endpoint_is_refused_at_registration_plan_assertion() {
    // An endpoint whose query lowers to a write effect (REMOVE) is refused at compile time.
    let engine = engine_with_mock();
    let def = endpoint(
        "purge",
        "POST",
        "/purge/:p_id",
        "REMOVE /mock/items WHERE id == p_id",
    );
    let result = compile_endpoint(&def, &engine, None);
    assert!(
        matches!(result, Err(crate::route::CompileError::Policy(_))),
        "a write-lowering endpoint must be refused by the read-only gate, got: {result:?}"
    );
}

#[test]
fn write_endpoint_is_allowed_when_a_policy_grants_it() {
    let engine = engine_with_mock();
    let def = endpoint(
        "purge",
        "POST",
        "/purge/:p_id",
        "REMOVE /mock/items WHERE id == p_id",
    );
    // t35: an explicit `ALLOW REMOVE` grants the irreversible REMOVE the endpoint lowers to
    // (a broad `ALLOW ALL` would NOT — irreversible strictness). The canonical rule string is
    // what `CREATE POLICY … ALLOW REMOVE` desugars into the `allow` array.
    let policy = qfs_server::PolicyDef {
        name: "writer".to_string(),
        handler: "purge".to_string(),
        allow: vec!["ALLOW REMOVE".to_string()],
    };
    let result = compile_endpoint(&def, &engine, Some(&policy));
    assert!(
        result.is_ok(),
        "a granting policy must open the write gate, got: {result:?}"
    );

    // Counter-case: a broad `ALLOW ALL` must NOT grant the irreversible REMOVE (it needs an
    // explicit ALLOW REMOVE) — the t35 irreversible-strictness rule.
    let broad = qfs_server::PolicyDef {
        name: "broad".to_string(),
        handler: "purge".to_string(),
        allow: vec!["ALLOW ALL".to_string()],
    };
    assert!(
        compile_endpoint(&def, &engine, Some(&broad)).is_err(),
        "ALLOW ALL must not silently grant irreversible REMOVE"
    );
}

/// The policy gate evaluates the RESOLVED ACTOR, not always `anonymous()` (mission acceptance 3).
/// A `FOR user:alice` rule bites when Alice is the resolved principal and contributes nothing when
/// the request is anonymous — proved BOTH directions — and a write with no policy is denied
/// (fail-closed, acceptance 4). Uses `assert_read_only` directly (the request-time gate).
#[test]
fn policy_gate_evaluates_the_resolved_actor_both_directions() {
    use crate::policy::assert_read_only;
    use qfs_server::DecisionContext;

    let engine = engine_with_mock();
    // A write plan (REMOVE is a write effect the gate must adjudicate).
    let stmt = parse("REMOVE /mock/items WHERE id == 1").expect("parses");
    let plan = qfs_exec::build_plan(&stmt, &engine).expect("plans");

    // A t57-narrowed grant: REMOVE is allowed only FOR user:alice (explicit ALLOW REMOVE — the
    // irreversible-strictness rule — narrowed to a subject).
    let policy = qfs_server::PolicyDef {
        name: "p".to_string(),
        handler: String::new(),
        allow: vec!["ALLOW REMOVE FOR user:alice".to_string()],
    };

    // Direction 1: the rule BITES for the resolved principal ⇒ the write is permitted.
    assert!(
        assert_read_only(&plan, Some(&policy), &DecisionContext::for_user("alice")).is_ok(),
        "a FOR user:alice rule must grant the write when Alice is the resolved actor"
    );

    // Direction 2: the SAME rule contributes NOTHING under the anonymous actor ⇒ default-deny.
    assert!(
        assert_read_only(&plan, Some(&policy), &DecisionContext::anonymous()).is_err(),
        "the narrowed rule must contribute nothing without a principal (fail closed)"
    );

    // The wrong principal is likewise denied — a rule for Alice does not grant Bob.
    assert!(
        assert_read_only(&plan, Some(&policy), &DecisionContext::for_user("bob")).is_err(),
        "a FOR user:alice rule must not grant a different actor"
    );

    // Fail-closed (acceptance 4): a write with NO attached policy is denied regardless of actor —
    // threading a principal widens nothing. If the default ever widened, this assertion fails.
    assert!(
        assert_read_only(&plan, None, &DecisionContext::for_user("alice")).is_err(),
        "no policy ⇒ default-deny holds even for a resolved user (the default must never widen)"
    );
    assert!(assert_read_only(&plan, None, &DecisionContext::anonymous()).is_err());

    // decision_for maps the request principal onto the actor axis (the single resolution point).
    assert_eq!(
        crate::policy::decision_for(&qfs_core::RequestContext::for_user("alice")),
        DecisionContext::for_user("alice")
    );
    assert_eq!(
        crate::policy::decision_for(&qfs_core::RequestContext::anonymous()),
        DecisionContext::anonymous()
    );
}

/// `identity::Role` is NOT an authorization grant, and this mission did not convert it into one
/// (mission acceptance 6, the invariant). The request-principal seam carries the USER axis only —
/// never a membership `Role` label — so a `Role::Admin` member is not privileged by that label:
/// a role-scoped rule cannot bite on the strength of a membership (identity ≠ authorization, §4.1).
/// If a later change ever wires `identity::Role` into the principal seam, `roles` becomes non-empty
/// and the first assertion fails; the role rule would then bite and the second fails — either way
/// this test catches the accidental conversion.
#[test]
fn identity_role_is_not_an_authorization_grant() {
    use crate::policy::{assert_read_only, decision_for};

    // The seam resolves only a user id — it carries NO role set (no membership label flows in).
    let actor = decision_for(&qfs_core::RequestContext::for_user("alice"));
    assert!(
        actor.roles.is_empty(),
        "the principal seam must carry no role grant (identity::Role is not authorization)"
    );

    let engine = engine_with_mock();
    let stmt = parse("REMOVE /mock/items WHERE id == 1").expect("parses");
    let plan = qfs_exec::build_plan(&stmt, &engine).expect("plans");

    // A rule granting REMOVE to `role:admin` does NOT bite for a user resolved through the seam —
    // even one who is an `identity::Role::Admin` member — because the label is not a grant.
    let role_policy = qfs_server::PolicyDef {
        name: "r".to_string(),
        handler: String::new(),
        allow: vec!["ALLOW REMOVE FOR role:admin".to_string()],
    };
    assert!(
        assert_read_only(&plan, Some(&role_policy), &actor).is_err(),
        "identity::Role must not be an authorization grant (Role::Admin is still not privileged)"
    );
}

#[test]
fn content_negotiation_json_default_and_csv_on_request() {
    let mut binding = HttpBinding::new(engine_with_mock(), reads_with_mock(), 10_000);
    let state = state_with(vec![endpoint("all", "GET", "/all", "/mock/items")]);
    binding.reconcile(&state).unwrap();

    // Default → JSON.
    let json = serve_once(&binding, &HttpRequest::new(Method::Get, "/all"));
    assert_eq!(json.status, 200);
    assert_eq!(json.content_type, "application/json");

    // ?format=csv → CSV.
    let csv_q = serve_once(
        &binding,
        &HttpRequest::new(Method::Get, "/all").with_query("format", "csv"),
    );
    assert_eq!(csv_q.status, 200);
    assert_eq!(csv_q.content_type, "text/csv");

    // Accept: text/csv → CSV.
    let csv_h = serve_once(
        &binding,
        &HttpRequest::new(Method::Get, "/all").with_header("Accept", "text/csv"),
    );
    assert_eq!(csv_h.status, 200);
    assert_eq!(csv_h.content_type, "text/csv");
    assert!(
        csv_h.body_text().contains("alpha"),
        "csv body: {}",
        csv_h.body_text()
    );
}

#[test]
fn missing_param_bind_error_is_400_naming_the_param() {
    // Assert the bind layer for a declared param with no source → 400 naming the param.
    let err = crate::QueryArgs::bind(
        &["id".to_string()],
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
    )
    .unwrap_err();
    let http: crate::HttpError = err.into();
    let resp = http.into_response();
    assert_eq!(resp.status, 400);
    let body = resp.body_text();
    assert!(
        body.contains("\"param\":\"id\""),
        "400 body names param: {body}"
    );
    assert!(body.contains("\"error\":\"bind\""), "body: {body}");
}

#[test]
fn extra_query_param_is_400_naming_the_param() {
    let mut query = BTreeMap::new();
    query.insert("surprise".to_string(), "1".to_string());
    let err = crate::QueryArgs::bind(&[], &BTreeMap::new(), &query, &BTreeMap::new()).unwrap_err();
    let resp: crate::HttpResponse = crate::HttpError::from(err).into_response();
    assert_eq!(resp.status, 400);
    assert!(
        resp.body_text().contains("\"param\":\"surprise\""),
        "{}",
        resp.body_text()
    );
}

#[test]
fn unknown_route_is_404() {
    let mut binding = HttpBinding::new(engine_with_mock(), reads_with_mock(), 10_000);
    binding
        .reconcile(&state_with(vec![endpoint("a", "GET", "/a", "/mock/items")]))
        .unwrap();
    let resp = serve_once(&binding, &HttpRequest::new(Method::Get, "/does-not-exist"));
    assert_eq!(resp.status, 404);
    assert!(
        resp.body_text().contains("\"error\":\"not_found\""),
        "{}",
        resp.body_text()
    );
}

#[test]
fn eval_error_is_422() {
    // An endpoint over a source with NO registered read driver: registration succeeds (it
    // lowers to a pure read), but evaluation fails at scan time → 422.
    let engine = engine_with_mock();
    let reads = Arc::new(ReadRegistry::new()); // no `mock` read driver registered
    let mut binding = HttpBinding::new(engine, reads, 10_000);
    binding
        .reconcile(&state_with(vec![endpoint("a", "GET", "/a", "/mock/items")]))
        .unwrap();
    let resp = serve_once(&binding, &HttpRequest::new(Method::Get, "/a"));
    assert_eq!(resp.status, 422, "body: {}", resp.body_text());
    assert!(
        resp.body_text().contains("\"error\":\"eval\""),
        "{}",
        resp.body_text()
    );
}

#[test]
fn oversize_result_is_413() {
    // A 1-row cap with a 3-row result trips the bounded guard.
    let mut binding = HttpBinding::new(engine_with_mock(), reads_with_mock(), 1);
    binding
        .reconcile(&state_with(vec![endpoint(
            "all",
            "GET",
            "/all",
            "/mock/items",
        )]))
        .unwrap();
    let resp = serve_once(&binding, &HttpRequest::new(Method::Get, "/all"));
    assert_eq!(resp.status, 413, "body: {}", resp.body_text());
    assert!(
        resp.body_text().contains("\"error\":\"oversize\""),
        "{}",
        resp.body_text()
    );
}

#[test]
fn injection_path_param_binds_as_typed_value_with_identical_plan() {
    // A path param containing DSL-like text binds as a typed string literal; the bound query
    // AST is structurally IDENTICAL to a benign string bind (only the literal CONTENT differs).
    let route_query = "/mock/items |> WHERE name == q_name";
    let stmt = parse(route_query).unwrap();

    let benign = crate::QueryArgs::new().with("q_name", Value::Text("alpha".to_string()));
    let malicious =
        crate::QueryArgs::new().with("q_name", Value::Text("'; REMOVE /mock/items".to_string()));

    let mut s_benign = stmt.clone();
    let mut s_malicious = stmt.clone();
    crate::rewrite::bind_params(&mut s_benign, &benign);
    crate::rewrite::bind_params(&mut s_malicious, &malicious);

    // Both bind to a single `Expr::Lit(Literal::Str(_))` node — same SHAPE. Strip the literal
    // string content (replace with a fixed marker) and the two ASTs are byte-identical: proof
    // that the malicious text is data, not parsed structure.
    let canon = |s: &qfs_parser::Statement| -> String {
        let json = serde_json::to_string(s).unwrap();
        // Replace the two distinct literal payloads with a marker so only the SHAPE remains.
        json.replace("'; REMOVE /mock/items", "MARKER")
            .replace("alpha", "MARKER")
    };
    assert_eq!(
        canon(&s_benign),
        canon(&s_malicious),
        "the malicious bind must produce a structurally identical plan (typed literal), not an \
         injected query"
    );

    // And the malicious bind genuinely lowers without introducing any write effect.
    let engine = engine_with_mock();
    let plan = qfs_exec::build_plan(&s_malicious, &engine).unwrap();
    assert!(
        plan.nodes()
            .iter()
            .all(|n| !crate::policy::is_write_effect(&n.kind)),
        "the malicious bind must not introduce any write effect"
    );
}

#[test]
fn hot_reload_adds_then_removes_a_route_via_reconcile_swap() {
    // A counting read driver proves the live route is served, and that a reconcile swap
    // re-binds the table: add /live (200), then remove it (404) — without restart.
    let scans = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let mut engine = Engine::new();
    engine.mounts.register(Arc::new(FakeItems::new())).unwrap();
    engine.codecs = qfs_core::CodecRegistry::with_builtins();
    let reads = Arc::new(ReadRegistry::new().with(
        DriverId::new("mock"),
        Arc::new(CountingItems {
            inner: FakeItems::new(),
            scans: Arc::clone(&scans),
        }),
    ));
    let mut binding = HttpBinding::new(Arc::new(engine), reads, 10_000);

    // Initially empty → 404.
    binding.reconcile(&ServerState::new()).unwrap();
    assert_eq!(
        serve_once(&binding, &HttpRequest::new(Method::Get, "/live")).status,
        404
    );

    // Add the endpoint (a committed /server/endpoints mutation) → reconcile → 200.
    binding
        .reconcile(&state_with(vec![endpoint(
            "live",
            "GET",
            "/live",
            "/mock/items",
        )]))
        .unwrap();
    assert_eq!(
        serve_once(&binding, &HttpRequest::new(Method::Get, "/live")).status,
        200
    );
    assert!(
        scans.load(std::sync::atomic::Ordering::SeqCst) >= 1,
        "the live route must have hit the read driver"
    );

    // Remove it (a committed REMOVE) → reconcile → 404 again.
    binding.reconcile(&ServerState::new()).unwrap();
    assert_eq!(
        serve_once(&binding, &HttpRequest::new(Method::Get, "/live")).status,
        404
    );
}

#[test]
fn route_pattern_extracts_named_params_and_rejects_mismatches() {
    let p = RoutePattern::parse("/items/:id/sub/{kind}");
    assert_eq!(p.param_names(), vec!["id".to_string(), "kind".to_string()]);
    let m = p.match_path("/items/42/sub/widget").unwrap();
    assert_eq!(m.get("id"), Some(&"42".to_string()));
    assert_eq!(m.get("kind"), Some(&"widget".to_string()));
    assert!(
        p.match_path("/items/42/sub").is_none(),
        "segment count mismatch"
    );
    assert!(
        p.match_path("/other/42/sub/widget").is_none(),
        "literal mismatch"
    );
}

#[test]
fn param_shadowing_a_referenced_column_is_refused_at_registration() {
    // The t32 security fix: a route param `:id` whose name collides with the `id` COLUMN the
    // query reads would make the typed-AST rewrite replace the wrong `Expr::Col` node and
    // collapse `WHERE id == id` into the tautology `WHERE 2 = 2` (access widening). This is
    // refused at registration with a structured error naming the param — no route is created.
    let engine = engine_with_mock();
    let def = endpoint(
        "shadow",
        "GET",
        "/items/:id",
        "/mock/items |> WHERE id == id",
    );
    let result = compile_endpoint(&def, &engine, None);
    match result {
        Err(crate::route::CompileError::ParamShadowsColumn { param }) => {
            assert_eq!(param, "id", "the error must name the shadowing param");
        }
        other => panic!("a param shadowing a referenced column must be refused, got: {other:?}"),
    }

    // And via the binding's reconcile, the shadowing endpoint is SKIPPED (no live route): a GET
    // against it is 404, never a (silently widened) 200.
    let mut binding = HttpBinding::new(engine_with_mock(), reads_with_mock(), 10_000);
    binding
        .reconcile(&state_with(vec![endpoint(
            "shadow",
            "GET",
            "/items/:id",
            "/mock/items |> WHERE id == id",
        )]))
        .unwrap();
    assert_eq!(
        binding.current_router().len(),
        0,
        "no route may be registered"
    );
    assert_eq!(
        serve_once(&binding, &HttpRequest::new(Method::Get, "/items/2")).status,
        404
    );
}

#[test]
fn non_colliding_param_registers_and_serves() {
    // The benign counterpart: a route param DISTINCT from every queried column registers and
    // serves normally — the shadow gate does not false-refuse a correctly-authored endpoint.
    let engine = engine_with_mock();
    let def = endpoint(
        "ok",
        "GET",
        "/items/:p_id",
        "/mock/items |> WHERE id == p_id",
    );
    assert!(
        compile_endpoint(&def, &engine, None).is_ok(),
        "a param distinct from every queried column must register"
    );

    let mut binding = HttpBinding::new(engine_with_mock(), reads_with_mock(), 10_000);
    binding
        .reconcile(&state_with(vec![endpoint(
            "ok",
            "GET",
            "/items/:p_id",
            "/mock/items |> WHERE id == p_id",
        )]))
        .unwrap();
    assert_eq!(binding.current_router().len(), 1, "the route must register");
    let resp = serve_once(&binding, &HttpRequest::new(Method::Get, "/items/2"));
    assert_eq!(resp.status, 200, "body: {}", resp.body_text());
    assert!(
        resp.body_text().contains("\"beta\""),
        "body: {}",
        resp.body_text()
    );
}

// ---- t37: HTTP eval-error hygiene hardening (the t32 carry-over) ----

#[test]
fn eval_error_non_allowlisted_class_drops_raw_message() {
    use qfs_exec::{ErrorKind, ExecError};

    // A driver that carelessly stuffed an upstream secret into an Auth-class error message. With
    // the t37 hardening this NEVER reaches the caller-facing body: a non-allowlisted class is
    // reduced to `code` + a generic detail, so hygiene is UNCONDITIONAL (not driver-dependent).
    let leaky = ExecError::new(
        ErrorKind::Auth,
        "token_rejected",
        "upstream said: Bearer sk-LEAK-7f9c-PLANTED is invalid",
    );
    let problem = crate::HttpError::Eval(leaky).problem();
    assert_eq!(problem.error, "eval");
    assert!(
        !problem.detail.contains("sk-LEAK-7f9c-PLANTED"),
        "non-allowlisted eval error must not echo the raw message: {}",
        problem.detail
    );
    assert!(
        !problem.detail.contains("Bearer"),
        "the bearer token shape must be gone: {}",
        problem.detail
    );
    // The stable structured code is still surfaced for the caller/agent to branch on.
    assert!(
        problem.detail.starts_with("token_rejected:"),
        "the structured code is retained: {}",
        problem.detail
    );

    // An Internal class is likewise reduced to code + a generic detail.
    let internal = ExecError::new(
        ErrorKind::Internal,
        "bug",
        "panic at /home/secret/path token=X",
    );
    let p2 = crate::HttpError::Eval(internal).problem();
    assert!(!p2.detail.contains("token=X"), "{}", p2.detail);
    assert_eq!(p2.detail, "bug: an internal error occurred");
}

#[test]
fn eval_error_safe_class_keeps_structured_message() {
    use qfs_exec::{ErrorKind, ExecError};

    // The allowlisted/safe classes keep the executor's well-typed, secret-free message — these
    // are the diagnostics an agent branches on (e.g. which verb is unsupported). They never carry
    // a raw upstream string, so retaining them is safe.
    let cap = ExecError::new(
        ErrorKind::Capability,
        "unsupported_verb",
        "driver `mock` does not support REMOVE",
    );
    let problem = crate::HttpError::Eval(cap).problem();
    assert_eq!(problem.error, "eval");
    assert_eq!(problem.detail, "driver `mock` does not support REMOVE");
}

// ---------------------------------------------------------------------------
// The serve READ path is policy-gated (the mission's enforcement half)
// ---------------------------------------------------------------------------

/// A binding serving ONE read endpoint, with the `/mock` scans COUNTED so a test can prove a
/// refusal happened BEFORE the driver ran, and an optional resolved principal.
///
/// `allow` is the endpoint's bound policy rules — `None` binds no policy at all (the fail-closed
/// shape). `user` injects a [`crate::PrincipalResolver`] resolving every request to that user;
/// `None` leaves every request anonymous.
struct ReadFixture {
    binding: HttpBinding,
    scans: Arc<std::sync::atomic::AtomicUsize>,
}

impl ReadFixture {
    /// Dispatch one GET against `path` and return the response.
    fn get(&self, path: &str) -> crate::HttpResponse {
        serve_once(&self.binding, &HttpRequest::new(Method::Get, path))
    }

    /// How many times the read driver was asked to scan.
    fn scan_count(&self) -> usize {
        self.scans.load(std::sync::atomic::Ordering::SeqCst)
    }
}

fn read_fixture(
    route: &str,
    query: &str,
    allow: Option<&[&str]>,
    user: Option<&str>,
) -> ReadFixture {
    let scans = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let mut engine = Engine::new();
    engine.mounts.register(Arc::new(FakeItems::new())).unwrap();
    engine.codecs = qfs_core::CodecRegistry::with_builtins();
    let reads = Arc::new(ReadRegistry::new().with(
        DriverId::new("mock"),
        Arc::new(CountingItems {
            inner: FakeItems::new(),
            scans: Arc::clone(&scans),
        }),
    ));
    let mut binding = HttpBinding::new(Arc::new(engine), reads, 10_000);
    if let Some(u) = user {
        let u = u.to_string();
        binding = binding.with_principal_resolver(Arc::new(move |_req: &HttpRequest| {
            RequestContext::for_user(u.clone())
        }));
    }

    let mut def = endpoint_without_policy("gated", "GET", route, query);
    let mut state = ServerState::new();
    if let Some(rules) = allow {
        def.policy = Some("gate".to_string());
        state.policies.insert(
            "gate".to_string(),
            qfs_server::PolicyDef {
                name: "gate".to_string(),
                handler: String::new(),
                allow: rules.iter().map(|r| (*r).to_string()).collect(),
            },
        );
    }
    state.endpoints.insert(def.name.clone(), def);
    binding.reconcile(&state).unwrap();
    ReadFixture { binding, scans }
}

/// The headline behaviour (ticket 20260724013000): a read is gated like a write. An endpoint whose
/// policy grants `SELECT` on the scanned path serves; the same request under a `DENY SELECT`, and
/// under a policy with no matching rule, is REFUSED — and refused BEFORE the driver scan runs, so
/// the denial is a genuine gate and not a post-hoc filter of rows already fetched.
#[test]
fn serve_read_is_gated_and_denial_precedes_the_driver_scan() {
    // Direction 1 — granted: `ALLOW SELECT ON mock` admits the read.
    let granted = read_fixture(
        "/items",
        "/mock/items",
        Some(&["ALLOW SELECT ON mock"]),
        None,
    );
    let ok = granted.get("/items");
    assert_eq!(ok.status, 200, "body: {}", ok.body_text());
    assert!(ok.body_text().contains("alpha"), "body: {}", ok.body_text());
    assert!(
        granted.scan_count() >= 1,
        "the granted read reached the driver"
    );

    // Direction 2 — an explicit earlier DENY wins over a later ALLOW (first-match), and the read
    // never reaches the driver.
    let denied = read_fixture(
        "/items",
        "/mock/items",
        Some(&["DENY SELECT", "ALLOW SELECT"]),
        None,
    );
    let refused = denied.get("/items");
    assert_eq!(refused.status, 403, "body: {}", refused.body_text());
    assert!(
        refused.body_text().contains("\"error\":\"policy\""),
        "a denied read is a structured policy refusal: {}",
        refused.body_text()
    );
    assert_eq!(
        denied.scan_count(),
        0,
        "the refusal must precede the driver scan — nothing was read"
    );

    // Direction 3 — no matching rule under the fail-closed default: a grant for ANOTHER driver
    // leaves this read default-denied.
    let unmatched = read_fixture(
        "/items",
        "/mock/items",
        Some(&["ALLOW SELECT ON other"]),
        None,
    );
    let out = unmatched.get("/items");
    assert_eq!(out.status, 403, "body: {}", out.body_text());
    assert_eq!(unmatched.scan_count(), 0);
}

/// The gate consumes the principal the predecessor mission threaded: a rule narrowed with `FOR
/// user:alice` admits Alice's read and contributes nothing to an anonymous one — the same rule,
/// both directions, decided by who is asking.
#[test]
fn read_gate_evaluates_the_resolved_actor_both_directions() {
    let rules: &[&str] = &["ALLOW SELECT ON mock FOR user:alice"];

    let alice = read_fixture("/items", "/mock/items", Some(rules), Some("alice"));
    assert_eq!(
        alice.get("/items").status,
        200,
        "the narrowed rule must bite for the resolved principal"
    );

    let anon = read_fixture("/items", "/mock/items", Some(rules), None);
    assert_eq!(
        anon.get("/items").status,
        403,
        "the same rule contributes nothing to an anonymous read (fail closed)"
    );
    assert_eq!(anon.scan_count(), 0);

    let bob = read_fixture("/items", "/mock/items", Some(rules), Some("bob"));
    assert_eq!(
        bob.get("/items").status,
        403,
        "a rule for Alice does not grant Bob"
    );
}

/// The request-time gate resolves the endpoint's bound policy REF, not a policy that happens to
/// share the endpoint's name. A policy named differently from its endpoint must still apply — and a
/// policy whose name merely matches some other endpoint must not be picked up.
#[test]
fn request_time_gate_resolves_the_endpoints_policy_ref_not_its_name() {
    // The endpoint is named `gated`; its policy is named `gate`. The read is granted only if the
    // gate follows the REF.
    let by_ref = read_fixture("/items", "/mock/items", Some(&["ALLOW SELECT"]), None);
    assert_eq!(
        by_ref.get("/items").status,
        200,
        "the endpoint's `policy:` ref must be the key the request-time gate resolves"
    );
    // The compiled route carries the ref so the request path can resolve it at all.
    let route = by_ref.binding.current_router();
    assert_eq!(route.len(), 1);
}

/// The CLI/local read path is untouched by this change: `execute_read` still takes no policy and
/// still returns rows. The gate lives at the serve seam only (the mission's explicit scope ruling).
#[test]
fn local_read_execution_is_not_policy_gated() {
    let engine = engine_with_mock();
    let reads = reads_with_mock();
    let stmt = parse("/mock/items").expect("parses");
    let rows = qfs_exec::block_on_read(&stmt, &engine.mounts, &reads, &RequestContext::anonymous())
        .expect("the local read path runs with no policy in sight");
    assert_eq!(
        rows.rows.len(),
        3,
        "the operator's own terminal is unchanged"
    );
}

// ---------------------------------------------------------------------------
// Fail-closed + structured denial on the serve read path
// ---------------------------------------------------------------------------

/// Build a binding serving ONE read endpoint whose `policy:` field is `policy_ref` verbatim, with
/// only `registered` present in the live policy table. Lets a test drive the three resolution
/// shapes: a present policy, a DANGLING ref, and no ref at all.
fn binding_with_policy_ref(
    policy_ref: Option<&str>,
    registered: Option<(&str, &[&str])>,
) -> HttpBinding {
    let mut binding = HttpBinding::new(engine_with_mock(), reads_with_mock(), 10_000);
    let mut def = endpoint_without_policy("gated", "GET", "/items", "/mock/items");
    def.policy = policy_ref.map(str::to_string);
    let mut state = ServerState::new();
    if let Some((name, rules)) = registered {
        state.policies.insert(
            name.to_string(),
            qfs_server::PolicyDef {
                name: name.to_string(),
                handler: String::new(),
                allow: rules.iter().map(|r| (*r).to_string()).collect(),
            },
        );
    }
    state.endpoints.insert(def.name.clone(), def);
    binding.reconcile(&state).unwrap();
    binding
}

/// Fail-closed on reads, all three resolution shapes: NO attached policy denies, a DANGLING policy
/// ref denies (a typo must never accidentally open a path), and only a present, granting policy
/// serves. Threading a real principal widens none of them.
#[test]
fn no_policy_and_dangling_policy_ref_both_deny_the_read() {
    let none = binding_with_policy_ref(None, None);
    let resp = serve_once(&none, &HttpRequest::new(Method::Get, "/items"));
    assert_eq!(
        resp.status,
        403,
        "an endpoint with no policy serves nothing: {}",
        resp.body_text()
    );

    let dangling = binding_with_policy_ref(Some("ghost"), Some(("real", &["ALLOW SELECT"])));
    let resp = serve_once(&dangling, &HttpRequest::new(Method::Get, "/items"));
    assert_eq!(
        resp.status,
        403,
        "a dangling policy ref is default-deny, never a fallback to another policy: {}",
        resp.body_text()
    );

    let present = binding_with_policy_ref(Some("real"), Some(("real", &["ALLOW SELECT"])));
    assert_eq!(
        serve_once(&present, &HttpRequest::new(Method::Get, "/items")).status,
        200,
        "only a present, granting policy opens the read"
    );
}

/// A policy-denied read is a STRUCTURED REFUSAL, never an empty relation at 200. The response is a
/// 403 problem body naming the policy class; it carries no `rows`/`schema` payload, and its detail
/// is secret-free (no token/secret/password markers, no rule internals beyond the verb, driver and
/// the failing axis). The contrast with a granted read's 200 body is asserted in the same test, so
/// "distinguishable from an empty result" is proven rather than assumed.
#[test]
fn a_denied_read_is_a_structured_refusal_not_an_empty_relation() {
    let denied =
        binding_with_policy_ref(Some("narrow"), Some(("narrow", &["ALLOW SELECT ON other"])));
    let resp = serve_once(&denied, &HttpRequest::new(Method::Get, "/items"));
    let body = resp.body_text();

    assert_eq!(
        resp.status, 403,
        "a denied read is refused, not emptied: {body}"
    );
    assert_eq!(resp.content_type, "application/json");
    assert!(
        body.contains("\"error\":\"policy\""),
        "the refusal names its class so an agent can branch on it: {body}"
    );
    assert!(
        !body.contains("\"rows\""),
        "a refusal must carry no result payload: {body}"
    );
    assert!(
        !body.contains("\"schema\""),
        "a refusal must carry no result schema: {body}"
    );
    assert!(
        body.contains("SELECT"),
        "the refusal names the verb it refused: {body}"
    );
    for marker in ["token", "secret", "password", "credential"] {
        assert!(
            !body.to_lowercase().contains(marker),
            "the refusal must be secret-free (found `{marker}`): {body}"
        );
    }

    // The contrast: a granted read returns 200 WITH a rows payload. An empty relation and a
    // refusal are therefore never confusable — different status, different body shape.
    let granted = binding_with_policy_ref(Some("open"), Some(("open", &["ALLOW SELECT"])));
    let ok = serve_once(&granted, &HttpRequest::new(Method::Get, "/items"));
    assert_eq!(ok.status, 200);
    assert!(
        ok.body_text().contains("\"rows\""),
        "a granted read's envelope carries rows: {}",
        ok.body_text()
    );
}

/// The signed-out state stays a first-class ANSWER where it is granted to everyone: an unscoped
/// `ALLOW SELECT` (subject `anyone`) serves the anonymous request 200 with rows. Refusing every
/// unauthenticated request outright is explicitly NOT what this gate does.
#[test]
fn an_anyone_granted_read_still_answers_an_anonymous_request() {
    let public = binding_with_policy_ref(Some("public"), Some(("public", &["ALLOW SELECT"])));
    let resp = serve_once(&public, &HttpRequest::new(Method::Get, "/items"));
    assert_eq!(resp.status, 200, "not a 401: {}", resp.body_text());
    assert!(resp.body_text().contains("alpha"), "{}", resp.body_text());
}
