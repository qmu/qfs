//! The JSON frames carried as WebSocket text messages between a member and its host.

use serde::{Deserialize, Serialize};

/// One Claude Code session a member reports (a `/claude/sessions` row).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct SessionReport {
    /// Session id.
    pub id: String,
    /// Working directory.
    pub cwd: Option<String>,
    /// Session name.
    pub name: Option<String>,
    /// Status (e.g. `busy`, `idle`).
    pub status: Option<String>,
    /// The last visible message.
    pub last_message: Option<String>,
}

/// A member's heartbeat payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Report {
    /// The member machine's hostname.
    pub hostname: String,
    /// Global CPU usage, percent.
    pub cpu_pct: f32,
    /// Used memory, bytes.
    pub mem_used: u64,
    /// Total memory, bytes.
    pub mem_total: u64,
    /// Used disk, bytes.
    pub disk_used: u64,
    /// Total disk, bytes.
    pub disk_total: u64,
    /// The member's live Claude Code sessions.
    pub sessions: Vec<SessionReport>,
}

/// A cluster frame. `Request`/`Response` are reserved for borrowed execution (the next ticket:
/// "lend the query, not the secret") and are answered with a refusal in this slice.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Frame {
    /// Member → host, first frame: the join token plus the member's name and heartbeat cadence.
    Hello {
        /// The join token (a bearer credential — never logged).
        token: String,
        /// The member's presented name; must equal the token's `member`.
        name: String,
        /// The member's heartbeat interval in milliseconds (the host lapses it after 3 misses).
        heartbeat_ms: u64,
    },
    /// Host → member: the hello was accepted.
    Welcome {
        /// The admitted member name.
        member: String,
    },
    /// Host → member: the hello was refused; the host closes the socket and registers nothing.
    Refused {
        /// Why (never echoes the token).
        reason: String,
    },
    /// Member → host: a resource report.
    Heartbeat {
        /// The report.
        report: Report,
    },
    /// Reserved: run a statement on the peer.
    Request {
        /// Correlation id.
        id: u64,
        /// The qfs statement.
        statement: String,
        /// Apply (`true`) or preview (`false`).
        commit: bool,
    },
    /// Reserved: the answer to a [`Frame::Request`].
    Response {
        /// Correlation id.
        id: u64,
        /// Whether the statement succeeded.
        ok: bool,
        /// Result rows or an error message.
        body: serde_json::Value,
    },
}

impl Frame {
    /// Encode as JSON text.
    #[must_use]
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    /// Decode from JSON text.
    ///
    /// # Errors
    /// The serde error when the text is not a frame.
    pub fn from_json(s: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(s)
    }
}

/// `Hello`'s `Debug` must never print the token; this helper renders a frame for logs.
#[must_use]
pub fn redacted(frame: &Frame) -> String {
    match frame {
        Frame::Hello {
            name, heartbeat_ms, ..
        } => format!("Hello {{ name: {name:?}, heartbeat_ms: {heartbeat_ms}, token: <redacted> }}"),
        other => format!("{other:?}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_round_trip_with_a_type_tag() {
        let f = Frame::Request {
            id: 7,
            statement: "/sys/processes".into(),
            commit: false,
        };
        let j = f.to_json();
        assert!(j.contains(r#""type":"request""#));
        assert_eq!(Frame::from_json(&j).unwrap(), f);
    }

    #[test]
    fn redaction_hides_the_token() {
        let f = Frame::Hello {
            token: "SECRET.TOKEN".into(),
            name: "a".into(),
            heartbeat_ms: 1,
        };
        assert!(!redacted(&f).contains("SECRET"));
    }
}
