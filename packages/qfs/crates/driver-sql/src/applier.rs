//! [`SqlApplier`] — the SQL driver's apply leg (blueprint §7, ticket steps 9/11). It lowers a DML
//! effect node ([`EffectKind::Insert`]/`Upsert`/`Update`/`Remove`) into one or more parameterized
//! [`DmlOp`]s and applies them inside **one ACID transaction** on the addressed connection (BEGIN
//! → ops → COMMIT; ROLLBACK on any error so a mid-way failure leaves zero rows changed).
//!
//! ## Single-connection = single transaction (blueprint §7)
//! Every effect in one apply targets one `<conn>`; spanning two connections is **not** this
//! ticket's job and is rejected with the structured [`SqlError::CrossSource`]. The applier is
//! stateless (the live handle is behind an `Arc` in the registry), so it implements
//! [`SharedApplier`] (the `&self` apply the runtime bridge requires).
//!
//! ## Capability gating belt-and-suspenders (blueprint §6)
//! A write to a **view** is rejected here with [`SqlError::ReadOnlyView`] even though the
//! parse-time capability gate ([`qfs_driver::check_capability`]) rejects it first — so even a
//! hand-built plan that bypassed the gate cannot mutate a view.
//!
//! ## Secret safety
//! No connection string, password, or bound parameter VALUE is ever logged or placed in a
//! [`SqlError`]; the structured logs render the verb + table + affected count only.

use qfs_plan::{AppliedEffect, ApplyError, EffectKind, EffectNode, PlanApplier};
use qfs_runtime::{EffectError, EffectOutput, SharedApplier};
use qfs_types::{ColumnType, Value};

use crate::conn::ConnRegistry;
use crate::error::SqlError;
use crate::path::SqlPath;
use qfs_sql_core::TableCatalog;
use qfs_sql_core::{render_ddl, DdlColumn, DdlOp, Dialect, DmlOp, Param, SqlPredicate};

/// The synchronous SQL apply leg. Holds the connection registry (handles are behind `Arc`s, so
/// the leg is cheap to clone and `&self`-apply). Stateless across calls.
#[derive(Clone)]
pub struct SqlApplier {
    registry: ConnRegistry,
}

impl SqlApplier {
    /// Build an applier over a connection registry.
    #[must_use]
    pub fn new(registry: ConnRegistry) -> Self {
        Self { registry }
    }

    /// Apply one effect node. A write to a concrete table (`/sql/<conn>/<table>`) lowers to a
    /// [`DmlOp`] committed in one transaction; a write to the **catalog node** (`/sql/<conn>`,
    /// ADR 0009 §1) is schema DDL — `INSERT` creates a table, `REMOVE` drops one — executed via the
    /// backend's DDL path. Returns the affected count (rows for DML, schema objects for DDL).
    fn apply_node(&self, node: &EffectNode) -> Result<u64, SqlError> {
        let path = SqlPath::parse_str(node.target.path.as_str())?;
        match &path {
            SqlPath::Table {
                conn,
                schema,
                table,
            } => self.apply_table_dml(node, conn, schema, table),
            SqlPath::Connection { conn } => self.apply_catalog_ddl(node, conn),
            SqlPath::Root => Err(SqlError::InvalidPath {
                path: node.target.path.as_str().to_string(),
                reason: "a write effect cannot target the /sql root; name a connection or table",
            }),
        }
    }

    /// The DML apply leg: gate the verb against the catalog and commit the lowered [`DmlOp`].
    fn apply_table_dml(
        &self,
        node: &EffectNode,
        conn: &str,
        schema: &str,
        table: &str,
    ) -> Result<u64, SqlError> {
        let handle = self.registry.get(conn)?;
        let catalog = handle.catalog();
        let table_cat = catalog.table(table).ok_or_else(|| SqlError::UnknownTable {
            table: table.to_string(),
        })?;

        // Capability gate (belt-and-suspenders): a view is SELECT-only.
        if table_cat.is_view() {
            return Err(SqlError::ReadOnlyView {
                path: node.target.path.as_str().to_string(),
                verb: static_verb_label(&node.kind),
            });
        }

        let op = lower_effect(node, schema, table_cat)?;
        handle.backend().commit_transaction(&[op])
    }

