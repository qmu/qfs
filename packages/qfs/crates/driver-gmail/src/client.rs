//! [`GmailClient`] — the thin, **mockable** Gmail API seam (blueprint §11 no-heavy-SDK,
//! boundary B3), plus [`GoogleApiGmailClient`] (the real client over the t19
//! [`GoogleApiClient`]) and [`MockGmailClient`] (an in-memory fake for tests — no live Gmail,
//! no network).
//!
//! The trait trades **only** in owned, vendor-free DTOs ([`crate::schema::MailMessage`] etc.);
//! Gmail JSON never crosses it. The real impl builds an [`HttpRequest`] (no `Authorization`
//! header — the [`GoogleApiClient`] injects the bearer and refreshes on a 401), sends it, and
//! translates the response JSON into the owned DTOs. The token discipline is wholly inherited
//! from t19: the bearer lives behind a [`qfs_secrets::Secret`], is written only into a header the
//! redacting `HttpRequest` `Debug` hides, and is **never** logged or surfaced in a [`GmailError`].

use std::sync::{Arc, Mutex};

use qfs_google_auth::GoogleApiClient;
use qfs_http_core::{HttpMethod, HttpRequest, HttpResponse};

use crate::error::GmailError;
use crate::schema::{AttachmentMeta, MailMessage};

/// The Gmail API base URL for the authenticated user (`me`). Every op is a path under this.
const API_BASE: &str = "https://gmail.googleapis.com/gmail/v1/users/me";

/// The result of listing message ids (Gmail `messages.list` returns ids only — the detail
/// fetch is a separate per-id call, the N+1 the planner collapses into parallel leaves).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MessageIdPage {
    /// The message ids on this page (ids only — details fetched separately).
    pub ids: Vec<String>,
    /// The next-page token, if there are further pages.
    pub next_page_token: Option<String>,
}

/// A draft's two ids (Gmail `drafts.list`/`drafts.get` return both): the **draft id** (the
/// `drafts.send`/`drafts.update` key, and the identity `/mail/drafts` lists and `/mail/drafts/<id>`
/// addresses) and the **message id** (the message inside the draft, used to fetch its detail). They
/// are distinct strings — sending needs the draft id, so the drafts surface exposes it, never the
/// message id.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DraftRef {
    /// The Gmail draft id (the sendable identity).
    pub id: String,
    /// The id of the message contained in the draft (for the detail fetch).
    pub message_id: String,
}

/// The thin Gmail API seam. A driver issues every Gmail call through this; the real impl rides
/// the t19 [`GoogleApiClient`] (bearer + refresh-on-401), the test impl answers from in-memory
/// fixtures. `Send + Sync` so an `Arc<dyn GmailClient>` can be shared across the runtime's
/// blocking apply threads.
pub trait GmailClient: Send + Sync {
    /// List label ids for the account (the `/mail` root listing → directories).
    ///
    /// # Errors
    /// [`GmailError`] on a non-2xx status, a decode failure, or an auth/transport failure.
    fn list_labels(&self) -> Result<Vec<String>, GmailError>;

    /// Search/list message ids matching `query` (the Gmail `q=`), capped at `max_results`.
    /// Returns ids only — the planner fans the detail fetch into independent leaves.
    ///
    /// # Errors
    /// [`GmailError`] on a non-2xx status, a decode failure, or an auth/transport failure.
    fn search_message_ids(
        &self,
        query: &str,
        max_results: Option<u32>,
    ) -> Result<MessageIdPage, GmailError>;

    /// List the account's drafts matching `query` (the Gmail `q=`), capped at `max_results`.
    /// Returns each draft's `(draft id, message id)` pair (`drafts.list`) — the draft id is the
    /// sendable identity, the message id feeds the per-draft [`get_message`](Self::get_message)
    /// detail fetch. Distinct from [`search_message_ids`](Self::search_message_ids), which returns
    /// message ids that cannot be sent.
    ///
    /// # Errors
    /// [`GmailError`] on a non-2xx status, a decode failure, or an auth/transport failure.
    fn list_drafts(
        &self,
        query: &str,
        max_results: Option<u32>,
    ) -> Result<Vec<DraftRef>, GmailError>;

