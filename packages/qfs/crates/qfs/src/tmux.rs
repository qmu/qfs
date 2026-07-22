//! Safe teardown for a product-spawned tmux server (ticket 20260719105527).
//!
//! The mission `claude-code-sessions-are-queryable-and-steerable-as-qfs-paths` evaluates tmux as a
//! medium for launching (`new-session`) and steering (`send-keys`) Claude Code sessions. Whatever
//! else that path does, one rule is non-negotiable: **tearing a tmux server or session down must be
//! safe by construction, never by remembering to isolate.**
//!
//! ## The foot-gun this module removes
//! A bare `tmux kill-server` destroys **every session on that server**, not only the one a harness
//! created. It is safe *only* while the server is guaranteed to be a private throwaway — and when
//! that guarantee rests on `TMUX_TMPDIR` being exported into the same shell, a single missed export
//! (a child process, a subshell, a copied teardown line without its setup) lands the `kill-server`
//! on the developer's **default** socket and takes down all of their real, unrelated sessions. That
//! has repeatedly crashed the owner's live sessions on this shared host, so the whole live tmux path
//! is container-only (owner rulings 2026-07-19 / 2026-07-22).
//!
//! ## Safety by construction (not by environment variables)
//! Every server this module addresses carries an explicit, unique **`-L <socket>` name**. A `-L`
//! name travels as an **argument** on every subsequent command, so — unlike an un-exported env var —
//! it cannot be silently dropped. Teardown is a **targeted** `kill-session -t <session>` (or, at
//! most, a `kill-server` *scoped to the dedicated `-L` socket*); a bare `kill-server` that could
//! resolve to the default socket is never emitted. And before any teardown the isolation is
//! **asserted**: a [`TmuxSocket`] cannot exist without a non-empty dedicated name, so a mis-set
//! ("no `-L` socket") teardown is refused ([`TmuxError::NotIsolated`]) rather than killing the
//! default server.
//!
//! ## Host safety
//! The argv builders and the isolation guard are **pure** — they touch no process and are safe in the
//! ordinary hermetic suite. The one method that runs a real `tmux` ([`TmuxSocket::teardown`]) is
//! exercised only by the container-gated test; it never runs against the shared host's tmux.

use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

/// A structured, secret-free error from the tmux safe-teardown surface.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TmuxError {
    /// Teardown was requested without a dedicated `-L` socket (an empty/blank name). Refused rather
    /// than run: without an explicit `-L <name>` a tmux command resolves to the **default** socket,
    /// where a teardown would kill the developer's real sessions. This is the "assert isolation
    /// before killing" guard firing.
    #[error("refusing tmux teardown: no dedicated -L socket is set (a bare command would target the default server)")]
    NotIsolated,
    /// The `tmux` process could not be spawned or exited unsuccessfully. The reason is a
    /// secret-free string (a socket name / exit status is infra, never a credential).
    #[error("tmux teardown failed: {0}")]
    Teardown(String),
}

/// A **dedicated** tmux server socket, addressed by a unique `-L <name>`. Constructing one is the
/// isolation guarantee: a `TmuxSocket` cannot hold an empty name, so every command it builds carries
/// `-L <name>` and can never resolve to the developer's default socket.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TmuxSocket {
    /// The `-L` socket name — unique and non-empty by construction.
    name: String,
}

impl TmuxSocket {
    /// Wrap an explicit dedicated socket `name`. **Fail-closed**: a blank name is refused
    /// ([`TmuxError::NotIsolated`]) — the "assert isolation before killing" rule, enforced at the
    /// type boundary so no later code path can tear down without a dedicated `-L` socket.
    ///
    /// # Errors
    /// [`TmuxError::NotIsolated`] when `name` is empty or only whitespace.
    pub fn new(name: impl Into<String>) -> Result<Self, TmuxError> {
        let name = name.into();
        if name.trim().is_empty() {
            return Err(TmuxError::NotIsolated);
        }
        Ok(Self { name })
    }

