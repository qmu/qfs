//! The `/claude/*` node model: the [`ClaudeNode`] sum type, its path↔node mapping, the
//! single-source-of-truth [`claude_node_schema`], and the per-node [`claude_node_capabilities`].
//!
//! This is the **pure, credential-free** introspective surface (blueprint §3 purity / §6). It
//! mirrors the `/sys` driver's `sys_node_schema` pattern: `DESCRIBE /claude/sessions` returns a
//! stable typed [`Schema`] with **no session source and no secrets**, so describe (and the
//! parse-time capability gate) read one source of truth that can never drift from the rows the
//! backend later scans. NOTHING here reads a file, opens a socket, or talks to an agent runtime.
//!
//! ## The `/claude` driver calls no model (blueprint §15, decision W supersedes decision K)
//! A `/claude` path is NOT qfs calling the Claude API. `/claude/sessions` is a **path façade over
//! session metadata** (what an agent is doing), and `.../instructions` is an **append-log** the
//! agent reads — the model runs ELSEWHERE; qfs only exposes/steers the session surface. There is
//! no inference dependency anywhere in THIS crate. (qfs does call a model via the `|> transform`
//! surface — blueprint §15 / decision W — but that lives in `qfs-driver-transform` + the binary,
//! never here.)
//!
//! ## Redaction is structural
//! `/claude/sessions` declares ONLY `id` / `cwd` / `name` / `status` / `last_message` — session
//! metadata the store actually records, plus the latest visible message text. There is **no
//! column** for a token, a key, or a raw transcript, so a credential cannot surface through this
//! path even by accident: the schema is the boundary.

use qfs_types::{Column, ColumnType, Schema};

/// The reserved mount point for the AI-sessions driver. The session service is reached ONLY
/// under the host realm — `/hosts/<host>/claude/...` (decision P / §1.3; owner ruling
/// 2026-07-16, honouring t64): the engine's host-realm peel strips `/hosts/local` and routes the
/// `/claude/...` **service** path here, so the driver mounts at `/claude` and names no host
/// segment itself. The bare top-level `/claude/...` spelling is retired — the binary marks this
/// mount host-realm-only (`MountRegistry::require_host_realm`), so a bare address fails with a
/// `retired_path` pointer at the canonical form. `claude` is NOT a reserved realm, so the mount
/// is admitted.
pub const CLAUDE_MOUNT: &str = "/claude";

/// One addressable `/claude/<node>` relation (roadmap §3.3 / M7). A **closed set**; a new view
/// adds a variant here, never a side-channel API (the one-engine constraint).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaudeNode {
    /// `/claude/sessions` — the agent SESSIONS relation: what each live Claude Code session is
    /// doing (`id`/`cwd`/`name`/`status`/`last_message`), READ-ONLY metadata. The model runs
    /// elsewhere; this is a façade over its session state, never an inference call (decision W: the /claude façade calls no model).
    Sessions,
    /// `/claude/sessions/<id>/instructions` — the per-session INSTRUCTIONS append-log: the
    /// steering messages handed to a running agent. APPEND-ONLY: `SELECT` to read the log, a
    /// single reversible `INSERT` to steer. Never `UPDATE`/`REMOVE` — "stop the agent" would be a
    /// separate irreversible `Remove`, not a silent reversible op (the safety floor).
    Instructions,
}

impl ClaudeNode {
    /// The path segment naming this node's family (`sessions`, `instructions`).
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Sessions => "sessions",
            Self::Instructions => "instructions",
        }
    }

    /// Whether this node is the append-only instructions log (read + a single reversible append;
    /// no `UPDATE`/`REMOVE`).
    #[must_use]
    pub const fn is_append_log(self) -> bool {
        matches!(self, Self::Instructions)
    }
}

/// Resolve a `/claude/...` **service** path to its [`ClaudeNode`], if it names a known relation.
/// Recognised shapes (the realm is already peeled, so the path begins at the mount):
/// - `/claude/sessions` (and `/claude/sessions` with a trailing slash) → [`ClaudeNode::Sessions`];
/// - `/claude/sessions/<id>/instructions[/...]` → [`ClaudeNode::Instructions`].
///
/// Returns `None` for `/claude` itself, a bare `/claude/sessions/<id>` (a single session is read
/// through the `sessions` relation filtered by `id`, not addressed as a node), or any other shape.
#[must_use]
pub fn node_for_path(path: &str) -> Option<ClaudeNode> {
    let rest = path
        .strip_prefix("/claude/")
        .or_else(|| path.strip_prefix("claude/"))?;
    let segments: Vec<&str> = rest.split('/').filter(|s| !s.is_empty()).collect();
    match segments.as_slice() {
        ["sessions"] => Some(ClaudeNode::Sessions),
        // `/claude/sessions/<id>/instructions[/...]`: the per-session append-log.
        ["sessions", _id, "instructions", ..] => Some(ClaudeNode::Instructions),
        _ => None,
    }
}

