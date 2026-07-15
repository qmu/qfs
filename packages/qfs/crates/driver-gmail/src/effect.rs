//! [`GmailEffect`] — the owned effect the driver realises a plan leaf as (blueprint §7), and the
//! decode from a runtime [`EffectNode`] onto it. The applier ([`crate::applier`]) interprets one
//! of these against the Gmail API under `COMMIT`.
//!
//! ## Why an explicit effect enum
//! The closed core [`EffectKind`] (`Insert`/`Upsert`/`Update`/`Remove`/`Call`) is universal. The
//! Gmail driver maps each onto a concrete Gmail op via the `(kind, path, args)` triple:
//! - `INSERT INTO /mail/drafts`  → [`GmailEffect::CreateDraft`]
//! - `UPSERT INTO /mail/drafts`  → [`GmailEffect::UpsertDraft`] (retry-safe, keyed by draft id)
//! - `UPDATE /mail/<label>`      → [`GmailEffect::ModifyLabels`]
//! - `REMOVE id:<msg>`           → [`GmailEffect::TrashMessage`]
//! - `REMOVE id:thread:<id>`     → [`GmailEffect::TrashThread`]
//! - `CALL mail.send`            → [`GmailEffect::Send`] (irreversible)
//!
//! The draft fields ride in well-known row columns ([`TO_COL`] etc.); the label add/remove ride
//! in [`ADD_LABELS_COL`]/[`REMOVE_LABELS_COL`]. No vendor type appears here.

use qfs_plan::{EffectKind, EffectNode};
use qfs_types::Value;

use crate::error::GmailError;
use crate::mime;
use crate::path::MailPath;
use crate::schema::{Attachment, MailDraft};

/// Row column carrying the draft id (the `UPSERT` key, and the `mail.send` draft-id de-dupe key).
pub const DRAFT_ID_COL: &str = "draft_id";
/// Row column carrying the `To` recipients (comma-separated).
pub const TO_COL: &str = "to";
/// Row column carrying the `Cc` recipients (comma-separated).
pub const CC_COL: &str = "cc";
/// Row column carrying the `Subject`.
pub const SUBJECT_COL: &str = "subject";
/// Row column carrying the plain-text body.
pub const BODY_COL: &str = "body";
/// Row column carrying label ids to add (comma-separated) for a label `UPDATE`.
pub const ADD_LABELS_COL: &str = "add_labels";
/// Row column carrying label ids to remove (comma-separated) for a label `UPDATE`.
pub const REMOVE_LABELS_COL: &str = "remove_labels";
/// Row column carrying a new label's name (`INSERT INTO /mail/labels`, the label-create `name`).
pub const NAME_COL: &str = "name";

/// One fully-decoded Gmail effect — what the apply leg executes against the API. Owned DTOs;
/// no google type appears here. `Send`, `TrashMessage`, `TrashThread` are irreversible (blueprint §8).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum GmailEffect {
    /// Create a draft (`INSERT INTO /mail/drafts`).
    CreateDraft {
        /// The draft to create.
        draft: MailDraft,
    },
    /// Create-or-replace a draft by id (`UPSERT INTO /mail/drafts`) — retry-safe.
    UpsertDraft {
        /// The draft id to replace.
        id: String,
        /// The draft content.
        draft: MailDraft,
    },
    /// Create a new user label (`INSERT INTO /mail/labels`, gmail-ftp `mkdir`) — reversible.
    CreateLabel {
        /// The new label's name (may be nested, e.g. `Work/Receipts`).
        name: String,
    },
    /// Modify a message's labels (`UPDATE /mail/<label>`).
    ModifyLabels {
        /// The message id whose labels change.
        message: String,
        /// Label ids to add.
        add: Vec<String>,
        /// Label ids to remove.
        remove: Vec<String>,
    },
    /// Trash a single message (`REMOVE id:<msg>`) — the `gmail.modify` trash op, not delete.
    TrashMessage {
        /// The message id to trash.
        id: String,
    },
    /// Trash a whole thread (`REMOVE id:thread:<id>`).
    TrashThread {
        /// The thread id to trash.
        id: String,
    },
    /// Send a draft (`CALL mail.send`) — irreversible. Carries the draft id (the de-dupe key)
    /// when sending a previously-created draft, or a freshly-built draft to create-then-send.
    Send {
        /// An existing draft id to send (the retry-safe path), or `None` to send `draft`.
        draft_id: Option<String>,
        /// The draft to create-then-send when `draft_id` is `None`.
        draft: Option<MailDraft>,
    },
    /// Reply into a parent message's thread (`<parent> |> CALL mail.reply`) — **reversible** (it
    /// creates a draft, exactly like an `INSERT INTO /mail/drafts`, held until a separate send).
    /// The applier reads the parent (`get_message`) at COMMIT to resolve the thread id + parent
    /// `Message-Id` + the `to`/`subject` defaults, then creates the threaded draft.
    Reply {
        /// The parent message id the reply threads under (the addressed node).
        parent: String,
        /// Explicit `To` override; empty means default to the parent's `From`.
        to: Vec<String>,
        /// Explicit `Cc` recipients (empty for none).
        cc: Vec<String>,
        /// Explicit `Subject` override; `None` means default to `Re: <parent subject>`.
        subject: Option<String>,
        /// The reply body (required).
        body: String,
        /// Attachments to include (each carries its bytes); empty for none. Same
        /// `Array(Struct{filename, mime, bytes})` shape as an `INSERT INTO /mail/drafts`.
        attachments: Vec<Attachment>,
    },
}

