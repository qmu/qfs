//! `qfs-driver-gmail` — the **first real `Driver`** (blueprint §6, E4 t20) and a flagship of the
//! thesis (§1): the Gmail mailbox is exposed as one mount at `/mail`
//! under the uniform VFS + pipe-SQL DSL. It maps the mailbox onto the **Append/log archetype**:
//! **labels = directories, messages = files, attachments = nested entries**, addressed by `id:`
//! / `id:thread:<id>`.
//!
//! ## Surface
//! - [`GmailDriver`] — the introspective `Driver`: `mount()` = `/mail`, per-path archetype
//!   ([`Archetype::AppendLog`]) + the [`MailMessage`](schema::MailMessage) schema, **path-keyed**
//!   capabilities (`/mail/<label>` = `Select|Update|Remove`; `/mail/drafts` =
//!   `Insert|Upsert|Select`; a message is read/trash-only), the irreversible `mail.send`
//!   procedure, the pure `SEND` prelude alias, and `Partial { where_, limit }` pushdown (the
//!   Gmail search `q=`). `version_support` is [`VersionSupport::None`] for now.
//! - [`GmailApplier`] — the synchronous apply leg `applier()` returns and the
//!   [`qfs_runtime::SharedApplier`] the bridge drives under `COMMIT`.
//! - [`gmail_apply_driver`] — wraps the applier in a [`qfs_runtime::PlanApplierBridge`] ready to
//!   `register` into a `DriverRegistry` under the driver id `mail`, so a plan over `/mail`
//!   executes end-to-end through the t10 interpreter.
//!
//! ## Purity invariant (blueprint §3)
//! Every write — `INSERT INTO /mail/drafts`, `UPSERT`, `REMOVE` (trash), `CALL mail.send` —
//! **constructs a `Plan` node and performs no I/O during planning**; only [`GmailApplier`] under
//! `COMMIT` touches the Gmail API. The introspective methods are pure data (no `&mut self`, no
//! future). `PREVIEW` calls **no** client method (the mock asserts zero calls).
//!
//! ## Auth + multi-account + least privilege (blueprint §8)
//! Auth (tokens, refresh, multi-account) comes from t19. This driver requests **only**
//! [`GMAIL_MODIFY_SCOPE`] + [`GMAIL_COMPOSE_SCOPE`] (no full `mail.google.com`, no permanent
//! delete). The bearer is injected by the t19 `GoogleApiClient` and lives behind a
//! [`qfs_secrets::Secret`]; it is **never** logged, never in a DTO, never in plan output or a
//! [`GmailError`]. Multi-account is the t19 base: one `GoogleApiClient` per account email; the
//! driver is account-agnostic (the resolved account is bound at client construction).
//!
//! ## No vendor leak (blueprint §11)
//! Gmail JSON is translated into owned DTOs at the [`client`] boundary; the `Driver` surface and
//! the `Plan` carry zero google types. The HTTP client is behind the mockable [`GmailClient`]
//! trait so it mocks in tests (no live Gmail, no network) and `reqwest` stays in
//! `qfs-driver-http` — this crate rides the t19 `HttpExchange` seam.
//!
//! ## Deferred for t20 (named parks)
//! - **Attachment bytes fetch — wired (t92).** A listing row carries attachment *metadata* only
//!   ([`AttachmentMeta`]); the on-demand bytes fetch reads the single node
//!   [`MailPath::Attachment`](path::MailPath) (`/mail/<label>/<msg>/<att>`, its `Select`
//!   capability) via [`GmailClient::get_attachment`](client::GmailClient::get_attachment) —
//!   attachments.get bytes (base64url-decoded at the client seam) paired with the message part's
//!   `filename`/`mime`/`size` into a `content`-bearing row. gmail-ftp `get id:att:<msg>:<att>` parity.
//! - **`historyId` / `@version` incremental sync — parked.** [`VersionSupport::None`]; deferred
//!   to the E7 trigger sibling.
//! - **Live create→send→trash smoke test — parked.** The suite is mock-only (no env-gated live
//!   Gmail test); the opt-in smoke test is tracked as a follow-up acceptance item.

#![forbid(unsafe_code)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

