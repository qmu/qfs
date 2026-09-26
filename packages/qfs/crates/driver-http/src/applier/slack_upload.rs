//! Slack's scoped file upload: reserve an upload URL, send the bytes, complete the share.
//!
//! The mirror image of [`super::slack_file`]. Slack retired the single-call `files.upload`, and
//! its replacement is a three-call external flow — `files.getUploadURLExternal` returns a
//! one-shot URL on `files.slack.com`, the bytes go there, and `files.completeUploadExternal`
//! turns the reserved id into a file shared into a channel. A declared map is ONE request, so the
//! flow lives here as a provider-specific transport primitive, confined the same way the download
//! is: the account's credential reaches `slack.com/api` and the URL Slack itself delivered, every
//! redirect is refused, and no caller-supplied URL is ever admitted.

use super::*;
use qfs_types::{Column, ColumnType, Row, Schema, Value};

/// The resource segment a declared map addresses this primitive by.
pub(super) const RESOURCE: &str = "qfs.file-upload";

fn failure(code: &'static str) -> HttpError {
    let hint = match code {
        "slack_upload_requires_authenticated_slack_mount" => {
            "Use an authenticated Slack mount for the selected account."
        }
        "slack_upload_missing_filename" => {
            "Address the upload with a file name: `upsert into /slack/<ws>/<channel>/files/<name>`."
        }
        "slack_upload_missing_channel" => {
            "Name the destination channel in the path; it resolves by name or by id."
        }
        "slack_upload_empty_content" => {
            "Write a row carrying `content` (bytes) — an empty upload is refused."
        }
        "slack_upload_invalid_row" => {
            "Write one row carrying filename, channel and content; check the map's VALUES."
        }
        "slack_upload_missing_scope" => {
            "Grant files:write to the selected Slack account and reconnect."
        }
        "slack_upload_auth_failed" => "Reconnect the selected Slack account.",
        "slack_upload_access_denied" => {
            "Check the selected account's membership of the destination channel."
        }
        "slack_upload_channel_unresolved" => {
            "Address the destination by its channel ID; Slack's complete-upload call does not take \
             a name. Find the id in the channels or private-channels view."
        }
        "slack_upload_url_unavailable" => {
            "Slack reserved no upload URL for this file; retry, and check the file size."
        }
        "slack_upload_untrusted_url" | "slack_upload_redirect_refused" => {
            "Slack must deliver a direct https://files.slack.com/ upload URL; redirects are refused."
        }
        "slack_upload_rejected" => "Slack refused the bytes; retry the upload.",
        "slack_upload_incomplete" => {
            "Slack accepted the bytes but did not complete the share; check files:write and the channel."
        }
        _ => "Check Slack service availability and retry the upload.",
    };
    HttpError::Application {
        code,
        operation: "slack.files.upload",
        hint,
    }
}

/// One evaluated upload: what the declared map's wire body named.
struct Upload {
    filename: String,
    channel: String,
    content: Vec<u8>,
    title: Option<String>,
    comment: Option<String>,
}

