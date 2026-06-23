//! The synchronous filesystem core: sandbox path resolution, directory/glob scan,
//! streaming reads/writes, and the [`LocalEffect`] apply leg. All `std::fs` types stay
//! **internal** to this module (RFD §9) — only [`LocalRow`], owned paths, byte counts, and
//! [`LocalError`] cross the boundary.
//!
//! ## Sandbox (RFD §10 least privilege)
//! Every VFS path crosses [`Sandbox::resolve`] first: it strips the `/local` mount prefix,
//! joins onto `root`, lexically normalises `.`/`..` **without** following the path past
//! `root`, and — for existing paths — canonicalises and re-checks containment so a symlink
//! cannot point outside the mount. A path that escapes yields [`LocalError::OutsideSandbox`]
//! and performs **no** I/O.
//!
//! ## Streaming (RFD §6, the hard part)
//! Reads and the copy/move verify hash run through a fixed [`COPY_BUF`] buffer — never
//! `read_to_end` on the hot copy path — so a multi-GB blob moves in bounded memory. Writes
//! go to a sibling temp file and atomically `rename` on finish (retry-safe, no torn writes).

use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Component, Path as StdPath, PathBuf};

use crate::error::LocalError;
use crate::row::LocalRow;

/// The fixed streaming buffer size (64 KiB) for bounded-memory reads and copy/verify.
const COPY_BUF: usize = 64 * 1024;

/// The mount prefix every local VFS path carries.
pub(crate) const MOUNT: &str = "/local";

/// A least-privilege sandbox: all I/O is confined to `root`. `root` is canonicalised at
/// construction so containment checks compare canonical prefixes.
#[derive(Debug, Clone)]
pub struct Sandbox {
    root: PathBuf,
}

impl Sandbox {
    /// Build a sandbox confined to `root` (canonicalised). The caller is responsible for
    /// `root` existing; a non-existent root makes every resolve fail closed.
    #[must_use]
    pub fn new(root: PathBuf) -> Self {
        let root = fs::canonicalize(&root).unwrap_or(root);
        Self { root }
    }

    /// The sandbox root (canonical).
    #[must_use]
    pub fn root(&self) -> &StdPath {
        &self.root
    }

    /// Map a VFS path (`/local/sub/a.md`) to the relative segment under the mount
    /// (`sub/a.md`). A path that is exactly the mount maps to the empty relative path
    /// (the root directory). A path outside the mount is rejected.
    fn vfs_to_rel(vfs: &str) -> Result<PathBuf, LocalError> {
        let rel = if vfs == MOUNT {
            ""
        } else if let Some(stripped) = vfs.strip_prefix(&format!("{MOUNT}/")) {
            stripped
        } else {
            return Err(LocalError::OutsideSandbox(vfs.to_string()));
        };
        Ok(PathBuf::from(rel))
    }

    /// Lexically normalise a relative path, rejecting any component that would climb above
    /// the root (`..` at depth 0) or inject an absolute jump. This guards traversal
    /// **before** touching the filesystem (so `..` escapes never perform I/O).
    fn normalise(rel: &StdPath, original: &str) -> Result<PathBuf, LocalError> {
        let mut out = PathBuf::new();
        let mut depth = 0i32;
        for comp in rel.components() {
            match comp {
                Component::Normal(seg) => {
                    out.push(seg);
                    depth += 1;
                }
                Component::CurDir => {}
                Component::ParentDir => {
                    depth -= 1;
                    if depth < 0 {
                        return Err(LocalError::OutsideSandbox(original.to_string()));
                    }
                    out.pop();
                }
                // An absolute root or Windows prefix in a *relative* segment is an escape.
                Component::RootDir | Component::Prefix(_) => {
                    return Err(LocalError::OutsideSandbox(original.to_string()));
                }
            }
        }
        Ok(out)
    }

    /// Resolve a VFS path to an absolute filesystem path inside `root`, rejecting any escape
    /// (`..`, absolute jump, or — for an existing path — a symlink canonicalising outside).
    ///
    /// # Errors
    /// [`LocalError::OutsideSandbox`] if the path escapes; **no** I/O is performed in that
    /// case beyond the containment canonicalisation of an existing prefix.
    pub fn resolve(&self, vfs: &str) -> Result<PathBuf, LocalError> {
        let rel = Self::vfs_to_rel(vfs)?;
        let norm = Self::normalise(&rel, vfs)?;
        let joined = self.root.join(&norm);

        // Re-check containment against symlinks: canonicalise the longest existing ancestor
        // and assert it stays under root. A path that does not exist yet (a write target)
        // canonicalises its parent instead.
        let probe = if joined.exists() {
            &joined
        } else {
            joined.parent().unwrap_or(&self.root)
        };
        if let Ok(canon) = fs::canonicalize(probe) {
            if !canon.starts_with(&self.root) {
                return Err(LocalError::OutsideSandbox(vfs.to_string()));
            }
        }
        Ok(joined)
    }

