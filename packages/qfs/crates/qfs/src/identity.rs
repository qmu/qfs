//! The `qfs identity` composition root (t45): the System-DB-backed identity store I/O behind
//! `qfs identity whoami`, injected into `qfs-cmd` as the [`qfs_cmd::IdentityLauncher`].
//! (Signing up moved to `qfs init` — ADR 0008 §2 retired the unverified-password signup;
//! see [`crate::init`].)
//!
//! `qfs-cmd` may not depend on the concrete `qfs-store` / `qfs-identity` backends (the dep_direction
//! guard), so — exactly like the connection launcher — the binary owns this and `qfs-cmd` only parses
//! the verb and calls in. The binary is also the one crate that resolves a real DB path (decision F).
//!
//! ## Scope + security (decision §4.1, blueprint §8)
//! - READ-BACK ONLY: identity is not authorization. Sessions (t46) have shipped but serve the
//!   web / OAuth face — no session rides a CLI invocation, so `whoami` resolves the sole local
//!   user (or a named email), never a session principal.
//! - [`read_password_from_stdin`] (kept for the t55 invite redeem, which DOES set a real
//!   password): STDIN or an echo-off TTY prompt, never argv; carried as a [`Secret`].
//! - `whoami` prints only the email + user id — **never** a password hash. The hash is never
//!   logged, never returned, never serialized into an audit event.

use std::io::Read;

use qfs_cmd::IdentityAction;
use qfs_identity::{IdentityStore, Secret, SoleUser};
use qfs_store::SqliteIdentityStore;

/// The injected identity launcher. Returns the process exit code (`0` ok, `1` on a structured,
/// secret-free error). Never panics.
#[must_use]
pub fn run_identity(action: &IdentityAction) -> i32 {
    match run_inner(action) {
        Ok(msg) => {
            println!("{msg}");
            0
        }
        Err(e) => {
            eprintln!("qfs: error: {e}");
            1
        }
    }
}

/// Open the System DB at the default path and build the identity store over its owned, migrated
/// connection (the t45 seam — `SystemDb::into_db().into_connection()`). The identity migration (v3)
/// is applied by `SystemDb::open`.
pub(crate) fn open_identity_store() -> Result<SqliteIdentityStore, String> {
    let sys = crate::store::open_system_db()
        .map_err(|e| format!("opening the system database: {e}"))?
        .ok_or("cannot determine the system database path (set HOME or XDG_CONFIG_HOME)")?;
    Ok(SqliteIdentityStore::from_db(sys.into_db()))
}

/// Read the password being SET as a [`Secret`] (never argv). A human at a terminal is PROMPTED
/// (echo off, confirmed twice so a typo can't lock them out); automation keeps the stdin path —
/// `printf %s "$PW" | …` — trimming a single trailing newline and rejecting an empty password.
/// Shared with the t55 invite-redeem launcher (which sets the redeemer's password the same way).
pub(crate) fn read_password_from_stdin() -> Result<Secret, String> {
    if crate::tty::stdin_is_terminal() {
        return crate::tty::prompt_secret_confirmed("Choose a password: ", "Confirm password: ");
    }
    let mut buf = String::new();
    std::io::stdin()
        .read_to_string(&mut buf)
        .map_err(|e| format!("reading the password from stdin: {e}"))?;
    let trimmed = buf.trim_end_matches(['\n', '\r']);
    if trimmed.is_empty() {
        return Err(
            "no password on stdin — pipe it, e.g. `printf %s \"$PW\" | qfs invite redeem <token>`"
                .into(),
        );
    }
    Ok(Secret::from(trimmed))
}

