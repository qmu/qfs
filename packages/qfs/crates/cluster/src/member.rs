//! The member side: connect, hello, heartbeat, reconnect with backoff.

use std::sync::Arc;
use std::time::Duration;

use futures::{SinkExt as _, StreamExt as _};
use tokio_tungstenite::tungstenite::Message;

use crate::frame::{Frame, Report};
use crate::host::WS_PATH;

/// What a member needs to join.
#[derive(Clone)]
pub struct MemberConfig {
    /// `ws://host:port` (the `/cluster/ws` path is appended when absent).
    pub url: String,
    /// The join token (never logged).
    pub token: String,
    /// The presented name; empty means "the name the token admits".
    pub name: String,
    /// Heartbeat interval.
    pub heartbeat: Duration,
}

impl std::fmt::Debug for MemberConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MemberConfig")
            .field("url", &self.url)
            .field("token", &"<redacted>")
            .field("name", &self.name)
            .field("heartbeat", &self.heartbeat)
            .finish()
    }
}

/// Why a member session ended.
#[derive(Debug)]
pub enum MemberError {
    /// The host refused the hello — fatal, never retried.
    Refused(String),
    /// A transport failure — retried with backoff.
    Transport(String),
}

impl std::fmt::Display for MemberError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Refused(r) => write!(f, "the host refused this member: {r}"),
            Self::Transport(r) => write!(f, "connection lost: {r}"),
        }
    }
}

impl std::error::Error for MemberError {}

/// The heartbeat payload source (resource sampling + Claude sessions), called off the reactor.
pub type ReportSource = Arc<dyn Fn() -> Report + Send + Sync>;

fn ws_url(url: &str) -> String {
    let u = url.trim_end_matches('/');
    if u.ends_with(WS_PATH) {
        u.to_string()
    } else {
        format!("{u}{WS_PATH}")
    }
}

async fn sample(source: &ReportSource) -> Report {
    let s = Arc::clone(source);
    tokio::task::spawn_blocking(move || s())
        .await
        .unwrap_or_default()
}

/// One connection: hello, then heartbeat until the socket drops. `on_welcome` fires once admitted.
///
/// # Errors
/// [`MemberError::Refused`] on a refused hello; [`MemberError::Transport`] otherwise.
pub async fn session_once(
    cfg: &MemberConfig,
    source: &ReportSource,
    on_welcome: &(dyn Fn(&str) + Send + Sync),
) -> Result<(), MemberError> {
    let (ws, _) = tokio_tungstenite::connect_async(ws_url(&cfg.url))
        .await
        .map_err(|e| MemberError::Transport(e.to_string()))?;
    let (mut tx, mut rx) = ws.split();
    let hello = Frame::Hello {
        token: cfg.token.clone(),
        name: cfg.name.clone(),
        heartbeat_ms: u64::try_from(cfg.heartbeat.as_millis()).unwrap_or(u64::MAX),
    };
    tx.send(Message::text(hello.to_json()))
        .await
        .map_err(|e| MemberError::Transport(e.to_string()))?;
    match rx.next().await {
        Some(Ok(Message::Text(t))) => match Frame::from_json(t.as_str()) {
            Ok(Frame::Welcome { member }) => on_welcome(&member),
            Ok(Frame::Refused { reason }) => return Err(MemberError::Refused(reason)),
            _ => return Err(MemberError::Transport("unexpected reply to hello".into())),
        },
        Some(Ok(Message::Close(_))) | None => {
            return Err(MemberError::Refused(
                "the host closed the connection".into(),
            ))
        }
        Some(Ok(_)) => return Err(MemberError::Transport("unexpected reply to hello".into())),
        Some(Err(e)) => return Err(MemberError::Transport(e.to_string())),
    }

    let mut tick = tokio::time::interval(cfg.heartbeat);
    loop {
        tokio::select! {
            _ = tick.tick() => {
                let report = sample(source).await;
                tx.send(Message::text(Frame::Heartbeat { report }.to_json()))
                    .await
                    .map_err(|e| MemberError::Transport(e.to_string()))?;
            }
            msg = rx.next() => match msg {
                Some(Ok(Message::Text(t))) => {
                    if let Ok(Frame::Request { id, .. }) = Frame::from_json(t.as_str()) {
                        let reply = Frame::Response {
                            id,
                            ok: false,
                            body: serde_json::Value::String("this member does not execute requests yet".into()),
                        };
                        tx.send(Message::text(reply.to_json()))
                            .await
                            .map_err(|e| MemberError::Transport(e.to_string()))?;
                    }
                }
                Some(Ok(Message::Close(_))) | None => return Err(MemberError::Transport("the host closed the connection".into())),
                Some(Ok(_)) => {}
                Some(Err(e)) => return Err(MemberError::Transport(e.to_string())),
            }
        }
    }
}

/// Run a member forever: reconnect with exponential backoff (1 s → 30 s) on transport loss; stop
/// only when the host refuses the token.
///
/// # Errors
/// [`MemberError::Refused`] — the only way this returns.
pub async fn run_member(cfg: MemberConfig, source: ReportSource) -> Result<(), MemberError> {
    let mut backoff = Duration::from_secs(1);
    loop {
        let admitted = std::sync::atomic::AtomicBool::new(false);
        let on_welcome = |m: &str| {
            admitted.store(true, std::sync::atomic::Ordering::SeqCst);
            eprintln!("qfs cluster: joined {} as `{m}`", cfg.url);
        };
        match session_once(&cfg, &source, &on_welcome).await {
            Err(MemberError::Refused(r)) => return Err(MemberError::Refused(r)),
            Err(e) => eprintln!("qfs cluster: {e}; reconnecting in {}s", backoff.as_secs()),
            Ok(()) => {}
        }
        if admitted.load(std::sync::atomic::Ordering::SeqCst) {
            backoff = Duration::from_secs(1);
        }
        tokio::time::sleep(backoff).await;
        backoff = (backoff * 2).min(Duration::from_secs(30));
    }
}
