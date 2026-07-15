//! The `/fs` driver's **read scan**: translate a VFS path + the pushed query into the owned
//! listing [`RowBatch`] the read executor consumes — the read counterpart of the
//! [`applier`](crate::applier) write leg. Templated on `qfs-driver-local`'s `read::scan_rows`.
//!
//! It owns no async and no `qfs-exec` dependency (keeping the driver crate off the integration
//! layer): a pure, synchronous `VFS path -> RowBatch` over the [`fs_core`](crate::fs_core) scan
//! primitives. The async `qfs_exec::ReadDriver` adapter that drives this lives in the **`qfs`
//! binary crate** (the composition root), exactly like `/local`.
//!
//! ## Pushdown honesty
//! The fs driver declares `Partial { project: true }`: it can honour a projection but not a
//! `WHERE`/`LIMIT`. This scan returns the **full listing** (over-returning is allowed) and lets
//! the executor's residual re-filter trim it. We apply the projection when the pushed query
//! carries one (cheap, keeps the returned schema honest), but correctness never depends on it.

use qfs_types::{Name, Row, RowBatch, Schema, Value};

use crate::error::FsError;
use crate::fs_core::{self, FsRoots};
use crate::row::FsRow;

/// Scan `vfs` into the owned listing [`RowBatch`] (the `FsRow` schema), optionally narrowed to
/// `project` columns. Dispatches on the path shape:
/// - a **glob** (`*`/`?`/`**`) → [`fs_core::resolve_glob`] matches;
/// - the **bare mount `/fs`** → the configured root NAMES (virtual directories);
/// - an **existing directory** → [`fs_core::scan_dir`] (one level);
/// - otherwise a **single entry** (a file, or a non-existent path → empty).
///
/// Over-returns relative to any unpushable predicate/limit (the executor's residual trims it).
///
/// # Errors
/// [`FsError`] on a confinement breach or an I/O failure (a missing path is **not** an error — it
/// yields an empty batch, so a scan over a partially-present tree is robust).
pub fn scan_rows(
    roots: &FsRoots,
    vfs: &str,
    project: Option<&[Name]>,
) -> Result<RowBatch, FsError> {
    // A single existing file reads its *content*: the listing row plus a `content` (Bytes) column,
    // so a downstream codec (`DECODE`/`ENCODE`) or `transform` can consume the bytes. Directory and
    // glob listings fall through and carry the SAME schema with a null `content` (below).
    if let Some(batch) = scan_file_content(roots, vfs)? {
        return Ok(apply_project(batch, project));
    }
    // A directory / glob listing carries the SAME schema as the single-file read and describe()
    // ([`FsRow::content_schema`]) so plan-time and runtime agree, but its `content` is null: a
    // listing does not materialise each entry's bytes (read the single file path to get them).
    let fs_rows = scan_fs_rows(roots, vfs)?;
    let rows: Vec<Row> = fs_rows
        .iter()
        .map(|fr| {
            let mut values = fr.to_row().values;
            values.push(Value::Null);
            Row::new(values)
        })
        .collect();
    Ok(apply_project(
        RowBatch::new(FsRow::content_schema(), rows),
        project,
    ))
}

/// Apply an optional projection to `batch`, preserving requested column order. A `None`/empty
/// projection returns the batch unchanged.
fn apply_project(batch: RowBatch, project: Option<&[Name]>) -> RowBatch {
    match project {
        Some(cols) if !cols.is_empty() => project_batch(&batch, cols),
        _ => batch,
    }
}

/// If `vfs` is a non-glob path that resolves to a single **regular file**, read its bytes and
/// return the listing row augmented with a `content` (Bytes) column ([`FsRow::content_schema`]).
/// Returns `None` for globs, directories, and missing paths — those fall through to the listing
/// scan. A confinement breach surfaces as a [`FsError`] (propagated, not `None`).
fn scan_file_content(roots: &FsRoots, vfs: &str) -> Result<Option<RowBatch>, FsError> {
    if vfs.contains(['*', '?']) {
        return Ok(None);
    }
    // The bare mount is a namespace of root names, never a single file.
    if vfs == "/fs" {
        return Ok(None);
    }
    let abs = roots.resolve(vfs)?;
    let is_file = matches!(abs.symlink_metadata(), Ok(meta) if meta.is_file());
    if !is_file {
        return Ok(None);
    }
    let listing = fs_core::resolve_glob(roots, vfs)?;
    let bytes = fs_core::read_blob(roots, vfs)?;
    let rows: Vec<Row> = listing
        .iter()
        .map(|fr| {
            let mut values = fr.to_row().values;
            values.push(Value::Bytes(bytes.clone()));
            Row::new(values)
        })
        .collect();
    Ok(Some(RowBatch::new(FsRow::content_schema(), rows)))
}

/// Resolve `vfs` to its listing [`FsRow`]s (the shape-dispatch above), pure over `fs_core`.
fn scan_fs_rows(roots: &FsRoots, vfs: &str) -> Result<Vec<FsRow>, FsError> {
    if vfs.contains(['*', '?']) {
        return fs_core::resolve_glob(roots, vfs);
    }
    // The bare mount lists the configured root names (each a virtual directory).
    if vfs == "/fs" {
        return fs_core::scan_dir(roots, vfs);
    }
    // Probe the resolved path: a directory lists its children; a file lists itself.
    let abs = roots.resolve(vfs)?;
    match abs.symlink_metadata() {
        Ok(meta) if meta.is_dir() => fs_core::scan_dir(roots, vfs),
        Ok(_) => fs_core::resolve_glob(roots, vfs),
        // A path that does not exist yet yields an empty listing (robust, not an error).
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(FsError::from_io(vfs, &e)),
    }
}

