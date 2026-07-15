//! SQL table `OF <type>` contracts and declared column-type membership.
//!
//! The SQL driver is a leaf: it knows how to create/mutate SQL tables, not how to read qfs's
//! `/type` catalog. This binary-side apply facet keeps that boundary intact. It resolves declared
//! type names from the System DB before a catalog-node CREATE reaches the SQL applier, records the
//! table's qfs contract, and checks later row writes with `qfs_core::check_membership`.

use std::sync::Arc;

use qfs_core::ddl::types::{ColumnRefinement, ResolvedTypeDef};
use qfs_core::EffectKind;
use qfs_runtime::{ApplyCx, ApplyDriver, EffectError, EffectInput, EffectOutput};
use qfs_types::{Column, ColumnType, Fields, Row, RowBatch, Schema, Value};
use rusqlite::OptionalExtension;

/// Apply facet wrapping the stock SQL applier with qfs declared-type contract handling.
pub struct SqlContractApplyDriver {
    inner: Arc<dyn ApplyDriver>,
}

impl SqlContractApplyDriver {
    #[must_use]
    pub fn new(inner: Arc<dyn ApplyDriver>) -> Self {
        Self { inner }
    }
}

#[async_trait::async_trait]
impl ApplyDriver for SqlContractApplyDriver {
    async fn apply_batch(
        &self,
        kind: EffectKind,
        effects: &[EffectInput],
        cx: &ApplyCx,
    ) -> Vec<Result<EffectOutput, EffectError>> {
        let _ = kind;
        let mut out = Vec::with_capacity(effects.len());
        for effect in effects {
            out.push(self.apply_one(effect, cx).await);
        }
        out
    }

    async fn apply_one(
        &self,
        effect: &EffectInput,
        cx: &ApplyCx,
    ) -> Result<EffectOutput, EffectError> {
        let prepared = prepare_effect(effect)?;
        let out = self.inner.apply_one(&prepared.effect, cx).await?;
        if let Some(record) = prepared.record {
            persist_table_contract(&record)?;
        }
        Ok(out)
    }
}

struct PreparedEffect {
    effect: EffectInput,
    record: Option<TableContractRecord>,
}

#[derive(Debug, Clone)]
struct TableContractRecord {
    table_path: String,
    of_type: Option<String>,
    body: Option<String>,
}

fn prepare_effect(effect: &EffectInput) -> Result<PreparedEffect, EffectError> {
    match qfs_driver_sql::SqlPath::parse_str(effect.target.path.as_str()) {
        Ok(qfs_driver_sql::SqlPath::Connection { .. })
            if matches!(effect.kind, EffectKind::Insert) =>
        {
            prepare_catalog_create(effect)
        }
        Ok(qfs_driver_sql::SqlPath::Table { .. }) if writes_rows(&effect.kind) => {
            check_table_membership(effect)?;
            Ok(PreparedEffect {
                effect: effect.clone(),
                record: None,
            })
        }
        _ => Ok(PreparedEffect {
            effect: effect.clone(),
            record: None,
        }),
    }
}

fn writes_rows(kind: &EffectKind) -> bool {
    matches!(kind, EffectKind::Insert | EffectKind::Upsert)
}

fn prepare_catalog_create(effect: &EffectInput) -> Result<PreparedEffect, EffectError> {
    let table = text_field(&effect.args, "name")
        .filter(|s| !s.is_empty())
        .ok_or_else(|| EffectError::terminal("CREATE TABLE requires a non-empty `name`"))?;
    let table_path = format!(
        "{}/{}",
        effect.target.path.as_str().trim_end_matches('/'),
        table
    );
    let catalog = TypeCatalog::load_required()?;

    if let Some(of_type) = text_field(&effect.args, "of_type") {
        let resolved = catalog.resolve(&of_type)?;
        let mut prepared = effect.clone();
        upsert_arg(&mut prepared.args, "columns", columns_value(&resolved));
        return Ok(PreparedEffect {
            effect: prepared,
            record: Some(TableContractRecord {
                table_path,
                of_type: Some(of_type),
                body: None,
            }),
        });
    }

    let Some(columns) = value_field(&effect.args, "columns") else {
        return Ok(PreparedEffect {
            effect: effect.clone(),
            record: None,
        });
    };
    let body = type_body_from_columns_value(columns)?;
    let has_declared_refs = body_has_declared_refs(&body);
    if !has_declared_refs {
        return Ok(PreparedEffect {
            effect: effect.clone(),
            record: None,
        });
    }

    let resolved = catalog.resolve_body(&body)?;
    let mut prepared = effect.clone();
    upsert_arg(&mut prepared.args, "columns", columns_value(&resolved));
    Ok(PreparedEffect {
        effect: prepared,
        record: Some(TableContractRecord {
            table_path,
            of_type: None,
            body: Some(body),
        }),
    })
}