impl GmailEffect {
    /// Decode a runtime [`EffectNode`] into the concrete Gmail operation.
    ///
    /// # Errors
    /// [`GmailError`] if the `(kind, path)` pair is not one the Gmail driver services, or the
    /// row args carry no usable payload.
    pub fn from_node(node: &EffectNode) -> Result<Self, GmailError> {
        let path = MailPath::parse_str(node.target.path.as_str())?;
        match &node.kind {
            EffectKind::Insert => match &path {
                MailPath::Labels => Self::decode_create_label(node),
                MailPath::Replies { parent } => Self::decode_reply_insert(node, parent),
                _ => Self::decode_create_draft(node, &path),
            },
            EffectKind::Upsert => Self::decode_upsert_draft(node, &path),
            EffectKind::Update => Self::decode_modify_labels(node, &path),
            EffectKind::Remove => Self::decode_trash(&path, node),
            EffectKind::Call(proc) => Self::decode_call(proc.as_str(), node),
            other => Err(GmailError::MalformedEffect {
                verb: "EFFECT",
                path: node.target.path.as_str().to_string(),
                reason: format!("{} is not serviced by the Gmail driver", other.label()),
            }),
        }
    }

    fn decode_create_draft(node: &EffectNode, path: &MailPath) -> Result<Self, GmailError> {
        require_drafts(path, "INSERT", node)?;
        let draft = draft_from_row(node)?;
        Ok(GmailEffect::CreateDraft { draft })
    }

    /// Decode `INSERT INTO /mail/labels VALUES ('<name>')` — the label-create (gmail-ftp `mkdir`).
    fn decode_create_label(node: &EffectNode) -> Result<Self, GmailError> {
        let name = text_col(node, NAME_COL).ok_or_else(|| GmailError::MalformedEffect {
            verb: "INSERT",
            path: node.target.path.as_str().to_string(),
            reason: format!("INSERT INTO /mail/labels needs a `{NAME_COL}` for the new label"),
        })?;
        Ok(GmailEffect::CreateLabel { name })
    }

    fn decode_upsert_draft(node: &EffectNode, path: &MailPath) -> Result<Self, GmailError> {
        require_drafts(path, "UPSERT", node)?;
        let draft = draft_from_row(node)?;
        let id = draft
            .id
            .clone()
            .ok_or_else(|| GmailError::MalformedEffect {
                verb: "UPSERT",
                path: node.target.path.as_str().to_string(),
                reason: format!("UPSERT INTO /mail/drafts needs a `{DRAFT_ID_COL}` to key on"),
            })?;
        Ok(GmailEffect::UpsertDraft { id, draft })
    }

