//! Borrowed execution end to end with a fake engine: granted works, ungranted is refused before
//! execution, an unauthenticated request is refused, and no frame carries the host's credential.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::{SinkExt as _, StreamExt as _};
use qfs_cluster::grants::segments;
use qfs_cluster::{
    mint, now_secs, run_once, serve_with, Borrow, Frame, Grants, MemberConfig, MemberError,
    Registry, Touched,
};
use serde_json::json;
use tokio_tungstenite::tungstenite::Message;

const SECRET: &[u8] = b"test-cluster-secret-32-bytes-xxx";
/// The credential the host's fake "secret store" holds for the `/chat` mount.
const STORED_CREDENTIAL: &str = "xoxb-HOST-ONLY-CREDENTIAL-7f3a9c";

struct Host {
    addr: String,
    dir: PathBuf,
    calls: Arc<AtomicUsize>,
    registry: Registry,
    /// Every statement the fake engine ran, with its commit flag.
    ran: Arc<Mutex<Vec<(String, bool)>>>,
}

/// A fake statement analysis: the touched paths are the whitespace tokens starting with `/`; a
/// statement with none, or containing `TRANSFORM`, cannot be proven and is refused.
fn fake_paths_of(stmt: &str) -> Result<Touched, String> {
    if stmt.contains("TRANSFORM") {
        return Err("cannot determine what this statement touches".into());
    }
    Ok(Touched {
        paths: stmt
            .split_whitespace()
            .filter(|t| t.starts_with('/'))
            .map(segments)
            .collect(),
        forces_commit: false,
    })
}