/// Extract the session id from a `/claude/sessions/<id>/instructions[/...]` path — the address of
/// the agent an `INSERT` steers (e.g. `current`, or a concrete session id). Returns `None` for any
/// non-instructions path. Pure string work; no I/O.
#[must_use]
pub fn instruction_session(path: &str) -> Option<String> {
    let rest = path
        .strip_prefix("/claude/")
        .or_else(|| path.strip_prefix("claude/"))?;
    let segments: Vec<&str> = rest.split('/').filter(|s| !s.is_empty()).collect();
    match segments.as_slice() {
        ["sessions", id, "instructions", ..] => Some((*id).to_string()),
        _ => None,
    }
}

/// The typed [`Schema`] of a `/claude/<node>` relation — the **canonical** source of truth
/// `DESCRIBE` and the backend scan both read. Pure data; no live source, no creds.
///
/// Neither relation carries a column for a token, a key, or a raw transcript secret — session
/// metadata + a steering message only (the redaction contract).
#[must_use]
pub fn claude_node_schema(node: ClaudeNode) -> Schema {
    let col = |name: &str, ty: ColumnType, nullable: bool| Column::new(name, ty, nullable);
    match node {
        // The agent sessions relation: one row per LIVE session, the fields Claude Code's own
        // store records (`~/.claude/sessions/<pid>.json`) plus the transcript's latest visible
        // message text. METADATA ONLY — no secret, no raw transcript, so it is safe to render as
        // a relation (the boundary `describe` enforces).
        ClaudeNode::Sessions => Schema::new(vec![
            col("id", ColumnType::Text, false),
            col("cwd", ColumnType::Text, true),
            col("name", ColumnType::Text, true),
            col("status", ColumnType::Text, false),
            col("last_message", ColumnType::Text, true),
        ]),
        // The per-session instructions append-log: a steering message + when it was appended.
        ClaudeNode::Instructions => Schema::new(vec![
            col("ts", ColumnType::Text, true),
            col("instruction", ColumnType::Text, false),
        ]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_for_path_resolves_known_shapes() {
        assert_eq!(
            node_for_path("/claude/sessions"),
            Some(ClaudeNode::Sessions)
        );
        assert_eq!(
            node_for_path("/claude/sessions/"),
            Some(ClaudeNode::Sessions)
        );
        assert_eq!(
            node_for_path("/claude/sessions/current/instructions"),
            Some(ClaudeNode::Instructions)
        );
        assert_eq!(
            node_for_path("/claude/sessions/abc123/instructions"),
            Some(ClaudeNode::Instructions)
        );
        // A bare single-session path is NOT a node (read it through `sessions` filtered by id).
        assert_eq!(node_for_path("/claude/sessions/current"), None);
        // The mount itself and unknown shapes are not describable.
        assert_eq!(node_for_path("/claude"), None);
        assert_eq!(node_for_path("/claude/nope"), None);
    }

    #[test]
    fn instruction_session_extracts_the_agent_address() {
        assert_eq!(
            instruction_session("/claude/sessions/current/instructions").as_deref(),
            Some("current")
        );
        assert_eq!(
            instruction_session("/claude/sessions/s-42/instructions").as_deref(),
            Some("s-42")
        );
        assert_eq!(instruction_session("/claude/sessions"), None);
    }

    /// The redaction contract is structural: neither relation declares a column a secret could
    /// ride in.
    #[test]
    fn no_relation_exposes_a_secret_column() {
        for node in [ClaudeNode::Sessions, ClaudeNode::Instructions] {
            let schema = claude_node_schema(node);
            for forbidden in [
                "token",
                "api_key",
                "apikey",
                "secret",
                "password",
                "bearer",
                "transcript",
            ] {
                assert!(
                    schema.column(forbidden).is_none(),
                    "/claude/{} must never expose `{forbidden}`",
                    node.label()
                );
            }
        }
    }
}