    /// Convert an absolute filesystem path back into its VFS form (`root/sub/a.md` →
    /// `/local/sub/a.md`). Used to label scan rows. Falls back to the mount root if `abs`
    /// is not under `root` (should not happen for resolved paths).
    fn abs_to_vfs(&self, abs: &StdPath) -> String {
        match abs.strip_prefix(&self.root) {
            Ok(rel) if rel.as_os_str().is_empty() => MOUNT.to_string(),
            Ok(rel) => format!("{MOUNT}/{}", rel.to_string_lossy().replace('\\', "/")),
            Err(_) => MOUNT.to_string(),
        }
    }
}

/// Build a [`LocalRow`] from a resolved absolute path + its `lstat` metadata. Internal:
/// the `std::fs::Metadata` never escapes this function.
fn row_for(sandbox: &Sandbox, abs: &StdPath, meta: &fs::Metadata) -> LocalRow {
    let name = abs
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let modified = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .and_then(|d| i64::try_from(d.as_millis()).ok())
        .unwrap_or(0);
    LocalRow {
        name,
        path: sandbox.abs_to_vfs(abs),
        size: if meta.is_dir() { 0 } else { meta.len() },
        modified,
        is_dir: meta.is_dir(),
        mode: file_mode(meta),
    }
}

/// The Unix permission bits, or 0 on platforms without them.
fn file_mode(meta: &fs::Metadata) -> u32 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.permissions().mode()
    }
    #[cfg(not(unix))]
    {
        let _ = meta;
        0
    }
}

/// Whether a name is a dotfile (leading `.`). By documented default, `*`/`**` do **not**
/// match dotfiles — mirroring shell glob semantics. An explicit dotfile literal still
/// resolves (it is matched directly, not by a wildcard segment).
fn is_dotfile(name: &str) -> bool {
    name.starts_with('.')
}

/// List a single directory (one level), returning owned [`LocalRow`]s sorted by name for
/// deterministic golden snapshots. Dotfiles are excluded (shell-like default).
///
/// # Errors
/// [`LocalError::OutsideSandbox`] / [`LocalError::NotFound`] / [`LocalError::Io`].
pub fn scan_dir(sandbox: &Sandbox, vfs_dir: &str) -> Result<Vec<LocalRow>, LocalError> {
    let abs = sandbox.resolve(vfs_dir)?;
    let entries = fs::read_dir(&abs).map_err(|e| LocalError::from_io(vfs_dir, &e))?;
    let mut rows = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| LocalError::from_io(vfs_dir, &e))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if is_dotfile(&name) {
            continue;
        }
        // lstat (symlink_metadata) — do not follow links when describing the entry itself.
        let meta = entry
            .path()
            .symlink_metadata()
            .map_err(|e| LocalError::from_io(vfs_dir, &e))?;
        rows.push(row_for(sandbox, &entry.path(), &meta));
    }
    rows.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(rows)
}

/// Match a single path segment against a glob token supporting `*`, `?`, and literals.
/// Greedy backtracking matcher; no `**` here (`**` is handled at the directory-walk level).
fn segment_matches(pattern: &str, name: &str) -> bool {
    // Dotfiles never match a wildcard-bearing segment (shell default); a literal does.
    if is_dotfile(name) && pattern.contains(['*', '?']) {
        return false;
    }
    let p: Vec<char> = pattern.chars().collect();
    let n: Vec<char> = name.chars().collect();
    glob_seg(&p, &n)
}

/// Recursive `*`/`?` segment matcher over char slices.
fn glob_seg(p: &[char], n: &[char]) -> bool {
    match p.first() {
        None => n.is_empty(),
        Some('*') => glob_seg(&p[1..], n) || (!n.is_empty() && glob_seg(p, &n[1..])),
        Some('?') => !n.is_empty() && glob_seg(&p[1..], &n[1..]),
        Some(&c) => !n.is_empty() && n[0] == c && glob_seg(&p[1..], &n[1..]),
    }
}