    /// The DDL apply leg over the connection's catalog node (ADR 0009 §1). `INSERT` decodes a
    /// `{ name, columns }` row into a `CREATE TABLE`; `REMOVE` decodes a `name` filter into a
    /// `DROP TABLE` (inherently irreversible, so the commit gate has already required an explicit
    /// acknowledgement). Other verbs are not DDL and are rejected. The dialect renders the SQL and
    /// the backend executes it.
    fn apply_catalog_ddl(&self, node: &EffectNode, conn: &str) -> Result<u64, SqlError> {
        let handle = self.registry.get(conn)?;
        let dialect = handle.dialect();
        let op = match node.kind {
            EffectKind::Insert => decode_create_table(dialect, node)?,
            EffectKind::Remove => decode_drop_table(node)?,
            _ => {
                return Err(SqlError::MalformedEffect {
                    reason: format!(
                        "{} on a /sql/<conn> catalog node is not a DDL verb; INSERT creates a \
                         table and REMOVE drops one",
                        static_verb_label(&node.kind)
                    ),
                })
            }
        };
        let sql = render_ddl(dialect, &op);
        handle.backend().execute_ddl(&sql)?;
        // Refresh the cached catalog so a subsequent DESCRIBE/SELECT in the same process sees the
        // new schema (ADR 0009 §4). The handle shares its catalog cell with the driver's clone, so
        // the refresh is visible there too. A refresh failure after a successful DDL is not fatal —
        // the DDL applied; the next fresh handle re-introspects — so it is not propagated.
        let _ = handle.refresh_catalog();
        // One schema object created/dropped. (A DDL statement is all-or-nothing at the backend.)
        Ok(1)
    }
}

/// Look up a field value by column name in an effect's single-row payload — the applier maps a
/// named catalog-row cell (e.g. `name`, `columns`) onto its meaning regardless of column order.
fn field<'a>(node: &'a EffectNode, name: &str) -> Option<&'a Value> {
    let idx = node
        .args
        .schema
        .columns
        .iter()
        .position(|c| c.name == name)?;
    node.args.rows.first()?.values.get(idx)
}

/// Decode an `INSERT INTO /sql/<conn>` row into a `CREATE TABLE` (ADR 0009 §1): a text `name` and a
/// `columns` array of `{ name, type, nullable?, primary_key?, unique? }` structs.
fn decode_create_table(dialect: Dialect, node: &EffectNode) -> Result<DdlOp, SqlError> {
    let table = match field(node, "name") {
        Some(Value::Text(t)) if !t.is_empty() => t.clone(),
        _ => {
            return Err(SqlError::MalformedEffect {
                reason:
                    "CREATE TABLE via INSERT INTO /sql/<conn> requires a non-empty text `name` \
                         column"
                        .to_string(),
            })
        }
    };
    let columns = match field(node, "columns") {
        Some(Value::Array(items)) => decode_columns(dialect, items)?,
        _ => {
            return Err(SqlError::MalformedEffect {
                reason: "CREATE TABLE via INSERT INTO /sql/<conn> requires a `columns` array of \
                         column definitions"
                    .to_string(),
            })
        }
    };
    if columns.is_empty() {
        return Err(SqlError::MalformedEffect {
            reason: "CREATE TABLE requires at least one column".to_string(),
        });
    }
    Ok(DdlOp::CreateTable {
        schema: String::new(),
        table,
        columns,
        if_not_exists: false,
    })
}

