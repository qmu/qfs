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
//! ## Steering is NOT wired yet (fail-closed, rewire ticket 20260717010500)
//! The retired layout's `instructions` append-log was written by qfs and read by **nothing** — an
//! append that steered no session. Until the rewire ticket lands a medium a live session actually
//! reads, [`SessionSource::append_instruction`] here fails closed with a structured error and the
//! instructions log reads back empty. Honest refusal over a write-only no-op.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use qfs_core::{CfsError, RowBatch};
use qfs_driver_claude::{
    claude_node_schema, instruction_session, node_for_path, ClaudeError, ClaudeNode, SessionSource,
};
use qfs_exec::ReadDriver;
use qfs_pushdown::ScanNode;
use qfs_types::{Row, Value};

/// The env var naming the Claude Code home dir (the parent of `sessions/` and `projects/`):
/// `QFS_CLAUDE_SESSIONS=<claude-home>`, conventionally `~/.claude`.
const CLAUDE_ENV: &str = "QFS_CLAUDE_SESSIONS";

/// How many bytes of a transcript tail are scanned for the last visible message. Transcripts grow
/// to many megabytes; the last visible text virtually always lives in the final few entries.
const TRANSCRIPT_TAIL_BYTES: u64 = 256 * 1024;

/// The bound on a surfaced `last_message` (characters). A relation cell is a summary surface, not
/// a transcript dump; the full transcript never leaves this module.
const LAST_MESSAGE_MAX_CHARS: usize = 2000;

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
        // Steering is not wired to a medium any session reads (rewire ticket 20260717010500);
        // until it is, the append-log truthfully reads back empty rather than replaying a file
        // nothing consumed.
        Ok(RowBatch::new(
            claude_node_schema(ClaudeNode::Instructions),
            Vec::new(),
        ))
    }

    fn append_instruction(&self, _session: &str, _row: &RowBatch) -> Result<u64, ClaudeError> {
        // Fail closed: the retired on-disk append-log was read by NO session — an append that
        // steers nothing. The rewire ticket (20260717010500) lands a medium a live session
        // actually reads; until then an honest refusal beats a write-only no-op.
        Err(ClaudeError::Source(
            "steering is not wired to a live session yet (rewire ticket 20260717010500); \
             the append is refused rather than written where no session reads"
                .to_string(),
        ))
    }
}

/// One parsed `sessions/<pid>.json` record — the selectors-only subset this module surfaces.
struct SessionRecord {
    pid: u64,
    id: String,
    cwd: Option<String>,
    name: Option<String>,
    status: String,
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
    async fn scan(&self, scan: &ScanNode) -> Result<RowBatch, CfsError> {
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

    /// Steering fails closed until the rewire ticket lands a medium a session actually reads.
    #[test]
    fn steering_fails_closed_pending_rewire() {
        let (_d, source) = fixture();
        let schema = qfs_types::Schema::new(vec![qfs_types::Column::new(
            "instruction",
            qfs_types::ColumnType::Text,
            false,
        )]);
        let row = RowBatch::new(schema, vec![Row::new(vec![Value::Text("hi".into())])]);
        let err = source.append_instruction("s-live", &row).unwrap_err();
        assert!(matches!(err, ClaudeError::Source(_)));
        assert!(
            err.to_string().contains("20260717010500"),
            "names the rewire ticket"
        );
        // The unwired append-log reads back empty (truthful, not an error).
        assert!(source.scan_instructions("s-live").unwrap().rows.is_empty());
    }

    /// `open_default` is fail-closed: with `QFS_CLAUDE_SESSIONS` unset there is no source (the
    /// process under test sets no such var), so `/claude`'s read facet is left unwired.
    #[test]
    fn open_default_is_fail_closed_without_env() {
        // The test process does not set QFS_CLAUDE_SESSIONS.
        assert!(std::env::var(CLAUDE_ENV).is_err());
        assert!(ClaudeStoreSource::open_default().is_none());
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
