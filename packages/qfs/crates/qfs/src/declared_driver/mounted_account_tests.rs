use super::*;
use qfs_core::{
    Column, ColumnType, EffectKind, EffectNode, NodeId, PlanBuilder, RequestContext, Row, RowBatch,
    Schema, Target, Value, VfsPath,
};
use qfs_exec::ReadDriver as _;
use qfs_runtime::{CapabilitySet, DriverRegistry, Interpreter};

fn account_key(label: &str) -> CredentialKey {
    CredentialKey::new(
        qfs_secrets::DriverId("slack".into()),
        qfs_secrets::ConnectionId::new(label).unwrap(),
    )
}

fn bind(path: &str, account: &str, secret_ref: Option<&str>) {
    crate::path_binding::db_upsert_binding(
        &crate::connection::open_system_conn().unwrap(),
        path,
        "slack",
        None,
        secret_ref,
        None,
        Some(account),
        None,
    )
    .unwrap();
}

fn seed_accounts() {
    let store = crate::connection::open_store().unwrap();
    for (label, token) in [
        ("default", "poisoned-default"),
        ("work-a", "token-a"),
        ("work-b", "token-b"),
    ] {
        store.put(&account_key(label), Secret::from(token)).unwrap();
    }
}

async fn mounted_read(
    path: &str,
    mock: Arc<qfs_driver_http::MockHttpClient>,
) -> Result<RowBatch, qfs_core::CfsError> {
    let d = shipped_slack_declared_driver();
    let binding =
        crate::path_binding::db_get_binding(&crate::connection::open_system_conn().unwrap(), path)
            .unwrap()
            .unwrap();
    let secrets = declared_secrets(
        &d,
        binding.secret_ref.as_deref(),
        binding.account.as_deref(),
        None,
    );
    let driver = live_rest_driver(&d, &qfs_core::DeclaredTypeDefs::new(), mock, secrets).unwrap();
    let read = crate::mount_adapter::MountReadDriver::new(
        declared_remap(path, "slack").unwrap(),
        Arc::new(crate::read_facets::RestReadDriver::new(
            driver.rest_applier().clone(),
            d.name.clone(),
            crate::declared_eval::view_specs(&d, &shipped_slack_types()),
        )),
    );
    read.scan(
        &qfs_pushdown::ScanNode {
            source: qfs_pushdown::SourceId::new(path.trim_start_matches('/')),
            path: format!("{path}/same-workspace/C1/messages"),
            pushed: qfs_pushdown::PushedQuery::default(),
            schema: Schema::new(vec![]),
            materialize_content: false,
        },
        &RequestContext::anonymous(),
    )
    .await
}

async fn mounted_write(path: &str, mock: Arc<qfs_driver_http::MockHttpClient>) -> bool {
    let d = shipped_slack_declared_driver();
    let binding =
        crate::path_binding::db_get_binding(&crate::connection::open_system_conn().unwrap(), path)
            .unwrap()
            .unwrap();
    let secrets = declared_secrets(
        &d,
        binding.secret_ref.as_deref(),
        binding.account.as_deref(),
        None,
    );
    let driver = live_rest_driver(&d, &qfs_core::DeclaredTypeDefs::new(), mock, secrets).unwrap();
    let remap = declared_remap(path, "slack").unwrap();
    let id = remap.outer_id();
    let facet = crate::apply_facets::RestApplyDriver::new(
        Arc::new(qfs_driver_http::rest_apply_driver(&driver)),
        d.name.clone(),
        crate::declared_eval::map_specs(&d),
        crate::declared_eval::view_specs(&d, &shipped_slack_types()),
        driver.rest_applier().clone(),
        crate::declared_eval::shared_lookups(&d),
    );
    let registry = DriverRegistry::new().with(
        id.clone(),
        Arc::new(crate::mount_adapter::MountApplyDriver::new(
            remap,
            Arc::new(facet),
        )),
    );
    let mut plan = PlanBuilder::new();
    plan.push(
        EffectNode::new(
            NodeId(0),
            EffectKind::Insert,
            Target::new(
                id.clone(),
                VfsPath::new(format!("{path}/same-workspace/C1/messages")),
            ),
        )
        .with_args(RowBatch::new(
            Schema::new(vec![Column::new("text", ColumnType::Text, false)]),
            vec![Row::new(vec![Value::Text("hello".into())])],
        )),
    );
    let caps = CapabilitySet::none().grant(id.clone(), &EffectKind::Insert);
    Interpreter::with_defaults(registry)
        .commit(plan.build(), &caps)
        .await
        .map(|out| out.is_complete())
        .unwrap_or(false)
}

