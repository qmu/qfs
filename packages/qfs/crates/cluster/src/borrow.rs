//! Borrowed execution — "lend the query, not the secret".
//!
//! A member sends a [`crate::Frame::Request`]; the host decides, **before executing anything**,
//! whether every host path the statement touches is granted to that member ([`crate::grants`]),
//! then runs it through the host's own engine and answers with the result rows or an error. The
//! credential stays inside the host's engine: a frame carries only the statement and its result.
//!
//! This crate stays engine-agnostic: the binary injects the two engine seams — [`PathsOf`]
//! (parse the statement and list every host path it touches, failing closed when that cannot be
//! determined) and [`Execute`] (run it through the same one-shot path `qfs run` uses).

use std::io::Write as _;
use std::path::PathBuf;
use std::sync::Arc;

use serde_json::{json, Value};

use crate::grants::Grants;

/// The audit log file name inside the cluster state dir (one JSON line per borrowed request).
pub const AUDIT_FILE: &str = "audit.log";

/// What the host's statement analysis found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Touched {
    /// Every host path the statement touches, as segments.
    pub paths: Vec<Vec<String>>,
    /// Whether the statement itself forces an apply (a `COMMIT <stmt>` wrapper).
    pub forces_commit: bool,
}

/// Parse `statement` and list what it touches. `Err` refuses it (unparsable, or a construct
/// whose reach cannot be proven — fail closed).
pub type PathsOf = Arc<dyn Fn(&str) -> Result<Touched, String> + Send + Sync>;

/// Run `statement` on the host (`commit` = apply, else preview). `Ok` carries the result
/// envelope, `Err` the engine's error envelope. Called off the reactor.
pub type Execute = Arc<dyn Fn(&str, bool) -> Result<Value, Value> + Send + Sync>;

/// The host's borrowed-execution wiring.
#[derive(Clone)]
pub struct Borrow {
    /// The cluster state dir (grant table + audit log).
    pub state_dir: PathBuf,
    /// Statement analysis.
    pub paths_of: PathsOf,
    /// The engine.
    pub execute: Execute,
}

impl std::fmt::Debug for Borrow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Borrow")
            .field("state_dir", &self.state_dir)
            .finish_non_exhaustive()
    }
}

/// The decision for one request, before any execution.
fn authorize(b: &Borrow, member: &str, statement: &str) -> Result<Touched, String> {
    let touched = (b.paths_of)(statement)?;
    let grants = Grants::load(&b.state_dir)?;
    if let Some(denied) = grants.first_denied(member, &touched.paths) {
        return Err(format!(
            "`{denied}` is not granted to member `{member}` on this host \
             (the host operator runs `qfs cluster grant {member} <mount>`)"
        ));
    }
    Ok(touched)
}

/// Handle one borrowed request end to end: authorize, execute, audit. Returns `(ok, body)`.
/// Blocking — call it off the reactor.
#[must_use]
pub fn handle(b: &Borrow, member: &str, statement: &str, commit: bool) -> (bool, Value) {
    let (outcome, ok, body, effective_commit) = match authorize(b, member, statement) {
        Err(reason) => (
            "refused",
            false,
            json!({ "error": { "kind": "refused", "message": reason } }),
            commit,
        ),
        Ok(t) => {
            let c = commit || t.forces_commit;
            match (b.execute)(statement, commit) {
                Ok(v) => ("ok", true, v, c),
                Err(v) => ("error", false, v, c),
            }
        }
    };
    audit(b, member, statement, effective_commit, outcome, &body);
    (ok, body)
}

fn audit(b: &Borrow, member: &str, statement: &str, commit: bool, outcome: &str, body: &Value) {
    let mut line = json!({
        "ts": crate::now_secs(),
        "member": member,
        "statement": statement,
        "commit": commit,
        "outcome": outcome,
    });
    if outcome != "ok" {
        line["detail"] = body.clone();
    }
    let path = b.state_dir.join(AUDIT_FILE);
    let res = std::fs::create_dir_all(&b.state_dir).and_then(|()| {
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
