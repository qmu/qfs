//! The `/claude` AI-sessions composition root: the on-disk [`SessionSource`] implementation over
//! **Claude Code's real store** + the async [`ClaudeReadDriver`] read facet, both hosted in the
//! **`qfs` binary crate**.
//!
//! ## Why the source lives in the binary (not the leaf driver crate)
//! `qfs-driver-claude` is the vendor-free AI-sessions driver (the pure introspective half + the
//! `ClaudeApplier` over the `SessionSource` seam) and is a **`qfs-runtime` consumer**, so the
//! dep-direction guard requires it to be a **leaf** — only the terminal `qfs` binary may depend on
//! it. The binary IS that leaf and the ONE place that opens a real path (decision F), so the actual
//! `std::fs` reads over Claude Code's session state dead-end here. No filesystem path crosses
//! the `SessionSource` boundary (owned qfs DTOs only).
//!
//! ## The `/claude` composition root calls no model (blueprint §15, decision W supersedes decision K)
//! This composition root reads session METADATA an agent runtime already wrote. The model runs
//! ELSEWHERE; nothing HERE calls or hosts an LLM. (qfs's model-calling surface is `|> transform` —
//! §15 / decision W — wired in `crate::transform`'s `BinaryTransformExecutor`, never this `/claude`
//! root.)
//!
//! ## Config (no credentials) — fail-closed / opt-in by default
//! The store location is one env var `QFS_CLAUDE_SESSIONS=<claude-home-dir>` (the directory that
//! contains `sessions/` and `projects/`, conventionally `~/.claude`). **With it unset there is no
//! source — fail-closed**: the binary registers neither the live read facet nor the apply driver.
//! The introspective mount itself is always registered (describe is pure), so an unconfigured scan
//! surfaces a structured read-registry error rather than an unroutable path. The live
//! cross-machine `<host>` hop (`/hosts/<host>/claude/...`) rides the t63 tunnel and re-checks
//! `POLICY` at the destination — a DOCUMENTED SEAM, not wired here.
//!
//! ## The real on-disk layout (verified 2026-07-17, mission
//! `claude-code-sessions-are-queryable-and-steerable-as-qfs-paths`)
//! This reader replaces the earlier hand-invented `<base>/<id>/meta` layout, which existed on no
//! machine (pointing it at a real host yielded zero rows). Claude Code actually writes:
//!
//! - `<home>/sessions/<pid>.json` — one JSON record per session process: `pid`, `sessionId`,
//!   `cwd`, `name`, `status` (`busy`/…), `kind`, timestamps. This is the store's own liveness
//!   registry; a record whose process is gone is a leftover, not a session.
//! - `<home>/projects/<slugified-cwd>/<sessionId>.jsonl` — the transcript: one JSON entry per
//!   line; `user`/`assistant` entries carry `message.content` (a string, or an array of typed
//!   blocks). The slug maps every character outside `[A-Za-z0-9-]` to `-`.
//!
//! `last_message` is the last transcript entry whose content yields visible text — string content
//! as-is, array content as its `text` blocks (tool_use/tool_result traffic never surfaces),
//! bounded to a fixed length. Only the schema's five metadata columns ever leave this module.
//!
//! ## Steering rides the session's own peer-messaging socket (ticket 20260805113100)
//! The retired layout's `instructions` append-log was written by qfs and read by **nothing** — an
//! append that steered no session — so this seam failed closed for a year. The medium a live
//! session actually reads was captured by the spike ticket 20260805113000 (design brief
//! `design-brief-steering-transport.md`) against a real Claude Code 2.1.233 session:
//!
//! - The **teams inbox is not it.** A session that never formed a team has no
//!   `<home>/teams/<team>/inboxes/` at all, and an inbox file planted for one is never drained
//!   (measured: ten candidate spellings, 75 s, zero reads). So this module never creates one.
//! - The medium is the session's **peer-messaging Unix domain socket**, whose path the session
//!   publishes in the very liveness record [`scan_sessions`](ClaudeStoreSource::scan_sessions)
//!   already reads — `sessions/<pid>.json` → `messagingSocketPath` — authenticated by the
//!   `peerToken` in the sibling `sessions/<pid>.<hash>.key`. Two newline-delimited JSON lines
//!   (`{"type":"auth",…}` then `{"type":"user",…}`) hand a running session a user message it acts
//!   on. Both inputs live in the directory this reader already scans: no new store, no new config.
//!
//! [`SessionSource::append_instruction`] writes exactly that, and **fails closed with a named
//! reason** for every way the target can be unreachable (unknown session, dead process, a record
//! with no socket path, no readable peer token, a refused connection). It invents no path and
//! creates no directory — refusing beats writing where nothing reads.
//!
//! The instructions log still **reads back empty**: the socket is a transport with no queryable
//! backlog, and an honest empty read beats replaying a file nothing consumed.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use qfs_core::{CfsError, RequestContext, RowBatch};
use qfs_driver_claude::{
    claude_node_schema, instruction_session, node_for_path, ClaudeError, ClaudeNode, LaunchSpec,
    SessionLauncher, SessionSource,
};
use qfs_exec::ReadDriver;
use qfs_pushdown::ScanNode;
use qfs_types::{Row, Value};

/// The env var naming the Claude Code home dir (the parent of `sessions/` and `projects/`):
/// `QFS_CLAUDE_SESSIONS=<claude-home>`, conventionally `~/.claude`.
const CLAUDE_ENV: &str = "QFS_CLAUDE_SESSIONS";

/// The env var naming the Claude Code CLI binary the launcher spawns (`INSERT INTO
/// /claude/sessions`). The binary path is **configuration**, never a query input; unset ⇒ the
/// default `claude` on `PATH`. This is the ONLY trusted input to the launcher — `cwd`/`prompt`/
/// `name` are data, passed as discrete arguments (never a shell line).
const CLAUDE_BINARY_ENV: &str = "QFS_CLAUDE_BINARY";

/// How many bytes of a transcript tail are scanned for the last visible message. Transcripts grow
/// to many megabytes; the last visible text virtually always lives in the final few entries.
const TRANSCRIPT_TAIL_BYTES: u64 = 256 * 1024;

/// The bound on a surfaced `last_message` (characters). A relation cell is a summary surface, not
/// a transcript dump; the full transcript never leaves this module.
const LAST_MESSAGE_MAX_CHARS: usize = 2000;

