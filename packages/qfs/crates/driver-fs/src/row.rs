//! [`FsRow`] — the schema DTO for a `/fs` directory listing entry (blueprint §6). Owned data
//! only; no `std::fs::DirEntry` crosses the driver boundary (blueprint §11 no-vendor-leak).
//!
//! Structurally identical to the t28 `/local` listing row — the `fs` driver is templated on
//! `qfs-driver-local` — but its `path` carries the `/fs/<root>/…` shape (an operator-named
//! root segment) rather than the fixed `/local/…` sandbox prefix.

use qfs_types::{Column, ColumnType, Row, Schema, Value};

/// One entry in a `/fs` directory/glob listing — the row a `/fs/<root>/<dir>` scan yields.
/// Fields mirror `lstat`/`metadata`; `mode` is the Unix permission bits (0 on platforms
/// without them). Owned, vendor-free.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct FsRow {
    /// The entry's final path component (file/dir/root name).
    pub name: String,
    /// The entry's VFS path within the mount, e.g. `/fs/projects/src/a.md`.
    pub path: String,
    /// The byte length (0 for directories).
    pub size: u64,
    /// The modified time as epoch milliseconds (0 if unavailable).
    pub modified: i64,
    /// Whether the entry is a directory.
    pub is_dir: bool,
    /// The Unix permission bits (e.g. `0o644`); 0 where the platform has none.
    pub mode: u32,
}

impl FsRow {
    /// The canonical listing [`Schema`] — the typed columns `describe` reports and the `scan`
    /// rows conform to. Stable column order powers deterministic golden snapshots. Identical to
    /// the `/local` listing schema, so a `/fs` blob is queryable with the same shape.
    #[must_use]
    pub fn schema() -> Schema {
        Schema::new(vec![
            Column::new("name", ColumnType::Text, false),
            Column::new("path", ColumnType::Text, false),
            Column::new("size", ColumnType::Int, false),
            Column::new("modified", ColumnType::Timestamp, false),
            Column::new("is_dir", ColumnType::Bool, false),
            Column::new("mode", ColumnType::Int, false),
        ])
    }

    /// The single-file **content** schema: the listing columns plus a nullable `content`
    /// ([`ColumnType::Bytes`]) column carrying the file's raw bytes. A single-file `/fs/<file>`
    /// read returns this so a downstream codec (`DECODE`/`ENCODE`) or `transform` can consume the
    /// bytes; a directory/glob listing carries the same schema with a null `content` so plan and
    /// runtime agree. Mirrors `qfs-driver-local`'s `LocalRow::content_schema`.
    #[must_use]
    pub fn content_schema() -> Schema {
        let mut cols = Self::schema().columns;
        cols.push(Column::new("content", ColumnType::Bytes, true));
        Schema::new(cols)
    }

    /// Project this row onto the canonical [`FsRow::schema`] column order as a typed [`Row`].
    #[must_use]
    pub fn to_row(&self) -> Row {
        Row::new(vec![
            Value::Text(self.name.clone()),
            Value::Text(self.path.clone()),
            Value::Int(i64::try_from(self.size).unwrap_or(i64::MAX)),
            Value::Timestamp(self.modified),
            Value::Bool(self.is_dir),
            Value::Int(i64::from(self.mode)),
        ])
    }
}