/// Resolve a glob pattern (a VFS path possibly containing `*`, `?`, `**`) to the matching
/// set of files, scoped to `root` and sorted by path (deterministic). `**` matches across
/// directory levels (recursive descent). Symlinked directories are **not** descended, which
/// also bounds symlink cycles. Returns files only (not the directories walked through).
///
/// # Errors
/// [`LocalError::OutsideSandbox`] if the literal prefix escapes; [`LocalError::Io`] on a
/// read failure.
pub fn resolve_glob(sandbox: &Sandbox, pattern: &str) -> Result<Vec<LocalRow>, LocalError> {
    // A pattern with no wildcard is a single path: resolve + lstat it directly.
    if !pattern.contains(['*', '?']) {
        let abs = sandbox.resolve(pattern)?;
        return match abs.symlink_metadata() {
            Ok(meta) => Ok(vec![row_for(sandbox, &abs, &meta)]),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(e) => Err(LocalError::from_io(pattern, &e)),
        };
    }

    let rel = pattern
        .strip_prefix(&format!("{MOUNT}/"))
        .ok_or_else(|| LocalError::OutsideSandbox(pattern.to_string()))?;
    let segments: Vec<&str> = rel.split('/').filter(|s| !s.is_empty()).collect();

    // Validate the literal (wildcard-free) prefix stays in the sandbox before walking.
    sandbox.resolve(MOUNT)?;

    let mut matches = Vec::new();
    walk_glob(
        sandbox,
        sandbox.root().to_path_buf(),
        &segments,
        &mut matches,
    )?;
    matches.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(matches)
}

/// Recursively walk `dir` matching the remaining glob `segments`. `**` consumes zero or
/// more directory levels. Only regular files are collected into `out`.
fn walk_glob(
    sandbox: &Sandbox,
    dir: PathBuf,
    segments: &[&str],
    out: &mut Vec<LocalRow>,
) -> Result<(), LocalError> {
    let Some((head, tail)) = segments.split_first() else {
        return Ok(());
    };

    if *head == "**" {
        // `**` matches the current directory (tail applied here) and every subdirectory.
        walk_glob(sandbox, dir.clone(), tail, out)?;
        for child in read_children(sandbox, &dir)? {
            if child.is_dir {
                let child_abs = sandbox
                    .root()
                    .join(child.path.trim_start_matches(&format!("{MOUNT}/")));
                walk_glob(sandbox, child_abs, segments, out)?;
            }
        }
        return Ok(());
    }

    for child in read_children(sandbox, &dir)? {
        if !segment_matches(head, &child.name) {
            continue;
        }
        if tail.is_empty() {
            if !child.is_dir {
                out.push(child);
            }
        } else if child.is_dir {
            let child_abs = sandbox
                .root()
                .join(child.path.trim_start_matches(&format!("{MOUNT}/")));
            walk_glob(sandbox, child_abs, tail, out)?;
        }
    }
    Ok(())
}

/// Read the immediate children of `dir` as owned rows (dotfiles excluded, links not
/// followed). A directory that does not exist yields an empty set rather than an error, so
/// a glob over a partially-present tree is robust.
fn read_children(sandbox: &Sandbox, dir: &StdPath) -> Result<Vec<LocalRow>, LocalError> {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(LocalError::from_io(&sandbox.abs_to_vfs(dir), &e)),
    };
    let mut rows = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| LocalError::from_io(&sandbox.abs_to_vfs(dir), &e))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if is_dotfile(&name) {
            continue;
        }
        let meta = entry
            .path()
            .symlink_metadata()
            .map_err(|e| LocalError::from_io(&sandbox.abs_to_vfs(&entry.path()), &e))?;
        rows.push(row_for(sandbox, &entry.path(), &meta));
    }
    Ok(rows)
}

/// Stream-read a blob's bytes into an owned `Vec` through the fixed [`COPY_BUF`] buffer
/// (bounded per-iteration memory; the caller owns the final buffer). The decoded relation
/// is produced by a codec downstream — this function only moves bytes.
///
/// # Errors
/// [`LocalError::OutsideSandbox`] / [`LocalError::NotFound`] / [`LocalError::Io`].
pub fn read_blob(sandbox: &Sandbox, vfs: &str) -> Result<Vec<u8>, LocalError> {
    let abs = sandbox.resolve(vfs)?;
    let mut file = File::open(&abs).map_err(|e| LocalError::from_io(vfs, &e))?;
    let mut out = Vec::new();
    let mut buf = [0u8; COPY_BUF];
    loop {
        let n = file
            .read(&mut buf)
            .map_err(|e| LocalError::from_io(vfs, &e))?;
        if n == 0 {
            break;
        }
        out.extend_from_slice(&buf[..n]);
    }
    Ok(out)
}

