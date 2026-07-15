//! t42: the binary's System-DB composition root — open the per-host [`qfs_store::SystemDb`] and
//! apply embedded migrations on start.
//!
//! The binary is the ONLY crate that resolves a real DB path (decision F + the dep-direction guard:
//! `qfs-store` is a leaf; nothing in the spine names a file). Opening the DB and running migrations
//! is **start-time infrastructure**, NOT a qfs effect-plan — it never goes through preview/commit;
//! it is the substrate later `/sys/*` writes (t53) preview and commit *over*.
//!
//! t42 wires this WITHOUT routing any existing command through it: the file vault still backs
//! secrets until t43. So this is best-effort — a host with no resolvable config home, or a transient
//! open error, must not block the CLI.

use std::path::PathBuf;

use qfs_store::{FileSource, ProjectDb, StoreError, SystemDb};

/// Resolve the default System DB path.
///
/// **OPEN PRODUCT DECISION (flagged in t42 for the reviewer, not baked in):** the System DB sits
/// alongside the existing credential vault under `~/.config/qfs/` — the current
/// `qfs_secrets::default_credentials_path()` XDG/HOME convention — rather than a new
/// `~/.local/share/qfs/`. t43/t53 may revisit once a real surface uses it; until then this is the
/// least-surprising home (one `qfs` config dir, not two).
#[must_use]
pub fn default_system_db_path() -> Option<PathBuf> {
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        if !xdg.is_empty() {
            return Some(PathBuf::from(xdg).join("qfs").join("system.db"));
        }
    }
    #[cfg(test)]
    forbid_shared_home_fallback_in_tests();
    std::env::var("HOME")
        .ok()
        .filter(|h| !h.is_empty())
        .map(|home| {
            PathBuf::from(home)
                .join(".config")
                .join("qfs")
                .join("system.db")
        })
}

/// In this crate's own test build only, forbid the `$HOME/.config/qfs` fallback.
///
/// A qfs unit test that resolves the store here would open a **shared** file on the runner and race
/// its siblings' `schema_version` migration (the flaky-CI root cause, ticket 20260705022000). Every
/// env-mutating test isolates via [`crate::testenv::HomeGuard`], which sets a fresh
/// `XDG_CONFIG_HOME`, so reaching the HOME branch means a test forgot to — fail it loudly rather
/// than let it silently touch the shared home. `cfg(test)` is this crate only, so the shipped binary
/// and the integration-test crate keep the real HOME fallback.
#[cfg(test)]
fn forbid_shared_home_fallback_in_tests() {
    panic!(
        "the store resolved to the shared $HOME/.config/qfs (XDG_CONFIG_HOME unset) inside a qfs \
         unit test — wrap the test in `testenv::HomeGuard` so it uses an isolated config home"
    );
}

/// Open the System DB at the default path and apply migrations (idempotent; a second start is a
/// no-op). Returns:
/// - `Ok(Some(db))` — opened + migrated;
/// - `Ok(None)` — no resolvable config home (HOME/XDG unset), so the binary runs without it (no
///   command routes through the System DB yet, t42);
/// - `Err(_)` — a real open/migration failure the caller may log (still non-fatal in t42).
pub fn open_system_db() -> Result<Option<SystemDb>, StoreError> {
    let Some(path) = default_system_db_path() else {
        return Ok(None);
    };
    Ok(Some(SystemDb::open(&FileSource::new(path))?))
}

/// Resolve the default Project DB path (`$XDG_CONFIG_HOME/qfs/project.db`, falling back to
/// `~/.config/qfs/project.db`), mirroring [`default_system_db_path`] so both DBs share the one `qfs`
/// config dir.
///
/// **OPEN PRODUCT DECISION (flagged for the reviewer, t43 — not baked in):** today this is a SINGLE
/// default `project.db` (one project per host). The roadmap's §4.2 model is one Project DB *per
/// project*; the unresolved question is whether each project gets its OWN `project-<slug>.db` file
/// (file-per-project) or whether projects become rows inside one DB keyed by a project id
/// (rows-in-System, decision F's tenant→DB route). Until a real multi-project surface lands (t44+),
/// the binary opens one default `project.db`; revisit the file layout then.
#[must_use]
pub fn default_project_db_path() -> Option<PathBuf> {
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        if !xdg.is_empty() {
            return Some(PathBuf::from(xdg).join("qfs").join("project.db"));
        }
    }
    #[cfg(test)]
    forbid_shared_home_fallback_in_tests();
    std::env::var("HOME")
        .ok()
        .filter(|h| !h.is_empty())
        .map(|home| {
            PathBuf::from(home)
                .join(".config")
                .join("qfs")
                .join("project.db")
        })
}

/// Resolve the default session-unlock cache path (`$XDG_CONFIG_HOME/qfs/session.unlock`, falling back
/// to `~/.config/qfs/session.unlock`), beside `project.db` (ticket 20260704170000). `None` when no
/// config home resolves — the cache is then simply unavailable and the binary prompts as before.
#[must_use]
pub fn default_session_unlock_path() -> Option<PathBuf> {
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        if !xdg.is_empty() {
            return Some(PathBuf::from(xdg).join("qfs").join("session.unlock"));
        }
    }
    std::env::var("HOME")
        .ok()
        .filter(|h| !h.is_empty())
        .map(|home| {
            PathBuf::from(home)
                .join(".config")
                .join("qfs")
                .join("session.unlock")
        })
}

