//! In-process host + member: join, heartbeat aggregation, refusal, and offline lapse.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;
use std::time::Duration;

use qfs_cluster::member::{session_once, ReportSource};
use qfs_cluster::{
    fetch_json, mint, now_secs, serve, MemberConfig, MemberError, Registry, Report, SessionReport,
};

const SECRET: &[u8] = b"test-cluster-secret-32-bytes-xxx";

async fn start_host() -> (String, Registry) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    let reg = Registry::new();
    tokio::spawn(serve(listener, Arc::new(SECRET.to_vec()), reg.clone()));
    (addr, reg)
}

fn source() -> ReportSource {
    Arc::new(|| Report {
        hostname: "box-1".into(),
        cpu_pct: 12.5,
        mem_used: 1,
        mem_total: 16,
        disk_used: 2,
        disk_total: 512,
        sessions: vec![SessionReport {
            id: "s-1".into(),
            cwd: Some("/work".into()),
            status: Some("idle".into()),
            ..SessionReport::default()
        }],
    })
}

fn cfg(addr: &str, token: String, name: &str, ms: u64) -> MemberConfig {
    MemberConfig {
        url: format!("ws://{addr}"),
        token,
        name: name.into(),
        heartbeat: Duration::from_millis(ms),
    }
}

async fn wait_for(mut cond: impl FnMut() -> bool) {
    for _ in 0..200 {
        if cond() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("condition not reached");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_member_joins_reports_and_lapses_offline() {
    let (addr, reg) = start_host().await;
    let c = cfg(&addr, mint(SECRET, "alice", 60, now_secs()), "alice", 30);
    let member = tokio::spawn(async move { session_once(&c, &source(), &|_| {}).await });

    wait_for(|| reg.members().first().is_some_and(|m| m.mem_total == 16)).await;
    let m = &reg.members()[0];
    assert_eq!((m.name.as_str(), m.status.as_str()), ("alice", "online"));
    assert_eq!(m.disk_total, 512);
    assert_eq!(m.hostname.as_deref(), Some("box-1"));
    let s = reg.sessions();
    assert_eq!((s[0].member.as_str(), s[0].id.as_str()), ("alice", "s-1"));

    // The loopback JSON API answers the same rows.
    let a = addr.clone();
    let body = tokio::task::spawn_blocking(move || fetch_json(&a, "/api/cluster/members"))
        .await
        .unwrap()
        .unwrap();
    let rows: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(rows[0]["name"], "alice");
    assert_eq!(rows[0]["mem_total"], 16);
    let a = addr.clone();
    let body = tokio::task::spawn_blocking(move || fetch_json(&a, "/api/cluster/sessions"))
        .await
        .unwrap()
        .unwrap();
    assert!(body.contains("\"s-1\""));

    // Disconnect: after three missed heartbeats the member shows offline.
    member.abort();
    wait_for(|| reg.members()[0].status == "offline").await;
}

#[tokio::test(flavor = "multi_thread")]
async fn bad_tokens_are_refused_and_nothing_is_registered() {
    let (addr, reg) = start_host().await;
    let cases = [
        ("expired", mint(SECRET, "bob", 1, now_secs() - 10), "bob"),
        (
            "tampered",
            format!("{}x", mint(SECRET, "bob", 60, now_secs())),
            "bob",
        ),
        (
            "foreign",
            mint(b"some-other-secret-some-other-sec", "bob", 60, now_secs()),
            "bob",
        ),
        ("mismatch", mint(SECRET, "bob", 60, now_secs()), "mallory"),
    ];
    for (label, token, name) in cases {
        let c = cfg(&addr, token, name, 30);
        let r = session_once(&c, &source(), &|_| {}).await;
        assert!(matches!(r, Err(MemberError::Refused(_))), "{label}: {r:?}");
    }
    assert!(reg.members().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn unknown_http_routes_404() {
    let (addr, _reg) = start_host().await;
    let r = tokio::task::spawn_blocking(move || fetch_json(&addr, "/nope"))
        .await
        .unwrap();
    assert!(r.unwrap_err().contains("404"));
}
