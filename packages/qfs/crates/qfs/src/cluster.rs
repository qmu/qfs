//! `qfs cluster host|token|join|members|sessions` — the binary composition root for the cluster.
//!
//! The protocol, registry and listener live in `qfs-cluster`; this module owns what only the binary
//! may: the persisted cluster secret under the qfs config dir, the tokio runtime, and the member's
//! Claude Code session rows (read through the same [`ClaudeStoreSource`] the `/claude` driver
//! uses). Trust model: `docs/adr/0008-cluster-trust-model.md`.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use qfs_cluster::sample::Sampler;
use qfs_cluster::{MemberConfig, Registry, Report, SessionReport};
use qfs_cmd::ClusterRequest;
use qfs_driver_claude::SessionSource as _;
use qfs_types::Value;

use crate::claude::ClaudeStoreSource;

/// The secret's file name inside the cluster state dir.
const SECRET_FILE: &str = "secret";

/// The default cluster state dir: `<qfs config dir>/cluster` (beside the System DB, so the test
/// build's shared-home guard applies to it too).
fn default_state_dir() -> Option<PathBuf> {
    crate::store::default_system_db_path().and_then(|p| p.parent().map(|d| d.join("cluster")))
}

fn state_dir(explicit: Option<&Path>) -> Result<PathBuf, String> {
    explicit
        .map(Path::to_path_buf)
        .or_else(default_state_dir)
        .ok_or_else(|| "no config home (set HOME or XDG_CONFIG_HOME, or pass --state-dir)".into())
}

/// Load the host's cluster secret from `dir`, creating a random 32-byte one (mode 0600) on first
/// use. The secret never leaves the host and is never printed.
///
/// # Errors
/// A message naming the I/O failure or a corrupt secret file.
pub fn load_or_create_secret(dir: &Path) -> Result<Vec<u8>, String> {
    let path = dir.join(SECRET_FILE);
    match std::fs::read_to_string(&path) {
        Ok(hex) => decode_hex(hex.trim())
            .filter(|b| b.len() == 32)
            .ok_or_else(|| format!("the cluster secret at {} is corrupt", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            std::fs::create_dir_all(dir)
                .map_err(|e| format!("cannot create {} ({e})", dir.display()))?;
            let secret = qfs_cluster::token::new_secret();
            // Write a private temp file, then hard-link it into place: the link is atomic and
            // fails if another process (a concurrent `host` / `token`) won the race, in which case
            // we read the winner's secret instead of clobbering it or reading a half-written file.
            let tmp = dir.join(format!("{SECRET_FILE}.{}.tmp", std::process::id()));
            let _ = std::fs::remove_file(&tmp);
            write_private(&tmp, &encode_hex(&secret))
                .map_err(|e| format!("cannot write {} ({e})", tmp.display()))?;
            let linked = std::fs::hard_link(&tmp, &path);
            let _ = std::fs::remove_file(&tmp);
            match linked {
                Ok(()) => Ok(secret.to_vec()),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    load_or_create_secret(dir)
                }
                Err(e) => Err(format!("cannot write {} ({e})", path.display())),
            }
        }
        Err(e) => Err(format!("cannot read {} ({e})", path.display())),
    }
}

#[cfg(unix)]
fn write_private(path: &Path, text: &str) -> std::io::Result<()> {
    use std::io::Write as _;
    use std::os::unix::fs::OpenOptionsExt as _;
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    f.write_all(text.as_bytes())
}

#[cfg(not(unix))]
fn write_private(path: &Path, text: &str) -> std::io::Result<()> {
    std::fs::write(path, text)
}

fn encode_hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn decode_hex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| s.get(i..i + 2).and_then(|h| u8::from_str_radix(h, 16).ok()))
        .collect()
}

/// The member's Claude session source: `QFS_CLAUDE_SESSIONS` when set, else `~/.claude` when it
/// exists, else none (an empty session list).
fn claude_source() -> Option<ClaudeStoreSource> {
    ClaudeStoreSource::open_default().or_else(|| {
        let home = std::env::var("HOME").ok().filter(|h| !h.is_empty())?;
        let dir = PathBuf::from(home).join(".claude");
        dir.is_dir().then(|| ClaudeStoreSource::new(dir))
    })
}

fn text(v: Option<&Value>) -> Option<String> {
    match v {
        Some(Value::Text(s)) => Some(s.clone()),
        _ => None,
    }
}

/// Read the session rows as reports (a read failure is an empty list, never fatal).
fn session_reports(source: Option<&ClaudeStoreSource>) -> Vec<SessionReport> {
    let Some(batch) = source.and_then(|s| s.scan_sessions().ok()) else {
        return Vec::new();
    };
    let idx = |name: &str| {
        batch
            .schema
            .columns
            .iter()
            .position(|c| c.name.as_str() == name)
    };
    let (id, cwd, nm, st, lm) = (
        idx("id"),
        idx("cwd"),
        idx("name"),
        idx("status"),
        idx("last_message"),
    );
    let cell = |row: &qfs_types::Row, i: Option<usize>| text(i.and_then(|i| row.values.get(i)));
    batch
        .rows
        .iter()
        .filter_map(|row| {
            Some(SessionReport {
                id: cell(row, id)?,
                cwd: cell(row, cwd),
                name: cell(row, nm),
                status: cell(row, st),
                last_message: cell(row, lm),
            })
        })
        .collect()
}