/// Open the Project DB at the default path and apply migrations (idempotent). Returns:
/// - `Ok(Some(db))` — opened + migrated (the t43 secret-store schema is now present);
/// - `Ok(None)` — no resolvable config home (HOME/XDG unset);
/// - `Err(_)` — a real open/migration failure.
///
/// The binary moves the migrated connection into the SQLite credential backend via the t42 seam
/// (`into_db().into_connection()` → `SqliteSecrets::open_or_init`).
pub fn open_project_db() -> Result<Option<ProjectDb>, StoreError> {
    let Some(path) = default_project_db_path() else {
        return Ok(None);
    };
    Ok(Some(ProjectDb::open(&FileSource::new(path))?))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    // We mutate process env (HOME/XDG) which is global; serialize via the crate-wide lock
    // (`crate::testenv::env_guard`) so the cases that read it don't race each other — OR the sibling
    // `oauth.rs` tests, which also mutate `XDG_CONFIG_HOME` (a shared lock, not a module-local one,
    // is what makes that mutual exclusion cross-module and the suite deterministic under the
    // parallel harness).
    use crate::testenv::env_guard;

    #[test]
    fn xdg_config_home_takes_precedence() {
        let _g = env_guard();
        let prev_xdg = std::env::var_os("XDG_CONFIG_HOME");
        std::env::set_var("XDG_CONFIG_HOME", "/x/cfg");
        assert_eq!(
            default_system_db_path(),
            Some(PathBuf::from("/x/cfg/qfs/system.db"))
        );
        match prev_xdg {
            Some(v) => std::env::set_var("XDG_CONFIG_HOME", v),
            None => std::env::remove_var("XDG_CONFIG_HOME"),
        }
    }

    #[test]
    fn project_db_path_follows_xdg_then_home() {
        let _g = env_guard();
        let prev_xdg = std::env::var_os("XDG_CONFIG_HOME");
        std::env::set_var("XDG_CONFIG_HOME", "/x/cfg");
        assert_eq!(
            default_project_db_path(),
            Some(PathBuf::from("/x/cfg/qfs/project.db"))
        );
        match prev_xdg {
            Some(v) => std::env::set_var("XDG_CONFIG_HOME", v),
            None => std::env::remove_var("XDG_CONFIG_HOME"),
        }
    }

    #[test]
    fn open_project_db_creates_and_migrates_the_secret_store() {
        let _g = env_guard();
        let dir = tempfile::tempdir().unwrap();
        let prev_xdg = std::env::var_os("XDG_CONFIG_HOME");
        std::env::set_var("XDG_CONFIG_HOME", dir.path());
        let proj = open_project_db().unwrap().expect("config home resolves");
        // All project migrations applied (skeleton + t43 secret store + t54 consent ledger +
        // t81 shared-connection registry + t79 rotation/revocation columns + t80 per-recipient E2E
        // wrap + t66 brokered team-connection registry + t100020 path-binding registry + ADR 0008
        // mount-coordinate columns + ADR 0008 vault-key slots + ADR 0008 active_account drop +
        // labeled Google app selectors).
        assert_eq!(qfs_store::applied_migrations(proj.db()).unwrap().len(), 12);
        match prev_xdg {
            Some(v) => std::env::set_var("XDG_CONFIG_HOME", v),
            None => std::env::remove_var("XDG_CONFIG_HOME"),
        }
    }

    #[test]
    fn open_system_db_creates_and_remigrates_idempotently() {
        let _g = env_guard();
        let dir = tempfile::tempdir().unwrap();
        let prev_xdg = std::env::var_os("XDG_CONFIG_HOME");
        std::env::set_var("XDG_CONFIG_HOME", dir.path());
        // First open creates + migrates (v1 skeleton + v2 audit chain t76 + v3 identity t45 + v4
        // sessions t46 + v5 oauth keys t48 + v6 oauth flow clients/codes t49 + v7 /sys/policies t53 +
        // v8 invites/memberships t55 + v9 oidc providers t56 + v10 /sys/settings t59 + v11 member
        // public keys t80 + v12 /sys/billing t67 + v13 hosts registry ADR 0008 + v14 /sys/drivers
        // §13 + v15 replayable DDL/config event log + v16 /transform definitions §15).
        let sys = open_system_db().unwrap().expect("config home resolves");
        assert_eq!(qfs_store::applied_migrations(sys.db()).unwrap().len(), 16);
        drop(sys);
        // Second open is a verified no-op (still the same applied migrations).
        let sys2 = open_system_db().unwrap().expect("config home resolves");
        assert_eq!(qfs_store::applied_migrations(sys2.db()).unwrap().len(), 16);
        match prev_xdg {
            Some(v) => std::env::set_var("XDG_CONFIG_HOME", v),
            None => std::env::remove_var("XDG_CONFIG_HOME"),
        }
    }
}
