//! Hermetic tests for the pure provisioning core (blueprint §16, Decision X). No network, no
//! credentials, no `$HOME` — every fixture is an in-memory [`ConfigState`]. (The dispatching
//! applier is exercised binary-side, where the concrete `SysApplier` is composed.)

use qfs_core::{preview, ServerNode, ServerWriteOp, StatementSpec};
use qfs_parser::parse_statement;
use qfs_server::{
    AgentDef, EndpointDef, JobDef, PolicyDef, ServerState, StatementSource, TriggerDef, ViewDef,
    WebhookDef,
};

use crate::{
    build_plan, diff, load, ConfigState, PathBindingRow, ReconcileNode, SysCollection,
    SysDriverRow, SysPolicyRow, SysState, TransformRow,
};

/// The canonical span-normalised spec string a real `/server` row stores for a body — exactly
/// what `binding_config_row` writes. Contains `"` (JSON keys), so on reload the qfs lexer rejects
/// it and it is kept verbatim (the round-trip guarantee).
fn body(src: &str) -> StatementSource {
    let stmt = parse_statement(src).unwrap();
    StatementSource::new(StatementSpec::from_statement(stmt).canonical())
}

fn stamp() -> crate::GenerationStamp {
    crate::GenerationStamp {
        system_migrations: 7,
        project_migrations: Some(3),
        ddl_event_head: Some(crate::DdlEventHead {
            seq: 42,
            hash: "deadbeef".to_string(),
        }),
    }
}

/// Wrap a `/server`-only state into the two-store universe.
fn cfg(server: ServerState) -> ConfigState {
    ConfigState {
        server,
        sys: SysState::default(),
    }
}

/// A representative `/server` state exercising every collection, with runtime fields
/// (`last_run`, `cache_json`) set so their exclusion is meaningful.
fn sample_server() -> ServerState {
    let mut s = ServerState::new();
    s.endpoints.insert(
        "recent".to_string(),
        EndpointDef {
            name: "recent".to_string(),
            method: "GET".to_string(),
            route: "/recent".to_string(),
            query: body("/mail |> LIMIT 10"),
            policy: None,
        },
    );
    s.triggers.insert(
        "onmail".to_string(),
        TriggerDef {
            name: "onmail".to_string(),
            on: "inbox".to_string(),
            predicate: StatementSource::new(String::new()),
            plan: body("/mail |> LIMIT 10"),
            policy: None,
        },
    );
    s.jobs.insert(
        "nightly".to_string(),
        JobDef {
            name: "nightly".to_string(),
            every: "1h".to_string(),
            plan: body("/mail |> LIMIT 10"),
            policy: None,
            last_run: Some(1_700_000_000),
        },
    );
    s.views.insert(
        "digest".to_string(),
        ViewDef {
            name: "digest".to_string(),
            query: body("/mail |> LIMIT 10"),
            materialized: true,
            last_run: Some(1_700_000_500),
            cache_json: Some("{\"rows\":[]}".to_string()),
        },
    );
    s.policies.insert(
        "readers".to_string(),
        PolicyDef {
            name: "readers".to_string(),
            handler: String::new(),
            allow: vec!["ALLOW SELECT".to_string()],
        },
    );
    s.webhooks.insert(
        "ingest".to_string(),
        WebhookDef {
            name: "ingest".to_string(),
            route: "/hooks/ingest".to_string(),
            secret: String::new(),
        },
    );
    s
}