mod applier;
pub mod client;
mod effect;
mod error;
pub mod mime;
mod path;
pub mod query;
pub mod read;
mod schema;

use std::sync::Arc;

use qfs_driver::{
    AliasFn, Archetype, Capabilities, Driver, NodeDesc, Param, Path, ProcSig, PushdownProfile,
    Verb, VersionSupport,
};
use qfs_plan::{Plan, PlanApplier};
use qfs_runtime::PlanApplierBridge;
use qfs_types::{ColumnType, RowBatch};

pub use applier::GmailApplier;
pub use client::{
    DraftRef, GmailClient, GoogleApiGmailClient, MessageIdPage, MockGmailClient, RecordedCall,
};
pub use effect::{
    GmailEffect, ADD_LABELS_COL, BODY_COL, CC_COL, DRAFT_ID_COL, NAME_COL, REMOVE_LABELS_COL,
    SUBJECT_COL, TO_COL,
};
pub use error::GmailError;
pub use path::{MailPath, DRAFTS_SEGMENT, MOUNT};
pub use read::read_rows;
pub use schema::{Attachment, AttachmentMeta, MailDraft, MailMessage, ReplyContext};

/// The least-privilege **modify** scope — list/search/read, trash, and label modify. NOT the
/// full `https://mail.google.com/` scope and NOT a delete scope (blueprint §8 blast radius).
pub const GMAIL_MODIFY_SCOPE: &str = "https://www.googleapis.com/auth/gmail.modify";
/// The least-privilege **compose** scope — create drafts and send. The `mail.send` procedure
/// declares this in `requires_scopes` so the server `POLICY` can reason about blast radius.
pub const GMAIL_COMPOSE_SCOPE: &str = "https://www.googleapis.com/auth/gmail.compose";

/// The Gmail driver (blueprint §6). Owns the synchronous [`GmailApplier`] the contract returns from
/// `applier()`, plus the declared procedures, prelude, and pushdown profile. Construct with
/// [`GmailDriver::new`], injecting the [`GmailClient`] (auth is injected there at construction —
/// the real client wraps a per-account `GoogleApiClient`; never on the contract surface).
pub struct GmailDriver {
    applier: GmailApplier,
    pushdown: PushdownProfile,
    procs: Vec<ProcSig>,
    prelude: Vec<AliasFn>,
}

impl GmailDriver {
    /// Build a Gmail driver over `client`. In production `client` is a
    /// [`GoogleApiGmailClient`] wrapping a per-account `GoogleApiClient` (bearer + refresh-on-
    /// 401); in tests it is a [`MockGmailClient`].
    #[must_use]
    pub fn new(client: Arc<dyn GmailClient>) -> Self {
        Self {
            applier: GmailApplier::new(client),
            // Gmail search `q=` covers many WHERE predicates and a result cap (maxResults);
            // ordering / projection / joins stay local (blueprint §7). Residual predicates combine
            // locally — see `query::build_query`.
            pushdown: PushdownProfile::Partial {
                where_: true,
                project: false,
                limit: true,
                order: false,
                join: false,
                aggregate: false,
                distinct: false,
                group_by: false,
            },
            procs: vec![
                // The irreducible, irreversible state transition (blueprint §3/§8).
                ProcSig::new("send")
                    .with_params(vec![
                        Param::new("to", ColumnType::Text),
                        Param::new("subject", ColumnType::Text),
                        Param::new("body", ColumnType::Text),
                        // Attach files on a create-then-send (`call mail.send(..., attachments =>
                        // [ { filename, mime, bytes } ])`) — the same `Array(Struct{..})` shape an
                        // `INSERT INTO /mail/drafts` `attachments` column carries.
                        Param::new("attachments", schema::attachments_param_type()),
                    ])
                    .irreversible(true)
                    .requires_scopes(vec![GMAIL_COMPOSE_SCOPE.to_string()]),
                // Reply into a parent message's thread — a **reversible** reply draft (addressed at
                // the parent, `<parent> |> call mail.reply(body => …)`). `to`/`cc`/`subject` are
                // optional overrides (default: the parent's `From` / `Re: <subject>`). Sending it is
                // the existing `/mail/drafts/<id> |> call mail.send`, which threads because the draft
                // carries the thread id (blueprint §3/§8 — no second threaded-send path).
                ProcSig::new("reply")
                    .with_params(vec![
                        Param::new("body", ColumnType::Text),
                        Param::new("to", ColumnType::Text),
                        Param::new("cc", ColumnType::Text),
                        Param::new("subject", ColumnType::Text),
                        // Attach files on a reply — same `Array(Struct{filename, mime, bytes})`.
                        Param::new("attachments", schema::attachments_param_type()),
                    ])
                    .irreversible(false)
                    .requires_scopes(vec![GMAIL_COMPOSE_SCOPE.to_string()]),
            ],
            // The pure prelude alias: `SEND(d) = d |> CALL mail.send` (blueprint §3).
            prelude: vec![AliasFn::new("SEND", "mail.send")],
        }
    }

