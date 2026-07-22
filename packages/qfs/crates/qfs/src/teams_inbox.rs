//! The teams-inbox steering medium — the durable, non-process-touching sink a running Claude Code
//! session drains (ticket 20260717010500, "steering rewire").
//!
//! ## Why this medium (owner-attended probe, 2026-07-19)
//! Steering a session means delivering it a message it will read. The pty/rendezvous-socket
//! transport was RETIRED as unsafe — speaking a live session's socket reaches *into* a running
//! process and repeatedly crashed the owner's sessions on this shared host. The canonical sink is
//! instead the **teams inbox**: `~/.claude/teams/<session>/inboxes/<member>.json`, one JSON file
//! **per recipient**, each a **JSON array of messages** the running session drains on its own
//! schedule (the files sit at `[]` once drained). Appending to that array is a plain durable-file
//! enqueue — qfs writes a message and returns; it spawns nothing, signals nothing, kills nothing.
//! The session-id → inbox mapping is `member.agentId = <name>@session-<id>`, so a sessions-relation
//! `id` resolves to the `session-<id>` half and thence to the member's `inboxes/<member>.json`.
//!
//! ## What is proven here, and what stays fail-closed
//! This module is the **medium mechanic**: append one message object to a per-recipient inbox JSON
//! array, read it back, and fail closed on an unknown recipient (a standalone / non-team session has
//! no inbox — never a silent create). It is deliberately **schema-agnostic**: the message element is
//! an opaque [`serde_json::Value`] the caller supplies, because the exact element field names
//! (`from`/`to`/`text`/`id`/`ts` or similar) are the ONE remaining unknown — every observed inbox was
//! `[]`, so the shape must be captured from one real in-flight message (the container live-round,
//! ticket 20260719231005). Until that capture, `ClaudeStoreSource::append_instruction` stays
//! **fail-closed** and does not call this primitive with a guessed schema: an honest refusal beats an
//! append a real session would misparse. The mechanic below is exercised hermetically over a *fake*
//! inbox dir so it is ready to wire the instant the schema is captured.

use std::path::{Path, PathBuf};

/// A structured, secret-free error from the teams-inbox medium. Names a recipient/reason — never a
/// path payload or a credential (an inbox path is infra).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InboxError {
    /// The addressed recipient has no inbox on this host — an unknown session, or a standalone
    /// (non-team) session with no `inboxes/` dir. Fail-closed: the inbox is never created to make a
    /// steer "succeed"; the append is refused.
    #[error("claude session `{recipient}` has no teams inbox (unknown or non-team session); the steer is refused")]
    UnknownSession {
        /// The recipient (member name / session id) — a label, never a secret.
        recipient: String,
    },
    /// The inbox file exists but is not a JSON array (corrupt / unexpected shape). A structured
    /// refusal, never a panic and never an overwrite of unrecognised state.
    #[error("claude teams inbox for `{recipient}` is not a JSON array")]
    Malformed {
        /// The recipient whose inbox could not be decoded.
        recipient: String,
    },
    /// An underlying I/O failure reading/writing the inbox (mapped to a secret-free string).
    #[error("claude teams inbox I/O: {0}")]
    Io(String),
}

/// A teams-inbox directory — the `inboxes/` folder holding one `<recipient>.json` JSON-array queue
/// per member. Owns only the directory path; every op is a plain file read/append (no process, no
/// socket, no signal), so it is safe anywhere and hermetic behind a fake dir.
#[derive(Debug, Clone)]
pub struct TeamsInbox {
    /// The `inboxes/` directory that holds the per-recipient JSON-array files.
    inboxes_dir: PathBuf,
}

impl TeamsInbox {
    /// Build over an `inboxes/` directory (the real path is `~/.claude/teams/<session>/inboxes`; a
    /// test points this at a fake tempdir).
    #[must_use]
    pub fn new(inboxes_dir: impl Into<PathBuf>) -> Self {
        Self {
            inboxes_dir: inboxes_dir.into(),
        }
    }

    /// The inbox file for one recipient: `<inboxes_dir>/<recipient>.json`.
    fn inbox_path(&self, recipient: &str) -> PathBuf {
        self.inboxes_dir.join(format!("{recipient}.json"))
    }

    /// Read a recipient's inbox JSON array. Fail-closed: a missing inbox file is
    /// [`InboxError::UnknownSession`] (never an empty success), so the read face agrees with the
    /// append face on what a valid recipient is. A drained inbox reads back as an empty vec.
    ///
    /// # Errors
    /// [`InboxError::UnknownSession`] if the inbox file is absent, [`InboxError::Malformed`] if it is
    /// not a JSON array, [`InboxError::Io`] on a read failure.
    pub fn scan(&self, recipient: &str) -> Result<Vec<serde_json::Value>, InboxError> {
        let path = self.inbox_path(recipient);
        if !path.exists() {
            return Err(InboxError::UnknownSession {
                recipient: recipient.to_string(),
            });
        }
        self.read_array(recipient, &path)
    }

