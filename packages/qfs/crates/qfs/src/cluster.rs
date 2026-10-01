//! `qfs cluster host|token|join|members|sessions|grant|revoke|grants|run|account` — the binary composition
//! root for the cluster.
//!
//! The protocol, registry and listener live in `qfs-cluster`; this module owns what only the binary
//! may: the persisted cluster secret under the qfs config dir, the tokio runtime, and the member's
//! Claude Code session rows (read through the same [`ClaudeStoreSource`] the `/claude` driver
//! uses). It also owns the two engine seams of **borrowed execution**: [`touched_paths`] (parse a
//! member's statement and list every host path it touches, failing closed) and [`execute_on_host`]
//! (run it through the same one-shot path `qfs run` uses, irreversible gate included). Trust
//! model: `docs/adr/0008-cluster-trust-model.md`.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use qfs_cluster::sample::Sampler;
use qfs_cluster::{
    Accounts, Borrow, Grants, MemberConfig, Registry, Report, SessionReport, Touched,
};
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

/// Walk a serialized statement AST and collect every host path it touches. Paths come from every
/// `PathExpr` (`segments`), every `FOLLOW … INTO` template, and every `CALL <driver>.<action>`.
/// Constructs whose reach is not a host mount path are refused (fail closed): server DDL (a
/// member must not define bindings on the host), `TRANSFORM` (it spends the host's model
/// provider), and a bare-name source that is not bound by a `LET` in the same statement.
fn walk_ast(
    v: &serde_json::Value,
    lets: &mut Vec<String>,
    out: &mut Touched,
) -> Result<(), String> {
    use serde_json::Value as J;
    match v {
        J::Array(items) => items.iter().try_for_each(|i| walk_ast(i, lets, out)),
        J::Object(map) => {
            if map.contains_key("Ddl") {
                return Err("server DDL (CREATE …) cannot be borrowed".into());
            }
            if map.contains_key("Transform") {
                return Err(
                    "a TRANSFORM stage cannot be borrowed (it spends the host's model provider)"
                        .into(),
                );
            }
            if let Some(J::Object(plan)) = map.get("Plan") {
                if plan.get("commit") == Some(&J::Bool(true)) {
                    out.forces_commit = true;
                }
            }
            if let Some(J::Object(l)) = map.get("Let") {
                if let Some(J::String(n)) = l.get("name") {
                    lets.push(n.clone());
                }
            }
            if let Some(J::String(n)) = map.get("Name") {
                if !lets.contains(n) {
                    return Err(format!(
                        "the bare source `{n}` is not a LET binding of this statement"
                    ));
                }
            }
            let seg_names = |segs: &J| -> Option<Vec<String>> {
                segs.as_array()?
                    .iter()
                    .map(|s| s.get("name").and_then(J::as_str).map(str::to_string))
                    .collect()
            };
            if let Some(segs) = map.get("segments") {
                let names = seg_names(segs).ok_or("a path could not be read")?;
                out.paths.push(names);
            }
            if let Some(into) = map.get("into").filter(|i| i.is_array()) {
                out.paths
                    .push(seg_names(into).ok_or("a FOLLOW target could not be read")?);
            }
            if let (Some(J::String(driver)), Some(J::String(_))) =
                (map.get("driver"), map.get("action"))
            {
                out.paths.push(vec![driver.clone()]);
            }
            map.values().try_for_each(|x| walk_ast(x, lets, out))
        }
        _ => Ok(()),
    }
}

/// The borrowed-execution statement analysis ([`qfs_cluster::PathsOf`]): parse `statement` on the
/// shipped grammar and list every host path it touches. Any failure refuses the statement.
///
/// # Errors
/// A parse error, or a construct whose reach cannot be proven (see [`walk_ast`]).
pub fn touched_paths(statement: &str) -> Result<Touched, String> {
    let stmt =
        qfs_exec::parse(statement).map_err(|e| format!("the statement does not parse: {e}"))?;
    let ast = serde_json::to_value(&stmt).map_err(|e| e.to_string())?;
    let mut out = Touched {
        paths: Vec::new(),
        forces_commit: false,
    };
    walk_ast(&ast, &mut Vec::new(), &mut out)?;
    if out.paths.iter().any(Vec::is_empty) {
        return Err("a path with no segments cannot be granted".into());
    }
    Ok(out)
}

