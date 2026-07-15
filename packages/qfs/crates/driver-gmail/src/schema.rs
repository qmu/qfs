//! Owned message/draft/attachment DTOs and their canonical typed [`Schema`] (blueprint §6/§11).
//!
//! Gmail JSON is translated into these owned DTOs at the [`crate::client`] boundary; the
//! `Driver` trait surface and the effect `Plan` carry **zero** google types (the no-vendor-leak
//! invariant, blueprint §11). The message [`Schema`] is the canonical `qfs_types::Schema` so
//! `DESCRIBE /mail/<label>` and type-checking agree on the same typed columns. `attachments` is
//! a nested `Array(Struct{..})` column — the `EXPAND` target (blueprint §4 "same operator for mail
//! attachments and JSON arrays").

use qfs_types::{Column, ColumnType, Row, Schema, Value};

/// One Gmail message projected into the owned, vendor-free DTO (blueprint §11). `date` is epoch
/// milliseconds (the canonical `Timestamp` runtime form). Attachment **bytes are not carried
/// here** — the listing row holds attachment metadata; the bytes are fetched on demand via the
/// attachment read path so a label scan stays cheap.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct MailMessage {
    /// The Gmail message id.
    pub id: String,
    /// The Gmail thread id this message belongs to.
    pub thread_id: String,
    /// The label ids applied to this message (e.g. `INBOX`, `UNREAD`).
    pub label_ids: Vec<String>,
    /// The message date as epoch milliseconds (0 if unavailable).
    pub date: i64,
    /// The `From` header value.
    pub from: String,
    /// The `Subject` header value.
    pub subject: String,
    /// The Gmail-provided snippet (a short preview).
    pub snippet: String,
    /// The RFC 5322 `Message-Id` header value (with angle brackets, e.g. `<abc@mail.gmail.com>`),
    /// empty if the message carries none. Not a read column — it is the source a **reply** threads
    /// from (`In-Reply-To`/`References`), read off the parent DTO by the applier, never projected
    /// into a listing row.
    pub message_id: String,
    /// Attachment metadata (filename + mime + attachment id); bytes fetched on demand.
    pub attachments: Vec<AttachmentMeta>,
}

/// Attachment metadata carried in a listing row (no bytes). Owned, vendor-free.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct AttachmentMeta {
    /// The attachment filename.
    pub filename: String,
    /// The attachment MIME type (e.g. `application/pdf`).
    pub mime: String,
    /// The Gmail attachment id (used to fetch the bytes on demand).
    pub attachment_id: String,
    /// The attachment size in bytes (0 if unknown).
    pub size: i64,
}

/// An attachment with bytes — the form a draft carries (the MIME builder consumes it) and the
/// form an attachment read yields. Owned bytes; no vendor type.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Attachment {
    /// The attachment filename.
    pub filename: String,
    /// The attachment MIME type.
    pub mime: String,
    /// The attachment bytes.
    pub bytes: Vec<u8>,
}

/// Where a draft lands in a thread — the typed linkage a **reply** carries and a standalone draft
/// does not. Its presence (`Some`) *is* the "reply-in-thread" case; its absence (`None`) the
/// "new standalone message" case, so the two are a sum type an unhandled arm cannot silently
/// confuse (type-driven-design). The `thread_id` rides in the Gmail `message.threadId` (server-side
/// threading); the `references` value seeds the RFC 5322 `In-Reply-To`/`References` headers so every
/// mail client threads it too, not only Gmail's view.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ReplyContext {
    /// The Gmail thread id the reply draft joins (the API `message.threadId`).
    pub thread_id: String,
    /// The parent's RFC 5322 `Message-Id` (with angle brackets) — the `In-Reply-To`/`References`
    /// value. Never empty when a [`ReplyContext`] is built (the applier fails closed otherwise).
    pub references: String,
}

/// A draft to create/replace — the owned DTO an `INSERT`/`UPSERT INTO /mail/drafts` carries.
/// The MIME builder ([`crate::mime::build_mime`]) turns it into RFC 5322 bytes. Owned only.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct MailDraft {
    /// The draft id, when replacing an existing draft (the `UPSERT` key); `None` to create.
    pub id: Option<String>,
    /// The `To` recipients.
    pub to: Vec<String>,
    /// The `Cc` recipients.
    pub cc: Vec<String>,
    /// The `Subject` (may be non-ASCII — encoded RFC 2047 by the MIME builder).
    pub subject: String,
    /// The plain-text body.
    pub body: String,
    /// The attachments (each carries its bytes for the multipart build).
    pub attachments: Vec<Attachment>,
    /// The thread linkage when this draft is a **reply** (`Some`), or `None` for a new standalone
    /// message. Drives the Gmail `threadId` and the `In-Reply-To`/`References` MIME headers.
    pub reply: Option<ReplyContext>,
}

