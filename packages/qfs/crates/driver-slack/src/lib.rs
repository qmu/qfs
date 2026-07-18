//! `qfs-driver-slack` — the **Slack multi-archetype `Driver`** (blueprint §6, E4 t25). It mounts
//! Slack under `/slack/<ws>/...` as a path tree crossing the **Append/log** archetype (messages,
//! replies, reactions, DMs) with a **Blob/namespace** (files) and a **Relational** directory
//! (users) — each a per-node [`Archetype`](qfs_driver::Archetype) with a typed
//! [`Schema`](qfs_types::Schema) powering
//! `DESCRIBE`. It implements the t13 [`qfs_driver::Driver`] contract and reuses the t18/t24
//! reusable-HTTP-seam *shape* — Bearer (bot-token) injection, cursor pagination, 429/`Retry-After`
//! bounded retry on idempotent GETs only — over the **shared `qfs_http_core` HTTP DTOs + the single
//! redaction authority** through a local [`HttpTransport`](client::HttpTransport) seam (a structural
//! twin of t18's `HttpClient`). There is **no hand-rolled HTTP DTO** (the t19 redaction-drift token
//! leak stays closed) and the driver does **not** depend on `qfs-driver-http` as a crate: a
//! `qfs-runtime` consumer must stay a leaf (the dep-direction confinement test).
//!
//! ## The t18 BodyErrorRule (the reason t25 consumes it)
//! Slack returns **HTTP 200** with `{"ok":false,"error":"…"}` on an application error. The
//! [`BodyErrorRule`](client::BodyErrorRule) — opt-in on the config (default-off per t18; Slack turns
//! it on) — maps that to a structured **terminal** [`SlackError::Body`] **inside the seam**, so a
//! false success can never reach the interpreter.
//!
//! ## The genuinely-hard, genuinely-wasm part — `parse_event`
//! [`events::parse_event`] is **pure, no I/O**: it verifies `X-Slack-Signature` = HMAC-SHA256 over
//! `v0:timestamp:body` with a **constant-time** compare + timestamp-skew rejection (replay
//! defense), handles the `url_verification` challenge, surfaces `event_id` for dedupe, and
//! normalizes `message`/`reaction_added`/`app_mention`/`file_shared`. The HMAC-SHA256/SHA-256 is a
//! dependency-free, wasm-safe primitive ([`hmac`]). The pure subset (`parse_event` + the
//! introspective `Driver` surface) carries **zero** `qfs-runtime`/tokio dependency (`qfs-runtime` is
//! an optional, default-on `runtime` feature), so `--no-default-features --features events` compiles
//! for `wasm32-unknown-unknown` (the Workers `WEBHOOK` ingress, blueprint §10).
//!
//! ## Token safety (blueprint §8)
//! The bot token + the signing secret are [`qfs_secrets::Secret`]s read **only** at commit / verify
//! time — the bot token into an `Authorization: Bearer …` header the redacting
//! [`qfs_http_core::HttpRequest`] `Debug` hides, the signing secret only inside [`events`]. Neither
//! is ever logged, in a DTO/error, in a config `Debug`, or in a serialized plan (a planted-canary
//! test asserts this).
//!
//! ## Idempotency (blueprint §7)
//! `chat.postMessage` is **not** idempotent: a `client_msg_id` idempotency key is attached, the
//! at-least-once risk surfaced in `PREVIEW`, and an ambiguous post is **never** auto-retried.
//! `reactions.add`/`pins.add` are naturally idempotent (already-reacted/-pinned is swallowed).
//! `pin`/`delete` are flagged **irreversible**.
//!
//! ## Channel/user id resolution is the applier's job (planning stays pure)
//! `#name`→`Cxxxx` is I/O performed by the applier at commit, never during planning — PREVIEW shows
//! the symbolic `#channel` (blueprint §3 purity invariant).
//!
//! ## Named parks (deferred)
//! - **Live Slack API + live token — surface present, no live test (t38).** Every test drives the
//!   mocked [`SlackClient`](client::SlackClient) seam; the real
//!   [`RestSlackClient`](client::RestSlackClient) over the reqwest transport is construction-checked
//!   but never sent over a socket here.
//! - **Trigger/webhook registration + dispatch (E7).** This driver *produces* normalized events
//!   ([`events::parse_event`]); the server consumes them. The `CREATE TRIGGER`/`CREATE WEBHOOK`
//!   ingress route is E7/t34.
//! - **Richer message pushdown (E3).** Only `oldest/latest/limit` push down here; everything else
//!   is a truthful residual.

#![forbid(unsafe_code)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

mod applier;
pub mod client;
pub mod dto;
pub mod effect;
mod error;
pub mod events;
/// The crypto primitives `parse_event` verifies the `X-Slack-Signature` with (HMAC-SHA256 over
/// `v0:timestamp:body` + a constant-time compare, blueprint §8). t34 single-sourced these into the
/// shared pure leaf `qfs-crypto-core` (deleting this crate's former private `src/hmac.rs` copy);
/// this re-export keeps the public `qfs_driver_slack::hmac::*` path stable while the one
/// implementation now lives in — and is vector-pinned by — the shared leaf.
pub use qfs_crypto_core as hmac;
pub mod path;
pub mod procs;
pub mod pushdown;
pub mod read;
pub mod schema;