fn check_table_membership(effect: &EffectInput) -> Result<(), EffectError> {
    if effect.args.rows.is_empty() {
        return Ok(());
    }
    let Some(contract) = load_table_contract(effect.target.path.as_str())? else {
        return Ok(());
    };
    for row in &effect.args.rows {
        let shaped = shape_row(&effect.args.schema, row, &contract.schema);
        if let Some(pred) = &contract.refinement {
            qfs_core::check_membership(&contract.schema, pred, &shaped).map_err(|e| {
                EffectError::terminal(format!(
                    "row violates `{}` for `{}`: {e}",
                    contract.label, effect.target.path
                ))
            })?;
        }
        for refinement in &contract.column_refinements {
            check_column_refinement(effect, &contract.schema, row, refinement)?;
        }
    }
    Ok(())
}

fn check_column_refinement(
    effect: &EffectInput,
    input_schema: &Schema,
    row: &Row,
    refinement: &ColumnRefinement,
) -> Result<(), EffectError> {
    let value = input_schema
        .columns
        .iter()
        .position(|c| c.name == refinement.column)
        .and_then(|idx| row.values.get(idx).cloned())
        .unwrap_or(Value::Null);
    qfs_core::check_membership(
        &refinement.schema,
        &refinement.predicate,
        &Row::new(vec![value]),
    )
    .map_err(|e| {
        EffectError::terminal(format!(
            "row violates `{}` for `{}` column `{}`: {e}",
            refinement.ty, effect.target.path, refinement.column
        ))
    })
}

struct TableContract {
    label: String,
    schema: Schema,
    refinement: Option<qfs_exec::Expr>,
    column_refinements: Vec<ColumnRefinement>,
}

fn load_table_contract(table_path: &str) -> Result<Option<TableContract>, EffectError> {
    let Some(sys) = crate::store::open_system_db()
        .map_err(|e| EffectError::terminal(format!("opening System DB: {e}")))?
    else {
        return Ok(None);
    };
    let conn = sys.into_db().into_connection();
    let row: Option<(Option<String>, Option<String>)> = conn
        .query_row(
            "SELECT of_type, body FROM sys_drivers WHERE kind = 'table' AND name = ?1 \
             ORDER BY id DESC LIMIT 1",
            [table_path],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(|e| EffectError::terminal(format!("reading table contract: {e}")))?;
    let Some((of_type, body)) = row else {
        return Ok(None);
    };
    let catalog = TypeCatalog::from_conn(&conn)?;
    let (label, resolved) = match (of_type, body) {
        (Some(path), _) => {
            let resolved = catalog.resolve(&path)?;
            (path, resolved)
        }
        (None, Some(body)) => (
            "inline table contract".to_string(),
            catalog.resolve_body(&body)?,
        ),
        (None, None) => return Ok(None),
    };
    Ok(Some(TableContract {
        label,
        schema: resolved.schema,
        refinement: resolved.refinement,
        column_refinements: resolved.column_refinements,
    }))
}

fn persist_table_contract(record: &TableContractRecord) -> Result<(), EffectError> {
    let Some(sys) = crate::store::open_system_db()
        .map_err(|e| EffectError::terminal(format!("opening System DB: {e}")))?
    else {
        return Err(EffectError::terminal(
            "CREATE TABLE OF requires the System DB type catalog",
        ));
    };
    let conn = sys.into_db().into_connection();
    conn.execute(
        "INSERT INTO sys_drivers (kind, name, of_type, body) VALUES ('table', ?1, ?2, ?3)",
        rusqlite::params![record.table_path, record.of_type, record.body],
    )
    .map_err(|e| EffectError::terminal(format!("recording table contract: {e}")))?;
    Ok(())
}

#[derive(Clone)]
struct TypeCatalog {
    entries: Vec<(String, String)>,
}

impl TypeCatalog {
    fn load_required() -> Result<Self, EffectError> {
        let Some(sys) = crate::store::open_system_db()
            .map_err(|e| EffectError::terminal(format!("opening System DB: {e}")))?
        else {
            return Err(EffectError::terminal(
                "CREATE TABLE OF requires the System DB type catalog",
            ));
        };
        let conn = sys.into_db().into_connection();
        Self::from_conn(&conn)
    }

    fn from_conn(conn: &rusqlite::Connection) -> Result<Self, EffectError> {
        let mut stmt = conn
            .prepare("SELECT name, body FROM sys_drivers WHERE kind = 'type' ORDER BY id")
            .map_err(|e| EffectError::terminal(format!("reading type catalog: {e}")))?;
        let entries = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, Option<String>>(1)?.unwrap_or_default(),
                ))
            })
            .map_err(|e| EffectError::terminal(format!("reading type catalog: {e}")))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| EffectError::terminal(format!("reading type catalog: {e}")))?;
        Ok(Self { entries })
    }

    fn body(&self, path: &str) -> Option<String> {
        self.entries
            .iter()
            .rev()
            .find(|(name, _)| name == path)
            .map(|(_, body)| body.clone())
    }

    fn resolve(&self, path: &str) -> Result<ResolvedTypeDef, EffectError> {
        let body = self
            .body(path)
            .ok_or_else(|| EffectError::terminal(format!("unknown declared type `{path}`")))?;
        self.resolve_body(&body)
    }

    fn resolve_body(&self, body: &str) -> Result<ResolvedTypeDef, EffectError> {
        qfs_core::ddl::types::resolve_type_def(body, |path| self.body(path))
            .map_err(|e| EffectError::terminal(e.to_string()))
    }
}

