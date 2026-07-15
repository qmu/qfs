//! The **binary I/O for the time-boxed session-unlock cache** (ticket 20260704170000): enter the
//! passphrase once, skip the re-prompt for a bounded window on repeated one-shot `qfs run`s.
//!
//! The cross-invocation seam the in-process [`crate::connection`] cache (`PROMPTED_PASSPHRASE`) can
//! never reach: it dies with the process, so a fresh command (or a new tmux pane) re-prompts. The OS
//! keychain guardian answers that on a desktop, but this host is a headless EC2 with no secret
//! service — so this cache is the headless, TIME-BOXED sibling (ssh-agent / `sudo`-timestamp style).
//!
//! ## What is cached, and how it is protected
//! The store **DEK** (never the passphrase — the argon2id derivation is the expensive step, and the
//! DEK is what actually opens sealed values) is AEAD-wrapped under a short-lived, machine/session-
//! bound key and written to a `0600` file beside the Project DB, with a typed `expires_at`. On a
//! later invocation the binary re-derives that key from the same machine facts and unwraps the DEK,
//! opening the store through the injected-slot seam ([`SqliteSecrets::open_with_slot`]) — no DB slot
//! is enrolled, so nothing accumulates.
//!
//! The pure record shape + expiry/owner DECISION live in [`qfs_secrets::session`]; this module owns
//! only the I/O it needs (the file, the clock, the machine facts, the uid).
//!
//! ## Threat model (honest)
//! The binding key is derived (fast SHA-256, not argon2 — the inputs are high-entropy machine facts +
//! a CSPRNG salt, so no password stretch is warranted, and this runs on every credentialed command)
//! from `(salt, uid, expires_at, machine-id, boot-id)`. Folding `uid` + `expires_at` into the key
//! AUTHENTICATES them: tampering the plaintext deadline (or uid) to extend the window changes the key
//! and the unwrap fails — fail closed. `boot-id` ties the session to the current boot, so a REBOOT
//! silently invalidates it (a re-auth, beyond the TTL). Machine-id/uid make the file non-replayable
//! to another machine or user. What the key does NOT provide is CONFIDENTIALITY against a local
//! attacker who can already read the `0600` file (machine-id/boot-id are world-readable) — that rests
//! on the file mode, exactly like `sudo`'s timestamp. The AEAD wrap still keeps the raw DEK off disk
//! and detects tampering. Anything not present-parseable-current-unexpired-and-openable is treated as
//! ABSENT: the binary falls through the resolution ladder and never silently unlocks.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use qfs_secrets::{classify_session, SessionRecord, SessionState, SlotWrap};

use crate::secret_store::SqliteSecrets;

/// The synthetic guardian kind for the injected session slot — never persisted in `vault_key_slot`
/// (the cache is a file, not a DB slot). Distinct from `passphrase` / `keychain`.
const GUARDIAN_SESSION: &str = "session";

/// The default session TTL: a work-day. Overridable via `QFS_SESSION_TTL` (see [`resolved_ttl_secs`]).
const DEFAULT_TTL_SECS: i64 = 8 * 60 * 60;
/// TTL clamp floor/ceiling (1 minute .. 7 days) — a garbled override can neither disable the cache
/// nor extend it unreasonably.
const MIN_TTL_SECS: i64 = 60;
const MAX_TTL_SECS: i64 = 7 * 24 * 60 * 60;

/// Set true the moment [`crate::connection::resolve_store_passphrase`] prompts INTERACTIVELY (not the
/// env-var / already-cached paths) — the one signal that authorizes minting a persistent session
/// cache. [`maybe_mint_session`] consumes it (swap to false) so exactly one mint happens per fresh
/// prompt. An env-var (`QFS_PASSPHRASE`) unlock never sets it, so it never silently mints.
pub static JUST_PROMPTED: AtomicBool = AtomicBool::new(false);

/// The current effective uid — the session is bound to (and re-checked against) this OS user.
#[cfg(unix)]
fn current_uid() -> u32 {
    rustix::process::geteuid().as_raw()
}
#[cfg(not(unix))]
fn current_uid() -> u32 {
    0
}

