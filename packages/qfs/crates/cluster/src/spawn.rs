//! The member's opt-in agent runner: a [`Frame::Spawn`](crate::frame::Frame::Spawn) from the host
//! runs `claude -p <prompt>` (non-interactive Claude Code) as this machine's OS user and answers
//! with its stdout.
//!
//! Default deny, least privilege (ADR 0008, "Spawning an agent"):
//! - a member that did not join with `--accept-spawn` refuses every spawn ([`refused`]);
//! - `--allowedTools` is passed only when the host sent a non-empty list, so by default the
//!   agent may use no permission-gated tool;
//! - the agent runs in a fresh temp dir (removed afterwards) unless the member set a spawn root,
//!   and a host-requested directory must canonicalize under that root;
//! - it is killed at the timeout, and its captured output is capped;
//! - at most [`MAX_CONCURRENT`] agents run at once.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt as _};

/// The default spawn timeout.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(600);

/// The captured output cap (stdout and stderr each).
pub const OUTPUT_CAP: usize = 64 * 1024;

/// How many agents one member runs at once.
pub const MAX_CONCURRENT: usize = 4;

/// The env var naming the `claude` binary to run.
pub const CLAUDE_BIN_ENV: &str = "QFS_CLUSTER_CLAUDE_BIN";

/// The answer a member that did not opt in sends.
pub const NOT_ACCEPTED: &str = "spawn not accepted by this member";

/// A member's spawn policy (present only with `--accept-spawn`).
#[derive(Debug, Clone)]
pub struct SpawnConfig {
    /// The `claude` executable.
    pub claude_bin: PathBuf,
    /// The spawn root (`--spawn-cwd`): the default working directory and the only tree a
    /// host-requested directory may lie in. `None`: a fresh temp dir per run, and a
    /// host-requested directory is refused.
    pub root: Option<PathBuf>,
    /// Kill the agent after this long.
    pub timeout: Duration,
    /// Output cap in bytes.
    pub output_cap: usize,
}

impl SpawnConfig {
    /// A policy with the default timeout and cap.
    #[must_use]
    pub fn new(claude_bin: PathBuf, root: Option<PathBuf>) -> Self {
        Self {
            claude_bin,
            root,
            timeout: DEFAULT_TIMEOUT,
            output_cap: OUTPUT_CAP,
        }
    }
}

/// Resolve the `claude` binary: `$QFS_CLUSTER_CLAUDE_BIN`, else `claude` on `PATH`, else
/// `~/.local/bin/claude`.
#[must_use]
pub fn resolve_claude_bin() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os(CLAUDE_BIN_ENV).filter(|p| !p.is_empty()) {
        return Some(PathBuf::from(p));
    }
    if let Some(path) = std::env::var_os("PATH") {
        if let Some(p) = std::env::split_paths(&path)
            .map(|d| d.join("claude"))
            .find(|p| p.is_file())
        {
            return Some(p);
        }
    }
    let home = std::env::var_os("HOME").filter(|h| !h.is_empty())?;
    let p = PathBuf::from(home).join(".local/bin/claude");
    p.is_file().then_some(p)
}

/// One run's outcome: `(ok, output, exit_code)`.
pub type Outcome = (bool, String, Option<i32>);

/// The refusal sent when the member did not opt in.
#[must_use]
pub fn refused() -> Outcome {
    (false, NOT_ACCEPTED.into(), None)
}

/// The command line a run uses (without the binary): `-p <prompt>` plus `--allowedTools` only
/// for a non-empty allowlist.
#[must_use]
pub fn args(prompt: &str, allowed_tools: &[String]) -> Vec<String> {
    let mut a = vec!["-p".to_string(), prompt.to_string()];
    if !allowed_tools.is_empty() {
        a.push("--allowedTools".into());
        a.push(allowed_tools.join(","));
    }
    a
}

fn resolve_cwd(cfg: &SpawnConfig, requested: Option<&str>) -> Result<Option<PathBuf>, String> {
    let Some(req) = requested else {
        return Ok(cfg.root.clone());
    };
    let Some(root) = &cfg.root else {
        return Err("this member accepts no working directory from the host (join with --spawn-cwd to allow one under it)".into());
    };
    let root = root
        .canonicalize()
        .map_err(|e| format!("the spawn root is unusable ({e})"))?;
    let p = Path::new(req);
    let p = if p.is_absolute() {
        p.to_path_buf()
    } else {
        root.join(p)
    };
    let p = p
        .canonicalize()
        .map_err(|e| format!("the working directory `{req}` is unusable ({e})"))?;
    if !p.starts_with(&root) || !p.is_dir() {
        return Err(format!(
            "the working directory `{req}` is not a directory under the spawn root"
        ));
    }
    Ok(Some(p))
}

/// How long the output readers may keep draining after the agent exited or was killed (a
/// grandchild the agent left behind can hold the pipe open).
const DRAIN_GRACE: Duration = Duration::from_secs(2);

/// Captured bytes and whether the cap truncated them, shared with the reader task so a reader
/// abandoned after [`DRAIN_GRACE`] still leaves what it read.
type Captured = std::sync::Arc<std::sync::Mutex<(Vec<u8>, bool)>>;

