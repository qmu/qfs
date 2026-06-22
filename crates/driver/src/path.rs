//! [`Path`] — a cfs virtual path (RFD-0001 §2.1 VFS: "a path is just a query that
//! resolves to a set").
//!
//! E0 ships an owned, opaque wrapper around the path string. Structured parsing
//! (mount segment, `@version` temporal coordinate §4, attribute predicates) lands
//! in later epics; the type exists now so the `Driver` trait signatures are stable.
//! Owned data only — no borrowing of vendor types (owned-DTO discipline, §9).

/// A cfs virtual path, e.g. `/mail/inbox`, `/s3/bucket/key@versionId`,
/// `/git/repo@ref/path`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Path {
    raw: String,
}

impl Path {
    /// Construct a path from an owned string.
    #[must_use]
    pub fn new(raw: impl Into<String>) -> Self {
        Self { raw: raw.into() }
    }

    /// The raw path text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.raw
    }
}

impl From<&str> for Path {
    fn from(s: &str) -> Self {
        Self::new(s)
    }
}