impl RestApplier {
    /// Upload `body`'s bytes to Slack and share them into its named channel, with this applier's
    /// selected account. Three exchanges, all redirect-free: reserve, send, complete.
    ///
    /// The wire body is the declared map's evaluated struct, JSON-encoded by the shipped write
    /// path (`{filename, channel, content, title?, comment?}`), where `content` is the byte array
    /// `value_to_json` renders a `Value::Bytes` as.
    ///
    /// # Errors
    /// Fails closed for other API bases, non-bearer auth, a body that is not one upload, an
    /// untrusted or redirecting upload URL, and any leg Slack refuses. Response text is never an
    /// error message.
    pub fn slack_file_upload(&self, body: Option<&[u8]>) -> Result<RowBatch, HttpError> {
        if self.config.base_url.trim_end_matches('/') != "https://slack.com/api"
            || !matches!(
                self.config.auth,
                AuthStrategy::Bearer { .. } | AuthStrategy::Account { .. }
            )
        {
            return Err(failure("slack_upload_requires_authenticated_slack_mount"));
        }
        let upload = parse_upload(body)?;

        // 1. Reserve. Slack wants the file's own byte length here, so it is taken from the bytes
        //    themselves rather than from anything the caller asserted.
        let reserve = format!(
            "https://slack.com/api/files.getUploadURLExternal?filename={}&length={}",
            urlencode(&upload.filename),
            upload.content.len()
        );
        let request = self.inject_auth(HttpRequest::new(HttpMethod::Get, reserve))?;
        let reserved = self.client.send_without_redirects(&request)?;
        let reserved = ok_envelope(&reserved)?;
        let upload_url = reserved
            .get("upload_url")
            .and_then(serde_json::Value::as_str)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| failure("slack_upload_url_unavailable"))?;
        let file_id = reserved
            .get("file_id")
            .and_then(serde_json::Value::as_str)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| failure("slack_upload_url_unavailable"))?
            .to_string();
        validate_upload_url(upload_url)?;

        // 2. Send the bytes to the URL Slack delivered. Same host class and same account as the
        //    private download, so the credential goes where the download's already goes.
        let request = self
            .inject_auth(HttpRequest::new(HttpMethod::Post, upload_url.to_owned()))?
            .header("Content-Type", "application/octet-stream")
            .with_body(upload.content);
        let sent = self.client.send_without_redirects(&request)?;
        require_upload_success(&sent)?;

        // 3. Complete the share. Only now does the file exist for the channel's readers.
        let mut file = serde_json::Map::new();
        file.insert("id".to_string(), serde_json::Value::String(file_id.clone()));
        if let Some(title) = &upload.title {
            file.insert(
                "title".to_string(),
                serde_json::Value::String(title.clone()),
            );
        }
        let mut complete = serde_json::Map::new();
        complete.insert(
            "files".to_string(),
            serde_json::Value::Array(vec![serde_json::Value::Object(file)]),
        );
        complete.insert(
            "channel_id".to_string(),
            serde_json::Value::String(upload.channel.clone()),
        );
        if let Some(comment) = &upload.comment {
            complete.insert(
                "initial_comment".to_string(),
                serde_json::Value::String(comment.clone()),
            );
        }
        let payload = serde_json::to_vec(&serde_json::Value::Object(complete))
            .map_err(|_| failure("slack_upload_invalid_row"))?;
        let request = self
            .inject_auth(HttpRequest::new(
                HttpMethod::Post,
                "https://slack.com/api/files.completeUploadExternal".to_string(),
            ))?
            .header("Content-Type", "application/json")
            .with_body(payload);
        let completed = self.client.send_without_redirects(&request)?;
        let completed = ok_envelope(&completed)?;
        let shared = completed
            .get("files")
            .and_then(serde_json::Value::as_array)
            .and_then(|files| files.first())
            .ok_or_else(|| failure("slack_upload_incomplete"))?;
        let id = shared
            .get("id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or(&file_id)
            .to_string();
        let name = shared
            .get("name")
            .and_then(serde_json::Value::as_str)
            .unwrap_or(&upload.filename)
            .to_string();
        Ok(RowBatch::new(
            Schema::new(vec![
                Column::new("id", ColumnType::Text, false),
                Column::new("name", ColumnType::Text, false),
            ]),
            vec![Row::new(vec![Value::Text(id), Value::Text(name)])],
        ))
    }
}

/// Read the declared map's JSON wire body into one upload. Every field is validated here, before
/// a credential is resolved or a byte is sent.
fn parse_upload(body: Option<&[u8]>) -> Result<Upload, HttpError> {
    let body = body.ok_or_else(|| failure("slack_upload_invalid_row"))?;
    let json: serde_json::Value =
        serde_json::from_slice(body).map_err(|_| failure("slack_upload_invalid_row"))?;
    let text = |field: &str| -> Option<String> {
        json.get(field)
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    };
    let filename = text("filename").ok_or_else(|| failure("slack_upload_missing_filename"))?;
    let channel = text("channel").ok_or_else(|| failure("slack_upload_missing_channel"))?;
    // `value_to_json` renders `Value::Bytes` as an array of byte numbers; anything else in this
    // field is a map that did not bind the row's bytes, which is a refusal rather than a guess.
    let content = json
        .get("content")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| failure("slack_upload_empty_content"))?
        .iter()
        .map(|b| {
            b.as_u64()
                .filter(|n| *n <= u64::from(u8::MAX))
                .map(|n| n as u8)
                .ok_or_else(|| failure("slack_upload_invalid_row"))
        })
        .collect::<Result<Vec<u8>, HttpError>>()?;
    if content.is_empty() {
        return Err(failure("slack_upload_empty_content"));
    }
    Ok(Upload {
        filename,
        channel,
        content,
        title: text("title"),
        comment: text("comment"),
    })
}

