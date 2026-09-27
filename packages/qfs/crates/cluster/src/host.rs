//! The host listener: one TCP port serving both the member WebSocket (`/cluster/ws`) and a small
//! loopback-only JSON read API (`GET /api/cluster/members`, `GET /api/cluster/sessions`).
//!
//! Each connection's request head is *peeked* (not consumed) to route it: a WebSocket upgrade on
//! `/cluster/ws` goes to the member session; anything else is answered as plain HTTP by
//! [`http_route`], which is the single place to add routes (a GUI's `GET /` lands there).

use std::io::{Read as _, Write as _};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use futures::{SinkExt as _, StreamExt as _};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::tungstenite::Message;

use crate::frame::{redacted, Frame};
use crate::registry::Registry;
use crate::token;

/// The default host listen address: loopback only. Exposing needs an explicit `--listen`.
pub const DEFAULT_LISTEN: &str = "127.0.0.1:7466";

/// The WebSocket path members connect to.
pub const WS_PATH: &str = "/cluster/ws";

const MAX_HEAD: usize = 8 * 1024;
const HELLO_TIMEOUT: Duration = Duration::from_secs(10);

/// Run the accept loop forever on `listener`.
///
/// # Errors
/// Only an `accept` failure that is not per-connection.
pub async fn serve(
    listener: TcpListener,
    secret: Arc<Vec<u8>>,
    registry: Registry,
) -> std::io::Result<()> {
    loop {
        let (stream, peer) = listener.accept().await?;
        let secret = Arc::clone(&secret);
        let registry = registry.clone();
        tokio::spawn(async move {
            if let Err(e) = handle(stream, peer, secret, registry).await {
                eprintln!("qfs cluster: connection from {peer}: {e}");
            }
        });
    }
}

/// Peek until the end of the request head (or the cap), returning the head text.
async fn peek_head(stream: &TcpStream) -> std::io::Result<String> {
    let mut buf = vec![0u8; MAX_HEAD];
    let deadline = tokio::time::Instant::now() + HELLO_TIMEOUT;
    loop {
        let n = tokio::time::timeout_at(deadline, stream.peek(&mut buf))
            .await
            .map_err(|_| {
                std::io::Error::new(std::io::ErrorKind::TimedOut, "request head timeout")
            })??;
        if n == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "closed before a request",
            ));
        }
        let text = String::from_utf8_lossy(&buf[..n]);
        if let Some(end) = text.find("\r\n\r\n") {
            return Ok(text[..end + 4].to_string());
        }
        if n >= MAX_HEAD {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "request head too large",
            ));
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

fn is_upgrade(head: &str) -> bool {
    head.lines().any(|l| {
        let l = l.to_ascii_lowercase();
        l.starts_with("upgrade:") && l.contains("websocket")
    })
}

async fn handle(
    mut stream: TcpStream,
    peer: SocketAddr,
    secret: Arc<Vec<u8>>,
    registry: Registry,
) -> std::io::Result<()> {
    let head = peek_head(&stream).await?;
    let mut first = head.lines().next().unwrap_or("").split_whitespace();
    let method = first.next().unwrap_or("").to_string();
    let path = first.next().unwrap_or("").to_string();
    if path == WS_PATH && is_upgrade(&head) {
        return member_session(stream, peer, &secret, &registry).await;
    }
    // Plain HTTP: consume the head we peeked, then answer and close.
    let mut sink = vec![0u8; head.len()];
    stream.read_exact(&mut sink).await?;
    let (status, ctype, body) = if !peer.ip().to_canonical().is_loopback() {
        (
            403,
            "text/plain",
            "the cluster read API is served to loopback only\n".to_string(),
        )
    } else if method != "GET" {
        (405, "text/plain", "method not allowed\n".to_string())
    } else {
        http_route(&path, &registry)
    };
    let reason = match status {
        200 => "OK",
        403 => "Forbidden",
        404 => "Not Found",
        _ => "Method Not Allowed",
    };
    let resp = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(resp.as_bytes()).await?;
    stream.shutdown().await
}