/// Run one borrowed statement through the SAME one-shot path `qfs run` uses (the live run
/// context, the real commit applier, the resolved safety mode) with JSON output. The irreversible
/// acknowledgement is never given: an irreversible effect is refused exactly as a plain
/// `qfs run --commit` without `--commit-irreversible` refuses it. `Ok` is the result envelope,
/// `Err` the error envelope (with the process exit code under `exit_code`).
///
/// Runs on its own OS thread: the engine's drivers and the commit applier build their own tokio
/// runtimes, which must not nest inside the host's reactor.
///
/// # Errors
/// The engine's error envelope.
pub fn execute_on_host(
    statement: &str,
    commit: bool,
) -> Result<serde_json::Value, serde_json::Value> {
    let stmt = statement.to_string();
    std::thread::spawn(move || run_one(&stmt, commit))
        .join()
        .unwrap_or_else(|_| {
            Err(serde_json::json!({ "error": { "kind": "internal", "message": "the execution thread panicked" } }))
        })
}

fn run_one(statement: &str, commit: bool) -> Result<serde_json::Value, serde_json::Value> {
    let (engine, reads, safety_mode, transform) = crate::shell::run_context();
    let apply: &qfs_exec::WorldApply = &crate::commit::apply_plan;
    let ctx = qfs_exec::ExecCtx {
        engine: &engine,
        reads: &reads,
        world_apply: Some(apply),
        safety_mode,
        transform,
    };
    let source = qfs_exec::StmtSource::Positional(statement.to_string());
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = {
        let mut streams = qfs_exec::Streams {
            out: &mut out,
            err: &mut err,
        };
        qfs_exec::run_oneshot(
            &source,
            &ctx,
            qfs_exec::OutputFormat::Json,
            commit,
            false,
            &mut streams,
        )
        .code()
    };
    let as_json = |bytes: &[u8]| {
        let text = String::from_utf8_lossy(bytes);
        serde_json::from_str::<serde_json::Value>(text.trim())
            .unwrap_or_else(|_| serde_json::Value::String(text.trim().to_string()))
    };
    if code == 0 {
        Ok(as_json(&out))
    } else {
        let mut e = as_json(&err);
        if let serde_json::Value::Object(m) = &mut e {
            m.insert("exit_code".into(), code.into());
        } else {
            e = serde_json::json!({ "error": { "message": e }, "exit_code": code });
        }
        Err(e)
    }
}

