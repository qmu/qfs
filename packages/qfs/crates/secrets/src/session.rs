//! [`session`](self) — the **pure time-boxed session-unlock record** (ticket 20260704170000).
//!
//! A cross-invocation cache that lets one interactive passphrase entry unlock the credential store
//! for a bounded window (ssh-agent / `sudo`-timestamp style), so a repeated one-shot `qfs run` on a
//! headless host (where the OS-keychain guardian is unavailable) does not re-prompt every time.
//!
//! **This module is pure** (no filesystem, no clock, no keyring — the same discipline as
//! [`crate::slots`]): it owns only the on-disk RECORD shape and the EXPIRY / OWNER DECISION. The
//! binary reads the file, the wall clock, and the current uid, then asks [`classify`] whether the
//! cached wrap is usable. What the record protects — the store DEK **wrapped** ([`crate::wrap_dek`])
//! under a short-lived machine/session-bound key — is produced and consumed by the binary through the
//! existing envelope seam; this module never sees a raw key.
//!
//! ## Fail-closed
//! Anything other than a present, parseable, current-owner, unexpired record classifies as a
//! non-[`SessionState::Valid`] state the binary treats as ABSENT — it falls through to the resolution
//! ladder (keychain → passphrase prompt) and never silently unlocks. The value-free classification
//! never reveals key material.
//!
//! ## Binding, not confidentiality
//! The record binds the wrap to the OS user + machine/boot (via the binary's key derivation) so it
//! cannot be replayed by another user or copied to another machine, and the AEAD wrap detects
//! tampering. The at-rest **confidentiality** of the file rests on its `0600` owner-only mode (the
//! binary creates + re-checks it), exactly like `sudo`'s timestamp — there is no OS secret service to
//! bind to on the headless host this feature targets.

/// The on-disk session-unlock record: the store DEK wrapped under a machine/session-bound key, plus
/// the typed expiry and the owner it was minted for. Selectors + wrapped material only — the
/// `wrapped_dek` reveals nothing without the machine-bound KEK the binary re-derives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionRecord {
    /// The OS uid the session was minted for. A second binding beside the machine-KEK derivation (a
    /// different uid derives a different KEK and cannot unwrap anyway): a mismatch is rejected early.
    pub owner_uid: u32,
    /// When the session expires, as a Unix epoch in **seconds** (UTC). `now >= expires_at` ⇒ expired.
    pub expires_at: i64,
    /// The per-file KDF salt the binary mixes into the machine/session-bound KEK derivation.
    pub salt: [u8; SALT_LEN],
    /// The store DEK AEAD-wrapped under the machine/session KEK ([`crate::wrap_dek`] format).
    pub wrapped_dek: Vec<u8>,
}

/// The per-file KDF salt length (16 bytes — mirrors [`crate::envelope`]'s store salt).
pub const SALT_LEN: usize = 16;

/// Magic + version prefix so a format change (or an unrelated file) is detected, distinct from the
/// wrapped-DEK and vault-blob magics — this is a different artifact.
const RECORD_MAGIC: &[u8] = b"QFSUNLK1";
/// Fixed header length: magic(8) + owner_uid(4) + expires_at(8) + salt(16); the wrapped DEK follows.
const HEADER_LEN: usize = RECORD_MAGIC.len() + 4 + 8 + SALT_LEN;

impl SessionRecord {
    /// Serialize to the on-disk byte layout: `MAGIC || owner_uid(u32 LE) || expires_at(i64 LE) ||
    /// salt(16) || wrapped_dek`.
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(HEADER_LEN + self.wrapped_dek.len());
        out.extend_from_slice(RECORD_MAGIC);
        out.extend_from_slice(&self.owner_uid.to_le_bytes());
        out.extend_from_slice(&self.expires_at.to_le_bytes());
        out.extend_from_slice(&self.salt);
        out.extend_from_slice(&self.wrapped_dek);
        out
    }

    /// Parse a record from disk bytes, or `None` on a bad magic / truncation / empty wrap. A parse
    /// failure is value-free (it names no bytes) and classifies as [`SessionState::Corrupt`].
    #[must_use]
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        let rest = bytes.strip_prefix(RECORD_MAGIC)?;
        if rest.len() < HEADER_LEN - RECORD_MAGIC.len() {
            return None;
        }
        let (uid_b, rest) = rest.split_at(4);
        let (exp_b, rest) = rest.split_at(8);
        let (salt_b, wrapped) = rest.split_at(SALT_LEN);
        if wrapped.is_empty() {
            return None;
        }
        let owner_uid = u32::from_le_bytes(uid_b.try_into().ok()?);
        let expires_at = i64::from_le_bytes(exp_b.try_into().ok()?);
        let salt: [u8; SALT_LEN] = salt_b.try_into().ok()?;
        Some(SessionRecord {
            owner_uid,
            expires_at,
            salt,
            wrapped_dek: wrapped.to_vec(),
        })
    }
}

