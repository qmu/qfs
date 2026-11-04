//! The host's agent-run table: one row per `Spawn` the host dispatched to a member, plus the
//! live per-member links a spawn is sent over. In memory only (like the registry); every spawn
//! and every result is also appended to the cluster audit log when a state dir is known.
//!
//! A run moves `running` → `done` | `failed`. It fails when the member is not connected, when
//! the member refuses (it did not join with `--accept-spawn`), when the agent exits non-zero or
//! times out, or when the member disconnects before answering.

use std::collections::BTreeMap;
use std::io::Write as _;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use tokio::sync::mpsc::UnboundedSender;

use crate::frame::Frame;

/// The table keeps at most this many runs (oldest dropped first).
pub const MAX_RUNS: usize = 500;

/// The longest prompt the host accepts.
pub const MAX_PROMPT: usize = 32 * 1024;

/// A `/api/cluster/agents` row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentRun {
    /// Run id.
    pub id: u64,
    /// The member it was sent to.
    pub member: String,
    /// The prompt.
    pub prompt: String,
    /// The requested working directory (`None`: the member's default).
    pub cwd: Option<String>,
    /// The tool allowlist sent (empty: none).
    pub allowed_tools: Vec<String>,
    /// `running`, `done` or `failed`.
    pub status: String,
    /// The answer, or why it failed.
    pub output: String,
    /// The agent's exit code, when it exited.
    pub exit_code: Option<i32>,
    /// Dispatch time, epoch seconds.
    pub started: u64,
    /// Completion time, epoch seconds.
    pub finished: Option<u64>,
}

/// A spawn request (the `POST /api/cluster/agents` body).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpawnRequest {
    /// One member by name …
    #[serde(default)]
    pub member: Option<String>,
    /// … or every connected member.
    #[serde(default)]
    pub all: bool,
    /// The prompt.
    pub prompt: String,
    /// Working directory on the member (must lie under its `--spawn-cwd`).
    #[serde(default)]
    pub cwd: Option<String>,
    /// Tool allowlist (default none).
    #[serde(default)]
    pub allowed_tools: Vec<String>,
}

#[derive(Debug, Default)]
struct Inner {
    next_id: u64,
    next_link: u64,
    runs: BTreeMap<u64, AgentRun>,
    links: BTreeMap<String, (u64, UnboundedSender<Frame>)>,
    audit_dir: Option<PathBuf>,
}

/// The shared run-table handle.
#[derive(Debug, Clone, Default)]
pub struct Agents {
    inner: Arc<Mutex<Inner>>,
}

impl Agents {
    fn with<R>(&self, f: impl FnOnce(&mut Inner) -> R) -> R {
        let mut g = match self.inner.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        f(&mut g)
    }

    /// Append spawns and results to `<dir>/audit.log`.
    pub fn set_audit_dir(&self, dir: PathBuf) {
        self.with(|i| i.audit_dir = Some(dir));
    }

    /// Register a joined member's live link; returns a link id for [`Self::unlink`].
    pub fn link(&self, member: &str, tx: UnboundedSender<Frame>) -> u64 {
        self.with(|i| {
            i.next_link += 1;
            let id = i.next_link;
            i.links.insert(member.to_string(), (id, tx));
            id
        })
    }

    /// Drop a member's link (only if it is still `link_id`) and fail its running runs.
    pub fn unlink(&self, member: &str, link_id: u64) {
        let failed: Vec<AgentRun> = self.with(|i| {
            if i.links.get(member).is_some_and(|(id, _)| *id == link_id) {
                i.links.remove(member);
            } else {
                return Vec::new();
            }
            i.runs
                .values_mut()
                .filter(|r| r.member == member && r.status == "running")
                .map(|r| {
                    finish_run(r, false, "the member disconnected before answering", None);
                    r.clone()
                })
                .collect()
        });
        for r in &failed {
            self.audit_result(r);
        }
    }

