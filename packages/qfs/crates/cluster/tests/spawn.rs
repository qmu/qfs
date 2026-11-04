//! In-process host + members with a fake `claude` executable: opt-out refuses, opt-in runs and
//! answers, the timeout kills, `all` fans out, and the loopback write API dispatches.
#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use qfs_cluster::member::{session_once, ReportSource};
use qfs_cluster::{
    fetch_json, mint, now_secs, post_json, serve, MemberConfig, Registry, Report, SpawnConfig,
    SpawnRequest,
};

const SECRET: &[u8] = b"test-cluster-secret-32-bytes-xxx";

/// A fake `claude`: `-p <prompt>` echoes `answer: <prompt>`; a prompt `sleep` hangs.
fn fake_claude() -> PathBuf {
    use std::os::unix::fs::PermissionsExt as _;
    let dir = std::env::temp_dir().join(format!(
        "qfs-fake-claude-{}-{}",
        std::process::id(),
        rand_suffix()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let bin = dir.join("claude");
    std::fs::write(
        &bin,
        "#!/bin/sh\n[ \"$1\" = \"-p\" ] || exit 2\nif [ \"$2\" = sleep ]; then sleep 30; fi\necho \"answer: $2 tools: $4 cwd-ok: $(test -w . && echo y)\"\n",
    )
    .unwrap();
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
    bin
}

fn rand_suffix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64
}

async fn start_host() -> (String, Registry) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    let reg = Registry::new();
    tokio::spawn(serve(listener, Arc::new(SECRET.to_vec()), reg.clone()));
    (addr, reg)
}

fn join(addr: &str, name: &str, spawn: Option<SpawnConfig>) {
    let cfg = MemberConfig {
        url: format!("ws://{addr}"),
        token: mint(SECRET, name, 60, now_secs()),
        name: name.into(),
        heartbeat: Duration::from_millis(50),
        spawn,
    };
    let source: ReportSource = Arc::new(Report::default);
    tokio::spawn(async move { session_once(&cfg, &source, &|_| {}).await });
}

async fn wait_for(mut cond: impl FnMut() -> bool) {
    for _ in 0..500 {
        if cond() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("condition not reached");
}

fn req(member: Option<&str>, prompt: &str) -> SpawnRequest {
    SpawnRequest {
        member: member.map(str::to_string),
        all: member.is_none(),
        prompt: prompt.into(),
        ..SpawnRequest::default()
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn opt_in_answers_opt_out_refuses_and_all_fans_out() {
    let (addr, reg) = start_host().await;
    join(&addr, "yes", Some(SpawnConfig::new(fake_claude(), None)));
    join(&addr, "no", None);
    wait_for(|| reg.members().len() == 2).await;

    let runs = reg.agents().spawn(&req(None, "tell a joke")).unwrap();
    assert_eq!(runs.len(), 2);
    let a = reg.agents().clone();
    wait_for(|| a.runs().iter().all(|r| r.status != "running")).await;
    let rows = reg.agents().runs();
    let yes = rows.iter().find(|r| r.member == "yes").unwrap();
    assert_eq!(yes.status, "done");
    assert!(
        yes.output
            .starts_with("answer: tell a joke tools:  cwd-ok: y"),
        "{}",
        yes.output
    );
    assert_eq!(yes.exit_code, Some(0));
    let no = rows.iter().find(|r| r.member == "no").unwrap();
    assert_eq!(no.status, "failed");
    assert_eq!(no.output, "spawn not accepted by this member");

    // A tool allowlist reaches the command line only when given.
    let mut r = req(Some("yes"), "with tools");
    r.allowed_tools = vec!["Read".into()];
    let id = reg.agents().spawn(&r).unwrap()[0].id;
    wait_for(|| a.get(id).is_some_and(|r| r.status != "running")).await;
    assert!(a.get(id).unwrap().output.contains("tools: Read"));

    // Heartbeats kept flowing: both members are still online.
    assert!(reg.members().iter().all(|m| m.status == "online"));
}

#[tokio::test(flavor = "multi_thread")]
async fn the_timeout_kills_the_agent() {
    let (addr, reg) = start_host().await;
    let mut cfg = SpawnConfig::new(fake_claude(), None);
    cfg.timeout = Duration::from_millis(300);
    join(&addr, "slow", Some(cfg));
    wait_for(|| reg.members().len() == 1).await;
    let id = reg.agents().spawn(&req(Some("slow"), "sleep")).unwrap()[0].id;
    let a = reg.agents().clone();
    wait_for(|| a.get(id).is_some_and(|r| r.status != "running")).await;
    let r = a.get(id).unwrap();
    assert_eq!(r.status, "failed");
    assert!(r.output.contains("timed out"), "{}", r.output);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_loopback_api_spawns_and_lists_runs() {
    let (addr, reg) = start_host().await;
    join(&addr, "yes", Some(SpawnConfig::new(fake_claude(), None)));
    wait_for(|| reg.members().len() == 1).await;
    let a2 = addr.clone();
    let body = tokio::task::spawn_blocking(move || {
        post_json(&a2, "/api/cluster/agents", r#"{"all":true,"prompt":"hi"}"#)
    })
    .await
    .unwrap()
    .unwrap();
    let runs: serde_json::Value = serde_json::from_str(&body).unwrap();
    let id = runs[0]["id"].as_u64().unwrap();
    let a = reg.agents().clone();
    wait_for(|| a.get(id).is_some_and(|r| r.status == "done")).await;
    let a2 = addr.clone();
    let one = tokio::task::spawn_blocking(move || {
        fetch_json(&a2, &format!("/api/cluster/agents?id={id}"))
    })
    .await
    .unwrap()
    .unwrap();
    let one: serde_json::Value = serde_json::from_str(&one).unwrap();
    assert_eq!(one["member"], "yes");
    assert!(one["output"].as_str().unwrap().starts_with("answer: hi"));
    // A bad request is a 400, never a run.
    let a2 = addr.clone();
    let bad = tokio::task::spawn_blocking(move || {
        post_json(&a2, "/api/cluster/agents", r#"{"prompt":"no target"}"#)
    })
    .await
    .unwrap();
    assert!(bad.is_err());
    assert_eq!(reg.agents().runs().len(), 1);
}
