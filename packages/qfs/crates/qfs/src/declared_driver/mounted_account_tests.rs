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

#[test]
fn shipped_slack_file_content_is_discoverable_as_bytes_on_named_mounts() {
    let (_, _, mount) = shipped_mount(qfs_skill::SLACK_DRIVER, "slack", "/slack-work");
    let description = report(&mount, "/slack-work/acme/files/F0123/content");
    assert_eq!(column_names(&description), ["content"]);
    assert_eq!(description.columns[0].ty, ColumnType::Bytes);
    qfs_core::check_capability(&mount, &qfs_core::Path::new("/slack-work/acme/files/F0123/content"), qfs_core::Verb::Select).unwrap();
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
    mounted_read_node(path, "same-workspace/C1/messages", mock).await
}

async fn mounted_read_node(
    path: &str,
    node: &str,
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
            path: format!("{path}/{node}"),
            pushed: qfs_pushdown::PushedQuery::default(),
            schema: Schema::new(vec![]),
            materialize_content: false,
        },
        &RequestContext::anonymous(),
    )
    .await
}

#[tokio::test]
async fn mounted_slack_file_listing_and_content_use_the_same_selected_account() {
    let _home = crate::testenv::HomeGuard::with_passphrase("slack-attachment-accounts");
    seed_accounts();
    for (path, account, token, payload) in [
        ("/slack-a", "work-a", "token-a", b"%PDF-1.7\n\0\xffaccount-a\n%%EOF".as_slice()),
        ("/slack-b", "work-b", "token-b", b"%PDF-1.7\n\0\xfeaccount-b\n%%EOF".as_slice()),
    ] {
        bind(path, account, None);
        let mock = Arc::new(qfs_driver_http::MockHttpClient::new());
        mock.push_response(qfs_driver_http::HttpResponse::new(200,
            br#"{"ok":true,"files":[{"id":"F1","name":"report.pdf","mimetype":"application/pdf","size":27,"created":1,"user":"U1"}]}"#.to_vec()));
        let listing = mounted_read_node(path, "workspace/C1/files", mock.clone()).await.unwrap();
        assert_eq!(listing.rows[0].values[0], Value::Text("F1".into()));
        mock.push_response(qfs_driver_http::HttpResponse::new(200,
            br#"{"ok":true,"file":{"id":"F1","url_private":"https://files.slack.com/files-pri/T1-F1/report.pdf"}}"#.to_vec()));
        mock.push_response(qfs_driver_http::HttpResponse::new(200, payload.to_vec()));
        let content = mounted_read_node(path, "workspace/files/F1/content", mock.clone()).await.unwrap();
        assert_eq!(content.schema.columns[0].name, "content");
        assert_eq!(content.rows[0].values, vec![Value::Bytes(payload.to_vec())]);
        let requests = mock.recorded();
        assert_eq!(requests.len(), 3);
        assert_eq!(requests[0].url, "https://slack.com/api/files.list?channel=C1");
        assert_eq!(requests[1].url, "https://slack.com/api/files.info?file=F1");
        assert!(requests.iter().all(|req| req.header_value("Authorization") == Some(format!("Bearer {token}").as_str())));
    }
}

#[tokio::test]
async fn mounted_slack_file_read_fails_closed_for_an_inaccessible_selected_account() {
    let _home = crate::testenv::HomeGuard::with_passphrase("slack-attachment-denied");
    seed_accounts();
    bind("/slack-a", "work-a", None);
    let mock = Arc::new(qfs_driver_http::MockHttpClient::new());
    mock.push_response(qfs_driver_http::HttpResponse::new(200,
        br#"{"ok":false,"error":"file_not_found"}"#.to_vec()));
    let error = mounted_read_node("/slack-a", "workspace/files/F1/content", mock.clone()).await.unwrap_err();
    assert!(error.to_string().contains("slack_file_not_found"));
    assert_eq!(mock.recorded().len(), 1);
    bind("/slack-a", "absent", None);
    let mock = Arc::new(qfs_driver_http::MockHttpClient::new());
    assert!(mounted_read_node("/slack-a", "workspace/files/F1/content", mock.clone()).await.is_err());
    assert!(mock.recorded().is_empty());
}

fn mounted_registry(
    path: &str,
    mock: Arc<qfs_driver_http::MockHttpClient>,
) -> (qfs_core::DriverId, DriverRegistry) {
    mounted_registry_for(path, mock, &shipped_slack_declared_driver())
}

fn mounted_registry_for(
    path: &str,
    mock: Arc<qfs_driver_http::MockHttpClient>,
    d: &DeclaredDriver,
) -> (qfs_core::DriverId, DriverRegistry) {
    let binding =
        crate::path_binding::db_get_binding(&crate::connection::open_system_conn().unwrap(), path)
            .unwrap()
            .unwrap();
    let secrets = declared_secrets(
        d,
        binding.secret_ref.as_deref(),
        binding.account.as_deref(),
        None,
    );
    let driver = live_rest_driver(d, &qfs_core::DeclaredTypeDefs::new(), mock, secrets).unwrap();
    let remap = declared_remap(path, "slack").unwrap();
    let id = remap.outer_id();
    let facet = crate::apply_facets::RestApplyDriver::new(
        Arc::new(qfs_driver_http::rest_apply_driver(&driver)),
        d.name.clone(),
        crate::declared_eval::map_specs(d),
        crate::declared_eval::view_specs(d, &shipped_slack_types()),
        driver.rest_applier().clone(),
        crate::declared_eval::shared_lookups(d),
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

fn parsed_mount_plan(path: &str, d: &DeclaredDriver, source: &str) -> qfs_core::Plan {
    let mut mounts = qfs_core::MountRegistry::new();
    mounts
        .register(Arc::new(
            declared_describe_mount_with_types(path, d, &qfs_core::DeclaredTypeDefs::new())
                .unwrap(),
        ))
        .unwrap();
    let stmt = qfs_exec::parse(source).expect("shipped spelling parses");
    qfs_core::Evaluator::new(&mounts)
        .eval(&stmt)
        .expect("selected mount resolves")
        .as_plan()
        .unwrap()
        .clone()
}

#[tokio::test]
async fn shipped_slack_post_example_commits_explicit_text() {
    let _home = crate::testenv::HomeGuard::with_passphrase("slack-explicit-example");
    seed_accounts();
    let cookbook = include_str!("../../../../../../docs/cookbook/slack.md");
    let mut cases = vec![(
        include_str!("../../../skill/assets/examples/slack.qfs").to_string(),
        "/slack",
        "general",
        "Deploy finished",
    )];
    let embedded = qfs_skill::SKILL_MD
        .lines()
        .find(|line| line.trim_start().starts_with("insert into /slack/"))
        .unwrap();
    cases.push((
        embedded.trim().to_string(),
        "/slack",
        "general",
        "Deploy finished",
    ));
    let recipes: Vec<_> = cookbook
        .split("```qfs\n")
        .skip(1)
        .map(|part| part.split("```").next().unwrap().trim())
        .filter(|recipe| recipe.starts_with("insert into /slack"))
        .collect();
    assert_eq!(
        recipes.len(),
        3,
        "update fixture expectations when adding a posting recipe"
    );
    for (recipe, (mount, text)) in recipes.into_iter().zip([
        ("/slack", "Deploy finished ✅"),
        ("/slack", "Deploy finished ✅"),
        ("/slack-me", "Sent from my own account 👋"),
    ]) {
        cases.push((recipe.to_string(), mount, "general", text));
    }
    let shell: Vec<_> = cookbook
        .lines()
        .filter(|line| line.starts_with(r#"qfs run -e "insert into /slack"#))
        .collect();
    assert_eq!(shell.len(), 2);
    for (line, (mount, channel, text)) in shell.into_iter().zip([
        ("/slack-a", "C0123456789", "Hello from work-a"),
        ("/slack-b", "C9876543210", "Hello from work-b"),
    ]) {
        cases.push((
            line.split('"').nth(1).unwrap().to_string(),
            mount,
            channel,
            text,
        ));
    }
    for (source, path, channel, text) in cases {
        bind(path, "work-a", None);
        let d = shipped_slack_declared_driver();
        let plan = parsed_mount_plan(path, &d, &source);
        let mock = Arc::new(qfs_driver_http::MockHttpClient::new());
        response(&mock);
        let (id, registry) = mounted_registry(path, mock.clone());
        let caps = CapabilitySet::none().grant(id, &EffectKind::Insert);
        assert!(Interpreter::with_defaults(registry)
            .commit(plan, &caps)
            .await
            .unwrap()
            .is_complete());
        let requests = mock.recorded();
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(requests[0].body.as_ref().unwrap())
                .unwrap(),
            serde_json::json!({"channel":channel,"text":text}),
            "{source}"
        );
    }
}

#[tokio::test]
async fn advertised_slack_calls_commit_on_the_selected_hyphenated_mount() {
    let _home = crate::testenv::HomeGuard::with_passphrase("slack-named-calls");
    seed_accounts();
    bind("/slack-a", "work-a", None);
    bind("/slack-b", "work-b", None);
    let d = shipped_slack_declared_driver();
    for (path, token) in [("/slack-a", "token-a"), ("/slack-b", "token-b")] {
        for proc in d.procedures() {
            let args = match proc.name.as_str() {
                "react" => "channel => 'general', ts => '1.1', emoji => 'eyes'",
                "update" => "channel => 'general', ts => '1.1', text => 'updated'",
                _ => "channel => 'general', ts => '1.1'",
            };
            let source = format!(
                "{path}/W1/general/messages |> CALL {}.{}({args})",
                path.trim_start_matches('/'),
                proc.name
            );
            let plan = parsed_mount_plan(path, &d, &source);
            assert_eq!(plan.nodes()[0].irreversible, proc.irreversible);
            let mock = Arc::new(qfs_driver_http::MockHttpClient::new());
            mock.push_response(qfs_driver_http::HttpResponse::new(
                200,
                SLACK_CHANNELS_FIXTURE.as_bytes().to_vec(),
            ));
            response(&mock);
            let (id, registry) = mounted_registry(path, mock.clone());
            let kind = EffectKind::Call(qfs_core::ProcId::new(format!(
                "{}.{}",
                path.trim_start_matches('/'),
                proc.name
            )));
            let caps = CapabilitySet::none().grant(id, &kind);
            let outcome = Interpreter::with_defaults(registry)
                .commit(plan, &caps)
                .await
                .unwrap();
            assert!(outcome.is_complete(), "{source}: {outcome:?}");
            let requests = mock.recorded();
            assert_eq!(requests.len(), 2, "{source}");
            assert!(requests
                .iter()
                .all(|r| r.header_value("authorization") == Some(&format!("Bearer {token}"))));
            let endpoint = match proc.name.as_str() {
                "react" => "reactions.add",
                "pin" => "pins.add",
                "unpin" => "pins.remove",
                "update" => "chat.update",
                "delete" => "chat.delete",
                other => panic!("{other}"),
            };
            assert_eq!(requests[1].url, format!("https://slack.com/api/{endpoint}"));
            let body: serde_json::Value =
                serde_json::from_slice(requests[1].body.as_ref().unwrap()).unwrap();
            let expected = match proc.name.as_str() {
                "react" => serde_json::json!({"channel":"C0EQUIV","timestamp":"1.1","name":"eyes"}),
                "pin" | "unpin" => serde_json::json!({"channel":"C0EQUIV","timestamp":"1.1"}),
                "update" => serde_json::json!({"channel":"C0EQUIV","ts":"1.1","text":"updated"}),
                "delete" => serde_json::json!({"channel":"C0EQUIV","ts":"1.1"}),
                other => panic!("{other}"),
            };
            assert_eq!(body, expected);
        }
    }
}

#[tokio::test]
async fn explicit_slack_text_survives_schema_order_and_thread_mapping() {
    let _home = crate::testenv::HomeGuard::with_passphrase("slack-schema-thread");
    seed_accounts();
    bind("/slack-a", "work-a", None);
    let mut d = shipped_slack_declared_driver();
    let post = d.maps.iter_mut().find(|m| m.verb == "INSERT").unwrap();
    post.body = serde_json::to_string(&qfs_exec::parse(
        "INSERT INTO /http/slack/chat.postMessage VALUES ({channel: path.channel, text: row.text, thread_ts: row.thread_ts})"
    ).unwrap()).unwrap();
    for columns in [
        vec!["ts", "user", "text", "thread_ts", "subtype"],
        vec!["subtype", "thread_ts", "text", "user", "ts"],
    ] {
        let schema = Schema::new(
            columns
                .into_iter()
                .map(|n| Column::new(n, ColumnType::Text, true))
                .collect(),
        );
        let mut types = qfs_core::DeclaredTypeDefs::new();
        types.insert(
            "slack/message".into(),
            qfs_core::ddl::types::ResolvedTypeDef {
                columns: vec![],
                schema,
                refinement: None,
                column_refinements: vec![],
            },
        );
        let mut mounts = qfs_core::MountRegistry::new();
        mounts
            .register(Arc::new(
                declared_describe_mount_with_types("/slack-a", &d, &types).unwrap(),
            ))
            .unwrap();
        let evaluator = qfs_core::Evaluator::new(&mounts);
        let positional =
            qfs_exec::parse("INSERT INTO /slack-a/W/C1/messages VALUES ('reply')").unwrap();
        let positional = evaluator
            .eval(&positional)
            .unwrap()
            .as_plan()
            .unwrap()
            .clone();
        let args = &positional.nodes()[0].args;
        assert_ne!(
            args.schema.columns[0].name, "text",
            "the old positional spelling binds the first described column"
        );
        let source = qfs_exec::parse(
            "INSERT INTO /slack-a/W/C1/messages VALUES (text, thread_ts) ('reply', '1.000001')",
        )
        .unwrap();
        let plan = evaluator.eval(&source).unwrap().as_plan().unwrap().clone();
        let mock = Arc::new(qfs_driver_http::MockHttpClient::new());
        response(&mock);
        let (id, registry) = mounted_registry_for("/slack-a", mock.clone(), &d);
        let caps = CapabilitySet::none().grant(id, &EffectKind::Insert);
        assert!(Interpreter::with_defaults(registry)
            .commit(plan, &caps)
            .await
            .unwrap()
            .is_complete());
        let requests = mock.recorded();
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(requests[0].body.as_ref().unwrap())
                .unwrap(),
            serde_json::json!({"channel":"C1","text":"reply","thread_ts":"1.000001"})
        );
    }
}

#[test]
fn named_slack_calls_do_not_resolve_a_missing_mount_or_bad_arguments() {
    let d = shipped_slack_declared_driver();
    let mut mounts = qfs_core::MountRegistry::new();
    mounts
        .register(Arc::new(
            declared_describe_mount_with_types("/slack-a", &d, &qfs_core::DeclaredTypeDefs::new())
                .unwrap(),
        ))
        .unwrap();
    for source in [
        "/slack-b/W/C/messages |> CALL slack-b.pin('C', '1')",
        "/slack-a/W/C/messages |> CALL slack-b.pin('C', '1')",
        "/slack-a/W/C/messages |> CALL slack-a.pin(channel => 'C', emoji => 'eyes')",
    ] {
        let stmt = qfs_exec::parse(source).unwrap();
        assert!(
            qfs_core::Evaluator::new(&mounts).eval(&stmt).is_err(),
            "{source}"
        );
    }
}