/// Decode the `columns` array into [`DdlColumn`]s. Each entry is a struct with a required text
/// `name`, an optional `type` name (default `text`), and optional `nullable`/`primary_key`/`unique`
/// booleans.
fn decode_columns(dialect: Dialect, items: &[Value]) -> Result<Vec<DdlColumn>, SqlError> {
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        let Value::Struct(fields) = item else {
            return Err(SqlError::MalformedEffect {
                reason: "each entry in `columns` must be a struct { name, type, ... }".to_string(),
            });
        };
        let name = match fields.get("name") {
            Some(Value::Text(t)) if !t.is_empty() => t.clone(),
            _ => {
                return Err(SqlError::MalformedEffect {
                    reason: "a column definition needs a non-empty text `name`".to_string(),
                })
            }
        };
        let ty = match fields.get("type") {
            Some(Value::Text(t)) => column_type_from_name(dialect, t),
            None => ColumnType::Text,
            _ => {
                return Err(SqlError::MalformedEffect {
                    reason: "a column `type` must be a type-name string".to_string(),
                })
            }
        };
        let nullable = match fields.get("nullable") {
            Some(Value::Bool(b)) => *b,
            _ => true,
        };
        let primary_key = matches!(fields.get("primary_key"), Some(Value::Bool(true)));
        let unique = matches!(fields.get("unique"), Some(Value::Bool(true)));
        out.push(DdlColumn::new(name, ty, nullable, primary_key, unique));
    }
    Ok(out)
}

/// Map a user-facing qfs type-name string (as written in a `columns` definition) onto a canonical
/// [`ColumnType`]. Recognises the qfs type vocabulary; an unrecognised name defers to the backend
/// SQL-type mapping ([`Dialect::map_type`]) so a raw SQL type name still resolves.
fn column_type_from_name(dialect: Dialect, name: &str) -> ColumnType {
    match name.trim().to_ascii_lowercase().as_str() {
        "bool" | "boolean" => ColumnType::Bool,
        "int" | "integer" | "bigint" => ColumnType::Int,
        "float" | "double" | "real" => ColumnType::Float,
        "decimal" | "numeric" => ColumnType::Decimal,
        "text" | "string" | "varchar" => ColumnType::Text,
        "bytes" | "blob" | "bytea" => ColumnType::Bytes,
        "timestamp" | "datetime" => ColumnType::Timestamp,
        "date" => ColumnType::Date,
        "uuid" => ColumnType::Uuid,
        "json" | "jsonb" => ColumnType::Json,
        other => dialect.map_type(other),
    }
}

/// Decode a `REMOVE FROM /sql/<conn> WHERE name = '<table>'` into a `DROP TABLE IF EXISTS`. The
/// `WHERE name = …` equality lands on the **WHERE-selector** (blueprint §7) — a REMOVE writes
/// nothing, so its `args` is empty and the selector is the only channel this key travels on.
fn decode_drop_table(node: &EffectNode) -> Result<DdlOp, SqlError> {
    let table = match node.selector_text("name") {
        Some(t) => t,
        None => {
            return Err(SqlError::MalformedEffect {
                reason: "DROP TABLE via REMOVE FROM /sql/<conn> requires a `name` filter, e.g. \
                         WHERE name = 'orders'"
                    .to_string(),
            })
        }
    };
    Ok(DdlOp::DropTable {
        schema: String::new(),
        table,
        if_exists: true,
    })
}

