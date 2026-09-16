//! Slack's scoped file read: resolve an ID with the selected account before downloading bytes.
//! This is a provider-specific transport primitive, never an authenticated generic FOLLOW.

use super::*;
use qfs_types::{Column, ColumnType, Row, Schema, Value};

fn failure(code: &'static str) -> HttpError {
    HttpError::Application { code }
}

impl RestApplier {
    /// Read a Slack file by ID with this applier's selected account. Both exchanges refuse all
    /// redirects; only `https://files.slack.com/files-pri/…` returned by `files.info` is admitted.
    /// The result uses the ordinary blob `content: bytes` convention.
    ///
    /// # Errors
    /// Fails closed for other API bases, non-bearer auth, invalid IDs, inaccessible metadata,
    /// untrusted download URLs, redirects, and failed downloads. Response text is never an error.
    pub fn slack_file_content(&self, file_id: &str) -> Result<RowBatch, HttpError> {
        if self.config.base_url.trim_end_matches('/') != "https://slack.com/api"
            || !matches!(
                self.config.auth,
                AuthStrategy::Bearer { .. } | AuthStrategy::Account { .. }
            )
        {
            return Err(failure("slack_file_requires_authenticated_slack_mount"));
        }
        if !file_id.starts_with('F')
            || file_id.len() < 2
            || !file_id.bytes().all(|b| b.is_ascii_alphanumeric())
        {
            return Err(failure("slack_file_invalid_id"));
        }
        let metadata_url = format!("https://slack.com/api/files.info?file={file_id}");
        let request = self.inject_auth(HttpRequest::new(HttpMethod::Get, metadata_url))?;
        let metadata = self.client.send_without_redirects(&request)?;
        require_success(&metadata)?;
        let metadata: serde_json::Value = serde_json::from_slice(&metadata.body)
            .map_err(|_| failure("slack_file_invalid_metadata"))?;
        if metadata.get("ok").and_then(serde_json::Value::as_bool) != Some(true) {
            return Err(failure(
                match metadata.get("error").and_then(serde_json::Value::as_str) {
                    Some("file_not_found") => "slack_file_not_found",
                    Some("missing_scope") => "slack_file_missing_scope",
                    Some("invalid_auth" | "token_revoked" | "not_authed") => {
                        "slack_file_auth_failed"
                    }
                    _ => "slack_file_access_denied",
                },
            ));
        }
        let file = &metadata["file"];
        if file["id"].as_str() != Some(file_id) {
            return Err(failure("slack_file_invalid_metadata"));
        }
        let url = file["url_private_download"]
            .as_str()
            .filter(|s| !s.is_empty())
            .or_else(|| file["url_private"].as_str())
            .ok_or_else(|| failure("slack_file_download_unavailable"))?;
        validate_download_url(url)?;
        let request = self.inject_auth(HttpRequest::new(HttpMethod::Get, url.to_owned()))?;
        let content = self.client.send_without_redirects(&request)?;
        require_success(&content)?;
        Ok(RowBatch::new(
            Schema::new(vec![Column::new("content", ColumnType::Bytes, false)]),
            vec![Row::new(vec![Value::Bytes(content.body)])],
        ))
    }
}

fn require_success(response: &HttpResponse) -> Result<(), HttpError> {
    match response.status {
        200..=299 => Ok(()),
        300..=399 => Err(failure("slack_file_redirect_refused")),
        401 | 403 => Err(failure("slack_file_access_denied")),
        404 => Err(failure("slack_file_not_found")),
        _ => Err(failure("slack_file_download_failed")),
    }
}

