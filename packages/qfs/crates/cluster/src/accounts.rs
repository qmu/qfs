//! The host's **account pool**: the Claude Code / Codex accounts registered on the host.
//!
//! Metadata only — provider, label, email, plan, status and the member an account is assigned
//! to. No credential material is stored here in this slice (switching accounts on a usage limit is
//! later work). The pool is a small JSON file under the host's cluster state dir, re-read on every
//! `GET /api/cluster/accounts` so `qfs cluster account add` shows up without restarting the host.

use std::path::Path;

use serde::{Deserialize, Serialize};

/// The account pool file name inside the cluster state dir.
pub const ACCOUNTS_FILE: &str = "accounts.json";

/// The providers an account may belong to.
pub const PROVIDERS: [&str; 2] = ["claude-code", "codex"];

/// One `/cluster/accounts` row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountRow {
    /// `claude-code` or `codex`.
    pub provider: String,
    /// The unique label the operator names the account by.
    pub label: String,
    /// The account's email, if recorded.
    pub email: Option<String>,
    /// The plan (e.g. `max`, `pro`), if recorded.
    pub plan: Option<String>,
    /// `available` (unassigned) or `assigned`.
    pub status: String,
    /// The member the account is assigned to.
    pub assigned_member: Option<String>,
}

/// The account pool.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Accounts {
    /// Rows, in insertion order.
    pub accounts: Vec<AccountRow>,
}

fn status_of(assigned: Option<&String>) -> String {
    if assigned.is_some() {
        "assigned".into()
    } else {
        "available".into()
    }
}

fn opt(s: Option<&str>) -> Option<String> {
    s.map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

impl Accounts {
    /// Load the pool from `dir`; a missing file is an empty pool.
    ///
    /// # Errors
    /// A message naming an unreadable or corrupt file.
    pub fn load(dir: &Path) -> Result<Self, String> {
        let path = dir.join(ACCOUNTS_FILE);
        match std::fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str(&text)
                .map_err(|e| format!("the account pool at {} is corrupt ({e})", path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(format!("cannot read {} ({e})", path.display())),
        }
    }

    /// Write the pool to `dir` atomically (temp file + rename).
    ///
    /// # Errors
    /// A message naming the I/O failure.
    pub fn save(&self, dir: &Path) -> Result<(), String> {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("cannot create {} ({e})", dir.display()))?;
        let path = dir.join(ACCOUNTS_FILE);
        let tmp = dir.join(format!("{ACCOUNTS_FILE}.{}.tmp", std::process::id()));
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(&tmp, text).map_err(|e| format!("cannot write {} ({e})", tmp.display()))?;
        std::fs::rename(&tmp, &path).map_err(|e| format!("cannot write {} ({e})", path.display()))
    }

    /// Register an account.
    ///
    /// # Errors
    /// An unknown provider, an empty label, or a label already in the pool.
    pub fn add(
        &mut self,
        provider: &str,
        label: &str,
        email: Option<&str>,
        plan: Option<&str>,
    ) -> Result<(), String> {
        let provider = provider.trim();
        if !PROVIDERS.contains(&provider) {
            return Err(format!(
                "unknown provider `{provider}` (expected one of: {})",
                PROVIDERS.join(", ")
            ));
        }
        let label = label.trim();
        if label.is_empty() {
            return Err("the account label must not be empty".into());
        }
        if self.accounts.iter().any(|a| a.label == label) {
            return Err(format!("an account labelled `{label}` already exists"));
        }
        self.accounts.push(AccountRow {
            provider: provider.to_string(),
            label: label.to_string(),
            email: opt(email),
            plan: opt(plan),
            status: status_of(None),
            assigned_member: None,
        });
        Ok(())
    }

    /// Remove the account labelled `label`. Returns whether it was present.
    pub fn remove(&mut self, label: &str) -> bool {
        let before = self.accounts.len();
        self.accounts.retain(|a| a.label != label.trim());
        self.accounts.len() != before
    }

    /// Assign the account labelled `label` to `member` (`None` unassigns it).
    ///
    /// # Errors
    /// No account with that label.
    pub fn assign(&mut self, label: &str, member: Option<&str>) -> Result<(), String> {
        let row = self
            .accounts
            .iter_mut()
            .find(|a| a.label == label.trim())
            .ok_or_else(|| format!("no account labelled `{}`", label.trim()))?;
        row.assigned_member = opt(member);
        row.status = status_of(row.assigned_member.as_ref());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_assign_remove_and_validation() {
        let mut a = Accounts::default();
        a.add("claude-code", "work", Some("a@example.com"), Some("max"))
            .unwrap();
        assert!(a.add("claude-code", "work", None, None).is_err());
        assert!(a.add("gemini", "x", None, None).is_err());
        assert!(a.add("codex", "  ", None, None).is_err());
        a.add("codex", "cx", None, Some("")).unwrap();
        assert_eq!(a.accounts[1].plan, None);
        assert_eq!(a.accounts[0].status, "available");
        a.assign("work", Some("m1")).unwrap();
        assert_eq!(a.accounts[0].status, "assigned");
        assert_eq!(a.accounts[0].assigned_member.as_deref(), Some("m1"));
        a.assign("work", None).unwrap();
        assert_eq!(a.accounts[0].status, "available");
        assert!(a.assign("nope", Some("m")).is_err());
        assert!(a.remove("cx"));
        assert!(!a.remove("cx"));
    }

    #[test]
    fn round_trips_and_a_corrupt_pool_is_an_error() {
        let dir = std::env::temp_dir().join(format!("qfs-accounts-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(Accounts::load(&dir).unwrap(), Accounts::default());
        let mut a = Accounts::default();
        a.add("codex", "c", None, None).unwrap();
        a.save(&dir).unwrap();
        assert_eq!(Accounts::load(&dir).unwrap(), a);
        std::fs::write(dir.join(ACCOUNTS_FILE), "{").unwrap();
        assert!(Accounts::load(&dir).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
