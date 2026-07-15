//! `ddl` — the per-dialect **DDL emitter** (ADR 0009: "managing a database as data"). It renders a
//! [`DdlOp`] (a lowered `CREATE TABLE` / `DROP TABLE`) into a dialect-specific SQL string.
//!
//! ## Why DDL is a separate op, not a `DmlOp`
//! ADR 0009 decides that creating a table is `INSERT INTO /sql/<conn>` and dropping one is
//! `REMOVE FROM /sql/<conn>` — CRUD over the connection's **catalog** node, because blueprint §3
//! freezes the keyword set and forbids a `CREATE TABLE` statement keyword. The applier decodes
//! that catalog-row write into a [`DdlOp`]; this module renders it. Unlike a [`DmlOp`], a DDL
//! statement carries **no bound parameters** — a table/column *name* is an identifier, not a
//! value, so it is dialect-quoted (never a placeholder). Identifier quoting doubles an embedded
//! quote (`"a""b"`), so a name can never break out of its quoting even before the applier's
//! catalog validation.
//!
//! ## Purity
//! Like the rest of `qfs-sql-core`, every function here is pure: it renders SQL text and does no
//! I/O. Executing the DDL (and refreshing the cached catalog afterward) is the backend/driver's
//! job, not this crate's.

use qfs_types::ColumnType;

use crate::dialect::Dialect;

/// One column in a `CREATE TABLE` — the name, the canonical qfs [`ColumnType`] (the emitter maps
/// it to the dialect's SQL type via [`Dialect::sql_type`]), and the constraint flags. Mirrors
/// [`crate::catalog::ColumnDef`] but is the **input** to DDL (a definition the caller supplies)
/// rather than the **output** of introspection.
#[derive(Debug, Clone, PartialEq)]
pub struct DdlColumn {
    /// The column name (dialect-quoted at render time; never a bound value).
    pub name: String,
    /// The canonical qfs column type; [`Dialect::sql_type`] renders the dialect SQL type.
    pub ty: ColumnType,
    /// Whether the column admits `NULL`. A primary-key column is rendered `NOT NULL` regardless.
    pub nullable: bool,
    /// Whether the column is (part of) the primary key. Rendered as a table-level `PRIMARY KEY`.
    pub primary_key: bool,
    /// Whether the column carries a standalone `UNIQUE` constraint (ignored when it is also part
    /// of the primary key, which already implies uniqueness).
    pub unique: bool,
}

impl DdlColumn {
    /// Construct a column definition.
    #[must_use]
    pub fn new(
        name: impl Into<String>,
        ty: ColumnType,
        nullable: bool,
        primary_key: bool,
        unique: bool,
    ) -> Self {
        Self {
            name: name.into(),
            ty,
            nullable,
            primary_key,
            unique,
        }
    }
}

/// A lowered DDL operation — the schema-changing counterpart to [`crate::DmlOp`]. Produced by the
/// SQL driver's applier from a catalog-row write (ADR 0009 §1), rendered by [`render_ddl`].
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum DdlOp {
    /// `CREATE TABLE [IF NOT EXISTS] <table> (<col> <type> [NOT NULL] [UNIQUE], ..., PRIMARY KEY
    /// (<pk...>))`. Reversible (a `CREATE` loses no data), so it commits without the irreversible
    /// gate.
    CreateTable {
        /// The schema (empty for the connection's default schema, e.g. SQLite).
        schema: String,
        /// The table name.
        table: String,
        /// The columns, in declaration order.
        columns: Vec<DdlColumn>,
        /// Emit `IF NOT EXISTS` so re-creating an existing table is a no-op rather than an error.
        if_not_exists: bool,
    },
    /// `DROP TABLE [IF EXISTS] <table>`. **Irreversible** (it destroys the table and its rows);
    /// the plan carries the `irreversible` flag and the commit gate enforces it (ADR 0009 §4).
    DropTable {
        /// The schema (empty for default).
        schema: String,
        /// The table name.
        table: String,
        /// Emit `IF EXISTS` so dropping an absent table is a no-op rather than an error.
        if_exists: bool,
    },
}

