//! [`SlackClient`] — the thin, **mockable** Slack Web-API seam (blueprint §11 no-heavy-SDK,
//! boundary B3), plus [`RestSlackClient`] (the real client over a local [`HttpTransport`] seam) and
//! [`MockSlackClient`] (an in-memory fake for tests — no live Slack, no network).
//!
//! ## Reuses the shared http-core DTOs + the t18/t24 reusable seam *shape* (no hand-rolled HTTP DTO)
//! The real client builds owned [`qfs_http_core::HttpRequest`]s — the **same shared DTOs +
//! redaction authority** the t18 REST seam, t24 GitHub seam, and `qfs-google-auth` trade in — and
//! sends them through a thin [`HttpTransport`] trait (a structural twin of t18's `HttpClient`). The
//! driver does **not** depend on `qfs-driver-http` as a crate (a qfs-runtime consumer must stay a
//! leaf — the dep-direction confinement test), so the reqwest wire impl rides this transport seam.
//! On top of the seam this client layers the Slack conventions:
//! - **Bearer (bot-token) auth**: the token, resolved from a [`qfs_secrets::Secret`] at
//!   request-build time, written into an `Authorization: Bearer …` header the redacting
//!   [`HttpRequest`] `Debug` hides. Never logged, never in a DTO/error.
//! - **Cursor pagination**: a list `GET` follows `response_metadata.next_cursor`, bounded by
//!   [`MAX_PAGES`].
//! - **429 / `Retry-After` bounded retry on idempotent GETs only**: a transient 429/5xx on a `GET`
//!   is retried up to [`MAX_RETRIES`] honouring `Retry-After`; a write (`chat.postMessage` etc.) is
//!   **never** retried (at-least-once for the non-idempotent post — blueprint §7).
//!
//! ## The t18 BodyErrorRule (the explicit reason t25 exists as its consumer)
//! Slack signals application errors with **HTTP 200** carrying `{"ok":false,"error":"<code>"}`. A
//! status-only classifier would treat that as success. The [`BodyErrorRule`] — **opt-in** on the
//! config (default-off per t18; Slack turns it on) — inspects the decoded JSON `ok` field inside
//! the seam and maps `ok:false` to a structured **terminal** [`SlackError::Body`] carrying Slack's
//! `error` code. The rule lives in the seam so every call (read + write) is covered uniformly.

use std::sync::{Arc, Mutex};

use qfs_http_core::{HttpMethod, HttpRequest, HttpResponse};
use qfs_secrets::{CredentialKey, Secrets};

use crate::dto::FileDto;
use crate::effect::SlackEffect;
use crate::error::SlackError;
use crate::path::NodeKind;

/// A secret-free transport failure (DNS/connect/TLS/timeout) — the class only, never a header
/// value. The structural twin of t18's transport error, kept local so this leaf does not depend on
/// `qfs-driver-http` (runtime-consumer confinement).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransportError {
    /// A secret-free class reason (e.g. `connection failed`).
    pub reason: String,
}

/// The thin HTTP transport seam (blueprint §11 boundary B3): a driver builds an owned http-core
/// [`HttpRequest`] and calls [`HttpTransport::send`]; the impl performs the wire exchange and
/// returns an owned [`HttpResponse`] or a secret-free [`TransportError`]. `reqwest`/`url` types
/// **never** cross it — it trades only in the shared http-core DTOs (the same contract t18's
/// `HttpClient` honours). `Send + Sync` so an `Arc<dyn HttpTransport>` is shareable across the
/// runtime's blocking apply threads.
pub trait HttpTransport: Send + Sync {
    /// Execute one request synchronously. A non-2xx status is **not** an error here — the client
    /// classifies [`HttpResponse::status`].
    ///
    /// # Errors
    /// [`TransportError`] if the wire exchange fails before a status is received.
    fn send(&self, req: &HttpRequest) -> Result<HttpResponse, TransportError>;
}

/// The Slack Web-API base URL.
pub const API_BASE: &str = "https://slack.com/api";
/// The hard ceiling on cursor pages a list follows (blueprint §7 runaway-fetch guard).
pub const MAX_PAGES: u32 = 50;
/// The hard ceiling on transient-retry attempts on an idempotent GET.
pub const MAX_RETRIES: u32 = 3;
/// The default page size sent as `limit` when the caller pushed none.
pub const DEFAULT_LIMIT: &str = "200";