/// How long a steering write may spend connected to a session's peer socket. A session that is
/// busy still accepts the two lines immediately (the socket is a queue, not a request/response),
/// so this bounds a pathological peer rather than ordinary latency.
const STEER_WRITE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// The on-disk [`SessionSource`] over Claude Code's REAL store: the `sessions/<pid>.json`
/// liveness registry joined with the `projects/<slug>/<id>.jsonl` transcripts. Owns only the
/// home path — no credential, no model handle (decision W: this /claude root calls no model).
pub struct ClaudeStoreSource {
    /// The Claude home dir (contains `sessions/` and `projects/`).
    home: PathBuf,
}

impl ClaudeStoreSource {
    /// Build a source over `home` (the test + composition seam).
    #[must_use]
    pub fn new(home: impl Into<PathBuf>) -> Self {
        Self { home: home.into() }
    }

    /// Open the configured source from `QFS_CLAUDE_SESSIONS`, or `None` when unset/empty — the
    /// **fail-closed default** (the `/claude` read facet + applier are simply not wired rather
    /// than binding a source that resolves nothing). Mirrors the `/fs` deny-all-by-default opt-in
    /// (t68).
    #[must_use]
    pub fn open_default() -> Option<Self> {
        match std::env::var(CLAUDE_ENV) {
            Ok(home) if !home.is_empty() => Some(Self::new(home)),
            _ => None,
        }
    }

    /// The transcript path for a session record: `<home>/projects/<slug(cwd)>/<id>.jsonl`.
    fn transcript_path(&self, cwd: &str, id: &str) -> PathBuf {
        self.home
            .join("projects")
            .join(slugify_cwd(cwd))
            .join(format!("{id}.jsonl"))
    }

    /// Resolve a session id to the peer endpoint that steers it, or say precisely why it cannot be
    /// steered. Reads only the liveness registry this module already scans — the record names its
    /// own socket, and the sibling key file carries the token that socket demands.
    ///
    /// Every failure is a **named, secret-free** refusal (the honest-surfaces floor): a caller that
    /// gets `Ok` back has a socket that accepted the message, and one that gets an error is told
    /// which of the five ways the target was unreachable, never a silent success.
    fn resolve_peer(&self, session: &str) -> Result<SessionPeer, ClaudeError> {
        let dir = self.home.join("sessions");
        let entries = std::fs::read_dir(&dir).map_err(|e| {
            ClaudeError::Source(format!(
                "the session registry could not be read ({})",
                e.kind()
            ))
        })?;
        let mut record = None;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            match read_session_record(&path) {
                Some(r) if r.id == session => {
                    record = Some(r);
                    break;
                }
                _ => continue,
            }
        }
        let record = record.ok_or_else(|| ClaudeError::UnknownSession {
            session: session.to_string(),
        })?;
        // A record outlives its process; steering a leftover would report success and reach nobody.
        if !pid_is_live(record.pid) {
            return Err(ClaudeError::Source(format!(
                "claude session `{session}` is a leftover record: its process is no longer running, \
                 so there is nothing to steer"
            )));
        }
        let socket = record.messaging_socket.ok_or_else(|| {
            ClaudeError::Source(format!(
                "claude session `{session}` publishes no messaging socket \
                 (its record carries no `messagingSocketPath`), so it cannot be steered"
            ))
        })?;
        let token = read_peer_token(&dir, record.pid).ok_or_else(|| {
            ClaudeError::Source(format!(
                "no readable peer token for claude session `{session}` \
                 (no `<pid>.<hash>.key` beside its record), so its socket would refuse the message"
            ))
        })?;
        Ok(SessionPeer {
            socket: PathBuf::from(socket),
            token,
        })
    }
}

/// The endpoint that steers one live session: the socket it published and the token that socket
/// demands. Held only for the duration of one write — never surfaced through a relation (the
/// `/claude` schemas declare no token column, and this type is private to this module).
struct SessionPeer {
    /// The session's own `messagingSocketPath`, read from its liveness record — never a path this
    /// module composed.
    socket: PathBuf,
    /// The `peerToken` from the record's sibling key file. Never logged, never echoed in an error.
    token: String,
}

/// Read the `peerToken` for `pid` from `<sessions-dir>/<pid>.<hash>.key`. The hash segment is
/// opaque, so the file is found by its `<pid>.` prefix and `.key` suffix. `None` when no such file
/// exists or none carries a non-empty token.
fn read_peer_token(sessions_dir: &Path, pid: u64) -> Option<String> {
    let prefix = format!("{pid}.");
    for entry in std::fs::read_dir(sessions_dir).ok()?.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("key") {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if !name.starts_with(&prefix) {
            continue;
        }
        let text = std::fs::read_to_string(&path).ok()?;
        let v: serde_json::Value = serde_json::from_str(&text).ok()?;
        if let Some(token) = v.get("peerToken").and_then(|t| t.as_str()) {
            if !token.is_empty() {
                return Some(token.to_string());
            }
        }
    }
    None
}

/// Hand `instruction` to a live session over its peer socket: connect, send the **auth line**, then
/// the **user message**, both newline-delimited JSON (the protocol Claude Code's own `uds-messaging`
/// layer documents). The instruction is *data* — it crosses as a JSON string value, so no content
/// can forge a second protocol line.
///
/// Unix only: the peer channel is a Unix domain socket. Elsewhere the write is a named refusal
/// rather than a pretence.
#[cfg(unix)]
fn send_peer_message(peer: &SessionPeer, instruction: &str) -> Result<(), ClaudeError> {
    use std::io::Write as _;
    use std::os::unix::net::UnixStream;

    let mut stream = UnixStream::connect(&peer.socket).map_err(|e| {
        ClaudeError::Source(format!(
            "the session's messaging socket refused the connection ({}); \
             the instruction was not delivered",
            e.kind()
        ))
    })?;
    stream
        .set_write_timeout(Some(STEER_WRITE_TIMEOUT))
        .map_err(|e| ClaudeError::Source(format!("the peer socket is unusable ({})", e.kind())))?;
    let auth = serde_json::json!({ "type": "auth", "token": peer.token });
    let message = serde_json::json!({
        "type": "user",
        "message": { "role": "user", "content": instruction },
    });
    // One buffer, one write: the auth line must not be observable without the message behind it.
    let payload = format!("{auth}\n{message}\n");
    stream.write_all(payload.as_bytes()).map_err(|e| {
        ClaudeError::Source(format!(
            "the instruction could not be written to the session's messaging socket ({})",
            e.kind()
        ))
    })?;
    stream.flush().map_err(|e| {
        ClaudeError::Source(format!(
            "the instruction was not flushed to the session's messaging socket ({})",
            e.kind()
        ))
    })?;
    Ok(())
}

