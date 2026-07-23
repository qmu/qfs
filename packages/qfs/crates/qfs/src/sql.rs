//! The `qfs run`/commit SQL composition root: the production, file-backed SQLite [`SqlBackend`]
//! plus the live [`ConnRegistry`] the binary plans + commits `/sql/<conn>/...` statements against.
//!
//! ## Why the backend lives in the binary (not a leaf crate)
//! `qfs-driver-sql` is the vendor-free driver (the `SqlBackend` trait + the dialect-rendered
//! compiler) and is a **`qfs-runtime` consumer** (its `sql_apply_driver` bridges to the runtime).
//! The dep_direction guard requires every runtime consumer to be a **leaf** — only the terminal
//! `qfs` binary may depend onto it — so a *separate* backend crate depending back onto
//! `qfs-driver-sql` would (structurally) un-leaf it. The binary IS that terminal leaf, so the real
//! `rusqlite` engine (and its `libsqlite3` C build) dead-ends here, exactly like tokio + the
//! reqwest transport. `qfs-cmd`/`qfs-exec` stay off both. No `rusqlite` type crosses the
//! `SqlBackend` boundary (owned qfs DTOs only).
//!
//! Guarantees: every value is a **bound** parameter (injection-safe — `'; DROP TABLE` is inert
//! data); a multi-op commit is one **ACID** transaction (`BEGIN -> ops -> COMMIT`, auto-`ROLLBACK`
//! on any error).
//!
//! ## Config (no credentials in argv/logs)
//! A connection is declared with `CONNECT /sql/<conn> TO sqlite|postgres|mysql AT '<loc>' [SECRET
//! '<ref>']` (or `qfs connect`), persisted in the project-DB `path_binding` registry — the SINGLE
//! source (the retired `QFS_SQL_*` env var and `connections.qfs` loader are gone; experimental, no
//! backward compat). The `<conn>` segment after `/sql/` is the `/sql/<conn>/...` path segment. A
//! connection whose file cannot be opened or introspected is skipped, so a `/sql/<conn>` commit for
//! an unresolvable conn fails closed.

use std::path::Path;
use std::sync::{Arc, Mutex};

use qfs_driver_sql::{
    render_dml, Catalog, ColumnDef, ConnHandle, ConnRegistry, Dialect, DmlOp, Param, RelationKind,
    SqlBackend, SqlDriver, SqlError, TableCatalog,
};
use qfs_types::{Row, Value};

/// A live SQLite backend wrapping a `rusqlite::Connection` behind a `Mutex` (the connection is
/// `!Sync`; the mutex provides the `Send + Sync` the trait requires). Opens a real database file,
/// so commits persist.
pub struct SqliteBackend {
    conn: Mutex<rusqlite::Connection>,
}

impl SqliteBackend {
    /// Open (creating if absent) the SQLite database at `path`.
    fn open(path: impl AsRef<Path>) -> Result<Self, SqlError> {
        let conn = rusqlite::Connection::open(path.as_ref())
            .map_err(|e| SqlError::backend("sqlite", "open", e.to_string()))?;
        // Arm a bounded busy wait so a commit against a USER `/sql` file that another process holds a
        // lock on WAITS rather than failing `SQLITE_BUSY` immediately (same busy-handler protection
        // as the qfs system/project DBs — ticket 20260709024731). Note: NO `journal_mode=WAL` here —
        // unlike qfs's own store DBs, the journal mode is a persistent property of the *user's* file
        // and is not qfs's to rewrite; only the per-connection busy handler is set.
        conn.busy_timeout(std::time::Duration::from_secs(5))
            .map_err(|e| SqlError::backend("sqlite", "open", e.to_string()))?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }
}

/// Bind a `&[Param]` onto a rusqlite statement positionally — the proof that every value is BOUND,
/// never interpolated.
fn bind_params<'a>(params: &'a [Param]) -> Vec<Box<dyn rusqlite::ToSql + 'a>> {
    params
        .iter()
        .map(|p| -> Box<dyn rusqlite::ToSql + 'a> {
            match p {
                Param::Null => Box::new(rusqlite::types::Null),
                Param::Bool(b) => Box::new(*b),
                Param::Int(n) => Box::new(*n),
                Param::Float(f) => Box::new(*f),
                Param::Text(t) => Box::new(t.as_str()),
                Param::Bytes(b) => Box::new(b.as_slice()),
            }
        })
        .collect()
}