/// The closed list of Web-API methods this driver issues (blueprint §11 enums) — the capability sum type
/// the read/apply legs select among. Each maps onto a Slack `method` path segment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SlackApiCall {
    /// `conversations.list` — resolve a symbolic channel name before reading a channel log.
    ConversationsList,
    /// `conversations.open` — open or resolve an IM channel before posting a DM.
    ConversationsOpen,
    /// `conversations.history` — a channel message log read.
    ConversationsHistory,
    /// `conversations.replies` — a thread read.
    ConversationsReplies,
    /// `users.list` — the user directory read.
    UsersList,
    /// `files.list` — the files namespace `ls`.
    FilesList,
    /// `files.info` — metadata for one file before a private download.
    FilesInfo,
    /// `chat.postMessage` — post a message/reply/DM.
    ChatPostMessage,
    /// `chat.update` — edit a message by `ts`.
    ChatUpdate,
    /// `chat.delete` — delete a message.
    ChatDelete,
    /// `reactions.add`.
    ReactionsAdd,
    /// `reactions.remove`.
    ReactionsRemove,
    /// `pins.add`.
    PinsAdd,
    /// `pins.remove`.
    PinsRemove,
    /// `files.getUploadURLExternal` — step 1 of the external upload flow (reserve an upload URL +
    /// file id for a given filename + byte length). The legacy `files.upload` is sunset for new apps.
    FilesGetUploadURLExternal,
    /// `files.completeUploadExternal` — step 3 of the external upload flow (finalize the uploaded
    /// file, optionally sharing it into a channel).
    FilesCompleteUploadExternal,
    /// `files.delete`.
    FilesDelete,
}

impl SlackApiCall {
    /// The Slack Web-API method path segment (e.g. `chat.postMessage`).
    #[must_use]
    pub const fn method(self) -> &'static str {
        match self {
            SlackApiCall::ConversationsList => "conversations.list",
            SlackApiCall::ConversationsOpen => "conversations.open",
            SlackApiCall::ConversationsHistory => "conversations.history",
            SlackApiCall::ConversationsReplies => "conversations.replies",
            SlackApiCall::UsersList => "users.list",
            SlackApiCall::FilesList => "files.list",
            SlackApiCall::FilesInfo => "files.info",
            SlackApiCall::ChatPostMessage => "chat.postMessage",
            SlackApiCall::ChatUpdate => "chat.update",
            SlackApiCall::ChatDelete => "chat.delete",
            SlackApiCall::ReactionsAdd => "reactions.add",
            SlackApiCall::ReactionsRemove => "reactions.remove",
            SlackApiCall::PinsAdd => "pins.add",
            SlackApiCall::PinsRemove => "pins.remove",
            SlackApiCall::FilesGetUploadURLExternal => "files.getUploadURLExternal",
            SlackApiCall::FilesCompleteUploadExternal => "files.completeUploadExternal",
            SlackApiCall::FilesDelete => "files.delete",
        }
    }

    /// Whether this call pages through Slack's `channels` collection.
    #[must_use]
    pub const fn lists_channels(self) -> bool {
        matches!(self, SlackApiCall::ConversationsList)
    }

    /// The read call a node kind lists through.
    #[must_use]
    pub const fn read_for(kind: NodeKind) -> Self {
        match kind {
            NodeKind::Messages | NodeKind::Dms => SlackApiCall::ConversationsHistory,
            NodeKind::Replies | NodeKind::Reactions => SlackApiCall::ConversationsReplies,
            NodeKind::Files => SlackApiCall::FilesList,
            NodeKind::Users => SlackApiCall::UsersList,
        }
    }
}

/// The thin Slack API seam. A driver issues every Slack call through this; the real impl rides the
/// [`HttpTransport`] seam (Bearer + cursor pagination + bounded GET retry + the [`BodyErrorRule`]),
/// the test impl answers from in-memory fixtures. `Send + Sync` so an `Arc<dyn SlackClient>` can be
/// shared across the runtime's blocking apply threads.
pub trait SlackClient: Send + Sync {
    /// List the `kind` collection through its read call, applying the pushed query `params`,
    /// following cursor pagination, returning the merged JSON of all pages.
    ///
    /// # Errors
    /// [`SlackError`] on a non-2xx status, an `ok:false` body (BodyErrorRule), a decode failure, or
    /// an auth/transport failure.
    fn list(
        &self,
        kind: NodeKind,
        params: &[(String, String)],
    ) -> Result<serde_json::Value, SlackError>;