    /// Borrow the synchronous applier (e.g. to drive a `qfs_plan::commit` directly, or to build
    /// the runtime bridge).
    #[must_use]
    pub fn gmail_applier(&self) -> &GmailApplier {
        &self.applier
    }

    /// **Purity invariant proof (t40, blueprint §3/§7).** Desugar the `SEND` prelude alias for a draft
    /// node into the [`qfs_plan::Plan`] it represents — a single `CALL mail.send` effect node —
    /// **performing no I/O**. This is the in-code witness for `docs/language.md`'s purity section:
    /// `SEND(d) = d |> CALL mail.send` is a *pure function returning a Plan*; nothing is sent until
    /// a separate `COMMIT` applies the plan (the applier seam, never reached here). No credential
    /// is resolved, no socket is opened — building the plan only allocates owned data.
    ///
    /// ```
    /// use qfs_driver_gmail::GmailDriver;
    /// use qfs_plan::EffectKind;
    ///
    /// // SEND(d) desugars to a plan with exactly one CALL mail.send node — and runs NO I/O.
    /// let plan = GmailDriver::send_alias_plan("id:draft-1");
    /// assert_eq!(plan.nodes().len(), 1);
    /// match &plan.nodes()[0].kind {
    ///     EffectKind::Call(proc) => assert_eq!(proc.0, "mail.send"),
    ///     other => panic!("SEND must desugar to a CALL node, got {other:?}"),
    /// }
    /// // mail.send is irreversible (blueprint §8) — surfaced on the node so PREVIEW can warn.
    /// assert!(plan.nodes()[0].irreversible);
    /// assert!(plan.is_irreversible());
    /// ```
    #[must_use]
    pub fn send_alias_plan(draft: &str) -> qfs_plan::Plan {
        use qfs_plan::{DriverId, EffectKind, EffectNode, NodeId, ProcId, Target, VfsPath};
        // The `SEND` alias (declared in the prelude as `SEND -> mail.send`) desugars to a single
        // `CALL mail.send` effect on the draft target. The procedure is declared irreversible, so
        // the node carries that bit (the planner's per-proc irreversibility, set explicitly).
        let target = Target::new(DriverId::new("mail"), VfsPath::new(draft));
        let node = EffectNode::new(
            NodeId(0),
            EffectKind::Call(ProcId::new("mail.send")),
            target,
        )
        .irreversible(true);
        qfs_plan::Plan::leaf(node)
    }

