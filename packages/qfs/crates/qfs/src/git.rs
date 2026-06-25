//! The `qfs run`/commit git composition root: builds the live [`GitDriver`] the binary plans +
//! commits `/git/<repo>/...` statements against, over **real on-disk repositories** driven by the
//! `git` CLI (ADR-0003's CLI backend — no `gix`/heavy dep).
//!
//! The driver's write planner (`plan_insert_commit`, invoked through the engine's
//! [`Driver::plan_write`](qfs_driver::Driver::plan_write) seam) lowers `INSERT INTO
//! /git/<repo>/commits` into the encoded `blob→tree→commit→ref→reflog` effect plan; the apply leg
//! (`RepoStore::at_path`) persists it through `git hash-object -w` + the atomic `git update-ref`
//! CAS. `qfs-cmd`/`qfs-exec` stay off the concrete driver (the dep_direction guard); the terminal
//! binary owns this wiring — like the local / sql composition. The `git` process dead-ends here.
//!
//! ## Config (no credentials)
//! Each repository is one env var `QFS_GIT_<REPO>=<path-to-worktree-or-.git>`; the `<REPO>` suffix
//! (lower-cased) is the `/git/<repo>/...` path segment. A repo whose refs cannot be read is skipped
//! (best-effort), so a `/git/<repo>` commit for an unconfigured repo fails closed.

use std::path::Path;
use std::process::Command;
use std::sync::Arc;

use qfs_driver_git::{GitApplier, GitDriver, LooseObjectDb, Oid, Repo, RepoResolver, RepoStore};

/// The env-var prefix naming a git repository: `QFS_GIT_<REPO>=<path>`.
const GIT_ENV_PREFIX: &str = "QFS_GIT_";

/// Read the current refs of the repository at `path` via `git show-ref`, returning
/// `(ref_name, oid)` pairs. Best-effort: a fresh repo with no commits (or an unreadable path)
/// yields an empty list (the first commit then has no parent). Never panics.
fn read_refs(path: &Path) -> Vec<(String, Oid)> {
    let Ok(out) = Command::new("git")
        .arg("-C")
        .arg(path)
        .args(["show-ref"])
        .output()
    else {
        return Vec::new();
    };
    if !out.status.success() {
        return Vec::new();
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let mut refs = Vec::new();
    for line in text.lines() {
        // Each line is `<oid> <refname>` (e.g. `<sha> refs/heads/main`).
        if let Some((oid, name)) = line.split_once(' ') {
            if let Ok(oid) = Oid::parse(oid.trim()) {
                refs.push((name.trim().to_string(), oid));
            }
        }
    }
    refs
}

/// Build a [`Repo`] for planning: an empty object db (planning a commit reads only the parent ref
/// oid, never objects) seeded with the real repository's current refs — registered under BOTH the
/// qualified name (`refs/heads/main`) and the bare branch (`main`) so the planner resolves the live
/// tip whichever form the statement uses.
fn planning_repo(path: &Path) -> Repo {
    let mut repo = Repo::new(Arc::new(LooseObjectDb::new()));
    for (name, oid) in read_refs(path) {
        repo.set_ref(name.clone(), oid.clone());
        if let Some(bare) = name.strip_prefix("refs/heads/") {
            repo.set_ref(bare.to_string(), oid);
        }
    }
    repo
}

/// Whether any `/git` repository is configured.
#[must_use]
pub fn has_connections() -> bool {
    std::env::vars().any(|(k, v)| k.starts_with(GIT_ENV_PREFIX) && !v.is_empty())
}

/// Build the live [`GitDriver`]: the resolver (real-ref planning repos) + the applier (real-repo
/// CLI-backed stores), one entry per `QFS_GIT_<repo>` env var.
#[must_use]
pub fn git_driver() -> GitDriver {
    let mut resolver = RepoResolver::new();
    let mut applier = GitApplier::new();
    for (key, path) in std::env::vars() {
        let Some(repo) = key.strip_prefix(GIT_ENV_PREFIX) else {
            continue;
        };
        if repo.is_empty() || path.is_empty() {
            continue;
        }
        let repo = repo.to_ascii_lowercase();
        let p = Path::new(&path);
        resolver = resolver.with_repo(repo.clone(), planning_repo(p));
        applier = applier.with_store(repo, RepoStore::at_path(p));
    }
    GitDriver::new(resolver, applier)
}