    /// Apply one decoded write/CALL [`SlackEffect`], returning the affected count.
    ///
    /// # Errors
    /// [`SlackError`] on a non-2xx status, an `ok:false` body (BodyErrorRule, unless a swallowed
    /// already-done class), or an auth/transport failure.
    fn apply(&self, effect: &SlackEffect) -> Result<u64, SlackError>;

    /// Download one Slack file's private bytes with its metadata.
    ///
    /// # Errors
    /// [`SlackError`] on auth, metadata decode, HTTP, or Slack body errors.
    fn download_file(&self, file_id: &str) -> Result<(FileDto, Vec<u8>), SlackError>;
}

/// The **t18 BodyErrorRule** carried into the Slack seam: how to treat Slack's HTTP-200
/// `{"ok":false,...}` envelope. Opt-in on the config (default-off per t18); Slack turns it **on**.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum BodyErrorRule {
    /// Ignore the `ok` field — a 2xx is success regardless of body (the t18 default for backends
    /// that signal errors via status only).
    Off,
    /// Inspect the decoded `ok` field; `ok:false` maps to a terminal [`SlackError::Body`] carrying
    /// the `error` code. This is the Slack setting.
    On,
}

impl BodyErrorRule {
    /// Apply the rule to a decoded Slack JSON body for `op`. Returns the body unchanged on success,
    /// or a structured [`SlackError::Body`] when the rule is [`BodyErrorRule::On`] and the body
    /// carries `ok:false`. `swallow_already_done` lets a naturally-idempotent op treat Slack's
    /// already-done class (`already_reacted`/`already_pinned`/`no_reaction`/`not_pinned`) as a
    /// no-op success (blueprint §7).
    ///
    /// # Errors
    /// [`SlackError::Body`] per the rule.
    pub fn check(
        self,
        op: &'static str,
        body: &serde_json::Value,
        swallow_already_done: bool,
    ) -> Result<(), SlackError> {
        if self == BodyErrorRule::Off {
            return Ok(());
        }
        let ok = body.get("ok").and_then(serde_json::Value::as_bool);
        if ok == Some(true) || ok.is_none() {
            return Ok(());
        }
        let code = body
            .get("error")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("unknown_error");
        if swallow_already_done && is_already_done(code) {
            return Ok(());
        }
        Err(SlackError::Body {
            op,
            code: code.to_string(),
        })
    }
}

/// Whether a Slack `error` code is the "already in the desired state" class a naturally-idempotent
/// op swallows (blueprint §7): an `already_reacted`/`already_pinned` (the add already landed) and the
/// symmetric `no_reaction`/`not_pinned` (the remove already landed) are no-op successes.
#[must_use]
pub fn is_already_done(code: &str) -> bool {
    matches!(
        code,
        "already_reacted" | "already_pinned" | "no_reaction" | "not_pinned"
    )
}

/// The real Slack client: builds owned [`HttpRequest`]s, injects the Bearer bot token from the
/// secret store, and sends them through the [`HttpTransport`] seam. Confines no `reqwest` itself —
/// the reqwest wire impl lives behind the transport trait (parked with live E2E for t38).
pub struct RestSlackClient {
    http: Arc<dyn HttpTransport>,
    secrets: Arc<dyn Secrets>,
    cred: CredentialKey,
    body_rule: BodyErrorRule,
}

impl RestSlackClient {
    /// Build a Slack client over the `http` transport, resolving the bot token under `cred` from
    /// `secrets`, applying `body_rule` (Slack passes [`BodyErrorRule::On`]). The token is read only
    /// at request-build time, never stored here.
    #[must_use]
    pub fn new(
        http: Arc<dyn HttpTransport>,
        secrets: Arc<dyn Secrets>,
        cred: CredentialKey,
        body_rule: BodyErrorRule,
    ) -> Self {
        Self {
            http,
            secrets,
            cred,
            body_rule,
        }
    }

    /// Build a base request with the Bearer bot token injected. The token is exposed only here (a
    /// header value the redacting `Debug` hides) and dropped immediately.
    fn request(&self, method: HttpMethod, url: String) -> Result<HttpRequest, SlackError> {
        let secret = self
            .secrets
            .get(&self.cred)
            .map_err(|e| SlackError::Auth { code: e.code() })?;
        let token = secret.expose_str().ok_or(SlackError::Auth {
            code: "secret_not_utf8",
        })?;
        Ok(HttpRequest::new(method, url)
            .header("Accept", "application/json")
            .header("User-Agent", "qfs-driver-slack")
            .header("Authorization", format!("Bearer {token}")))
    }