/// The current wall clock as a Unix epoch in **seconds** (UTC). Mirrors `commit::now_rfc3339`'s clock.
fn now_epoch() -> i64 {
    time::OffsetDateTime::now_utc().unix_timestamp()
}

/// The default session-unlock file path: `<config>/qfs/session.unlock`, beside `project.db` (mirrors
/// [`crate::store::default_project_db_path`]). `None` when no config home resolves.
fn session_unlock_path() -> Option<std::path::PathBuf> {
    crate::store::default_session_unlock_path()
}

/// Resolve the TTL in seconds: `QFS_SESSION_TTL` (a bare seconds count, or a `30m`/`8h`/`2d` duration)
/// clamped to `[MIN_TTL_SECS, MAX_TTL_SECS]`, else the 8-hour default. A garbled value falls back to
/// the default rather than disabling the cache.
fn resolved_ttl_secs() -> i64 {
    match std::env::var("QFS_SESSION_TTL") {
        Ok(v) if !v.trim().is_empty() => parse_duration_secs(v.trim())
            .unwrap_or(DEFAULT_TTL_SECS)
            .clamp(MIN_TTL_SECS, MAX_TTL_SECS),
        _ => DEFAULT_TTL_SECS,
    }
}

/// Parse `"3600"` / `"30m"` / `"8h"` / `"2d"` to seconds. `None` on garbage.
fn parse_duration_secs(s: &str) -> Option<i64> {
    let s = s.trim();
    let (num, mult): (&str, i64) = match s.chars().last() {
        Some('s') => (&s[..s.len() - 1], 1),
        Some('m') => (&s[..s.len() - 1], 60),
        Some('h') => (&s[..s.len() - 1], 60 * 60),
        Some('d') => (&s[..s.len() - 1], 24 * 60 * 60),
        Some(c) if c.is_ascii_digit() => (s, 1),
        _ => return None,
    };
    let n: i64 = num.trim().parse().ok()?;
    if n <= 0 {
        return None;
    }
    n.checked_mul(mult)
}

/// The non-secret machine facts the binding key folds in: `/etc/machine-id` (host identity, stable)
/// and the boot id (`/proc/.../boot_id`, changes per boot so a reboot invalidates the session).
/// Missing files degrade to empty — the binding then rests on uid + salt + `0600` (the macOS/other
/// case, where the keychain guardian is the real answer anyway).
fn machine_facts() -> (Vec<u8>, Vec<u8>) {
    let machine_id = std::fs::read("/etc/machine-id")
        .or_else(|_| std::fs::read("/var/lib/dbus/machine-id"))
        .unwrap_or_default();
    let boot_id = std::fs::read("/proc/sys/kernel/random/boot_id").unwrap_or_default();
    (machine_id, boot_id)
}

/// Derive the 32-byte machine/session binding KEK from `(salt, uid, expires_at, machine-id, boot-id)`
/// with a single SHA-256 (fast, per-command). Folding `uid` + `expires_at` in AUTHENTICATES those
/// plaintext record fields — a tampered deadline or uid derives a different KEK and the unwrap fails.
fn derive_session_kek(salt: &[u8], uid: u32, expires_at: i64) -> [u8; 32] {
    let (machine_id, boot_id) = machine_facts();
    let mut input = Vec::with_capacity(64 + machine_id.len() + boot_id.len());
    input.extend_from_slice(b"qfs-session-unlock-v1\0");
    input.extend_from_slice(salt);
    input.extend_from_slice(&uid.to_le_bytes());
    input.extend_from_slice(&expires_at.to_le_bytes());
    input.push(0);
    input.extend_from_slice(&machine_id);
    input.push(0);
    input.extend_from_slice(&boot_id);
    qfs_crypto_core::sha256(&input)
}

/// Read the session file's bytes ONLY when it is present, `0600`, and owned by the current uid;
/// otherwise `None` (fail closed — a loose-perms or wrong-owner file reads as absent, so the caller
/// falls through to the prompt). The mode/owner check is the file-side companion to the record's
/// in-band uid + AEAD binding.
fn read_owner_only_bytes(path: &Path) -> Option<Vec<u8>> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let meta = std::fs::metadata(path).ok()?;
        if meta.permissions().mode() & 0o077 != 0 {
            return None;
        }
        if meta.uid() != current_uid() {
            return None;
        }
    }
    std::fs::read(path).ok()
}