/// Convert a rusqlite value reference into the canonical qfs [`Value`] (the owned-DTO boundary —
/// no rusqlite type crosses past here).
fn sqlite_value_to_qfs(v: rusqlite::types::ValueRef<'_>) -> Value {
    use rusqlite::types::ValueRef;
    match v {
        ValueRef::Null => Value::Null,
        ValueRef::Integer(n) => Value::Int(n),
        ValueRef::Real(f) => Value::Float(f),
        ValueRef::Text(t) => Value::Text(String::from_utf8_lossy(t).into_owned()),
        ValueRef::Blob(b) => Value::Bytes(b.to_vec()),
    }
}

impl SqlBackend for SqliteBackend {
    fn dialect(&self) -> Dialect {
        Dialect::Sqlite
    }

    fn introspect(&self) -> Result<Catalog, SqlError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| SqlError::backend("sqlite", "lock", "poisoned connection mutex"))?;
        let mut tables = Vec::new();
        let mut stmt = conn
            .prepare("SELECT name, type FROM sqlite_master WHERE type IN ('table','view') AND name NOT LIKE 'sqlite_%' ORDER BY name")
            .map_err(|e| SqlError::backend("sqlite", "introspect", e.to_string()))?;
        let rels: Vec<(String, String)> = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
            .map_err(|e| SqlError::backend("sqlite", "introspect", e.to_string()))?
            .collect::<Result<_, _>>()
            .map_err(|e| SqlError::backend("sqlite", "introspect", e.to_string()))?;
        drop(stmt);

        for (name, kind) in rels {
            let mut cols = Vec::new();
            let mut info = conn
                .prepare(&format!("PRAGMA table_info(\"{name}\")"))
                .map_err(|e| SqlError::backend("sqlite", "introspect", e.to_string()))?;
            // PRAGMA table_info columns: cid, name, type, notnull, dflt_value, pk.
            let rows: Vec<(String, String, i64, i64)> = info
                .query_map([], |r| {
                    Ok((
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, i64>(3)?,
                        r.get::<_, i64>(5)?,
                    ))
                })
                .map_err(|e| SqlError::backend("sqlite", "introspect", e.to_string()))?
                .collect::<Result<_, _>>()
                .map_err(|e| SqlError::backend("sqlite", "introspect", e.to_string()))?;
            drop(info);
            for (col_name, ty, notnull, pk) in rows {
                let mapped = Dialect::Sqlite.map_type(&ty);
                let is_pk = pk > 0;
                cols.push(ColumnDef::new(col_name, mapped, notnull == 0, is_pk, is_pk));
            }
            let relkind = if kind == "view" {
                RelationKind::View
            } else {
                RelationKind::Table
            };
            tables.push(TableCatalog::new(name, relkind, cols));
        }
        Ok(Catalog::new(tables))
    }

    fn execute_read(&self, sql: &str, params: &[Param]) -> Result<Vec<Row>, SqlError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| SqlError::backend("sqlite", "lock", "poisoned connection mutex"))?;
        let mut stmt = conn
            .prepare(sql)
            .map_err(|e| SqlError::backend("sqlite", "select", e.to_string()))?;
        let col_count = stmt.column_count();
        let boxed = bind_params(params);
        let refs: Vec<&dyn rusqlite::ToSql> = boxed.iter().map(|b| b.as_ref()).collect();
        let mut rows = stmt
            .query(refs.as_slice())
            .map_err(|e| SqlError::backend("sqlite", "select", e.to_string()))?;
        let mut out = Vec::new();
        while let Some(r) = rows
            .next()
            .map_err(|e| SqlError::backend("sqlite", "select", e.to_string()))?
        {
            let mut values = Vec::with_capacity(col_count);
            for i in 0..col_count {
                let v = r
                    .get_ref(i)
                    .map_err(|e| SqlError::backend("sqlite", "select", e.to_string()))?;
                values.push(sqlite_value_to_qfs(v));
            }
            out.push(Row::new(values));
        }
        Ok(out)
    }

    fn commit_transaction(&self, ops: &[DmlOp]) -> Result<u64, SqlError> {
        let mut conn = self
            .conn
            .lock()
            .map_err(|_| SqlError::backend("sqlite", "lock", "poisoned connection mutex"))?;
        let tx = conn
            .transaction()
            .map_err(|e| SqlError::backend("sqlite", "begin", e.to_string()))?;
        let mut affected = 0u64;
        for op in ops {
            let (sql, params) = render_dml(Dialect::Sqlite, op);
            let boxed = bind_params(&params);
            let refs: Vec<&dyn rusqlite::ToSql> = boxed.iter().map(|b| b.as_ref()).collect();
            // On ANY error the `tx` is dropped without commit → automatic ROLLBACK (zero rows
            // changed), satisfying the all-or-nothing guarantee.
            let n = tx
                .execute(&sql, refs.as_slice())
                .map_err(|e| SqlError::backend("sqlite", "dml", e.to_string()))?;
            affected += n as u64;
        }
        tx.commit()
            .map_err(|e| SqlError::backend("sqlite", "commit", e.to_string()))?;
        Ok(affected)
    }

    fn execute_ddl(&self, sql: &str) -> Result<(), SqlError> {
        // A rendered `CREATE TABLE` / `DROP TABLE` (ADR 0009) — no bound params (identifiers only).
        // `execute_batch` runs it as its own statement, distinct from the parameterized DML path.
        let conn = self
            .conn
            .lock()
            .map_err(|_| SqlError::backend("sqlite", "lock", "poisoned connection mutex"))?;
        conn.execute_batch(sql)
            .map_err(|e| SqlError::backend("sqlite", "ddl", e.to_string()))
    }
}