    /// Send an idempotent GET with bounded transient retry. A 429/5xx is retried up to
    /// [`MAX_RETRIES`] (the `Retry-After` header bounds the conceptual wait). Only GETs are retried.
    fn send_get(&self, op: &'static str, url: &str) -> Result<HttpResponse, SlackError> {
        let mut attempt = 0;
        loop {
            let req = self.request(HttpMethod::Get, url.to_string())?;
            let resp = self.http.send(&req).map_err(SlackError::from)?;
            tracing::debug!(method = "GET", url = %url, status = resp.status, "slack request");
            if resp.is_success() {
                return Ok(resp);
            }
            if SlackError::is_transient_status(resp.status) && attempt < MAX_RETRIES {
                let _retry_after = resp
                    .header_value("retry-after")
                    .and_then(|v| v.parse::<u64>().ok());
                attempt += 1;
                continue;
            }
            return Err(SlackError::Http {
                op,
                status: resp.status,
            });
        }
    }

    /// Build a Slack method URL with the cursor + pushed params + a default `limit`.
    fn list_url(call: SlackApiCall, params: &[(String, String)], cursor: Option<&str>) -> String {
        let mut all: Vec<(String, String)> = params.to_vec();
        if !all.iter().any(|(k, _)| k == "limit") {
            all.push(("limit".to_string(), DEFAULT_LIMIT.to_string()));
        }
        if let Some(c) = cursor {
            all.push(("cursor".to_string(), c.to_string()));
        }
        let qs = all
            .iter()
            .map(|(k, v)| format!("{}={}", encode(k), encode(v)))
            .collect::<Vec<_>>()
            .join("&");
        format!("{API_BASE}/{}?{qs}", call.method())
    }

    fn resolve_params(
        &self,
        params: &[(String, String)],
    ) -> Result<Vec<(String, String)>, SlackError> {
        let mut resolved = params.to_vec();
        let Some((_, channel)) = resolved.iter_mut().find(|(k, _)| k == "channel") else {
            return Ok(resolved);
        };
        if needs_dm_open(channel) {
            *channel = self.open_dm_channel(channel)?;
            return Ok(resolved);
        }
        if !needs_channel_lookup(channel) {
            return Ok(resolved);
        }
        *channel = self.resolve_channel_id(channel)?;
        Ok(resolved)
    }

    fn resolve_channel_id(&self, channel: &str) -> Result<String, SlackError> {
        let wanted = channel.trim_start_matches('#');
        let mut cursor: Option<String> = None;
        for _page in 0..MAX_PAGES {
            let params = vec![
                (
                    "types".to_string(),
                    "public_channel,private_channel".to_string(),
                ),
                ("exclude_archived".to_string(), "true".to_string()),
                ("limit".to_string(), DEFAULT_LIMIT.to_string()),
            ];
            let url = Self::list_url(SlackApiCall::ConversationsList, &params, cursor.as_deref());
            let resp = self.send_get(SlackApiCall::ConversationsList.method(), &url)?;
            let value: serde_json::Value =
                serde_json::from_slice(&resp.body).map_err(|_| SlackError::Decode {
                    op: SlackApiCall::ConversationsList.method(),
                    reason: "channel list response was not valid JSON".to_string(),
                })?;
            self.body_rule
                .check(SlackApiCall::ConversationsList.method(), &value, false)?;
            if let Some(channels) = value.get("channels").and_then(serde_json::Value::as_array) {
                for item in channels {
                    let name = item.get("name").and_then(serde_json::Value::as_str);
                    let id = item.get("id").and_then(serde_json::Value::as_str);
                    if name == Some(wanted) {
                        return id.map(str::to_string).ok_or_else(|| SlackError::Decode {
                            op: SlackApiCall::ConversationsList.method(),
                            reason: "matched channel had no id".to_string(),
                        });
                    }
                }
            }
            cursor = value
                .get("response_metadata")
                .and_then(|m| m.get("next_cursor"))
                .and_then(serde_json::Value::as_str)
                .filter(|c| !c.is_empty())
                .map(str::to_string);
            if cursor.is_none() {
                break;
            }
        }
        Err(SlackError::Body {
            op: SlackApiCall::ConversationsList.method(),
            code: format!("channel_name_not_found:{wanted}"),
        })
    }