/// Best-effort purge of the session file (on lock / expiry / tamper). A failure to remove is ignored
/// — the next consult re-classifies it and a wrong/expired file never unlocks anyway.
fn purge(path: &Path) {
    let _ = std::fs::remove_file(path);
}

/// The public purge: drop the current session so the very next command re-prompts (`qfs auth --lock`).
/// Returns whether a file was present to remove.
#[must_use]
pub fn purge_session() -> bool {
    let Some(path) = session_unlock_path() else {
        return false;
    };
    let existed = path.exists();
    purge(&path);
    if existed {
        // Secret-free lifecycle audit (ticket 20260704170000 Quality Gate 4): the session cache was
        // dropped (an explicit `qfs auth --lock`, or an open-fail eviction). Metadata only — no key bytes.
        crate::connection::emit_connection_audit("SESSION_PURGE", "vault");
    }
    existed
}

/// CONSULT the session cache: read + classify the file at `path` against `now`/`uid`, PURGE a stale
/// (expired/corrupt) file, and — for a valid unexpired session THIS uid minted — return the injected
/// [`SlotWrap`] + the re-derived machine KEK the caller unwraps the DEK with. `None` on
/// absent/expired/corrupt/loose-perms. Never prompts, never errors (best-effort, like keychain).
fn consult(path: &Path, now: i64, uid: u32) -> Option<(SlotWrap, [u8; 32])> {
    let bytes = read_owner_only_bytes(path);
    let state = classify_session(bytes.as_deref(), now, uid);
    if state.should_purge() {
        purge(path);
    }
    let SessionState::Valid(record) = state else {
        return None;
    };
    // Re-derive the KEK from the RECORD's (authenticated) fields; a tampered field yields a wrong KEK
    // and the caller's unwrap fails closed.
    let kek = derive_session_kek(&record.salt, record.owner_uid, record.expires_at);
    let slot = SlotWrap {
        slot_id: 0,
        guardian_kind: GUARDIAN_SESSION.to_string(),
        wrapped_dek: record.wrapped_dek,
        kdf_salt: None,
    };
    Some((slot, kek))
}

/// MINT a session file at `path` from an unlocked `store`: wrap the DEK under a fresh machine KEK
/// bound to `(salt, uid, expires_at)`, and write the `0600` record. Best-effort — a failure to derive
/// the wrap or write the file is swallowed (the session is a convenience; the command already
/// succeeded). Returns the deadline on success (for the caller's message), `None` on any failure.
fn mint(path: &Path, store: &SqliteSecrets, ttl: i64, now: i64, uid: u32) -> Option<i64> {
    let expires_at = now.saturating_add(ttl);
    let salt = qfs_secrets::generate_salt();
    let kek = derive_session_kek(&salt, uid, expires_at);
    let wrapped_dek = store.session_wrap(&kek).ok()?;
    let record = SessionRecord {
        owner_uid: uid,
        expires_at,
        salt,
        wrapped_dek,
    };
    qfs_store::fs_perms::write_owner_only(path, &record.to_bytes()).ok()?;
    Some(expires_at)
}

/// The session cache's injected slot + machine KEK IF a valid unexpired session exists for this user
/// on this machine, else `None`. Consulted by [`crate::connection::open_store`] /
/// `open_store_for_commit` BEFORE the passphrase fallback, exactly mirroring the keychain branch.
/// Purges a stale file as a side effect (so the next command re-prompts).
#[must_use]
pub fn session_unlock_material() -> Option<(SlotWrap, [u8; 32])> {
    let path = session_unlock_path()?;
    consult(&path, now_epoch(), current_uid())
}