/// Render a [`DdlOp`] into dialect-specific SQL. Pure; returns only the SQL string (DDL binds no
/// parameters). Every identifier is dialect-quoted; every column type is mapped via
/// [`Dialect::sql_type`]. The `match` over [`DdlOp`] is exhaustive up to its `#[non_exhaustive]`
/// future arms.
#[must_use]
pub fn render_ddl(dialect: Dialect, op: &DdlOp) -> String {
    match op {
        DdlOp::CreateTable {
            schema,
            table,
            columns,
            if_not_exists,
        } => render_create_table(dialect, schema, table, columns, *if_not_exists),
        DdlOp::DropTable {
            schema,
            table,
            if_exists,
        } => {
            let table_sql = dialect.quote_qualified(schema, table);
            let guard = if *if_exists { "IF EXISTS " } else { "" };
            format!("DROP TABLE {guard}{table_sql}")
        } // `DdlOp` is `#[non_exhaustive]` for *external* crates; within `qfs-sql-core` this match is
          // exhaustive, so a future variant added here fails to compile until it is rendered — the
          // intended forcing function (no silent mis-render).
    }
}

/// Render `CREATE TABLE`. The primary key is always emitted as a table-level `PRIMARY KEY (...)`
/// clause (uniform for single- and multi-column keys); a PK column is forced `NOT NULL`. A column
/// that is `unique` but not part of the PK gets an inline `UNIQUE`.
fn render_create_table(
    dialect: Dialect,
    schema: &str,
    table: &str,
    columns: &[DdlColumn],
    if_not_exists: bool,
) -> String {
    let table_sql = dialect.quote_qualified(schema, table);
    let mut defs: Vec<String> = columns.iter().map(|c| render_column(dialect, c)).collect();

    let pk_cols: Vec<&DdlColumn> = columns.iter().filter(|c| c.primary_key).collect();
    if !pk_cols.is_empty() {
        let list = pk_cols
            .iter()
            .map(|c| dialect.quote_ident(&c.name))
            .collect::<Vec<_>>()
            .join(", ");
        defs.push(format!("PRIMARY KEY ({list})"));
    }

    let guard = if if_not_exists { "IF NOT EXISTS " } else { "" };
    format!("CREATE TABLE {guard}{table_sql} ({})", defs.join(", "))
}