/// Route a loopback GET. Returns `(status, content-type, body)`.
#[must_use]
pub fn http_route(path: &str, registry: &Registry) -> (u16, &'static str, String) {
    let path = path.split('?').next().unwrap_or("");
    match path {
        "/api/cluster/members" => (
            200,
            "application/json",
            serde_json::to_string(&registry.members()).unwrap_or_else(|_| "[]".into()),
        ),
        "/api/cluster/sessions" => (
            200,
            "application/json",
            serde_json::to_string(&registry.sessions()).unwrap_or_else(|_| "[]".into()),
        ),
        _ => (404, "text/plain", "not found\n".to_string()),
    }
}

async fn member_session(
    stream: TcpStream,
    peer: SocketAddr,
    secret: &[u8],
    registry: &Registry,
) -> std::io::Result<()> {
    let ws = tokio_tungstenite::accept_async(stream)
        .await
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    let (mut tx, mut rx) = ws.split();

    // The first frame must be a valid Hello, within the timeout. Nothing is registered before it.
    let hello = tokio::time::timeout(HELLO_TIMEOUT, rx.next()).await;
    let Ok(Some(Ok(Message::Text(text)))) = hello else {
        return Ok(());
    };
    let (token, name, heartbeat_ms) = match Frame::from_json(text.as_str()) {
        Ok(Frame::Hello {
            token,
            name,
            heartbeat_ms,
        }) => (token, name, heartbeat_ms),
        _ => {
            let _ = tx
                .send(Message::text(
                    Frame::Refused {
                        reason: "expected a hello frame".into(),
                    }
                    .to_json(),
                ))
                .await;
            return Ok(());
        }
    };
    let presented = if name.is_empty() {
        None
    } else {
        Some(name.as_str())
    };
    let claims = match token::verify(secret, &token, presented, crate::now_secs()) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("qfs cluster: refused a member from {peer}: {e}");
            let _ = tx
                .send(Message::text(
                    Frame::Refused {
                        reason: e.to_string(),
                    }
                    .to_json(),
                ))
                .await;
            let _ = tx.close().await;
            return Ok(());
        }
    };
    let member = claims.member;
    let heartbeat = Duration::from_millis(heartbeat_ms.clamp(10, 3_600_000));
    registry.admit(&member, &peer.to_string(), heartbeat);
    eprintln!("qfs cluster: member `{member}` joined from {peer}");
    tx.send(Message::text(
        Frame::Welcome {
            member: member.clone(),
        }
        .to_json(),
    ))
    .await
    .map_err(|e| std::io::Error::other(e.to_string()))?;

    while let Some(msg) = rx.next().await {
        let Ok(msg) = msg else { break };
        match msg {
            Message::Text(text) => match Frame::from_json(text.as_str()) {
                Ok(Frame::Heartbeat { report }) => registry.heartbeat(&member, report),
                Ok(Frame::Request { id, .. }) => {
                    let _ = tx
                        .send(Message::text(
                            Frame::Response {
                                id,
                                ok: false,
                                body: serde_json::Value::String(
                                    "borrowed execution is not enabled on this host yet".into(),
                                ),
                            }
                            .to_json(),
                        ))
                        .await;
                }
                Ok(other) => {
                    eprintln!("qfs cluster: ignoring {} from `{member}`", redacted(&other))
                }
                Err(_) => eprintln!("qfs cluster: ignoring a non-frame message from `{member}`"),
            },
            Message::Close(_) => break,
            _ => {}
        }
    }
    eprintln!("qfs cluster: member `{member}` disconnected (lapses to offline)");
    Ok(())
}

/// Blocking loopback GET of `path` on the host at `addr`; returns the body on 200.
///
/// # Errors
/// A message naming the connect/read failure or the non-200 status.
pub fn fetch_json(addr: &str, path: &str) -> Result<String, String> {
    let mut s = std::net::TcpStream::connect(addr).map_err(|e| {
        format!("cannot reach the cluster host at {addr} ({e}); is `qfs cluster host` running?")
    })?;
    let _ = s.set_read_timeout(Some(Duration::from_secs(10)));
    write!(
        s,
        "GET {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n"
    )
    .map_err(|e| e.to_string())?;
    let mut out = String::new();
    s.read_to_string(&mut out).map_err(|e| e.to_string())?;
    let (head, body) = out.split_once("\r\n\r\n").ok_or("malformed response")?;
    if !head.starts_with("HTTP/1.1 200") {
        return Err(format!(
            "the host answered: {}",
            head.lines().next().unwrap_or("")
        ));
    }
    Ok(body.to_string())
}
