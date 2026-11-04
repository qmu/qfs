//! The host listener: one TCP port serving both the member WebSocket (`/cluster/ws`) and a small
//! loopback-only JSON API (`GET /api/cluster/{members,sessions,accounts,agents}`, and the one write,
//! `POST /api/cluster/agents`, which spawns an agent on members) plus the cluster console GUI
//! (`GET /`, a single self-contained page embedded from `ui/index.html`).
//!
//! The write is guarded beyond loopback: it must carry `Content-Type: application/json` (so a
//! cross-site page cannot send it without a CORS preflight this listener never grants), a
//! loopback `Host` (defeats DNS rebinding), and no foreign `Origin`.
//!
//! Each connection's request head is *peeked* (not consumed) to route it: a WebSocket upgrade on
//! `/cluster/ws` goes to the member session; anything else is answered as plain HTTP by
//! [`http_route`], which is the single place to add routes.

use std::io::{Read as _, Write as _};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use futures::{SinkExt as _, StreamExt as _};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::tungstenite::Message;

use crate::accounts::Accounts;
use crate::borrow::Borrow;
use crate::frame::{redacted, Frame};
use crate::registry::Registry;
use crate::token;

/// The default host listen address: loopback only. Exposing needs an explicit `--listen`.
pub const DEFAULT_LISTEN: &str = "127.0.0.1:7466";

/// The WebSocket path members connect to.
pub const WS_PATH: &str = "/cluster/ws";

/// The cluster console GUI: one self-contained HTML page (inlined JS/CSS) built from
/// `packages/qfs-viewer/packages/cluster-console` and committed under `ui/`, so `cargo build`
/// needs no node toolchain.
pub const CONSOLE_HTML: &[u8] = include_bytes!("../ui/index.html");

const MAX_HEAD: usize = 8 * 1024;
const MAX_BODY: usize = 64 * 1024;
const HELLO_TIMEOUT: Duration = Duration::from_secs(10);

/// Run the accept loop forever on `listener`, with borrowed execution disabled (every
/// [`Frame::Request`] is refused).
///
/// # Errors
/// Only an `accept` failure that is not per-connection.
pub async fn serve(
    listener: TcpListener,
    secret: Arc<Vec<u8>>,
    registry: Registry,
) -> std::io::Result<()> {
    serve_with(listener, secret, registry, None).await
}

/// [`serve`], with borrowed execution wired to `borrow` (grant-checked; see [`crate::borrow`]).
///
/// # Errors
/// Only an `accept` failure that is not per-connection.
pub async fn serve_with(
    listener: TcpListener,
    secret: Arc<Vec<u8>>,
    registry: Registry,
    borrow: Option<Borrow>,
) -> std::io::Result<()> {
    let state_dir: Option<Arc<PathBuf>> = borrow.as_ref().map(|b| Arc::new(b.state_dir.clone()));
    if let Some(d) = &state_dir {
        registry.agents().set_audit_dir(d.as_ref().clone());
    }
    let borrow = borrow.map(Arc::new);
    loop {
        let (stream, peer) = listener.accept().await?;
        let secret = Arc::clone(&secret);
        let registry = registry.clone();
        let borrow = borrow.clone();
        let state_dir = state_dir.clone();
        tokio::spawn(async move {
            if let Err(e) = handle(stream, peer, secret, registry, borrow, state_dir).await {
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
    borrow: Option<Arc<Borrow>>,
    state_dir: Option<Arc<PathBuf>>,
) -> std::io::Result<()> {
    let head = peek_head(&stream).await?;
    let mut first = head.lines().next().unwrap_or("").split_whitespace();
    let method = first.next().unwrap_or("").to_string();
    let path = first.next().unwrap_or("").to_string();
    if path == WS_PATH && is_upgrade(&head) {
        return member_session(stream, peer, &secret, &registry, borrow).await;
    }
    // Plain HTTP: consume the head we peeked, then answer and close.
    let mut sink = vec![0u8; head.len()];
    stream.read_exact(&mut sink).await?;
    let (status, ctype, body) = if !peer.ip().to_canonical().is_loopback() {
        (
            403,
            "text/plain",
            b"the cluster read API is served to loopback only\n".to_vec(),
        )
    } else if method == "GET" {
        http_route(&path, &registry, state_dir.as_deref().map(PathBuf::as_path))
    } else if method == "POST" {
        match read_body(&mut stream, &head).await {
            Ok(body) => match write_guard(&head) {
                Ok(()) => http_post(&path, &body, &registry),
                Err(why) => (403, "text/plain", format!("{why}\n").into_bytes()),
            },
            Err(why) => (400, "text/plain", format!("{why}\n").into_bytes()),
        }
    } else {
        (405, "text/plain", b"method not allowed\n".to_vec())
    };
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        500 => "Internal Server Error",
        _ => "Method Not Allowed",
    };
    let head = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes()).await?;
    stream.write_all(&body).await?;
    stream.shutdown().await
}

fn header<'h>(head: &'h str, name: &str) -> Option<&'h str> {
    head.lines().skip(1).find_map(|l| {
        let (k, v) = l.split_once(':')?;
        k.trim().eq_ignore_ascii_case(name).then(|| v.trim())
    })
}