fn run_inner(action: &IdentityAction) -> Result<String, String> {
    match action {
        IdentityAction::Whoami { email, json } => {
            let store = open_identity_store()?;
            match email {
                // An explicit email: look that user up.
                Some(e) => match store
                    .find_user_by_email(e)
                    .map_err(|err| format!("looking up the user: {err}"))?
                {
                    Some(u) => Ok(whoami_render(
                        *json,
                        WhoamiOutcome::Found(&u.primary_email, u.id.0),
                    )),
                    None => Ok(whoami_render(*json, WhoamiOutcome::NotFound(e))),
                },
                // No email + no session (the CLI carries none): resolve the sole user, if exactly one.
                None => match store
                    .sole_user()
                    .map_err(|err| format!("resolving the current user: {err}"))?
                {
                    SoleUser::One(u) => Ok(whoami_render(
                        *json,
                        WhoamiOutcome::Found(&u.primary_email, u.id.0),
                    )),
                    SoleUser::None => Ok(whoami_render(*json, WhoamiOutcome::NoUsers)),
                    SoleUser::Many => Ok(whoami_render(*json, WhoamiOutcome::ManyUsers)),
                },
            }
        }
    }
}

/// The resolved shape of a `whoami` lookup, rendered as prose or credential-free JSON. NEVER a
/// password hash — the schema carries only the email handle + the numeric user id.
enum WhoamiOutcome<'a> {
    /// A concrete user (explicit-email hit, or the sole user): email + id.
    Found(&'a str, i64),
    /// An explicit email that resolves to no local user.
    NotFound(&'a str),
    /// No users exist on this host yet.
    NoUsers,
    /// More than one user and no session to disambiguate.
    ManyUsers,
}

/// Render a `whoami` outcome. Human mode keeps the historical prose; `--json` (the global flag,
/// threaded through [`IdentityAction::Whoami`]) emits a machine-readable, credential-free object so
/// the identity read-back is consumable without prose-parsing (mission Experience 5).
fn whoami_render(json: bool, outcome: WhoamiOutcome<'_>) -> String {
    if json {
        let value = match outcome {
            WhoamiOutcome::Found(email, id) => {
                serde_json::json!({ "user": { "email": email, "id": id } })
            }
            WhoamiOutcome::NotFound(email) => {
                serde_json::json!({ "user": null, "reason": "not_found", "email": email })
            }
            WhoamiOutcome::NoUsers => serde_json::json!({ "user": null, "reason": "no_users" }),
            WhoamiOutcome::ManyUsers => {
                serde_json::json!({ "user": null, "reason": "multiple_users" })
            }
        };
        return value.to_string();
    }
    match outcome {
        WhoamiOutcome::Found(email, id) => format!("{email} (user {id})"),
        WhoamiOutcome::NotFound(email) => format!("no user is signed up for {email}"),
        WhoamiOutcome::NoUsers => "no users yet — run `qfs init`".to_string(),
        WhoamiOutcome::ManyUsers => {
            "multiple users on this host and no session — specify `qfs identity whoami <email>`"
                .to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whoami_json_is_machine_readable_and_credential_free() {
        // `--json` emits a parseable object, never prose, and never a password-hash column.
        let out = whoami_render(true, WhoamiOutcome::Found("a@qmu.jp", 1));
        let v: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
        assert_eq!(v["user"]["email"], "a@qmu.jp");
        assert_eq!(v["user"]["id"], 1);
        assert!(v.get("password").is_none() && v.get("hash").is_none());

        // The not-signed-in / unresolved states are first-class in JSON: `user` is null with a reason.
        for (outcome, reason) in [
            (WhoamiOutcome::NoUsers, "no_users"),
            (WhoamiOutcome::ManyUsers, "multiple_users"),
        ] {
            let out = whoami_render(true, outcome);
            let v: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
            assert!(v["user"].is_null());
            assert_eq!(v["reason"], reason);
        }
    }

    #[test]
    fn whoami_human_mode_is_unchanged_prose() {
        assert_eq!(
            whoami_render(false, WhoamiOutcome::Found("a@qmu.jp", 1)),
            "a@qmu.jp (user 1)"
        );
        assert_eq!(
            whoami_render(false, WhoamiOutcome::NoUsers),
            "no users yet — run `qfs init`"
        );
    }
}