    fn decode_modify_labels(node: &EffectNode, path: &MailPath) -> Result<Self, GmailError> {
        // UPDATE targets a label collection; the message id rides in the WHERE-SELECTOR (§7) —
        // never `args`, which carries only the SET payload (`add_labels`/`remove_labels`).
        let (MailPath::Label { .. } | MailPath::Message { .. }) = path else {
            return Err(GmailError::CapabilityDenied {
                path: node.target.path.as_str().to_string(),
                verb: "UPDATE",
            });
        };
        let message = match path {
            MailPath::Message { id } => id.clone(),
            _ => node
                .selector_text("id")
                .ok_or_else(|| GmailError::MalformedEffect {
                    verb: "UPDATE",
                    path: node.target.path.as_str().to_string(),
                    reason: "a collection UPDATE needs the exact target message `id` \
                         (`update /mail/<label> set add_labels = … where id == '<msgid>'`); a \
                         set-wide predicate write is refused (it would risk over-matching Gmail's \
                         search)"
                        .to_string(),
                })?,
        };
        let add = list_col(node, ADD_LABELS_COL);
        let remove = list_col(node, REMOVE_LABELS_COL);
        if add.is_empty() && remove.is_empty() {
            return Err(GmailError::MalformedEffect {
                verb: "UPDATE",
                path: node.target.path.as_str().to_string(),
                reason: format!(
                    "UPDATE names no labels (set `{ADD_LABELS_COL}`/`{REMOVE_LABELS_COL}`)"
                ),
            });
        }
        Ok(GmailEffect::ModifyLabels {
            message,
            add,
            remove,
        })
    }

    fn decode_trash(path: &MailPath, node: &EffectNode) -> Result<Self, GmailError> {
        match path {
            MailPath::Message { id } => Ok(GmailEffect::TrashMessage { id: id.clone() }),
            MailPath::Thread { id } => Ok(GmailEffect::TrashThread { id: id.clone() }),
            // A collection REMOVE (`remove /mail/<label> where id == '<msgid>'`) trashes the ONE
            // message its exact `id` equality key names — the same exact-key contract UPDATE uses
            // ([`Self::decode_modify_labels`]), so a collection node's advertised `Remove` is honest.
            // A set-wide predicate (`where subject LIKE …` / `where from == …`) carries NO `id` key
            // and is refused CLOSED here: enumerating by Gmail's *lossy* `q=` search could trash
            // messages the predicate never exactly matched (a data-loss risk), so a collection trash
            // requires the exact id, never a fuzzy set (blueprint §6; ticket 20260704155500).
            MailPath::Label { .. } => {
                // The exact `id` key rides the WHERE-SELECTOR (§7). A REMOVE's `args` is now empty
                // — it writes nothing — so the selector is the only channel this key travels on.
                let id = node
                    .selector_text("id")
                    .ok_or_else(|| GmailError::MalformedEffect {
                        verb: "REMOVE",
                        path: node.target.path.as_str().to_string(),
                        reason: "a collection REMOVE needs the exact target message `id` \
                             (`remove /mail/<label> where id == '<msgid>'`); a set-wide predicate \
                             write is refused (it would risk over-matching Gmail's search)"
                            .to_string(),
                    })?;
                Ok(GmailEffect::TrashMessage { id })
            }
            _ => Err(GmailError::CapabilityDenied {
                path: node.target.path.as_str().to_string(),
                verb: "REMOVE",
            }),
        }
    }

    fn decode_call(proc: &str, node: &EffectNode) -> Result<Self, GmailError> {
        match proc {
            "mail.send" => Self::decode_send(node),
            "mail.reply" => Self::decode_reply(node),
            _ => Err(GmailError::UnknownProcedure(proc.to_string())),
        }
    }

