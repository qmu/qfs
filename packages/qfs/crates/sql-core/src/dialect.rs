//! [`Dialect`] — the **single decision point** for every place the three SQL backends diverge
//! (blueprint §6, ticket "Hard part — dialect divergence"). Identifier quoting, placeholder
//! syntax (`$n` vs `?`), upsert form (`ON CONFLICT` vs `ON DUPLICATE KEY` vs `INSERT OR REPLACE`),
//! and SQL→qfs type mapping all branch here and **only** here, so a new divergence is one
//! exhaustive `match` rather than a scattered `if dialect == ...`. Every match is exhaustive (no
//! `_ =>` fallthrough that could silently mis-render — the ticket's coding-standards requirement).

use qfs_types::ColumnType;

/// One of the three SQL backends this driver renders for. The connection string selects it
/// (`postgres://` / `mysql://` / `sqlite:`); from there the dialect is the only thing that
/// differs in the emitter. Owned, `Copy`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialect {
    /// PostgreSQL — `"ident"` quoting, `$n` placeholders, `ON CONFLICT (...) DO UPDATE`,
    /// native `RETURNING`.
    Postgres,
    /// MySQL / MariaDB — `` `ident` `` quoting, `?` placeholders, `ON DUPLICATE KEY UPDATE`,
    /// no `RETURNING` (a secondary select / `LAST_INSERT_ID()` emulates it).
    Mysql,
    /// SQLite (also the D1 dialect, t23) — `"ident"` quoting, `?` placeholders,
    /// `ON CONFLICT (...) DO UPDATE`, native `RETURNING`.
    Sqlite,
}

impl Dialect {
    /// Parse the dialect from a connection-string scheme (the part before `://` or `:`).
    /// `None` for an unrecognised scheme (the caller raises a structured error rather than
    /// guessing a default — mis-routing a dialect would mis-render every statement).
    #[must_use]
    pub fn from_scheme(scheme: &str) -> Option<Self> {
        match scheme {
            "postgres" | "postgresql" => Some(Dialect::Postgres),
            "mysql" | "mariadb" => Some(Dialect::Mysql),
            "sqlite" | "file" => Some(Dialect::Sqlite),
            _ => None,
        }
    }