/// Mint a session IFF an interactive prompt just happened this process ([`JUST_PROMPTED`]) — the
/// gate that keeps a `QFS_PASSPHRASE`-env unlock from silently persisting a cache. Consumes the flag
/// (swap to false) so exactly one mint happens per fresh prompt. Best-effort; never fails the caller.
pub fn maybe_mint_session(store: &SqliteSecrets) {
    if !JUST_PROMPTED.swap(false, Ordering::SeqCst) {
        return;
    }
    let Some(path) = session_unlock_path() else {
        return;
    };
    if mint(
        &path,
        store,
        resolved_ttl_secs(),
        now_epoch(),
        current_uid(),
    )
    .is_some()
    {
        // Secret-free lifecycle audit (ticket 20260704170000 Quality Gate 4): a time-boxed session
        // was minted. The event carries the verb + a "vault" selector only — never any key material.
        crate::connection::emit_connection_audit("SESSION_MINT", "vault");
    }
}

/// Force-mint a session from an unlocked `store`, IGNORING [`JUST_PROMPTED`] — the explicit
/// `qfs auth` path (ticket 20260706145610). Unlike [`maybe_mint_session`], this mints even
/// when the store was unlocked NON-interactively (an enrolled keychain slot, a still-live session,
/// or `QFS_PASSPHRASE`): an explicit `unlock` invocation IS the operator's intentional "warm it now"
/// signal, distinct from the implicit store-opens `JUST_PROMPTED` guards against silently persisting
/// an env-var unlock. Best-effort; never fails the caller. Returns the deadline on success (for the
/// message), `None` when no session dir resolves or the wrap/write fails.
#[must_use]
pub fn force_mint_session(store: &SqliteSecrets) -> Option<i64> {
    let path = session_unlock_path()?;
    let out = mint(
        &path,
        store,
        resolved_ttl_secs(),
        now_epoch(),
        current_uid(),
    );
    if out.is_some() {
        // Same secret-free lifecycle audit as maybe_mint_session — metadata only, never key bytes.
        crate::connection::emit_connection_audit("SESSION_MINT", "vault");
    }
    out
}

/// A human-readable one-line status of the session cache for `qfs vault slots` — presence + remaining
/// TTL, never any key material. `None` when no valid session is cached (so the caller prints nothing).
#[must_use]
pub fn status_line() -> Option<String> {
    let path = session_unlock_path()?;
    let now = now_epoch();
    let uid = current_uid();
    let bytes = read_owner_only_bytes(&path);
    match classify_session(bytes.as_deref(), now, uid) {
        SessionState::Valid(record) => {
            let remaining = record.expires_at.saturating_sub(now).max(0);
            Some(format!(
                "session\ttime-boxed unlock cached\texpires in {}",
                humanize_secs(remaining)
            ))
        }
        _ => None,
    }
}

/// Render a seconds count as a compact `Nh Nm` / `Nm` / `Ns` string (display only).
fn humanize_secs(s: i64) -> String {
    let h = s / 3600;
    let m = (s % 3600) / 60;
    let sec = s % 60;
    if h > 0 {
        format!("{h}h {m}m")
    } else if m > 0 {
        format!("{m}m")
    } else {
        format!("{sec}s")
    }
}

