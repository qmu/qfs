//! The **injected** session-source seam (the vendor-free analogue of `qfs-driver-sys`'s
//! `SysBackend`). The introspective driver half is pure; the impure read/append half — reading
//! Claude Code's on-disk session state and appending a steering instruction — is provided by the
//! `qfs` binary leaf through this trait, so this crate stays tokio-free and I/O-free (the
//! dep-direction guard: only the terminal binary opens a real path; the crate is wasm-buildable
//! and the purity proof stays green).
//!
//! No vendor type and no filesystem path crosses this boundary — only owned qfs DTOs (`RowBatch`,
//! a session-id `&str`, and the structured [`ClaudeError`]). The exact session-state format
//! (Claude Code's on-disk layout vs. a local IPC/API) and how the "current" session is identified
//! are the implementor's concern (the open product decision flagged in the ticket); this seam
//! names none of it.
//!
//! ## This seam calls no model (blueprint §15, decision W supersedes decision K)
//! Nothing here calls or hosts an LLM. `scan_sessions` reads metadata an agent runtime already
//! wrote; `append_instruction` hands the agent a steering message. The model runs elsewhere. (qfs
//! DOES call a model via `|> transform` — §15 / decision W — through `qfs-driver-transform`'s
//! injected provider, never this façade seam.)

use qfs_types::{RowBatch, Value};

/// The typed payload of an `INSERT INTO /claude/sessions` — a **session launch request**. Built
/// by the applier from the effect's row (the `VALUES` payload); handed to the [`SessionLauncher`]
/// seam. `cwd` + `prompt` are the required data; `name` is optional metadata the store records so
/// the launched session is later locatable by name.
///
/// ## Safety floor (blueprint §15; ticket 20260717010600)
/// These fields are **data**, never a shell line: the launcher passes each as a discrete process
/// **argument** (`Command::new(<configured binary>).arg(&cwd)…`), so nothing here is ever
/// interpolated into a shell. The binary path is *configuration*; `cwd`/`prompt`/`name` are the
/// only query-supplied inputs, and they cross the seam as owned strings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchSpec {
    /// The working directory the new session runs in (a path string; passed as an argument, not
    /// interpolated). Required.
    pub cwd: String,
    /// The initial prompt handed to the launched session (passed as an argument). Required.
    pub prompt: String,
    /// An optional human name recorded for the session (so it is locatable by name in a later
    /// `/claude/sessions` query). `None` when the `INSERT` omitted the `name` column.
    pub name: Option<String>,
}

impl LaunchSpec {
    /// Build a launch spec from the effect's `VALUES` row batch. The columns are named by the
    /// `INSERT (cwd, prompt [, name])` list (blueprint §7 lowering). Pure string work — no I/O,
    /// no spawn: extraction only, so it is safe to run in the credential-free applier before the
    /// impure launcher seam is reached.
    ///
    /// # Errors
    /// [`ClaudeError::MalformedEffect`] when the batch carries no row, or `cwd`/`prompt` is
    /// absent or not text (a launch with no working directory or no prompt is refused, never
    /// spawned with a blank).
    pub fn from_row_batch(batch: &RowBatch) -> Result<Self, ClaudeError> {
        let row = batch
            .rows
            .first()
            .ok_or_else(|| ClaudeError::MalformedEffect {
                reason: "session launch carries no VALUES row".to_string(),
            })?;
        let text = |col: &str| -> Option<String> {
            let idx = batch
                .schema
                .columns
                .iter()
                .position(|c| c.name.as_str() == col)?;
            match row.values.get(idx) {
                Some(Value::Text(s)) => Some(s.clone()),
                _ => None,
            }
        };
        let required = |col: &str| -> Result<String, ClaudeError> {
            text(col).ok_or_else(|| ClaudeError::MalformedEffect {
                reason: format!("session launch is missing the `{col}` column"),
            })
        };
        Ok(Self {
            cwd: required("cwd")?,
            prompt: required("prompt")?,
            name: text("name"),
        })
    }
}

/// The **launcher seam** the binary implements: spawn a new Claude Code session from a
/// [`LaunchSpec`] and return the new session id. This is the ONE irreversible write the sessions
/// relation admits (ticket 20260717010600); it lives in the applier lane behind this seam so the
/// pure driver crate stays I/O-free and hermetic tests drive a fake launcher (no real spawn, no
/// spend).
///
/// ## Fail-closed / opt-in
/// A binary with no configured launcher registers none, so the applier refuses a sessions `INSERT`
/// ([`ClaudeError::LaunchNotConfigured`]) rather than spawning nothing — the same posture as the
/// unconfigured session source.
///
/// ## Safety floor
/// The implementor MUST pass `cwd`/`prompt`/`name` as discrete process **arguments** and NEVER
/// build a shell line from them (no `sh -c`, no interpolation). The binary path is configuration,
/// the only trusted input.
pub trait SessionLauncher: Send + Sync {
    /// Spawn a background session for `spec`, returning the new session id the caller can address
    /// in `/claude/sessions`. WRITE side; **irreversible** (the turn runs, the spend lands).
    ///
    /// # Errors
    /// [`ClaudeError::LaunchFailed`] on a validation or spawn failure (a bad `cwd`, a missing
    /// binary, a non-zero exit) — the reason is a secret-free string, never a credential.
    fn launch(&self, spec: &LaunchSpec) -> Result<String, ClaudeError>;
}