    fn decode_send(node: &EffectNode) -> Result<Self, GmailError> {
        // `mail.send` resolves the draft to send, in order:
        //  1) an explicit `draft_id` column (the retry-safe de-dupe key, e.g. an UPSERT-shaped row);
        //  2) the **addressed draft node** `/mail/drafts/<id>` — its target path carries the draft
        //     id (the only channel: a CALL's args are its literal arguments, never upstream rows);
        //  3) otherwise the call's `to`/`subject`/`body` args build a fresh draft to create-then-send.
        // Case 2 is the fix for sending an existing draft by id (the path segment was never lowered
        // into a `draft_id` before, so every form fell into a byteless create-then-send).
        if let Some(draft_id) = text_col(node, DRAFT_ID_COL) {
            return Ok(GmailEffect::Send {
                draft_id: Some(draft_id),
                draft: None,
            });
        }
        if let Ok(MailPath::Draft { id }) = MailPath::parse_str(node.target.path.as_str()) {
            return Ok(GmailEffect::Send {
                draft_id: Some(id),
                draft: None,
            });
        }
        let draft = draft_from_row(node)?;
        Ok(GmailEffect::Send {
            draft_id: None,
            draft: Some(draft),
        })
    }

    /// Decode `<parent> |> CALL mail.reply(body => …[, to, cc, subject])`. The parent is the
    /// **addressed message node** (`id:<msg>` or `/mail/<label>/<msg>`) — the thread source; the
    /// applier resolves the thread + defaults from it at COMMIT. `body` is required here; `to`/`cc`/
    /// `subject` are optional overrides carried in the call's args (defaults are applied against the
    /// parent, so they are not resolvable at decode time — an empty `to` stays empty until apply).
    fn decode_reply(node: &EffectNode) -> Result<Self, GmailError> {
        let parent = match MailPath::parse_str(node.target.path.as_str())? {
            MailPath::Message { id } => id,
            _ => {
                return Err(GmailError::MalformedEffect {
                    verb: "CALL",
                    path: node.target.path.as_str().to_string(),
                    reason: "mail.reply must be addressed at a parent message \
                             (`/mail/<label>/<msg> |> call mail.reply(...)` or `id:<msg> |> …`)"
                        .to_string(),
                });
            }
        };
        let body = text_col(node, BODY_COL).ok_or_else(|| GmailError::MalformedEffect {
            verb: "CALL",
            path: node.target.path.as_str().to_string(),
            reason: format!("mail.reply needs a `{BODY_COL}`"),
        })?;
        Ok(GmailEffect::Reply {
            parent,
            to: list_col(node, TO_COL),
            cc: list_col(node, CC_COL),
            subject: text_col(node, SUBJECT_COL),
            body,
            // A reply attaches via the same `attachments` `Array(Struct{filename, mime, bytes})`
            // column as any draft write (empty when the call passes none).
            attachments: attachments_col(node),
        })
    }

    /// Decode `<source> |> INSERT INTO /mail/<label>/<msg>/replies` — the **pipeline-composable**
    /// reply. The parent message id comes from the addressed `replies` node's path (not a row), so
    /// the row payload is free to carry `attachments` sourced from ANOTHER service (a Drive blob, a
    /// Gmail attachment) materialized at the commit boundary — the leg a `CALL mail.reply`'s literal
    /// args cannot express. Produces the SAME [`GmailEffect::Reply`] the CALL form does, so the
    /// applier's thread resolution + `Re:` defaulting are shared verbatim; `body` is required, and
    /// `to`/`cc`/`subject` are optional overrides read from the materialized row.
    fn decode_reply_insert(node: &EffectNode, parent: &str) -> Result<Self, GmailError> {
        let body = text_col(node, BODY_COL).ok_or_else(|| GmailError::MalformedEffect {
            verb: "INSERT",
            path: node.target.path.as_str().to_string(),
            reason: format!(
                "INSERT INTO /mail/<label>/<msg>/replies needs a `{BODY_COL}` (the reply text)"
            ),
        })?;
        Ok(GmailEffect::Reply {
            parent: parent.to_string(),
            to: list_col(node, TO_COL),
            cc: list_col(node, CC_COL),
            subject: text_col(node, SUBJECT_COL),
            body,
            attachments: attachments_col(node),
        })
    }

    /// Whether this effect is irreversible (blueprint §8): `Send` + both trash ops. `Reply` is
    /// **reversible** — it only creates a draft (nothing is sent until a separate `mail.send`).
    #[must_use]
    pub const fn is_irreversible(&self) -> bool {
        matches!(
            self,
            GmailEffect::Send { .. }
                | GmailEffect::TrashMessage { .. }
                | GmailEffect::TrashThread { .. }
        )
    }