/// The non-Unix refusal: there is no peer socket to write to, so steering is unavailable rather
/// than silently successful.
#[cfg(not(unix))]
fn send_peer_message(_peer: &SessionPeer, _instruction: &str) -> Result<(), ClaudeError> {
    Err(ClaudeError::Source(
        "steering a claude session needs its peer messaging socket, which exists on unix only"
            .to_string(),
    ))
}

/// The steering text of an `INSERT INTO …/instructions` row batch — the `instruction` column.
fn instruction_text(batch: &RowBatch) -> Result<String, ClaudeError> {
    let row = batch
        .rows
        .first()
        .ok_or_else(|| ClaudeError::MalformedEffect {
            reason: "the steering INSERT carries no VALUES row".to_string(),
        })?;
    let idx = batch
        .schema
        .columns
        .iter()
        .position(|c| c.name.as_str() == "instruction")
        .ok_or_else(|| ClaudeError::MalformedEffect {
            reason: "the steering INSERT names no `instruction` column".to_string(),
        })?;
    match row.values.get(idx) {
        Some(Value::Text(s)) if !s.trim().is_empty() => Ok(s.clone()),
        _ => Err(ClaudeError::MalformedEffect {
            reason: "the steering INSERT's `instruction` is absent, non-text, or blank".to_string(),
        }),
    }
}

impl SessionSource for ClaudeStoreSource {
    fn scan_sessions(&self) -> Result<RowBatch, ClaudeError> {
        let schema = claude_node_schema(ClaudeNode::Sessions);
        let mut rows: Vec<(String, Row)> = Vec::new();
        // A missing sessions dir yields zero sessions (fail-closed read, never an error).
        if let Ok(entries) = std::fs::read_dir(self.home.join("sessions")) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("json") {
                    continue;
                }
                let Some(record) = read_session_record(&path) else {
                    continue; // an unparseable record is skipped, never a panic
                };
                // Liveness: the registry keeps a record per session PROCESS; a dead pid is a
                // leftover (crash / unclean exit), not a running session.
                if !pid_is_live(record.pid) {
                    continue;
                }
                let last_message = record
                    .cwd
                    .as_deref()
                    .map(|cwd| self.transcript_path(cwd, &record.id))
                    .and_then(|p| last_visible_message(&p))
                    .map_or(Value::Null, Value::Text);
                let row = Row::new(vec![
                    Value::Text(record.id.clone()),
                    record.cwd.map_or(Value::Null, Value::Text),
                    record.name.map_or(Value::Null, Value::Text),
                    Value::Text(record.status),
                    last_message,
                ]);
                rows.push((record.id, row));
            }
        }
        // Deterministic order (the registry iterates by pid filename; sort by session id).
        rows.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(RowBatch::new(
            schema,
            rows.into_iter().map(|(_, r)| r).collect(),
        ))
    }

    fn scan_instructions(&self, _session: &str) -> Result<RowBatch, ClaudeError> {
        // The steering medium is a socket, not a file: it carries no queryable backlog, so the
        // log truthfully reads back empty rather than replaying something nothing consumed.
        Ok(RowBatch::new(
            claude_node_schema(ClaudeNode::Instructions),
            Vec::new(),
        ))
    }

    fn append_instruction(&self, session: &str, row: &RowBatch) -> Result<u64, ClaudeError> {
        // Order matters: a malformed payload is rejected before any session is addressed, so a
        // blank steer never opens a socket.
        let instruction = instruction_text(row)?;
        let peer = self.resolve_peer(session)?;
        send_peer_message(&peer, &instruction)?;
        Ok(1)
    }
}

/// The on-disk [`SessionLauncher`] over the real Claude Code CLI (`INSERT INTO /claude/sessions`).
/// Spawns a background session with `Command::new(<configured binary>)`, passing `cwd`/`prompt`/
/// `name` as **discrete process arguments** — never a shell line (no `sh -c`, no interpolation;
/// ticket 20260717010600 safety floor). The binary path is *configuration* (`QFS_CLAUDE_BINARY`,
/// default `claude`), the only trusted input; the cwd/prompt/name are the query's data. Owns only
/// the binary path — no credential, no model handle (decision W: this root calls no model).
///
/// ## The live spawn is owner-attended (not exercised hermetically)
/// A real launch spends money and starts an autonomous session (mission acceptance, owner-attended
/// live proof). Hermetic tests point `binary` at a harmless fake that echoes its argv, so the
/// argument-passing safety floor and the id-capture path are proven without any real launch.
pub struct ClaudeCliLauncher {
    /// The CLI binary to spawn (configuration; default `claude`).
    binary: PathBuf,
}

impl ClaudeCliLauncher {
    /// Build a launcher over an explicit binary path (the test + composition seam).
    #[must_use]
    pub fn new(binary: impl Into<PathBuf>) -> Self {
        Self {
            binary: binary.into(),
        }
    }

    /// The configured launcher: `QFS_CLAUDE_BINARY` when set and non-empty, else the default
    /// `claude` resolved on `PATH`. Always returns a launcher (the binary need not exist until a
    /// launch is committed — a missing binary is a structured [`ClaudeError::LaunchFailed`] then,
    /// not at wiring time); the whole `/claude` write surface stays fail-closed via the session
    /// store (`open_default`), which must be configured for the applier to register at all.
    #[must_use]
    pub fn open_default() -> Self {
        let binary = std::env::var(CLAUDE_BINARY_ENV)
            .ok()
            .filter(|b| !b.is_empty())
            .map_or_else(|| PathBuf::from("claude"), PathBuf::from);
        Self::new(binary)
    }
}