impl MailDraft {
    /// The Gmail thread id this draft should join, if it is a reply — the value the client sends as
    /// `message.threadId`. `None` for a standalone draft.
    #[must_use]
    pub fn thread_id(&self) -> Option<&str> {
        self.reply.as_ref().map(|r| r.thread_id.as_str())
    }
}

impl MailDraft {
    /// A test-only constructor for a [`MailDraft`] with the given recipients/subject/body and the
    /// rest defaulted (no `id`, no `cc`, no attachments). Lets a downstream consumer fabricate the
    /// otherwise `#[non_exhaustive]` DTO directly in their own tests. Gated behind `cfg(test)` or
    /// the `test-util` feature so it never widens the production surface.
    #[cfg(any(test, feature = "test-util"))]
    #[must_use]
    pub fn for_test(to: Vec<String>, subject: &str, body: &str) -> Self {
        Self {
            id: None,
            to,
            cc: Vec::new(),
            subject: subject.to_string(),
            body: body.to_string(),
            attachments: Vec::new(),
            reply: None,
        }
    }
}

/// The **write** attachment struct schema (`{filename, mime, bytes}`) — the shape a draft/send/reply
/// `attachments` column carries. Distinct from [`MailMessage::attachment_schema`] (the **read** shape
/// `{filename, mime, size}`): a listing carries metadata only, a write carries the raw `Bytes`, so a
/// read→write attachment copy sources its bytes from the attachment byte-read, not the listing row.
/// This is the `Struct` element of the `attachments` `Array(Struct{..})` a `mail.send`/`mail.reply`
/// `attachments` param (and an `INSERT`/`UPSERT` `attachments` column) accepts.
#[must_use]
pub fn attachment_write_schema() -> Schema {
    Schema::new(vec![
        Column::new("filename", ColumnType::Text, false),
        Column::new("mime", ColumnType::Text, false),
        Column::new("bytes", ColumnType::Bytes, false),
    ])
}

/// The type of an `attachments` procedure parameter / column: `Array(Struct{filename, mime, bytes})`
/// — the write attachment shape ([`attachment_write_schema`]) an attach carries on every draft/send/
/// reply form.
#[must_use]
pub fn attachments_param_type() -> ColumnType {
    ColumnType::Array(Box::new(ColumnType::Struct(attachment_write_schema())))
}

/// The **write** schema a reply append-log advertises (`/mail/<label>/<msg>/replies`, `INSERT`):
/// `body` (required reply text) plus the optional `to`/`cc`/`subject` overrides and the same
/// `attachments` `Array(Struct{filename, mime, bytes})` every compose form accepts. Advertising
/// these column NAMES lets a cross-service `… |> select … as bytes |> aggregate array_agg(att) as
/// attachments |> extend body = … |> insert into /mail/<msg>/replies` resolve its projection at
/// plan time; the parent id rides the path, so it is not a column here.
#[must_use]
pub fn reply_write_schema() -> Schema {
    Schema::new(vec![
        Column::new("body", ColumnType::Text, false),
        Column::new("to", ColumnType::Text, true),
        Column::new("cc", ColumnType::Text, true),
        Column::new("subject", ColumnType::Text, true),
        Column::new("attachments", attachments_param_type(), true),
    ])
}

/// The label-listing [`Schema`] for `ls /mail` (the mailbox root): one `name` (Text) row per Gmail
/// label — the directory view of the archetype (labels = directories, blueprint §4). The client's
/// `list_labels` yields the label names; richer label metadata (id/type/counts) is a later add.
#[must_use]
pub fn label_listing_schema() -> Schema {
    Schema::new(vec![Column::new("name", ColumnType::Text, false)])
}

