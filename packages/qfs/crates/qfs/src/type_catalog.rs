//! The `/type` composition root: the System-DB-backed catalog reader + the async
//! [`TypeReadDriver`] read facet, both hosted in the **`qfs` binary crate**.
//!
//! Like `/transform` (see [`crate::transform`]), `qfs-driver-type` is vendor-free and DB-free, and
//! the binary is the ONE place that opens a real DB path (decision F). So the real `rusqlite` read
//! over the declared-driver registry's `kind='type'` rows dead-ends here; no `rusqlite` type crosses
//! the facet boundary (owned qfs DTOs only).
//!
//! ## Read-only by construction
//! Unlike `/transform`, there is no backend *write* seam and no applier: a declared type is
//! installed/removed by a previewed write to `/sys/drivers` (`kind='type'`), the one table `CREATE
//! TYPE` desugars into (blueprint §5.5). This module is purely the read face over those same rows —
//! `ls /type` is SHOW TYPES.
//!
//! ## The catalog lists NAMES, not the stored paths
//! `sys_drivers.name` stores a declared type's key in its **path** form (`/type/chatwork/message` —
//! the key `of` normalises a bare name into, and the label its errors carry). But the catalog's
//! `name` column is the REFERENCE face (§5.5): what you may paste into `of <name>` or a column type.
//! So the scan strips the `/type/` prefix back to the name (`chatwork/message`) — listing the raw
//! stored path would print the one spelling the grammar REJECTS (`of /type/x` is a parse error).
//!
//! ## Newest declaration wins
//! `sys_drivers` is append-shaped: re-installing a type inserts a *second* row with the same name.
//! The resolution paths (`load_declared_types` / `load_declared_type_defs` / the describe path) all
//! take `ORDER BY id DESC` newest-first, so the catalog listing does the same and collapses to ONE
//! row per name. A listing that showed the superseded rows too would contradict what `of <name>`
//! actually resolves.

use std::sync::{Arc, Mutex};

use qfs_core::{CfsError, RowBatch};
use qfs_driver_type::{name_from_path, node_for_path, type_node_schema, TypeNode};
use qfs_exec::ReadDriver;
use qfs_pushdown::ScanNode;
use qfs_types::{Row, Value};
use rusqlite::Connection;

/// The System-DB-backed `/type` catalog reader: the real rusqlite read over `sys_drivers`'
/// `kind='type'` rows. The connection is held behind a `Mutex` (rusqlite is `!Sync`; the mutex
/// provides `Send + Sync`).
pub struct TypeDbBackend {
    system: Mutex<Connection>,
}

impl TypeDbBackend {
    /// Build a reader over an already-migrated System-DB connection (the test + composition seam).
    #[must_use]
    pub fn new(system: Connection) -> Self {
        Self {
            system: Mutex::new(system),
        }
    }

    /// Open the default System DB and build the reader. Returns `None` when no config home resolves
    /// — the `/type` READ surface is then simply not wired (the mount still describes, cred-free,
    /// so `describe /type` keeps teaching the shape), the same best-effort posture as
    /// [`crate::transform::TransformDbBackend::open_default`].
    #[must_use]
    pub fn open_default() -> Option<Self> {
        match crate::store::open_system_db() {
            Ok(Some(sys)) => Some(Self::new(sys.into_db().into_connection())),
            _ => None,
        }
    }