/// Build the live connection registry from the `path_binding` registry — the SINGLE source of a
/// `/sql/<conn>` mount (a `CONNECT /sql/<conn> TO sqlite|postgres|mysql AT '<loc>' [SECRET '<ref>']`
/// binding, persisted by `qfs connect`). The retired `QFS_SQL_*` env var and `connections.qfs`
/// loader are gone. Best-effort: a connection that cannot be opened/introspected is skipped (never
/// panics). Returns an empty registry when nothing is bound. Run, commit, and describe all read this
/// ONE source, so they converge.
#[must_use]
pub fn conn_registry() -> ConnRegistry {
    let mut reg = ConnRegistry::new();
    for (conn, driver, at, secret) in path_binding_sql_connections() {
        let pw = || resolve_db_password(secret.as_deref());
        let handle = match driver.as_str() {
            "sqlite" => open_sqlite_handle(&at),
            "postgres" => open_pg_handle(&at, pw().as_deref()),
            "mysql" => open_mysql_handle(&at, pw().as_deref()),
            _ => None,
        };
        if let Some(handle) = handle {
            reg = reg.with(conn, handle);
        }
    }
    reg
}

/// The `qfs connect` sql connections from the project-DB `path_binding` registry (the canonical
/// source): each FULL-connect binding whose path is under `/sql/` and whose driver is a relational
/// backend, as `(conn, driver, at_locator, secret_ref)`. `conn` is the segment after `/sql/`, so a
/// `CONNECT /sql/shop TO sqlite …` mounts at `/sql/shop/<table>`. Empty when no system DB / no sql
/// binding (best-effort, never panics — a persisted-but-unresolvable binding just fails closed).
fn path_binding_sql_connections() -> Vec<(String, String, String, Option<String>)> {
    let Ok(Some(sys)) = crate::store::open_system_db() else {
        return Vec::new();
    };
    let conn = sys.into_db().into_connection();
    crate::path_binding::db_list_bindings(&conn)
        .unwrap_or_default()
        .into_iter()
        .filter(|b| b.alias_of.is_none())
        .filter_map(|b| {
            let driver = b.driver_id.as_deref()?;
            if !matches!(driver, "sqlite" | "postgres" | "mysql") {
                return None;
            }
            let conn_name = b
                .path
                .strip_prefix("/sql/")?
                .split('/')
                .next()
                .filter(|s| !s.is_empty())?
                .to_ascii_lowercase();
            let at = b.at_locator.clone()?;
            Some((conn_name, driver.to_string(), at, b.secret_ref.clone()))
        })
        .collect()
}