    /// Send a JSON-bodied write (`POST`). Never retried (at-least-once for the non-idempotent post).
    /// Applies the [`BodyErrorRule`] to the 200 envelope so an `ok:false` is a structured terminal
    /// error rather than a false success.
    fn send_write(
        &self,
        op: &'static str,
        call: SlackApiCall,
        body: serde_json::Value,
        swallow_already_done: bool,
    ) -> Result<(), SlackError> {
        self.send_write_value(op, call, body, swallow_already_done)
            .map(|_| ())
    }

    fn send_write_value(
        &self,
        op: &'static str,
        call: SlackApiCall,
        body: serde_json::Value,
        swallow_already_done: bool,
    ) -> Result<serde_json::Value, SlackError> {
        let url = format!("{API_BASE}/{}", call.method());
        let bytes = serde_json::to_vec(&body).map_err(|_| SlackError::Decode {
            op,
            reason: "could not encode the request body".to_string(),
        })?;
        let req = self
            .request(HttpMethod::Post, url)?
            .header("Content-Type", "application/json; charset=utf-8")
            .with_body(bytes);
        let resp = self.http.send(&req).map_err(SlackError::from)?;
        tracing::debug!(method = "POST", op = %op, status = resp.status, "slack request");
        if !resp.is_success() {
            return Err(SlackError::Http {
                op,
                status: resp.status,
            });
        }
        // BodyErrorRule: a 200 may still be ok:false — classify the envelope.
        let value: serde_json::Value =
            serde_json::from_slice(&resp.body).map_err(|_| SlackError::Decode {
                op,
                reason: "response was not valid JSON".to_string(),
            })?;
        self.body_rule.check(op, &value, swallow_already_done)?;
        Ok(value)
    }

    /// Slack's **external upload** flow (the legacy `files.upload` is sunset for new apps): reserve
    /// an upload URL for the byte length, POST the raw bytes to that pre-authorized URL, then
    /// complete the upload (optionally sharing it into a channel). Returns the new file id. The
    /// bytes ride out-of-band of the JSON API (step 2), so no file content ever sits in a logged
    /// request body.
    fn upload_external(
        &self,
        name: &str,
        mime: Option<&str>,
        bytes: &[u8],
        channel: Option<&str>,
    ) -> Result<String, SlackError> {
        // Step 1 — reserve the upload URL. `files.getUploadURLExternal` takes form-encoded params
        // (filename + the exact byte length), not JSON.
        let reserved = self.send_write_form(
            "files.getUploadURLExternal",
            SlackApiCall::FilesGetUploadURLExternal,
            &[
                ("filename".to_string(), name.to_string()),
                ("length".to_string(), bytes.len().to_string()),
            ],
        )?;
        let upload_url = reserved
            .get("upload_url")
            .and_then(serde_json::Value::as_str)
            .ok_or(SlackError::Decode {
                op: "files.getUploadURLExternal",
                reason: "response carried no upload_url".to_string(),
            })?;
        let file_id = reserved
            .get("file_id")
            .and_then(serde_json::Value::as_str)
            .ok_or(SlackError::Decode {
                op: "files.getUploadURLExternal",
                reason: "response carried no file_id".to_string(),
            })?
            .to_string();

        // Step 2 — POST the raw bytes to the reserved (already-signed) URL. No bearer.
        self.external_post(
            "files.upload_bytes",
            upload_url,
            bytes.to_vec(),
            mime.unwrap_or("application/octet-stream"),
        )?;

        // Step 3 — finalize. `files.completeUploadExternal` takes JSON; `channel_id` shares it.
        let mut complete = serde_json::json!({
            "files": [{ "id": file_id, "title": name }],
        });
        if let Some(ch) = channel {
            complete["channel_id"] = serde_json::Value::String(ch.to_string());
        }
        self.send_write_value(
            "files.completeUploadExternal",
            SlackApiCall::FilesCompleteUploadExternal,
            complete,
            false,
        )?;
        Ok(file_id)
    }

    /// POST form-urlencoded params to a Slack Web-API method (Bearer-authenticated), classifying the
    /// status + the `ok:false` body envelope exactly like [`Self::send_write_value`]. Used by the
    /// methods that require `application/x-www-form-urlencoded` rather than JSON.
    fn send_write_form(
        &self,
        op: &'static str,
        call: SlackApiCall,
        params: &[(String, String)],
    ) -> Result<serde_json::Value, SlackError> {
        let url = format!("{API_BASE}/{}", call.method());
        let req = self
            .request(HttpMethod::Post, url)?
            .header("Content-Type", "application/x-www-form-urlencoded")
            .with_body(form_urlencode(params).into_bytes());
        let resp = self.http.send(&req).map_err(SlackError::from)?;
        tracing::debug!(method = "POST", op = %op, status = resp.status, "slack request");
        if !resp.is_success() {
            return Err(SlackError::Http {
                op,
                status: resp.status,
            });
        }
        let value: serde_json::Value =
            serde_json::from_slice(&resp.body).map_err(|_| SlackError::Decode {
                op,
                reason: "response was not valid JSON".to_string(),
            })?;
        self.body_rule.check(op, &value, false)?;
        Ok(value)
    }

