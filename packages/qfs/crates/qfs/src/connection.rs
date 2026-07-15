//! The **credential-store plumbing + the connect layer's CLI I/O**: opening the envelope-encrypted
//! vault (the passphrase / keychain guardians), the sign-in gate, the rotation/rekey audit
//! emitter, and the `qfs connect` / `qfs disconnect` / `qfs connect --list` defined-path verbs —
//! injected into `qfs-cmd` as the [`qfs_cmd::ConnectionLauncher`]. The credentialed
//! `qfs connection` verb NAMESPACE is retired (ADR 0008 §3): accounts live under `qfs account`,
//! app registrations under `qfs app`, the store re-wrap under `qfs vault rekey`, and the account
//! a statement runs as comes off the MOUNT — selection state (`active_account`) is dropped by
//! migration #11.
//!
//! `qfs-cmd` may not depend on the concrete `qfs-secrets` backend (the dep_direction guard), so —
//! exactly like the shell / serve / describe launchers — the binary owns this and `qfs-cmd` only
//! parses the verb and calls in.
//!
//! ## Security (blueprint §8)
//! - A credential **value** is read from **stdin**, never from argv (argv leaks into shell
//!   history and `ps`).
//! - Credentials live in the envelope-encrypted SQLite **Project DB** ([`crate::secret_store`]):
//!   a random data-key (DEK) encrypts each secret value (ChaCha20-Poly1305), and the DEK is
//!   wrapped once per KeyGuardian slot (ADR 0008 §5). Secrets are never printed, logged, or
//!   echoed.

use qfs_cmd::ConnectionAction;
use qfs_identity::{IdentityStore, SoleUser};
use qfs_secrets::{Secret, Secrets};
use qfs_store::audit::AuditEvent;
use rusqlite::{Connection, OptionalExtension};

use crate::secret_store::{self, SqliteSecrets};

/// The injected connection launcher. Returns the process exit code (`0` ok, `1` on a structured,
/// secret-free error). Never panics.
#[must_use]
pub fn run_connection(action: &ConnectionAction) -> i32 {
    match run_inner(action) {
        Ok(msg) => {
            eprintln!("qfs: {msg}");
            0
        }
        Err(e) => {
            eprintln!("qfs: error: {e}");
            1
        }
    }
}

/// Open the migrated Project DB and return its **owned** connection (the t42 seam). The connection
/// carries the t43 secret-store schema; callers either move it into [`SqliteSecrets`] (the credential
/// path) or use it directly for the passphrase-free `active_account` table.
pub(crate) fn open_project_conn() -> Result<Connection, String> {
    let proj = crate::store::open_project_db()
        .map_err(|e| format!("opening the project database: {e}"))?
        .ok_or("cannot determine the project database path (set HOME or XDG_CONFIG_HOME)")?;
    Ok(proj.into_db().into_connection())
}

/// Has the credential store already been initialized on this host? True once any vault-key slot
/// exists (the first [`SqliteSecrets::open_or_init`] enrolls the passphrase slot; migration #10
/// forward-copied the pre-slot `secret_meta` wrap). Passphrase-free — it reads only a presence
/// flag, never key material — so it can decide *before* we know the passphrase whether this is a
/// first-run store-creation (prompt + confirm) or an unlock.
fn store_initialized(conn: &Connection) -> bool {
    conn.query_row("SELECT 1 FROM vault_key_slot LIMIT 1", [], |_| Ok(()))
        .optional()
        .ok()
        .flatten()
        .is_some()
}

/// A passphrase entered at an interactive prompt EARLIER in this same process. The store is opened
/// many times per process — once per credentialed driver the shell binds at startup ([`commit`]'s
/// `networked_credential` opens it before it even knows if the driver is configured), plus every
/// read/commit leg — so without this an interactive session would prompt several times. Caching the
/// first-entered passphrase makes `qfs` (the interactive shell especially) prompt AT MOST ONCE and
/// reuse it for the whole session — the in-memory equivalent of `read -rs QFS_PASSPHRASE; export`,
/// except the secret never touches your shell env or history. Process-scoped only: it can never
/// reach a *sibling* `qfs` invocation (a child cannot mutate its parent shell), which is why a
/// multi-command workflow wants the shell, not repeated one-shots. Not populated from
/// `QFS_PASSPHRASE` (that path needs no cache) and never logged.
static PROMPTED_PASSPHRASE: std::sync::OnceLock<Secret> = std::sync::OnceLock::new();