    /// A short, stable label for golden snapshots, logs, and structured errors.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Dialect::Postgres => "postgres",
            Dialect::Mysql => "mysql",
            Dialect::Sqlite => "sqlite",
        }
    }

    /// Quote a single SQL identifier (table / column / schema name) for this dialect.
    ///
    /// Postgres and SQLite use ANSI double quotes; MySQL uses backticks. An embedded quote
    /// character is **doubled** (the SQL standard escape: `"a""b"` / `` `a``b` ``) so an
    /// identifier can never break out of its quoting — defense in depth alongside the
    /// catalog-validation that rejects unknown identifiers before they ever reach here.
    #[must_use]
    pub fn quote_ident(self, ident: &str) -> String {
        let (open, close, escaped) = match self {
            Dialect::Postgres | Dialect::Sqlite => ('"', '"', ident.replace('"', "\"\"")),
            Dialect::Mysql => ('`', '`', ident.replace('`', "``")),
        };
        format!("{open}{escaped}{close}")
    }

    /// Quote a possibly-qualified `schema.table` reference, quoting each segment independently
    /// (so `public.users` → `"public"."users"`). An empty / default schema yields the bare table.
    #[must_use]
    pub fn quote_qualified(self, schema: &str, table: &str) -> String {
        if schema.is_empty() {
            self.quote_ident(table)
        } else {
            format!("{}.{}", self.quote_ident(schema), self.quote_ident(table))
        }
    }

    /// Render the bind placeholder for the `index`-th (1-based) parameter. Postgres uses the
    /// positional `$n`; MySQL and SQLite use the anonymous `?`. The emitter increments `index`
    /// for every bound value so the rendered SQL and the `Vec<Param>` stay in lockstep.
    #[must_use]
    pub fn placeholder(self, index: usize) -> String {
        match self {
            Dialect::Postgres => format!("${index}"),
            Dialect::Mysql | Dialect::Sqlite => "?".to_string(),
        }
    }

    /// Whether this dialect supports a native `RETURNING` clause. Postgres and SQLite do;
    /// MySQL does not (the applier emulates it with `LAST_INSERT_ID()` / a secondary select).
    #[must_use]
    pub const fn supports_returning(self) -> bool {
        match self {
            Dialect::Postgres | Dialect::Sqlite => true,
            Dialect::Mysql => false,
        }
    }

    /// Map a backend SQL type name (lower-cased, as introspection reports it) onto the canonical
    /// [`ColumnType`] (blueprint §4). Unrecognised / vendor-specific types fall back to
    /// [`ColumnType::Unknown`] — still queryable (late-bound), never a hard error. The mapping is
    /// intentionally conservative: only well-understood families are typed precisely.
    #[must_use]
    pub fn map_type(self, sql_type: &str) -> ColumnType {
        // Normalise: lower-case and strip any size/precision suffix like `varchar(255)`.
        let base = sql_type
            .trim()
            .to_ascii_lowercase()
            .split(['(', ' '])
            .next()
            .unwrap_or("")
            .to_string();
        match base.as_str() {
            "bool" | "boolean" | "bit" => ColumnType::Bool,
            "int" | "integer" | "int2" | "int4" | "int8" | "smallint" | "bigint" | "mediumint"
            | "tinyint" | "serial" | "bigserial" => ColumnType::Int,
            "real" | "double" | "float" | "float4" | "float8" => ColumnType::Float,
            "numeric" | "decimal" | "money" => ColumnType::Decimal,
            "text" | "varchar" | "char" | "character" | "varying" | "nvarchar" | "clob"
            | "string" | "name" | "citext" => ColumnType::Text,
            "blob" | "bytea" | "varbinary" | "binary" => ColumnType::Bytes,
            "timestamp" | "timestamptz" | "datetime" => ColumnType::Timestamp,
            "date" => ColumnType::Date,
            "uuid" => ColumnType::Uuid,
            "json" | "jsonb" => ColumnType::Json,
            _ => ColumnType::Unknown,
        }
    }

    /// Map a canonical qfs [`ColumnType`] onto the SQL column type for this dialect — the inverse
    /// of [`Dialect::map_type`], used by the DDL emitter to render a `CREATE TABLE` column. Each
    /// dialect diverges here and only here (SQLite's loose type affinities, Postgres's rich types,
    /// MySQL's own spellings), so a new type is one `match` arm rather than scattered `if`s.
    ///
    /// Non-scalar and future types (`Struct`/`Array`/`Json`, plus any variant a later epic adds to
    /// the `#[non_exhaustive]` `ColumnType`) are stored as the dialect's JSON/text carrier — the
    /// same conservative, always-storable fallback [`Dialect::map_type`] takes on the read side, so
    /// a round-trip never hard-errors on an exotic column.
    #[must_use]
    pub fn sql_type(self, ty: &ColumnType) -> &'static str {
        match self {
            Dialect::Postgres => match ty {
                ColumnType::Bool => "BOOLEAN",
                ColumnType::Int => "BIGINT",
                ColumnType::Float => "DOUBLE PRECISION",
                ColumnType::Decimal => "NUMERIC",
                ColumnType::Text => "TEXT",
                ColumnType::Bytes => "BYTEA",
                ColumnType::Timestamp => "TIMESTAMPTZ",
                ColumnType::Date => "DATE",
                ColumnType::Uuid => "UUID",
                ColumnType::Struct(_) | ColumnType::Array(_) | ColumnType::Json => "JSONB",
                ColumnType::Unknown => "TEXT",
                _ => "TEXT",
            },
            Dialect::Mysql => match ty {
                ColumnType::Bool => "TINYINT(1)",
                ColumnType::Int => "BIGINT",
                ColumnType::Float => "DOUBLE",
                ColumnType::Decimal => "DECIMAL",
                ColumnType::Text => "TEXT",
                ColumnType::Bytes => "LONGBLOB",
                ColumnType::Timestamp => "DATETIME",
                ColumnType::Date => "DATE",
                ColumnType::Uuid => "CHAR(36)",
                ColumnType::Struct(_) | ColumnType::Array(_) | ColumnType::Json => "JSON",
                ColumnType::Unknown => "TEXT",
                _ => "TEXT",
            },
            Dialect::Sqlite => match ty {
                // SQLite has dynamic type affinity: INTEGER / REAL / TEXT / BLOB / NUMERIC are the
                // affinities every declared type resolves to.
                ColumnType::Bool | ColumnType::Int => "INTEGER",
                ColumnType::Float => "REAL",
                ColumnType::Decimal => "NUMERIC",
                ColumnType::Bytes => "BLOB",
                ColumnType::Text
                | ColumnType::Timestamp
                | ColumnType::Date
                | ColumnType::Uuid
                | ColumnType::Struct(_)
                | ColumnType::Array(_)
                | ColumnType::Json
                | ColumnType::Unknown => "TEXT",
                _ => "TEXT",
            },
        }
    }
}
