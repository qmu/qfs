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
//! A repository is configured three ways, in precedence order (last wins a name clash): a
//! `connections.qfs` `DRIVER git` declaration, a `QFS_GIT_<REPO>=<path>` env var (the deprecated
//! fallback), and — the CANONICAL source (ticket 20260706170000, matching the `/sql` convergence of
//! 20260705000500) — a persisted `qfs connect /git/<repo> TO git AT '<path>'` binding in the project
//! DB `path_binding` registry. The `<REPO>` segment (lower-cased) is the `/git/<repo>/...` path
//! segment. A repo whose refs cannot be read is skipped (best-effort), so a `/git/<repo>` commit for
//! an unconfigured repo fails closed. `has_connections`, `git_driver`, and the `qfs describe` mount
//! all read the SAME three sources, so run / commit / describe converge on one registry.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use qfs_driver_git::{
    GitApplier, GitDriver, GitError, ObjectDb, ObjectKind, Oid, RawObject, Repo, RepoResolver,
    RepoStore,
};

/// The env-var prefix naming a git repository: `QFS_GIT_<REPO>=<path>`.
const GIT_ENV_PREFIX: &str = "QFS_GIT_";

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

/// Whether any `/git` repository is configured (a declared `DRIVER git` connection, a `QFS_GIT_*`
/// env var, OR a persisted `qfs connect /git/<repo>` binding — the canonical `path_binding` source).
#[must_use]
pub fn has_connections() -> bool {
    std::env::vars().any(|(k, v)| k.starts_with(GIT_ENV_PREFIX) && !v.is_empty())
        || crate::connections_config::declared_for("git")
            .iter()
            .any(|c| c.at_locator.is_some())
        // The canonical source: a persisted `qfs connect /git/<repo> TO git …` binding.
        || !path_binding_git_connections().is_empty()
}

/// The `qfs connect` git repositories from the project-DB `path_binding` registry (the canonical
/// source, mirroring `sql.rs`'s `path_binding_sql_connections` — ticket 20260706170000): each
/// FULL-connect binding whose `driver_id` is `git`, as `(repo, at_locator)`. `repo` is the segment
/// after `/git/`, so a `CONNECT /git/app TO git AT '<path>'` mounts at `/git/app/...`. Git needs no
/// secret, so no `SECRET` is carried. Empty when no project DB / no git binding (best-effort, never
/// panics — a persisted-but-unreadable repo just fails closed at read/commit time).
fn path_binding_git_connections() -> Vec<(String, String)> {
    let Ok(Some(proj)) = crate::store::open_project_db() else {
        return Vec::new();
    };
    let conn = proj.into_db().into_connection();
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
/// CLI-backed stores), one entry per declared `CREATE CONNECTION … DRIVER git AT '<path>'` AND per
/// `QFS_GIT_<repo>` env var (the deprecated fallback, which overrides a same-named declaration).
#[must_use]
pub fn git_driver() -> GitDriver {
    let mut resolver = RepoResolver::new();
    let mut applier = GitApplier::new();
    // Declared connections first; an equally-named env var below then overrides.
    for decl in crate::connections_config::declared_for("git") {
        let Some(path) = decl.at_locator.as_deref() else {
            continue;
        };
        let p = Path::new(path);
        let repo = decl.name.to_ascii_lowercase();
        resolver = resolver.with_repo(repo.clone(), planning_repo(p));
        applier = applier.with_store(repo, RepoStore::at_path(p));
    }
    for (key, path) in std::env::vars() {
        let Some(repo) = key.strip_prefix(GIT_ENV_PREFIX) else {
            continue;
        };
        if repo.is_empty() || path.is_empty() {
            continue;
        }
        let repo = repo.to_ascii_lowercase();
        let p = Path::new(&path);
        crate::connections_config::warn_env_var_deprecation_once();
        resolver = resolver.with_repo(repo.clone(), planning_repo(p));
        applier = applier.with_store(repo, RepoStore::at_path(p));
    }
    // `qfs connect` (the `path_binding` DB registry) — the CANONICAL local-connection mechanism
    // (ticket 20260706170000, extending the `/sql` convergence of 20260705000500 to git). A
    // `qfs connect /git/<repo> TO git AT '<path>'` binding wires the runtime git driver under
    // `<repo>`, so a persisted connection is no longer invisible to `qfs run` / `qfs describe`.
    // Registered LAST so a persisted binding wins a name clash with the deprecated
    // env-var/`connections.qfs` shims.
    for (repo, at) in path_binding_git_connections() {
        let p = Path::new(&at);
        resolver = resolver.with_repo(repo.clone(), planning_repo(p));
        applier = applier.with_store(repo, RepoStore::at_path(p));
    }
    GitDriver::new(resolver, applier)
}