/// Resolve the passphrase that unlocks (or initializes) the credential store. `QFS_PASSPHRASE` is
/// the fast path automation always takes; else a passphrase already entered this process is reused
/// ([`PROMPTED_PASSPHRASE`]); else a human at a terminal is PROMPTED (echo off), so no one has to
/// `export QFS_PASSPHRASE` by hand. On a brand-new store the prompt confirms twice (a typo would
/// otherwise lock the operator out of their own vault); on an existing store it asks once.
/// Non-interactive + unset stays the same clear error automation already relies on.
fn resolve_store_passphrase(conn: &Connection) -> Result<Secret, String> {
    match std::env::var("QFS_PASSPHRASE") {
        Ok(pass) if !pass.is_empty() => return Ok(Secret::from(pass)),
        Ok(_) => return Err("QFS_PASSPHRASE is empty".into()),
        Err(_) => {}
    }
    // Reuse a passphrase already entered this process (prompt once per session, not per driver).
    if let Some(cached) = PROMPTED_PASSPHRASE.get() {
        return Ok(Secret::from(cached.expose().to_vec()));
    }
    // Gate on the CONTROLLING terminal, not stdin: the pipe-a-secret commands (`app add`,
    // `account add`) carry the credential on stdin by design, and the prompt below reads the
    // passphrase from /dev/tty — a piped stdin must not disable it (the v0.0.14 regression).
    if !crate::tty::can_prompt_secret() {
        return Err(
            "QFS_PASSPHRASE is not set and no terminal is available to prompt — export it \
                    (`read -rs QFS_PASSPHRASE; export QFS_PASSPHRASE`) to unlock the encrypted \
                    credential store"
                .into(),
        );
    }
    let entered = if store_initialized(conn) {
        crate::tty::prompt_secret("QFS passphrase (unlocks your local credential store): ")?
    } else {
        eprintln!(
            "Welcome to qfs. Setting up your encrypted credential store on this machine — choose a \
             passphrase you'll reuse to unlock it (it never leaves this host)."
        );
        crate::tty::prompt_secret_confirmed("Choose a passphrase: ", "Confirm passphrase: ")?
    };
    // A fresh INTERACTIVE prompt happened this process — authorize minting a time-boxed session
    // cache (ticket 20260704170000) so sibling one-shot invocations skip the re-prompt within the
    // TTL. Only this interactive branch sets the flag; a QFS_PASSPHRASE-env unlock never mints.
    crate::session_unlock::JUST_PROMPTED.store(true, std::sync::atomic::Ordering::SeqCst);
    // Cache for the rest of this process so later store-opens don't re-prompt. First writer wins;
    // a lost race just means the redundant entry is dropped (both are the same passphrase anyway).
    let _ = PROMPTED_PASSPHRASE.set(Secret::from(entered.expose().to_vec()));
    Ok(entered)
}

/// Open the envelope-encrypted SQLite credential store: open + migrate the Project DB, then unlock
/// through the guardian slots (ADR 0008 §5) — an enrolled OS-keychain slot first (non-interactive:
/// no prompt, no env var), else the passphrase (`QFS_PASSPHRASE` / the process cache / an
/// interactive prompt — see [`resolve_store_passphrase`]), which also initializes a fresh store.
pub(crate) fn open_store() -> Result<SqliteSecrets, String> {
    // Guardian 1 — the OS keychain (non-interactive), if enrolled AND available on this host.
    {
        let conn = open_project_conn()?;
        let slots = SqliteSecrets::db_load_slots(&conn)
            .map_err(|e| format!("reading the vault key slots: {e}"))?;
        if slots
            .iter()
            .any(|s| s.guardian_kind == secret_store::GUARDIAN_KEYCHAIN)
        {
            if let Some(kek) = crate::vault::keychain_kek() {
                if let Ok(store) = SqliteSecrets::open_with_resolver(conn, |s| {
                    (s.guardian_kind == secret_store::GUARDIAN_KEYCHAIN).then_some(kek)
                }) {
                    return Ok(store);
                }
                // Stale keychain material (rotated store, restored backup): fall through to the
                // session / passphrase guardians below on a fresh connection.
            }
        }
    }
    // Guardian 2 — the time-boxed session cache (ticket 20260704170000): a prior interactive unlock
    // this TTL window, non-interactive. Consulted BEFORE the passphrase, mirroring the keychain
    // branch; a machine-changed / tampered wrap fails to open and is purged, then we fall through.
    if let Some((slot, kek)) = crate::session_unlock::session_unlock_material() {
        let conn = open_project_conn()?;
        if let Ok(store) = SqliteSecrets::open_with_slot(conn, &slot, kek) {
            return Ok(store);
        }
        let _ = crate::session_unlock::purge_session();
    }
    // Guardian 3 — the passphrase (may PROMPT; also initializes a fresh store). A fresh interactive
    // prompt mints a session cache so the next one-shot invocation skips the prompt within the TTL.
    let conn = open_project_conn()?;
    let pass = resolve_store_passphrase(&conn)?;
    let store = SqliteSecrets::open_or_init(conn, &pass)
        .map_err(|e| format!("opening the credential store: {e}"))?;
    crate::session_unlock::maybe_mint_session(&store);
    Ok(store)
}