    /// Append one **opaque** message object to a recipient's inbox array — the single gated steer.
    /// Reversible-append semantics: it grows the array, never rewrites or removes prior messages.
    /// Fail-closed: the inbox file MUST already exist (a real team creates it; an unknown / non-team
    /// session has none), so a missing inbox is [`InboxError::UnknownSession`], never created to fake
    /// a success. Returns the array's new length.
    ///
    /// `message` is a caller-supplied [`serde_json::Value`] — this primitive fixes **no** element
    /// schema (that is the uncaptured unknown); it owns only the array-append mechanic.
    ///
    /// # Errors
    /// [`InboxError::UnknownSession`] for an absent inbox, [`InboxError::Malformed`] if the existing
    /// inbox is not a JSON array, [`InboxError::Io`] on a read/write failure.
    pub fn append(&self, recipient: &str, message: serde_json::Value) -> Result<usize, InboxError> {
        let path = self.inbox_path(recipient);
        if !path.exists() {
            return Err(InboxError::UnknownSession {
                recipient: recipient.to_string(),
            });
        }
        let mut arr = self.read_array(recipient, &path)?;
        arr.push(message);
        let bytes = serde_json::to_vec(&serde_json::Value::Array(arr.clone()))
            .map_err(|e| InboxError::Io(e.to_string()))?;
        std::fs::write(&path, bytes).map_err(|e| InboxError::Io(e.kind().to_string()))?;
        Ok(arr.len())
    }

    /// Read `path` as a JSON array of messages (a drained inbox is `[]`).
    fn read_array(
        &self,
        recipient: &str,
        path: &Path,
    ) -> Result<Vec<serde_json::Value>, InboxError> {
        let text =
            std::fs::read_to_string(path).map_err(|e| InboxError::Io(e.kind().to_string()))?;
        let value: serde_json::Value =
            serde_json::from_str(&text).map_err(|_| InboxError::Malformed {
                recipient: recipient.to_string(),
            })?;
        match value {
            serde_json::Value::Array(a) => Ok(a),
            _ => Err(InboxError::Malformed {
                recipient: recipient.to_string(),
            }),
        }
    }
}

/// Resolve a sessions-relation `id` to the `session-<id>` half of a `member.agentId`
/// (`<name>@session-<id>`). The mission's documented mapping between a `/claude/sessions` id and the
/// teams-inbox recipient key runs through this; it is a pure string join with no I/O.
#[must_use]
pub fn session_agent_suffix(session_id: &str) -> String {
    format!("session-{session_id}")
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;
    use tempfile::TempDir;

    /// A fake `inboxes/` dir with one drained (`[]`) recipient inbox — the shape a real team leaves
    /// behind, hermetic (a tempdir, no session, no process).
    fn fake_inboxes(recipients: &[&str]) -> (TempDir, TeamsInbox) {
        let dir = TempDir::new().unwrap();
        for r in recipients {
            std::fs::write(dir.path().join(format!("{r}.json")), "[]").unwrap();
        }
        let inbox = TeamsInbox::new(dir.path().to_path_buf());
        (dir, inbox)
    }

    /// The round-trip: appending an (opaque) message object to a drained inbox grows its array, and
    /// `scan` reads the same message back — the read face stays truthful.
    #[test]
    fn append_then_scan_round_trips_the_message() {
        let (_d, inbox) = fake_inboxes(&["drive-lead"]);
        // The element shape is the caller's / the captured schema's concern — this proves the array
        // append mechanic with an opaque object.
        let msg = serde_json::json!({ "opaque": "focus on the failing test" });
        let len = inbox.append("drive-lead", msg.clone()).unwrap();
        assert_eq!(len, 1, "the array grew by one");
        let back = inbox.scan("drive-lead").unwrap();
        assert_eq!(back, vec![msg], "the same message reads back");
    }

    /// Append is reversible-append: a second message is added, prior messages preserved and ordered.
    #[test]
    fn append_preserves_prior_messages_in_order() {
        let (_d, inbox) = fake_inboxes(&["team-lead"]);
        inbox
            .append("team-lead", serde_json::json!({ "n": 1 }))
            .unwrap();
        let len = inbox
            .append("team-lead", serde_json::json!({ "n": 2 }))
            .unwrap();
        assert_eq!(len, 2);
        let back = inbox.scan("team-lead").unwrap();
        assert_eq!(back[0]["n"], 1);
        assert_eq!(back[1]["n"], 2, "append order preserved");
    }

    /// Fail-closed: steering an unknown recipient (no inbox file — an unknown or non-team session) is
    /// REFUSED, never created. Both the append and the read face agree.
    #[test]
    fn unknown_session_fails_closed_on_append_and_scan() {
        let (_d, inbox) = fake_inboxes(&["present"]);
        let err = inbox
            .append("absent", serde_json::json!({ "x": 1 }))
            .unwrap_err();
        assert!(matches!(err, InboxError::UnknownSession { .. }));
        let err = inbox.scan("absent").unwrap_err();
        assert!(matches!(err, InboxError::UnknownSession { .. }));
        // The refusal did NOT create the inbox (no silent success by side effect).
        assert!(inbox.scan("absent").is_err());
    }

    /// A corrupt inbox (not a JSON array) is a structured refusal, never a panic and never an
    /// overwrite of unrecognised state.
    #[test]
    fn a_non_array_inbox_is_a_structured_error() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("bad.json"), r#"{"not":"an array"}"#).unwrap();
        let inbox = TeamsInbox::new(dir.path().to_path_buf());
        assert!(matches!(
            inbox.append("bad", serde_json::json!({})).unwrap_err(),
            InboxError::Malformed { .. }
        ));
    }

    /// The documented session-id → recipient-key mapping is a pure `session-<id>` suffix.
    #[test]
    fn session_agent_suffix_matches_the_probed_mapping() {
        assert_eq!(session_agent_suffix("abc123"), "session-abc123");
    }
}
