//! Apply-time capability gating (blueprint §6/§8) and concurrency limits.
//!
//! The parse-time capability gate is t13 (`qfs_driver::check_capability`); this is the
//! **defense-in-depth re-check** the interpreter performs immediately before dispatching an
//! effect, so a plan that slipped past parsing (or was constructed programmatically) still
//! cannot reach the World with an ungranted `(driver, verb)` on an out-of-scope path. The check
//! keys on owned identity only — a [`DriverId`], the effect's verb label, and an optional
//! [`PathScope`] — never a credential.
//!
//! ## Path-scoped grants (blueprint §8, ticket 20260704110923)
//! A grant is `(driver, verb, Option<PathScope>)`. An **unscoped** grant (`None`) matches any
//! path — the additive default, so no existing policy narrows when path-awareness lands. A
//! **scoped** grant matches only targets whose path the [`PathScope`] glob admits, which is what
//! makes the ADR 0009 §6 matrix expressible: `INSERT` on `/sql/<conn>/<table>` (DML) and `INSERT`
//! on `/sql/<conn>` (DDL / create table) are the *same* `(sql, INSERT)` and are told apart only by
//! path. This composes with, and never replaces, the irreversible-commit gate (defense in depth).

use std::collections::HashSet;

use qfs_plan::{EffectKind, Target};
use qfs_types::DriverId;

/// A segment-glob over a VFS path (blueprint §8). `*` matches exactly one path segment; a trailing
/// `**` matches any depth of subtree (zero or more segments); a literal segment matches itself.
/// So `/sql/*` matches the catalog node `/sql/shop` but **not** the table `/sql/shop/items`,
/// `/sql/*/*` matches the table but not the catalog, and `/sql/**` matches both — the exact
/// prefix/depth distinction the data-only vs read-only vs admin grant sets need.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PathScope {
    /// The glob segments (leading `/` and empty segments dropped).
    segments: Vec<String>,
}

impl PathScope {
    /// Parse a path glob (`/sql/*/*`, `/sql/**`, `/sql/shop`). The leading `/` is optional and
    /// empty segments are dropped; an empty pattern (`/`) matches every path (equivalent to an
    /// unscoped grant).
    #[must_use]
    pub fn parse(pattern: &str) -> Self {
        Self {
            segments: pattern
                .split('/')
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect(),
        }
    }

    /// Whether this scope admits a target VFS path.
    #[must_use]
    pub fn matches(&self, path: &str) -> bool {
        let node: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        glob_segments_match(&self.segments, &node)
    }
}

/// Segment-glob match: `**` (as a segment) swallows the rest, `*` matches one segment, a literal
/// matches itself, and an exhausted glob admits only an exhausted node (no implicit subtree). An
/// empty glob matches every path (the "any path" pattern). Mirrors the server policy engine's
/// `glob_segments_match` so a scope means the same thing at both enforcement layers.
fn glob_segments_match(glob: &[String], node: &[&str]) -> bool {
    if glob.is_empty() {
        return true;
    }
    let mut gi = 0;
    let mut ni = 0;
    while gi < glob.len() {
        let g = glob[gi].as_str();
        if g == "**" {
            return true;
        }
        let Some(&n) = node.get(ni) else {
            return false;
        };
        if g != "*" && g != n {
            return false;
        }
        gi += 1;
        ni += 1;
    }
    ni == node.len()
}

/// The set of `(driver, verb, scope)` grants in force for a commit — the least-privilege envelope
/// the interpreter enforces at apply time. An effect whose `(driver, verb)` is absent, or present
/// only under a [`PathScope`] its target path does not match, is rejected with a structured
/// `capability-denied` error (naming the path) **before** the driver is called.
#[derive(Debug, Clone, Default)]
pub struct CapabilitySet {
    grants: HashSet<(DriverId, String, Option<PathScope>)>,
    allow_all: bool,
}

impl CapabilitySet {
    /// An empty set — every effect is denied. The safe default for unattended runs.
    #[must_use]
    pub fn none() -> Self {
        Self::default()
    }

    /// A set that grants everything — used by trusted callers and most tests that are not
    /// exercising the gate itself. Explicit, never the default.
    #[must_use]
    pub fn allow_all() -> Self {
        Self {
            grants: HashSet::new(),
            allow_all: true,
        }
    }