    /// Resolve one draft id to its `(draft id, message id)` pair (`drafts.get`, minimal) — the
    /// single-node `/mail/drafts/<id>` read consults this, then fetches the message detail.
    ///
    /// # Errors
    /// [`GmailError`] on a non-2xx status, a decode failure, or an auth/transport failure.
    fn get_draft(&self, draft_id: &str) -> Result<DraftRef, GmailError>;

    /// Fetch one message's metadata/detail → the owned [`MailMessage`] DTO.
    ///
    /// # Errors
    /// [`GmailError`] on a non-2xx status, a decode failure, or an auth/transport failure.
    fn get_message(&self, id: &str) -> Result<MailMessage, GmailError>;

    /// Download and decode one attachment's raw bytes (`messages.{message_id}.attachments.
    /// {attachment_id}.get`, gmail-ftp `get id:att:<msg>:<att>`). The base64url `data` is decoded
    /// here so no vendor encoding crosses the client seam (the caller pairs these bytes with the
    /// message's [`AttachmentMeta`](crate::schema::AttachmentMeta) for `filename`/`mime`).
    ///
    /// # Errors
    /// [`GmailError`] on a non-2xx status, a response missing/undecodable `data`, or a transport
    /// failure.
    fn get_attachment(&self, message_id: &str, attachment_id: &str) -> Result<Vec<u8>, GmailError>;

    /// Create a draft from a base64url-encoded RFC 5322 `raw` message; returns the new draft id.
    /// `thread_id` is `Some` when the draft is a **reply** (it joins that Gmail thread via the API
    /// `message.threadId`), `None` for a standalone draft.
    ///
    /// # Errors
    /// [`GmailError`] on a non-2xx status, a decode failure, or an auth/transport failure.
    fn create_draft(
        &self,
        raw_base64url: &str,
        thread_id: Option<&str>,
    ) -> Result<String, GmailError>;

    /// Create-or-replace a draft by id from a base64url `raw` message (the retry-safe `UPSERT`).
    /// Returns the draft id (the same `id` on replace). `thread_id` threads a reply draft (as for
    /// [`create_draft`](Self::create_draft)).
    ///
    /// # Errors
    /// [`GmailError`] on a non-2xx status, a decode failure, or an auth/transport failure.
    fn upsert_draft(
        &self,
        id: &str,
        raw_base64url: &str,
        thread_id: Option<&str>,
    ) -> Result<String, GmailError>;

    /// Send a previously-created draft by id (the de-dupe-keyed one-shot send — a retry
    /// re-sends the *same* draft id rather than a fresh message). Returns the sent message id.
    ///
    /// # Errors
    /// [`GmailError`] on a non-2xx status, a decode failure, or an auth/transport failure.
    fn send_draft(&self, draft_id: &str) -> Result<String, GmailError>;

    /// Trash a single message by id (the `gmail.modify` trash op — **not** permanent delete).
    ///
    /// # Errors
    /// [`GmailError`] on a non-2xx status or an auth/transport failure.
    fn trash_message(&self, id: &str) -> Result<(), GmailError>;

    /// Trash a whole thread by id (`threads.trash`).
    ///
    /// # Errors
    /// [`GmailError`] on a non-2xx status or an auth/transport failure.
    fn trash_thread(&self, id: &str) -> Result<(), GmailError>;

    /// Modify a message's labels (`messages.modify`): add `add` ids, remove `remove` ids.
    ///
    /// # Errors
    /// [`GmailError`] on a non-2xx status or an auth/transport failure.
    fn modify_labels(&self, id: &str, add: &[String], remove: &[String]) -> Result<(), GmailError>;

    /// Create a new user label (`labels.create`, gmail-ftp `mkdir`); returns the new label id.
    ///
    /// # Errors
    /// [`GmailError`] on a non-2xx status, a decode failure, or an auth/transport failure.
    fn create_label(&self, name: &str) -> Result<String, GmailError>;
}

/// The real Gmail client: builds owned [`HttpRequest`]s and sends them through the t19
/// [`GoogleApiClient`], which injects the per-account bearer and refreshes on a 401. The
/// account selection is wholly upstream (the `GoogleApiClient` is constructed per account from
/// a [`qfs_google_auth::TokenSource`]); this client is account-agnostic.
pub struct GoogleApiGmailClient {
    api: Arc<GoogleApiClient>,
}