/// A representative `/sys` state exercising every universe collection: a declared driver
/// (JSON auth scheme), a sys policy, a setting, a full binding, and an alias binding.
fn sample_sys() -> SysState {
    let mut sys = SysState::default();
    sys.drivers.insert(
        "chatwork".to_string(),
        SysDriverRow {
            name: "chatwork".to_string(),
            kind: "driver".to_string(),
            base_url: Some("https://api.chatwork.com/v2".to_string()),
            auth: Some("{\"kind\":\"header\",\"name\":\"x-chatworktoken\"}".to_string()),
            pagination: None,
            of_type: None,
            verb: None,
            body: None,
            irreversible: false,
        },
    );
    sys.policies.insert(
        "analysts".to_string(),
        SysPolicyRow {
            name: "analysts".to_string(),
            allow: Some("SELECT".to_string()),
            target: Some("/sql/*".to_string()),
        },
    );
    sys.settings
        .insert("safety_mode".to_string(), "policy-only".to_string());
    sys.bindings.insert(
        "/chat".to_string(),
        PathBindingRow {
            path: "/chat".to_string(),
            driver: Some("chatwork".to_string()),
            at: Some("https://api.chatwork.com/v2".to_string()),
            secret_ref: Some("vault:chatwork/work".to_string()),
            alias_of: None,
            host: None,
            account: Some("work".to_string()),
            app: None,
        },
    );
    sys.bindings.insert(
        "/chat2".to_string(),
        PathBindingRow {
            path: "/chat2".to_string(),
            alias_of: Some("/chat".to_string()),
            ..PathBindingRow::default()
        },
    );
    // §15: a transform definition (the top-level `/transform` collection).
    sys.transforms.insert(
        "classify".to_string(),
        TransformRow {
            name: "classify".to_string(),
            input: "[{\"name\":\"body\",\"type\":\"text\",\"nullable\":true}]".to_string(),
            output: "[{\"name\":\"label\",\"type\":\"text\",\"nullable\":true}]".to_string(),
            provider: "claude".to_string(),
            model: "claude-sonnet-5".to_string(),
            effort: Some("medium".to_string()),
            secret_ref: Some("vault:models/key".to_string()),
        },
    );
    sys
}

fn sample_state() -> ConfigState {
    ConfigState {
        server: sample_server(),
        sys: sample_sys(),
    }
}

// ---------------------------------------------------------------------------
// Emitter + round trip
// ---------------------------------------------------------------------------

#[test]
fn emit_is_deterministic() {
    let s = sample_state();
    let a = crate::emit(&s, &stamp());
    let b = crate::emit(&s, &stamp());
    assert_eq!(a, b, "same (state, stamp) must emit byte-identically");
    // The generation stamp rides the comment header.
    assert!(a.contains("system_migrations=7 project_migrations=3"));
    assert!(a.contains("ddl_event_head=42:deadbeef"));
}

#[test]
fn generation_stamp_round_trips_through_the_document_header() {
    let s = sample_state();
    let doc = crate::emit(&s, &stamp());
    let parsed = crate::GenerationStamp::parse_from_document(&doc).unwrap();
    assert_eq!(parsed, stamp());
    // A document with no header carries no stamp (hand-written; nothing to compare).
    assert!(crate::GenerationStamp::parse_from_document(
        "UPSERT INTO /sys/settings VALUES (key, value) ('a', 'b');"
    )
    .is_none());
    // The `-`/empty forms parse back to None.
    let empty = crate::emit(&ConfigState::new(), &crate::GenerationStamp::default());
    let parsed = crate::GenerationStamp::parse_from_document(&empty).unwrap();
    assert_eq!(parsed, crate::GenerationStamp::default());
}

#[test]
fn emit_orders_rows_by_btreemap_name() {
    let mut s = cfg(ServerState::new());
    // Insert out of name order; the emitter must render them alphabetically (BTreeMap order).
    for name in ["zebra", "mango", "apple"] {
        s.server.webhooks.insert(
            name.to_string(),
            WebhookDef {
                name: name.to_string(),
                route: format!("/hooks/{name}"),
                secret: String::new(),
            },
        );
    }
    let doc = crate::emit(&s, &stamp());
    let apple = doc.find("'apple'").unwrap();
    let mango = doc.find("'mango'").unwrap();
    let zebra = doc.find("'zebra'").unwrap();
    assert!(
        apple < mango && mango < zebra,
        "rows must emit in name order"
    );
}