fn validate_download_url(value: &str) -> Result<(), HttpError> {
    let url = reqwest::Url::parse(value).map_err(|_| failure("slack_file_untrusted_url"))?;
    if url.scheme() != "https"
        || url.host_str() != Some("files.slack.com")
        || url.port_or_known_default() != Some(443)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || !url.path().starts_with("/files-pri/")
        || value.contains('\\')
        || value.chars().any(char::is_control)
    {
        return Err(failure("slack_file_untrusted_url"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{MockHttpClient, SecretRef};
    use qfs_secrets::{InMemoryStore, Secret};

    const PRIVATE: &str = "https://files.slack.com/files-pri/T1-F1/download/report.pdf";
    const PDF: &[u8] = b"%PDF-1.7\n\0\xff\xfe\n%%EOF";

    fn applier(mock: Arc<dyn crate::HttpClient>) -> RestApplier {
        let key = SecretRef::new("slack", "selected");
        let secrets = Arc::new(InMemoryStore::new());
        secrets
            .put(
                &key.credential_key().unwrap(),
                Secret::from("selected-token"),
            )
            .unwrap();
        RestApplier::new(
            Arc::new(
                RestApiConfig::new("https://slack.com/api", vec![])
                    .with_auth(AuthStrategy::Bearer { secret_ref: key }),
            ),
            Arc::new(qfs_codec::JsonCodec),
            mock,
            secrets,
        )
    }

    fn info(mock: &MockHttpClient, url: &str) {
        mock.push_response(HttpResponse::new(
            200,
            serde_json::to_vec(&serde_json::json!({
                "ok":true,"file":{"id":"F1","url_private":url}
            }))
            .unwrap(),
        ));
    }

    #[test]
    fn slack_content_preserves_binary_bytes_and_never_changes_account() {
        let mock = Arc::new(MockHttpClient::new());
        info(&mock, PRIVATE);
        mock.push_response(HttpResponse::new(200, PDF.to_vec()));
        let result = applier(mock.clone()).slack_file_content("F1").unwrap();
        assert_eq!(result.rows[0].values, vec![Value::Bytes(PDF.to_vec())]);
        let requests = mock.recorded();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].url, "https://slack.com/api/files.info?file=F1");
        assert_eq!(requests[1].url, PRIVATE);
        for request in requests {
            assert_eq!(
                request.header_value("Authorization"),
                Some("Bearer selected-token")
            );
            assert!(!format!("{request:?}").contains("selected-token"));
        }
    }

    #[test]
    fn slack_content_rejects_untrusted_urls_before_download() {
        for url in [
            "https://evil.test/files-pri/T1-F1/a.pdf",
            "https://files.slack.com.evil.test/files-pri/a.pdf",
            "http://files.slack.com/files-pri/a.pdf",
            "https://files.slack.com:444/files-pri/a.pdf",
            "https://user:pass@files.slack.com/files-pri/a.pdf",
            "https://files.slack.com/other/a.pdf",
            "https://files.slack.com/files-pri/../other/a.pdf",
            "https://files.slack.com/files-pri/a.pdf#fragment",
            "https://files.slack.com\\@evil.test/files-pri/a.pdf",
        ] {
            let mock = Arc::new(MockHttpClient::new());
            info(&mock, url);
            let error = applier(mock.clone()).slack_file_content("F1").unwrap_err();
            assert_eq!(error.code(), "slack_file_untrusted_url", "{url}");
            assert_eq!(mock.recorded().len(), 1);
        }
    }

    #[test]
    fn slack_content_stops_on_missing_denied_and_malformed_metadata() {
        for (body, code) in [
            (
                r#"{"ok":false,"error":"file_not_found"}"#,
                "slack_file_not_found",
            ),
            (
                r#"{"ok":false,"error":"missing_scope"}"#,
                "slack_file_missing_scope",
            ),
            (
                r#"{"ok":false,"error":"secret-response-text"}"#,
                "slack_file_access_denied",
            ),
            (
                r#"{"ok":true,"file":{"id":"F2"}}"#,
                "slack_file_invalid_metadata",
            ),
            (
                r#"{"ok":true,"file":{"id":"F1"}}"#,
                "slack_file_download_unavailable",
            ),
            ("not json", "slack_file_invalid_metadata"),
        ] {
            let mock = Arc::new(MockHttpClient::new());
            mock.push_response(HttpResponse::new(200, body.as_bytes().to_vec()));
            let error = applier(mock.clone()).slack_file_content("F1").unwrap_err();
            assert_eq!(error.code(), code);
            assert!(!error.to_string().contains("secret-response-text"));
            assert_eq!(mock.recorded().len(), 1);
        }
    }

    #[test]
    fn slack_content_refuses_redirects_on_both_legs_and_download_denials() {
        for metadata_redirect in [true, false] {
            let mock = Arc::new(MockHttpClient::new());
            if !metadata_redirect {
                info(&mock, PRIVATE);
            }
            let mut redirect = HttpResponse::new(302, Vec::new());
            redirect
                .headers
                .push(("Location".into(), "https://evil.test/steal".into()));
            mock.push_response(redirect);
            assert_eq!(
                applier(mock.clone())
                    .slack_file_content("F1")
                    .unwrap_err()
                    .code(),
                "slack_file_redirect_refused"
            );
            assert_eq!(mock.recorded().len(), if metadata_redirect { 1 } else { 2 });
        }
        for (status, code) in [
            (403, "slack_file_access_denied"),
            (404, "slack_file_not_found"),
        ] {
            let mock = Arc::new(MockHttpClient::new());
            info(&mock, PRIVATE);
            mock.push_response(HttpResponse::new(status, b"secret-response-text".to_vec()));
            assert_eq!(
                applier(mock).slack_file_content("F1").unwrap_err().code(),
                code
            );
        }
    }

    #[test]
    fn slack_content_refuses_unscoped_credentials_and_invalid_ids_without_http() {
        for base in [
            "https://evil.test/api",
            "http://slack.com/api",
            "https://slack.com/other",
        ] {
            let mock = Arc::new(MockHttpClient::new());
            let mut a = applier(mock.clone());
            Arc::make_mut(&mut a.config).base_url = base.into();
            assert_eq!(
                a.slack_file_content("F1").unwrap_err().code(),
                "slack_file_requires_authenticated_slack_mount"
            );
            assert!(mock.recorded().is_empty());
        }
        for id in [
            "",
            "F",
            "F1?url=evil",
            "F1/../../",
            "https://evil.test",
            "%46%31",
        ] {
            let mock = Arc::new(MockHttpClient::new());
            assert_eq!(
                applier(mock.clone())
                    .slack_file_content(id)
                    .unwrap_err()
                    .code(),
                "slack_file_invalid_id"
            );
            assert!(mock.recorded().is_empty());
        }
    }
}
