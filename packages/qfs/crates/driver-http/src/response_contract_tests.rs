use super::*;
use qfs_codec::JsonCodec;
use qfs_runtime::SharedApplier;

fn config(contract: bool) -> RestApiConfig {
    let mut config = RestApiConfig::new(
        "https://example.test/api",
        vec![ResourceMap::new(
            "messages",
            vec![RestVerb::Select, RestVerb::Insert],
        )],
    );
    if contract {
        config.response_contract = Some(JsonResponseContract {
            success_field: "ok".into(),
            error_field: "error".into(),
        });
    }
    config
}

fn driver(config: RestApiConfig, mock: Arc<MockHttpClient>) -> RestDriver {
    RestDriver::new(
        config,
        Arc::new(JsonCodec),
        mock,
        Arc::new(qfs_secrets::InMemoryStore::new()),
    )
}

fn post() -> EffectNode {
    EffectNode::new(
        NodeId(0),
        EffectKind::Insert,
        Target::new(DriverId::new("rest"), VfsPath::new("/rest/test/messages")),
    )
    .with_args(crate::http_body_args(&Value::Text("hello".into())))
}

#[test]
fn application_rejections_and_invalid_envelopes_cannot_count_as_success() {
    for (body, code) in [
        (
            r#"{"ok":false,"error":"missing_post_type"}"#,
            "missing_post_type",
        ),
        (r#"{"ok":false,"error":"missing_scope"}"#, "missing_scope"),
        (r#"{"ok":false,"error":"no_text"}"#, "no_text"),
        (
            r#"{"ok":false,"error":"channel_not_found"}"#,
            "channel_not_found",
        ),
        (r#"{"ok":false,"error":"ratelimited"}"#, "ratelimited"),
        (
            r#"{"ok":false,"error":"SECRET-DO-NOT-ECHO"}"#,
            "service_rejected",
        ),
        (r#"{"ok":false,"error":{}}"#, "service_error_malformed"),
        (r#"{"ok":false}"#, "service_error_malformed"),
        (r#"{"ok":"true"}"#, "http_response_contract"),
        (r#"{}"#, "http_response_contract"),
        (r#"not json SECRET-DO-NOT-ECHO"#, "http_response_contract"),
    ] {
        let mock = Arc::new(
            MockHttpClient::new().with_response(HttpResponse::new(200, body.as_bytes().to_vec())),
        );
        let d = driver(config(true), mock.clone());
        let err = d.rest_applier().apply_shared(&post()).unwrap_err();
        let rendered = format!("{err:?}");
        assert!(rendered.contains(code), "{rendered}");
        assert!(!rendered.contains("SECRET-DO-NOT-ECHO"));
        assert_eq!(mock.recorded().len(), 1);
    }
}

#[test]
fn unknown_errors_keep_safe_operation_and_guidance_without_echoing_data() {
    // Construct synthetic credential-shaped data without checking in a token literal.
    let secret_like = ["xoxb", "SECRET-DO-NOT-ECHO"].join("-");
    for error in [
        "SECRET_IDENTIFIER",
        secret_like.as_str(),
        "novel_api_error",
    ] {
        let mock = Arc::new(MockHttpClient::new().with_response(HttpResponse::new(200,
            serde_json::to_vec(&serde_json::json!({"ok":false,"error":error,"response_metadata":{"messages":["PRIVATE_BODY"]}})).unwrap())));
        let d = driver(config(true), mock);
        let mut effect = post();
        effect.target.path = VfsPath::new("/rest/test/chat.postMessage?token=PRIVATE_QUERY");
        let err = d.rest_applier().apply_shared(&effect).unwrap_err();
        let rendered = format!("{err:?}");
        assert!(
            rendered.contains("chat.postMessage")
                && rendered.contains("unrecognized upstream error code withheld"),
            "{rendered}"
        );
        for private in [error, "PRIVATE_BODY", "PRIVATE_QUERY", "hello"] {
            assert!(!rendered.contains(private), "{rendered}");
        }
    }
}

#[test]
fn content_contract_rejects_missing_null_or_empty_before_credentials_or_http() {
    for body in [
        serde_json::json!({}),
        serde_json::json!({"text":null}),
        serde_json::json!({"text":" "}),
        serde_json::json!({"text":42}),
    ] {
        let mock = Arc::new(MockHttpClient::new());
        let mut cfg = config(true);
        cfg.auth = AuthStrategy::Bearer {
            secret_ref: crate::config::SecretRef::new("slack", "missing"),
        };
        cfg.request_contracts.push(JsonRequestContract {
            resource: "messages".into(),
            nonempty_any: vec!["text".into(), "blocks".into(), "attachments".into()],
        });
        let d = driver(cfg, mock.clone());
        let mut effect = post();
        effect.args = crate::http_body_args(&Value::Null);
        effect.args.rows[0].values[0] = Value::Bytes(serde_json::to_vec(&body).unwrap());
        let err = d.rest_applier().apply_shared(&effect).unwrap_err();
        assert!(format!("{err:?}").contains("explicit input column bindings"));
        assert!(mock.recorded().is_empty());
    }
    for body in [
        serde_json::json!({"text":"hello"}),
        serde_json::json!({"text":null,"blocks":[{"type":"divider"}]}),
        serde_json::json!({"attachments":[{"text":"hello"}]}),
    ] {
        let mock = Arc::new(
            MockHttpClient::new().with_response(HttpResponse::new(200, br#"{"ok":true}"#.to_vec())),
        );
        let mut cfg = config(true);
        cfg.request_contracts.push(JsonRequestContract {
            resource: "messages".into(),
            nonempty_any: vec!["text".into(), "blocks".into(), "attachments".into()],
        });
        let d = driver(cfg, mock.clone());
        let mut effect = post();
        effect.args = crate::http_body_args(&Value::Null);
        effect.args.rows[0].values[0] = Value::Bytes(serde_json::to_vec(&body).unwrap());
        assert!(d.rest_applier().apply_shared(&effect).is_ok());
        assert_eq!(mock.recorded().len(), 1);
    }
}

#[test]
fn json_writes_and_read_over_post_carry_their_content_type() {
    let mock = Arc::new(MockHttpClient::new());
    for _ in 0..2 {
        mock.push_response(HttpResponse::new(200, br#"{"ok":true}"#.to_vec()));
    }
    let d = driver(config(true), mock.clone());
    assert_eq!(d.rest_applier().apply_shared(&post()).unwrap().affected, 1);
    rest_read_rows_post(
        d.rest_applier(),
        "/rest/test/messages",
        &Value::Text("hello".into()),
    )
    .unwrap();
    for request in mock.recorded() {
        assert_eq!(request.method, HttpMethod::Post);
        assert_eq!(
            request.header_value("content-type"),
            Some("application/json")
        );
        assert_eq!(request.body.as_deref(), Some(br#""hello""#.as_slice()));
    }
}

#[test]
fn generic_rest_preserves_business_data_and_empty_success() {
    for (status, body, count) in [
        (200, br#"{"ok":false}"#.as_slice(), 1),
        (204, b"".as_slice(), 0),
    ] {
        let mock =
            Arc::new(MockHttpClient::new().with_response(HttpResponse::new(status, body.to_vec())));
        let d = driver(config(false), mock);
        assert_eq!(
            d.rest_applier().apply_shared(&post()).unwrap().affected,
            count
        );
    }
    let mock = Arc::new(MockHttpClient::new().with_response(HttpResponse::new(204, vec![])));
    assert!(driver(config(true), mock)
        .rest_applier()
        .apply_shared(&post())
        .is_err());
}

#[test]
fn pagination_rejects_a_failed_page_instead_of_returning_partial_rows() {
    let mock = Arc::new(MockHttpClient::new());
    mock.push_response(HttpResponse::new(
        200,
        br#"{"ok":true,"next":"page2"}"#.to_vec(),
    ));
    mock.push_response(HttpResponse::new(
        200,
        br#"{"ok":false,"error":"missing_scope","next":"page3"}"#.to_vec(),
    ));
    let config = config(true).with_pagination(Pagination::Cursor {
        next_field: "next".into(),
        param: "cursor".into(),
        max_pages: 10,
    });
    let d = driver(config, mock.clone());
    let err = rest_read_rows(d.rest_applier(), "/rest/test/messages").unwrap_err();
    assert_eq!(err.code(), "missing_scope");
    assert_eq!(mock.recorded().len(), 2);
}