#[test]
fn round_trip_preserves_config_projection() {
    let s = sample_state();
    let doc = crate::emit(&s, &stamp());
    let loaded = load(&doc).unwrap();

    // Per-collection cardinality is preserved across BOTH stores.
    assert_eq!(loaded.server.endpoints.len(), 1);
    assert_eq!(loaded.server.triggers.len(), 1);
    assert_eq!(loaded.server.jobs.len(), 1);
    assert_eq!(loaded.server.views.len(), 1);
    assert_eq!(loaded.server.policies.len(), 1);
    assert_eq!(loaded.server.webhooks.len(), 1);
    assert_eq!(loaded.sys.drivers.len(), 1);
    assert_eq!(loaded.sys.policies.len(), 1);
    assert_eq!(loaded.sys.settings.len(), 1);
    assert_eq!(loaded.sys.bindings.len(), 2);
    assert_eq!(loaded.sys.transforms.len(), 1);

    // The bodies survive verbatim (canonical spec kept literal, never re-canonicalised twice).
    assert_eq!(
        loaded.server.endpoints["recent"].query.as_str(),
        s.server.endpoints["recent"].query.as_str()
    );
    assert_eq!(
        loaded.server.policies["readers"].allow,
        vec!["ALLOW SELECT"]
    );
    // The /sys rows survive exactly — including the JSON auth scheme and the secret REFERENCE.
    assert_eq!(loaded.sys.drivers["chatwork"], s.sys.drivers["chatwork"]);
    assert_eq!(loaded.sys.bindings["/chat"], s.sys.bindings["/chat"]);
    assert_eq!(loaded.sys.bindings["/chat2"], s.sys.bindings["/chat2"]);
    // §15: the transform definition survives exactly — INPUT/OUTPUT JSON + the secret REFERENCE.
    assert_eq!(
        loaded.sys.transforms["classify"],
        s.sys.transforms["classify"]
    );

    // The config projection is identical ⇒ an empty reconcile plan (idempotent).
    assert!(
        diff(&s, &loaded).is_empty(),
        "emit -> load must reproduce the config projection"
    );
}

#[test]
fn agent_binding_round_trips_through_dump_restore() {
    // blueprint §19 axis A: the §16 provision dump/restore loop round-trips an agent binding
    // (credential-free: name + attached policy handle only). emit -> load reproduces the exact
    // projection, so the reconcile plan is empty (idempotent).
    let mut server = ServerState::new();
    server.agents.insert(
        "triage".to_string(),
        AgentDef {
            name: "triage".to_string(),
            policy: Some("narrow".to_string()),
        },
    );
    server.agents.insert(
        "sweeper".to_string(),
        AgentDef {
            name: "sweeper".to_string(),
            policy: None,
        },
    );
    let s = cfg(server);

    let doc = crate::emit(&s, &stamp());
    let loaded = load(&doc).unwrap();
    assert_eq!(loaded.server.agents.len(), 2);
    assert_eq!(
        loaded.server.agents["triage"].policy.as_deref(),
        Some("narrow")
    );
    assert_eq!(loaded.server.agents["sweeper"].policy, None);
    assert!(
        diff(&s, &loaded).is_empty(),
        "emit -> load must reproduce the agent config projection"
    );
}

#[test]
fn runtime_fields_are_excluded_and_not_drift() {
    let with_runtime = sample_state();
    let mut fresh = sample_state();
    // Clear the runtime fields on the second state.
    let job = fresh.server.jobs.get_mut("nightly").unwrap();
    job.last_run = None;
    let view = fresh.server.views.get_mut("digest").unwrap();
    view.last_run = None;
    view.cache_json = None;

    // The emitted documents are byte-identical (runtime fields never emit)...
    assert_eq!(
        crate::emit(&with_runtime, &stamp()),
        crate::emit(&fresh, &stamp())
    );
    // ...and the diff between them is empty (a refresh is not drift).
    assert!(diff(&with_runtime, &fresh).is_empty());
    assert!(diff(&fresh, &with_runtime).is_empty());
}

#[test]
fn a_double_dash_in_a_path_does_not_truncate_the_document() {
    // The twin of the server crate's boot test: both `.qfs` consumers share one splitter, so a
    // `--` inside a path must be path text here too. It used to cut the statement at the `--`,
    // swallow the `;`, and merge the next statement in — a bogus parse error on the wrong line.
    let doc = "\
CREATE POLICY readers ALLOW SELECT ON /local/a--b/*;
UPSERT INTO /server/webhooks VALUES (name, route) ('ingest', '/hooks/ingest');
";
    let state = load(doc).expect("a `--` inside a path must not break the document");
    assert_eq!(state.server.policies.len(), 1, "the `--` policy loaded");
    assert_eq!(
        state.server.webhooks.len(),
        1,
        "the statement after the `--` path loaded too — its `;` was not swallowed"
    );
}

#[test]
fn a_trailing_hash_comment_containing_a_semicolon_does_not_split_the_document() {
    // The old stripper honoured `#` only at the start of a trimmed line, so the `;` inside this
    // trailing comment split off a bogus statement and raised UNEXPECTED_EOF.
    let doc = "CREATE POLICY readers ALLOW SELECT; # note; more\n";
    let state = load(doc).expect("a trailing `#` comment must be a comment");
    assert_eq!(state.server.policies.len(), 1);
}