async fn read_body(stream: &mut TcpStream, head: &str) -> Result<Vec<u8>, String> {
    let len: usize = header(head, "content-length")
        .ok_or("a Content-Length is required")?
        .parse()
        .map_err(|_| "a malformed Content-Length")?;
    if len > MAX_BODY {
        return Err(format!("the body exceeds {MAX_BODY} bytes"));
    }
    let mut body = vec![0u8; len];
    tokio::time::timeout(HELLO_TIMEOUT, stream.read_exact(&mut body))
        .await
        .map_err(|_| "body timeout".to_string())?
        .map_err(|e| e.to_string())?;
    Ok(body)
}

/// The host part of a `Host`/`Origin` authority (`[::1]:7466` → `[::1]`).
fn host_part(authority: &str) -> &str {
    if authority.starts_with('[') {
        authority.split_inclusive(']').next().unwrap_or(authority)
    } else {
        authority.split(':').next().unwrap_or(authority)
    }
}

/// The extra guard on the one write route (see the module docs).
fn write_guard(head: &str) -> Result<(), &'static str> {
    let ctype = header(head, "content-type").unwrap_or("");
    if !ctype.to_ascii_lowercase().starts_with("application/json") {
        return Err("a write must be Content-Type: application/json");
    }
    let host = header(head, "host").ok_or("a write needs a Host header")?;
    if !matches!(host_part(host), "127.0.0.1" | "localhost" | "[::1]") {
        return Err("a write must address the host by a loopback name");
    }
    match header(head, "origin") {
        None => Ok(()),
        Some(o) if o.strip_prefix("http://") == Some(host) => Ok(()),
        Some(_) => Err("a cross-origin write is refused"),
    }
}

/// Route a loopback POST (already past [`write_guard`]). Returns `(status, content-type, body)`.
#[must_use]
pub fn http_post(path: &str, body: &[u8], registry: &Registry) -> (u16, &'static str, Vec<u8>) {
    match path.split('?').next().unwrap_or("") {
        "/api/cluster/agents" => {
            let req: crate::agents::SpawnRequest = match serde_json::from_slice(body) {
                Ok(r) => r,
                Err(e) => {
                    return (
                        400,
                        "text/plain",
                        format!("bad spawn request: {e}\n").into_bytes(),
                    )
                }
            };
            match registry.agents().spawn(&req) {
                Ok(runs) => (
                    200,
                    "application/json",
                    serde_json::to_vec(&runs).unwrap_or_default(),
                ),
                Err(e) => (400, "text/plain", format!("{e}\n").into_bytes()),
            }
        }
        _ => (404, "text/plain", b"not found\n".to_vec()),
    }
}

/// Route a loopback GET. Returns `(status, content-type, body)`. `state_dir` is the host's
/// cluster state dir (the account pool lives there); `None` serves an empty account pool.
#[must_use]
pub fn http_route(
    path: &str,
    registry: &Registry,
    state_dir: Option<&Path>,
) -> (u16, &'static str, Vec<u8>) {
    let (path, query) = path.split_once('?').unwrap_or((path, ""));
    let json = |v: Result<String, serde_json::Error>| {
        (
            200,
            "application/json",
            v.unwrap_or_else(|_| "[]".into()).into_bytes(),
        )
    };
    match path {
        "/" | "/index.html" => (200, "text/html; charset=utf-8", CONSOLE_HTML.to_vec()),
        "/api/cluster/members" => json(serde_json::to_string(&registry.members())),
        "/api/cluster/sessions" => json(serde_json::to_string(&registry.sessions())),
        "/api/cluster/agents" => {
            let id = query
                .split('&')
                .find_map(|kv| kv.strip_prefix("id="))
                .map(str::parse::<u64>);
            match id {
                None => json(serde_json::to_string(&registry.agents().runs())),
                Some(Ok(id)) => match registry.agents().get(id) {
                    Some(r) => json(serde_json::to_string(&r)),
                    None => (404, "text/plain", b"no such run\n".to_vec()),
                },
                Some(Err(_)) => (400, "text/plain", b"a malformed run id\n".to_vec()),
            }
        }
        "/api/cluster/accounts" => match state_dir.map(Accounts::load).transpose() {
            Ok(a) => json(serde_json::to_string(&a.unwrap_or_default().accounts)),
            Err(e) => (500, "text/plain", format!("{e}\n").into_bytes()),
        },
        _ => (404, "text/plain", b"not found\n".to_vec()),
    }
}