impl GoogleApiGmailClient {
    /// Build a Gmail client over an authenticated [`GoogleApiClient`] (one per account).
    #[must_use]
    pub fn new(api: Arc<GoogleApiClient>) -> Self {
        Self { api }
    }

    /// Send a request through the auth client, mapping its [`AuthError`](qfs_google_auth::AuthError)
    /// to a secret-free [`GmailError`] and classifying a non-2xx status under `op`.
    fn send(&self, op: &'static str, req: &HttpRequest) -> Result<HttpResponse, GmailError> {
        let resp = self.api.send(req).map_err(GmailError::from)?;
        if resp.is_success() {
            Ok(resp)
        } else {
            Err(GmailError::Api {
                op,
                status: resp.status,
            })
        }
    }

    /// Parse a response body as JSON, mapping a failure to a secret-free decode error.
    fn parse_json(op: &'static str, resp: &HttpResponse) -> Result<serde_json::Value, GmailError> {
        serde_json::from_slice(&resp.body).map_err(|_| GmailError::Decode {
            op,
            reason: "response body was not valid JSON".to_string(),
        })
    }

    /// A JSON `POST` to a Gmail API path (body is `Content-Type: application/json`).
    fn post_json(
        &self,
        op: &'static str,
        path: &str,
        body: serde_json::Value,
    ) -> Result<HttpResponse, GmailError> {
        let bytes = serde_json::to_vec(&body).map_err(|_| GmailError::Decode {
            op,
            reason: "could not encode the request body".to_string(),
        })?;
        let req = HttpRequest::new(HttpMethod::Post, format!("{API_BASE}{path}"))
            .header("Content-Type", "application/json")
            .with_body(bytes);
        self.send(op, &req)
    }
}

impl GmailClient for GoogleApiGmailClient {
    fn list_labels(&self) -> Result<Vec<String>, GmailError> {
        let op = "labels.list";
        let req = HttpRequest::new(HttpMethod::Get, format!("{API_BASE}/labels"));
        let resp = self.send(op, &req)?;
        let json = Self::parse_json(op, &resp)?;
        Ok(decode_label_names(&json))
    }

    fn search_message_ids(
        &self,
        query: &str,
        max_results: Option<u32>,
    ) -> Result<MessageIdPage, GmailError> {
        let op = "messages.list";
        let mut url = format!("{API_BASE}/messages");
        let mut params: Vec<String> = Vec::new();
        if !query.is_empty() {
            params.push(format!("q={}", encode_query(query)));
        }
        if let Some(n) = max_results {
            params.push(format!("maxResults={n}"));
        }
        if !params.is_empty() {
            url.push('?');
            url.push_str(&params.join("&"));
        }
        let req = HttpRequest::new(HttpMethod::Get, url);
        let resp = self.send(op, &req)?;
        let json = Self::parse_json(op, &resp)?;
        let ids = json
            .get("messages")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|m| m.get("id").and_then(|v| v.as_str()).map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        let next_page_token = json
            .get("nextPageToken")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        Ok(MessageIdPage {
            ids,
            next_page_token,
        })
    }