/// Render one `<name> <type> [NOT NULL] [UNIQUE]` column definition.
fn render_column(dialect: Dialect, col: &DdlColumn) -> String {
    let mut parts = vec![
        dialect.quote_ident(&col.name),
        dialect.sql_type(&col.ty).to_string(),
    ];
    // A PK column is NOT NULL by SQL rule even if the caller left `nullable` true.
    if !col.nullable || col.primary_key {
        parts.push("NOT NULL".to_string());
    }
    // The table-level PRIMARY KEY already enforces uniqueness for PK columns.
    if col.unique && !col.primary_key {
        parts.push("UNIQUE".to_string());
    }
    parts.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn users_table() -> DdlOp {
        DdlOp::CreateTable {
            schema: String::new(),
            table: "users".to_string(),
            columns: vec![
                DdlColumn::new("id", ColumnType::Int, false, true, false),
                DdlColumn::new("name", ColumnType::Text, false, false, false),
                DdlColumn::new("email", ColumnType::Text, true, false, true),
            ],
            if_not_exists: false,
        }
    }

    #[test]
    fn create_table_sqlite() {
        let sql = render_ddl(Dialect::Sqlite, &users_table());
        assert_eq!(
            sql,
            "CREATE TABLE \"users\" (\"id\" INTEGER NOT NULL, \"name\" TEXT NOT NULL, \
             \"email\" TEXT UNIQUE, PRIMARY KEY (\"id\"))"
        );
    }

    #[test]
    fn create_table_postgres() {
        let sql = render_ddl(Dialect::Postgres, &users_table());
        assert_eq!(
            sql,
            "CREATE TABLE \"users\" (\"id\" BIGINT NOT NULL, \"name\" TEXT NOT NULL, \
             \"email\" TEXT UNIQUE, PRIMARY KEY (\"id\"))"
        );
    }

    #[test]
    fn create_table_mysql_uses_backticks_and_own_types() {
        let sql = render_ddl(Dialect::Mysql, &users_table());
        assert_eq!(
            sql,
            "CREATE TABLE `users` (`id` BIGINT NOT NULL, `name` TEXT NOT NULL, \
             `email` TEXT UNIQUE, PRIMARY KEY (`id`))"
        );
    }

    #[test]
    fn create_table_if_not_exists_and_qualified_schema() {
        let op = DdlOp::CreateTable {
            schema: "public".to_string(),
            table: "orders".to_string(),
            columns: vec![DdlColumn::new("id", ColumnType::Int, false, true, false)],
            if_not_exists: true,
        };
        let sql = render_ddl(Dialect::Postgres, &op);
        assert_eq!(
            sql,
            "CREATE TABLE IF NOT EXISTS \"public\".\"orders\" (\"id\" BIGINT NOT NULL, \
             PRIMARY KEY (\"id\"))"
        );
    }

    #[test]
    fn composite_primary_key_is_table_level() {
        let op = DdlOp::CreateTable {
            schema: String::new(),
            table: "memberships".to_string(),
            columns: vec![
                DdlColumn::new("user_id", ColumnType::Int, false, true, false),
                DdlColumn::new("group_id", ColumnType::Int, false, true, false),
            ],
            if_not_exists: false,
        };
        let sql = render_ddl(Dialect::Sqlite, &op);
        assert_eq!(
            sql,
            "CREATE TABLE \"memberships\" (\"user_id\" INTEGER NOT NULL, \
             \"group_id\" INTEGER NOT NULL, PRIMARY KEY (\"user_id\", \"group_id\"))"
        );
    }

    #[test]
    fn drop_table_if_exists() {
        let op = DdlOp::DropTable {
            schema: String::new(),
            table: "users".to_string(),
            if_exists: true,
        };
        assert_eq!(
            render_ddl(Dialect::Sqlite, &op),
            "DROP TABLE IF EXISTS \"users\""
        );
        assert_eq!(
            render_ddl(Dialect::Mysql, &op),
            "DROP TABLE IF EXISTS `users`"
        );
    }

    #[test]
    fn identifier_quote_is_doubled_never_broken_out_of() {
        // A table name carrying the quote character cannot escape its quoting.
        let op = DdlOp::DropTable {
            schema: String::new(),
            table: "we\"ird".to_string(),
            if_exists: false,
        };
        assert_eq!(render_ddl(Dialect::Sqlite, &op), "DROP TABLE \"we\"\"ird\"");
    }

    #[test]
    fn nonscalar_and_temporal_types_map_per_dialect() {
        let op = DdlOp::CreateTable {
            schema: String::new(),
            table: "events".to_string(),
            columns: vec![
                DdlColumn::new("at", ColumnType::Timestamp, false, false, false),
                DdlColumn::new("payload", ColumnType::Json, true, false, false),
                DdlColumn::new("blob", ColumnType::Bytes, true, false, false),
            ],
            if_not_exists: false,
        };
        assert_eq!(
            render_ddl(Dialect::Postgres, &op),
            "CREATE TABLE \"events\" (\"at\" TIMESTAMPTZ NOT NULL, \"payload\" JSONB, \
             \"blob\" BYTEA)"
        );
        assert_eq!(
            render_ddl(Dialect::Sqlite, &op),
            "CREATE TABLE \"events\" (\"at\" TEXT NOT NULL, \"payload\" TEXT, \"blob\" BLOB)"
        );
        assert_eq!(
            render_ddl(Dialect::Mysql, &op),
            "CREATE TABLE `events` (`at` DATETIME NOT NULL, `payload` JSON, `blob` LONGBLOB)"
        );
    }
}