fn shape_row(input_schema: &Schema, row: &Row, contract_schema: &Schema) -> Row {
    Row::new(
        contract_schema
            .columns
            .iter()
            .map(|contract_col| {
                input_schema
                    .columns
                    .iter()
                    .position(|c| c.name == contract_col.name)
                    .and_then(|idx| row.values.get(idx).cloned())
                    .unwrap_or(Value::Null)
            })
            .collect(),
    )
}

fn columns_value(resolved: &ResolvedTypeDef) -> Value {
    Value::Array(
        resolved
            .columns
            .iter()
            .zip(resolved.schema.columns.iter())
            .map(|(decl, col)| {
                Value::Struct(Fields::new(vec![
                    ("name".to_string(), Value::Text(decl.name.clone())),
                    (
                        "type".to_string(),
                        Value::Text(col.ty.type_token().to_string()),
                    ),
                    ("nullable".to_string(), Value::Bool(decl.nullable)),
                    ("primary_key".to_string(), Value::Bool(decl.primary_key)),
                    ("unique".to_string(), Value::Bool(decl.unique)),
                ]))
            })
            .collect(),
    )
}

fn body_has_declared_refs(body: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v.get("columns").and_then(|c| c.as_array()).cloned())
        .unwrap_or_default()
        .iter()
        .filter_map(|c| c.get("type").and_then(|t| t.as_str()))
        .any(|ty| ty.starts_with("/type/"))
}

fn type_body_from_columns_value(value: &Value) -> Result<String, EffectError> {
    let Value::Array(items) = value else {
        return Err(EffectError::terminal(
            "CREATE TABLE columns must be an array of column definitions",
        ));
    };
    let mut cols = Vec::with_capacity(items.len());
    for item in items {
        let Value::Struct(fields) = item else {
            return Err(EffectError::terminal(
                "CREATE TABLE column definitions must be structs",
            ));
        };
        let name = field_text(fields, "name").ok_or_else(|| {
            EffectError::terminal("CREATE TABLE column definition needs a text `name`")
        })?;
        let ty = field_text(fields, "type").unwrap_or_else(|| "text".to_string());
        cols.push(serde_json::json!({
            "name": name,
            "type": ty,
            "nullable": field_bool(fields, "nullable").unwrap_or(true),
            "primary_key": field_bool(fields, "primary_key").unwrap_or(false),
            "unique": field_bool(fields, "unique").unwrap_or(false),
        }));
    }
    Ok(serde_json::json!({ "columns": cols, "where": null }).to_string())
}

fn field_text(fields: &Fields, name: &str) -> Option<String> {
    match fields.get(name) {
        Some(Value::Text(s)) => Some(s.clone()),
        _ => None,
    }
}

fn field_bool(fields: &Fields, name: &str) -> Option<bool> {
    match fields.get(name) {
        Some(Value::Bool(b)) => Some(*b),
        _ => None,
    }
}

fn text_field(batch: &RowBatch, name: &str) -> Option<String> {
    match value_field(batch, name) {
        Some(Value::Text(s)) => Some(s.clone()),
        _ => None,
    }
}

fn value_field<'a>(batch: &'a RowBatch, name: &str) -> Option<&'a Value> {
    let idx = batch.schema.columns.iter().position(|c| c.name == name)?;
    batch.rows.first()?.values.get(idx)
}