#[test]
fn cosmetic_formatting_is_not_drift() {
    // Two documents differing only in whitespace, blank lines, and comments.
    let tidy = "\
UPSERT INTO /sys/settings VALUES (key, value) ('safety_mode', 'policy-only');
UPSERT INTO /server/webhooks VALUES (name, route) ('ingest', '/hooks/ingest');
CREATE POLICY readers ALLOW SELECT;
";
    let messy = "\
# a leading comment

UPSERT INTO /sys/settings   VALUES (key, value)   ('safety_mode', 'policy-only')  ;
UPSERT INTO /server/webhooks    VALUES (name, route)   ('ingest', '/hooks/ingest')  ;

   # another comment
CREATE POLICY   readers   ALLOW SELECT ;
";
    let a = load(tidy).unwrap();
    let b = load(messy).unwrap();
    assert!(
        diff(&a, &b).is_empty(),
        "cosmetic-only differences must load to equal states"
    );
    assert_eq!(a.server.webhooks.len(), 1);
    assert_eq!(a.server.policies.len(), 1);
    assert_eq!(a.sys.settings.len(), 1);
}

#[test]
fn connect_sugar_loads_as_its_sys_paths_twin() {
    // The in-language CONNECT form and the raw /sys/paths write twin load to the SAME binding
    // (the parser desugars CONNECT itself — the loader sees one canonical write).
    let sugar = load(
        "CONNECT /chat TO chatwork AT 'https://api.chatwork.com/v2' \
         SECRET 'vault:chatwork/work' ACCOUNT 'work';
         CONNECT /chat2 TO /chat;",
    )
    .unwrap();
    let twin = load(
        "UPSERT INTO /sys/paths VALUES (account, at, driver, path, secret_ref) \
         ('work', 'https://api.chatwork.com/v2', 'chatwork', '/chat', 'vault:chatwork/work');
         UPSERT INTO /sys/paths VALUES (alias_of, path) ('/chat', '/chat2');",
    )
    .unwrap();
    assert_eq!(sugar.sys.bindings["/chat"], twin.sys.bindings["/chat"]);
    assert_eq!(sugar.sys.bindings["/chat2"], twin.sys.bindings["/chat2"]);
    assert!(diff(&sugar, &twin).is_empty(), "two forms, one projection");
}

#[test]
fn emitted_document_carries_only_universe_statement_forms() {
    // Sanity: every statement line is a config write against one of the two stores — the
    // emitter's own structure never introduces another form, and every `"` in the document
    // rides inside a single-quoted literal (a canonical body / JSON descriptor).
    let doc = crate::emit(&sample_state(), &stamp());
    for line in doc.lines() {
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        assert!(
            line.starts_with("UPSERT INTO /server/")
                || line.starts_with("CREATE POLICY ")
                || line.starts_with("UPSERT INTO /sys/")
                || line.starts_with("INSERT INTO /sys/")
                // §15: the top-level `/transform` definition collection.
                || line.starts_with("UPSERT INTO /transform "),
            "unexpected statement form: {line}"
        );
    }
}

// ---------------------------------------------------------------------------
// Diff cases
// ---------------------------------------------------------------------------

#[test]
fn diff_unchanged_is_empty() {
    let s = sample_state();
    let plan = diff(&s, &s);
    assert!(plan.is_empty());
    assert_eq!(plan.add_count(), 0);
    assert_eq!(plan.change_count(), 0);
    assert_eq!(plan.destroy_count(), 0);
    assert!(!plan.has_destroy());
}

#[test]
fn diff_new_row_is_insert() {
    let current = sample_state();
    let mut desired = sample_state();
    desired.server.jobs.insert(
        "hourly".to_string(),
        JobDef {
            name: "hourly".to_string(),
            every: "1h".to_string(),
            plan: body("/mail |> LIMIT 10"),
            policy: None,
            last_run: None,
        },
    );
    let plan = diff(&current, &desired);
    assert_eq!(plan.add_count(), 1);
    assert_eq!(plan.change_count(), 0);
    assert_eq!(plan.destroy_count(), 0);
    let op = plan.ops().iter().find(|o| o.name == "hourly").unwrap();
    assert_eq!(op.op, ServerWriteOp::Insert);
    assert_eq!(op.node, ReconcileNode::Server(ServerNode::Jobs));
    // The op carries a schema-valid payload for the plan builder.
    assert!(op.row_batch().is_ok());
}

