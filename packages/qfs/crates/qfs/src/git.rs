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
//! A repository is declared with `CONNECT /git/<repo> TO git AT '<path>'` (or `qfs connect`),
//! persisted in the project-DB `path_binding` registry — the SINGLE source (the retired
//! `QFS_GIT_*` env var and `connections.qfs` `DRIVER git` declaration are gone; experimental, no
//! backward compat). The `<repo>` segment after `/git/` (lower-cased) is the `/git/<repo>/...`
//! path segment. A repo whose refs cannot be read is skipped (best-effort), so a `/git/<repo>`
//! commit for an unresolvable repo fails closed. `has_connections`, `git_driver`, and the `qfs
//! describe` mount all read the SAME source, so run / commit / describe converge on one registry.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use qfs_driver_git::{
    GitApplier, GitDriver, GitError, ObjectDb, ObjectKind, Oid, RawObject, Repo, RepoResolver,
    RepoStore,
};

/// A read-side [`ObjectDb`] that fetches git objects (loose OR **packed**) by shelling out to
/// `git cat-file` against a real repository — the read counterpart of [`RepoStore::at_path`]'s
/// git-CLI apply path. The in-house [`qfs_driver_git::LooseObjectDb`] only reads loose objects held
/// in memory; a real working repo keeps most history in packfiles, so the t3 read facet resolves
/// objects through `git` (the same tool the apply leg already trusts). Reads on demand — planning,
/// which never touches an object, pays nothing.
struct CliObjectDb {
    path: PathBuf,
}

impl CliObjectDb {
    /// Run `git -C <path> <args...>`, returning stdout bytes on success.
    fn git(&self, args: &[&str]) -> Option<Vec<u8>> {
        let out = Command::new("git")
            .arg("-C")
            .arg(&self.path)
            .args(args)
            .output()
            .ok()?;
        out.status.success().then_some(out.stdout)
    }
}

impl ObjectDb for CliObjectDb {
    fn read(&self, oid: &Oid) -> Result<RawObject, GitError> {
        let missing = || GitError::ObjectNotFound {
            oid: oid.as_str().to_string(),
        };
        let type_bytes = self
            .git(&["cat-file", "-t", oid.as_str()])
            .ok_or_else(missing)?;
        let kind = match String::from_utf8_lossy(&type_bytes).trim() {
            "blob" => ObjectKind::Blob,
            "tree" => ObjectKind::Tree,
            "commit" => ObjectKind::Commit,
            "tag" => ObjectKind::Tag,
            _ => return Err(missing()),
        };
        // `git cat-file <type> <oid>` emits the RAW object payload (the same bytes the in-house
        // reader hands back after framing/inflation), which the relational/blobfs parsers consume.
        let payload = self
            .git(&["cat-file", kind.keyword(), oid.as_str()])
            .ok_or_else(missing)?;
        Ok(RawObject { kind, payload })
    }

    fn contains(&self, oid: &Oid) -> bool {
        // `git cat-file -e <oid>` exits 0 iff the object exists (and is valid).
        self.git(&["cat-file", "-e", oid.as_str()]).is_some()
    }
}

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

/// Build a [`Repo`] over the real repository at `path`: a [`CliObjectDb`] (reads loose + packed
/// objects via `git`, on demand — so planning, which never reads an object, stays free) seeded with
/// the repository's current refs, registered under BOTH the qualified name (`refs/heads/main`) and
/// the bare branch (`main`), plus `HEAD` (which `git show-ref` omits) so a default no-`@ref` read
/// resolves the live tip. This backs both the planning mount (describe is cred-free, reads only ref
/// oids) AND the t3 read facet (which reads commit/tree/blob objects through the `CliObjectDb`).
fn planning_repo(path: &Path) -> Repo {
    let mut repo = Repo::new(Arc::new(CliObjectDb {
        path: path.to_path_buf(),
    }));
    for (name, oid) in read_refs(path) {
        repo.set_ref(name.clone(), oid.clone());
        if let Some(bare) = name.strip_prefix("refs/heads/") {
            repo.set_ref(bare.to_string(), oid);
        }
    }
    // `HEAD` is the default coordinate for a no-`@ref` read; `show-ref` does not list it, so resolve
    // it explicitly (a detached or branch HEAD both `rev-parse` to a commit oid).
    if let Some(out) = (CliObjectDb {
        path: path.to_path_buf(),
    })
    .git(&["rev-parse", "HEAD"])
    {
        if let Ok(oid) = Oid::parse(String::from_utf8_lossy(&out).trim()) {
            repo.set_ref("HEAD", oid);
        }
    }
    repo
}

