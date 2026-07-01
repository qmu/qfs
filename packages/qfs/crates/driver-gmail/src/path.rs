//! [`MailPath`] — the parse of a qfs [`Path`](qfs_driver::Path) / `id:` address into the
//! concrete Gmail node it names (RFD-0001 §5). The mailbox maps onto the Append/log
//! archetype: **labels = directories, messages = files, attachments = nested entries**.
//!
//! ## Addressing
//! - `/mail` — the virtual root; lists **labels** (directories).
//! - `/mail/<label>` — a label; lists **messages** (files). `<label>` is a Gmail label id
//!   (e.g. `INBOX`, `SENT`, or a user label id).
//! - `/mail/drafts` — the drafts collection (INSERT/UPSERT/SELECT/REMOVE target).
//! - `id:<msg>` — a single message addressed by its Gmail message id.
//! - `id:thread:<id>` — a whole thread addressed by its Gmail thread id.
//! - `/mail/<label>/<msg>` — a message under a label (the file-under-directory form).
//! - `/mail/<label>/<msg>/<att>` — an attachment nested under a message.
//!
//! Pure parsing only — no I/O. Owned data only; no vendor type crosses.

use qfs_driver::Path;

use crate::error::GmailError;

/// The mount this driver answers for. The virtual root lists labels; sub-paths list
/// messages and attachments.
pub const MOUNT: &str = "/mail";

/// The reserved label segment naming the drafts collection (the INSERT/UPSERT target).
pub const DRAFTS_SEGMENT: &str = "drafts";

/// Gmail's built-in **system** label ids. Their canonical form is UPPERCASE (`INBOX`, `SENT`, …),
/// but a user naturally writes `/mail/inbox`, so a label segment is matched against these
/// case-insensitively and canonicalized (see [`canonical_label`]).
const SYSTEM_LABELS: &[&str] = &[
    "INBOX",
    "SENT",
    "DRAFT",
    "SPAM",
    "TRASH",
    "STARRED",
    "IMPORTANT",
    "UNREAD",
    "CHAT",
    "CATEGORY_PERSONAL",
    "CATEGORY_SOCIAL",
    "CATEGORY_PROMOTIONS",
    "CATEGORY_UPDATES",
    "CATEGORY_FORUMS",
];

/// Canonicalize a label segment to its Gmail label id. A **system** label is case-insensitive —
/// `inbox`, `Inbox`, and `INBOX` all name the `INBOX` system label — so the ergonomic lowercase
/// spelling works. A segment that is not a known system label is a **user** label id, which Gmail
/// treats case-sensitively, so it passes through unchanged.
#[must_use]
fn canonical_label(segment: &str) -> String {
    let upper = segment.to_ascii_uppercase();
    if SYSTEM_LABELS.contains(&upper.as_str()) {
        upper
    } else {
        segment.to_string()
    }
}

/// A parsed Gmail address — what a `/mail/...` path or an `id:` selector resolves to.
/// Owned, vendor-free. The applier and the introspective methods branch on this.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum MailPath {
    /// `/mail` — the virtual root (lists labels).
    Root,
    /// `/mail/<label>` — a label node (lists messages); `name` is the Gmail label id.
    Label {
        /// The Gmail label id (e.g. `INBOX`).
        name: String,
    },
    /// `/mail/drafts` — the drafts collection.
    Drafts,
    /// A single message addressed by `id:<msg>` or `/mail/<label>/<msg>`.
    Message {
        /// The Gmail message id.
        id: String,
    },
    /// A whole thread addressed by `id:thread:<id>`.
    Thread {
        /// The Gmail thread id.
        id: String,
    },
    /// An attachment nested under a message (`/mail/<label>/<msg>/<att>`).
    Attachment {
        /// The owning message id.
        message: String,
        /// The attachment id.
        attachment: String,
    },
}

impl MailPath {
    /// Parse a driver [`Path`] string into a [`MailPath`].
    ///
    /// # Errors
    /// [`GmailError::InvalidPath`] if the path is not under `/mail`, an `id:` selector is
    /// empty/malformed, or it has too many segments.
    pub fn parse(path: &Path) -> Result<Self, GmailError> {
        Self::parse_str(path.as_str())
    }

    /// Parse a raw path/selector string into a [`MailPath`] (the core parse).
    ///
    /// # Errors
    /// [`GmailError::InvalidPath`] on a malformed address.
    pub fn parse_str(raw: &str) -> Result<Self, GmailError> {
        // `id:` addressing — a message or a thread by id, independent of any label.
        if let Some(rest) = raw.strip_prefix("id:") {
            return Self::parse_id(raw, rest);
        }

        let trimmed = raw.trim_end_matches('/');
        // The bare mount (with or without a trailing slash) is the virtual root.
        if trimmed == MOUNT || raw == MOUNT {
            return Ok(MailPath::Root);
        }
        let Some(after) = trimmed.strip_prefix(&format!("{MOUNT}/")) else {
            return Err(GmailError::InvalidPath {
                path: raw.to_string(),
                reason: "path is not under the /mail mount",
            });
        };

        let segments: Vec<&str> = after.split('/').filter(|s| !s.is_empty()).collect();
        match segments.as_slice() {
            [] => Ok(MailPath::Root),
            [one] if one.eq_ignore_ascii_case(DRAFTS_SEGMENT) => Ok(MailPath::Drafts),
            [label] => Ok(MailPath::Label {
                name: canonical_label(label),
            }),
            // `/mail/<label>/<msg>` — a message under a label.
            [_label, msg] => Ok(MailPath::Message {
                id: (*msg).to_string(),
            }),
            // `/mail/<label>/<msg>/<att>` — an attachment nested under a message.
            [_label, msg, att] => Ok(MailPath::Attachment {
                message: (*msg).to_string(),
                attachment: (*att).to_string(),
            }),
            _ => Err(GmailError::InvalidPath {
                path: raw.to_string(),
                reason: "too many path segments for a /mail address",
            }),
        }
    }

    /// Parse the part after the `id:` prefix into a [`MailPath::Message`]/[`MailPath::Thread`].
    fn parse_id(raw: &str, rest: &str) -> Result<Self, GmailError> {
        if let Some(thread_id) = rest.strip_prefix("thread:") {
            if thread_id.is_empty() {
                return Err(GmailError::InvalidPath {
                    path: raw.to_string(),
                    reason: "id:thread: selector carries no thread id",
                });
            }
            return Ok(MailPath::Thread {
                id: thread_id.to_string(),
            });
        }
        if rest.is_empty() {
            return Err(GmailError::InvalidPath {
                path: raw.to_string(),
                reason: "id: selector carries no message id",
            });
        }
        Ok(MailPath::Message {
            id: rest.to_string(),
        })
    }

    /// Whether this node is a *collection* (root, a label, or drafts) vs. a single
    /// message/thread/attachment — used to key capabilities and the archetype schema.
    #[must_use]
    pub const fn is_collection(&self) -> bool {
        matches!(
            self,
            MailPath::Root | MailPath::Label { .. } | MailPath::Drafts
        )
    }
}
