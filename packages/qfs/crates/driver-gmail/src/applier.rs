//! [`GmailApplier`] — the Gmail driver's synchronous apply leg (blueprint §7). It is the lone
//! impure seam the introspective [`crate::GmailDriver`] hands back via `applier()`, and the
//! [`qfs_runtime::SharedApplier`] the runtime's [`qfs_runtime::PlanApplierBridge`] drives under
//! `COMMIT`.
//!
//! Stateless across the call: it holds the [`GmailClient`] behind an `Arc` and performs fresh
//! Gmail API I/O on every call — so it implements `SharedApplier` (`&self` apply), the
//! statelessness contract the bridge requires. Each effect is decoded to a [`GmailEffect`] and
//! dispatched to the client; the token is wholly behind the client (t19), never here.
//!
//! ## Idempotency / recovery (blueprint §7)
//! `mail.send` is not naturally idempotent. The de-dupe key is a draft id: when `mail.send`
//! carries a draft id we send *that* draft (a retry re-sends the same draft id, not a fresh
//! message); when it carries draft content we **create a draft first**, then send it by id, so a
//! mid-send crash leaves a recoverable draft rather than a possible duplicate send. `UPSERT`
//! drafts are retry-safe (PUT by id). Trash + send are flagged irreversible upstream, so the
//! runtime never auto-retries them.

use std::sync::Arc;

use qfs_plan::{AppliedEffect, ApplyError, EffectNode, PlanApplier};
use qfs_runtime::{EffectError, EffectOutput, SharedApplier};

use crate::client::GmailClient;
use crate::effect::{draft_raw, GmailEffect};
use crate::error::GmailError;
use crate::schema::{MailDraft, ReplyContext};

/// The synchronous Gmail apply leg. Holds the [`GmailClient`] (the real auth-bearing client in
/// production, an in-memory mock in tests) behind an `Arc` so the leg is cheap to clone for the
/// runtime bridge and safe to share across blocking apply threads.
#[derive(Clone)]
pub struct GmailApplier {
    client: Arc<dyn GmailClient>,
}

impl GmailApplier {
    /// Build an applier over `client`.
    #[must_use]
    pub fn new(client: Arc<dyn GmailClient>) -> Self {
        Self { client }
    }

    /// Apply one decoded [`GmailEffect`], returning the affected count. The single place Gmail
    /// API I/O happens.
    fn apply_effect(&self, effect: &GmailEffect) -> Result<u64, GmailError> {
        match effect {
            GmailEffect::CreateDraft { draft } => {
                let raw = draft_raw(draft)?;
                self.client.create_draft(&raw, draft.thread_id())?;
                Ok(1)
            }
            GmailEffect::CreateLabel { name } => {
                self.client.create_label(name)?;
                Ok(1)
            }
            GmailEffect::UpsertDraft { id, draft } => {
                let raw = draft_raw(draft)?;
                self.client.upsert_draft(id, &raw, draft.thread_id())?;
                Ok(1)
            }
            GmailEffect::Reply {
                parent,
                to,
                cc,
                subject,
                body,
                attachments,
            } => {
                let draft =
                    self.build_reply_draft(parent, to, cc, subject.as_deref(), body, attachments)?;
                self.client
                    .create_draft(&draft_raw(&draft)?, draft.thread_id())?;
                Ok(1)
            }
            GmailEffect::ModifyLabels {
                message,
                add,
                remove,
            } => {
                self.client.modify_labels(message, add, remove)?;
                Ok(1)
            }
            GmailEffect::TrashMessage { id } => {
                self.client.trash_message(id)?;
                Ok(1)
            }
            GmailEffect::TrashThread { id } => {
                self.client.trash_thread(id)?;
                Ok(1)
            }
            GmailEffect::Send { draft_id, draft } => {
                // Resolve a draft id to send: an existing id (retry-safe), or create one first.
                let id = match draft_id {
                    Some(id) => id.clone(),
                    None => {
                        let draft = draft.as_ref().ok_or(GmailError::Mime {
                            reason: "mail.send carried neither a draft id nor draft content",
                        })?;
                        let raw = draft_raw(draft)?;
                        self.client.create_draft(&raw, draft.thread_id())?
                    }
                };
                self.client.send_draft(&id)?;
                Ok(1)
            }
        }
    }

    /// Resolve a `mail.reply` against its parent message (the impure read at COMMIT) into the
    /// threaded reply [`MailDraft`]: fetch the parent, take its thread id + `Message-Id`, default
    /// `to` to the parent's `From` and `subject` to `Re: <parent subject>` unless overridden. Fails
    /// closed (no panic, secret-free) if the parent resolves no thread id or `Message-Id` — a reply
    /// with no thread to join is a caller error, not a bare-header send.
    fn build_reply_draft(
        &self,
        parent: &str,
        to: &[String],
        cc: &[String],
        subject: Option<&str>,
        body: &str,
        attachments: &[crate::schema::Attachment],
    ) -> Result<MailDraft, GmailError> {
        let msg = self.client.get_message(parent)?;
        if msg.thread_id.is_empty() || msg.message_id.is_empty() {
            return Err(GmailError::MalformedEffect {
                verb: "CALL",
                path: format!("id:{parent}"),
                reason: "mail.reply cannot thread: the parent message has no resolvable thread id \
                         or Message-Id"
                    .to_string(),
            });
        }
        let to = if to.is_empty() {
            vec![msg.from.clone()]
        } else {
            to.to_vec()
        };
        let subject = subject
            .map(str::to_string)
            .unwrap_or_else(|| reply_subject(&msg.subject));
        Ok(MailDraft {
            id: None,
            to,
            cc: cc.to_vec(),
            subject,
            body: body.to_string(),
            attachments: attachments.to_vec(),
            reply: Some(ReplyContext {
                thread_id: msg.thread_id,
                references: msg.message_id,
            }),
        })
    }
}

/// The default reply subject: `Re: <subject>`, but idempotent — a subject already beginning with a
/// `Re:` prefix (case-insensitive, any surrounding space) is reused verbatim so a reply-to-a-reply
/// does not stack `Re: Re:`.
fn reply_subject(subject: &str) -> String {
    if subject
        .trim_start()
        .get(..3)
        .is_some_and(|p| p.eq_ignore_ascii_case("re:"))
    {
        subject.to_string()
    } else {
        format!("Re: {subject}")
    }
}

impl SharedApplier for GmailApplier {
    fn apply_shared(&self, node: &EffectNode) -> Result<EffectOutput, EffectError> {
        let effect = GmailEffect::from_node(node)?;
        let affected = self.apply_effect(&effect)?;
        Ok(EffectOutput::new(node.id, affected))
    }
}

impl PlanApplier for GmailApplier {
    /// The introspective `qfs_driver::Driver::applier()` seam (t09): a synchronous, `&mut self`
    /// apply leg. The Gmail applier is stateless, so this delegates to the same `&self` core as
    /// [`SharedApplier::apply_shared`]. The structured [`GmailError`] is reduced to the plan
    /// crate's owned `(id, reason)` shape — secret-free by construction — so no driver type
    /// leaks into `qfs-plan`.
    fn apply(&mut self, node: &EffectNode) -> Result<AppliedEffect, ApplyError> {
        let effect =
            GmailEffect::from_node(node).map_err(|e| ApplyError::new(node.id, e.to_string()))?;
        let affected = self
            .apply_effect(&effect)
            .map_err(|e| ApplyError::new(node.id, e.to_string()))?;
        Ok(AppliedEffect::new(node.id, affected))
    }
}