/// Resolve a connection's `SECRET '<ref>'` to a password string. `env:` refs (the dev-stack
/// convention) resolve without the encrypted vault; a `vault:` ref needs the unlock flow (a
/// follow-up) and best-effort resolves to `None` here, so the URL-embedded password is used instead.
fn resolve_db_password(secret_ref: Option<&str>) -> Option<String> {
    let reference = secret_ref?;
    let vault = qfs_secrets::InMemoryStore::new();
    crate::secret_ref::resolve_secret_ref(reference, &vault)
        .ok()
        .and_then(|s| s.expose_str().map(str::to_string))
}

/// Open one Postgres connection into a [`ConnHandle`] (connect + introspect). `None` on any failure
/// — best-effort, never panics, so an unreachable DB leaves `/sql/<conn>` unregistered (fail closed).
fn open_pg_handle(url: &str, password: Option<&str>) -> Option<ConnHandle> {
    let backend: Arc<dyn SqlBackend> =
        Arc::new(crate::sql_backends::PostgresBackend::connect(url, password).ok()?);
    ConnHandle::new(backend).ok()
}

/// Open one MySQL connection into a [`ConnHandle`] (connect + introspect). `None` on any failure.
fn open_mysql_handle(url: &str, password: Option<&str>) -> Option<ConnHandle> {
    let backend: Arc<dyn SqlBackend> =
        Arc::new(crate::sql_backends::MysqlBackend::connect(url, password).ok()?);
    ConnHandle::new(backend).ok()
}

/// Open one SQLite file into a [`ConnHandle`] (the shared open path for a `CONNECT`-bound
/// connection). `None` when the file cannot be opened/introspected — best-effort, never panics.
fn open_sqlite_handle(path: &str) -> Option<ConnHandle> {
    let backend: Arc<dyn SqlBackend> = Arc::new(SqliteBackend::open(path).ok()?);
    ConnHandle::new(backend).ok()
}

/// Whether any `/sql` connection is bound (a persisted `CONNECT /sql/<conn> …` `path_binding` row —
/// the single source) — the binary only registers the sql mount + apply driver when at least one
/// resolves (so an unbound `/sql` commit fails closed).
#[must_use]
pub fn has_connections() -> bool {
    !path_binding_sql_connections().is_empty()
}

/// A fresh [`SqlDriver`] over the live registry — the planning mount AND the source the apply
/// driver is built from.
#[must_use]
pub fn sql_driver() -> SqlDriver {
    SqlDriver::new(conn_registry())
}