async fn read_capped<R: AsyncRead + Unpin>(mut r: R, cap: usize, into: Captured) {
    let mut chunk = [0u8; 8192];
    loop {
        match r.read(&mut chunk).await {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                let mut g = match into.lock() {
                    Ok(g) => g,
                    Err(p) => p.into_inner(),
                };
                let room = cap.saturating_sub(g.0.len());
                if n > room {
                    g.1 = true;
                }
                g.0.extend_from_slice(&chunk[..n.min(room)]);
            }
        }
    }
}

fn take(c: &Captured) -> (Vec<u8>, bool) {
    match c.lock() {
        Ok(g) => g.clone(),
        Err(p) => p.into_inner().clone(),
    }
}

fn text(bytes: &[u8], truncated: bool) -> String {
    let mut s = String::from_utf8_lossy(bytes).trim().to_string();
    if truncated {
        s.push_str("\n[output truncated]");
    }
    s
}

/// Run one agent under `cfg`.
pub async fn run(
    cfg: &SpawnConfig,
    id: u64,
    prompt: &str,
    cwd: Option<&str>,
    allowed_tools: &[String],
) -> Outcome {
    let dir = match resolve_cwd(cfg, cwd) {
        Ok(d) => d,
        Err(e) => return (false, e, None),
    };
    let temp = if dir.is_none() {
        let t = std::env::temp_dir().join(format!(
            "qfs-spawn-{}-{id}-{}",
            std::process::id(),
            rand::random::<u32>()
        ));
        if let Err(e) = std::fs::create_dir_all(&t) {
            return (false, format!("cannot create a temp dir ({e})"), None);
        }
        Some(t)
    } else {
        None
    };
    let workdir = dir.or_else(|| temp.clone()).unwrap_or_default();
    let out = run_in(cfg, prompt, &workdir, allowed_tools).await;
    if let Some(t) = temp {
        let _ = std::fs::remove_dir_all(t);
    }
    out
}

async fn run_in(cfg: &SpawnConfig, prompt: &str, dir: &Path, allowed_tools: &[String]) -> Outcome {
    let mut child = match tokio::process::Command::new(&cfg.claude_bin)
        .args(args(prompt, allowed_tools))
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            return (
                false,
                format!("cannot start `{}` ({e})", cfg.claude_bin.display()),
                None,
            )
        }
    };
    let (Some(stdout), Some(stderr)) = (child.stdout.take(), child.stderr.take()) else {
        return (
            false,
            "the agent's output could not be captured".into(),
            None,
        );
    };
    let cap = cfg.output_cap;
    let (out_buf, err_buf) = (Captured::default(), Captured::default());
    let out_task = tokio::spawn(read_capped(stdout, cap, out_buf.clone()));
    let err_task = tokio::spawn(read_capped(stderr, cap, err_buf.clone()));
    let waited = tokio::time::timeout(cfg.timeout, child.wait()).await;
    let timed_out = waited.is_err();
    if timed_out {
        let _ = child.kill().await;
    }
    let (oa, ea) = (out_task.abort_handle(), err_task.abort_handle());
    if tokio::time::timeout(DRAIN_GRACE, async {
        let _ = out_task.await;
        let _ = err_task.await;
    })
    .await
    .is_err()
    {
        oa.abort();
        ea.abort();
    }
    let (out, out_trunc) = take(&out_buf);
    let (err, err_trunc) = take(&err_buf);
    let stdout = text(&out, out_trunc);
    let stderr = text(&err, err_trunc);
    match waited {
        Err(_) => (
            false,
            format!(
                "the agent timed out after {}s and was killed{}",
                cfg.timeout.as_secs(),
                if stdout.is_empty() {
                    String::new()
                } else {
                    format!("; partial output:\n{stdout}")
                }
            ),
            None,
        ),
        Ok(Err(e)) => (false, format!("waiting for the agent failed ({e})"), None),
        Ok(Ok(status)) if status.success() => (true, stdout, status.code()),
        Ok(Ok(status)) => {
            let mut msg = format!("the agent exited with {status}");
            for part in [stdout, stderr] {
                if !part.is_empty() {
                    msg.push('\n');
                    msg.push_str(&part);
                }
            }
            (false, msg, status.code())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allowed_tools_are_passed_only_when_non_empty() {
        assert_eq!(args("hi", &[]), ["-p", "hi"]);
        assert_eq!(
            args("hi", &["Read".into(), "Bash(git log:*)".into()]),
            ["-p", "hi", "--allowedTools", "Read,Bash(git log:*)"]
        );
    }

    #[test]
    fn a_host_cwd_needs_a_spawn_root_and_must_stay_under_it() {
        let cfg = SpawnConfig::new("claude".into(), None);
        assert!(resolve_cwd(&cfg, Some("/tmp")).is_err());
        assert_eq!(resolve_cwd(&cfg, None).unwrap(), None);
        let root = std::env::temp_dir().join(format!("qfs-spawn-root-{}", std::process::id()));
        std::fs::create_dir_all(root.join("sub")).unwrap();
        let cfg = SpawnConfig::new("claude".into(), Some(root.clone()));
        assert!(resolve_cwd(&cfg, Some("sub")).unwrap().is_some());
        assert!(resolve_cwd(&cfg, Some("..")).is_err());
        assert!(resolve_cwd(&cfg, Some("/")).is_err());
        let _ = std::fs::remove_dir_all(root);
    }
}