/// Project `batch` onto the `cols` subset, preserving the requested column order. A requested
/// column absent from the listing schema is dropped (the executor's residual would reject an
/// impossible projection earlier; this stays total).
fn project_batch(batch: &RowBatch, cols: &[Name]) -> RowBatch {
    let src = &batch.schema;
    let picks: Vec<(usize, &qfs_types::Column)> = cols
        .iter()
        .filter_map(|name| {
            src.columns
                .iter()
                .position(|c| c.name.as_str() == name.as_str())
                .map(|i| (i, &src.columns[i]))
        })
        .collect();
    let schema = Schema::new(picks.iter().map(|(_, c)| (*c).clone()).collect());
    let rows = batch
        .rows
        .iter()
        .map(|r| {
            Row::new(
                picks
                    .iter()
                    .map(|(i, _)| r.values.get(*i).cloned().unwrap_or(Value::Null))
                    .collect(),
            )
        })
        .collect();
    RowBatch::new(schema, rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn fixture() -> (TempDir, FsRoots) {
        let dir = TempDir::new().expect("tempdir");
        std::fs::write(dir.path().join("a.md"), b"alpha").unwrap();
        std::fs::write(dir.path().join("b.txt"), b"beta").unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        std::fs::write(dir.path().join("sub").join("c.md"), b"gamma").unwrap();
        let roots = FsRoots::new().with_root("projects", dir.path());
        (dir, roots)
    }

    #[test]
    fn bare_mount_lists_configured_roots() {
        let (_d, roots) = fixture();
        let batch = scan_rows(&roots, "/fs", None).unwrap();
        let names: Vec<String> = batch
            .rows
            .iter()
            .filter_map(|r| match &r.values[0] {
                Value::Text(s) => Some(s.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(names, vec!["projects"]);
    }

    #[test]
    fn scans_root_listing() {
        let (_d, roots) = fixture();
        let batch = scan_rows(&roots, "/fs/projects", None).unwrap();
        let names: Vec<String> = batch
            .rows
            .iter()
            .filter_map(|r| match &r.values[0] {
                Value::Text(s) => Some(s.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(names, vec!["a.md", "b.txt", "sub"]);
        assert_eq!(
            batch.schema.columns.len(),
            7,
            "full FsRow content schema (listing + null content)"
        );
    }

    #[test]
    fn single_file_returns_one_row_with_fs_path() {
        let (_d, roots) = fixture();
        let batch = scan_rows(&roots, "/fs/projects/a.md", None).unwrap();
        assert_eq!(batch.rows.len(), 1);
        match &batch.rows[0].values[1] {
            Value::Text(s) => assert_eq!(s, "/fs/projects/a.md"),
            other => panic!("expected fs path text, got {other:?}"),
        }
    }

    #[test]
    fn single_file_read_materialises_content_bytes() {
        let (_d, roots) = fixture();
        let batch = scan_rows(&roots, "/fs/projects/a.md", None).unwrap();
        assert_eq!(batch.schema.columns.len(), 7, "listing + content column");
        assert_eq!(batch.schema.columns[6].name.as_str(), "content");
        assert_eq!(batch.rows.len(), 1);
        match batch.rows[0].values.last() {
            Some(Value::Bytes(b)) => assert_eq!(b.as_slice(), b"alpha"),
            other => panic!("expected content bytes, got {other:?}"),
        }
    }

    #[test]
    fn directory_listing_has_a_null_content_column() {
        let (_d, roots) = fixture();
        let batch = scan_rows(&roots, "/fs/projects", None).unwrap();
        // The listing schema now matches the single-file read + describe() (plan==runtime), but a
        // listing does not materialise each entry's bytes: `content` is present-but-null.
        assert_eq!(batch.schema.columns.len(), 7, "listing + content column");
        assert_eq!(batch.schema.columns[6].name.as_str(), "content");
        assert!(
            batch
                .rows
                .iter()
                .all(|r| matches!(r.values.last(), Some(Value::Null))),
            "a directory listing carries a null content per entry"
        );
    }

    #[test]
    fn missing_path_is_empty_not_error() {
        let (_d, roots) = fixture();
        let batch = scan_rows(&roots, "/fs/projects/nope", None).unwrap();
        assert!(batch.rows.is_empty());
    }

    #[test]
    fn projection_narrows_columns() {
        let (_d, roots) = fixture();
        let cols = vec![Name::from("name"), Name::from("size")];
        let batch = scan_rows(&roots, "/fs/projects", Some(&cols)).unwrap();
        assert_eq!(batch.schema.columns.len(), 2);
        assert_eq!(batch.schema.columns[0].name.as_str(), "name");
        assert_eq!(batch.schema.columns[1].name.as_str(), "size");
    }

    #[test]
    fn glob_matches_files() {
        let (_d, roots) = fixture();
        let batch = scan_rows(&roots, "/fs/projects/*.md", None).unwrap();
        assert_eq!(batch.rows.len(), 1, "only top-level a.md matches *.md");
    }

    #[test]
    fn unknown_root_is_denied() {
        let (_d, roots) = fixture();
        let err = scan_rows(&roots, "/fs/secrets/key", None).unwrap_err();
        assert_eq!(err.code(), "unknown_root");
    }

    #[test]
    fn parent_escape_is_refused() {
        let (_d, roots) = fixture();
        let err = scan_rows(&roots, "/fs/projects/../../etc", None).unwrap_err();
        assert_eq!(err.code(), "outside_root");
    }
}
