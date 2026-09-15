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

fn mounted_registry(
    path: &str,
    mock: Arc<qfs_driver_http::MockHttpClient>,
) -> (qfs_core::DriverId, DriverRegistry) {
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
    (id, registry)
}

async fn mounted_write(path: &str, mock: Arc<qfs_driver_http::MockHttpClient>) -> bool {
    let (id, registry) = mounted_registry(path, mock);
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

#[tokio::test]
async fn mounted_slack_rejections_fail_reads_and_commits_without_retry() {
    let _home = crate::testenv::HomeGuard::with_passphrase("slack-response-contract");
    seed_accounts();
    bind("/slack-a", "work-a", None);
    for code in ["missing_post_type", "missing_scope", "channel_not_found"] {
        let mock = Arc::new(qfs_driver_http::MockHttpClient::new());
        let body = format!(r#"{{"ok":false,"error":"{code}"}}"#);
        mock.push_response(qfs_driver_http::HttpResponse::new(
            200,
            body.as_bytes().to_vec(),
        ));
        assert!(!mounted_write("/slack-a", mock.clone()).await);
        assert_eq!(mock.recorded().len(), 1, "a failed POST must not retry");
        assert_eq!(
            mock.recorded()[0].header_value("Content-Type"),
            Some("application/json")
        );
        mock.push_response(qfs_driver_http::HttpResponse::new(200, body.into_bytes()));
        let error = mounted_read("/slack-a", mock.clone()).await.unwrap_err();
        assert!(error.to_string().contains(code), "{error}");
    }
}

#[test]
fn slack_response_contract_is_selected_by_exact_api_base_not_driver_label() {
    let mut d = shipped_slack_declared_driver();
    d.name = "another-label".into();
    assert!(d.rest_config().response_contract.is_some());
    d.base_url = "https://slack.com/api/".into();
    assert!(d.rest_config().response_contract.is_some());
    for url in [
        "https://slack.com.evil.test/api",
        "https://example.test/api",
        "https://slack.com/other",
        "http://slack.com/api",
    ] {
        d.base_url = url.into();
        assert!(d.rest_config().response_contract.is_none(), "{url}");
    }
}

#[test]
fn slack_failure_reaches_cli_exit_and_output_without_success_receipt() {
    use qfs_exec::{
        run_oneshot, ErrorKind, ExecCtx, ExecError, OutputFormat, ReadRegistry, StmtSource, Streams,
    };
    let _home = crate::testenv::HomeGuard::with_passphrase("slack-cli-outcome");
    seed_accounts();
    bind("/slack-a", "work-a", None);
    let d = shipped_slack_declared_driver();
    let mut engine = qfs_core::Engine::new();
    engine
        .mounts
        .register(Arc::new(
            declared_describe_mount_with_types("/slack-a", &d, &qfs_core::DeclaredTypeDefs::new())
                .unwrap(),
        ))
        .unwrap();
    let reads = ReadRegistry::new();
    let mock = Arc::new(qfs_driver_http::MockHttpClient::new());
    mock.push_response(qfs_driver_http::HttpResponse::new(
        200,
        br#"{"ok":false,"error":"missing_scope"}"#.to_vec(),
    ));
    let world = |plan: &qfs_core::Plan| -> Result<(), ExecError> {
        let (id, registry) = mounted_registry("/slack-a", mock.clone());
        let caps = CapabilitySet::none().grant(id, &EffectKind::Insert);
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let result = rt.block_on(Interpreter::with_defaults(registry).commit(plan.clone(), &caps));
        match result {
            Ok(outcome) if outcome.is_complete() => Ok(()),
            other => Err(ExecError::new(
                ErrorKind::CommitFailed,
                "commit_failed",
                format!("{other:?}"),
            )),
        }
    };
    let ctx = ExecCtx {
        engine: &engine,
        reads: &reads,
        world_apply: Some(&world),
        safety_mode: qfs_core::SafetyMode::default(),
        transform: None,
    };
    let source = StmtSource::Expr(
        "insert into /slack-a/workspace/C1/messages values (text) ('hello')".into(),
    );
    for commit in [false, true] {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = run_oneshot(
            &source,
            &ctx,
            OutputFormat::Json,
            commit,
            false,
            &mut Streams {
                out: &mut out,
                err: &mut err,
            },
        )
        .code();
        let out = String::from_utf8(out).unwrap();
        let err = String::from_utf8(err).unwrap();
        if commit {
            assert_eq!(code, 5, "{out} {err}");
            assert!(
                err.contains("commit_failed") && err.contains("missing_scope"),
                "{err}"
            );
            assert!(!out.contains(r#""committed":true"#));
            assert_eq!(mock.recorded().len(), 1);
        } else {
            assert_eq!(code, 0, "{err}");
            assert!(mock.recorded().is_empty(), "preview must not send");
        }
    }
}

#[test]
fn service_read_failure_has_a_non_usage_exit_code_and_preserves_reason() {
    let err = crate::declared_driver::read_http_error(
        "/rest/slack/conversations.history",
        qfs_driver_http::HttpError::Application {
            code: "channel_not_found",
        },
    );
    let err = qfs_exec::ExecError::from_qfs(&err);
    assert_eq!(err.exit_code().code(), 5);
    assert_eq!(err.code, "channel_not_found");
    assert!(err.message.contains("channel_not_found"));
}