#[cfg(all(test, unix))]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    // `Secrets` is the trait carrying `get`/`put`; it must be in scope to call them on the store.
    use qfs_secrets::{Secret, Secrets};
    use qfs_store::{FileSource, ProjectDb};
    use std::os::unix::fs::PermissionsExt;

    fn file_project_conn(path: &Path) -> rusqlite::Connection {
        ProjectDb::open(&FileSource::new(path))
            .unwrap()
            .into_db()
            .into_connection()
    }

    fn ckey(driver: &str, conn: &str) -> qfs_secrets::CredentialKey {
        qfs_secrets::CredentialKey::new(
            qfs_secrets::DriverId::new(driver),
            qfs_secrets::ConnectionId::new(conn).unwrap(),
        )
    }

    /// The whole cache round-trips: mint from an unlocked store, then a fresh open through the
    /// consulted (slot, kek) unlocks the SAME store and decrypts the stored secret.
    #[test]
    fn mint_then_consult_unlocks_the_same_store_within_ttl() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("project.db");
        let sess = dir.path().join("session.unlock");
        let uid = current_uid();

        // Unlock the store, store a secret, mint a session from it.
        let store =
            SqliteSecrets::open_or_init(file_project_conn(&db), &Secret::from("pw")).unwrap();
        store
            .put(&ckey("gh", "main"), Secret::from("ghp_cached"))
            .unwrap();
        assert!(mint(&sess, &store, 3600, 1_000, uid).is_some());
        drop(store);

        // The session file is 0600.
        let mode = std::fs::metadata(&sess).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "session file is owner-only, got {mode:o}");

        // Consult within the TTL: a fresh store opened through the injected slot decrypts the secret.
        let (slot, kek) = consult(&sess, 1_100, uid).expect("valid session");
        let reopened = SqliteSecrets::open_with_slot(file_project_conn(&db), &slot, kek).unwrap();
        assert_eq!(
            reopened.get(&ckey("gh", "main")).unwrap().expose_str(),
            Some("ghp_cached")
        );
    }

    /// The on-disk record contains neither the passphrase nor the raw DEK bytes.
    #[test]
    fn on_disk_record_leaks_no_plaintext_key_material() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("project.db");
        let sess = dir.path().join("session.unlock");
        let store =
            SqliteSecrets::open_or_init(file_project_conn(&db), &Secret::from("pw")).unwrap();
        mint(&sess, &store, 3600, 1_000, current_uid()).unwrap();
        let raw = std::fs::read(&sess).unwrap();
        assert!(
            !raw.windows(2).any(|w| w == b"pw"),
            "passphrase must not be on disk"
        );
        // The DEK lives only behind the AEAD wrap: re-deriving the KEK from the record's fields opens
        // the store, proving the wrap is valid — yet the raw DEK bytes are never in the clear on disk.
        let record = SessionRecord::from_bytes(&raw).unwrap();
        let kek = derive_session_kek(&record.salt, current_uid(), record.expires_at);
        SqliteSecrets::open_with_slot(
            file_project_conn(&db),
            &SlotWrap {
                slot_id: 0,
                guardian_kind: GUARDIAN_SESSION.into(),
                wrapped_dek: record.wrapped_dek,
                kdf_salt: None,
            },
            kek,
        )
        .expect("the session wrap opens the store");
    }

    /// Past the TTL, consult returns None AND purges the stale file (next command re-prompts).
    #[test]
    fn expired_session_is_rejected_and_purged() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("project.db");
        let sess = dir.path().join("session.unlock");
        let store =
            SqliteSecrets::open_or_init(file_project_conn(&db), &Secret::from("pw")).unwrap();
        mint(&sess, &store, 3600, 1_000, current_uid()).unwrap();
        assert!(sess.exists());
        // now == expires_at (1000 + 3600): expired, rejected, purged.
        assert!(consult(&sess, 4_600, current_uid()).is_none());
        assert!(!sess.exists(), "an expired session file is purged");
    }

    /// A session minted for another uid is rejected (corrupt) and purged — a copied file cannot unlock.
    #[test]
    fn wrong_uid_session_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("project.db");
        let sess = dir.path().join("session.unlock");
        let store =
            SqliteSecrets::open_or_init(file_project_conn(&db), &Secret::from("pw")).unwrap();
        mint(&sess, &store, 3600, 1_000, 4242).unwrap();
        // Consult as a DIFFERENT uid: classify sees the owner mismatch -> None.
        assert!(consult(&sess, 1_100, 9999).is_none());
    }

    /// A tampered record (a flipped deadline byte to extend the window) fails closed: the KEK is bound
    /// to the deadline, so the caller's unwrap fails even though the file still "parses".
    #[test]
    fn tampered_deadline_fails_to_unlock() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("project.db");
        let sess = dir.path().join("session.unlock");
        let uid = current_uid();
        let store =
            SqliteSecrets::open_or_init(file_project_conn(&db), &Secret::from("pw")).unwrap();
        mint(&sess, &store, 3600, 1_000, uid).unwrap();

        // Flip a byte in the expires_at field (offset 8 magic + 4 uid = 12), extending the window.
        let mut raw = std::fs::read(&sess).unwrap();
        raw[12] ^= 0x40;
        std::fs::write(&sess, &raw).unwrap();
        std::fs::set_permissions(&sess, std::fs::Permissions::from_mode(0o600)).unwrap();

        // consult may still classify it Valid (a later deadline), but the KEK is bound to the
        // tampered deadline, so the unwrap fails closed.
        if let Some((slot, kek)) = consult(&sess, 1_100, uid) {
            assert!(
                SqliteSecrets::open_with_slot(file_project_conn(&db), &slot, kek).is_err(),
                "a tampered deadline must not unlock (KEK binds the deadline)"
            );
        }
    }

    /// A loose-perms (0644) session file reads as absent (fail closed) — never used.
    #[test]
    fn loose_perms_session_file_is_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("project.db");
        let sess = dir.path().join("session.unlock");
        let store =
            SqliteSecrets::open_or_init(file_project_conn(&db), &Secret::from("pw")).unwrap();
        mint(&sess, &store, 3600, 1_000, current_uid()).unwrap();
        std::fs::set_permissions(&sess, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(
            consult(&sess, 1_100, current_uid()).is_none(),
            "a group/other-readable session file must be ignored"
        );
    }

    #[test]
    fn ttl_parsing_handles_durations_and_clamps() {
        assert_eq!(parse_duration_secs("3600"), Some(3600));
        assert_eq!(parse_duration_secs("30m"), Some(1800));
        assert_eq!(parse_duration_secs("8h"), Some(28800));
        assert_eq!(parse_duration_secs("2d"), Some(172800));
        assert_eq!(parse_duration_secs("0"), None);
        assert_eq!(parse_duration_secs("abc"), None);
        assert_eq!(parse_duration_secs(""), None);
    }

    /// The `qfs auth` path (ticket 20260706145610): `force_mint_session` mints a live
    /// session even when [`JUST_PROMPTED`] is FALSE (a non-interactive unlock — env/keychain/live),
    /// exactly where `maybe_mint_session` deliberately mints nothing. Driven through the real path
    /// resolver over an isolated `XDG_CONFIG_HOME` (the crate-wide env lock via `HomeGuard`).
    #[test]
    fn force_mint_session_mints_without_the_prompt_flag() {
        let _home = crate::testenv::HomeGuard::new();
        // The resolver points at `<home>/qfs/…`; write_owner_only does not mkdir -p, so create it.
        let sess =
            crate::store::default_session_unlock_path().expect("xdg resolves the session path");
        std::fs::create_dir_all(sess.parent().unwrap()).unwrap();
        let db = crate::store::default_project_db_path().expect("xdg resolves the project db");
        let store =
            SqliteSecrets::open_or_init(file_project_conn(&db), &Secret::from("pw")).unwrap();

        // Gate OFF (the env/keychain/live-session case): maybe_mint_session must persist nothing.
        JUST_PROMPTED.store(false, Ordering::SeqCst);
        maybe_mint_session(&store);
        assert!(
            !sess.exists(),
            "maybe_mint_session must NOT mint while JUST_PROMPTED is false"
        );

        // The explicit unlock path force-mints regardless of the flag.
        let deadline = force_mint_session(&store).expect("force-mint writes a session");
        assert!(
            deadline > now_epoch(),
            "the minted deadline is in the future"
        );
        assert!(
            sess.exists(),
            "force_mint_session mints even with JUST_PROMPTED false"
        );
        // The just-minted session is live: it consults back and status_line reports its TTL.
        assert!(
            session_unlock_material().is_some(),
            "the forced session unlocks the store"
        );
        let line = status_line().expect("a live session renders a status line");
        assert!(
            line.contains("expires in"),
            "status line shows remaining TTL: {line}"
        );
    }

    #[test]
    fn kek_binds_uid_and_deadline_and_salt() {
        let base = derive_session_kek(&[1u8; 16], 1000, 5_000);
        assert_ne!(
            base,
            derive_session_kek(&[2u8; 16], 1000, 5_000),
            "salt binds"
        );
        assert_ne!(
            base,
            derive_session_kek(&[1u8; 16], 1001, 5_000),
            "uid binds"
        );
        assert_ne!(
            base,
            derive_session_kek(&[1u8; 16], 1000, 5_001),
            "deadline binds"
        );
        assert_eq!(
            base,
            derive_session_kek(&[1u8; 16], 1000, 5_000),
            "deterministic"
        );
    }
}