/// Non-interactive passphrase resolution for the best-effort paths: `QFS_PASSPHRASE`, else a
/// passphrase already prompted earlier in this process ([`PROMPTED_PASSPHRASE`]), else `None`.
/// **Never prompts** — see [`open_store_for_commit`] for why.
fn quiet_store_passphrase() -> Option<Secret> {
    match std::env::var("QFS_PASSPHRASE") {
        Ok(pass) if !pass.is_empty() => return Some(Secret::from(pass)),
        _ => {}
    }
    PROMPTED_PASSPHRASE
        .get()
        .map(|cached| Secret::from(cached.expose().to_vec()))
}

/// Make the credential store unlockable for a scan that **provably needs a cloud credential**
/// (the executing query reads a cloud mount): the quiet paths first (keychain slot,
/// `QFS_PASSPHRASE`, a passphrase already prompted this process — exactly what
/// [`open_store_for_commit`] consults), else PROMPT the human on the controlling terminal once,
/// caching the entry process-wide so every later store-open resolves quietly. Returns `false`
/// when no path can unlock (headless without the env var) — the caller surfaces a locked-store
/// hint instead of a misleading "no account" one.
///
/// This deliberately does NOT weaken [`open_store_for_commit`]'s never-prompt contract: the
/// registry build stays quiet; only a scan that actually executes over a cloud mount reaches
/// this, at the moment the credential is genuinely needed (the same moment `qfs account add`
/// would prompt).
pub(crate) fn ensure_store_unlocked_for_scan() -> bool {
    if open_store_for_commit().is_some() {
        return true;
    }
    if !crate::tty::can_prompt_secret() {
        return false;
    }
    let Ok(conn) = open_project_conn() else {
        return false;
    };
    // Prompts on /dev/tty and caches into PROMPTED_PASSPHRASE, so the quiet paths succeed from
    // here on. A wrong passphrase surfaces later as a store-open error, not a silent bind skip.
    resolve_store_passphrase(&conn).is_ok()
}

/// Open the credential store for the **commit resolver** (read path): the same envelope-encrypted
/// SQLite store `qfs account add` writes to, when the passphrase + the Project DB are both available.
/// Returns `None` (best-effort, never an error) when the store cannot be unlocked — the commit
/// registry then falls back to the env-var store, and a missing credential surfaces lazily as a
/// clear per-leg auth error rather than a panic. Never logs the passphrase.
///
/// **Never prompts.** The commit registry is built for every `qfs run` — including a
/// credential-free PREVIEW — and once per cloud driver, so an interactive prompt here would
/// interrogate the operator for a passphrase the command may not even need (and block a
/// non-human PTY forever). Only the explicit credential-management paths ([`open_store`]:
/// `qfs account add`/`list`/`remove`/…) may prompt; this path reuses their cached entry or the
/// env var, else falls back quietly.
#[must_use]
pub fn open_store_for_commit() -> Option<SqliteSecrets> {
    // Guardian 1 — ADR 0008 §5: an enrolled OS-keychain slot unlocks the commit resolver
    // non-interactively — exactly the guardian this best-effort path is allowed to use (no prompt).
    {
        let conn = open_project_conn().ok()?;
        let slots = SqliteSecrets::db_load_slots(&conn).ok()?;
        if slots
            .iter()
            .any(|s| s.guardian_kind == secret_store::GUARDIAN_KEYCHAIN)
        {
            if let Some(kek) = crate::vault::keychain_kek() {
                if let Ok(store) = SqliteSecrets::open_with_resolver(conn, |s| {
                    (s.guardian_kind == secret_store::GUARDIAN_KEYCHAIN).then_some(kek)
                }) {
                    return Some(store);
                }
                // Stale keychain material: fall through to the session / quiet-passphrase guardians.
            }
        }
    }
    // Guardian 2 — the time-boxed session cache (ticket 20260704170000), non-interactive: THIS is
    // what lets a repeated one-shot `qfs run` skip the re-prompt after one unlock within the TTL. A
    // machine-changed / tampered wrap fails to open and is purged, then we fall through.
    if let Some((slot, kek)) = crate::session_unlock::session_unlock_material() {
        let conn = open_project_conn().ok()?;
        if let Ok(store) = SqliteSecrets::open_with_slot(conn, &slot, kek) {
            return Some(store);
        }
        let _ = crate::session_unlock::purge_session();
    }
    // Guardian 3 — a passphrase already available quietly (env var / a prompt earlier this process).
    // NEVER prompts. If a prompt DID happen earlier (the scan path), mint a session now so the next
    // invocation resolves through guardian 2 without prompting.
    let conn = open_project_conn().ok()?;
    let pass = quiet_store_passphrase()?;
    let store = SqliteSecrets::open_or_init(conn, &pass).ok()?;
    crate::session_unlock::maybe_mint_session(&store);
    Some(store)
}