/// The read/append seam the binary implements over the host's Claude Code session state
/// (decision W: a façade/append-log, never inference — the `|> transform` surface is where qfs
/// calls a model). The driver crate holds only `Arc<dyn SessionSource>`; the concrete on-disk
/// implementation lives binary-side.
pub trait SessionSource: Send + Sync {
    /// Scan the agent sessions into the owned [`RowBatch`] shaped by
    /// [`claude_node_schema`](crate::claude_node_schema)`(Sessions)`. READ side.
    ///
    /// MUST return task metadata only — never a token or a raw transcript secret (the schema has
    /// no such column). A source with no sessions returns an empty batch, not an error.
    ///
    /// # Errors
    /// [`ClaudeError::Source`] on an I/O / decode failure.
    fn scan_sessions(&self) -> Result<RowBatch, ClaudeError>;

    /// Scan a single session's instructions append-log into the owned [`RowBatch`] shaped by
    /// [`claude_node_schema`](crate::claude_node_schema)`(Instructions)`. READ side. An unknown
    /// session yields an empty batch (robust, not an error).
    ///
    /// # Errors
    /// [`ClaudeError::Source`] on an I/O / decode failure.
    fn scan_instructions(&self, session: &str) -> Result<RowBatch, ClaudeError>;

    /// Append one steering instruction to `session`'s log — the single gated WRITE in this driver
    /// (a REVERSIBLE append; steering an agent adds a message, it never removes state). Returns
    /// the affected row count (1 on success). WRITE side.
    ///
    /// The agent runtime connection this ultimately reaches is FAIL-CLOSED / opt-in: a binary with
    /// no configured session source registers no applier, so a `/claude` commit fails closed (no
    /// driver) rather than silently steering nothing.
    ///
    /// # Errors
    /// [`ClaudeError::UnknownSession`] if `session` does not exist, [`ClaudeError::MalformedEffect`]
    /// if the row carries no instruction text, or [`ClaudeError::Source`] on an I/O failure.
    fn append_instruction(&self, session: &str, row: &RowBatch) -> Result<u64, ClaudeError>;
}

/// A structured, **secret-free** error from the `/claude` session source (blueprint §6, AI-consumable).
/// Names a node/session/verb and a redacted detail — never a credential, never a transcript.
#[derive(Debug, thiserror::Error)]
pub enum ClaudeError {
    /// The path did not resolve to a known `/claude/...` relation.
    #[error("`{path}` is not a known /claude node")]
    UnknownNode {
        /// The offending path (an opaque session path; carries no secret).
        path: String,
    },
    /// The addressed session does not exist on this host.
    #[error("claude session `{session}` not found")]
    UnknownSession {
        /// The session id (a label, never a secret).
        session: String,
    },
    /// A write verb is not supported at this node (e.g. any write on the read-only `/claude/sessions`
    /// relation, or `UPDATE`/`REMOVE` on the append-only instructions log).
    #[error("{verb} is not supported on /claude/{node} (read-only / append-only)")]
    Unsupported {
        /// The node family (`sessions`, `instructions`).
        node: &'static str,
        /// The rejected verb label.
        verb: &'static str,
    },
    /// The effect payload was malformed for the target node (missing the instruction column, etc.).
    #[error("malformed /claude write effect: {reason}")]
    MalformedEffect {
        /// A secret-free reason.
        reason: String,
    },
    /// A session launch (`INSERT INTO /claude/sessions`) was requested but no launcher is
    /// configured — the fail-closed default (a binary with no launcher wired refuses the spawn
    /// rather than launching nothing).
    #[error("session launch is not configured on this host (no launcher wired); the INSERT is refused rather than spawning nothing")]
    LaunchNotConfigured,
    /// A session launch failed validation or spawning (bad cwd, missing binary, non-zero exit).
    /// The reason is a redacted, secret-free string — a launcher path/cwd is infra, never a credential.
    #[error("claude session launch failed: {reason}")]
    LaunchFailed {
        /// A secret-free reason.
        reason: String,
    },
    /// An underlying session-source I/O failure (the binary maps its error in here as a secret-free
    /// string — a session-state path is infra, never a credential).
    #[error("claude session source: {0}")]
    Source(String),
}