async fn start_host(tag: &str) -> Host {
    let dir = std::env::temp_dir().join(format!("qfs-borrow-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let ran = Arc::new(Mutex::new(Vec::new()));
    // The fake engine owns the credential, the way a real driver reads it from the vault.
    let secret_store = STORED_CREDENTIAL.to_string();
    let (c, r) = (Arc::clone(&calls), Arc::clone(&ran));
    let execute: qfs_cluster::Execute = Arc::new(move |stmt: &str, commit: bool| {
        c.fetch_add(1, Ordering::SeqCst);
        r.lock().unwrap().push((stmt.to_string(), commit));
        assert!(
            secret_store.starts_with("xoxb-"),
            "the engine authenticates"
        );
        if stmt.contains("boom") {
            return Err(json!({"error": {"kind": "driver", "message": "upstream said no"}}));
        }
        Ok(json!({"rows": [{"text": "hello", "committed": commit}]}))
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    let registry = Registry::new();
    let borrow = Borrow {
        state_dir: dir.clone(),
        paths_of: Arc::new(fake_paths_of),
        execute,
    };
    tokio::spawn(serve_with(
        listener,
        Arc::new(SECRET.to_vec()),
        registry.clone(),
        Some(borrow),
    ));
    Host {
        addr,
        dir,
        calls,
        registry,
        ran,
    }
}

fn cfg(addr: &str, name: &str) -> MemberConfig {
    MemberConfig {
        url: format!("ws://{addr}"),
        token: mint(SECRET, name, 60, now_secs()),
        name: name.into(),
        heartbeat: Duration::from_secs(10),
    }
}

fn grant(dir: &std::path::Path, member: &str, mount: &str) {
    let mut g = Grants::load(dir).unwrap();
    g.grant(member, mount).unwrap();
    g.save(dir).unwrap();
}

fn audit_lines(dir: &std::path::Path) -> Vec<serde_json::Value> {
    std::fs::read_to_string(dir.join(qfs_cluster::borrow::AUDIT_FILE))
        .unwrap_or_default()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_granted_statement_runs_on_the_host_and_is_audited() {
    let h = start_host("granted").await;
    grant(&h.dir, "alice", "/chat");
    let stmt = "INSERT INTO /chat/general/messages VALUES (text) ('hi')";
    let (ok, body) = run_once(&cfg(&h.addr, "alice"), stmt, true).await.unwrap();
    assert!(ok, "{body}");
    assert_eq!(body["rows"][0]["committed"], true);
    assert_eq!(
        h.ran.lock().unwrap().as_slice(),
        &[(stmt.to_string(), true)]
    );
    // A one-shot connection is never registered as a member.
    assert!(h.registry.members().is_empty());
    let audit = audit_lines(&h.dir);
    assert_eq!(audit.len(), 1);
    assert_eq!(audit[0]["member"], "alice");
    assert_eq!(audit[0]["statement"], stmt);
    assert_eq!(audit[0]["commit"], true);
    assert_eq!(audit[0]["outcome"], "ok");

    // An engine error comes back as ok=false with the engine's envelope, and is audited.
    let (ok, body) = run_once(&cfg(&h.addr, "alice"), "/chat/boom", false)
        .await
        .unwrap();
    assert!(!ok);
    assert_eq!(body["error"]["kind"], "driver");
    assert_eq!(audit_lines(&h.dir)[1]["outcome"], "error");
}

#[tokio::test(flavor = "multi_thread")]
async fn an_ungranted_statement_is_refused_before_execution() {
    let h = start_host("ungranted").await;
    grant(&h.dir, "alice", "/chat");
    grant(&h.dir, "bob", "/mail");
    for stmt in [
        // A mount granted to someone else.
        "/mail/inbox",
        // One granted and one ungranted path in the same statement.
        "INSERT INTO /chat/x SELECT FROM /mail/inbox",
        // A sibling mount whose name merely starts with the granted one.
        "/chatx/y",
        // A statement whose reach cannot be determined (fail closed).
        "TRANSFORM triage",
        // No path at all.
        "VALUES (1)",
    ] {
        let (ok, body) = run_once(&cfg(&h.addr, "alice"), stmt, true).await.unwrap();
        assert!(!ok, "{stmt} must be refused");
        assert_eq!(body["error"]["kind"], "refused", "{stmt}: {body}");
    }
    assert_eq!(
        h.calls.load(Ordering::SeqCst),
        0,
        "nothing reached the engine"
    );
    let audit = audit_lines(&h.dir);
    assert_eq!(audit.len(), 5);
    assert!(audit.iter().all(|l| l["outcome"] == "refused"));

    // Revoking takes effect on the next request without restarting the host.
    let mut g = Grants::load(&h.dir).unwrap();
    g.revoke("alice", "/chat");
    g.save(&h.dir).unwrap();
    let (ok, _) = run_once(&cfg(&h.addr, "alice"), "/chat/general", false)
        .await
        .unwrap();
    assert!(!ok);
    assert_eq!(h.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn an_unauthenticated_request_is_refused() {
    let h = start_host("unauth").await;
    grant(&h.dir, "alice", "/chat");

    // A request with no hello at all.
    let (ws, _) = tokio_tungstenite::connect_async(format!("ws://{}/cluster/ws", h.addr))
        .await
        .unwrap();
    let (mut tx, mut rx) = ws.split();
    let req = Frame::Request {
        id: 1,
        statement: "/chat/general".into(),
        commit: true,
    };
    tx.send(Message::text(req.to_json())).await.unwrap();
    let Some(Ok(Message::Text(t))) = rx.next().await else {
        panic!("expected a refusal");
    };
    assert!(matches!(
        Frame::from_json(t.as_str()),
        Ok(Frame::Refused { .. })
    ));

    // A forged token.
    let mut bad = cfg(&h.addr, "alice");
    bad.token = mint(b"some-other-secret-32-bytes-xxxxx", "alice", 60, now_secs());
    let err = run_once(&bad, "/chat/general", true).await.unwrap_err();
    assert!(matches!(err, MemberError::Refused(_)), "{err}");

    // A token for alice presented as bob.
    let mut other = cfg(&h.addr, "alice");
    other.name = "bob".into();
    assert!(run_once(&other, "/chat/general", true).await.is_err());

    assert_eq!(h.calls.load(Ordering::SeqCst), 0);
    assert!(audit_lines(&h.dir).is_empty());
}

/// Record every frame a member sends and receives over the wire, for a granted, a refused, and a
/// failed request, and prove the host's stored credential appears in none of them.
#[tokio::test(flavor = "multi_thread")]
async fn no_frame_carries_the_host_credential() {
    let h = start_host("nosecret").await;
    grant(&h.dir, "alice", "/chat");
    let (ws, _) = tokio_tungstenite::connect_async(format!("ws://{}/cluster/ws", h.addr))
        .await
        .unwrap();
    let (mut tx, mut rx) = ws.split();
    let mut wire: Vec<String> = Vec::new();
    let hello = Frame::Hello {
        token: mint(SECRET, "alice", 60, now_secs()),
        name: "alice".into(),
        heartbeat_ms: 10_000,
        exec_only: true,
    }
    .to_json();
    wire.push(hello.clone());
    tx.send(Message::text(hello)).await.unwrap();
    let Some(Ok(Message::Text(welcome))) = rx.next().await else {
        panic!("expected a welcome");
    };
    wire.push(welcome.to_string());
    for (id, stmt) in [
        (1, "/chat/general/messages"),
        (2, "/mail/inbox"),
        (3, "/chat/boom"),
    ] {
        let req = Frame::Request {
            id,
            statement: stmt.into(),
            commit: true,
        }
        .to_json();
        wire.push(req.clone());
        tx.send(Message::text(req)).await.unwrap();
        let Some(Ok(Message::Text(t))) = rx.next().await else {
            panic!("expected a response");
        };
        let t = t.to_string();
        assert!(matches!(Frame::from_json(&t), Ok(Frame::Response { id: i, .. }) if i == id));
        wire.push(t);
    }
    assert_eq!(h.calls.load(Ordering::SeqCst), 2);
    for frame in &wire {
        assert!(
            !frame.contains(STORED_CREDENTIAL) && !frame.contains("xoxb-"),
            "a frame leaked the host credential: {frame}"
        );
    }
    // Nor does the host's audit log.
    let audit = std::fs::read_to_string(h.dir.join(qfs_cluster::borrow::AUDIT_FILE)).unwrap();
    assert!(!audit.contains(STORED_CREDENTIAL));
}
