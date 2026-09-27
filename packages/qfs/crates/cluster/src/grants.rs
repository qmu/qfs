//! The host's grant table: which member may borrow which host mount.
//!
//! Default deny. A grant is a **path prefix** compared segment by segment (`/slack-acct` admits
//! every path under that mount; `/sql/demo` admits only that SQL connection). The table is a small
//! JSON file under the host's cluster state dir, re-read on every borrowed request so a
//! `qfs cluster grant` / `revoke` takes effect without restarting the host.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::{Deserialize, Serialize};

/// The grant table file name inside the cluster state dir.
pub const GRANTS_FILE: &str = "grants.json";

/// member name → granted path prefixes (each stored in its canonical `/a/b` form).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Grants {
    /// The table.
    pub members: BTreeMap<String, BTreeSet<String>>,
}

/// Split a path or mount name into its non-empty segments (`slack`, `/slack/`, `/sql/demo`).
#[must_use]
pub fn segments(path: &str) -> Vec<String> {
    path.split('/')
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

/// The canonical `/a/b` form of a grant, or `None` when it names no segment at all.
#[must_use]
pub fn canonical(path: &str) -> Option<String> {
    let segs = segments(path);
    (!segs.is_empty()).then(|| format!("/{}", segs.join("/")))
}

impl Grants {
    /// Load the table from `dir`; a missing file is an empty (deny-all) table.
    ///
    /// # Errors
    /// A message naming an unreadable or corrupt file (corrupt is never read as "allow").
    pub fn load(dir: &Path) -> Result<Self, String> {
        let path = dir.join(GRANTS_FILE);
        match std::fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str(&text)
                .map_err(|e| format!("the grant table at {} is corrupt ({e})", path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(format!("cannot read {} ({e})", path.display())),
        }
    }

    /// Write the table to `dir` atomically (temp file + rename).
    ///
    /// # Errors
    /// A message naming the I/O failure.
    pub fn save(&self, dir: &Path) -> Result<(), String> {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("cannot create {} ({e})", dir.display()))?;
        let path = dir.join(GRANTS_FILE);
        let tmp = dir.join(format!("{GRANTS_FILE}.{}.tmp", std::process::id()));
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(&tmp, text).map_err(|e| format!("cannot write {} ({e})", tmp.display()))?;
        std::fs::rename(&tmp, &path).map_err(|e| format!("cannot write {} ({e})", path.display()))
    }

    /// Grant `path` to `member`. Returns whether it was new.
    ///
    /// # Errors
    /// An empty member or path.
    pub fn grant(&mut self, member: &str, path: &str) -> Result<bool, String> {
        let member = member.trim();
        if member.is_empty() {
            return Err("the member name must not be empty".into());
        }
        let p = canonical(path).ok_or("the mount must name at least one path segment")?;
        Ok(self
            .members
            .entry(member.to_string())
            .or_default()
            .insert(p))
    }

    /// Revoke `path` from `member`. Returns whether it was present.
    pub fn revoke(&mut self, member: &str, path: &str) -> bool {
        let Some(p) = canonical(path) else {
            return false;
        };
        let Some(set) = self.members.get_mut(member.trim()) else {
            return false;
        };
        let removed = set.remove(&p);
        if set.is_empty() {
            self.members.remove(member.trim());
        }
        removed
    }

    /// Whether `member` may touch the path with these `segs`.
    #[must_use]
    pub fn allows(&self, member: &str, segs: &[String]) -> bool {
        self.members.get(member).is_some_and(|set| {
            set.iter().any(|g| {
                let g = segments(g);
                !g.is_empty() && g.len() <= segs.len() && g.iter().zip(segs).all(|(a, b)| a == b)
            })
        })
    }

    /// The first touched path `member` is NOT granted, or `None` when every one is.
    /// An empty `touched` list is itself refused (fail closed: nothing proven about it).
    #[must_use]
    pub fn first_denied(&self, member: &str, touched: &[Vec<String>]) -> Option<String> {
        if touched.is_empty() {
            return Some("(no host path could be determined)".into());
        }
        touched
            .iter()
            .find(|segs| !self.allows(member, segs))
            .map(|segs| format!("/{}", segs.join("/")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> Vec<String> {
        segments(s)
    }

    #[test]
    fn default_deny_and_prefix_match() {
        let mut g = Grants::default();
        assert!(!g.allows("m", &p("/slack/x")));
        g.grant("m", "slack").unwrap();
        g.grant("m", "/sql/demo/").unwrap();
        assert!(g.allows("m", &p("/slack/qmu/c/messages")));
        assert!(g.allows("m", &p("/sql/demo/t")));
        assert!(!g.allows("m", &p("/sql/other/t")));
        assert!(!g.allows("m", &p("/sql")));
        assert!(!g.allows("m", &p("/slackx/y")));
        assert!(!g.allows("other", &p("/slack/x")));
        assert_eq!(
            g.first_denied("m", &[]),
            Some("(no host path could be determined)".into())
        );
        assert_eq!(
            g.first_denied("m", &[p("/slack/a"), p("/mail/b")]),
            Some("/mail/b".into())
        );
        assert!(g.revoke("m", "/slack"));
        assert!(!g.allows("m", &p("/slack/x")));
    }

    #[test]
    fn round_trips_and_a_corrupt_table_is_an_error() {
        let dir = std::env::temp_dir().join(format!("qfs-grants-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(Grants::load(&dir).unwrap(), Grants::default());
        let mut g = Grants::default();
        g.grant("m", "/local").unwrap();
        g.save(&dir).unwrap();
        assert_eq!(Grants::load(&dir).unwrap(), g);
        std::fs::write(dir.join(GRANTS_FILE), "{not json").unwrap();
        assert!(Grants::load(&dir).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