    fn list_drafts(
        &self,
        query: &str,
        max_results: Option<u32>,
    ) -> Result<Vec<DraftRef>, GmailError> {
        let op = "drafts.list";
        let mut url = format!("{API_BASE}/drafts");
        let mut params: Vec<String> = Vec::new();
        if !query.is_empty() {
            params.push(format!("q={}", encode_query(query)));
        }
        if let Some(n) = max_results {
            params.push(format!("maxResults={n}"));
        }
        if !params.is_empty() {
            url.push('?');
            url.push_str(&params.join("&"));
        }
        let req = HttpRequest::new(HttpMethod::Get, url);
        let resp = self.send(op, &req)?;
        let json = Self::parse_json(op, &resp)?;
        Ok(json
            .get("drafts")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(decode_draft_ref).collect())
            .unwrap_or_default())
    }

    fn get_draft(&self, draft_id: &str) -> Result<DraftRef, GmailError> {
        let op = "drafts.get";
        // `format=minimal`: we only need the id pair here; the message detail is a separate
        // `get_message` call, so the heavy `format=full` fetch is not paid twice.
        let req = HttpRequest::new(
            HttpMethod::Get,
            format!("{API_BASE}/drafts/{draft_id}?format=minimal"),
        );
        let resp = self.send(op, &req)?;
        let json = Self::parse_json(op, &resp)?;
        decode_draft_ref(&json).ok_or(GmailError::Decode {
            op,
            reason: "drafts.get response missing draft/message id".to_string(),
        })
    }

    fn get_message(&self, id: &str) -> Result<MailMessage, GmailError> {
        let op = "messages.get";
        // `format=full` (not `metadata`): the metadata format omits `payload.parts`, so
        // `decode_attachments` saw no parts and every message/draft read back `attachments: []`.
        // The full format carries the part tree (filename + body.attachmentId/size) without the
        // attachment BYTES — those are fetched on demand via `get_attachment` (attachments.get).
        let req = HttpRequest::new(
            HttpMethod::Get,
            format!("{API_BASE}/messages/{id}?format=full"),
        );
        let resp = self.send(op, &req)?;
        let json = Self::parse_json(op, &resp)?;
        decode_message(&json).ok_or(GmailError::Decode {
            op,
            reason: "message JSON missing required fields".to_string(),
        })
    }

    fn get_attachment(&self, message_id: &str, attachment_id: &str) -> Result<Vec<u8>, GmailError> {
        let op = "attachments.get";
        let req = HttpRequest::new(
            HttpMethod::Get,
            format!("{API_BASE}/messages/{message_id}/attachments/{attachment_id}"),
        );
        let resp = self.send(op, &req)?;
        let json = Self::parse_json(op, &resp)?;
        let data = json
            .get("data")
            .and_then(|v| v.as_str())
            .ok_or(GmailError::Decode {
                op,
                reason: "attachment JSON missing `data`".to_string(),
            })?;
        crate::mime::decode_base64url(data).ok_or(GmailError::Decode {
            op,
            reason: "attachment `data` is not valid base64url".to_string(),
        })
    }

    fn create_draft(
        &self,
        raw_base64url: &str,
        thread_id: Option<&str>,
    ) -> Result<String, GmailError> {
        let op = "drafts.create";
        let mut message = serde_json::json!({ "raw": raw_base64url });
        if let Some(tid) = thread_id {
            message["threadId"] = serde_json::Value::String(tid.to_string());
        }
        let body = serde_json::json!({ "message": message });
        let resp = self.post_json(op, "/drafts", body)?;
        let json = Self::parse_json(op, &resp)?;
        json.get("id")
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .ok_or(GmailError::Decode {
                op,
                reason: "drafts.create response missing draft id".to_string(),
            })
    }

    fn upsert_draft(
        &self,
        id: &str,
        raw_base64url: &str,
        thread_id: Option<&str>,
    ) -> Result<String, GmailError> {
        let op = "drafts.update";
        let mut message = serde_json::json!({ "raw": raw_base64url });
        if let Some(tid) = thread_id {
            message["threadId"] = serde_json::Value::String(tid.to_string());
        }
        let bytes =
            serde_json::to_vec(&serde_json::json!({ "message": message })).map_err(|_| {
                GmailError::Decode {
                    op,
                    reason: "could not encode the draft body".to_string(),
                }
            })?;
        // PUT is the idempotent (retry-safe) create-or-replace by id.
        let req = HttpRequest::new(HttpMethod::Put, format!("{API_BASE}/drafts/{id}"))
            .header("Content-Type", "application/json")
            .with_body(bytes);
        let resp = self.send(op, &req)?;
        let json = Self::parse_json(op, &resp)?;
        Ok(json
            .get("id")
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .unwrap_or_else(|| id.to_string()))
    }

    fn send_draft(&self, draft_id: &str) -> Result<String, GmailError> {
        let op = "drafts.send";
        let body = serde_json::json!({ "id": draft_id });
        let resp = self.post_json(op, "/drafts/send", body)?;
        let json = Self::parse_json(op, &resp)?;
        json.get("id")
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .ok_or(GmailError::Decode {
                op,
                reason: "drafts.send response missing sent message id".to_string(),
            })
    }

    fn trash_message(&self, id: &str) -> Result<(), GmailError> {
        self.post_json(
            "messages.trash",
            &format!("/messages/{id}/trash"),
            serde_json::json!({}),
        )?;
        Ok(())
    }

    fn trash_thread(&self, id: &str) -> Result<(), GmailError> {
        self.post_json(
            "threads.trash",
            &format!("/threads/{id}/trash"),
            serde_json::json!({}),
        )?;
        Ok(())
    }

    fn modify_labels(&self, id: &str, add: &[String], remove: &[String]) -> Result<(), GmailError> {
        let body = serde_json::json!({
            "addLabelIds": add,
            "removeLabelIds": remove,
        });
        self.post_json("messages.modify", &format!("/messages/{id}/modify"), body)?;
        Ok(())
    }

    fn create_label(&self, name: &str) -> Result<String, GmailError> {
        let op = "labels.create";
        let body = serde_json::json!({ "name": name });
        let resp = self.post_json(op, "/labels", body)?;
        let json = Self::parse_json(op, &resp)?;
        json.get("id")
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .ok_or(GmailError::Decode {
                op,
                reason: "labels.create response missing label id".to_string(),
            })
    }
}

