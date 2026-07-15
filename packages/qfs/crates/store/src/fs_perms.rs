//! Owner-only (0600) permission discipline for the embedded credential-bearing DB files
//! (ticket 20260704170100).
//!
//! The Project/System DBs hold envelope-encrypted credentials, salts, and ciphertext. Created via a
//! bare `Connection::open`, the file would inherit the process **umask** and land typically
//! world/group-readable — a defense-in-depth gap (blueprint §8 secret hygiene / least-privilege): even
//! though the values are encrypted, a credential-bearing file readable by every local user leaks
//! metadata + salts + ciphertext and sets a bad precedent.
//!
//! [`ensure_owner_only`] closes that gap by CREATING the DB file at mode `0600` before the connection
//! opens (SQLite propagates the database file's mode + ownership to its `-wal` / `-shm` / journal
//! sidecars, so the whole set is owner-only), and by RE-CHECKING an existing file's permissions on
//! every open. A loose-but-**owned** file is **self-healed** — tightened to `0600` in place (ticket
//! 20260705015500), so a pre-v0.0.20 store created world/group-readable under the old umask heals on
//! the next open instead of bricking the CLI; it fails **closed** only when the tighten cannot be
//! done (a foreign-owned file, whose `chmod` fails with `EPERM`, or a filesystem that ignores the
//! mode). The guard only ever tightens, never loosens.
//!
//! This mirrors the legacy `qfs_secrets::LocalStore` owner-only pattern; it is shared here (a `pub`
//! leaf helper) so the terminal binary's time-boxed session-unlock cache (ticket 20260704170000)
//! reuses the same 0600-create discipline rather than hand-rolling permissions.
//!
//! Unix-only: on a non-POSIX host there is no 0600 notion, so both operations are a graceful no-op
//! (the at-rest AEAD envelope stays the confidentiality guard).

use std::path::Path;

use crate::StoreError;

/// Ensure the DB file at `path` is owner-only before it is opened.
///
/// - **Absent:** create it empty at mode `0600`, so the subsequent `Connection::open` uses that file
///   (and SQLite copies the mode to the `-wal`/`-shm`/journal sidecars it creates beside it).
/// - **Present:** verify it is not group/other-accessible; a credential-bearing DB that is
///   world/group-readable is **rejected** (fail closed) with a structured, value-free error naming
///   the remedy — never silently used.
///
/// Unix-only; a no-op on non-POSIX hosts.
///
/// # Errors
/// [`StoreError::Open`] if an existing file is group/other-accessible, or on a stat/create failure.
#[cfg(unix)]
pub fn ensure_owner_only(path: &Path) -> Result<(), StoreError> {
    use std::os::unix::fs::OpenOptionsExt;
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
    {
        // Created fresh at 0600: the opener will reuse this owner-only file.
        Ok(_) => Ok(()),
        // Already exists (possibly created by a concurrent open): verify its perms instead.
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => verify_owner_only(path),
        Err(e) => Err(StoreError::Open {
            detail: format!("creating the owner-only DB file {}: {e}", path.display()),
        }),
    }
}

/// Ensure an existing DB file is owner-only (no group/other permission bits), **self-healing** a
/// loose-but-owned file to `0600` on open rather than refusing (ticket 20260705015500).
///
/// - **Already owner-only** → Ok (the common reopen path).
/// - **Loose bits, owned by us** → `chmod 0600` (tighten) and continue. Tightening our own
///   credential DB is strictly safe and is the documented remedy, so a pre-v0.0.20 file created
///   world/group-readable under the old umask heals silently on the next open instead of bricking
///   the CLI.
/// - **Loose bits, NOT ours** → refuse. `set_permissions` fails with `EPERM` for a file we do not
///   own (only its owner or root may chmod), so the failure IS the refusal — a foreign-owned
///   credential DB is surfaced, never silently tightened, without an explicit uid syscall.
/// - **chmod did not take** (a mode-ignoring filesystem) → re-verify and refuse if still
///   group/other-accessible, naming the manual `chmod 600` remedy.
///
/// The guard only ever **tightens**; it never loosens. Errors are value-free — path (infrastructure,
/// not a secret) + mode, never DB contents (blueprint §8).
///
/// # Errors
/// [`StoreError::Open`] if a loose file cannot be tightened to owner-only (foreign owner / chmod
/// refused / the tighten did not take), or on a stat failure.
#[cfg(unix)]
pub fn verify_owner_only(path: &Path) -> Result<(), StoreError> {
    use std::os::unix::fs::PermissionsExt;
    let meta = std::fs::metadata(path).map_err(|e| StoreError::Open {
        detail: format!("stat DB file {}: {e}", path.display()),
    })?;
    if meta.permissions().mode() & 0o077 == 0 {
        return Ok(()); // already owner-only — the reopen fast path.
    }
    // Loose bits: self-heal by tightening to 0600. `set_permissions` succeeds only for a file we own
    // (or as root); a foreign-owned credential DB fails here with EPERM — the correct refusal.
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).map_err(|e| {
        StoreError::Open {
            detail: format!(
                "credential DB {} is group/other-accessible and could not be tightened to \
                 owner-only 0600 ({e}); run `chmod 600 {}` — or investigate if it is not yours",
                path.display(),
                path.display()
            ),
        }
    })?;
    // Re-verify the tighten actually landed (a mode-ignoring filesystem would leave it loose).
    let mode = std::fs::metadata(path)
        .map_err(|e| StoreError::Open {
            detail: format!("re-stat DB file {}: {e}", path.display()),
        })?
        .permissions()
        .mode();
    if mode & 0o077 != 0 {
        return Err(StoreError::Open {
            detail: format!(
                "credential DB {} stayed group/other-accessible (mode {:o}) after chmod; refusing \
                 to use it — run `chmod 600 {}` to restore owner-only access",
                path.display(),
                mode & 0o777,
                path.display()
            ),
        });
    }
    Ok(())
}