/// A Slack `{ok: true, …}` envelope, or the mapped refusal. Slack reports application failures
/// with HTTP 200, so the status and the envelope are both checked.
fn ok_envelope(response: &HttpResponse) -> Result<serde_json::Value, HttpError> {
    match response.status {
        200..=299 => {}
        300..=399 => return Err(failure("slack_upload_redirect_refused")),
        401 | 403 => return Err(failure("slack_upload_access_denied")),
        _ => return Err(failure("slack_upload_rejected")),
    }
    let json: serde_json::Value =
        serde_json::from_slice(&response.body).map_err(|_| failure("slack_upload_rejected"))?;
    if json.get("ok").and_then(serde_json::Value::as_bool) != Some(true) {
        return Err(failure(
            match json.get("error").and_then(serde_json::Value::as_str) {
                Some("missing_scope" | "not_allowed_token_type") => "slack_upload_missing_scope",
                Some("invalid_auth" | "token_revoked" | "not_authed") => "slack_upload_auth_failed",
                // `channel_not_found` is what Slack answers for a NAME as much as for a channel
                // the token cannot see, and the first is the likelier mistake — the path segment
                // is where a human writes `general`. Naming both beats naming neither.
                Some("channel_not_found") => "slack_upload_channel_unresolved",
                Some("not_in_channel" | "access_denied") => "slack_upload_access_denied",
                _ => "slack_upload_rejected",
            },
        ));
    }
    Ok(json)
}

/// The byte-sending leg answers with no envelope — only a status.
fn require_upload_success(response: &HttpResponse) -> Result<(), HttpError> {
    match response.status {
        200..=299 => Ok(()),
        300..=399 => Err(failure("slack_upload_redirect_refused")),
        401 | 403 => Err(failure("slack_upload_access_denied")),
        _ => Err(failure("slack_upload_rejected")),
    }
}

/// Admit only the direct `https://files.slack.com/…` upload URL Slack delivered — the same
/// discipline the private download applies to `url_private_download`.
fn validate_upload_url(value: &str) -> Result<(), HttpError> {
    let url = reqwest::Url::parse(value).map_err(|_| failure("slack_upload_untrusted_url"))?;
    if url.scheme() != "https"
        || url.host_str() != Some("files.slack.com")
        || url.port_or_known_default() != Some(443)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || value.contains('\\')
        || value.chars().any(char::is_control)
    {
        return Err(failure("slack_upload_untrusted_url"));
    }
    Ok(())
}