/// t54 / M4 — the **sign-in mandatory** gate for a cloud driver. A cloud credential is unusable for
/// an unauthenticated operator (decision B/C: fail closed), so `qfs account add`/`rotate` for a
/// cloud provider first resolves the signed-in identity from the System-DB identity store (t45). Returns the
/// operator's identity (their primary email) to record on the consent grant, or a structured,
/// secret-free error naming the remedy.
///
/// Sessions (t46) are not yet wired into the CLI, so "signed in" today means **a signed-up identity
/// exists on this host**: exactly one user resolves unambiguously; many users without a session can't
/// be attributed, so we fail closed and ask for an explicit identity rather than guessing.
///
/// OPEN PRODUCT DECISION (flagged, not guessed — roadmap §3.1 talks teams, not the solo case): does a
/// solo single-user laptop still need to sign in for a cloud connection? Today we apply the rule
/// uniformly (fail closed) — the safe default — and leave relaxing it for the solo case to a product
/// call rather than baking an implicit exception here.
pub(crate) fn require_signed_in(driver: &str) -> Result<String, String> {
    let store = crate::identity::open_identity_store()?;
    match store
        .sole_user()
        .map_err(|e| format!("checking sign-in status: {e}"))?
    {
        SoleUser::One(u) => Ok(u.primary_email),
        SoleUser::None => Err(format!(
            "cloud driver '{driver}' requires sign-in — run `qfs init` first \
             (cloud connections are unusable for an unauthenticated operator)"
        )),
        SoleUser::Many => Err(format!(
            "cloud driver '{driver}' requires a signed-in identity, but this host has multiple users \
             and no session yet — sign in as a specific identity before adding a cloud connection"
        )),
    }
}

/// The t76 audit event a rotation / revocation / re-key emits — metadata ONLY (the verb, the
/// `<driver>/<connection>` selector, the `/sys/connections` surface, a timestamp), **never** a
/// secret. Kept as a pure builder so the emitted shape is unit-testable over an explicit System DB.
fn connection_audit_event(verb: &str, connection: &str) -> AuditEvent {
    AuditEvent {
        actor: "cli".to_string(),
        connection: connection.to_string(),
        verb: verb.to_string(),
        path: "/sys/connections".to_string(),
        committed: true,
        ts: now_rfc3339(),
    }
}

/// Append a credential rotation/revocation/rekey event onto the t76 hash chain (best-effort,
/// exactly like the commit path's `emit_audit`): a missing/unavailable System DB is logged at
/// debug and never breaks the operation. Secret-free — the event carries selectors + metadata
/// only. Shared by `qfs account rotate/revoke` and `qfs vault rekey`.
pub(crate) fn emit_connection_audit(verb: &str, connection: &str) {
    let event = connection_audit_event(verb, connection);
    match crate::store::open_system_db() {
        Ok(Some(sys)) => {
            if let Err(e) = crate::audit::append_event(&sys, event) {
                tracing::debug!(target: "qfs::audit", "connection audit append failed (continuing): {e}");
            }
        }
        Ok(None) => {}
        Err(e) => {
            tracing::debug!(target: "qfs::audit", "connection audit skipped (system DB unavailable): {e}");
        }
    }
}

/// The current UTC time as an RFC3339 string for an audit event's `ts` (mirrors `commit::now_rfc3339`
/// — a clock read can fail to format only on an impossible date; fall back to the epoch, never panic).
fn now_rfc3339() -> String {
    use time::format_description::well_known::Rfc3339;
    time::OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string())
}

fn run_inner(action: &ConnectionAction) -> Result<String, String> {
    match action {
        ConnectionAction::ImportEnv => {
            // Print the CREATE CONNECTION declarations equivalent to the current QFS_SQL_*/QFS_GIT_*
            // env vars — the one-command migration off the deprecated convention. No secret is read
            // (SQLite/git connections carry none); the output is paste-ready for a connections.qfs.
            let decls = crate::connections_config::import_env_declarations();
            if decls.is_empty() {
                return Ok("no QFS_SQL_* / QFS_GIT_* env vars to import".into());
            }
            println!("{decls}");
            Ok("printed the equivalent CREATE CONNECTION declarations".into())
        }
        // t100020 (the CONNECT model): bind a defined PATH to a driver + credential reference (or an
        // alias). Direct Project-DB I/O — the twin of the `CONNECT` statement. No passphrase: the
        // binding is metadata + a secret REFERENCE, never a value.
        ConnectionAction::Connect {
            path,
            driver,
            at,
            secret_ref,
            alias_of,
            host,
            account,
            app,
        } => run_connect(
            path,
            driver.as_deref(),
            at.as_deref(),
            secret_ref.as_deref(),
            alias_of.as_deref(),
            host.as_deref(),
            account.as_deref(),
            app.as_deref(),
        ),
        ConnectionAction::Disconnect { path } => {
            let conn = open_project_conn()?;
            let n = crate::path_binding::db_remove_binding(&conn, path)
                .map_err(|e| format!("removing the defined path: {e}"))?;
            Ok(if n == 0 {
                format!("{path} was not connected (idempotent)")
            } else {
                format!("disconnected {path}")
            })
        }
        ConnectionAction::ListPaths => {
            let conn = open_project_conn()?;
            let rows = crate::path_binding::db_list_bindings(&conn)
                .map_err(|e| format!("listing defined paths: {e}"))?;
            let rendered = render_path_bindings(&rows);
            if !rendered.is_empty() {
                println!("{rendered}");
            }
            Ok(if rows.is_empty() {
                "no defined paths (use `qfs connect`)".into()
            } else {
                format!("{} defined path(s)", rows.len())
            })
        }
    }
}