/// Lower one effect node into a [`DmlOp`] against the catalogued table. The row payload (the
/// effect's `args` batch) supplies the column names (from the batch schema) and the bound values;
/// the WHERE for UPDATE/DELETE is carried as a `__where__`-keyed key/value match in the args
/// schema (the planner builds this) — here we support the common "match by key columns" shape:
/// the row's key-column values form the WHERE so a filtered UPDATE/DELETE binds those as params.
fn lower_effect(node: &EffectNode, schema: &str, table: &TableCatalog) -> Result<DmlOp, SqlError> {
    let batch = &node.args;
    let columns: Vec<String> = batch
        .schema
        .columns
        .iter()
        .map(|c| c.name.clone())
        .collect();

    match node.kind {
        EffectKind::Insert => {
            let row = single_row(node)?;
            Ok(DmlOp::Insert {
                schema: schema.to_string(),
                table: table.name.clone(),
                columns,
                values: row.values.iter().map(Param::from_value).collect(),
            })
        }
        EffectKind::Upsert => {
            let row = single_row(node)?;
            let conflict_keys: Vec<String> =
                table.key_columns().iter().map(|c| c.name.clone()).collect();
            if conflict_keys.is_empty() {
                return Err(SqlError::MalformedEffect {
                    reason: "UPSERT requires a primary-key or unique column to be retry-safe; \
                             the table has none"
                        .to_string(),
                });
            }
            Ok(DmlOp::Upsert {
                schema: schema.to_string(),
                table: table.name.clone(),
                columns,
                values: row.values.iter().map(Param::from_value).collect(),
                conflict_keys,
            })
        }
        EffectKind::Update => {
            let row = single_row(node)?;
            // SET is the whole `args` payload; the match is the REAL `WHERE` off the selector (§7).
            let (assignments, where_, where_params) = split_update(node, &columns, &row.values)?;
            Ok(DmlOp::Update {
                schema: schema.to_string(),
                table: table.name.clone(),
                assignments,
                where_,
                where_params,
            })
        }
        EffectKind::Remove => {
            // A REMOVE writes nothing — its `args` is empty — so the match comes wholly from the
            // selector. A REMOVE with no `WHERE` targets the whole table (irreversible) and is
            // rejected here to avoid an accidental mass delete.
            let (where_, where_params) = build_selector_where(node);
            if where_.is_none() {
                return Err(SqlError::MalformedEffect {
                    reason: "REMOVE without a WHERE would delete every row; supply an equality \
                             filter (`REMOVE /sql/<conn>/<table> WHERE <col> == <value>`)"
                        .to_string(),
                });
            }
            Ok(DmlOp::Delete {
                schema: schema.to_string(),
                table: table.name.clone(),
                where_,
                where_params,
            })
        }
        EffectKind::Read | EffectKind::List | EffectKind::Call(_) => {
            Err(SqlError::MalformedEffect {
                reason: format!(
                    "{} is not a SQL DML effect (only INSERT/UPSERT/UPDATE/REMOVE apply here)",
                    static_verb_label(&node.kind)
                ),
            })
        }
        // `EffectKind` is `#[non_exhaustive]`; any future kind is not a SQL DML effect here.
        _ => Err(SqlError::MalformedEffect {
            reason: format!(
                "{} is not a SQL DML effect (only INSERT/UPSERT/UPDATE/REMOVE apply here)",
                static_verb_label(&node.kind)
            ),
        }),
    }
}

/// The stable `&'static str` label for an effect kind — used where a structured error field is
/// `&'static str` (the borrowed [`EffectKind::label`] cannot satisfy that lifetime).
fn static_verb_label(kind: &EffectKind) -> &'static str {
    match kind {
        EffectKind::Read => "READ",
        EffectKind::List => "LIST",
        EffectKind::Insert => "INSERT",
        EffectKind::Upsert => "UPSERT",
        EffectKind::Update => "UPDATE",
        EffectKind::Remove => "REMOVE",
        EffectKind::Call(_) => "CALL",
        _ => "WRITE",
    }
}

/// The single row a DML effect carries (this driver applies one row per effect node; the planner
/// fans a multi-row write into multiple nodes). A missing/multi row is a structured shape error.
fn single_row(node: &EffectNode) -> Result<&qfs_types::Row, SqlError> {
    match node.args.rows.as_slice() {
        [row] => Ok(row),
        [] => Err(SqlError::MalformedEffect {
            reason: "DML effect carries no row payload".to_string(),
        }),
        _ => Err(SqlError::MalformedEffect {
            reason: "DML effect carries more than one row; the planner must split it per row"
                .to_string(),
        }),
    }
}

/// The lowering of an UPDATE row: the `SET col = ?` assignments, the compiled `WHERE` (on the key
/// columns), and the WHERE's bound params — the three pieces [`DmlOp::Update`] needs.
type UpdateLowering = (Vec<(String, Param)>, Option<SqlPredicate>, Vec<Param>);