fn response(mock: &qfs_driver_http::MockHttpClient) {
    mock.push_response(qfs_driver_http::HttpResponse::new(
        200,
        br#"{"ok":true,"messages":[]}"#.to_vec(),
    ));
}

#[test]
fn mounted_slack_legacy_default_and_non_bearer_auth_are_unchanged() {
    let _home = crate::testenv::HomeGuard::with_passphrase("mounted-legacy-test");
    seed_accounts();
    let mut d = shipped_slack_declared_driver();
    for account in [None, Some("")] {
        let token = declared_secrets(&d, None, account, None)
            .get(&account_key("default"))
            .unwrap();
        assert_eq!(token.expose_str(), Some("poisoned-default"));
    }
    for auth in [
        r#"{"kind":"header","name":"x-api-key"}"#,
        r#"{"kind":"none"}"#,
    ] {
        d.auth = auth.into();
        let token = declared_secrets(&d, None, Some("work-a"), None)
            .get(&account_key("default"))
            .unwrap();
        assert_eq!(token.expose_str(), Some("poisoned-default"));
    }
}

#[tokio::test]
async fn mounted_slack_accounts_isolate_reads_and_posts() {
    let _home = crate::testenv::HomeGuard::with_passphrase("mounted-accounts-test");
    seed_accounts();
    let mock = Arc::new(qfs_driver_http::MockHttpClient::new());
    bind("/slack-a", "work-a", None);
    bind("/slack-b", "work-b", None);
    for path in ["/slack-a", "/slack-b", "/slack-a"] {
        response(&mock);
        mounted_read(path, mock.clone()).await.unwrap();
        response(&mock);
        assert!(mounted_write(path, mock.clone()).await);
    }
    let requests = mock.recorded();
    assert_eq!(requests.len(), 6);
    for (request, token) in requests.iter().zip([
        "token-a", "token-a", "token-b", "token-b", "token-a", "token-a",
    ]) {
        assert!(request
            .headers
            .iter()
            .any(|(name, value)| name.eq_ignore_ascii_case("authorization")
                && value == &format!("Bearer {token}")));
    }
    for pair in requests.as_chunks::<2>().0 {
        assert!(pair[0].url.contains("conversations.history?channel=C1"));
        assert_eq!(pair[1].url, "https://slack.com/api/chat.postMessage");
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(pair[1].body.as_ref().unwrap()).unwrap(),
            serde_json::json!({"channel":"C1","text":"hello"})
        );
    }
}

#[tokio::test]
async fn mounted_slack_missing_and_revoked_accounts_never_use_default_or_send_http() {
    let _home = crate::testenv::HomeGuard::with_passphrase("mounted-missing-test");
    seed_accounts();
    crate::connection::open_store()
        .unwrap()
        .revoke(&account_key("work-b"))
        .unwrap();
    for account in ["absent", "work-b"] {
        bind("/slack-b", account, None);
        let mock = Arc::new(qfs_driver_http::MockHttpClient::new());
        response(&mock);
        assert!(mounted_read("/slack-b", mock.clone()).await.is_err());
        assert!(!mounted_write("/slack-b", mock.clone()).await);
        assert!(
            mock.recorded().is_empty(),
            "missing selected account must fail before HTTP"
        );
    }
}

#[tokio::test]
async fn mounted_slack_explicit_secret_retains_precedence_over_account() {
    let _home = crate::testenv::HomeGuard::with_passphrase("mounted-secret-test");
    seed_accounts();
    bind("/slack-custom", "absent", Some("vault:slack/work-b"));
    let mock = Arc::new(qfs_driver_http::MockHttpClient::new());
    response(&mock);
    mounted_read("/slack-custom", mock.clone()).await.unwrap();
    response(&mock);
    assert!(mounted_write("/slack-custom", mock.clone()).await);
    let requests = mock.recorded();
    assert_eq!(requests.len(), 2);
    assert!(requests
        .iter()
        .all(|request| request
            .headers
            .iter()
            .any(|(name, value)| name.eq_ignore_ascii_case("authorization")
                && value == "Bearer token-b")));
}