/// The classification of an on-disk session-unlock file against the current time + uid. Only
/// [`Valid`](SessionState::Valid) lets the binary attempt an unwrap; every other state is treated as
/// ABSENT (fall through the ladder) — the non-absent ones additionally tell the binary the stale file
/// should be purged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionState {
    /// No file present — fall through to the ladder, nothing to purge.
    Absent,
    /// Present but unusable: unparseable, wrong magic/version, or minted for another uid. Purge +
    /// fall through (never a silent unlock).
    Corrupt,
    /// Present + well-formed + this uid's, but past its deadline. Purge + fall through.
    Expired,
    /// Usable: the binary may re-derive the machine KEK and unwrap this record's DEK.
    Valid(SessionRecord),
}

impl SessionState {
    /// Whether a stale on-disk file backing this state should be purged (fail-closed hygiene): true
    /// for [`Corrupt`](SessionState::Corrupt) / [`Expired`](SessionState::Expired), false otherwise.
    #[must_use]
    pub fn should_purge(&self) -> bool {
        matches!(self, SessionState::Corrupt | SessionState::Expired)
    }
}

/// Classify a session-unlock file: `None` bytes ⇒ [`Absent`](SessionState::Absent); unparseable or
/// minted for a different `current_uid` ⇒ [`Corrupt`](SessionState::Corrupt); parseable + this uid's
/// but `now_epoch >= expires_at` ⇒ [`Expired`](SessionState::Expired); otherwise
/// [`Valid`](SessionState::Valid). Pure: the caller supplies the file bytes, the wall clock
/// (`now_epoch`, Unix seconds), and the current uid.
#[must_use]
pub fn classify(bytes: Option<&[u8]>, now_epoch: i64, current_uid: u32) -> SessionState {
    let Some(bytes) = bytes else {
        return SessionState::Absent;
    };
    let Some(record) = SessionRecord::from_bytes(bytes) else {
        return SessionState::Corrupt;
    };
    if record.owner_uid != current_uid {
        // Minted for another user (a copied file): reject rather than even attempt an unwrap.
        return SessionState::Corrupt;
    }
    if now_epoch >= record.expires_at {
        return SessionState::Expired;
    }
    SessionState::Valid(record)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(owner_uid: u32, expires_at: i64) -> SessionRecord {
        SessionRecord {
            owner_uid,
            expires_at,
            salt: [7u8; SALT_LEN],
            // A stand-in wrapped DEK (opaque to the record layer; the real one is a wrap_dek output).
            wrapped_dek: vec![0xABu8; 68],
        }
    }

    #[test]
    fn record_round_trips_through_bytes() {
        let r = record(1000, 1_900_000_000);
        let back = SessionRecord::from_bytes(&r.to_bytes()).expect("parses");
        assert_eq!(back, r);
    }

    #[test]
    fn absent_when_no_file() {
        assert_eq!(classify(None, 0, 1000), SessionState::Absent);
    }

    #[test]
    fn corrupt_on_bad_magic_or_truncation() {
        assert_eq!(
            classify(Some(b"not-a-record"), 0, 1000),
            SessionState::Corrupt
        );
        // Right magic, truncated header.
        assert_eq!(classify(Some(RECORD_MAGIC), 0, 1000), SessionState::Corrupt);
        // Header present but an empty wrapped DEK is rejected.
        let mut header = RECORD_MAGIC.to_vec();
        header.extend_from_slice(&1000u32.to_le_bytes());
        header.extend_from_slice(&9_999_999_999i64.to_le_bytes());
        header.extend_from_slice(&[0u8; SALT_LEN]);
        assert_eq!(classify(Some(&header), 0, 1000), SessionState::Corrupt);
    }

    #[test]
    fn corrupt_when_minted_for_another_uid() {
        let bytes = record(1000, 9_999_999_999).to_bytes();
        // Same well-formed record, but the CURRENT uid differs -> Corrupt (a copied file).
        assert_eq!(classify(Some(&bytes), 0, 2000), SessionState::Corrupt);
    }

    /// The expiry boundary is the load-bearing decision: Valid strictly BEFORE the deadline, Expired
    /// at or after it (`now >= expires_at`).
    #[test]
    fn valid_before_deadline_expired_at_or_after() {
        let deadline = 1_000_000i64;
        let bytes = record(1000, deadline).to_bytes();
        // Strictly before: Valid.
        match classify(Some(&bytes), deadline - 1, 1000) {
            SessionState::Valid(r) => assert_eq!(r.expires_at, deadline),
            other => panic!("expected Valid just before the deadline, got {other:?}"),
        }
        // Exactly at the deadline: Expired (the boundary is inclusive of expiry).
        assert_eq!(
            classify(Some(&bytes), deadline, 1000),
            SessionState::Expired
        );
        // After: Expired.
        assert_eq!(
            classify(Some(&bytes), deadline + 1, 1000),
            SessionState::Expired
        );
    }

    #[test]
    fn should_purge_only_for_corrupt_and_expired() {
        assert!(!SessionState::Absent.should_purge());
        assert!(SessionState::Corrupt.should_purge());
        assert!(SessionState::Expired.should_purge());
        assert!(!SessionState::Valid(record(1, 1)).should_purge());
    }
}