/// Split an UPDATE row into `SET <non-key> = ?` assignments and a `WHERE <key> = ?` match. The key
/// columns (PK / unique) become the WHERE; the rest become the SET. A row missing every key column
/// yields an empty WHERE (whole-table update) which the caller turns into the irreversible case.
fn split_update(
    node: &EffectNode,
    columns: &[String],
    values: &[Value],
) -> Result<UpdateLowering, SqlError> {
    // Every `args` column is a SET assignment: `args` is now purely the payload (§7). The old
    // rule — "SET the non-key columns, infer the WHERE from the key ones" — could not express
    // `SET id = 2 WHERE id = 1` (the key column collides), nor honour a non-key `WHERE`.
    let assignments: Vec<(String, Param)> = columns
        .iter()
        .zip(values.iter())
        .map(|(col, val)| (col.clone(), Param::from_value(val)))
        .collect();
    if assignments.is_empty() {
        return Err(SqlError::MalformedEffect {
            reason: "UPDATE has no column to set".to_string(),
        });
    }
    let (where_, where_params) = build_selector_where(node);
    if where_.is_none() {
        return Err(SqlError::MalformedEffect {
            reason: "UPDATE without a WHERE would update every row; supply an equality filter \
                     (`UPDATE /sql/<conn>/<table> SET … WHERE <col> == <value>`)"
                .to_string(),
        });
    }
    Ok((assignments, where_, where_params))
}

/// Build the `WHERE a = ? AND b = ?` predicate from the effect's **WHERE-selector** (blueprint §7) —
/// the operator's REAL filter, honoured as written. Returns `(None, [])` when the effect carries no
/// `WHERE` (the whole-table case both callers reject). Each bound value is a [`Param`]; the predicate
/// references them by index.
///
/// This retires PK-inference **for the match**: the old lowering scanned the payload row for
/// PRIMARY-KEY columns and matched on those, which meant a non-key `WHERE status == 'stale'` was
/// silently ignored, and a same-column `SET id = 2 WHERE id = 1` was inexpressible. (UPSERT's
/// `conflict_keys` stay PK-based — retry-safety is a separate concern from the match filter.)
fn build_selector_where(node: &EffectNode) -> (Option<SqlPredicate>, Vec<Param>) {
    let Some(selector) = node.selector.as_ref() else {
        return (None, Vec::new());
    };
    let Some(row) = selector.rows.first() else {
        return (None, Vec::new());
    };
    let mut params: Vec<Param> = Vec::new();
    let mut leaves: Vec<SqlPredicate> = Vec::new();
    for (col, val) in selector.schema.columns.iter().zip(row.values.iter()) {
        let idx = params.len();
        params.push(Param::from_value(val));
        leaves.push(SqlPredicate::Cmp {
            col: col.name.clone(),
            op: qfs_types::CmpOp::Eq,
            param: idx,
        });
    }
    let pred = leaves
        .into_iter()
        .reduce(|acc, leaf| SqlPredicate::And(Box::new(acc), Box::new(leaf)));
    (pred, params)
}

impl SharedApplier for SqlApplier {
    fn apply_shared(&self, node: &EffectNode) -> Result<EffectOutput, EffectError> {
        let affected = self
            .apply_node(node)
            .map_err(crate::error::sql_error_to_effect_error)?;
        Ok(EffectOutput::new(node.id, affected))
    }
}

impl PlanApplier for SqlApplier {
    /// The introspective `qfs_driver::Driver::applier()` seam (t09). Stateless, so it delegates to
    /// the same `&self` core as [`SharedApplier::apply_shared`]; the structured [`SqlError`] is
    /// reduced to the plan crate's owned `(id, reason)` shape — secret-free by construction.
    fn apply(&mut self, node: &EffectNode) -> Result<AppliedEffect, ApplyError> {
        let affected = self
            .apply_node(node)
            .map_err(|e| ApplyError::new(node.id, e.to_string()))?;
        Ok(AppliedEffect::new(node.id, affected))
    }
}