/// Atomically write `bytes` to `vfs`: stream into a sibling temp file, fsync, then `rename`
/// over the destination (retry-safe; an interrupted write leaves the original intact because
/// the temp file is discarded). Returns the byte count written.
///
/// # Errors
/// [`LocalError::OutsideSandbox`] / [`LocalError::Io`].
pub fn write_blob_atomic(sandbox: &Sandbox, vfs: &str, bytes: &[u8]) -> Result<u64, LocalError> {
    let abs = sandbox.resolve(vfs)?;
    let parent = abs
        .parent()
        .map(StdPath::to_path_buf)
        .unwrap_or_else(|| sandbox.root().to_path_buf());
    fs::create_dir_all(&parent).map_err(|e| LocalError::from_io(vfs, &e))?;

    // A unique sibling temp name keyed on the destination + a process/nanos suffix.
    let tmp = temp_sibling(&abs);
    {
        let mut f = File::create(&tmp).map_err(|e| LocalError::from_io(vfs, &e))?;
        f.write_all(bytes)
            .map_err(|e| LocalError::from_io(vfs, &e))?;
        f.sync_all().map_err(|e| LocalError::from_io(vfs, &e))?;
    }
    fs::rename(&tmp, &abs).map_err(|e| {
        // Best-effort cleanup of the temp file on a failed rename; ignore its result so the
        // original error is the one surfaced.
        let _ = fs::remove_file(&tmp);
        LocalError::from_io(vfs, &e)
    })?;
    Ok(bytes.len() as u64)
}

/// A sibling temp path next to `dst` (`<dst>.cfs-tmp.<nanos>`), so the atomic `rename` stays
/// on the same filesystem (cross-device rename would fail).
fn temp_sibling(dst: &StdPath) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let mut name = dst
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_default();
    name.push(format!(".cfs-tmp.{nanos}"));
    dst.with_file_name(name)
}

/// Stream-copy `src` → `dst` through the fixed buffer, computing a verify byte-length
/// incrementally, then assert `dst` size matches `src` size (the copy→verify shape, RFD §6).
/// Returns the byte count copied. Does **not** delete the source (that is `mv`'s extra step).
///
/// # Errors
/// [`LocalError`] on resolve/IO failure, or [`LocalError::VerifyFailed`] if the destination
/// size does not match the source after the copy.
pub fn copy_verify(sandbox: &Sandbox, src_vfs: &str, dst_vfs: &str) -> Result<u64, LocalError> {
    let src = sandbox.resolve(src_vfs)?;
    let dst = sandbox.resolve(dst_vfs)?;
    let expected = src
        .metadata()
        .map_err(|e| LocalError::from_io(src_vfs, &e))?
        .len();

    let parent = dst
        .parent()
        .map(StdPath::to_path_buf)
        .unwrap_or_else(|| sandbox.root().to_path_buf());
    fs::create_dir_all(&parent).map_err(|e| LocalError::from_io(dst_vfs, &e))?;

    let tmp = temp_sibling(&dst);
    let mut reader = File::open(&src).map_err(|e| LocalError::from_io(src_vfs, &e))?;
    let mut written: u64 = 0;
    {
        let mut writer = File::create(&tmp).map_err(|e| LocalError::from_io(dst_vfs, &e))?;
        let mut buf = [0u8; COPY_BUF];
        loop {
            let n = reader
                .read(&mut buf)
                .map_err(|e| LocalError::from_io(src_vfs, &e))?;
            if n == 0 {
                break;
            }
            writer
                .write_all(&buf[..n])
                .map_err(|e| LocalError::from_io(dst_vfs, &e))?;
            written += n as u64;
        }
        writer
            .sync_all()
            .map_err(|e| LocalError::from_io(dst_vfs, &e))?;
    }

    // Verify BEFORE publishing: size match (the incremental byte count vs. the source len).
    if written != expected {
        let _ = fs::remove_file(&tmp);
        return Err(LocalError::VerifyFailed {
            dst: dst_vfs.to_string(),
            expected,
            found: written,
        });
    }
    fs::rename(&tmp, &dst).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        LocalError::from_io(dst_vfs, &e)
    })?;
    Ok(written)
}

/// Remove a blob (file). A directory or missing path is rejected/structured.
///
/// # Errors
/// [`LocalError::OutsideSandbox`] / [`LocalError::NotFound`] / [`LocalError::Io`].
pub fn remove_blob(sandbox: &Sandbox, vfs: &str) -> Result<(), LocalError> {
    let abs = sandbox.resolve(vfs)?;
    fs::remove_file(&abs).map_err(|e| LocalError::from_io(vfs, &e))
}