use std::sync::Arc;

use qfs_driver::{
    AliasFn, Capabilities, Driver, NodeDesc, Path, ProcSig, PushdownProfile, Verb, VersionSupport,
};
use qfs_plan::PlanApplier;
use qfs_secrets::CredentialKey;

pub use applier::SlackApplier;
pub use client::{
    BodyErrorRule, HttpTransport, MockSlackClient, RecordedCall, RestSlackClient, SlackApiCall,
    SlackClient, TransportError,
};
pub use dto::{FileDto, MessageDto, ReactionDto, UserDto};
pub use effect::SlackEffect;
pub use error::SlackError;
pub use events::{
    parse_event, verify_signature, EventError, EventHeaders, SlackEvent, SlackEventKind,
    SlackInbound,
};
pub use path::{ChannelRef, NodeKind, SlackNode, SlackPath, UserRef, MOUNT};
pub use read::{read_rows, ReadPlan};
pub use schema::{archetype_for, schema_for};

/// The per-workspace Slack configuration — an owned DTO deserialized from qfs config, **one block
/// per workspace `<ws>`** (blueprint §6). The bot token + signing secret are [`CredentialKey`]
/// indirections into the [`qfs_secrets`] store; the raw values are resolved only at commit / verify
/// time, **never** stored in this struct and **never** in its `Debug` (the keys are selectors, not
/// values, so the derived `Debug` is already secret-free).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct SlackWsConfig {
    /// The workspace path segment (`<ws>`) this config answers for.
    pub workspace: String,
    /// The Slack `team_id` (`Txxxx`) — a selector, not a secret.
    pub team_id: String,
    /// The credential key resolving the **bot token** at commit (blueprint §8 — a selector, never the
    /// token value).
    pub token: CredentialKey,
    /// The credential key resolving the **signing secret** at verify time (for inbound events).
    pub signing_secret: CredentialKey,
    /// Whether the [`BodyErrorRule`] is on (Slack sets this `true`; the t18 default is off).
    pub body_error_rule: bool,
}

impl SlackWsConfig {
    /// Build a workspace config. `body_error_rule` is on for Slack (it signals errors via the
    /// HTTP-200 `ok:false` envelope).
    #[must_use]
    pub fn new(
        workspace: impl Into<String>,
        team_id: impl Into<String>,
        token: CredentialKey,
        signing_secret: CredentialKey,
    ) -> Self {
        Self {
            workspace: workspace.into(),
            team_id: team_id.into(),
            token,
            signing_secret,
            body_error_rule: true,
        }
    }

    /// The [`BodyErrorRule`] this config selects.
    #[must_use]
    pub const fn rule(&self) -> BodyErrorRule {
        if self.body_error_rule {
            BodyErrorRule::On
        } else {
            BodyErrorRule::Off
        }
    }
}

/// The Slack driver (blueprint §6). Owns the synchronous [`SlackApplier`] the contract returns from
/// `applier()`, plus the declared procedures, the prelude alias, and the pushdown profile.
/// Construct with [`SlackDriver::new`], injecting the [`SlackClient`] (auth is injected there at
/// construction — the real client resolves the bot token from the secret store; never on the
/// contract surface).
pub struct SlackDriver {
    applier: SlackApplier,
    pushdown: PushdownProfile,
    procs: Vec<ProcSig>,
    prelude: Vec<AliasFn>,
}

impl SlackDriver {
    /// Build a Slack driver over `client`. In production `client` is a [`RestSlackClient`] wrapping
    /// the transport + the secret store; in tests it is a [`MockSlackClient`].
    #[must_use]
    pub fn new(client: Arc<dyn SlackClient>) -> Self {
        Self {
            applier: SlackApplier::new(client),
            // Slack's history endpoints filter on a `oldest/latest` time window and cap with
            // `limit`; richer pushdown is E3. Ordering/projection/joins stay local. The residual
            // keeps exact correctness (blueprint §7).
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
            procs: procs::procedures(),
            prelude: procs::prelude(),
        }
    }

    /// Borrow the synchronous applier (e.g. to drive a `qfs_plan::commit` directly, or to build the
    /// runtime bridge).
    #[must_use]
    pub fn slack_applier(&self) -> &SlackApplier {
        &self.applier
    }