#[test]
fn diff_drifted_row_is_update() {
    let current = sample_state();
    let mut desired = sample_state();
    // Change the EVERY interval — a config-projection drift.
    desired.server.jobs.get_mut("nightly").unwrap().every = "2h".to_string();
    let plan = diff(&current, &desired);
    assert_eq!(plan.change_count(), 1);
    assert_eq!(plan.add_count(), 0);
    assert_eq!(plan.destroy_count(), 0);
    let op = plan.ops().iter().find(|o| o.name == "nightly").unwrap();
    assert_eq!(op.op, ServerWriteOp::Update);
}

#[test]
fn diff_absent_row_is_remove_and_flags_destroy() {
    let current = sample_state();
    let mut desired = sample_state();
    desired.server.webhooks.remove("ingest");
    let plan = diff(&current, &desired);
    assert_eq!(plan.destroy_count(), 1);
    assert_eq!(plan.add_count(), 0);
    assert_eq!(plan.change_count(), 0);
    assert!(plan.has_destroy(), "a Remove must set has_destroy");
    let op = plan.ops().iter().find(|o| o.name == "ingest").unwrap();
    assert_eq!(op.op, ServerWriteOp::Remove);
    // Remove still yields a name-only payload apply_server_write can consume.
    assert!(op.row_batch().is_ok());
}

#[test]
fn has_destroy_is_false_without_a_remove() {
    let current = sample_state();
    let mut desired = sample_state();
    desired.server.jobs.get_mut("nightly").unwrap().every = "2h".to_string();
    desired.server.endpoints.insert(
        "new".to_string(),
        EndpointDef {
            name: "new".to_string(),
            method: "GET".to_string(),
            route: "/new".to_string(),
            query: body("/mail |> LIMIT 10"),
            policy: None,
        },
    );
    let plan = diff(&current, &desired);
    assert!(plan.add_count() >= 1 && plan.change_count() >= 1);
    assert!(!plan.has_destroy(), "no Remove ⇒ has_destroy is false");
}

#[test]
fn sys_rows_diff_the_same_three_ways() {
    let current = sample_state();
    let mut desired = sample_state();
    // Drift the setting, add a driver, drop a binding.
    desired
        .sys
        .settings
        .insert("safety_mode".to_string(), "always-ask".to_string());
    desired.sys.drivers.insert(
        "linear".to_string(),
        SysDriverRow {
            name: "linear".to_string(),
            kind: "driver".to_string(),
            ..SysDriverRow::default()
        },
    );
    desired.sys.bindings.remove("/chat2");
    let plan = diff(&current, &desired);
    assert_eq!(plan.add_count(), 1);
    assert_eq!(plan.change_count(), 1);
    assert_eq!(plan.destroy_count(), 1);
    let changed = plan.ops().iter().find(|o| o.name == "safety_mode").unwrap();
    assert_eq!(changed.node, ReconcileNode::Sys(SysCollection::Settings));
    assert_eq!(changed.op, ServerWriteOp::Update);
    let removed = plan.ops().iter().find(|o| o.name == "/chat2").unwrap();
    assert_eq!(removed.node, ReconcileNode::Sys(SysCollection::Paths));
    assert_eq!(removed.op, ServerWriteOp::Remove);
}

#[test]
fn policies_with_the_same_name_diff_independently_per_store() {
    // One policy name in BOTH stores — two collections, never conflated (blueprint §16).
    let mut current = sample_state();
    current.server.policies.insert(
        "shared".to_string(),
        PolicyDef {
            name: "shared".to_string(),
            handler: String::new(),
            allow: vec!["ALLOW SELECT".to_string()],
        },
    );
    current.sys.policies.insert(
        "shared".to_string(),
        SysPolicyRow {
            name: "shared".to_string(),
            allow: Some("SELECT".to_string()),
            target: Some("/sql/*".to_string()),
        },
    );

    // Drift ONLY the /sys one: exactly one op, addressed at the /sys store.
    let mut desired = current.clone();
    desired.sys.policies.get_mut("shared").unwrap().target = Some("/mail/*".to_string());
    let plan = diff(&current, &desired);
    assert_eq!(plan.ops().len(), 1);
    assert_eq!(
        plan.ops()[0].node,
        ReconcileNode::Sys(SysCollection::Policies)
    );

    // Drift ONLY the /server one: exactly one op, addressed at the /server store.
    let mut desired = current.clone();
    desired
        .server
        .policies
        .get_mut("shared")
        .unwrap()
        .allow
        .push("DENY REMOVE".to_string());
    let plan = diff(&current, &desired);
    assert_eq!(plan.ops().len(), 1);
    assert_eq!(
        plan.ops()[0].node,
        ReconcileNode::Server(ServerNode::Policies)
    );
}