    /// Scan the declared-type catalog into the `/type` relation's rows: one row per declared type,
    /// newest declaration winning, ordered by name.
    fn scan_rows(&self) -> Result<RowBatch, String> {
        let schema = type_node_schema(TypeNode::Catalog);
        let conn = self.system.lock().map_err(|_| "system db lock poisoned")?;
        // Newest first so the by-name collapse below keeps the WINNING declaration — the same
        // `ORDER BY id DESC` rule `load_declared_type_defs` resolves `of <name>` with.
        let mut stmt = conn
            .prepare(
                "SELECT name, body, created_at FROM sys_drivers \
                 WHERE kind = 'type' ORDER BY id DESC",
            )
            .map_err(|_| "type_read_failed")?;
        let mapped = stmt
            .query_map([], |r| {
                let name: String = r.get(0)?;
                let body: Option<String> = r.get(1)?;
                let created_at: Option<String> = r.get(2)?;
                Ok((name, body.unwrap_or_default(), created_at))
            })
            .map_err(|_| "type_read_failed")?;

        // Collapse to one row per name (first seen = newest), then order by name for a stable,
        // human-readable `ls`.
        let mut seen: std::collections::BTreeMap<String, Row> = std::collections::BTreeMap::new();
        for entry in mapped {
            let (stored, body, created_at) = entry.map_err(|_| "type_read_failed")?;
            let name = reference_name(&stored);
            seen.entry(name.clone()).or_insert_with(|| {
                Row::new(vec![
                    Value::Text(name),
                    Value::Text(columns_json(&body)),
                    refinement_json(&body).map_or(Value::Null, Value::Text),
                    created_at.map_or(Value::Null, Value::Text),
                ])
            });
        }
        Ok(RowBatch::new(schema, seen.into_values().collect()))
    }
}

/// A stored `sys_drivers.name` (a `/type/...` path key) rendered back to its REFERENCE name — the
/// spelling `of <name>` takes (§5.5). A row that somehow carries a bare name already (a legacy or
/// hand-inserted row) is passed through unchanged rather than dropped: the catalog's job is to
/// report what is declared, honestly.
fn reference_name(stored: &str) -> String {
    name_from_path(stored).unwrap_or_else(|| stored.to_string())
}

/// The declared column descriptors of a stored `CREATE TYPE` body, re-rendered as JSON text
/// (blueprint §5.4: the body is a JSON OBJECT with a `columns` array and a `where` predicate slot).
/// A malformed/legacy body degrades to an empty array rather than failing the whole listing — the
/// same best-effort posture `type_column_names` takes.
fn columns_json(body_json: &str) -> String {
    serde_json::from_str::<serde_json::Value>(body_json)
        .ok()
        .and_then(|v| v.get("columns").cloned())
        .map_or_else(|| "[]".to_string(), |c| c.to_string())
}

/// The optional row-local refinement predicate of a stored body, as its stored AST JSON. `None` for
/// a purely structural type (a `null`/absent `where` slot) — the catalog's `refinement` column is
/// then NULL.
fn refinement_json(body_json: &str) -> Option<String> {
    let v = serde_json::from_str::<serde_json::Value>(body_json).ok()?;
    let w = v.get("where")?;
    (!w.is_null()).then(|| w.to_string())
}

/// The async read facet (the analogue of [`crate::transform::TransformReadDriver`]): adapts the
/// synchronous catalog scan to qfs-exec's [`ReadDriver`] seam. Lives in the binary because
/// `ReadDriver` is a qfs-exec type the driver crate must stay off (dep direction).
pub struct TypeReadDriver {
    backend: Arc<TypeDbBackend>,
}

impl TypeReadDriver {
    /// Build the read adapter over an injected catalog reader.
    #[must_use]
    pub fn new(backend: Arc<TypeDbBackend>) -> Self {
        Self { backend }
    }
}

