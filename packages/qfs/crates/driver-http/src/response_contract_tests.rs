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
        (
            r#"{"ok":false,"error":"channel_not_found"}"#,
            "channel_not_found",
        ),
        (r#"{"ok":false,"error":"ratelimited"}"#, "ratelimited"),
        (
            r#"{"ok":false,"error":"SECRET-DO-NOT-ECHO"}"#,
            "service_rejected",
        ),
        (r#"{"ok":false,"error":{}}"#, "service_rejected"),
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
