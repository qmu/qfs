//! Test-only environment isolation: one crate-wide lock + a fresh per-test config home.
//!
//! Two independent test-hygiene faults let two threads open the **same** system/project DB
//! concurrently under the parallel `cargo test` harness, racing the `schema_version` migration
//! check-then-insert and intermittently failing CI (ticket 20260705022000):
//!
//! 1. A test `remove_var("XDG_CONFIG_HOME")` fell back to the shared `$HOME/.config/qfs` — a single
//!    file on the runner — so two such tests opened the same DB.
//! 2. `ENV_LOCK` coverage was inconsistent (some env-mutating tests never held it), and a panic
//!    while holding it poisoned the lock, cascading `.lock().unwrap()` panics across every sibling.
//!
//! Every env-mutating test in this crate now goes through this module. [`HomeGuard`] holds the one
//! crate-wide lock, points `XDG_CONFIG_HOME` at a **fresh tempdir** (never the shared `$HOME`), and
//! restores the previous env on drop; [`env_guard`] is the bare lock for a test that only touches
//! namespaced vars. Both acquire the lock through a poison-clearing path so one failing test can
//! never cascade. The [`crate::store`] resolvers additionally panic (in this crate's test build
//! only) if they ever reach the `$HOME` fallback, so a future test cannot silently reintroduce
//! fault (1).

use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};

/// The one process-global lock serializing env-mutating tests across every module in this crate.
///
/// `XDG_CONFIG_HOME` (and the other config-home env vars the store openers read) is process-global;
/// a *module-local* lock would serialize within a module but not across modules, so a `store.rs`
/// test could still corrupt an in-flight `oauth.rs` test's config home. One shared lock makes every
/// env-mutating test across the crate mutually exclusive, so the suite is deterministic regardless
/// of thread scheduling.
static ENV_LOCK: Mutex<()> = Mutex::new(());

/// Acquire [`ENV_LOCK`], clearing any poison.
///
/// The lock guards no data invariant — it only serializes env mutation — so recovering a poisoned
/// guard is always safe. Routing every acquisition through here (never `ENV_LOCK.lock().unwrap()`)
/// means a test that panics while holding the lock cannot cascade into a wall of unrelated
/// `.lock().unwrap()` panics across every sibling env test.
pub(crate) fn env_guard() -> MutexGuard<'static, ()> {
    ENV_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// A fresh, isolated config home for one test: holds [`ENV_LOCK`], points `XDG_CONFIG_HOME` at a
/// tempdir, and restores the previous env on drop. Bind it for the whole test body
/// (`let _home = HomeGuard::new();`) — never `remove_var("XDG_CONFIG_HOME")` by hand, which would
/// leave the store resolving to the shared `$HOME/.config/qfs`.
pub(crate) struct HomeGuard {
    dir: tempfile::TempDir,
    prev_xdg: Option<OsString>,
    prev_pass: Option<OsString>,
    set_pass: bool,
    // Declared last so the lock is released *after* the env is restored and the tempdir removed
    // (fields drop in declaration order, and our `Drop` impl runs before any of them).
    _lock: MutexGuard<'static, ()>,
}

impl HomeGuard {
    /// A fresh config home with only `XDG_CONFIG_HOME` isolated (leaves `QFS_PASSPHRASE` untouched).
    pub(crate) fn new() -> Self {
        Self::build(None)
    }

    /// A fresh config home that also sets `QFS_PASSPHRASE` (the non-interactive automation path the
    /// `init`/`account`/`commit`/`shell` tests drive).
    pub(crate) fn with_passphrase(pass: &str) -> Self {
        Self::build(Some(pass))
    }

    fn build(pass: Option<&str>) -> Self {
        let lock = env_guard();
        let dir = tempfile::tempdir().expect("tempdir for isolated config home");
        let prev_xdg = std::env::var_os("XDG_CONFIG_HOME");
        let prev_pass = std::env::var_os("QFS_PASSPHRASE");
        std::env::set_var("XDG_CONFIG_HOME", dir.path());
        if let Some(pass) = pass {
            std::env::set_var("QFS_PASSPHRASE", pass);
        }
        Self {
            dir,
            prev_xdg,
            prev_pass,
            set_pass: pass.is_some(),
            _lock: lock,
        }
    }

    /// The system DB path under this isolated home (`<home>/qfs/system.db`).
    pub(crate) fn system_db_path(&self) -> PathBuf {
        self.dir.path().join("qfs").join("system.db")
    }
}

impl Drop for HomeGuard {
    fn drop(&mut self) {
        restore("XDG_CONFIG_HOME", self.prev_xdg.take());
        if self.set_pass {
            restore("QFS_PASSPHRASE", self.prev_pass.take());
        }
    }
}

fn restore(key: &str, prev: Option<OsString>) {
    match prev {
        Some(v) => std::env::set_var(key, v),
        None => std::env::remove_var(key),
    }
}