impl SessionLauncher for ClaudeCliLauncher {
    fn launch(&self, spec: &LaunchSpec) -> Result<String, ClaudeError> {
        // Validate the working directory BEFORE spawning: a bad cwd is a structured, secret-free
        // refusal, never a half-started session.
        let cwd = Path::new(&spec.cwd);
        if !cwd.is_dir() {
            return Err(ClaudeError::LaunchFailed {
                reason: "working directory does not exist".to_string(),
            });
        }
        // `<binary> --bg <prompt> [--name <name>]` in `cwd`. EVERY value crosses as a DISCRETE
        // ARGUMENT via `.arg(..)` — no shell, no interpolation (the safety floor). A prompt full of
        // shell metacharacters reaches the CLI verbatim as one argument.
        let mut cmd = std::process::Command::new(&self.binary);
        cmd.current_dir(cwd).arg("--bg").arg(&spec.prompt);
        if let Some(name) = &spec.name {
            cmd.arg("--name").arg(name);
        }
        let output = cmd.output().map_err(|e| ClaudeError::LaunchFailed {
            reason: format!("could not spawn the session binary ({})", e.kind()),
        })?;
        if !output.status.success() {
            return Err(ClaudeError::LaunchFailed {
                reason: format!("session binary exited unsuccessfully ({})", output.status),
            });
        }
        // The new session id: `claude --bg` prints a multi-line banner and the id lives on the
        // `backgrounded · <shortid>` line — NOT the first line (Claude Code 2.1.217 emits
        // `Starting background service…` first, so first-line capture returned that banner text
        // verbatim instead of the id; ticket 20260722213000). Parse the `backgrounded` line. The
        // exact id-discovery contract is re-proven in the owner-attended live round; hermetically a
        // fake binary emits the same banner shape.
        let stdout = String::from_utf8_lossy(&output.stdout);
        let id = parse_backgrounded_id(&stdout).ok_or_else(|| ClaudeError::LaunchFailed {
            reason: "the launch output carried no `backgrounded · <id>` session id".to_string(),
        })?;
        Ok(id)
    }
}

/// Extract the session id from a `claude --bg` banner. Claude Code prints the launched session's
/// short handle on a line shaped `backgrounded · <shortid>` (after a leading
/// `Starting background service…` line and before a help block). This finds the first line whose
/// first whitespace token is `backgrounded` and returns that line's LAST whitespace token — the
/// id — independent of the exact separator glyph (`·`). `None` when no such line carries an id.
fn parse_backgrounded_id(stdout: &str) -> Option<String> {
    stdout.lines().find_map(|line| {
        let mut tokens = line.split_whitespace();
        if tokens.next() != Some("backgrounded") {
            return None;
        }
        // The id is the last token; the separator token(s) (`·`) sit between and are skipped.
        line.split_whitespace()
            .next_back()
            .filter(|id| *id != "backgrounded" && !id.is_empty())
            .map(str::to_string)
    })
}

/// One parsed `sessions/<pid>.json` record — the selectors-only subset this module surfaces, plus
/// the session's own steering address. `messaging_socket` never reaches a relation cell (the
/// `sessions` schema declares no such column); it exists so a steer addresses the socket the
/// session itself published rather than a path qfs composed.
struct SessionRecord {
    pid: u64,
    id: String,
    cwd: Option<String>,
    name: Option<String>,
    status: String,
    messaging_socket: Option<String>,
}

/// Parse one liveness-registry record. Lenient: a missing/malformed field degrades (status
/// `"unknown"`, absent cwd/name) — only a missing `sessionId`/`pid` drops the record.
fn read_session_record(path: &Path) -> Option<SessionRecord> {
    let text = std::fs::read_to_string(path).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    let id = v.get("sessionId")?.as_str()?.to_string();
    let pid = v.get("pid")?.as_u64()?;
    let str_field = |key: &str| v.get(key).and_then(|s| s.as_str()).map(str::to_string);
    Some(SessionRecord {
        pid,
        id,
        cwd: str_field("cwd"),
        name: str_field("name"),
        status: str_field("status").unwrap_or_else(|| "unknown".to_string()),
        messaging_socket: str_field("messagingSocketPath").filter(|s| !s.is_empty()),
    })
}

/// Whether `pid` names a live process. On Linux this is a `/proc/<pid>` existence check (the
/// registry record itself is what the store offers; the check filters leftovers of dead
/// processes). Where `/proc` does not exist the record is trusted as-is — a documented
/// degradation, never a dropped live session.
fn pid_is_live(pid: u64) -> bool {
    let proc_root = Path::new("/proc");
    if !proc_root.is_dir() {
        return true;
    }
    proc_root.join(pid.to_string()).is_dir()
}

/// Claude Code's project-dir slug for a session cwd: every character outside `[A-Za-z0-9-]`
/// becomes `-` (verified against the real store: `/home/ec2-user/projects/strategy` →
/// `-home-ec2-user-projects-strategy`; `.worktrees` doubles the dash).
fn slugify_cwd(cwd: &str) -> String {
    cwd.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect()
}

/// The last transcript entry with visible text, reading only the file's tail. `None` when the
/// transcript is absent, unreadable, or its tail carries no visible message (tool traffic only).
fn last_visible_message(transcript: &Path) -> Option<String> {
    let tail = read_tail(transcript, TRANSCRIPT_TAIL_BYTES)?;
    for line in tail.lines().rev() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(entry) = serde_json::from_str::<serde_json::Value>(line) else {
            continue; // a truncated first line of the tail window parses as garbage — skip
        };
        if let Some(text) = entry_visible_text(&entry) {
            return Some(text);
        }
    }
    None
}

/// Visible text of one transcript entry: only `user`/`assistant` entries count; string content
/// surfaces as-is, array content as its concatenated `text` blocks. Tool calls, tool results,
/// snapshots, queue operations and other machinery entries yield `None`.
fn entry_visible_text(entry: &serde_json::Value) -> Option<String> {
    let kind = entry.get("type")?.as_str()?;
    if kind != "user" && kind != "assistant" {
        return None;
    }
    let content = entry.get("message")?.get("content")?;
    let text = match content {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Array(blocks) => {
            let texts: Vec<&str> = blocks
                .iter()
                .filter(|b| b.get("type").and_then(|t| t.as_str()) == Some("text"))
                .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
                .collect();
            texts.join("\n")
        }
        _ => return None,
    };
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    Some(bound_chars(text, LAST_MESSAGE_MAX_CHARS))
}

/// The first `max` characters of `text` (whole string when shorter) — the relation-cell bound.
fn bound_chars(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

/// Read up to `max_bytes` from the END of `path` as lossy UTF-8. `None` when the file cannot be
/// opened/read. When the window starts mid-file the first (possibly partial) line is dropped.
fn read_tail(path: &Path, max_bytes: u64) -> Option<String> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = std::fs::File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    let start = len.saturating_sub(max_bytes);
    if start > 0 {
        file.seek(SeekFrom::Start(start)).ok()?;
    }
    let mut buf = Vec::with_capacity(usize::try_from(len - start).unwrap_or(0));
    file.read_to_end(&mut buf).ok()?;
    let mut text = String::from_utf8_lossy(&buf).into_owned();
    if start > 0 {
        // Drop the partial first line of the window (its head lies before the seek point); a
        // window with no newline at all has no complete line to offer.
        let idx = text.find('\n')?;
        text.drain(..=idx);
    }
    Some(text)
}