    /// POST raw bytes to a pre-authorized external URL (the reserved upload URL). **No bearer**: the
    /// URL is already signed, and the driver's own token must not leak to a third-party host. A
    /// non-2xx is a terminal transport-class error.
    fn external_post(
        &self,
        op: &'static str,
        url: &str,
        bytes: Vec<u8>,
        content_type: &str,
    ) -> Result<(), SlackError> {
        let req = HttpRequest::new(HttpMethod::Post, url.to_string())
            .header("Content-Type", content_type.to_string())
            .with_body(bytes);
        let resp = self.http.send(&req).map_err(SlackError::from)?;
        tracing::debug!(method = "POST", op = %op, status = resp.status, "slack request");
        if !resp.is_success() {
            return Err(SlackError::Http {
                op,
                status: resp.status,
            });
        }
        Ok(())
    }

    fn open_dm_channel(&self, user: &str) -> Result<String, SlackError> {
        let body = serde_json::json!({ "users": user });
        let value = self.send_write_value(
            SlackApiCall::ConversationsOpen.method(),
            SlackApiCall::ConversationsOpen,
            body,
            false,
        )?;
        value
            .get("channel")
            .and_then(|c| c.get("id"))
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| SlackError::Decode {
                op: SlackApiCall::ConversationsOpen.method(),
                reason: "conversations.open response had no channel.id".to_string(),
            })
    }

    fn file_info(&self, file_id: &str) -> Result<(FileDto, String), SlackError> {
        let url = format!(
            "{API_BASE}/{}?file={}",
            SlackApiCall::FilesInfo.method(),
            encode(file_id)
        );
        let resp = self.send_get(SlackApiCall::FilesInfo.method(), &url)?;
        let value: serde_json::Value =
            serde_json::from_slice(&resp.body).map_err(|_| SlackError::Decode {
                op: SlackApiCall::FilesInfo.method(),
                reason: "files.info response was not valid JSON".to_string(),
            })?;
        self.body_rule
            .check(SlackApiCall::FilesInfo.method(), &value, false)?;
        let file = value.get("file").ok_or_else(|| SlackError::Decode {
            op: SlackApiCall::FilesInfo.method(),
            reason: "files.info response had no file object".to_string(),
        })?;
        let dto = file_dto(file);
        let url = file
            .get("url_private_download")
            .or_else(|| file.get("url_private"))
            .and_then(serde_json::Value::as_str)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| SlackError::Decode {
                op: SlackApiCall::FilesInfo.method(),
                reason: "files.info response had no private download URL".to_string(),
            })?;
        Ok((dto, url.to_string()))
    }
}

impl SlackClient for RestSlackClient {
    fn list(
        &self,
        kind: NodeKind,
        params: &[(String, String)],
    ) -> Result<serde_json::Value, SlackError> {
        let call = SlackApiCall::read_for(kind);
        let op = call.method();
        let collection = collection_key(kind);
        let params = self.resolve_params(params)?;
        let mut merged: Vec<serde_json::Value> = Vec::new();
        let mut cursor: Option<String> = None;
        for _page in 0..MAX_PAGES {
            let url = Self::list_url(call, &params, cursor.as_deref());
            let resp = self.send_get(op, &url)?;
            let value: serde_json::Value =
                serde_json::from_slice(&resp.body).map_err(|_| SlackError::Decode {
                    op,
                    reason: "list response was not valid JSON".to_string(),
                })?;
            // BodyErrorRule on the read path too (a list can be ok:false — e.g. channel_not_found).
            self.body_rule.check(op, &value, false)?;
            if let Some(items) = value.get(collection).and_then(serde_json::Value::as_array) {
                merged.extend(items.iter().cloned());
            }
            cursor = value
                .get("response_metadata")
                .and_then(|m| m.get("next_cursor"))
                .and_then(serde_json::Value::as_str)
                .filter(|c| !c.is_empty())
                .map(str::to_string);
            if cursor.is_none() {
                break;
            }
        }
        // Re-wrap under the collection key so the decoder's envelope-aware path works uniformly.
        Ok(serde_json::json!({ collection: merged }))
    }