fn render_path_bindings(rows: &[crate::path_binding::PathBindingRow]) -> String {
    let mut lines = Vec::new();
    for r in rows {
        // Selectors + metadata only — the `secret_ref` is a REFERENCE (env:/vault:), never a value.
        // An alias renders its target; a full connect its driver plus optional locator/account/app.
        if let Some(target) = &r.alias_of {
            lines.push(format!("{}\t-> {target}\t(alias)", r.path));
        } else {
            let driver = r.driver_id.as_deref().unwrap_or("?");
            let at = r
                .at_locator
                .as_deref()
                .map_or(String::new(), |a| format!("\tat {a}"));
            let secret = r
                .secret_ref
                .as_deref()
                .map_or(String::new(), |s| format!("\tsecret {s}"));
            let account = r
                .account
                .as_deref()
                .map_or(String::new(), |a| format!("\taccount {a}"));
            let app = r
                .app
                .as_deref()
                .map_or(String::new(), |a| format!("\tapp {a}"));
            lines.push(format!("{}\t{driver}{at}{secret}{account}{app}", r.path));
        }
    }
    lines.join("\n")
}

/// Bind a defined path (`qfs connect`): validate the arms (exactly one of `driver` / `alias_of`),
/// then UPSERT the binding into the Project DB `path_binding` table. A `vault:`/`env:` secret is
/// stored as a REFERENCE only (resolved at use time) — nothing secret touches argv or the row.
#[allow(clippy::too_many_arguments)]
fn run_connect(
    path: &str,
    driver: Option<&str>,
    at: Option<&str>,
    secret_ref: Option<&str>,
    alias_of: Option<&str>,
    host: Option<&str>,
    account: Option<&str>,
    app: Option<&str>,
) -> Result<String, String> {
    run_connect_with_cf_resolver(
        path,
        driver,
        at,
        secret_ref,
        alias_of,
        host,
        account,
        app,
        crate::cf::resolve_cf_account_id_for_connect,
    )
}

#[allow(clippy::too_many_arguments)]
fn run_connect_with_cf_resolver(
    path: &str,
    driver: Option<&str>,
    at: Option<&str>,
    secret_ref: Option<&str>,
    alias_of: Option<&str>,
    host: Option<&str>,
    account: Option<&str>,
    app: Option<&str>,
    resolve_cf_account: impl Fn(&str) -> Result<crate::cf::CloudflareAccountResolution, String>,
) -> Result<String, String> {
    if !path.starts_with('/') {
        return Err(format!("a defined path must be absolute, got `{path}`"));
    }
    // ADR 0008 §1: a --host must name a recorded host (or be the implicit `local`). The mount
    // records it; binding a mount to a remote host fails closed at bind time (the remote protocol
    // is deferred, ADR §6) — validated here so an unknown host is caught at connect, not at use.
    crate::hosts::require_known_host(host)?;
    let conn = open_project_conn()?;
    match (driver, alias_of) {
        (Some(_), Some(_)) => {
            Err("give either --driver (full connect) or --alias-of (alias), not both".into())
        }
        (None, None) => {
            Err("a full connect needs --driver (or --alias-of <path> for an alias)".into())
        }
        (Some(driver), None) => {
            // A cloud mount needs its account credential to exist before we write the binding
            // (origin/main): fail closed with an actionable message rather than a mount that later
            // breaks at use. For `cf` we then auto-discover the account id from that same token.
            require_account_for_cloud_connect(driver, account)?;
            let resolved_at = resolve_connect_at(driver, at, account, resolve_cf_account)?;
            let at = resolved_at.as_deref().or(at);
            // ADR 0008: the mount carries the (host, driver, account) coordinate — an omitted
            // --host is the implicit `local` host (defaulted in the binding I/O).
            crate::path_binding::db_upsert_binding(
                &conn, path, driver, at, secret_ref, host, account, app,
            )
            .map_err(|e| format!("connecting {path}: {e}"))?;
            let acct = account.map_or(String::new(), |a| format!(" ({a})"));
            let app = app.map_or(String::new(), |a| format!(" via app {a}"));
            Ok(format!("connected {path} -> {driver}{acct}{app}"))
        }
        (None, Some(target)) => {
            crate::path_binding::db_upsert_alias(&conn, path, target).map_err(|e| {
                // A foreign-key failure means the alias target is not a defined path (fail-closed).
                if matches!(&e, rusqlite::Error::SqliteFailure(err, _)
                    if err.code == rusqlite::ErrorCode::ConstraintViolation)
                {
                    format!("the alias target `{target}` is not a defined path — connect it first")
                } else {
                    format!("aliasing {path}: {e}")
                }
            })?;
            Ok(format!("connected {path} -> {target} (alias)"))
        }
    }
}

