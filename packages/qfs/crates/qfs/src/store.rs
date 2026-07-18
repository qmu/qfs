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
    let sys = SystemDb::open(&FileSource::new(path))?;
    copy_legacy_config_registry(&sys)?;
    Ok(Some(sys))
}

/// One-shot boot copy (ticket 20260716143641): move the two re-homed declarative tables'
/// PRE-EXISTING rows — `path_binding` + `connection_consent` — from their legacy Project-DB home
/// into the System DB, once. App-level and outside the per-DB migration framework because the two
/// files' migration chains cannot order against each other.
///
/// **Idempotency marker = the copy's own ledger event.** The copy is itself a config event, so it
/// lands in `sys_ddl_events` (verb `COPY`, secret-free row counts) in the SAME transaction as the
/// copied rows — and that event's presence is what makes every later boot a no-op. The guard is
/// marker-first (not "system tables empty"), so an operator who later DISCONNECTs every binding
/// does not resurrect dead Project-DB rows on the next boot.
///
/// Read-only toward the Project DB: nothing deletes vault rows; the dead tables are dropped by a
/// LATER Project-DB migration, deliberately not here (the drop must never run before the copy).
///
/// Loud failure over silent propagation: a copy that cannot complete fails the open (an operator
/// with legacy bindings must never boot into a silently-empty registry).
fn copy_legacy_config_registry(sys: &SystemDb) -> Result<(), StoreError> {
    let conn = sys.db().conn();
    let copy_failed = |detail: String| StoreError::Open { detail };

    // Marker check: has a copy event ever been recorded? (WORM tail — it can never disappear.)
    let already_copied = conn
        .query_row(
            "SELECT 1 FROM sys_ddl_events WHERE verb = 'COPY' AND target_path = '/sys/paths' \
             LIMIT 1",
            [],
            |_| Ok(()),
        )
        .is_ok();
    if already_copied {
        return Ok(());
    }
    // A System DB that already carries rows was written by the post-move writers (or a restore);
    // there is nothing legacy-shaped to heal. Skip WITHOUT recording a marker — the marker means
    // "the legacy rows were copied", never "we looked".
    let sys_rows: i64 = conn
        .query_row(
            "SELECT (SELECT COUNT(*) FROM path_binding) + (SELECT COUNT(*) FROM connection_consent)",
            [],
            |r| r.get(0),
        )
        .map_err(StoreError::from)?;
    if sys_rows > 0 {
        return Ok(());
    }
    // Only a Project DB that already EXISTS can hold legacy rows — never create one here (a fresh
    // install has nothing to copy, and creating the file would be a side effect of a read).
    let Some(project_path) = default_project_db_path() else {
        return Ok(());
    };
    if !project_path.exists() {
        return Ok(());
    }
    let project = ProjectDb::open(&FileSource::new(project_path))?;
    let pconn = project.db().conn();

    // Read the legacy rows (selectors + metadata + secret REFERENCES only — the tables' own rule).
    struct LegacyBinding {
        path: String,
        driver_id: Option<String>,
        at_locator: Option<String>,
        secret_ref: Option<String>,
        alias_of: Option<String>,
        created_at: String,
        host: String,
        account: Option<String>,
        app: Option<String>,
    }
    let mut bindings: Vec<LegacyBinding> = Vec::new();
    {
        let mut stmt = pconn
            .prepare(
                "SELECT path, driver_id, at_locator, secret_ref, alias_of, created_at, host, \
                 account, app FROM path_binding",
            )
            .map_err(StoreError::from)?;
        let rows = stmt
            .query_map([], |r| {
                Ok(LegacyBinding {
                    path: r.get(0)?,
                    driver_id: r.get(1)?,
                    at_locator: r.get(2)?,
                    secret_ref: r.get(3)?,
                    alias_of: r.get(4)?,
                    created_at: r.get(5)?,
                    host: r.get(6)?,
                    account: r.get(7)?,
                    app: r.get(8)?,
                })
            })
            .map_err(StoreError::from)?;
        for row in rows {
            bindings.push(row.map_err(StoreError::from)?);
        }
    }
    let mut consents: Vec<(String, String, String, String, String, Option<String>)> = Vec::new();
    {
        let mut stmt = pconn
            .prepare(
                "SELECT driver, connection, subject, scope, granted_at, app \
                 FROM connection_consent",
            )
            .map_err(StoreError::from)?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                ))
            })
            .map_err(StoreError::from)?;
        for row in rows {
            consents.push(row.map_err(StoreError::from)?);
        }
    }
    if bindings.is_empty() && consents.is_empty() {
        // Fresh install (or an already-drained legacy file): nothing to copy, no marker — the
        // post-move writers own the registry from here.
        return Ok(());
    }

    // Copy + the marker event, atomically. Full connects land before aliases (the alias_of FK).
    let tx = conn.unchecked_transaction().map_err(StoreError::from)?;
    bindings.sort_by_key(|b| b.alias_of.is_some());
    for b in &bindings {
        tx.execute(
            "INSERT INTO path_binding \
                 (path, driver_id, at_locator, secret_ref, alias_of, created_at, host, account, app) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            rusqlite::params![
                b.path,
                b.driver_id,
                b.at_locator,
                b.secret_ref,
                b.alias_of,
                b.created_at,
                b.host,
                b.account,
                b.app
            ],
        )
        .map_err(StoreError::from)?;
    }
    for c in &consents {
        tx.execute(
            "INSERT INTO connection_consent (driver, connection, subject, scope, granted_at, app) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![c.0, c.1, c.2, c.3, c.4, c.5],
        )
        .map_err(StoreError::from)?;
    }
    let ts = crate::sys::now_rfc3339();
    let payload = format!(
        "{{\"kind\":\"config_registry_copy\",\"path_bindings\":{},\"consents\":{}}}",
        bindings.len(),
        consents.len()
    );
    crate::sys::append_audit_tx(
        &tx,
        qfs_store::audit::AuditEvent {
            actor: "cli".to_string(),
            connection: "default".to_string(),
            verb: "COPY".to_string(),
            path: "/sys/paths".to_string(),
            committed: true,
            ts: ts.clone(),
        },
    )
    .map_err(|e| {
        copy_failed(format!(
            "recording the config-registry copy audit event: {e}"
        ))
    })?;
    crate::sys::append_ddl_event_tx(
        &tx,
        crate::sys::ddl_event("/sys/paths", "COPY", payload, ts),
    )
    .map_err(|e| {
        copy_failed(format!(
            "recording the config-registry copy ledger event: {e}"
        ))
    })?;
    tx.commit().map_err(StoreError::from)?;
    Ok(())
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
        // labeled Google app selectors + the consent ledger's bind-time secret_ref reference).
        assert_eq!(qfs_store::applied_migrations(proj.db()).unwrap().len(), 13);
        match prev_xdg {
            Some(v) => std::env::set_var("XDG_CONFIG_HOME", v),
            None => std::env::remove_var("XDG_CONFIG_HOME"),
        }
    }

    /// Ticket 20260716143641 QG2: the boot copy moves legacy Project-DB rows into the System DB
    /// once and only once — a second boot copies nothing, a fresh install copies nothing, and a
    /// row planted in the dead Project table AFTER the copy is invisible (even after every System
    /// row is later removed, the ledger marker blocks resurrection).
    #[test]
    fn boot_copy_moves_legacy_rows_once_and_only_once() {
        let _home = crate::testenv::HomeGuard::new();

        // Seed the LEGACY home: a Project DB (old shape) carrying one binding, one alias, and one
        // consent row — the operator's real pre-move state.
        let proj_path = default_project_db_path().unwrap();
        let project = ProjectDb::open(&FileSource::new(&proj_path)).unwrap();
        let pconn = project.db().conn();
        pconn
            .execute(
                "INSERT INTO path_binding (path, driver_id, at_locator, secret_ref, host, account) \
                 VALUES ('/chat', 'chatwork', 'https://api.chatwork.com/v2', 'vault:chatwork/work', \
                         'local', 'work')",
                [],
            )
            .unwrap();
        pconn
            .execute(
                "INSERT INTO path_binding (path, alias_of, host) VALUES ('/c', '/chat', 'local')",
                [],
            )
            .unwrap();
        pconn
            .execute(
                "INSERT INTO connection_consent (driver, connection, subject, scope) \
                 VALUES ('chatwork', 'work', 'op@example.com', '')",
                [],
            )
            .unwrap();
        drop(project);

        fn count(sys: &qfs_store::SystemDb, sql: &str) -> i64 {
            sys.db().conn().query_row(sql, [], |r| r.get(0)).unwrap()
        }

        // First boot copies (aliases after their targets, so the FK holds).
        let sys = open_system_db().unwrap().unwrap();
        assert_eq!(count(&sys, "SELECT COUNT(*) FROM path_binding"), 2);
        assert_eq!(count(&sys, "SELECT COUNT(*) FROM connection_consent"), 1);
        assert_eq!(
            count(
                &sys,
                "SELECT COUNT(*) FROM sys_ddl_events WHERE verb = 'COPY'"
            ),
            1,
            "the copy records itself as a ledger event (the idempotency marker)"
        );
        drop(sys);

        // Second boot: nothing copied again.
        let sys = open_system_db().unwrap().unwrap();
        assert_eq!(count(&sys, "SELECT COUNT(*) FROM path_binding"), 2);
        assert_eq!(
            count(
                &sys,
                "SELECT COUNT(*) FROM sys_ddl_events WHERE verb = 'COPY'"
            ),
            1
        );
        drop(sys);

        // A row planted in the DEAD Project table after the copy is invisible to every reader —
        // even after the operator removes every System-DB row (the marker, not emptiness, guards).
        let project = ProjectDb::open(&FileSource::new(&proj_path)).unwrap();
        project
            .db()
            .conn()
            .execute(
                "INSERT INTO path_binding (path, driver_id, host) \
                 VALUES ('/planted', 'chatwork', 'local')",
                [],
            )
            .unwrap();
        drop(project);
        let sys = open_system_db().unwrap().unwrap();
        sys.db()
            .conn()
            .execute("DELETE FROM path_binding", [])
            .unwrap();
        sys.db()
            .conn()
            .execute("DELETE FROM connection_consent", [])
            .unwrap();
        drop(sys);
        let sys = open_system_db().unwrap().unwrap();
        assert_eq!(
            count(&sys, "SELECT COUNT(*) FROM path_binding"),
            0,
            "dead-table rows must never resurrect after the copy has happened"
        );
        drop(sys);
    }

    /// QG2, fresh-install arm: no Project DB file → nothing copies, no marker is recorded (the
    /// marker means "the legacy rows were copied", never "we looked").
    #[test]
    fn boot_copy_is_a_no_op_on_a_fresh_install() {
        let _home = crate::testenv::HomeGuard::new();
        let sys = open_system_db().unwrap().unwrap();
        let conn = sys.db().conn();
        let bindings: i64 = conn
            .query_row("SELECT COUNT(*) FROM path_binding", [], |r| r.get(0))
            .unwrap();
        let markers: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sys_ddl_events WHERE verb = 'COPY'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!((bindings, markers), (0, 0));
        assert!(
            !default_project_db_path().unwrap().exists(),
            "the copy must never create a Project DB as a side effect of looking"
        );
    }

    /// Ticket 20260716143641 QG5 (no reader left behind), the source-level half: the modules that
    /// read or write the re-homed registry must not open the Project DB at all — their only
    /// database is the System DB. The Project DB (the vault) is opened by the credential-store
    /// plumbing (`connection.rs` guardians), the boot copy (this file), and the two state surfaces
    /// that report its migration count (`dump.rs`, `provision.rs`) — and nowhere else.
    #[test]
    fn no_registry_reader_opens_the_project_db() {
        let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        for file in [
            "cloud_mounts.rs",
            "describe.rs",
            "git.rs",
            "sql.rs",
            "declared_driver.rs",
            "account.rs",
            "commit.rs",
            "path_binding.rs",
            "restore.rs",
        ] {
            let body = std::fs::read_to_string(src.join(file)).unwrap();
            assert!(
                !body.contains("open_project_db") && !body.contains("open_project_conn"),
                "{file} must not open the Project DB — the config registry lives in the System DB \
                 (ticket 20260716143641)"
            );
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
        // §13 + v15 replayable DDL/config event log + v16 /transform definitions §15 + v17 the
        // re-homed config registry 20260716143641 + v18 the connection_consent `secret_ref`
        // selector column 20260718203325).
        let sys = open_system_db().unwrap().expect("config home resolves");
        assert_eq!(qfs_store::applied_migrations(sys.db()).unwrap().len(), 18);
        drop(sys);
        // Second open is a verified no-op (still the same applied migrations).
        let sys2 = open_system_db().unwrap().expect("config home resolves");
        assert_eq!(qfs_store::applied_migrations(sys2.db()).unwrap().len(), 18);
        match prev_xdg {
            Some(v) => std::env::set_var("XDG_CONFIG_HOME", v),
            None => std::env::remove_var("XDG_CONFIG_HOME"),
        }
    }
}