    /// The path-keyed capability set (blueprint §6):
    /// - `/mail/drafts` → `Insert|Upsert|Select` (create/replace/list drafts). NO collection-level
    ///   `Remove`: the applier trashes a draft only by its id (`remove /mail/drafts/<id>`, a
    ///   message node), never the set-wide `remove /mail/drafts where …` — advertising `Remove`
    ///   here would let a preview promise a trash the commit then rejects (`decode_trash` services
    ///   only `id:<msg>`/`id:thread:`). So the collection does not claim it (blueprint §6 describe-honesty).
    /// - `/mail/<label>` → `Select|Update|Remove`. `Update`/`Remove` are serviced for the **exact**
    ///   `where id == '<msgid>'` form only (the applier resolves the one named message); a set-wide
    ///   predicate write is refused closed (Gmail's `q=` is lossy — enumerating it could trash the
    ///   wrong mail). The verb IS supported, so the claim is honest (ticket 20260704155500).
    /// - a message (`id:<msg>` / `/mail/<label>/<msg>`) → `Select|Update|Remove` (read, relabel by
    ///   the message node directly, trash). `Update` here relabels the single message with no key
    ///   ambiguity — the committable relabel form.
    /// - a thread (`id:thread:<id>`) → `Remove` (trash only).
    /// - the `/mail` root → `Ls|Select` (list labels).
    /// - anything else → the empty set (every verb rejected at the parse-time gate).
    fn caps_for(&self, path: &Path) -> Capabilities {
        match MailPath::parse(path) {
            Ok(MailPath::Drafts) => {
                Capabilities::from_verbs(&[Verb::Insert, Verb::Upsert, Verb::Select])
            }
            Ok(MailPath::Label { .. }) => {
                Capabilities::from_verbs(&[Verb::Select, Verb::Update, Verb::Remove])
            }
            Ok(MailPath::Message { .. }) => {
                Capabilities::from_verbs(&[Verb::Select, Verb::Update, Verb::Remove])
            }
            // A single draft (`/mail/drafts/<draft-id>`) reads (`Select`) and sends (the `mail.send`
            // procedure — a `CALL`, not a verb cap). Draft discard (`drafts.delete`) is a named
            // follow-up: it is irreversible by draft id, distinct from a message trash, so it is not
            // wired onto this node here rather than shipping a message-id trash that a draft id breaks.
            Ok(MailPath::Draft { .. }) => Capabilities::from_verbs(&[Verb::Select]),
            Ok(MailPath::Thread { .. }) => Capabilities::from_verbs(&[Verb::Remove]),
            Ok(MailPath::Root) => Capabilities::from_verbs(&[Verb::Ls, Verb::Select]),
            Ok(MailPath::Labels) => Capabilities::from_verbs(&[Verb::Insert]),
            Ok(MailPath::Attachment { .. }) => Capabilities::from_verbs(&[Verb::Select]),
            // The reply append-log takes an `INSERT` (thread a reply onto the parent). Reversible
            // like `mail.reply` — it only drafts; nothing sends until a separate `mail.send`.
            Ok(MailPath::Replies { .. }) => Capabilities::from_verbs(&[Verb::Insert]),
            Err(_) => Capabilities::none(),
        }
    }
}

impl Driver for GmailDriver {
    fn mount(&self) -> &str {
        MOUNT
    }