/// The single-node [`Schema`] for reading one attachment's bytes (`/mail/<label>/<msg>/<att>`,
/// gmail-ftp `get id:att:<msg>:<att>`): the `filename`/`mime`/`size` metadata (from the owning
/// message's part) plus the downloaded `content` (`Bytes`). The `content` column name mirrors the
/// Drive content read so a cross-service attach pipeline lines the two up.
#[must_use]
pub fn attachment_read_schema() -> Schema {
    Schema::new(vec![
        Column::new("filename", ColumnType::Text, false),
        Column::new("mime", ColumnType::Text, false),
        Column::new("size", ColumnType::Int, false),
        Column::new("content", ColumnType::Bytes, true),
    ])
}

impl MailMessage {
    /// The canonical message listing [`Schema`] — the typed columns `DESCRIBE /mail/<label>`
    /// reports and a label scan's rows conform to. Stable column order powers golden snapshots.
    /// `label_ids` is `Array(Text)` and `attachments` is `Array(Struct{filename, mime, size})`
    /// — both `EXPAND` targets (blueprint §4).
    #[must_use]
    pub fn schema() -> Schema {
        Schema::new(vec![
            Column::new("id", ColumnType::Text, false),
            Column::new("thread_id", ColumnType::Text, false),
            Column::new("date", ColumnType::Timestamp, false),
            Column::new("from", ColumnType::Text, false),
            Column::new("subject", ColumnType::Text, false),
            Column::new("snippet", ColumnType::Text, false),
            Column::new(
                "label_ids",
                ColumnType::Array(Box::new(ColumnType::Text)),
                false,
            ),
            Column::new(
                "attachments",
                ColumnType::Array(Box::new(ColumnType::Struct(Self::attachment_schema()))),
                false,
            ),
        ])
    }

    /// The nested attachment struct schema (`{filename, mime, size}`) used inside the
    /// `attachments` `Array(Struct{..})` column.
    #[must_use]
    pub fn attachment_schema() -> Schema {
        // Note: the Gmail `attachmentId` is deliberately NOT a column here — it is **ephemeral**
        // (regenerated by every `messages.get`), so a literal id carried across statements is
        // already stale. An attachment is addressed by its **stable position** instead:
        // `/mail/<label>/<msg>/att<N>` (0-based; see `read.rs`), which resolves a fresh id inside
        // the read. The array's order here is that index.
        Schema::new(vec![
            Column::new("filename", ColumnType::Text, false),
            Column::new("mime", ColumnType::Text, false),
            Column::new("size", ColumnType::Int, false),
        ])
    }

    /// A test-only constructor for a [`MailMessage`] with the salient identity/header fields
    /// set and the rest defaulted (no labels, no attachments, `date` 0, empty `snippet`). Lets a
    /// downstream consumer fabricate the otherwise `#[non_exhaustive]` DTO to seed
    /// [`MockGmailClient::with_message`](crate::MockGmailClient::with_message) in their own tests.
    /// Gated behind `cfg(test)` (the crate's own tests) or the `test-util` feature so it never
    /// widens the production surface.
    #[cfg(any(test, feature = "test-util"))]
    #[must_use]
    pub fn for_test(id: &str, thread_id: &str, from: &str, subject: &str) -> Self {
        Self {
            id: id.to_string(),
            thread_id: thread_id.to_string(),
            label_ids: Vec::new(),
            date: 0,
            from: from.to_string(),
            subject: subject.to_string(),
            snippet: String::new(),
            message_id: String::new(),
            attachments: Vec::new(),
        }
    }

    /// Project this message onto the canonical [`MailMessage::schema`] column order as a typed
    /// [`Row`] — the form a label scan yields into a pipeline.
    #[must_use]
    pub fn to_row(&self) -> Row {
        let labels = self
            .label_ids
            .iter()
            .map(|l| Value::Text(l.clone()))
            .collect();
        let attachments = self
            .attachments
            .iter()
            .map(|a| {
                Value::Struct(qfs_types::Fields::new(vec![
                    ("filename".to_string(), Value::Text(a.filename.clone())),
                    ("mime".to_string(), Value::Text(a.mime.clone())),
                    ("size".to_string(), Value::Int(a.size)),
                ]))
            })
            .collect();
        Row::new(vec![
            Value::Text(self.id.clone()),
            Value::Text(self.thread_id.clone()),
            Value::Timestamp(self.date),
            Value::Text(self.from.clone()),
            Value::Text(self.subject.clone()),
            Value::Text(self.snippet.clone()),
            Value::Array(labels),
            Value::Array(attachments),
        ])
    }
}
