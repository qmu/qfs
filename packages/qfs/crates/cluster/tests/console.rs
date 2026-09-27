//! The host listener serves the embedded cluster console at `GET /` and the account pool at
//! `GET /api/cluster/accounts` (re-read per request), beside members and sessions.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;

use qfs_cluster::{fetch_json, serve_with, Accounts, Borrow, Registry, Touched};

const SECRET: &[u8] = b"test-cluster-secret-32-bytes-xxx";

#[tokio::test(flavor = "multi_thread")]
async fn the_console_and_the_account_pool_are_served() {
    let dir = std::env::temp_dir().join(format!("qfs-console-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let borrow = Borrow {
        state_dir: dir.clone(),
        paths_of: Arc::new(|_: &str| {
            Ok(Touched {
                paths: vec![],
                forces_commit: false,
            })
        }),
        execute: Arc::new(|_: &str, _: bool| Ok(serde_json::json!({}))),
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    tokio::spawn(serve_with(
        listener,
        Arc::new(SECRET.to_vec()),
        Registry::new(),
        Some(borrow),
    ));

    let a = addr.clone();
    let get = move |p: &'static str| {
        let a = a.clone();
        async move {
            tokio::task::spawn_blocking(move || fetch_json(&a, p))
                .await
                .unwrap()
        }
    };

    let page = get("/").await.unwrap();
    assert_eq!(page.as_bytes(), qfs_cluster::host::CONSOLE_HTML);
    assert!(page.contains("<html") || page.contains("<!doctype") || page.contains("<!DOCTYPE"));
    assert!(get("/nope").await.is_err());

    assert_eq!(get("/api/cluster/accounts").await.unwrap(), "[]");
    let mut pool = Accounts::default();
    pool.add("claude-code", "work", Some("a@example.com"), Some("max"))
        .unwrap();
    pool.assign("work", Some("m1")).unwrap();
    pool.save(&dir).unwrap();
    let rows: serde_json::Value =
        serde_json::from_str(&get("/api/cluster/accounts").await.unwrap()).unwrap();
    assert_eq!(rows[0]["label"], "work");
    assert_eq!(rows[0]["provider"], "claude-code");
    assert_eq!(rows[0]["status"], "assigned");
    assert_eq!(rows[0]["assigned_member"], "m1");
    assert_eq!(get("/api/cluster/members").await.unwrap(), "[]");
    let _ = std::fs::remove_dir_all(&dir);
}