    fn describe(&self, path: &Path) -> Result<NodeDesc, qfs_driver::CfsError> {
        // Every /mail node is the Append/log archetype. The ROOT lists labels (the directory view,
        // `ls /mail`), so it reports the label-listing schema; every other node reads messages, so
        // it reports the canonical MailMessage schema. Pure: builds data, no I/O.
        let schema = match MailPath::parse(path) {
            // The root lists labels; the label-management collection takes a `name` (its INSERT
            // column) — both report the `name` label schema. Every other node reads messages.
            Ok(MailPath::Root | MailPath::Labels) => schema::label_listing_schema(),
            // An attachment node reads ONE file's bytes: the scan returns
            // filename/mime/size/content, so describe must advertise the SAME columns (not the
            // message-listing schema). Without this, a cross-service
            // `SELECT filename AS name, mime AS mime_type, content AS bytes FROM /mail/<msg>/<att>`
            // fails column resolution at plan time — the columns line up, by design, with the Drive
            // upload row shape (name/mime_type/bytes) for a one-statement attachment→Drive transfer.
            Ok(MailPath::Attachment { .. }) => schema::attachment_read_schema(),
            // The reply append-log advertises the reply WRITE columns (body/to/cc/subject/
            // attachments) so a cross-service `… |> insert into /mail/<msg>/replies` resolves its
            // projection at plan time — the sibling of the attachment node's read-schema arm above.
            Ok(MailPath::Replies { .. }) => schema::reply_write_schema(),
            _ => MailMessage::schema(),
        };
        // The LABEL TREE is navigable (§9 enumerable-children): `/mail`'s children are labels and a
        // label's are messages — both LOCATIONS (a message addresses further: `/mail/<label>/<msg>/
        // <att>`), so `cd /mail` then `cd inbox` is the gmail-ftp reading the typed-path space
        // promises. Every other node (a message, a draft, an attachment, the reply log) is a leaf
        // whose children are rows.
        //
        // Note the archetype deliberately stays `AppendLog` for all of them: mail rows ARE an append
        // log, and `ls` is archetype-typed (§5.1) — calling the root a `BlobNamespace` would make
        // `ls /mail` lower to the blob `name/size/is_dir/modified` projection and fail against the
        // label schema. Navigability is an ORTHOGONAL fact about children, which is precisely why it
        // is its own describe-contract field rather than an archetype.
        let navigable = matches!(
            MailPath::parse(path),
            Ok(MailPath::Root | MailPath::Label { .. })
        );
        let desc = NodeDesc::new(Archetype::AppendLog, schema).navigable(navigable);
        // 番地の鍵の宣言 (plan.md, settled 2026-07-18): the driver declares the identity that
        // selects a child. The root/label-collection lists LABELS — the label name is the
        // containment segment itself (`/mail/INBOX`). A label's rows are MESSAGES selected by
        // `id` (`/mail/INBOX/@<id>` lowers to `where id == …`). A message and its
        // attachment/reply leaves declare no child — relation segments are a later phase.
        let desc = match MailPath::parse(path) {
            Ok(MailPath::Root | MailPath::Labels) => desc.child_entry_name("name"),
            Ok(MailPath::Label { .. }) => desc.child_key(["id"]),
            _ => desc,
        };
        Ok(desc)
    }

    fn capabilities(&self, path: &Path) -> Capabilities {
        self.caps_for(path)
    }

    fn procedures(&self) -> &[ProcSig] {
        &self.procs
    }

    fn pushdown(&self) -> &PushdownProfile {
        &self.pushdown
    }

    fn prelude(&self) -> &[AliasFn] {
        &self.prelude
    }

    fn version_support(&self, _path: &Path) -> VersionSupport {
        // @version / historyId incremental sync is deferred to the E7 trigger sibling.
        VersionSupport::None
    }

    /// **Plan-time guard for a drafts write.** A draft's *read* schema (message rows: `id`,
    /// `thread_id`, `from`, `subject`, …) differs from its *compose* fields (`to`, `subject`,
    /// `body`, `attachments`), so a **positional** `insert into /mail/drafts values ('a','b','c')`
    /// has its column names filled from the read schema and never lands on `to` — the applier then
    /// fails at COMMIT with "draft has no 'to' recipients" while the PREVIEW already claimed one
    /// effect. Rather than let preview and apply disagree, reject here at plan time when a drafts
    /// `INSERT`/`UPSERT` row carries no `to` column, naming the named-columns form. A well-formed
    /// named write (any column order, so long as it names `to`) returns `None` and takes the
    /// generic by-name lowering unchanged — drafts are decoded by column name, never by position
    /// (they are richer than a positional tuple: `cc`, `attachments`, and the `UPSERT` `draft_id`
    /// have no positional slot). A `FROM`-pipeline draft write has no literal row here and is
    /// checked by the applier decode.
    fn plan_write(
        &self,
        path: &Path,
        verb: Verb,
        args: &RowBatch,
        // The drafts lowering is INSERT/UPSERT-only (no WHERE), so the selector is unused.
        _selector: Option<&RowBatch>,
    ) -> Option<Result<Plan, qfs_driver::CfsError>> {
        if !matches!(MailPath::parse(path), Ok(MailPath::Drafts)) {
            return None;
        }
        if !matches!(verb, Verb::Insert | Verb::Upsert) {
            return None;
        }
        if args.schema.columns.iter().any(|c| c.name == TO_COL) {
            return None; // a named draft write — the generic by-name lowering handles it
        }
        Some(Err(qfs_driver::CfsError::InvalidPath {
            path: path.as_str().to_string(),
            reason: "a draft write needs named columns naming `to`: use \
                     `insert into /mail/drafts values (to, subject, body) (...)` — a positional \
                     VALUES row maps to the message read schema, which has no `to` column",
        }))
    }