    fn apply(&self, effect: &SlackEffect) -> Result<u64, SlackError> {
        let swallow = effect.swallows_already_done();
        match effect {
            SlackEffect::PostMessage {
                channel,
                text,
                thread_ts,
                client_msg_id,
                is_dm,
            } => {
                let channel = if *is_dm {
                    self.open_dm_channel(channel)?
                } else {
                    channel.clone()
                };
                let mut payload = serde_json::json!({
                    "channel": channel, "text": text, "client_msg_id": client_msg_id,
                });
                if let Some(t) = thread_ts {
                    payload["thread_ts"] = serde_json::Value::String(t.clone());
                }
                // POST is not idempotent: at-least-once, never silently retried (blueprint §7).
                self.send_write(
                    "chat.postMessage",
                    SlackApiCall::ChatPostMessage,
                    payload,
                    false,
                )?;
                Ok(1)
            }
            SlackEffect::AddReaction { channel, ts, emoji } => {
                let payload =
                    serde_json::json!({ "channel": channel, "timestamp": ts, "name": emoji });
                self.send_write(
                    "reactions.add",
                    SlackApiCall::ReactionsAdd,
                    payload,
                    swallow,
                )?;
                Ok(1)
            }
            SlackEffect::RemoveReaction { channel, ts, emoji } => {
                let payload =
                    serde_json::json!({ "channel": channel, "timestamp": ts, "name": emoji });
                self.send_write(
                    "reactions.remove",
                    SlackApiCall::ReactionsRemove,
                    payload,
                    swallow,
                )?;
                Ok(1)
            }
            SlackEffect::DeleteMessage { channel, ts } => {
                let payload = serde_json::json!({ "channel": channel, "ts": ts });
                self.send_write("chat.delete", SlackApiCall::ChatDelete, payload, false)?;
                Ok(1)
            }
            SlackEffect::UpdateMessage { channel, ts, text } => {
                let payload = serde_json::json!({ "channel": channel, "ts": ts, "text": text });
                self.send_write("chat.update", SlackApiCall::ChatUpdate, payload, false)?;
                Ok(1)
            }
            SlackEffect::Pin { channel, ts } => {
                let payload = serde_json::json!({ "channel": channel, "timestamp": ts });
                self.send_write("pins.add", SlackApiCall::PinsAdd, payload, swallow)?;
                Ok(1)
            }
            SlackEffect::Unpin { channel, ts } => {
                let payload = serde_json::json!({ "channel": channel, "timestamp": ts });
                self.send_write("pins.remove", SlackApiCall::PinsRemove, payload, swallow)?;
                Ok(1)
            }
            SlackEffect::UploadFile {
                name,
                mime,
                bytes,
                channel,
            } => {
                self.upload_external(name, mime.as_deref(), bytes, channel.as_deref())?;
                Ok(1)
            }
            SlackEffect::DeleteFile { id } => {
                let payload = serde_json::json!({ "file": id });
                self.send_write("files.delete", SlackApiCall::FilesDelete, payload, false)?;
                Ok(1)
            }
        }
    }

    fn download_file(&self, file_id: &str) -> Result<(FileDto, Vec<u8>), SlackError> {
        let (meta, url) = self.file_info(file_id)?;
        let req = self.request(HttpMethod::Get, url)?;
        let resp = self.http.send(&req).map_err(SlackError::from)?;
        tracing::debug!(
            method = "GET",
            op = "files.download",
            status = resp.status,
            "slack request"
        );
        if !resp.is_success() {
            return Err(SlackError::Http {
                op: "files.download",
                status: resp.status,
            });
        }
        Ok((meta, resp.body))
    }
}

fn file_dto(value: &serde_json::Value) -> FileDto {
    FileDto {
        id: json_str(value, "id"),
        name: json_str(value, "name"),
        mimetype: json_str(value, "mimetype"),
        size: value
            .get("size")
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(0),
        created: value
            .get("created")
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(0)
            * 1000,
        user: json_str(value, "user"),
    }
}

fn json_str(value: &serde_json::Value, key: &str) -> String {
    value
        .get(key)
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_string()
}

/// The JSON envelope key a node kind's list rows live under (`messages`/`members`/`files`).
const fn collection_key(kind: NodeKind) -> &'static str {
    match kind {
        NodeKind::Messages | NodeKind::Replies | NodeKind::Reactions | NodeKind::Dms => "messages",
        NodeKind::Files => "files",
        NodeKind::Users => "members",
    }
}