    /// The node-keyed capability set (blueprint §6), gating verbs at parse time. The [`NodeKind`] decides
    /// the verb set:
    /// - `messages`  → `SELECT|INSERT|REMOVE` (tail / post / delete-by-ts).
    /// - `replies`   → `SELECT|INSERT` (read a thread / post a reply).
    /// - `reactions` → `INSERT|REMOVE` (add / remove a reaction).
    /// - `dms`       → `SELECT|INSERT`.
    /// - `files`     → `LS|CP|RM` (the blob namespace).
    /// - `users`     → `SELECT` (read-mostly directory; `INSERT`/`UPDATE` rejected at the gate).
    /// - an unknown node → the empty set (every verb rejected).
    fn caps_for(path: &Path) -> Capabilities {
        let Ok(parsed) = SlackPath::parse(path) else {
            return Capabilities::none();
        };
        match parsed.kind() {
            NodeKind::Messages => {
                Capabilities::from_verbs(&[Verb::Select, Verb::Insert, Verb::Remove])
            }
            NodeKind::Replies | NodeKind::Dms => {
                Capabilities::from_verbs(&[Verb::Select, Verb::Insert])
            }
            NodeKind::Reactions => Capabilities::from_verbs(&[Verb::Insert, Verb::Remove]),
            NodeKind::Files => match parsed.node {
                // A single file node: read its content (`Select`) and delete it (`Remove` —
                // `files.delete`, irreversible). `remove /slack/<ws>/files/<id>` is the taught
                // detach form; the id rides in the path (see `decode_remove`).
                SlackNode::File { .. } => Capabilities::from_verbs(&[Verb::Select, Verb::Remove]),
                // The workspace file namespace: list, upload (`cp`, and the equivalent
                // `INSERT`/`UPSERT INTO /slack/<ws>/files` bytes-upload forms), and delete.
                SlackNode::Files => Capabilities::from_verbs(&[
                    Verb::Ls,
                    Verb::Cp,
                    Verb::Insert,
                    Verb::Upsert,
                    Verb::Rm,
                ]),
                // A channel/DM-scoped file listing is a read-only view of the files shared there —
                // `Ls` only; `cp`/`rm` belong to the workspace-global `files` namespace.
                SlackNode::ChannelFiles { .. } | SlackNode::DmFiles { .. } => {
                    Capabilities::from_verbs(&[Verb::Ls])
                }
                _ => Capabilities::none(),
            },
            NodeKind::Users => Capabilities::from_verbs(&[Verb::Select]),
        }
    }
}

impl Driver for SlackDriver {
    fn mount(&self) -> &str {
        MOUNT
    }

    fn describe(&self, path: &Path) -> Result<NodeDesc, qfs_driver::CfsError> {
        // Each Slack node carries its own archetype (the multi-archetype property) + the node
        // kind's canonical schema. Pure: builds data, no I/O. A path that names no node is an
        // honest structured InvalidPath.
        let parsed = SlackPath::parse(path).map_err(|_| qfs_driver::CfsError::InvalidPath {
            path: path.as_str().to_string(),
            reason: "not a /slack/<ws>/... node",
        })?;
        let kind = parsed.kind();
        let schema = match parsed.node {
            SlackNode::File { .. } => crate::dto::FileDto::content_schema(),
            _ => schema_for(kind),
        };
        let desc = NodeDesc::new(archetype_for(kind), schema);
        // 番地の鍵の宣言: a message log's rows are selected by `ts` (the same identity the
        // containment spelling `…/messages/<ts>/replies` already uses); the user directory
        // by `id`. Reactions/files declare no child.
        let desc = match kind {
            NodeKind::Messages | NodeKind::Replies | NodeKind::Dms => desc.child_key(["ts"]),
            NodeKind::Users => desc.child_key(["id"]),
            _ => desc,
        };
        Ok(desc)
    }

    fn capabilities(&self, path: &Path) -> Capabilities {
        Self::caps_for(path)
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

    fn version_support(&self, path: &Path) -> VersionSupport {
        // Messages are edit-by-`ts` (a point identifies a message) but Slack has no history rewind:
        // Snapshot. Other nodes carry no version coordinate.
        match SlackPath::parse(path).map(|p| p.kind()) {
            Ok(NodeKind::Messages | NodeKind::Replies | NodeKind::Dms) => VersionSupport::Snapshot,
            _ => VersionSupport::None,
        }
    }

    fn applier(&self) -> &dyn PlanApplier {
        &self.applier
    }
}

/// Wrap a [`SlackDriver`]'s synchronous applier in the runtime
/// [`PlanApplierBridge`](qfs_runtime::PlanApplierBridge), yielding the async `ApplyDriver` ready to
/// `register` into a `DriverRegistry` under the driver id `slack`. A plan routed to `/slack` then
/// executes end-to-end through the t10 interpreter, which dispatches each effect to this bridge.
///
/// Gated behind the default-on `runtime` feature so the pure `parse_event` + introspection subset
/// builds for `wasm32-unknown-unknown` without pulling `qfs-runtime`/tokio.
#[cfg(feature = "runtime")]
#[must_use]
pub fn slack_apply_driver(driver: &SlackDriver) -> qfs_runtime::PlanApplierBridge<SlackApplier> {
    qfs_runtime::PlanApplierBridge::new(Arc::new(driver.slack_applier().clone()))
}

#[cfg(test)]
mod tests;