/// Percent-encode a file name for the reserve call's query string. Conservative by design: every
/// byte outside the unreserved set is escaped, so a name carrying `&`, a space or UTF-8 cannot
/// reshape the URL.
fn urlencode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*byte as char);
            }
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{MockHttpClient, SecretRef};
    use qfs_secrets::{InMemoryStore, Secret};

    const UPLOAD: &str = "https://files.slack.com/upload/v1/ABC123";
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

    /// The wire body the shipped declared write path produces for this map: `Value::Bytes`
    /// renders as an array of byte numbers (`value_to_json`), which is what the primitive reads.
    fn body(filename: &str, channel: &str, content: &[u8]) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "filename": filename,
            "channel": channel,
            "content": content.to_vec(),
        }))
        .unwrap()
    }

    fn reserve(mock: &MockHttpClient, url: &str) {
        mock.push_response(HttpResponse::new(
            200,
            serde_json::to_vec(&serde_json::json!({
                "ok": true, "upload_url": url, "file_id": "F1"
            }))
            .unwrap(),
        ));
    }

    fn completed(mock: &MockHttpClient) {
        mock.push_response(HttpResponse::new(
            200,
            serde_json::to_vec(&serde_json::json!({
                "ok": true, "files": [{"id": "F1", "name": "report.pdf"}]
            }))
            .unwrap(),
        ));
    }

    #[test]
    fn upload_runs_the_three_calls_with_the_selected_account() {
        let mock = Arc::new(MockHttpClient::new());
        reserve(&mock, UPLOAD);
        mock.push_response(HttpResponse::new(200, b"OK".to_vec()));
        completed(&mock);

        let rows = applier(mock.clone())
            .slack_file_upload(Some(&body("report.pdf", "C1", PDF)))
            .unwrap();
        assert_eq!(
            rows.rows[0].values,
            vec![
                Value::Text("F1".to_string()),
                Value::Text("report.pdf".to_string())
            ]
        );

        let requests = mock.recorded();
        assert_eq!(requests.len(), 3, "reserve, send, complete — nothing else");
        assert_eq!(
            requests[0].url,
            format!(
                "https://slack.com/api/files.getUploadURLExternal?filename=report.pdf&length={}",
                PDF.len()
            ),
            "the reserved length is the file's own byte length"
        );
        assert_eq!(requests[1].url, UPLOAD);
        assert_eq!(
            requests[1].body.as_deref(),
            Some(PDF),
            "the bytes go to the wire unchanged"
        );
        assert_eq!(
            requests[2].url,
            "https://slack.com/api/files.completeUploadExternal"
        );
        let complete: serde_json::Value =
            serde_json::from_slice(requests[2].body.as_deref().unwrap()).unwrap();
        assert_eq!(complete["channel_id"], "C1");
        assert_eq!(complete["files"][0]["id"], "F1");
        for request in requests {
            assert_eq!(
                request.header_value("Authorization"),
                Some("Bearer selected-token")
            );
            assert!(!format!("{request:?}").contains("selected-token"));
        }
    }

    #[test]
    fn upload_escapes_the_filename_into_the_reserve_query() {
        let mock = Arc::new(MockHttpClient::new());
        reserve(&mock, UPLOAD);
        mock.push_response(HttpResponse::new(200, b"OK".to_vec()));
        completed(&mock);
        applier(mock.clone())
            .slack_file_upload(Some(&body("a b&length=9&x=報告.pdf", "C1", PDF)))
            .unwrap();
        let reserved = &mock.recorded()[0].url;
        assert!(
            reserved.starts_with(
                "https://slack.com/api/files.getUploadURLExternal?filename=a%20b%26length%3D9"
            ),
            "a name cannot reshape the query: {reserved}"
        );
        assert!(
            reserved.ends_with(&format!("&length={}", PDF.len())),
            "the real length is the last parameter: {reserved}"
        );
    }

    #[test]
    fn upload_refuses_an_untrusted_url_before_sending_bytes() {
        for url in [
            "https://evil.test/upload/v1/A",
            "https://files.slack.com.evil.test/upload/v1/A",
            "http://files.slack.com/upload/v1/A",
            "https://files.slack.com:444/upload/v1/A",
            "https://user:pass@files.slack.com/upload/v1/A",
            "https://files.slack.com/upload/v1/A#fragment",
            "https://files.slack.com\\@evil.test/upload/v1/A",
        ] {
            let mock = Arc::new(MockHttpClient::new());
            reserve(&mock, url);
            let error = applier(mock.clone())
                .slack_file_upload(Some(&body("report.pdf", "C1", PDF)))
                .unwrap_err();
            assert_eq!(error.code(), "slack_upload_untrusted_url", "{url}");
            assert_eq!(
                mock.recorded().len(),
                1,
                "only the reserve call was made for {url}"
            );
        }
    }

    #[test]
    fn upload_maps_each_slack_refusal_onto_its_own_code() {
        for (error, expected) in [
            ("missing_scope", "slack_upload_missing_scope"),
            ("invalid_auth", "slack_upload_auth_failed"),
            ("not_in_channel", "slack_upload_access_denied"),
            ("channel_not_found", "slack_upload_channel_unresolved"),
            ("something_else", "slack_upload_rejected"),
        ] {
            let mock = Arc::new(MockHttpClient::new());
            mock.push_response(HttpResponse::new(
                200,
                serde_json::to_vec(&serde_json::json!({"ok": false, "error": error})).unwrap(),
            ));
            let failed = applier(mock.clone())
                .slack_file_upload(Some(&body("report.pdf", "C1", PDF)))
                .unwrap_err();
            assert_eq!(failed.code(), expected);
            assert_eq!(mock.recorded().len(), 1, "no bytes were sent for {error}");
        }
    }

    #[test]
    fn upload_refuses_an_unusable_row_before_any_request() {
        for (body, expected) in [
            (
                serde_json::json!({"channel": "C1", "content": [1, 2]}),
                "slack_upload_missing_filename",
            ),
            (
                serde_json::json!({"filename": "a.pdf", "content": [1, 2]}),
                "slack_upload_missing_channel",
            ),
            (
                serde_json::json!({"filename": "a.pdf", "channel": "C1", "content": []}),
                "slack_upload_empty_content",
            ),
            (
                serde_json::json!({"filename": "a.pdf", "channel": "C1"}),
                "slack_upload_empty_content",
            ),
            (
                serde_json::json!({"filename": "a.pdf", "channel": "C1", "content": "not-bytes"}),
                "slack_upload_empty_content",
            ),
        ] {
            let mock = Arc::new(MockHttpClient::new());
            let failed = applier(mock.clone())
                .slack_file_upload(Some(&serde_json::to_vec(&body).unwrap()))
                .unwrap_err();
            assert_eq!(failed.code(), expected);
            assert!(mock.recorded().is_empty(), "nothing was sent for {body}");
        }
    }

    #[test]
    fn upload_refuses_a_foreign_mount() {
        let mock = Arc::new(MockHttpClient::new());
        let other = RestApplier::new(
            Arc::new(RestApiConfig::new("https://evil.test/api", vec![])),
            Arc::new(qfs_codec::JsonCodec),
            mock.clone(),
            Arc::new(InMemoryStore::new()),
        );
        let failed = other
            .slack_file_upload(Some(&body("report.pdf", "C1", PDF)))
            .unwrap_err();
        assert_eq!(
            failed.code(),
            "slack_upload_requires_authenticated_slack_mount"
        );
        assert!(mock.recorded().is_empty());
    }
}