/// Whether any `/git` repository is bound (a persisted `CONNECT /git/<repo> TO git …`
/// `path_binding` row — the single source).
#[must_use]
pub fn has_connections() -> bool {
    !path_binding_git_connections().is_empty()
}

/// The `qfs connect` git repositories from the project-DB `path_binding` registry (the canonical
/// source, mirroring `sql.rs`'s `path_binding_sql_connections` — ticket 20260706170000): each
/// FULL-connect binding whose `driver_id` is `git`, as `(repo, at_locator)`. `repo` is the segment
/// after `/git/`, so a `CONNECT /git/app TO git AT '<path>'` mounts at `/git/app/...`. Git needs no
/// secret, so no `SECRET` is carried. Empty when no system DB / no git binding (best-effort, never
/// panics — a persisted-but-unreadable repo just fails closed at read/commit time).
fn path_binding_git_connections() -> Vec<(String, String)> {
    let Ok(Some(sys)) = crate::store::open_system_db() else {
        return Vec::new();
    };
    let conn = sys.into_db().into_connection();
    crate::path_binding::db_list_bindings(&conn)
        .unwrap_or_default()
        .into_iter()
        .filter(|b| b.alias_of.is_none())
        .filter_map(|b| {
            if b.driver_id.as_deref() != Some("git") {
                return None;
            }
            let repo = b
                .path
                .strip_prefix("/git/")?
                .split('/')
                .next()
                .filter(|s| !s.is_empty())?
                .to_ascii_lowercase();
            let at = b.at_locator.clone()?;
            Some((repo, at))
        })
        .collect()
}

/// Build the live [`GitDriver`]: the resolver (real-ref planning repos) + the applier (real-repo
/// CLI-backed stores), one entry per persisted `CONNECT /git/<repo> TO git AT '<path>'`
/// `path_binding` row — the SINGLE source (the retired `QFS_GIT_*` env var and `connections.qfs`
/// declaration are gone).
#[must_use]
pub fn git_driver() -> GitDriver {
    let mut resolver = RepoResolver::new();
    let mut applier = GitApplier::new();
    for (repo, at) in path_binding_git_connections() {
        let p = Path::new(&at);
        resolver = resolver.with_repo(repo.clone(), planning_repo(p));
        applier = applier.with_store(repo, RepoStore::at_path(p));
    }
    GitDriver::new(resolver, applier)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_var_and_connections_file_bind_nothing_only_path_binding_binds() {
        // The single-source guarantee for git: the retired `QFS_GIT_*` env var and a
        // `connections.qfs` `CREATE CONNECTION … DRIVER git` file bind NOTHING (their loader +
        // fallback are gone — experimental, no backward compat). Only a persisted `CONNECT
        // /git/<repo> …` `path_binding` row wires a mount, so run / commit / describe converge.
        let _home = crate::testenv::HomeGuard::with_passphrase("git-only-path-binding");
        let dir = tempfile::tempdir().unwrap();
        let repo_path = dir.path().join("app.git");
        std::fs::create_dir_all(&repo_path).unwrap();
        let repo_str = repo_path.to_str().unwrap();

        // env-only / file-only configurations resolve to NO working mount.
        let conns = dir.path().join("connections.qfs");
        std::fs::write(
            &conns,
            format!("CREATE CONNECTION app DRIVER git AT '{repo_str}';"),
        )
        .unwrap();
        std::env::set_var("QFS_GIT_APP", repo_str);
        std::env::set_var("QFS_CONNECTIONS", conns.to_str().unwrap());
        assert!(
            !has_connections(),
            "a QFS_GIT_* / connections.qfs config binds no /git mount"
        );
        std::env::remove_var("QFS_GIT_APP");
        std::env::remove_var("QFS_CONNECTIONS");

        // Only the persisted path_binding row wires the mount.
        let proj = crate::store::open_system_db()
            .unwrap()
            .unwrap()
            .into_db()
            .into_connection();
        crate::path_binding::db_upsert_binding(
            &proj,
            "/git/app",
            "git",
            Some(repo_str),
            None,
            Some("local"),
            None,
            None,
        )
        .unwrap();
        drop(proj);
        assert!(
            has_connections(),
            "the path_binding row wires the /git mount"
        );
    }
}