    /// The stable verb label (for the audit ledger / capability-denied errors).
    #[must_use]
    pub const fn verb_label(&self) -> &'static str {
        match self {
            GmailEffect::CreateDraft { .. } | GmailEffect::CreateLabel { .. } => "INSERT",
            GmailEffect::UpsertDraft { .. } => "UPSERT",
            GmailEffect::ModifyLabels { .. } => "UPDATE",
            GmailEffect::TrashMessage { .. } | GmailEffect::TrashThread { .. } => "REMOVE",
            GmailEffect::Send { .. } | GmailEffect::Reply { .. } => "CALL",
        }
    }
}

/// Build the base64url `raw` message for a draft (the value the Gmail `raw` field carries).
///
/// # Errors
/// [`GmailError::Mime`] if the draft has no recipients.
pub fn draft_raw(draft: &MailDraft) -> Result<String, GmailError> {
    mime::raw_base64url(draft)
}

/// Require the path be `/mail/drafts` for a draft write; otherwise a capability denial.
fn require_drafts(
    path: &MailPath,
    verb: &'static str,
    node: &EffectNode,
) -> Result<(), GmailError> {
    if matches!(path, MailPath::Drafts) {
        Ok(())
    } else {
        Err(GmailError::CapabilityDenied {
            path: node.target.path.as_str().to_string(),
            verb,
        })
    }
}

/// Build a [`MailDraft`] from the node's first row, reading the well-known draft columns.
fn draft_from_row(node: &EffectNode) -> Result<MailDraft, GmailError> {
    let to = list_col(node, TO_COL);
    if to.is_empty() {
        return Err(GmailError::MalformedEffect {
            verb: "INSERT",
            path: node.target.path.as_str().to_string(),
            reason: format!("draft has no `{TO_COL}` recipients"),
        });
    }
    Ok(MailDraft {
        id: text_col(node, DRAFT_ID_COL),
        to,
        cc: list_col(node, CC_COL),
        subject: text_col(node, SUBJECT_COL).unwrap_or_default(),
        body: text_col(node, BODY_COL).unwrap_or_default(),
        // Attachments arrive as an `Array(Struct{filename, mime, bytes})` column when present.
        attachments: attachments_col(node),
        // An `INSERT`/`UPSERT`-shaped draft is standalone; thread linkage is added only by the
        // `mail.reply` applier path (which resolves it from the parent), never a write column.
        reply: None,
    })
}

/// Read a `Text` value from the node's first row by column name.
fn text_col(node: &EffectNode, name: &str) -> Option<String> {
    let idx = node
        .args
        .schema
        .columns
        .iter()
        .position(|c| c.name == name)?;
    match node.args.rows.first().and_then(|r| r.values.get(idx)) {
        Some(Value::Text(t)) if !t.is_empty() => Some(t.clone()),
        _ => None,
    }
}

/// Read a comma-separated `Text` column into a list of trimmed, non-empty items.
fn list_col(node: &EffectNode, name: &str) -> Vec<String> {
    text_col(node, name)
        .map(|s| {
            s.split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// Read the `attachments` `Array(Struct{filename, mime, bytes})` column from the first row.
fn attachments_col(node: &EffectNode) -> Vec<Attachment> {
    let Some(idx) = node
        .args
        .schema
        .columns
        .iter()
        .position(|c| c.name == "attachments")
    else {
        return Vec::new();
    };
    let Some(Value::Array(items)) = node.args.rows.first().and_then(|r| r.values.get(idx)) else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|v| {
            let Value::Struct(fields) = v else {
                return None;
            };
            let filename = match fields.get("filename") {
                Some(Value::Text(t)) => t.clone(),
                _ => return None,
            };
            let mime = match fields.get("mime") {
                Some(Value::Text(t)) => t.clone(),
                _ => "application/octet-stream".to_string(),
            };
            let bytes = match fields.get("bytes") {
                Some(Value::Bytes(b)) => b.clone(),
                Some(Value::Text(t)) => t.clone().into_bytes(),
                _ => Vec::new(),
            };
            Some(Attachment {
                filename,
                mime,
                bytes,
            })
        })
        .collect()
}