    /// Validate and dispatch a spawn request; returns the created runs.
    ///
    /// # Errors
    /// A message for an invalid request (empty prompt, no target, both targets, too long).
    pub fn spawn(&self, req: &SpawnRequest) -> Result<Vec<AgentRun>, String> {
        if req.prompt.trim().is_empty() {
            return Err("the prompt must not be empty".into());
        }
        if req.prompt.len() > MAX_PROMPT {
            return Err(format!("the prompt exceeds {MAX_PROMPT} bytes"));
        }
        let targets: Vec<String> = match (&req.member, req.all) {
            (Some(_), true) => return Err("give a member or all, not both".into()),
            (None, false) => return Err("give a member or all".into()),
            (Some(m), false) if m.trim().is_empty() => return Err("empty member name".into()),
            (Some(m), false) => vec![m.trim().to_string()],
            (None, true) => self.with(|i| i.links.keys().cloned().collect()),
        };
        let tools: Vec<String> = req
            .allowed_tools
            .iter()
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty())
            .collect();
        let cwd = req.cwd.clone().filter(|c| !c.trim().is_empty());
        let runs: Vec<AgentRun> = targets
            .iter()
            .map(|member| {
                self.with(|i| {
                    i.next_id += 1;
                    let id = i.next_id;
                    let mut run = AgentRun {
                        id,
                        member: member.clone(),
                        prompt: req.prompt.clone(),
                        cwd: cwd.clone(),
                        allowed_tools: tools.clone(),
                        status: "running".into(),
                        output: String::new(),
                        exit_code: None,
                        started: crate::now_secs(),
                        finished: None,
                    };
                    let frame = Frame::Spawn {
                        id,
                        prompt: req.prompt.clone(),
                        cwd: cwd.clone(),
                        allowed_tools: tools.clone(),
                    };
                    let sent = i
                        .links
                        .get(member)
                        .is_some_and(|(_, tx)| tx.send(frame).is_ok());
                    if !sent {
                        finish_run(&mut run, false, "the member is not connected", None);
                    }
                    i.runs.insert(id, run.clone());
                    while i.runs.len() > MAX_RUNS {
                        i.runs.pop_first();
                    }
                    run
                })
            })
            .collect();
        for r in &runs {
            self.audit_spawn(r);
            if r.status != "running" {
                self.audit_result(r);
            }
        }
        Ok(runs)
    }

    /// Record a member's [`Frame::SpawnResult`]. A result for an unknown run, from a member the
    /// run was not sent to, or for a finished run is ignored.
    pub fn finish(&self, member: &str, id: u64, ok: bool, output: &str, exit_code: Option<i32>) {
        let done = self.with(|i| {
            let r = i.runs.get_mut(&id)?;
            if r.member != member || r.status != "running" {
                return None;
            }
            finish_run(r, ok, output, exit_code);
            Some(r.clone())
        });
        if let Some(r) = done {
            self.audit_result(&r);
        }
    }

    /// Every run, oldest first.
    #[must_use]
    pub fn runs(&self) -> Vec<AgentRun> {
        self.with(|i| i.runs.values().cloned().collect())
    }

    /// One run.
    #[must_use]
    pub fn get(&self, id: u64) -> Option<AgentRun> {
        self.with(|i| i.runs.get(&id).cloned())
    }

    fn audit_spawn(&self, r: &AgentRun) {
        self.audit(&serde_json::json!({
            "ts": crate::now_secs(),
            "kind": "spawn",
            "id": r.id,
            "member": r.member,
            "prompt": r.prompt,
            "cwd": r.cwd,
            "allowed_tools": r.allowed_tools,
        }));
    }

    fn audit_result(&self, r: &AgentRun) {
        self.audit(&serde_json::json!({
            "ts": crate::now_secs(),
            "kind": "spawn_result",
            "id": r.id,
            "member": r.member,
            "status": r.status,
            "exit_code": r.exit_code,
            "output_bytes": r.output.len(),
        }));
    }

    fn audit(&self, line: &serde_json::Value) {
        let Some(dir) = self.with(|i| i.audit_dir.clone()) else {
            return;
        };
        let path = dir.join(crate::borrow::AUDIT_FILE);
        let res = std::fs::create_dir_all(&dir).and_then(|()| {
            let mut f = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)?;
            writeln!(f, "{line}")
        });
        if let Err(e) = res {
            eprintln!(
                "qfs cluster: cannot append the audit log {} ({e})",
                path.display()
            );
        }
    }
}

fn finish_run(r: &mut AgentRun, ok: bool, output: &str, exit_code: Option<i32>) {
    r.status = if ok { "done" } else { "failed" }.into();
    r.output = output.to_string();
    r.exit_code = exit_code;
    r.finished = Some(crate::now_secs());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(member: Option<&str>, all: bool) -> SpawnRequest {
        SpawnRequest {
            member: member.map(str::to_string),
            all,
            prompt: "joke".into(),
            ..SpawnRequest::default()
        }
    }

    #[test]
    fn an_unconnected_member_records_a_failed_run() {
        let a = Agents::default();
        let runs = a.spawn(&req(Some("ghost"), false)).unwrap();
        assert_eq!(runs[0].status, "failed");
        assert_eq!(a.runs().len(), 1);
    }

    #[test]
    fn invalid_requests_are_refused() {
        let a = Agents::default();
        assert!(a.spawn(&req(None, false)).is_err());
        assert!(a.spawn(&req(Some("m"), true)).is_err());
        let mut r = req(Some("m"), false);
        r.prompt = "  ".into();
        assert!(a.spawn(&r).is_err());
    }

    #[test]
    fn a_result_finishes_only_its_own_members_run_and_unlink_fails_the_rest() {
        let a = Agents::default();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let link = a.link("m", tx);
        let runs = a.spawn(&req(None, true)).unwrap();
        assert_eq!(runs.len(), 1);
        assert!(matches!(rx.try_recv(), Ok(Frame::Spawn { .. })));
        a.finish("other", runs[0].id, true, "spoofed", Some(0));
        assert_eq!(a.get(runs[0].id).unwrap().status, "running");
        let second = a.spawn(&req(Some("m"), false)).unwrap();
        a.finish("m", runs[0].id, true, "ha", Some(0));
        assert_eq!(a.get(runs[0].id).unwrap().output, "ha");
        a.unlink("m", link);
        assert_eq!(a.get(second[0].id).unwrap().status, "failed");
    }
}