async fn member_session(
    stream: TcpStream,
    peer: SocketAddr,
    secret: &[u8],
    registry: &Registry,
    borrow: Option<Arc<Borrow>>,
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
    let (token, name, heartbeat_ms, exec_only) = match Frame::from_json(text.as_str()) {
        Ok(Frame::Hello {
            token,
            name,
            heartbeat_ms,
            exec_only,
        }) => (token, name, heartbeat_ms, exec_only),
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
    if !exec_only {
        registry.admit(&member, &peer.to_string(), heartbeat);
        eprintln!("qfs cluster: member `{member}` joined from {peer}");
    }
    tx.send(Message::text(
        Frame::Welcome {
            member: member.clone(),
        }
        .to_json(),
    ))
    .await
    .map_err(|e| std::io::Error::other(e.to_string()))?;

    // A joined member gets a link the agent table dispatches spawns over.
    let (link_tx, mut link_rx) = tokio::sync::mpsc::unbounded_channel::<Frame>();
    let link = (!exec_only).then(|| registry.agents().link(&member, link_tx));

    loop {
        let msg = tokio::select! {
            out = link_rx.recv() => {
                if let Some(frame) = out {
                    if tx.send(Message::text(frame.to_json())).await.is_err() {
                        break;
                    }
                }
                continue;
            }
            msg = rx.next() => msg,
        };
        let Some(Ok(msg)) = msg else { break };
        match msg {
            Message::Text(text) => match Frame::from_json(text.as_str()) {
                Ok(Frame::Heartbeat { report }) if !exec_only => {
                    registry.heartbeat(&member, report);
                }
                Ok(Frame::Request {
                    id,
                    statement,
                    commit,
                }) => {
                    let (ok, body) = match &borrow {
                        None => (
                            false,
                            serde_json::json!({ "error": { "kind": "refused",
                                "message": "borrowed execution is not enabled on this host" } }),
                        ),
                        Some(b) => {
                            let b = Arc::clone(b);
                            let m = member.clone();
                            tokio::task::spawn_blocking(move || {
                                crate::borrow::handle(&b, &m, &statement, commit)
                            })
                            .await
                            .unwrap_or_else(|e| {
                                (
                                    false,
                                    serde_json::json!({ "error": { "kind": "internal",
                                        "message": format!("the execution task failed ({e})") } }),
                                )
                            })
                        }
                    };
                    let _ = tx
                        .send(Message::text(Frame::Response { id, ok, body }.to_json()))
                        .await;
                }
                Ok(Frame::SpawnResult {
                    id,
                    ok,
                    output,
                    exit_code,
                }) if !exec_only => {
                    registry
                        .agents()
                        .finish(&member, id, ok, &output, exit_code);
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
    if let Some(link) = link {
        registry.agents().unlink(&member, link);
    }
    if !exec_only {
        eprintln!("qfs cluster: member `{member}` disconnected (lapses to offline)");
    }
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

/// Blocking loopback `POST` of a JSON `body` to `path` on the host at `addr`; returns the body on
/// 200.
///
/// # Errors
/// A message naming the connect/read failure or the host's non-200 answer.
pub fn post_json(addr: &str, path: &str, body: &str) -> Result<String, String> {
    let mut s = std::net::TcpStream::connect(addr).map_err(|e| {
        format!("cannot reach the cluster host at {addr} ({e}); is `qfs cluster host` running?")
    })?;
    let _ = s.set_read_timeout(Some(Duration::from_secs(10)));
    write!(
        s,
        "POST {path} HTTP/1.1\r\nHost: {addr}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
    .map_err(|e| e.to_string())?;
    let mut out = String::new();
    s.read_to_string(&mut out).map_err(|e| e.to_string())?;
    let (head, rest) = out.split_once("\r\n\r\n").ok_or("malformed response")?;
    if !head.starts_with("HTTP/1.1 200") {
        return Err(format!(
            "the host answered: {} {}",
            head.lines().next().unwrap_or(""),
            rest.trim()
        ));
    }
    Ok(rest.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn head(extra: &str) -> String {
        format!("POST /api/cluster/agents HTTP/1.1\r\n{extra}\r\n\r\n")
    }

    #[test]
    fn the_write_guard_needs_json_a_loopback_host_and_no_foreign_origin() {
        let ok = "Host: 127.0.0.1:7466\r\nContent-Type: application/json";
        assert!(write_guard(&head(ok)).is_ok());
        assert!(write_guard(&head("Host: [::1]:7466\r\nContent-Type: application/json")).is_ok());
        assert!(write_guard(&head("Host: 127.0.0.1:7466\r\nContent-Type: text/plain")).is_err());
        assert!(write_guard(&head(
            "Host: evil.example:7466\r\nContent-Type: application/json"
        ))
        .is_err());
        let same = format!("{ok}\r\nOrigin: http://127.0.0.1:7466");
        assert!(write_guard(&head(&same)).is_ok());
        let foreign = format!("{ok}\r\nOrigin: http://evil.example");
        assert!(write_guard(&head(&foreign)).is_err());
    }
}
