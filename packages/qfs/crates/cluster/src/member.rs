//! The member side: connect, hello, heartbeat, reconnect with backoff — and, when the member
//! opted in with `--accept-spawn`, run the host's agent spawns concurrently ([`crate::spawn`]).

use std::sync::Arc;
use std::time::Duration;

use futures::{SinkExt as _, StreamExt as _};
use tokio_tungstenite::tungstenite::Message;

use crate::frame::{Frame, Report};
use crate::host::WS_PATH;
use crate::spawn::SpawnConfig;

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
    /// The spawn policy; `None` (the default) refuses every [`Frame::Spawn`].
    pub spawn: Option<SpawnConfig>,
}

impl std::fmt::Debug for MemberConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MemberConfig")
            .field("url", &self.url)
            .field("token", &"<redacted>")
            .field("name", &self.name)
            .field("heartbeat", &self.heartbeat)
            .field("spawn", &self.spawn)
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
        exec_only: false,
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
    // Spawned agents run as tasks and report here, so heartbeats keep flowing meanwhile.
    let (done_tx, mut done_rx) = tokio::sync::mpsc::unbounded_channel::<Frame>();
    let running = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    loop {
        tokio::select! {
            Some(result) = done_rx.recv() => {
                tx.send(Message::text(result.to_json()))
                    .await
                    .map_err(|e| MemberError::Transport(e.to_string()))?;
            }
            _ = tick.tick() => {
                let report = sample(source).await;
                tx.send(Message::text(Frame::Heartbeat { report }.to_json()))
                    .await
                    .map_err(|e| MemberError::Transport(e.to_string()))?;
            }
            msg = rx.next() => match msg {
                Some(Ok(Message::Text(t))) => {
                    if let Ok(Frame::Spawn { id, prompt, cwd, allowed_tools }) = Frame::from_json(t.as_str()) {
                        start_spawn(cfg.spawn.clone(), &running, &done_tx, id, prompt, cwd, allowed_tools);
                    } else if let Ok(Frame::Request { id, .. }) = Frame::from_json(t.as_str()) {
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

/// Answer one [`Frame::Spawn`]: refuse it without a policy or at the concurrency cap, else run
/// it on a task that sends its [`Frame::SpawnResult`] to `done`.
fn start_spawn(
    policy: Option<SpawnConfig>,
    running: &Arc<std::sync::atomic::AtomicUsize>,
    done: &tokio::sync::mpsc::UnboundedSender<Frame>,
    id: u64,
    prompt: String,
    cwd: Option<String>,
    allowed_tools: Vec<String>,
) {
    use std::sync::atomic::Ordering;
    let reply = move |(ok, output, exit_code): crate::spawn::Outcome| Frame::SpawnResult {
        id,
        ok,
        output,
        exit_code,
    };
    let Some(policy) = policy else {
        eprintln!("qfs cluster: refused spawn {id} (this member did not --accept-spawn)");
        let _ = done.send(reply(crate::spawn::refused()));
        return;
    };
    if running.fetch_add(1, Ordering::SeqCst) >= crate::spawn::MAX_CONCURRENT {
        running.fetch_sub(1, Ordering::SeqCst);
        let _ = done.send(reply((
            false,
            format!(
                "this member is already running {} agents",
                crate::spawn::MAX_CONCURRENT
            ),
            None,
        )));
        return;
    }
    eprintln!("qfs cluster: running spawn {id}");
    let running = Arc::clone(running);
    let done = done.clone();
    tokio::spawn(async move {
        let out = crate::spawn::run(&policy, id, &prompt, cwd.as_deref(), &allowed_tools).await;
        running.fetch_sub(1, Ordering::SeqCst);
        eprintln!("qfs cluster: spawn {id} finished (ok: {})", out.0);
        let _ = done.send(reply(out));
    });
}

/// How long a one-shot borrowed request waits for the host's answer.
pub const RUN_TIMEOUT: Duration = Duration::from_secs(300);

/// One borrowed execution over a one-shot connection (`qfs cluster run`): hello with the join
/// token (`exec_only`, so nothing is registered), send one [`Frame::Request`], await its
/// [`Frame::Response`], close. Returns `(ok, body)` exactly as the host answered.
///
/// # Errors
/// [`MemberError::Refused`] when the host refuses the token; [`MemberError::Transport`] on a
/// connection failure, a timeout, or an unexpected frame.
pub async fn run_once(
    cfg: &MemberConfig,
    statement: &str,
    commit: bool,
) -> Result<(bool, serde_json::Value), MemberError> {
    let t = |e: &dyn std::fmt::Display| MemberError::Transport(e.to_string());
    let (ws, _) = tokio_tungstenite::connect_async(ws_url(&cfg.url))
        .await
        .map_err(|e| t(&e))?;
    let (mut tx, mut rx) = ws.split();
    let hello = Frame::Hello {
        token: cfg.token.clone(),
        name: cfg.name.clone(),
        heartbeat_ms: u64::try_from(cfg.heartbeat.as_millis()).unwrap_or(u64::MAX),
        exec_only: true,
    };
    tx.send(Message::text(hello.to_json()))
        .await
        .map_err(|e| t(&e))?;
    let answer = async {
        let mut sent = false;
        while let Some(msg) = rx.next().await {
            let text = match msg.map_err(|e| t(&e))? {
                Message::Text(text) => text,
                Message::Close(_) => break,
                _ => continue,
            };
            match Frame::from_json(text.as_str()) {
                Ok(Frame::Refused { reason }) => return Err(MemberError::Refused(reason)),
                Ok(Frame::Welcome { .. }) if !sent => {
                    sent = true;
                    let req = Frame::Request {
                        id: 1,
                        statement: statement.to_string(),
                        commit,
                    };
                    tx.send(Message::text(req.to_json()))
                        .await
                        .map_err(|e| t(&e))?;
                }
                Ok(Frame::Response { id: 1, ok, body }) if sent => {
                    let _ = tx.close().await;
                    return Ok((ok, body));
                }
                _ => {}
            }
        }
        Err(if sent {
            MemberError::Transport("the host closed the connection before answering".into())
        } else {
            MemberError::Refused("the host closed the connection".into())
        })
    };
    tokio::time::timeout(RUN_TIMEOUT, answer)
        .await
        .map_err(|_| MemberError::Transport("timed out waiting for the host's answer".into()))?
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