/// Non-POSIX hosts have no 0600 notion; the AEAD envelope is the confidentiality guard. No-op.
#[cfg(not(unix))]
pub fn ensure_owner_only(_path: &Path) -> Result<(), StoreError> {
    Ok(())
}

/// Non-POSIX hosts have no 0600 notion; the AEAD envelope is the confidentiality guard. No-op.
#[cfg(not(unix))]
pub fn verify_owner_only(_path: &Path) -> Result<(), StoreError> {
    Ok(())
}

/// Write `bytes` to `path` as an owner-only (`0600`) file, truncating any existing content and
/// re-asserting the mode (the binary's time-boxed session-unlock cache, ticket 20260704170000,
/// reuses this so it never hand-rolls permissions). `.mode()` only applies when the file is created,
/// so a pre-existing file (e.g. a loose-perms leftover) is explicitly `chmod`ed back to `0600` before
/// the write lands. Unix-only; on non-POSIX the AEAD wrap is the guard.
///
/// # Errors
/// [`StoreError::Open`] on an open/chmod/write/sync failure (secret-free message).
#[cfg(unix)]
pub fn write_owner_only(path: &Path, bytes: &[u8]) -> Result<(), StoreError> {
    use std::io::Write;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
        .map_err(|e| StoreError::Open {
            detail: format!("opening owner-only file {}: {e}", path.display()),
        })?;
    f.set_permissions(std::fs::Permissions::from_mode(0o600))
        .map_err(|e| StoreError::Open {
            detail: format!("chmod owner-only file {}: {e}", path.display()),
        })?;
    f.write_all(bytes).map_err(|e| StoreError::Open {
        detail: format!("writing owner-only file {}: {e}", path.display()),
    })?;
    f.sync_all().map_err(|e| StoreError::Open {
        detail: format!("fsync owner-only file {}: {e}", path.display()),
    })?;
    Ok(())
}

/// Non-POSIX hosts have no 0600 notion; the AEAD envelope is the confidentiality guard. Plain write.
#[cfg(not(unix))]
pub fn write_owner_only(path: &Path, bytes: &[u8]) -> Result<(), StoreError> {
    std::fs::write(path, bytes).map_err(|e| StoreError::Open {
        detail: format!("writing file {}: {e}", path.display()),
    })
}

#[cfg(all(test, unix))]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn mode_of(path: &Path) -> u32 {
        std::fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    /// `ensure_owner_only` creates the DB file at EXACTLY mode 0600. We assert the exact mode rather
    /// than mutating the process umask (a global that would race the parallel test runner): an
    /// inherited create under any normal umask (0o022 → 0644, 0o027 → 0640) would NOT equal 0o600, so
    /// an exact match proves the 0600 is explicit, not umask-inherited.
    #[test]
    fn create_makes_the_file_exactly_0600() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("project.db");
        assert!(!path.exists());
        ensure_owner_only(&path).unwrap();
        assert!(path.exists(), "the file is created");
        assert_eq!(
            mode_of(&path),
            0o600,
            "created owner-only, not umask-inherited"
        );
    }

    /// An existing group- or world-accessible DB that WE own is **self-healed**: tightened to 0600
    /// on open and accepted (ticket 20260705015500) — a pre-v0.0.20 644 store heals rather than
    /// bricking the CLI, since tightening our own credential DB is safe and is the documented remedy.
    #[test]
    fn an_existing_loose_owned_file_is_tightened_to_0600() {
        let dir = tempfile::tempdir().unwrap();
        for loose in [0o644, 0o640, 0o604, 0o666] {
            let path = dir.path().join(format!("loose_{loose:o}.db"));
            std::fs::write(&path, b"seed").unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(loose)).unwrap();
            ensure_owner_only(&path).unwrap(); // self-heals, no error
            assert_eq!(mode_of(&path), 0o600, "loose {loose:o} tightened to 0600");
            // Idempotent: a second open of the now-0600 file stays Ok.
            ensure_owner_only(&path).unwrap();
            // The tighten preserved the file contents (chmod, not recreate).
            assert_eq!(std::fs::read(&path).unwrap(), b"seed");
        }
    }

    /// An existing already-0600 file passes the re-check idempotently (a reopen of our own DB).
    #[test]
    fn an_existing_owner_only_file_is_accepted() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("project.db");
        ensure_owner_only(&path).unwrap();
        // Second call verifies the existing 0600 file and stays Ok (the reopen path).
        ensure_owner_only(&path).unwrap();
        assert_eq!(mode_of(&path), 0o600);
    }
}