/// Build a [`SqlDriver`] over a freshly-seeded temp SQLite database under connection `conn` (the
/// `ddl` is executed before catalog introspection). Returns the temp path (so the caller deletes it)
/// and the introspected driver — a test-only helper for the read-facet adapter tests, which need a
/// live catalog without `QFS_SQL_*` env config.
#[cfg(test)]
pub(crate) fn seeded_test_driver(conn: &str, ddl: &str) -> (std::path::PathBuf, SqlDriver) {
    let mut path = std::env::temp_dir();
    path.push(format!("qfs-sqlread-{conn}-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let backend = SqliteBackend::open(&path).expect("open temp db");
    {
        let c = backend.conn.lock().expect("lock");
        c.execute_batch(ddl).expect("seed ddl");
    }
    let backend: Arc<dyn SqlBackend> = Arc::new(backend);
    let handle = ConnHandle::new(backend).expect("introspect catalog");
    let registry = ConnRegistry::new().with(conn.to_string(), handle);
    (path, SqlDriver::new(registry))
}

#[cfg(test)]
mod tests {
    //! Real-engine tests against a temp-file SQLite database (not in-memory), proving the
    //! production file-backed path persists, introspects, commits atomically, and reads back.
    use super::*;

    fn temp_db_path() -> std::path::PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("qfs-sql-binmod-test-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&p);
        p
    }

    #[test]
    fn file_backed_introspect_and_read_roundtrip() {
        let path = temp_db_path();
        let backend = SqliteBackend::open(&path).expect("open temp db");
        {
            let conn = backend.conn.lock().unwrap();
            conn.execute_batch(
                "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, age INTEGER);",
            )
            .unwrap();
        }
        let catalog = backend.introspect().expect("introspect");
        let users = catalog
            .tables
            .iter()
            .find(|t| t.name == "users")
            .expect("users catalogued");
        assert_eq!(users.kind, RelationKind::Table);
        assert!(users.columns.iter().any(|c| c.name == "id" && c.pk));
        assert!(users
            .columns
            .iter()
            .any(|c| c.name == "name" && !c.nullable));
        let rows = backend
            .execute_read("SELECT id, name FROM users", &[])
            .expect("read");
        assert!(rows.is_empty());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn four_column_types_roundtrip_via_seeded_driver() {
        // The rich column types NUMERIC / TIMESTAMP / UUID / JSON round-trip write→read through a
        // real SQLite backend: the declared types map to their canonical `ColumnType` on the read
        // (introspection) side, and the stored values survive intact. Hermetic end-to-end.
        use qfs_types::{ColumnType, Value};
        let (path, driver) = seeded_test_driver(
            "types",
            "CREATE TABLE vals (n NUMERIC, ts TIMESTAMP, u UUID, j JSON);\n\
             INSERT INTO vals (n, ts, u, j) VALUES \
               (12.34, '2026-07-18T00:00:00Z', '550e8400-e29b-41d4-a716-446655440000', '{\"a\":1}');",
        );
        let handle = driver
            .registry()
            .get("types")
            .expect("mounted `types` conn");
        // Write side: each declared type maps to the canonical qfs `ColumnType`.
        let cat = handle.catalog();
        let vals = cat.table("vals").expect("vals catalogued");
        let ty = |name: &str| vals.column(name).expect("column").ty.clone();
        assert_eq!(ty("n"), ColumnType::Decimal);
        assert_eq!(ty("ts"), ColumnType::Timestamp);
        assert_eq!(ty("u"), ColumnType::Uuid);
        assert_eq!(ty("j"), ColumnType::Json);
        // Read side: the stored values come back intact.
        let rows = handle
            .backend()
            .execute_read("SELECT n, ts, u, j FROM vals", &[])
            .expect("read back");
        assert_eq!(rows.len(), 1);
        let r = &rows[0];
        assert_eq!(r.values[0], Value::Float(12.34));
        assert_eq!(r.values[1], Value::Text("2026-07-18T00:00:00Z".to_string()));
        assert_eq!(
            r.values[2],
            Value::Text("550e8400-e29b-41d4-a716-446655440000".to_string())
        );
        assert_eq!(r.values[3], Value::Text("{\"a\":1}".to_string()));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn env_var_and_connections_file_bind_nothing_only_path_binding_binds() {
        // The single-source guarantee: the retired `QFS_SQL_*` env var and a `connections.qfs`
        // `CREATE CONNECTION` file bind NOTHING (their loader + fallback are gone — experimental, no
        // backward compat). Only a persisted `CONNECT /sql/<conn> …` `path_binding` row wires a
        // mount, so run / commit / describe converge on that ONE registry.
        let _home = crate::testenv::HomeGuard::with_passphrase("sql-only-path-binding");
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("shop.db");
        {
            let c = rusqlite::Connection::open(&db_path).unwrap();
            c.execute("CREATE TABLE items (id INTEGER, name TEXT)", [])
                .unwrap();
        }
        let db_str = db_path.to_str().unwrap();

        // env-only / file-only configurations resolve to NO working mount.
        let conns = dir.path().join("connections.qfs");
        std::fs::write(
            &conns,
            format!("CREATE CONNECTION shop DRIVER sqlite AT '{db_str}';"),
        )
        .unwrap();
        std::env::set_var("QFS_SQL_SHOP", db_str);
        std::env::set_var("QFS_CONNECTIONS", conns.to_str().unwrap());
        assert!(
            !has_connections(),
            "a QFS_SQL_* / connections.qfs config binds no /sql mount"
        );
        assert!(
            conn_registry().get("shop").is_err(),
            "no `shop` mount from env/file"
        );
        std::env::remove_var("QFS_SQL_SHOP");
        std::env::remove_var("QFS_CONNECTIONS");

        // Only the persisted path_binding row wires the mount.
        let proj = crate::store::open_system_db()
            .unwrap()
            .unwrap()
            .into_db()
            .into_connection();
        crate::path_binding::db_upsert_binding(
            &proj,
            "/sql/shop",
            "sqlite",
            Some(db_str),
            None,
            Some("local"),
            None,
            None,
        )
        .unwrap();
        drop(proj);
        assert!(
            has_connections(),
            "the path_binding row wires the /sql mount"
        );
        assert!(
            conn_registry().get("shop").is_ok(),
            "`shop` resolves from the path_binding registry"
        );
    }
}