/// The async read facet (the `/claude` counterpart of `shell.rs`'s `LocalReadDriver` and `sys.rs`'s
/// `SysReadDriver`): adapts the synchronous [`SessionSource`] reads to qfs-exec's [`ReadDriver`]
/// seam. Lives in the binary because `ReadDriver` is a qfs-exec type and the driver crate must stay
/// off qfs-exec (dep direction).
pub struct ClaudeReadDriver {
    source: Arc<dyn SessionSource>,
}

impl ClaudeReadDriver {
    /// Build the read adapter over an injected source.
    #[must_use]
    pub fn new(source: Arc<dyn SessionSource>) -> Self {
        Self { source }
    }
}

#[async_trait::async_trait]
impl ReadDriver for ClaudeReadDriver {
    async fn scan(&self, scan: &ScanNode, _ctx: &RequestContext) -> Result<RowBatch, CfsError> {
        let node = node_for_path(&scan.path).ok_or_else(|| CfsError::InvalidPath {
            path: scan.path.clone(),
            reason: "not a /claude session path",
        })?;
        let result = match node {
            ClaudeNode::Sessions => self.source.scan_sessions(),
            ClaudeNode::Instructions => {
                let session =
                    instruction_session(&scan.path).ok_or_else(|| CfsError::InvalidPath {
                        path: scan.path.clone(),
                        reason: "no session id in /claude instructions path",
                    })?;
                self.source.scan_instructions(&session)
            }
        };
        result.map_err(|e| CfsError::InvalidPath {
            path: scan.path.clone(),
            reason: claude_error_reason(&e),
        })
    }
}