fn resolve_connect_at(
    driver: &str,
    at: Option<&str>,
    account: Option<&str>,
    resolve_cf_account: impl Fn(&str) -> Result<crate::cf::CloudflareAccountResolution, String>,
) -> Result<Option<String>, String> {
    if driver != "cf" || at.is_some() {
        return Ok(None);
    }
    let account = account.unwrap_or("default");
    match resolve_cf_account(account)? {
        crate::cf::CloudflareAccountResolution::Resolved(id) => Ok(Some(id.as_str().to_string())),
        crate::cf::CloudflareAccountResolution::NoneVisible => Err(format!(
            "Cloudflare token for account `{account}` can see no accounts; check the token or pass \
             `--at <id>` to bind an explicit account id"
        )),
        crate::cf::CloudflareAccountResolution::Ambiguous(accounts) => {
            let visible = accounts
                .iter()
                .map(|a| format!("{} ({})", a.id.as_str(), a.name))
                .collect::<Vec<_>>()
                .join(", ");
            Err(format!(
                "Cloudflare token for account `{account}` can see multiple accounts: {visible}; \
                 pass `--at <id>` to choose one"
            ))
        }
    }
}

/// A cloud mount is only useful once the named service account exists. Refuse `qfs connect` before
/// writing `path_binding` so operators do not see a successful-looking mount that later fails at use.
fn require_account_for_cloud_connect(driver: &str, account: Option<&str>) -> Result<(), String> {
    let driver_id = qfs_secrets::DriverId::new(driver);
    if !qfs_secrets::is_cloud_driver(&driver_id) {
        return Ok(());
    }
    let Some(account) = account else {
        return Err(format!(
            "cloud driver `{driver}` needs --account <label> — run `qfs account add {} <label>` first",
            account_provider_hint(driver)
        ));
    };
    let key = credential_key_for_mount(driver, account)?;
    let store = open_store()?;
    store.get(&key).map_err(|e| match e {
        qfs_secrets::SecretError::NotFound(_) => format!(
            "no credential for {driver}/{account} — run `qfs account add {} {account}` before `qfs connect`",
            account_provider_hint(driver)
        ),
        qfs_secrets::SecretError::Revoked(_) => format!(
            "credential for {driver}/{account} is revoked — run `qfs account rotate {} {account}` or add another account before `qfs connect`",
            account_provider_hint(driver)
        ),
        other => format!("checking the {driver}/{account} credential before connect: {other}"),
    })?;
    Ok(())
}

fn account_provider_hint(driver: &str) -> &str {
    match driver {
        "gmail" | "gdrive" | "drive" | "ga" | "google-analytics" => "google",
        "s3" | "r2" => "objstore",
        other => other,
    }
}

