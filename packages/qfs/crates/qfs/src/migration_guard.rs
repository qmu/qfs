//! `check-migrations` — the pre-ship guard against a silent in-place edit of an ALREADY-SHIPPED
//! embedded migration body (ticket 20260706120200; concern 11 / 203120).
//!
//! The runtime `ChecksumMismatch` / heal path ([`qfs_store::migrate`]) only fires against a DB that
//! already RECORDED the old checksum — so a fresh CI test DB never trips it, and nothing catches a
//! developer editing a shipped `schema/*.sql` body BEFORE the PR merges. This guard closes that gap
//! at build time: it diffs each shipped migration body against its content at the last release tag,
//! and FAILS if a shipped body changed WITHOUT a matching [`qfs_store::SUPERSEDED_BODIES`] entry (the
//! audited heal-forward escape hatch). Wired into the anti-drift gate family, beside `gen-docs
//! --check`.
//!
//! Off the binary's spine: pure comparison logic plus a thin `git show` reader. The `qfs` crate
//! hosts it (like [`crate::docs`]) so `xtask` stays dep-light — it only calls this.

use std::io;
use std::path::Path;
use std::process::Command;

use qfs_crypto_core::sha256_hex;

/// The repo-relative directory holding the embedded migration bodies (`include_str!`-ed into
/// `qfs_store::{SYSTEM_MIGRATIONS, PROJECT_MIGRATIONS}`).
const SCHEMA_DIR: &str = "packages/qfs/crates/store/src/schema";

/// Verify no ALREADY-SHIPPED migration body was edited in place without an audited heal-forward
/// entry. Returns the offending files (empty ⇒ clean). Best-effort on the git side: with no release
/// tag yet (nothing has shipped) it returns `Ok(vec![])` — there is nothing to guard.
///
/// # Errors
/// [`io::Error`] if the schema directory cannot be read.
pub fn check_shipped_migrations(git_root: &Path) -> io::Result<Vec<String>> {
    let Some(tag) = last_release_tag(git_root) else {
        return Ok(Vec::new());
    };
    let schema = git_root.join(SCHEMA_DIR);
    let mut bodies: Vec<(String, Option<String>, String)> = Vec::new();
    for entry in std::fs::read_dir(&schema)? {
        let path = entry?.path();
        if path.extension().and_then(|e| e.to_str()) != Some("sql") {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let current = std::fs::read_to_string(&path)?;
        let shipped = git_show(git_root, &tag, &format!("{SCHEMA_DIR}/{name}"));
        bodies.push((name.to_string(), shipped, current));
    }
    bodies.sort_by(|a, b| a.0.cmp(&b.0));
    let superseded: Vec<&str> = qfs_store::SUPERSEDED_BODIES
        .iter()
        .map(|s| s.old_checksum)
        .collect();
    Ok(unapproved_edits(&bodies, &tag, &superseded))
}

/// The pure verdict (testable): an offending file is one whose SHIPPED body (present at the tag)
/// differs from the CURRENT body AND whose shipped-body checksum is not a registered
/// [`qfs_store::SUPERSEDED_BODIES`] entry. A file absent at the tag is NEW (never shipped) — not an
/// edit. `bodies` is `(name, shipped-body-at-tag, current-body)`.
fn unapproved_edits(
    bodies: &[(String, Option<String>, String)],
    tag: &str,
    superseded: &[&str],
) -> Vec<String> {
    bodies
        .iter()
        .filter_map(|(name, shipped, current)| {
            let old = sha256_hex(shipped.as_ref()?.as_bytes());
            let new = sha256_hex(current.as_bytes());
            if old == new || superseded.contains(&old.as_str()) {
                return None;
            }
            Some(format!(
                "{name}: shipped body changed since {tag} ({old} -> {new}) with no \
                 SUPERSEDED_BODIES heal-forward entry — append a NEW migration version instead, or \
                 (only if it already shipped) add a SupersededBody keyed on {old}"
            ))
        })
        .collect()
}

/// The most recent `v*` release tag reachable from HEAD, or `None` (nothing shipped yet / no git).
fn last_release_tag(git_root: &Path) -> Option<String> {
    let out = Command::new("git")
        .args(["describe", "--tags", "--match", "v*", "--abbrev=0"])
        .current_dir(git_root)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let tag = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!tag.is_empty()).then_some(tag)
}

/// The content of `rel_path` at `tag` via `git show`, or `None` if it did not exist there (a NEW
/// file — never shipped, so never an in-place edit).
fn git_show(git_root: &Path, tag: &str, rel_path: &str) -> Option<String> {
    let out = Command::new("git")
        .args(["show", &format!("{tag}:{rel_path}")])
        .current_dir(git_root)
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_changed_shipped_body_without_a_heal_entry_is_flagged() {
        let bodies = vec![
            // Unchanged shipped body → clean.
            (
                "a.sql".to_string(),
                Some("CREATE TABLE a(x);".to_string()),
                "CREATE TABLE a(x);".to_string(),
            ),
            // Changed shipped body, no heal entry → offending.
            (
                "b.sql".to_string(),
                Some("CREATE TABLE b(x);".to_string()),
                "CREATE TABLE b(y);".to_string(),
            ),
            // A NEW file (absent at the tag) → not an in-place edit.
            ("c.sql".to_string(), None, "CREATE TABLE c(z);".to_string()),
        ];
        let offenders = unapproved_edits(&bodies, "v1.2.3", &[]);
        assert_eq!(
            offenders.len(),
            1,
            "only the edited shipped body is flagged"
        );
        assert!(offenders[0].starts_with("b.sql:"));
    }

    #[test]
    fn a_changed_body_with_a_superseded_entry_is_allowed() {
        let old = "CREATE TABLE b(x);";
        let old_sum = sha256_hex(old.as_bytes());
        let bodies = vec![(
            "b.sql".to_string(),
            Some(old.to_string()),
            "CREATE TABLE b(y);".to_string(),
        )];
        // The old-body checksum is a registered heal-forward entry → the edit is audited, allowed.
        let offenders = unapproved_edits(&bodies, "v1.2.3", &[old_sum.as_str()]);
        assert!(offenders.is_empty(), "an audited heal-forward edit passes");
    }
}