    /// **Plan-time guard for `mail.send`.** A send must resolve a concrete draft/recipient; this is
    /// checked HERE at plan time so `PREVIEW` and `COMMIT` agree (a `PREVIEW` never decodes the
    /// effect, so an apply-time-only refusal would let preview claim a send the commit then rejects
    /// with a confusing `malformed INSERT … draft has no 'to'`). The resolution order mirrors
    /// [`GmailEffect::from_node`]'s `decode_call`: an **addressed draft node** (`/mail/drafts/<id>`),
    /// an explicit `draft_id` arg, or a non-empty `to`. Anything else is the byteless
    /// create-then-send — rejected here with the actionable send forms. A non-`mail.send` CALL, or a
    /// well-formed send, returns `None`/`Ok` and lowers unchanged.
    fn plan_call(
        &self,
        path: &Path,
        proc: &str,
        args: &RowBatch,
    ) -> Option<Result<(), qfs_driver::CfsError>> {
        // `mail.reply` must be addressed at a parent MESSAGE node and carry a `body` — guarded here
        // so PREVIEW and COMMIT agree (a PREVIEW never decodes the effect, so an apply-time-only
        // refusal would let preview claim a reply the commit then rejects). An addressed message +
        // a non-empty `body` resolve; anything else is refused with the actionable reply form.
        if proc == "mail.reply" {
            let addressed_msg = matches!(MailPath::parse(path), Ok(MailPath::Message { .. }));
            if addressed_msg && arg_is_nonempty(args, BODY_COL) {
                return Some(Ok(()));
            }
            return Some(Err(qfs_driver::CfsError::InvalidPath {
                path: path.as_str().to_string(),
                reason: "mail.reply must be addressed at a parent message and carry a body: \
                         `/mail/<label>/<msg> |> call mail.reply(body => …)` (or `id:<msg> |> …`) \
                         — a bare or draft-addressed reply resolves no parent thread",
            }));
        }
        if proc != "mail.send" {
            return None;
        }
        let addressed_draft = matches!(MailPath::parse(path), Ok(MailPath::Draft { .. }));
        if addressed_draft || arg_is_nonempty(args, DRAFT_ID_COL) || arg_is_nonempty(args, TO_COL) {
            return Some(Ok(()));
        }
        Some(Err(qfs_driver::CfsError::InvalidPath {
            path: path.as_str().to_string(),
            reason: "mail.send needs an existing draft or recipients: address a draft \
                     (`/mail/drafts/<id> |> call mail.send`) or pass recipients \
                     (`call mail.send(to => …, subject => …, body => …)`) — a bare \
                     `/mail/drafts |> call mail.send` resolves no draft",
        }))
    }

    fn applier(&self) -> &dyn PlanApplier {
        &self.applier
    }
}

/// Whether `args`' first row carries a non-empty `Text` value in the named column — the plan-time
/// echo of [`effect`]'s `text_col`, used by [`GmailDriver::plan_call`] to tell a resolvable send
/// (an explicit `draft_id`, or `to` recipients) from the byteless create-then-send.
fn arg_is_nonempty(args: &RowBatch, name: &str) -> bool {
    let Some(idx) = args.schema.columns.iter().position(|c| c.name == name) else {
        return false;
    };
    matches!(
        args.rows.first().and_then(|r| r.values.get(idx)),
        Some(qfs_types::Value::Text(t)) if !t.is_empty()
    )
}

/// Wrap a [`GmailDriver`]'s synchronous applier in the runtime [`PlanApplierBridge`], yielding
/// the async `ApplyDriver` ready to `register` into a `DriverRegistry` under the driver id
/// `mail`. A plan routed to `/mail` then executes end-to-end through the t10 interpreter, which
/// dispatches each effect to this bridge (and collapses the N+1 detail-fetch frontier).
#[must_use]
pub fn gmail_apply_driver(driver: &GmailDriver) -> PlanApplierBridge<GmailApplier> {
    PlanApplierBridge::new(Arc::new(driver.gmail_applier().clone()))
}

#[cfg(test)]
mod tests;