#[test]
fn secretish_settings_are_excluded_never_emitted_never_diffed_never_destroyed() {
    // A current state carrying a secretish setting (as a raw fetch of sys_settings would).
    let mut current = sample_state();
    current
        .sys
        .settings
        .insert("api_token".to_string(), "TOKEN-CANARY".to_string());

    // Never emitted: neither the key nor the value appears in the document.
    let doc = crate::emit(&current, &stamp());
    assert!(!doc.contains("api_token"));
    assert!(!doc.contains("TOKEN-CANARY"));

    // Never diffed / never destroyed by absence: an EMPTY desired universe destroys the
    // ordinary rows but produces ZERO ops naming the secretish setting. (Billing and
    // sys_ddl_events are excluded structurally: SysState has no collection for them, so no
    // op can ever address them.)
    let plan = diff(&current, &ConfigState::new());
    assert!(plan.has_destroy());
    assert!(
        plan.ops().iter().all(|o| o.name != "api_token"),
        "a secretish setting must never be planned, even for destroy"
    );
    // The non-secretish setting IS authoritatively destroyed.
    assert!(plan
        .ops()
        .iter()
        .any(|o| o.name == "safety_mode" && o.op == ServerWriteOp::Remove));

    // And the loader refuses a document that tries to declare one.
    let err =
        load("UPSERT INTO /sys/settings VALUES (key, value) ('api_token', 'x');").unwrap_err();
    assert!(matches!(err, crate::LoadError::Universe { .. }));
}

#[test]
fn excluded_sys_nodes_are_rejected_by_the_loader() {
    // Billing is outside the universe entirely (blueprint §16) — not loadable, so never
    // diffable, so never destroyable.
    let err =
        load("UPSERT INTO /sys/billing VALUES (team_id, tier, status) ('t', 'paid', 'active');")
            .unwrap_err();
    assert!(matches!(err, crate::LoadError::Universe { .. }));
}

// ---------------------------------------------------------------------------
// Plan builder + preview
// ---------------------------------------------------------------------------

#[test]
fn build_plan_flags_destroys_irreversible_in_preview() {
    let current = sample_state();
    let mut desired = sample_state();
    // One destroy per store + one add + one change.
    desired.server.webhooks.remove("ingest");
    desired.sys.bindings.remove("/chat2");
    desired.server.jobs.get_mut("nightly").unwrap().every = "2h".to_string();
    desired
        .sys
        .settings
        .insert("theme".to_string(), "dark".to_string());

    let rp = diff(&current, &desired);
    assert_eq!(rp.add_count(), 1);
    assert_eq!(rp.change_count(), 1);
    assert_eq!(rp.destroy_count(), 2);

    let plan = build_plan(&rp).unwrap();
    let pv = preview(&plan);
    // Every reconcile op renders as one preview row; the counts line up.
    assert_eq!(
        pv.rows.len(),
        rp.add_count() + rp.change_count() + rp.destroy_count()
    );
    // Both destroys — the /server webhook (flagged explicitly) and the /sys binding
    // (EffectKind::Remove, inherently irreversible) — are called out.
    assert_eq!(pv.irreversible.len(), rp.destroy_count());
    assert!(
        plan.is_irreversible(),
        "the IrreversibleGuard sees the plan"
    );

    // A destroy-free plan is fully reversible.
    let mut desired = sample_state();
    desired
        .sys
        .settings
        .insert("theme".to_string(), "dark".to_string());
    let rp = diff(&current, &desired);
    assert!(!rp.has_destroy());
    let plan = build_plan(&rp).unwrap();
    assert!(preview(&plan).irreversible.is_empty());
    assert!(!plan.is_irreversible());
}