/// A stable, secret-free reason code for a `/claude` read failure (the executor maps it to its kind).
fn claude_error_reason(e: &ClaudeError) -> &'static str {
    match e {
        ClaudeError::UnknownNode { .. } => "unknown_claude_node",
        ClaudeError::UnknownSession { .. } => "unknown_session",
        ClaudeError::Unsupported { .. } => "unsupported_verb",
        ClaudeError::MalformedEffect { .. } => "malformed_effect",
        ClaudeError::LaunchNotConfigured => "launch_not_configured",
        ClaudeError::LaunchFailed { .. } => "launch_failed",
        ClaudeError::Source(_) => "read_failed",
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;
    use tempfile::TempDir;

    /// A pid that is certainly dead: far above Linux's `pid_max` (4 194 304), so `/proc/<pid>`
    /// can never exist.
    const DEAD_PID: u64 = 999_999_999;

    /// Write one liveness-registry record.
    fn write_record(home: &Path, pid: u64, id: &str, cwd: &str, name: &str, status: &str) {
        let dir = home.join("sessions");
        std::fs::create_dir_all(&dir).unwrap();
        let record = serde_json::json!({
            "pid": pid,
            "sessionId": id,
            "cwd": cwd,
            "name": name,
            "status": status,
            "kind": "interactive",
        });
        std::fs::write(dir.join(format!("{pid}.json")), record.to_string()).unwrap();
    }

    /// Add the steering address to an existing record: the socket the session published plus the
    /// sibling key file carrying its peer token — exactly the two files a real session writes.
    fn write_peer_address(home: &Path, pid: u64, socket: &Path, token: &str) {
        let dir = home.join("sessions");
        let path = dir.join(format!("{pid}.json"));
        let mut record: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        record["messagingSocketPath"] = serde_json::json!(socket.to_string_lossy());
        std::fs::write(&path, record.to_string()).unwrap();
        // The hash segment is opaque to the reader — any value must be found by the `<pid>.` prefix.
        let key = serde_json::json!({ "peerToken": token, "procStart": "1" });
        std::fs::write(dir.join(format!("{pid}.deadbeef.key")), key.to_string()).unwrap();
    }

    /// Write a transcript for `(cwd, id)` from raw JSONL lines.
    fn write_transcript(home: &Path, cwd: &str, id: &str, lines: &[&str]) {
        let dir = home.join("projects").join(slugify_cwd(cwd));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(format!("{id}.jsonl")), lines.join("\n")).unwrap();
    }

    /// A FIXTURE store shaped exactly like the real `~/.claude` (hermetic: a tempdir, no agent
    /// runtime, no model): one live session with a transcript, one dead leftover record.
    fn fixture() -> (TempDir, ClaudeStoreSource) {
        let dir = TempDir::new().unwrap();
        let home = dir.path();
        // The live session: this test process's own pid is live by definition.
        let live_pid = u64::from(std::process::id());
        write_record(
            home,
            live_pid,
            "s-live",
            "/tmp/fixture-proj",
            "fixture",
            "busy",
        );
        write_transcript(
            home,
            "/tmp/fixture-proj",
            "s-live",
            &[
                r#"{"type":"mode","mode":"normal","sessionId":"s-live"}"#,
                r#"{"type":"user","message":{"role":"user","content":"run the tests"},"sessionId":"s-live"}"#,
                r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"tests are green"}]},"sessionId":"s-live"}"#,
                r#"{"type":"pr-link","url":"https://example.invalid/pr/1"}"#,
            ],
        );
        // A leftover record of a dead process — must be filtered on Linux.
        write_record(
            home,
            DEAD_PID,
            "s-dead",
            "/tmp/fixture-proj",
            "ghost",
            "busy",
        );
        let source = ClaudeStoreSource::new(home.to_path_buf());
        (dir, source)
    }

    fn cell(batch: &RowBatch, row: usize, col: &str) -> Value {
        let idx = batch
            .schema
            .columns
            .iter()
            .position(|c| c.name.as_str() == col)
            .expect("column present");
        batch.rows[row].values[idx].clone()
    }

    /// The headline read: one row per LIVE session, joined with its transcript's last visible
    /// message, in the exact described schema (no drift).
    #[test]
    #[cfg_attr(not(target_os = "linux"), ignore = "dead-pid filtering needs /proc")]
    fn scan_sessions_reads_the_real_store_shape() {
        let (_d, source) = fixture();
        let batch = source.scan_sessions().unwrap();
        assert_eq!(batch.schema, claude_node_schema(ClaudeNode::Sessions));
        assert_eq!(batch.rows.len(), 1, "the dead leftover is filtered");
        assert_eq!(cell(&batch, 0, "id"), Value::Text("s-live".into()));
        assert_eq!(
            cell(&batch, 0, "cwd"),
            Value::Text("/tmp/fixture-proj".into())
        );
        assert_eq!(cell(&batch, 0, "name"), Value::Text("fixture".into()));
        assert_eq!(cell(&batch, 0, "status"), Value::Text("busy".into()));
        assert_eq!(
            cell(&batch, 0, "last_message"),
            Value::Text("tests are green".into()),
            "the last VISIBLE message wins (the trailing pr-link entry is machinery)"
        );
    }

    /// A live record with no transcript still surfaces (last_message Null) — visibility beats
    /// completeness.
    #[test]
    fn live_session_without_transcript_surfaces_with_null_message() {
        let dir = TempDir::new().unwrap();
        let live_pid = u64::from(std::process::id());
        write_record(
            dir.path(),
            live_pid,
            "s-bare",
            "/tmp/nowhere",
            "bare",
            "idle",
        );
        let source = ClaudeStoreSource::new(dir.path().to_path_buf());
        let batch = source.scan_sessions().unwrap();
        assert_eq!(batch.rows.len(), 1);
        assert_eq!(cell(&batch, 0, "id"), Value::Text("s-bare".into()));
        assert_eq!(cell(&batch, 0, "last_message"), Value::Null);
    }

    /// A tool-traffic tail walks back to the last real text (tool_result blocks never surface).
    #[test]
    fn tool_traffic_tail_walks_back_to_visible_text() {
        let dir = TempDir::new().unwrap();
        let home = dir.path();
        let live_pid = u64::from(std::process::id());
        write_record(home, live_pid, "s-tools", "/tmp/toolproj", "tools", "busy");
        write_transcript(
            home,
            "/tmp/toolproj",
            "s-tools",
            &[
                r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"reading the file"}]}}"#,
                r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"t1","name":"Read","input":{}}]}}"#,
                r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"secret file body"}]}}"#,
                r#"{"type":"queue-operation","op":"drain"}"#,
            ],
        );
        let source = ClaudeStoreSource::new(home.to_path_buf());
        let batch = source.scan_sessions().unwrap();
        assert_eq!(batch.rows.len(), 1);
        assert_eq!(
            cell(&batch, 0, "last_message"),
            Value::Text("reading the file".into()),
            "tool_result bodies must never surface"
        );
    }

    /// The slug matches the REAL store (verified examples from a live `~/.claude/projects`).
    #[test]
    fn slugify_matches_the_observed_store() {
        assert_eq!(
            slugify_cwd("/home/ec2-user/projects/strategy"),
            "-home-ec2-user-projects-strategy"
        );
        assert_eq!(
            slugify_cwd("/home/ec2-user/projects/data-platform/.worktrees/ai-letter"),
            "-home-ec2-user-projects-data-platform--worktrees-ai-letter"
        );
    }

    /// A steering `INSERT`'s row batch, shaped like the instructions schema.
    fn steer(instruction: &str) -> RowBatch {
        RowBatch::new(
            claude_node_schema(ClaudeNode::Instructions),
            vec![Row::new(vec![
                Value::Null,
                Value::Text(instruction.to_string()),
            ])],
        )
    }

    /// Read every newline-delimited JSON line one connection delivered, until the peer hangs up.
    #[cfg(unix)]
    fn read_lines(stream: std::os::unix::net::UnixStream) -> Vec<serde_json::Value> {
        use std::io::{BufRead, BufReader};
        BufReader::new(stream)
            .lines()
            .map_while(Result::ok)
            .filter(|l| !l.trim().is_empty())
            .map(|l| serde_json::from_str(&l).expect("each line is one JSON object"))
            .collect()
    }

    /// The steering headline: an `INSERT` delivers **exactly one** authenticated user message to
    /// the addressed session's own socket — and nothing at all to a second session's. The socket
    /// is the one the record published; the token is the one its key file carried.
    #[test]
    #[cfg(unix)]
    #[cfg_attr(not(target_os = "linux"), ignore = "liveness filtering needs /proc")]
    fn steering_delivers_one_authenticated_message_to_the_addressed_session() {
        use std::os::unix::net::UnixListener;

        let dir = TempDir::new().unwrap();
        let home = dir.path();
        let live_pid = u64::from(std::process::id());
        // Two live sessions, each with its own socket — only the addressed one may hear anything.
        write_record(home, live_pid, "s-target", "/tmp/p", "target", "idle");
        write_record(home, live_pid + 1, "s-other", "/tmp/p", "other", "idle");
        let target_sock = dir.path().join("target.sock");
        let other_sock = dir.path().join("other.sock");
        let target = UnixListener::bind(&target_sock).unwrap();
        let other = UnixListener::bind(&other_sock).unwrap();
        write_peer_address(home, live_pid, &target_sock, "tok-target");
        write_peer_address(home, live_pid + 1, &other_sock, "tok-other");

        let source = ClaudeStoreSource::new(home.to_path_buf());
        let accept =
            std::thread::spawn(move || read_lines(target.incoming().next().unwrap().unwrap()));
        assert_eq!(
            source
                .append_instruction("s-target", &steer("run the tests"))
                .unwrap(),
            1,
            "one steer, one affected row"
        );
        let lines = accept.join().unwrap();

        assert_eq!(lines.len(), 2, "the auth line then the user message");
        assert_eq!(lines[0]["type"], "auth");
        assert_eq!(lines[0]["token"], "tok-target", "the record's own token");
        assert_eq!(lines[1]["type"], "user");
        assert_eq!(lines[1]["message"]["role"], "user");
        assert_eq!(
            lines[1]["message"]["content"], "run the tests",
            "the instruction crosses as one JSON string value"
        );
        // The other session was never addressed: its listener has no pending connection.
        other.set_nonblocking(true).unwrap();
        assert!(
            other.accept().is_err(),
            "a steer must reach only the session it addressed"
        );
    }

    /// Instruction text is DATA: a payload full of newlines and JSON cannot forge a second
    /// protocol line — it arrives as one string value inside the message.
    #[test]
    #[cfg(unix)]
    #[cfg_attr(not(target_os = "linux"), ignore = "liveness filtering needs /proc")]
    fn a_multiline_instruction_cannot_forge_a_protocol_line() {
        use std::os::unix::net::UnixListener;

        let dir = TempDir::new().unwrap();
        let home = dir.path();
        let live_pid = u64::from(std::process::id());
        write_record(home, live_pid, "s-live", "/tmp/p", "live", "idle");
        let sock = dir.path().join("s.sock");
        let listener = UnixListener::bind(&sock).unwrap();
        write_peer_address(home, live_pid, &sock, "tok");

        let evil = "line one\n{\"type\":\"auth\",\"token\":\"forged\"}\nline three";
        let source = ClaudeStoreSource::new(home.to_path_buf());
        let accept =
            std::thread::spawn(move || read_lines(listener.incoming().next().unwrap().unwrap()));
        source.append_instruction("s-live", &steer(evil)).unwrap();
        let lines = accept.join().unwrap();

        assert_eq!(lines.len(), 2, "still exactly two protocol lines");
        assert_eq!(
            lines[1]["message"]["content"], evil,
            "verbatim, as one value"
        );
    }

    /// Concurrent steers cannot lose each other. The socket is a stream, not a read-modify-write
    /// file, so two appends are two independent connections — the drain-race class the file
    /// medium would have had cannot arise here, and this pins that property.
    #[test]
    #[cfg(unix)]
    #[cfg_attr(not(target_os = "linux"), ignore = "liveness filtering needs /proc")]
    fn concurrent_steers_do_not_lose_each_other() {
        use std::os::unix::net::UnixListener;
        use std::sync::Arc as StdArc;

        let dir = TempDir::new().unwrap();
        let home = dir.path();
        let live_pid = u64::from(std::process::id());
        write_record(home, live_pid, "s-live", "/tmp/p", "live", "idle");
        let sock = dir.path().join("s.sock");
        let listener = UnixListener::bind(&sock).unwrap();
        write_peer_address(home, live_pid, &sock, "tok");

        let accept = std::thread::spawn(move || {
            let mut seen = Vec::new();
            for conn in listener.incoming().take(2) {
                seen.push(read_lines(conn.unwrap()));
            }
            seen
        });
        let source = StdArc::new(ClaudeStoreSource::new(home.to_path_buf()));
        let writers: Vec<_> = ["first", "second"]
            .into_iter()
            .map(|text| {
                let source = StdArc::clone(&source);
                std::thread::spawn(move || source.append_instruction("s-live", &steer(text)))
            })
            .collect();
        for w in writers {
            assert_eq!(w.join().unwrap().unwrap(), 1);
        }

        let mut delivered: Vec<String> = accept
            .join()
            .unwrap()
            .iter()
            .map(|lines| lines[1]["message"]["content"].as_str().unwrap().to_string())
            .collect();
        delivered.sort();
        assert_eq!(delivered, vec!["first", "second"], "neither steer was lost");
    }

    /// A record with no `messagingSocketPath` (an older CLI) is refused with a reason that names
    /// what is missing — and **nothing is created on disk**: no socket, no directory, no inbox.
    #[test]
    #[cfg_attr(not(target_os = "linux"), ignore = "liveness filtering needs /proc")]
    fn steering_refuses_a_session_that_publishes_no_socket() {
        let dir = TempDir::new().unwrap();
        let home = dir.path();
        let live_pid = u64::from(std::process::id());
        write_record(home, live_pid, "s-live", "/tmp/p", "live", "idle");
        let before = entry_names(&home.join("sessions"));

        let source = ClaudeStoreSource::new(home.to_path_buf());
        let err = source
            .append_instruction("s-live", &steer("go"))
            .unwrap_err();
        assert!(matches!(err, ClaudeError::Source(_)));
        assert!(
            err.to_string().contains("messagingSocketPath"),
            "the refusal names what is missing: {err}"
        );
        assert_eq!(
            entry_names(&home.join("sessions")),
            before,
            "a refused steer creates nothing"
        );
        assert!(
            !home.join("teams").exists(),
            "and never invents a teams inbox (the spike proved one is never drained)"
        );
    }

    /// A published socket with no readable peer token is refused, not attempted — an unauthenticated
    /// connection would be dropped by the session anyway, and reporting success would be a lie.
    #[test]
    #[cfg_attr(not(target_os = "linux"), ignore = "liveness filtering needs /proc")]
    fn steering_refuses_when_no_peer_token_is_readable() {
        let dir = TempDir::new().unwrap();
        let home = dir.path();
        let live_pid = u64::from(std::process::id());
        write_record(home, live_pid, "s-live", "/tmp/p", "live", "idle");
        // The record names a socket, but no `<pid>.<hash>.key` sits beside it.
        let path = home.join("sessions").join(format!("{live_pid}.json"));
        let mut record: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        record["messagingSocketPath"] = serde_json::json!("/tmp/nowhere.sock");
        std::fs::write(&path, record.to_string()).unwrap();

        let source = ClaudeStoreSource::new(home.to_path_buf());
        let err = source
            .append_instruction("s-live", &steer("go"))
            .unwrap_err();
        assert!(
            err.to_string().contains("peer token"),
            "the refusal names the missing token: {err}"
        );
    }

    /// An unknown session id is `UnknownSession` — not a silent success, and not an I/O error.
    #[test]
    fn steering_refuses_an_unknown_session() {
        let (_d, source) = fixture();
        let err = source
            .append_instruction("s-nobody", &steer("go"))
            .unwrap_err();
        assert!(matches!(err, ClaudeError::UnknownSession { .. }), "{err}");
    }

    /// A leftover record of a dead process is refused: steering it would report success and reach
    /// nobody — the exact write-only no-op this seam exists to prevent.
    #[test]
    #[cfg_attr(not(target_os = "linux"), ignore = "dead-pid filtering needs /proc")]
    fn steering_refuses_a_leftover_record() {
        let (_d, source) = fixture();
        let err = source
            .append_instruction("s-dead", &steer("go"))
            .unwrap_err();
        assert!(
            err.to_string().contains("leftover"),
            "the refusal says the process is gone: {err}"
        );
    }

    /// A blank or absent instruction is a malformed effect, rejected **before** any session is
    /// addressed — a blank steer never opens a socket.
    #[test]
    fn steering_refuses_a_blank_instruction() {
        let (_d, source) = fixture();
        for batch in [
            steer("   "),
            RowBatch::new(claude_node_schema(ClaudeNode::Instructions), vec![]),
        ] {
            let err = source.append_instruction("s-live", &batch).unwrap_err();
            assert!(matches!(err, ClaudeError::MalformedEffect { .. }), "{err}");
        }
    }

    /// The instructions log reads back empty: the socket carries no queryable backlog, and an
    /// honest empty read beats replaying something nothing consumed.
    #[test]
    fn instructions_read_back_empty() {
        let (_d, source) = fixture();
        assert!(source.scan_instructions("s-live").unwrap().rows.is_empty());
    }

    /// The sorted file names directly under `dir` — the "nothing was created" comparison.
    fn entry_names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    /// `open_default` is fail-closed: with `QFS_CLAUDE_SESSIONS` unset there is no source (the
    /// process under test sets no such var), so `/claude`'s read facet is left unwired.
    #[test]
    fn open_default_is_fail_closed_without_env() {
        // The test process does not set QFS_CLAUDE_SESSIONS.
        assert!(std::env::var(CLAUDE_ENV).is_err());
        assert!(ClaudeStoreSource::open_default().is_none());
    }

    /// Write an executable fake `claude` binary that records its argv + cwd to `record` and prints
    /// the REAL `claude --bg` banner shape to stdout — the hermetic stand-in for the CLI (no real
    /// launch, no spend). The banner leads with `Starting background service…`, carries the id on a
    /// `backgrounded · <shortid>` line, then a help block — exactly the 2.1.217 output the launcher
    /// must parse (first-line capture would wrongly return the `Starting…` banner).
    #[cfg(unix)]
    fn write_fake_binary(path: &Path, record: &Path, session_id: &str) {
        use std::os::unix::fs::PermissionsExt;
        // `printf '%s\n' "$@"` writes each ARGUMENT on its own line — if our launcher had built a
        // shell line instead of discrete args, the recorded argv would be mangled. `pwd` records
        // the cwd the child ran in. Then print the multi-line banner (what the launcher parses).
        let script = format!(
            "#!/bin/sh\n{{ pwd; printf 'ARG:%s\\n' \"$@\"; }} > \"{record}\"\n\
             printf 'Starting background service\\342\\200\\246\\n'\n\
             printf 'backgrounded \\302\\267 {session_id}\\n'\n\
             printf '  claude agents             list sessions\\n'\n",
            record = record.display(),
        );
        std::fs::write(path, script).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    /// The launcher passes cwd/prompt/name as DISCRETE ARGUMENTS (no shell interpolation), spawns
    /// the configured binary in the given cwd, and captures the printed session id. The prompt is
    /// packed with shell metacharacters to prove it reaches the CLI verbatim (never expanded).
    #[test]
    #[cfg(unix)]
    fn launcher_passes_data_as_arguments_and_captures_the_id() {
        let dir = TempDir::new().unwrap();
        let bin = dir.path().join("fake-claude");
        let record = dir.path().join("argv.txt");
        write_fake_binary(&bin, &record, "s-launched-7");
        // A workdir that exists (the launcher validates it before spawning).
        let workdir = dir.path().join("work");
        std::fs::create_dir(&workdir).unwrap();

        let launcher = ClaudeCliLauncher::new(&bin);
        let evil_prompt = "$(rm -rf /); `whoami`; a && b | c > d";
        let spec = LaunchSpec {
            cwd: workdir.to_string_lossy().into_owned(),
            prompt: evil_prompt.to_string(),
            name: Some("nightly-run".to_string()),
        };
        let id = launcher.launch(&spec).unwrap();
        assert_eq!(id, "s-launched-7", "the printed session id is captured");

        let recorded = std::fs::read_to_string(&record).unwrap();
        let lines: Vec<&str> = recorded.lines().collect();
        // Line 0 is the child's cwd — the launcher set current_dir.
        assert_eq!(
            std::fs::canonicalize(lines[0]).unwrap(),
            std::fs::canonicalize(&workdir).unwrap(),
            "the session ran in the requested cwd"
        );
        // The remaining lines are the argv, each prefixed `ARG:`. The metacharacter-laden prompt is
        // ONE verbatim argument — proof there was no shell interpolation.
        let args: Vec<&str> = lines[1..].iter().map(|l| &l[4..]).collect();
        assert_eq!(args[0], "--bg");
        assert_eq!(
            args[1], evil_prompt,
            "the prompt crossed as one literal argument"
        );
        assert_eq!(args[2], "--name");
        assert_eq!(args[3], "nightly-run");
    }

    /// The RECORDED real `claude --bg` banner (Claude Code 2.1.217, live-fire 2026-07-22): the id
    /// is on the `backgrounded · <shortid>` line, NOT the leading `Starting background service…`
    /// line. The parser returns the short handle, never the banner text.
    #[test]
    fn parse_backgrounded_id_reads_the_recorded_banner() {
        // The exact bytes the CLI printed: an ellipsis `…` and a middle-dot `·` separator.
        let banner = "Starting background service\u{2026}\n\
                      backgrounded \u{b7} eb5300ad\n\
                      \x20 claude agents             list sessions\n\
                      \x20 claude logs               tail output\n";
        assert_eq!(
            parse_backgrounded_id(banner),
            Some("eb5300ad".to_string()),
            "the id comes from the `backgrounded` line, not the `Starting…` first line"
        );
    }

    /// No `backgrounded` line ⇒ no id: the parser refuses rather than inventing one (e.g. from the
    /// banner's first line), so `launch` fails closed instead of handing back garbage.
    #[test]
    fn parse_backgrounded_id_absent_line_is_none() {
        assert_eq!(
            parse_backgrounded_id("Starting background service\u{2026}\n"),
            None
        );
        assert_eq!(parse_backgrounded_id(""), None);
    }

    /// A launch whose cwd does not exist is a structured, secret-free refusal — never a spawn.
    #[test]
    fn launcher_rejects_a_missing_cwd() {
        let launcher = ClaudeCliLauncher::new("claude");
        let spec = LaunchSpec {
            cwd: "/no/such/dir/anywhere".to_string(),
            prompt: "go".to_string(),
            name: None,
        };
        let err = launcher.launch(&spec).unwrap_err();
        assert!(
            matches!(err, ClaudeError::LaunchFailed { .. }),
            "bad cwd is a structured LaunchFailed"
        );
        // Secret-free: names the reason, not a path payload / credential.
        assert!(err.to_string().contains("working directory"));
    }

    /// `open_default` reads the binary path from `QFS_CLAUDE_BINARY`, defaulting to `claude`. It
    /// always returns a launcher — the fail-closed gate is the session store, not the binary path.
    #[test]
    fn launcher_open_default_defaults_to_claude() {
        // The test process sets no QFS_CLAUDE_BINARY.
        assert!(std::env::var(CLAUDE_BINARY_ENV).is_err());
        let launcher = ClaudeCliLauncher::open_default();
        assert_eq!(launcher.binary, PathBuf::from("claude"));
    }

    /// The tail reader drops the partial first line of a mid-file window.
    #[test]
    fn read_tail_drops_the_partial_first_line() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("t.jsonl");
        let long = "x".repeat(100);
        std::fs::write(&path, format!("{long}\nsecond\nthird")).unwrap();
        let tail = read_tail(&path, 20).unwrap();
        assert_eq!(tail, "second\nthird");
    }
}