/// Translate one Gmail draft JSON object (`{ id, message: { id } }`, from `drafts.list` /
/// `drafts.get`) into the owned [`DraftRef`]. Returns `None` if either id is absent.
fn decode_draft_ref(json: &serde_json::Value) -> Option<DraftRef> {
    let id = json.get("id")?.as_str()?.to_string();
    let message_id = json
        .get("message")
        .and_then(|m| m.get("id"))
        .and_then(|v| v.as_str())?
        .to_string();
    Some(DraftRef { id, message_id })
}

/// Translate one Gmail `messages.get?format=full` JSON object into the owned [`MailMessage`].
/// Returns `None` if the required `id`/`threadId` are absent. The full format (not `metadata`)
/// is required so `payload.parts` is present for [`decode_attachments`].
fn decode_message(json: &serde_json::Value) -> Option<MailMessage> {
    let id = json.get("id")?.as_str()?.to_string();
    let thread_id = json.get("threadId")?.as_str()?.to_string();
    let snippet = json
        .get("snippet")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let label_ids = json
        .get("labelIds")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|l| l.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    let date = json
        .get("internalDate")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(0);

    // Headers live under payload.headers as [{name, value}].
    let headers = json
        .get("payload")
        .and_then(|p| p.get("headers"))
        .and_then(|h| h.as_array());
    let header = |name: &str| -> String {
        headers
            .and_then(|hs| {
                hs.iter().find(|h| {
                    h.get("name")
                        .and_then(|v| v.as_str())
                        .is_some_and(|n| n.eq_ignore_ascii_case(name))
                })
            })
            .and_then(|h| h.get("value").and_then(|v| v.as_str()))
            .unwrap_or_default()
            .to_string()
    };

    let attachments = decode_attachments(json);

    Some(MailMessage {
        id,
        thread_id,
        label_ids,
        date,
        from: header("From"),
        subject: header("Subject"),
        snippet,
        // The RFC 5322 `Message-Id` header — the value a reply threads from (`In-Reply-To`/
        // `References`). Absent on some messages; empty then (a reply to such a message fails
        // closed at the applier rather than emitting a bare header).
        message_id: header("Message-Id"),
        attachments,
    })
}