fn credential_key_for_mount(
    driver: &str,
    account: &str,
) -> Result<qfs_secrets::CredentialKey, String> {
    match driver {
        "gmail" | "gdrive" | "drive" | "ga" | "google-analytics" => {
            qfs_google_auth::refresh_token_key(account).map_err(|e| e.to_string())
        }
        "s3" | "r2" => {
            let conn = qfs_secrets::ConnectionId::new(account).map_err(|e| e.to_string())?;
            Ok(qfs_secrets::CredentialKey::new(
                qfs_secrets::DriverId::new("objstore"),
                conn,
            ))
        }
        other => {
            let conn = qfs_secrets::ConnectionId::new(account).map_err(|e| e.to_string())?;
            Ok(qfs_secrets::CredentialKey::new(
                qfs_secrets::DriverId::new(other),
                conn,
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use qfs_driver_cf::{MockCfBackend, RecordedCall};

    const PLANTED_CF_TOKEN: &str = "PLANTED-CF-TOKEN-connect-test";

    fn resolve_from_mock(
        backend: Arc<MockCfBackend>,
    ) -> impl Fn(&str) -> Result<crate::cf::CloudflareAccountResolution, String> {
        move |_| {
            crate::cf::resolve_cf_account_id_from_backend(backend.as_ref())
                .map_err(|e| e.to_string())
        }
    }

    /// `store_initialized` is the passphrase-free flag that decides first-run (create + confirm) vs
    /// unlock in the interactive prompt: false on a migrated-but-empty Project DB, true once any
    /// vault-key slot exists (what the first `open_or_init` enrolls — ADR 0008 §5; migration #10
    /// forward-copies a pre-slot `secret_meta` wrap into slot #1, so old stores read the same way).
    #[test]
    fn store_initialized_reflects_the_vault_key_slots() {
        use qfs_store::{MemorySource, ProjectDb};
        let conn = ProjectDb::open(&MemorySource)
            .unwrap()
            .into_db()
            .into_connection();
        assert!(
            !store_initialized(&conn),
            "a fresh store is not initialized"
        );

        // The first real open enrolls the passphrase slot; emulate exactly that row (bytes are
        // opaque to the presence check) and confirm the flag flips.
        conn.execute(
            "INSERT INTO vault_key_slot (guardian_kind, wrapped_dek, kdf_salt) \
             VALUES ('passphrase', ?1, ?2)",
            rusqlite::params![vec![0u8; 4], vec![0u8; 4]],
        )
        .unwrap();
        assert!(
            store_initialized(&conn),
            "an established store is initialized"
        );
    }

    /// t79: a rotation and a revocation each append a t76 audit row carrying selectors + metadata
    /// ONLY (the verb, the `<driver>/<connection>` selector, the `/sys/connections` surface) — never
    /// a secret. Exercised over an explicit System DB (hermetic) using the exact event the handlers
    /// emit (`connection_audit_event` + `append_event`).
    #[test]
    fn rotation_and_revocation_append_an_audit_row() {
        use qfs_store::{FileSource, SystemDb};
        let dir = tempfile::tempdir().unwrap();
        let sys = SystemDb::open(&FileSource::new(dir.path().join("system.db"))).unwrap();

        crate::audit::append_event(&sys, connection_audit_event("ROTATE", "github/team")).unwrap();
        crate::audit::append_event(&sys, connection_audit_event("REVOKE", "github/leaver"))
            .unwrap();

        let tail = crate::audit::recent_tail(&sys).unwrap();
        assert_eq!(tail.len(), 2, "both events landed on the chain");
        assert_eq!(tail[0].event.verb, "ROTATE");
        assert_eq!(tail[0].event.connection, "github/team");
        assert_eq!(tail[0].event.path, "/sys/connections");
        assert_eq!(tail[1].event.verb, "REVOKE");
        assert_eq!(tail[1].event.connection, "github/leaver");
        // The chain links the two events (the second's prev is the first's hash).
        assert_eq!(tail[1].prev_hash, tail[0].hash);
        // The audit rows carry no secret material — selectors + metadata only.
        let dump = format!("{tail:?}");
        assert!(!dump.contains("ghp_") && !dump.contains("token"));
    }

    /// Plant a cf account token into the vault so the cloud-account guard
    /// (`require_account_for_cloud_connect`, from origin/main) is satisfied and the connect proceeds
    /// to account-id auto-discovery. The planted value is the leak canary asserted absent from
    /// rendered output and errors.
    fn plant_cf_credential(account: &str) {
        use qfs_secrets::{ConnectionId, CredentialKey, DriverId, Secret, Secrets};
        let store = open_store().unwrap();
        let key = CredentialKey::new(
            DriverId("cf".to_string()),
            ConnectionId::new(account).unwrap(),
        );
        store.put(&key, Secret::from(PLANTED_CF_TOKEN)).unwrap();
    }

    #[test]
    fn cf_connect_list_renders_metadata_without_credentials() {
        let credential_value = time::OffsetDateTime::now_utc()
            .unix_timestamp_nanos()
            .to_string();
        let row = crate::path_binding::PathBindingRow {
            path: "/cf".to_string(),
            driver_id: Some("cf".to_string()),
            at_locator: Some("cloudflare-account".to_string()),
            secret_ref: Some("vault:cf/mycf".to_string()),
            alias_of: None,
            host: "local".to_string(),
            account: Some("mycf".to_string()),
            app: None,
            created_at: String::new(),
        };

        let rendered = render_path_bindings(&[row]);

        assert!(rendered.contains("/cf\tcf\tat cloudflare-account"));
        assert!(rendered.contains("\taccount mycf"));
        assert!(rendered.contains("vault:cf/mycf"));
        assert!(!rendered.contains(&credential_value));
    }

    #[test]
    fn cf_connect_without_at_persists_single_discovered_account_id() {
        let _home = crate::testenv::HomeGuard::with_passphrase("cf-connect-single");
        plant_cf_credential("mycf");
        let backend = Arc::new(MockCfBackend::new().with_account("acct-one", "Production"));

        let msg = run_connect_with_cf_resolver(
            "/cf",
            Some("cf"),
            None,
            None,
            None,
            None,
            Some("mycf"),
            None,
            resolve_from_mock(backend.clone()),
        )
        .expect("single-account connect should succeed");

        assert_eq!(msg, "connected /cf -> cf (mycf)");
        let rows = crate::path_binding::db_list_bindings(&open_project_conn().unwrap()).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].path, "/cf");
        assert_eq!(rows[0].at_locator.as_deref(), Some("acct-one"));
        assert_eq!(rows[0].account.as_deref(), Some("mycf"));
        let rendered = render_path_bindings(&rows);
        assert!(rendered.contains("/cf\tcf\tat acct-one\taccount mycf"));
        assert!(!rendered.contains(PLANTED_CF_TOKEN));
        assert_eq!(backend.recorded(), vec![RecordedCall::AccountDiscovery]);
    }

    #[test]
    fn cf_connect_without_at_fails_closed_for_multi_account_token() {
        let _home = crate::testenv::HomeGuard::with_passphrase("cf-connect-multi");
        plant_cf_credential("mycf");
        let backend = Arc::new(
            MockCfBackend::new()
                .with_account("acct-one", "Production")
                .with_account("acct-two", "Staging"),
        );

        let err = run_connect_with_cf_resolver(
            "/cf",
            Some("cf"),
            None,
            None,
            None,
            None,
            Some("mycf"),
            None,
            resolve_from_mock(backend.clone()),
        )
        .unwrap_err();

        assert!(err.contains("multiple accounts"), "{err}");
        assert!(err.contains("acct-one (Production)"), "{err}");
        assert!(err.contains("acct-two (Staging)"), "{err}");
        assert!(err.contains("pass `--at <id>`"), "{err}");
        assert!(!err.contains(PLANTED_CF_TOKEN), "{err}");
        let rows = crate::path_binding::db_list_bindings(&open_project_conn().unwrap()).unwrap();
        assert!(rows.is_empty(), "ambiguous connect must not write binding");
        assert_eq!(backend.recorded(), vec![RecordedCall::AccountDiscovery]);
    }

    #[test]
    fn cf_connect_without_at_fails_closed_for_zero_visible_accounts() {
        let _home = crate::testenv::HomeGuard::with_passphrase("cf-connect-zero");
        plant_cf_credential("mycf");
        let backend = Arc::new(MockCfBackend::new());

        let err = run_connect_with_cf_resolver(
            "/cf",
            Some("cf"),
            None,
            None,
            None,
            None,
            Some("mycf"),
            None,
            resolve_from_mock(backend.clone()),
        )
        .unwrap_err();

        assert!(err.contains("can see no accounts"), "{err}");
        assert!(err.contains("pass `--at <id>`"), "{err}");
        assert!(!err.contains(PLANTED_CF_TOKEN), "{err}");
        let rows = crate::path_binding::db_list_bindings(&open_project_conn().unwrap()).unwrap();
        assert!(rows.is_empty(), "empty discovery must not write binding");
        assert_eq!(backend.recorded(), vec![RecordedCall::AccountDiscovery]);
    }

    #[test]
    fn cf_connect_with_explicit_at_does_not_probe_accounts() {
        let _home = crate::testenv::HomeGuard::with_passphrase("cf-connect-explicit-at");
        plant_cf_credential("mycf");
        let backend = Arc::new(MockCfBackend::new().with_account("unexpected", "Unexpected"));

        run_connect_with_cf_resolver(
            "/cf",
            Some("cf"),
            Some("explicit-account"),
            None,
            None,
            None,
            Some("mycf"),
            None,
            resolve_from_mock(backend.clone()),
        )
        .expect("explicit account id should keep existing offline behavior");

        let rows = crate::path_binding::db_list_bindings(&open_project_conn().unwrap()).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].at_locator.as_deref(), Some("explicit-account"));
        assert!(
            backend.recorded().is_empty(),
            "explicit --at must not call /accounts"
        );
    }

    #[test]
    fn cloud_connect_requires_existing_account_token() {
        let _home = crate::testenv::HomeGuard::with_passphrase("connect-account-test-pass");

        let err = run_connect(
            "/slack",
            Some("slack"),
            None,
            None,
            None,
            None,
            Some("team"),
            None,
        )
        .expect_err("missing Slack token must refuse the mount");
        assert!(
            err.contains("qfs account add slack team"),
            "error should tell the operator how to create the account first: {err}"
        );

        let conn = open_project_conn().unwrap();
        let rows = crate::path_binding::db_list_bindings(&conn).unwrap();
        assert!(
            rows.iter().all(|r| r.path != "/slack"),
            "failed cloud connect must not leave a path_binding row"
        );
    }

    #[test]
    fn local_connect_still_allows_no_account() {
        let _home = crate::testenv::HomeGuard::with_passphrase("connect-local-test-pass");

        let msg = run_connect(
            "/repo",
            Some("git"),
            Some("/tmp/repo"),
            None,
            None,
            None,
            None,
            None,
        )
        .expect("local git mounts need no qfs account");
        assert_eq!(msg, "connected /repo -> git");
    }
}