fn env_or(flag: Option<&String>, var: &str) -> Option<String> {
    flag.cloned()
        .or_else(|| std::env::var(var).ok())
        .filter(|s| !s.trim().is_empty())
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
            let dir = state_dir(dir.as_deref())?;
            let secret = Arc::new(load_or_create_secret(&dir)?);
            // Refuse to start over a corrupt grant table rather than discover it per request.
            Grants::load(&dir)?;
            Accounts::load(&dir)?;
            let borrow = Borrow {
                state_dir: dir,
                paths_of: Arc::new(touched_paths),
                execute: Arc::new(execute_on_host),
            };
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
                eprintln!("qfs cluster: console http://{addr}/ (loopback only)");
                eprintln!("qfs cluster: read API http://{addr}/api/cluster/members (loopback only)");
                qfs_cluster::serve_with(listener, secret, Registry::new(), Some(borrow))
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
        ClusterRequest::Grant {
            member,
            mount,
            state_dir: dir,
        } => {
            let dir = state_dir(dir.as_deref())?;
            let mut g = Grants::load(&dir)?;
            let new = g.grant(member, mount)?;
            g.save(&dir)?;
            let m = qfs_cluster::grants::canonical(mount).unwrap_or_default();
            println!(
                "{} `{m}` to member `{}`",
                if new { "granted" } else { "already granted" },
                member.trim()
            );
            Ok(())
        }
        ClusterRequest::Revoke {
            member,
            mount,
            state_dir: dir,
        } => {
            let dir = state_dir(dir.as_deref())?;
            let mut g = Grants::load(&dir)?;
            if !g.revoke(member, mount) {
                return Err(format!("member `{member}` holds no grant `{mount}`"));
            }
            g.save(&dir)?;
            println!("revoked `{mount}` from member `{member}`");
            Ok(())
        }
        ClusterRequest::Grants { state_dir: dir } => {
            let g = Grants::load(&state_dir(dir.as_deref())?)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&g.members).map_err(|e| e.to_string())?
            );
            Ok(())
        }
        ClusterRequest::AccountAdd {
            provider,
            label,
            email,
            plan,
            state_dir: dir,
        } => {
            let dir = state_dir(dir.as_deref())?;
            let mut a = Accounts::load(&dir)?;
            a.add(provider, label, email.as_deref(), plan.as_deref())?;
            a.save(&dir)?;
            println!("added {provider} account `{}`", label.trim());
            Ok(())
        }
        ClusterRequest::AccountList { state_dir: dir } => {
            let a = Accounts::load(&state_dir(dir.as_deref())?)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&a.accounts).map_err(|e| e.to_string())?
            );
            Ok(())
        }
        ClusterRequest::AccountRemove {
            label,
            state_dir: dir,
        } => {
            let dir = state_dir(dir.as_deref())?;
            let mut a = Accounts::load(&dir)?;
            if !a.remove(label) {
                return Err(format!("no account labelled `{label}`"));
            }
            a.save(&dir)?;
            println!("removed account `{label}`");
            Ok(())
        }
        ClusterRequest::AccountAssign {
            label,
            member,
            state_dir: dir,
        } => {
            let dir = state_dir(dir.as_deref())?;
            let mut a = Accounts::load(&dir)?;
            a.assign(label, member.as_deref())?;
            a.save(&dir)?;
            match member {
                Some(m) => println!("assigned account `{label}` to member `{m}`"),
                None => println!("unassigned account `{label}`"),
            }
            Ok(())
        }
        ClusterRequest::Run {
            statement,
            commit,
            url,
            token,
            name,
        } => {
            let url = env_or(url.as_ref(), "QFS_CLUSTER_URL")
                .ok_or("no host URL (pass --host-url or set QFS_CLUSTER_URL)")?;
            let token = env_or(token.as_ref(), "QFS_CLUSTER_TOKEN")
                .ok_or("no join token (pass --token or set QFS_CLUSTER_TOKEN)")?;
            let cfg = MemberConfig {
                url,
                token,
                name: env_or(name.as_ref(), "QFS_CLUSTER_NAME").unwrap_or_default(),
                heartbeat: Duration::from_secs(10),
            };
            let (ok, body) = runtime()?
                .block_on(qfs_cluster::run_once(&cfg, statement, *commit))
                .map_err(|e| e.to_string())?;
            let text = serde_json::to_string_pretty(&body).map_err(|e| e.to_string())?;
            if ok {
                println!("{text}");
                Ok(())
            } else {
                eprintln!("{text}");
                Err("the host did not run the statement".into())
            }
        }
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

    fn paths(stmt: &str) -> Result<Vec<String>, String> {
        touched_paths(stmt).map(|t| {
            t.paths
                .iter()
                .map(|p| format!("/{}", p.join("/")))
                .collect()
        })
    }

    #[test]
    fn borrowed_statements_list_every_path_they_touch() {
        assert_eq!(
            paths("/sql/demo/users |> limit 5").unwrap(),
            ["/sql/demo/users"]
        );
        let p = paths("INSERT INTO /slack-acct/qmu/c/messages VALUES (text) ('hi')").unwrap();
        assert_eq!(p, ["/slack-acct/qmu/c/messages"]);
        let mut p = paths("/sql/a/t |> join /mail/inbox on id == id").unwrap();
        p.sort();
        assert_eq!(p, ["/mail/inbox", "/sql/a/t"]);
        let mut p = paths("/mail/inbox |> CALL mail.send").unwrap();
        p.sort();
        assert_eq!(p, ["/mail", "/mail/inbox"]);
        let t = touched_paths("COMMIT INSERT INTO /local/x VALUES (a) (1)").unwrap();
        assert!(t.forces_commit);
        assert!(!touched_paths("/local/x").unwrap().forces_commit);
    }

    #[test]
    fn unprovable_statements_fail_closed() {
        assert!(touched_paths("this is not pipe sql").is_err());
        assert!(touched_paths("CREATE VIEW v AS /sql/a/t").is_err());
        assert!(touched_paths("nobody_bound_me |> limit 1").is_err());
    }

    #[test]
    fn account_verbs_write_the_pool_under_the_config_home() {
        let _home = HomeGuard::new();
        let add = ClusterRequest::AccountAdd {
            provider: "claude-code".into(),
            label: "work".into(),
            email: Some("a@example.com".into()),
            plan: Some("max".into()),
            state_dir: None,
        };
        assert_eq!(run_cluster(&add), 0);
        assert_eq!(run_cluster(&add), 1, "a duplicate label is refused");
        let assign = ClusterRequest::AccountAssign {
            label: "work".into(),
            member: Some("m1".into()),
            state_dir: None,
        };
        assert_eq!(run_cluster(&assign), 0);
        let pool = Accounts::load(&state_dir(None).unwrap()).unwrap();
        assert_eq!(pool.accounts.len(), 1);
        assert_eq!(pool.accounts[0].assigned_member.as_deref(), Some("m1"));
        let text = std::fs::read_to_string(
            state_dir(None)
                .unwrap()
                .join(qfs_cluster::accounts::ACCOUNTS_FILE),
        )
        .unwrap();
        assert!(!text.contains("token") && !text.contains("secret"));
        let rm = ClusterRequest::AccountRemove {
            label: "work".into(),
            state_dir: None,
        };
        assert_eq!(run_cluster(&rm), 0);
        assert_eq!(run_cluster(&rm), 1);
    }

    #[test]
    fn hex_round_trips() {
        assert_eq!(
            decode_hex(&encode_hex(&[0, 255, 16])),
            Some(vec![0, 255, 16])
        );
    }
}