    /// A fresh, **unique** dedicated socket for a product-spawned server: `qfs-<pid>-<nanos>`. Unique
    /// so two concurrent runs never share a server (and so it is never the default socket). Always
    /// non-empty, so it is isolated by construction.
    #[must_use]
    pub fn dedicated() -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        Self {
            name: format!("qfs-{}-{}", std::process::id(), nanos),
        }
    }

    /// The dedicated `-L` socket name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The argv (after the `tmux` program name) that starts a **detached** session on THIS dedicated
    /// socket: `-L <socket> new-session -d -s <session> -c <cwd>`. Keeping launch on the same `-L`
    /// socket is what makes the later teardown targetable. `cwd` (and any later command) are discrete
    /// arguments — never a shell line.
    #[must_use]
    pub fn new_session_argv(&self, session: &str, cwd: &str) -> Vec<String> {
        vec![
            "-L".to_string(),
            self.name.clone(),
            "new-session".to_string(),
            "-d".to_string(),
            "-s".to_string(),
            session.to_string(),
            "-c".to_string(),
            cwd.to_string(),
        ]
    }

    /// The argv (after `tmux`) that tears down ONE session **by target** on this dedicated socket:
    /// `-L <socket> kill-session -t <session>`.
    ///
    /// Two invariants hold for every value this returns, and the tests pin them: the argv always
    /// begins `-L <socket>` (so it can never resolve to the default socket) and uses
    /// `kill-session -t` — **never** the wholesale `kill-server`.
    #[must_use]
    pub fn kill_session_argv(&self, session: &str) -> Vec<String> {
        vec![
            "-L".to_string(),
            self.name.clone(),
            "kill-session".to_string(),
            "-t".to_string(),
            session.to_string(),
        ]
    }

    /// The argv (after `tmux`) that tears down the **whole dedicated server**, scoped to this `-L`
    /// socket: `-L <socket> kill-server`. This is the *only* sanctioned use of `kill-server` — it can
    /// destroy sessions only on THIS dedicated socket, never the default one, because the `-L <name>`
    /// travels as an argument ahead of it.
    #[must_use]
    pub fn kill_server_argv(&self) -> Vec<String> {
        vec![
            "-L".to_string(),
            self.name.clone(),
            "kill-server".to_string(),
        ]
    }

    /// Tear down `session` on this dedicated socket by running a real `tmux kill-session -t` (the
    /// isolation is guaranteed by construction). **Container-only**: a real tmux teardown must never
    /// run against the shared host's tmux server (mission ABSOLUTE prohibition) — the ordinary suite
    /// exercises only the pure argv builders + the [`TmuxError::NotIsolated`] guard.
    ///
    /// # Errors
    /// [`TmuxError::Teardown`] if `tmux` cannot be spawned or exits unsuccessfully.
    pub fn teardown(&self, session: &str) -> Result<(), TmuxError> {
        let output = Command::new("tmux")
            .args(self.kill_session_argv(session))
            .output()
            .map_err(|e| TmuxError::Teardown(format!("could not spawn tmux ({})", e.kind())))?;
        if !output.status.success() {
            return Err(TmuxError::Teardown(format!(
                "tmux exited unsuccessfully ({})",
                output.status
            )));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    /// The isolation guard: a teardown with **no dedicated `-L` socket** (an empty/blank name) is
    /// REFUSED rather than run — this is the "misconfigured isolation refuses instead of killing the
    /// default server" acceptance, proven without ever contacting a real tmux server.
    #[test]
    fn a_blank_socket_is_refused_never_targets_the_default_server() {
        assert_eq!(TmuxSocket::new("").unwrap_err(), TmuxError::NotIsolated);
        assert_eq!(TmuxSocket::new("   ").unwrap_err(), TmuxError::NotIsolated);
        // A real name builds a socket that is isolated by construction.
        assert!(TmuxSocket::new("qfs-test-1").is_ok());
    }

    /// Teardown argv is ALWAYS `-L <socket> kill-session -t <session>` — it carries the dedicated
    /// socket as its first argument (so it can never resolve to the default socket) and uses the
    /// targeted `kill-session`, NEVER the wholesale `kill-server`.
    #[test]
    fn kill_session_argv_is_targeted_and_socket_scoped_never_kill_server() {
        let sock = TmuxSocket::new("qfs-scratch-42").unwrap();
        let argv = sock.kill_session_argv("round");
        assert_eq!(
            argv,
            vec!["-L", "qfs-scratch-42", "kill-session", "-t", "round"]
        );
        assert_eq!(argv[0], "-L", "the -L socket leads every command");
        assert_eq!(
            argv[1], "qfs-scratch-42",
            "the dedicated socket, not default"
        );
        assert!(
            !argv.iter().any(|a| a == "kill-server"),
            "a targeted teardown never issues kill-server"
        );
    }

    /// Even the whole-server teardown is scoped to the dedicated `-L` socket — `kill-server` is only
    /// ever emitted with an explicit `-L <name>` ahead of it, so it can never hit the default server.
    #[test]
    fn kill_server_argv_is_always_scoped_to_the_dedicated_socket() {
        let sock = TmuxSocket::new("qfs-scratch-9").unwrap();
        let argv = sock.kill_server_argv();
        assert_eq!(argv, vec!["-L", "qfs-scratch-9", "kill-server"]);
        // The `-L <name>` prefix is what makes this safe: kill-server never appears bare / first.
        assert_eq!(argv[0], "-L");
        assert_ne!(argv[0], "kill-server", "kill-server is never the first arg");
    }

    /// A launch stays on the SAME dedicated socket (so the teardown can target it), and cwd rides as
    /// a discrete argument — no shell line.
    #[test]
    fn new_session_argv_stays_on_the_dedicated_socket() {
        let sock = TmuxSocket::new("qfs-scratch-7").unwrap();
        let argv = sock.new_session_argv("round", "/work/proj");
        assert_eq!(&argv[0..2], &["-L", "qfs-scratch-7"]);
        assert!(argv.contains(&"new-session".to_string()));
        assert!(argv.contains(&"/work/proj".to_string()));
    }

    /// A `dedicated()` socket is unique and non-empty (never the default), so two concurrent product
    /// runs cannot collide on one server.
    #[test]
    fn dedicated_sockets_are_unique_and_non_empty() {
        let a = TmuxSocket::dedicated();
        let b = TmuxSocket::dedicated();
        assert!(!a.name().is_empty());
        assert!(a.name().starts_with("qfs-"));
        assert_ne!(a.name(), b.name(), "each dedicated socket is unique");
    }

    /// The real-tmux-server proof is **container-only** (mission ABSOLUTE prohibition: never spawn or
    /// tear down a tmux server on the shared host). Ignored by default so `cargo test --workspace`
    /// never contacts a real server; the container live-round runs it with `--ignored`. Even then it
    /// no-ops unless `QFS_TMUX_LIVE=1` is set, a second belt over the `#[ignore]`.
    #[test]
    #[ignore = "spawns a real tmux server; container-only (QFS_TMUX_LIVE=1), never the shared host"]
    fn live_teardown_kills_only_the_targeted_session_on_the_dedicated_socket() {
        if std::env::var("QFS_TMUX_LIVE").ok().as_deref() != Some("1") {
            eprintln!("skipped: set QFS_TMUX_LIVE=1 in the container to run the real-server proof");
            return;
        }
        let sock = TmuxSocket::dedicated();
        // Start two detached sessions on the dedicated socket.
        for s in ["keep", "victim"] {
            let ok = Command::new("tmux")
                .args(sock.new_session_argv(s, "/"))
                .status()
                .expect("spawn tmux")
                .success();
            assert!(ok, "new-session {s} on the dedicated socket");
        }
        // Tear down ONLY `victim` by target.
        sock.teardown("victim").expect("targeted teardown");
        // `keep` must survive — the teardown was a kill-session, not a kill-server.
        let listed = Command::new("tmux")
            .args(["-L", sock.name(), "list-sessions"])
            .output()
            .expect("list-sessions");
        let names = String::from_utf8_lossy(&listed.stdout);
        assert!(
            names.contains("keep"),
            "the untargeted session survived: {names}"
        );
        assert!(
            !names.contains("victim"),
            "the targeted session is gone: {names}"
        );
        // Clean up the dedicated server (scoped kill-server — safe, it is our own -L socket).
        let _ = Command::new("tmux").args(sock.kill_server_argv()).status();
    }
}