/// The user-facing label NAMES from a `labels.list` response — the display names a mailbox-root
/// listing (`/mail |> select name`) shows and `/mail/<name>` reads back. Gmail keys a label by an
/// opaque `id` (`Label_5` for a user label) but carries a separate `name` (`Acme様`); the listing
/// must surface `name`, not the id (a system label like `INBOX` has `id == name`, so it is
/// unaffected). Reading the `id` here was the "labels list as raw ids" bug.
fn decode_label_names(json: &serde_json::Value) -> Vec<String> {
    json.get("labels")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|l| l.get("name").and_then(|v| v.as_str()).map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// Collect attachment metadata from a message payload's parts (a part with a `filename` and an
/// `attachmentId` is an attachment). Bytes are not fetched here — only metadata for the row.
fn decode_attachments(json: &serde_json::Value) -> Vec<AttachmentMeta> {
    let Some(parts) = json
        .get("payload")
        .and_then(|p| p.get("parts"))
        .and_then(|v| v.as_array())
    else {
        return Vec::new();
    };
    parts
        .iter()
        .filter_map(|part| {
            let filename = part.get("filename").and_then(|v| v.as_str())?;
            if filename.is_empty() {
                return None;
            }
            let mime = part
                .get("mimeType")
                .and_then(|v| v.as_str())
                .unwrap_or("application/octet-stream")
                .to_string();
            let body = part.get("body");
            let attachment_id = body
                .and_then(|b| b.get("attachmentId"))
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            let size = body
                .and_then(|b| b.get("size"))
                .and_then(serde_json::Value::as_i64)
                .unwrap_or(0);
            Some(AttachmentMeta {
                filename: filename.to_string(),
                mime,
                attachment_id,
                size,
            })
        })
        .collect()
}

/// Minimal percent-encoding for a `q=` query value (encode chars that break a query string).
/// Dependency-free; covers the Gmail search operators (`:`, spaces, quotes).
fn encode_query(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for b in value.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

/// An in-memory mock Gmail client (tests / CI / wasm): answers from pre-seeded fixtures and
/// **records** every call so a test asserts the exact API surface the driver exercised — with
/// **no socket and no credentials**. The recorded calls also prove `PREVIEW` performs zero I/O
/// (the mock asserts it was never called) and that a write goes to the expected op.
#[derive(Default)]
pub struct MockGmailClient {
    labels: Vec<String>,
    messages: Vec<MailMessage>,
    /// Seeded drafts (`draft id` + `message id`); the drafts read lists these and joins each to its
    /// seeded message by `message_id`.
    drafts: Vec<DraftRef>,
    /// Seeded attachment bytes, keyed by `(message_id, attachment_id)`.
    attachments: Vec<(String, String, Vec<u8>)>,
    search_ids: Mutex<Vec<MessageIdPage>>,
    recorded: Mutex<Vec<RecordedCall>>,
}

/// One recorded Gmail API call (the op + its salient owned arguments) — what a test asserts the
/// driver issued. Secret-free by construction (no token ever enters this seam).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum RecordedCall {
    /// `labels.list`.
    ListLabels,
    /// `messages.list` with the pushed `q=` query + the cap.
    Search {
        /// The Gmail `q=` search string the driver pushed down.
        query: String,
        /// The `maxResults` cap, if any.
        max_results: Option<u32>,
    },
    /// `messages.get` for one id (the N+1 detail leaf).
    GetMessage {
        /// The message id fetched.
        id: String,
    },
    /// `messages.{id}.attachments.{attId}.get` — an attachment bytes download.
    GetAttachment {
        /// The owning message id.
        message_id: String,
        /// The attachment id fetched.
        attachment_id: String,
    },
    /// `drafts.list` with the pushed `q=` query + the cap.
    ListDrafts {
        /// The Gmail `q=` search string the driver pushed down.
        query: String,
        /// The `maxResults` cap, if any.
        max_results: Option<u32>,
    },
    /// `drafts.get` for one draft id.
    GetDraft {
        /// The draft id resolved.
        draft_id: String,
    },
    /// `drafts.create` (carries the base64url raw so a test can decode it).
    CreateDraft {
        /// The base64url `raw` message.
        raw: String,
        /// The Gmail thread id the draft joins (a reply), or `None` for a standalone draft.
        thread_id: Option<String>,
    },
    /// `drafts.update` (the retry-safe upsert by id).
    UpsertDraft {
        /// The draft id replaced.
        id: String,
        /// The base64url `raw` message.
        raw: String,
        /// The Gmail thread id the draft joins (a reply), or `None` for a standalone draft.
        thread_id: Option<String>,
    },
    /// `drafts.send` for a draft id.
    SendDraft {
        /// The draft id sent.
        draft_id: String,
    },
    /// `messages.trash` for a message id.
    TrashMessage {
        /// The trashed message id.
        id: String,
    },
    /// `threads.trash` for a thread id.
    TrashThread {
        /// The trashed thread id.
        id: String,
    },
    /// `messages.modify` (label add/remove).
    ModifyLabels {
        /// The message id modified.
        id: String,
        /// The label ids added.
        add: Vec<String>,
        /// The label ids removed.
        remove: Vec<String>,
    },
    /// `labels.create` — a new user label.
    CreateLabel {
        /// The new label's name.
        name: String,
    },
}

impl MockGmailClient {
    /// An empty mock.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Seed the label ids the root listing returns.
    #[must_use]
    pub fn with_labels(mut self, labels: Vec<String>) -> Self {
        self.labels = labels;
        self
    }

    /// Seed a message returned by `get_message` (matched by id).
    #[must_use]
    pub fn with_message(mut self, message: MailMessage) -> Self {
        self.messages.push(message);
        self
    }

    /// Seed a draft (`draft id` + `message id`) returned by `list_drafts`/`get_draft`. Pair it with
    /// a [`with_message`](Self::with_message) whose id is `message_id` so the drafts read has detail
    /// to join.
    #[must_use]
    pub fn with_draft(mut self, draft_id: &str, message_id: &str) -> Self {
        self.drafts.push(DraftRef {
            id: draft_id.to_string(),
            message_id: message_id.to_string(),
        });
        self
    }

    /// Seed the raw bytes `get_attachment` returns for a `(message_id, attachment_id)` pair.
    #[must_use]
    pub fn with_attachment(
        mut self,
        message_id: &str,
        attachment_id: &str,
        bytes: Vec<u8>,
    ) -> Self {
        self.attachments
            .push((message_id.to_string(), attachment_id.to_string(), bytes));
        self
    }

    /// Queue one message-id page returned (FIFO) by `search_message_ids`.
    #[must_use]
    pub fn with_search_page(self, page: MessageIdPage) -> Self {
        if let Ok(mut q) = self.search_ids.lock() {
            q.push(page);
        }
        self
    }

    /// Seed (post-construction, `&self`) a message-id page returned by the next search. The
    /// `query` argument documents the q= the test expects to push (it is not matched on; the
    /// recorded call carries the actual query for assertion).
    pub fn search_ids_seed(&self, _query: &str, ids: Vec<String>) {
        if let Ok(mut q) = self.search_ids.lock() {
            q.push(MessageIdPage {
                ids,
                next_page_token: None,
            });
        }
    }

    /// The calls this mock received, in order — what a test asserts against.
    #[must_use]
    pub fn recorded(&self) -> Vec<RecordedCall> {
        self.recorded.lock().map(|r| r.clone()).unwrap_or_default()
    }

    fn record(&self, call: RecordedCall) {
        if let Ok(mut r) = self.recorded.lock() {
            r.push(call);
        }
    }
}

impl GmailClient for MockGmailClient {
    fn list_labels(&self) -> Result<Vec<String>, GmailError> {
        self.record(RecordedCall::ListLabels);
        Ok(self.labels.clone())
    }

    fn search_message_ids(
        &self,
        query: &str,
        max_results: Option<u32>,
    ) -> Result<MessageIdPage, GmailError> {
        self.record(RecordedCall::Search {
            query: query.to_string(),
            max_results,
        });
        let page = self
            .search_ids
            .lock()
            .ok()
            .and_then(|mut q| {
                if q.is_empty() {
                    None
                } else {
                    Some(q.remove(0))
                }
            })
            .unwrap_or_default();
        Ok(page)
    }

    fn list_drafts(
        &self,
        query: &str,
        max_results: Option<u32>,
    ) -> Result<Vec<DraftRef>, GmailError> {
        self.record(RecordedCall::ListDrafts {
            query: query.to_string(),
            max_results,
        });
        Ok(self.drafts.clone())
    }

    fn get_draft(&self, draft_id: &str) -> Result<DraftRef, GmailError> {
        self.record(RecordedCall::GetDraft {
            draft_id: draft_id.to_string(),
        });
        self.drafts
            .iter()
            .find(|d| d.id == draft_id)
            .cloned()
            .ok_or(GmailError::Api {
                op: "drafts.get",
                status: 404,
            })
    }

    fn get_message(&self, id: &str) -> Result<MailMessage, GmailError> {
        self.record(RecordedCall::GetMessage { id: id.to_string() });
        self.messages
            .iter()
            .find(|m| m.id == id)
            .cloned()
            .ok_or(GmailError::Api {
                op: "messages.get",
                status: 404,
            })
    }

    fn get_attachment(&self, message_id: &str, attachment_id: &str) -> Result<Vec<u8>, GmailError> {
        self.record(RecordedCall::GetAttachment {
            message_id: message_id.to_string(),
            attachment_id: attachment_id.to_string(),
        });
        self.attachments
            .iter()
            .find(|(m, a, _)| m == message_id && a == attachment_id)
            .map(|(_, _, bytes)| bytes.clone())
            .ok_or(GmailError::Api {
                op: "attachments.get",
                status: 404,
            })
    }

    fn create_draft(
        &self,
        raw_base64url: &str,
        thread_id: Option<&str>,
    ) -> Result<String, GmailError> {
        self.record(RecordedCall::CreateDraft {
            raw: raw_base64url.to_string(),
            thread_id: thread_id.map(str::to_string),
        });
        Ok("draft-new".to_string())
    }

    fn upsert_draft(
        &self,
        id: &str,
        raw_base64url: &str,
        thread_id: Option<&str>,
    ) -> Result<String, GmailError> {
        self.record(RecordedCall::UpsertDraft {
            id: id.to_string(),
            raw: raw_base64url.to_string(),
            thread_id: thread_id.map(str::to_string),
        });
        Ok(id.to_string())
    }

    fn send_draft(&self, draft_id: &str) -> Result<String, GmailError> {
        self.record(RecordedCall::SendDraft {
            draft_id: draft_id.to_string(),
        });
        Ok("sent-msg".to_string())
    }

    fn trash_message(&self, id: &str) -> Result<(), GmailError> {
        self.record(RecordedCall::TrashMessage { id: id.to_string() });
        Ok(())
    }

    fn trash_thread(&self, id: &str) -> Result<(), GmailError> {
        self.record(RecordedCall::TrashThread { id: id.to_string() });
        Ok(())
    }

    fn modify_labels(&self, id: &str, add: &[String], remove: &[String]) -> Result<(), GmailError> {
        self.record(RecordedCall::ModifyLabels {
            id: id.to_string(),
            add: add.to_vec(),
            remove: remove.to_vec(),
        });
        Ok(())
    }

    fn create_label(&self, name: &str) -> Result<String, GmailError> {
        self.record(RecordedCall::CreateLabel {
            name: name.to_string(),
        });
        Ok(format!("Label_{name}"))
    }
}

#[cfg(test)]
mod parse_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn label_listing_surfaces_display_names_not_raw_ids() {
        // labels.list carries an opaque `id` AND a display `name`: a user label's id is `Label_5`
        // while its name is `Acme様`; a system label has `id == name`. The mailbox-root listing must
        // surface the name (reading the id was the "labels list as raw ids" bug).
        let json = json!({
            "labels": [
                { "id": "INBOX", "name": "INBOX", "type": "system" },
                { "id": "Label_5", "name": "Acme様", "type": "user" },
            ]
        });
        assert_eq!(
            decode_label_names(&json),
            vec!["INBOX".to_string(), "Acme様".to_string()],
            "user labels surface their display name, not the raw Label_N id"
        );
    }

    #[test]
    fn full_format_payload_parts_yield_attachment_metadata() {
        // A `format=full` message carries `payload.parts`; an attachment part has a non-empty
        // filename and a `body.attachmentId`/`size`. `format=metadata` omits parts (the
        // read-back-`[]` bug), so this asserts the extraction once the parts are present.
        let json = json!({
            "id": "m1",
            "threadId": "t1",
            "payload": {
                "headers": [ { "name": "Subject", "value": "hi" } ],
                "parts": [
                    { "mimeType": "text/plain", "filename": "", "body": { "size": 5 } },
                    { "mimeType": "text/plain", "filename": "hello.txt",
                      "body": { "attachmentId": "att-9", "size": 5 } }
                ]
            }
        });
        let atts = decode_attachments(&json);
        assert_eq!(
            atts.len(),
            1,
            "only the part carrying a filename is an attachment"
        );
        assert_eq!(atts[0].filename, "hello.txt");
        assert_eq!(atts[0].attachment_id, "att-9");
        assert_eq!(atts[0].size, 5);

        // End-to-end: the attachment rides on the decoded MailMessage row.
        let msg = decode_message(&json).expect("message decodes");
        assert_eq!(msg.attachments.len(), 1);
        assert_eq!(msg.attachments[0].filename, "hello.txt");
    }

    #[test]
    fn payload_without_parts_yields_no_attachments() {
        // The `format=metadata` shape (no `payload.parts`) → empty, not a panic — this documents
        // WHY the fetch must be `format=full`.
        let json = json!({
            "id": "m1", "threadId": "t1",
            "payload": { "headers": [ { "name": "Subject", "value": "hi" } ] }
        });
        assert!(decode_attachments(&json).is_empty());
    }
}