fn runtime() -> Result<tokio::runtime::Runtime, String> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| format!("cannot start the async runtime ({e})"))
}

/// The binary's [`qfs_cmd::ClusterLauncher`].
#[must_use]
pub fn run_cluster(req: &ClusterRequest) -> i32 {
    match run(req) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("qfs: error: {e}");
            1
        }
    }
}

fn run(req: &ClusterRequest) -> Result<(), String> {
    match req {
        ClusterRequest::Token {
            name,
            ttl,
            state_dir: dir,
        } => {
            if name.trim().is_empty() {
                return Err("--name must not be empty".into());
            }
            let ttl = qfs_cluster::token::parse_ttl(ttl)?;
            let secret = load_or_create_secret(&state_dir(dir.as_deref())?)?;
            println!(
                "{}",
                qfs_cluster::mint(&secret, name, ttl, qfs_cluster::now_secs())
            );
            Ok(())
        }
        ClusterRequest::Host {
            listen,
            state_dir: dir,
        } => {
            let secret = Arc::new(load_or_create_secret(&state_dir(dir.as_deref())?)?);
            runtime()?.block_on(async move {
                let listener = tokio::net::TcpListener::bind(listen.as_str())
                    .await
                    .map_err(|e| format!("cannot listen on {listen} ({e})"))?;
                let addr = listener.local_addr().map_err(|e| e.to_string())?;
                if !addr.ip().is_loopback() {
                    eprintln!(
                        "qfs cluster: WARNING: listening on {addr} beyond loopback over plain ws:// \
                         — use only on a trusted LAN or behind an SSH/TLS tunnel"
                    );
                }
                eprintln!("qfs cluster: host listening on {addr} (members join ws://{addr})");
                eprintln!("qfs cluster: read API http://{addr}/api/cluster/members (loopback only)");
                qfs_cluster::serve(listener, secret, Registry::new())
                    .await
                    .map_err(|e| e.to_string())
            })
        }
        ClusterRequest::Join {
            url,
            token,
            name,
            heartbeat_secs,
        } => {
            if !heartbeat_secs.is_finite() || *heartbeat_secs < 0.05 {
                return Err("--heartbeat must be at least 0.05 seconds".into());
            }
            let cfg = MemberConfig {
                url: url.clone(),
                token: token.clone(),
                name: name.clone().unwrap_or_default(),
                heartbeat: Duration::from_secs_f64(*heartbeat_secs),
            };
            let sampler = Mutex::new(Sampler::new());
            let claude = claude_source();
            let source: qfs_cluster::member::ReportSource = Arc::new(move || {
                let (hostname, cpu_pct, mem_used, mem_total, disk_used, disk_total) =
                    match sampler.lock() {
                        Ok(mut s) => s.sample(),
                        Err(p) => p.into_inner().sample(),
                    };
                Report {
                    hostname,
                    cpu_pct,
                    mem_used,
                    mem_total,
                    disk_used,
                    disk_total,
                    sessions: session_reports(claude.as_ref()),
                }
            });
            runtime()?
                .block_on(qfs_cluster::run_member(cfg, source))
                .map_err(|e| e.to_string())
        }
        ClusterRequest::Members { host } => print_json(host, "/api/cluster/members"),
        ClusterRequest::Sessions { host } => print_json(host, "/api/cluster/sessions"),
    }
}

fn print_json(host: &str, path: &str) -> Result<(), String> {
    let body = qfs_cluster::fetch_json(host, path)?;
    let v: serde_json::Value = serde_json::from_str(&body).map_err(|e| e.to_string())?;
    println!(
        "{}",
        serde_json::to_string_pretty(&v).map_err(|e| e.to_string())?
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testenv::HomeGuard;

    #[test]
    fn the_secret_is_created_once_under_the_config_home_and_reused() {
        let _home = HomeGuard::new();
        let dir = state_dir(None).unwrap();
        let a = load_or_create_secret(&dir).unwrap();
        let b = load_or_create_secret(&dir).unwrap();
        assert_eq!(a.len(), 32);
        assert_eq!(a, b);
        let t = qfs_cluster::mint(&a, "m", 60, 100);
        assert!(qfs_cluster::verify(&b, &t, Some("m"), 101).is_ok());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mode = std::fs::metadata(dir.join(SECRET_FILE))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    #[test]
    fn a_corrupt_secret_is_an_error_not_a_new_secret() {
        let _home = HomeGuard::new();
        let dir = state_dir(None).unwrap();
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(SECRET_FILE), "zz").unwrap();
        assert!(load_or_create_secret(&dir).is_err());
    }

    #[test]
    fn hex_round_trips() {
        assert_eq!(
            decode_hex(&encode_hex(&[0, 255, 16])),
            Some(vec![0, 255, 16])
        );
    }
}