    /// Grant `verb` on `driver` for **any path** (builder form). The verb is the stable
    /// [`EffectKind`] label (`READ`/`INSERT`/`CALL`/…) so the grant set is owned, vendor-free
    /// data. An unscoped grant is the additive default: it matches every path, exactly as grants
    /// did before path-awareness.
    #[must_use]
    pub fn grant(mut self, driver: DriverId, kind: &EffectKind) -> Self {
        self.grants.insert((driver, kind.label().to_string(), None));
        self
    }

    /// Grant `verb` on `driver` **only for paths the [`PathScope`] admits** (builder form) — the
    /// path-scoped grant that expresses the ADR 0009 §6 matrix (e.g. DML on `/sql/*/*` while DDL
    /// on `/sql/*` stays ungranted).
    #[must_use]
    pub fn grant_scoped(mut self, driver: DriverId, kind: &EffectKind, scope: PathScope) -> Self {
        self.grants
            .insert((driver, kind.label().to_string(), Some(scope)));
        self
    }

    /// Whether this effect (its target driver + kind + path) is permitted. `allow_all` admits
    /// everything; otherwise a matching `(driver, verb)` grant admits the effect iff its scope is
    /// unscoped (any path) or its glob admits the target path.
    #[must_use]
    pub fn allows(&self, target: &Target, kind: &EffectKind) -> bool {
        if self.allow_all {
            return true;
        }
        let verb = kind.label();
        self.grants.iter().any(|(driver, grant_verb, scope)| {
            *driver == target.driver
                && grant_verb == verb
                && scope
                    .as_ref()
                    .is_none_or(|s| s.matches(target.path.as_str()))
        })
    }
}

#[cfg(test)]
mod caps_tests {
    use super::*;
    use qfs_plan::VfsPath;

    fn target(driver: &str, path: &str) -> Target {
        Target::new(DriverId::new(driver), VfsPath::new(path))
    }

    #[test]
    fn path_scope_segment_glob_distinguishes_depth() {
        // `/sql/*` = the catalog node only; `/sql/*/*` = a table only; `/sql/**` = the subtree.
        let catalog = PathScope::parse("/sql/*");
        assert!(catalog.matches("/sql/shop"));
        assert!(!catalog.matches("/sql/shop/items"));

        let table = PathScope::parse("/sql/*/*");
        assert!(!table.matches("/sql/shop"));
        assert!(table.matches("/sql/shop/items"));

        let subtree = PathScope::parse("/sql/**");
        assert!(subtree.matches("/sql/shop"));
        assert!(subtree.matches("/sql/shop/items"));

        let exact = PathScope::parse("/sql/shop");
        assert!(exact.matches("/sql/shop"));
        assert!(!exact.matches("/sql/shop/items"));
        assert!(!exact.matches("/sql/other"));

        // An empty pattern is the "any path" scope (equivalent to an unscoped grant).
        assert!(PathScope::parse("/").matches("/anything/at/all"));
    }

    #[test]
    fn unscoped_grant_matches_any_path_unchanged() {
        // The additive default: an unscoped grant admits the verb at every path, exactly as
        // grants behaved before path-awareness (no existing policy narrows silently).
        let caps = CapabilitySet::none().grant(DriverId::new("sql"), &EffectKind::Insert);
        assert!(caps.allows(&target("sql", "/sql/shop"), &EffectKind::Insert));
        assert!(caps.allows(&target("sql", "/sql/shop/items"), &EffectKind::Insert));
        // …but only the granted verb, and only the granted driver.
        assert!(!caps.allows(&target("sql", "/sql/shop"), &EffectKind::Remove));
        assert!(!caps.allows(&target("mail", "/sql/shop"), &EffectKind::Insert));
    }

    #[test]
    fn allow_all_admits_everything() {
        let caps = CapabilitySet::allow_all();
        assert!(caps.allows(&target("sql", "/sql/shop"), &EffectKind::Remove));
        assert!(caps.allows(&target("anything", "/x/y/z"), &EffectKind::Insert));
    }