#[async_trait::async_trait]
impl ReadDriver for TypeReadDriver {
    async fn scan(&self, scan: &ScanNode) -> Result<RowBatch, CfsError> {
        node_for_path(&scan.path).ok_or_else(|| CfsError::InvalidPath {
            path: scan.path.clone(),
            reason: "not a /type path",
        })?;
        self.backend.scan_rows().map_err(|_| CfsError::InvalidPath {
            path: scan.path.clone(),
            reason: "type_read_failed",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An in-memory System DB carrying just the `sys_drivers` shape the catalog reads — hermetic,
    /// no config home, no migration run.
    fn conn_with_types(rows: &[(&str, &str)]) -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE sys_drivers (
                 id         INTEGER PRIMARY KEY,
                 kind       TEXT NOT NULL,
                 name       TEXT NOT NULL,
                 body       TEXT,
                 created_at TEXT
             );",
        )
        .unwrap();
        for (name, body) in rows {
            conn.execute(
                "INSERT INTO sys_drivers (kind, name, body, created_at) \
                 VALUES ('type', ?1, ?2, '2026-07-14T00:00:00Z')",
                rusqlite::params![name, body],
            )
            .unwrap();
        }
        conn
    }

    fn text(row: &Row, i: usize) -> Option<&str> {
        match &row.values[i] {
            Value::Text(s) => Some(s.as_str()),
            _ => None,
        }
    }

    #[test]
    fn scan_lists_the_reference_name_not_the_stored_path() {
        // `sys_drivers` stores a type's key in its PATH form; the catalog's `name` column is the
        // REFERENCE face (§5.5) — the spelling `of <name>` takes. Listing `/type/customer` would
        // print the one spelling the grammar REJECTS (`of /type/x` is a parse error).
        let backend = TypeDbBackend::new(conn_with_types(&[
            (
                "/type/customer",
                r#"{"columns":[{"name":"id","type":"int"}],"where":null}"#,
            ),
            (
                "/type/chatwork/message",
                r#"{"columns":[{"name":"body","type":"text"}],"where":null}"#,
            ),
        ]));
        let batch = backend.scan_rows().unwrap();
        assert_eq!(batch.rows.len(), 2);
        // Ordered by name; a qualified (nested-catalog) name keeps BOTH segments — only the mount
        // prefix is stripped.
        assert_eq!(text(&batch.rows[0], 0), Some("chatwork/message"));
        assert_eq!(text(&batch.rows[1], 0), Some("customer"));
        // The shape rides along as the declared column JSON.
        assert!(text(&batch.rows[1], 1).unwrap().contains("\"id\""));
        // No refinement declared -> NULL, not the JSON literal `null`.
        assert_eq!(batch.rows[1].values[2], Value::Null);
    }

    #[test]
    fn a_reinstalled_type_lists_once_with_the_newest_declaration() {
        // §5.4/`load_declared_type_defs`: `sys_drivers` is append-shaped, so re-installing a type
        // leaves BOTH rows. The listing must collapse to the newest (highest id) — the one
        // `of customer` actually resolves — never show the superseded declaration too.
        let backend = TypeDbBackend::new(conn_with_types(&[
            (
                "/type/customer",
                r#"{"columns":[{"name":"old","type":"text"}],"where":null}"#,
            ),
            (
                "/type/customer",
                r#"{"columns":[{"name":"new","type":"text"}],"where":null}"#,
            ),
        ]));
        let batch = backend.scan_rows().unwrap();
        assert_eq!(batch.rows.len(), 1);
        let cols = text(&batch.rows[0], 1).unwrap();
        assert!(
            cols.contains("\"new\""),
            "newest declaration must win: {cols}"
        );
        assert!(!cols.contains("\"old\""));
    }

    #[test]
    fn a_refinement_is_surfaced_and_a_malformed_body_does_not_fail_the_listing() {
        let backend = TypeDbBackend::new(conn_with_types(&[
            (
                "/type/email",
                r#"{"columns":[{"name":"value","type":"text"}],"where":{"Like":{"expr":{"Col":"value"},"pattern":{"Lit":{"Str":"%@%"}}}}}"#,
            ),
            // A legacy/pre-§5.4 body (a bare column array, not the object form): best-effort, it
            // still lists — with an empty shape — rather than erroring the whole scan.
            ("/type/legacy", r#"[{"name":"x","type":"text"}]"#),
        ]));
        let batch = backend.scan_rows().unwrap();
        assert_eq!(batch.rows.len(), 2);
        assert!(text(&batch.rows[0], 2).unwrap().contains("Like"));
        assert_eq!(text(&batch.rows[1], 0), Some("legacy"));
        assert_eq!(text(&batch.rows[1], 1), Some("[]"));
        assert_eq!(batch.rows[1].values[2], Value::Null);
    }
}