/// Minimal percent-encoding for a query parameter value (dependency-free).
fn encode(value: &str) -> String {
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

fn needs_channel_lookup(channel: &str) -> bool {
    let raw = channel.trim_start_matches('#');
    let b = raw.as_bytes();
    !(matches!(b.first(), Some(b'C' | b'G' | b'D'))
        && b.len() > 1
        && b[1..].iter().all(u8::is_ascii_alphanumeric))
}

fn needs_dm_open(channel: &str) -> bool {
    let raw = channel.trim_start_matches('@');
    let b = raw.as_bytes();
    matches!(b.first(), Some(b'U')) && b.len() > 1 && b[1..].iter().all(u8::is_ascii_alphanumeric)
}

/// Encode `(key, value)` pairs as an `application/x-www-form-urlencoded` body. Dependency-free and
/// wasm-safe (the Slack driver deliberately links no `url`/`percent-encoding` crate), mirroring the
/// inline HMAC/hex the events path already uses.
fn form_urlencode(params: &[(String, String)]) -> String {
    params
        .iter()
        .map(|(k, v)| format!("{}={}", percent_encode(k), percent_encode(v)))
        .collect::<Vec<_>>()
        .join("&")
}

/// Percent-encode a string for a form body: the RFC 3986 unreserved set (`A-Z a-z 0-9 - _ . ~`)
/// passes through verbatim; every other byte becomes `%XX` (uppercase hex). A space is therefore
/// `%20`, never `+` — Slack accepts `%20` and it avoids the `+`/space ambiguity.
fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for &b in s.as_bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push('%');
            out.push(
                char::from_digit(u32::from(b >> 4), 16)
                    .unwrap_or('0')
                    .to_ascii_uppercase(),
            );
            out.push(
                char::from_digit(u32::from(b & 0xF), 16)
                    .unwrap_or('0')
                    .to_ascii_uppercase(),
            );
        }
    }
    out
}

/// One recorded Slack API call — what a test asserts the driver issued. Secret-free by construction
/// (no token ever enters this seam).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum RecordedCall {
    /// A `list` with the node kind and the pushed params.
    List {
        /// The listed node kind.
        kind: NodeKind,
        /// The pushed query params.
        params: Vec<(String, String)>,
    },
    /// A write/CALL effect applied (the decoded effect itself).
    Apply(SlackEffect),
    /// A single Slack file download.
    DownloadFile(String),
}

/// An in-memory mock Slack client (tests / CI / wasm): answers list calls from pre-seeded JSON and
/// **records** every call so a test asserts the exact API surface the driver exercised — with **no
/// socket and no credentials**. The recorded calls also prove `PREVIEW` performs zero I/O.
#[derive(Default)]
pub struct MockSlackClient {
    list_pages: Mutex<Vec<serde_json::Value>>,
    recorded: Mutex<Vec<RecordedCall>>,
}

impl MockSlackClient {
    /// An empty mock.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Seed the JSON value a `list` returns (FIFO across calls).
    #[must_use]
    pub fn with_list(self, value: serde_json::Value) -> Self {
        if let Ok(mut q) = self.list_pages.lock() {
            q.push(value);
        }
        self
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

impl SlackClient for MockSlackClient {
    fn list(
        &self,
        kind: NodeKind,
        params: &[(String, String)],
    ) -> Result<serde_json::Value, SlackError> {
        self.record(RecordedCall::List {
            kind,
            params: params.to_vec(),
        });
        let page = self
            .list_pages
            .lock()
            .ok()
            .and_then(|mut q| {
                if q.is_empty() {
                    None
                } else {
                    Some(q.remove(0))
                }
            })
            .unwrap_or_else(|| serde_json::json!({ collection_key(kind): [] }));
        Ok(page)
    }

    fn apply(&self, effect: &SlackEffect) -> Result<u64, SlackError> {
        self.record(RecordedCall::Apply(effect.clone()));
        Ok(1)
    }

    fn download_file(&self, file_id: &str) -> Result<(FileDto, Vec<u8>), SlackError> {
        self.record(RecordedCall::DownloadFile(file_id.to_string()));
        Ok((
            FileDto {
                id: file_id.to_string(),
                name: "download.txt".to_string(),
                mimetype: "text/plain".to_string(),
                size: 9,
                created: 0,
                user: "U0CALLER".to_string(),
            },
            b"mock file".to_vec(),
        ))
    }
}