    #[test]
    fn data_only_grant_admits_dml_denies_ddl() {
        // ADR 0009 §6 "data-only": DML on a table (`INSERT`/`REMOVE` at `/sql/<conn>/<table>`) is
        // granted, but DDL on the catalog (`INSERT`/`REMOVE` at `/sql/<conn>` — create/drop table)
        // is NOT — the exact distinction that was impossible when grants keyed on `(driver, verb)`.
        let data_only = CapabilitySet::none()
            .grant_scoped(
                DriverId::new("sql"),
                &EffectKind::Insert,
                PathScope::parse("/sql/*/*"),
            )
            .grant_scoped(
                DriverId::new("sql"),
                &EffectKind::Remove,
                PathScope::parse("/sql/*/*"),
            );
        // DML on a table: admitted.
        assert!(data_only.allows(&target("sql", "/sql/shop/items"), &EffectKind::Insert));
        assert!(data_only.allows(&target("sql", "/sql/shop/items"), &EffectKind::Remove));
        // DDL on the catalog (create/drop table): denied, though the verb is the same `INSERT`.
        assert!(!data_only.allows(&target("sql", "/sql/shop"), &EffectKind::Insert));
        assert!(!data_only.allows(&target("sql", "/sql/shop"), &EffectKind::Remove));
    }

    #[test]
    fn read_only_grant_denies_all_writes() {
        // ADR 0009 §6 "read-only": only READ is granted (over the whole subtree); every write is
        // denied at both the catalog and the table.
        let read_only = CapabilitySet::none().grant_scoped(
            DriverId::new("sql"),
            &EffectKind::Read,
            PathScope::parse("/sql/**"),
        );
        assert!(read_only.allows(&target("sql", "/sql/shop/items"), &EffectKind::Read));
        assert!(!read_only.allows(&target("sql", "/sql/shop/items"), &EffectKind::Insert));
        assert!(!read_only.allows(&target("sql", "/sql/shop"), &EffectKind::Insert));
    }
}

/// Two-level concurrency caps (blueprint §7 backpressure): a `global` ceiling on driver groups in
/// flight across the whole commit, and a `per_driver` ceiling so one driver cannot consume
/// the whole budget (respecting upstream rate limits). Config-driven so a wide DAG frontier
/// never spawns unbounded tasks or exhausts file descriptors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConcurrencyLimits {
    /// Max driver-groups dispatched concurrently across all drivers.
    pub global: usize,
    /// Max driver-groups dispatched concurrently *per driver id*.
    pub per_driver: usize,
}

impl ConcurrencyLimits {
    /// Construct limits, clamping each to at least 1 (a zero limit would deadlock the
    /// scheduler — a semaphore with no permits never admits a group).
    #[must_use]
    pub fn new(global: usize, per_driver: usize) -> Self {
        Self {
            global: global.max(1),
            per_driver: per_driver.max(1),
        }
    }
}

impl Default for ConcurrencyLimits {
    /// A conservative default: a modest global fan-out, modest per-driver fan-out.
    fn default() -> Self {
        Self::new(8, 4)
    }
}

/// Per-leg timeout + retry policy (blueprint §7 idempotency/observability). Retries apply **only**
/// to retryable, non-`irreversible` legs — the runtime never auto-retries an irreversible
/// effect (`REMOVE`, `CALL mail.send`) even on a transient error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryPolicy {
    /// Max attempts for a retryable, non-irreversible leg (1 = no retry).
    pub max_attempts: u32,
    /// Per-leg timeout in milliseconds (`None` = no timeout). A leg that exceeds it fails
    /// with [`EffectError::TimedOut`](crate::EffectError::TimedOut).
    pub timeout_millis: Option<u64>,
}

impl RetryPolicy {
    /// Construct a policy, clamping `max_attempts` to at least 1.
    #[must_use]
    pub fn new(max_attempts: u32, timeout_millis: Option<u64>) -> Self {
        Self {
            max_attempts: max_attempts.max(1),
            timeout_millis,
        }
    }
}

impl Default for RetryPolicy {
    /// A conservative default: up to 3 attempts on retryable legs, no wall-clock timeout
    /// (tests opt into a timeout explicitly so they stay deterministic).
    fn default() -> Self {
        Self::new(3, None)
    }
}