fn upsert_arg(batch: &mut RowBatch, name: &str, value: Value) {
    if let Some(idx) = batch.schema.columns.iter().position(|c| c.name == name) {
        for row in &mut batch.rows {
            if let Some(cell) = row.values.get_mut(idx) {
                *cell = value.clone();
            }
        }
        return;
    }
    batch.schema.columns.push(Column::new(
        name.to_string(),
        ColumnType::Array(Box::new(ColumnType::Json)),
        false,
    ));
    for row in &mut batch.rows {
        row.values.push(value.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use qfs_core::{Affected, EffectNode, NodeId, Target, VfsPath};
    use qfs_types::{DriverId, Row};

    fn type_body(columns: serde_json::Value, where_: serde_json::Value) -> String {
        serde_json::json!({ "columns": columns, "where": where_ }).to_string()
    }

    fn email_body() -> String {
        type_body(
            serde_json::json!([
                {
                    "name": "value",
                    "type": "text",
                    "nullable": true,
                    "primary_key": false,
                    "unique": false
                }
            ]),
            serde_json::json!({
                "Like": {
                    "expr": { "Col": "value" },
                    "pattern": { "Lit": { "Str": "%@%" } }
                }
            }),
        )
    }

    fn customer_body() -> String {
        type_body(
            serde_json::json!([
                {
                    "name": "id",
                    "type": "int",
                    "nullable": true,
                    "primary_key": true,
                    "unique": false
                },
                {
                    "name": "email",
                    "type": "/type/email",
                    "nullable": true,
                    "primary_key": false,
                    "unique": false
                }
            ]),
            serde_json::Value::Null,
        )
    }

    fn seed_type(path: &str, body: &str) {
        let sys = crate::store::open_system_db().unwrap().unwrap();
        let conn = sys.into_db().into_connection();
        conn.execute(
            "INSERT INTO sys_drivers (kind, name, body) VALUES ('type', ?1, ?2)",
            rusqlite::params![path, body],
        )
        .unwrap();
    }

    fn input(
        id: u32,
        kind: EffectKind,
        path: &str,
        schema: Schema,
        values: Vec<Value>,
    ) -> EffectInput {
        let node = EffectNode::new(
            NodeId(id),
            kind,
            Target::new(DriverId::new("sql"), VfsPath::new(path)),
        )
        .with_args(RowBatch::new(schema, vec![Row::new(values)]))
        .with_affected(Affected::Unknown);
        EffectInput::from_node(&node)
    }

    #[tokio::test]
    async fn create_table_of_resolves_columns_and_checks_later_inserts() {
        let _home = crate::testenv::HomeGuard::new();
        let email = email_body();
        let customer = customer_body();
        seed_type("/type/email", &email);
        seed_type("/type/customer", &customer);

        let (_path, driver) = crate::sql::seeded_test_driver("shop", "");
        let inner: Arc<dyn ApplyDriver> = Arc::new(qfs_driver_sql::sql_apply_driver(&driver));
        let wrapper = SqlContractApplyDriver::new(inner);

        let create_schema = Schema::new(vec![
            Column::new("name", ColumnType::Text, false),
            Column::new("of_type", ColumnType::Text, false),
        ]);
        let create = input(
            1,
            EffectKind::Insert,
            "/sql/shop",
            create_schema,
            vec![
                Value::Text("customers".to_string()),
                Value::Text("/type/customer".to_string()),
            ],
        );
        wrapper
            .apply_one(&create, &ApplyCx::default())
            .await
            .expect("CREATE TABLE OF applies");

        let row_schema = Schema::new(vec![
            Column::new("id", ColumnType::Int, false),
            Column::new("email", ColumnType::Text, false),
        ]);
        let ok = input(
            2,
            EffectKind::Insert,
            "/sql/shop/customers",
            row_schema.clone(),
            vec![Value::Int(1), Value::Text("a@b.test".to_string())],
        );
        wrapper
            .apply_one(&ok, &ApplyCx::default())
            .await
            .expect("conforming row applies");

        let bad = input(
            3,
            EffectKind::Insert,
            "/sql/shop/customers",
            row_schema,
            vec![Value::Int(2), Value::Text("not-an-email".to_string())],
        );
        let err = wrapper
            .apply_one(&bad, &ApplyCx::default())
            .await
            .expect_err("violating row is refused");
        let shown = err.to_string();
        assert!(shown.contains("email"), "{shown}");
        assert!(shown.contains("Like"), "{shown}");
    }
}
