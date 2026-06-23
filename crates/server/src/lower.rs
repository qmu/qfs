//! Lower a server-config statement to a **pure** `/server` write plan (RFD-0001 §6/§8).
//!
//! This is the single desugaring seam that makes the closed-core thesis hold for the
//! server: a `CREATE JOB … EVERY … DO …` and the equivalent `INSERT INTO /server/jobs …`
//! lower to **identical** [`EffectKind::ServerConfigWrite`](cfs_core::EffectKind::ServerConfigWrite)
//! plan nodes. Both paths normalise into the same canonical named-field row
//! ([`ConfigRow`]) and build the same [`cfs_core::RowBatch`] (columns in the node's schema
//! order), so the frozen `CREATE …` DDL is provably **sugar** over the `/server` write
//! (acceptance: golden plan equivalence).
//!
//! Lowering is **pure**: it constructs a [`cfs_core::Plan`] and mutates **nothing** — the
//! interpreter mutates [`ServerState`](crate::ServerState) only at `COMMIT`.

use std::collections::BTreeMap;

use cfs_core::{
    Affected, Column, EffectKind, EffectNode, Plan, PlanBuilder, Row, RowBatch, ServerNode,
    ServerWriteOp, Target, Value, VfsPath,
};
use cfs_parser::{
    DdlKind, EffectBody, EffectStmt, EffectVerb, Expr, Literal, ServerDdl, Statement, Values,
};

use crate::driver::{server_node_schema, SERVER_MOUNT};

/// The canonical, normalised config row both lowering paths produce: a name-keyed map of
/// field → value. Keyed/sorted by name so the resulting [`RowBatch`] is deterministic and
/// the CREATE-vs-INSERT equivalence is exact (byte-identical plan nodes).
#[derive(Debug, Clone, Default)]
pub struct ConfigRow {
    fields: BTreeMap<String, Value>,
}

impl ConfigRow {
    fn set(&mut self, key: &str, value: Value) {
        self.fields.insert(key.to_string(), value);
    }

    fn set_text(&mut self, key: &str, text: impl Into<String>) {
        self.fields
            .insert(key.to_string(), Value::Text(text.into()));
    }
}

/// The driver id `/server` writes route to (the mount stripped of its leading `/`).
fn server_driver_id() -> cfs_core::DriverId {
    cfs_core::DriverId::new(SERVER_MOUNT.trim_start_matches('/'))
}

/// Build the canonical [`RowBatch`] for `node` from a normalised [`ConfigRow`], emitting
/// columns in the node's schema order (the single source of truth shared with `DESCRIBE`),
/// filling absent fields with `Null`. This canonical ordering is what makes the CREATE and
/// INSERT lowerings produce identical batches.
#[must_use]
pub fn config_row_batch(node: ServerNode, row: &ConfigRow) -> RowBatch {
    let schema = server_node_schema(node);
    let mut cols = Vec::with_capacity(schema.columns.len());
    let mut values = Vec::with_capacity(schema.columns.len());
    for col in &schema.columns {
        let v = row.fields.get(&col.name).cloned().unwrap_or(Value::Null);
        cols.push(Column::new(col.name.clone(), col.ty.clone(), col.nullable));
        values.push(v);
    }
    RowBatch::new(cfs_core::Schema::new(cols), vec![Row::new(values)])
}

/// Assemble a one-node `/server` write [`Plan`] (the pure effect node). `irreversible =
/// false` (config writes are reversible, RFD §6); the affected estimate is `Exact(1)` (one
/// config row). Building this mutates nothing — the COMMIT-time apply is the only impure op.
#[must_use]
pub fn server_write_plan(node: ServerNode, op: ServerWriteOp, args: RowBatch) -> Plan {
    let mut builder = PlanBuilder::new();
    let target = Target::new(server_driver_id(), VfsPath::new(node.path()));
    let effect = EffectNode::new(
        builder.next_id(),
        EffectKind::ServerConfigWrite { node, op },
        target,
    )
    .with_args(args)
    .with_affected(Affected::Exact(1));
    builder.push(effect);
    builder.build()
}

/// The kind of write a parsed statement lowers to, plus the normalised row — the seam both
/// `CREATE` and `INSERT` resolve into before building the (identical) plan.
struct Lowered {
    node: ServerNode,
    op: ServerWriteOp,
    row: ConfigRow,
}

/// Lower a parsed [`Statement`] to a `/server` config write plan, or `Ok(None)` if the
/// statement is not a server-config statement (the caller decides whether that is an error).
///
/// Handles both forms:
/// - `Statement::Ddl(CREATE …)` — the frozen DDL **sugar**;
/// - `Statement::Effect(INSERT/UPSERT/UPDATE/REMOVE INTO /server/<node> …)` — the explicit
///   write the sugar desugars to.
///
/// Both normalise into the same [`Lowered`] and so produce **identical** plan nodes.
///
/// # Errors
/// A secret-free detail string if the statement targets `/server` but is malformed (bad
/// node segment, unsupported verb, non-literal values).
pub fn lower_statement(stmt: &Statement) -> Result<Option<Plan>, String> {
    let lowered = match stmt {
        Statement::Ddl(ddl) => Some(lower_ddl(ddl)?),
        Statement::Effect(effect) if targets_server(effect) => Some(lower_effect(effect)?),
        // A `PREVIEW`/`COMMIT` wrapper around a server statement unwraps transparently.
        Statement::Plan(wrap) => return lower_statement(&wrap.inner),
        _ => None,
    };
    Ok(lowered.map(|l| server_write_plan(l.node, l.op, config_row_batch(l.node, &l.row))))
}

/// Whether an effect statement targets the `/server/...` mount.
fn targets_server(effect: &EffectStmt) -> bool {
    effect
        .target
        .segments
        .first()
        .is_some_and(|s| s.name == "server")
}

/// Lower a `CREATE … DDL` to the normalised config write. This is the **desugar**: each DDL
/// clause maps onto the canonical config field, identical to what the equivalent
/// `INSERT INTO /server/<node>` carries.
fn lower_ddl(ddl: &ServerDdl) -> Result<Lowered, String> {
    let node = ddl_node(ddl.kind);
    let mut row = ConfigRow::default();
    row.set_text("name", ddl.name.clone());

    match ddl.kind {
        DdlKind::Endpoint => {
            // `ON 'METHOD /route'` → method + route; `AS <query>` → query source text.
            let (method, route) = split_method_route(ddl.on.as_deref().unwrap_or(""));
            row.set_text("method", method);
            row.set_text("route", route);
            row.set_text("query", statement_src(ddl.as_query.as_deref()));
        }
        DdlKind::Trigger => {
            row.set_text("on", ddl.on.clone().unwrap_or_default());
            row.set_text("plan", statement_src(ddl.do_plan.as_deref()));
        }
        DdlKind::Job => {
            row.set_text("every", ddl.every.clone().unwrap_or_default());
            row.set_text("plan", statement_src(ddl.do_plan.as_deref()));
        }
        DdlKind::View => {
            row.set_text("query", statement_src(ddl.as_query.as_deref()));
            row.set("materialized", Value::Bool(false));
        }
        DdlKind::MaterializedView => {
            row.set_text("query", statement_src(ddl.as_query.as_deref()));
            row.set("materialized", Value::Bool(true));
        }
        DdlKind::Policy => {
            row.set_text("handler", ddl.on.clone().unwrap_or_default());
            row.set("allow", Value::Array(Vec::new()));
        }
        DdlKind::Webhook => {
            row.set_text("route", ddl.on.clone().unwrap_or_default());
        }
    }

    // CREATE is a declarative "make this exist" — its retry/replay-safe verb is UPSERT
    // (RFD §6 idempotency), which is exactly what a boot replay needs.
    Ok(Lowered {
        node,
        op: ServerWriteOp::Upsert,
        row,
    })
}

/// Map a [`DdlKind`] to its [`ServerNode`]. `MaterializedView` and `View` share the
/// `views` collection (a materialized view is a view row with `materialized = true`).
fn ddl_node(kind: DdlKind) -> ServerNode {
    match kind {
        DdlKind::Endpoint => ServerNode::Endpoints,
        DdlKind::Trigger => ServerNode::Triggers,
        DdlKind::Job => ServerNode::Jobs,
        DdlKind::View | DdlKind::MaterializedView => ServerNode::Views,
        DdlKind::Policy => ServerNode::Policies,
        DdlKind::Webhook => ServerNode::Webhooks,
    }
}

/// Lower an explicit `INSERT/UPSERT/UPDATE/REMOVE INTO /server/<node> …` to the normalised
/// config write. The `/server/<node>` segment selects the collection; `VALUES (cols)(vals)`
/// (or `SET`) carries the named fields.
fn lower_effect(effect: &EffectStmt) -> Result<Lowered, String> {
    let segments: Vec<&str> = effect
        .target
        .segments
        .iter()
        .map(|s| s.name.as_str())
        .collect();
    let node_seg = segments
        .get(1)
        .ok_or_else(|| "/server write needs a collection, e.g. /server/jobs".to_string())?;
    let node = ServerNode::from_segment(node_seg)
        .ok_or_else(|| format!("unknown /server collection `{node_seg}`"))?;

    let op = match effect.verb {
        EffectVerb::Insert => ServerWriteOp::Insert,
        EffectVerb::Upsert => ServerWriteOp::Upsert,
        EffectVerb::Update => ServerWriteOp::Update,
        EffectVerb::Remove => ServerWriteOp::Remove,
    };

    let mut row = ConfigRow::default();
    // A trailing path segment after the collection names the row (e.g.
    // `REMOVE /server/jobs/nightly`); else the row name comes from a `name` column.
    if let Some(name_seg) = segments.get(2) {
        row.set_text("name", (*name_seg).to_string());
    }

    match &effect.body {
        EffectBody::Values(values) => {
            collect_values_row(values, &mut row)?;
        }
        EffectBody::SetWhere { set, .. } => {
            for assignment in set {
                row.set(&assignment.name, literal_value(&assignment.value)?);
            }
        }
        EffectBody::Pipeline(_) => {
            return Err("/server writes take VALUES / SET, not a sub-pipeline".to_string());
        }
    }

    Ok(Lowered { node, op, row })
}

/// Collect a single-row `VALUES (col, …)(val, …)` into the normalised [`ConfigRow`]. Only a
/// single literal row is supported (a config write names one row); explicit column names are
/// required so the mapping to named fields is unambiguous.
fn collect_values_row(values: &Values, row: &mut ConfigRow) -> Result<(), String> {
    let columns = values.columns.as_ref().ok_or_else(|| {
        "/server INSERT requires explicit column names, e.g. VALUES (name, every) (...)".to_string()
    })?;
    let first = values
        .rows
        .first()
        .ok_or_else(|| "/server INSERT requires one VALUES row".to_string())?;
    if columns.len() != first.len() {
        return Err(format!(
            "/server INSERT column/value count mismatch ({} cols, {} vals)",
            columns.len(),
            first.len()
        ));
    }
    for (col, expr) in columns.iter().zip(first) {
        row.set(col, literal_value(expr)?);
    }
    Ok(())
}

/// Extract a [`Value`] from a literal expression. Server-config writes carry only literals
/// (names, routes, intervals, plan source text) — a non-literal is a malformed config write.
fn literal_value(expr: &Expr) -> Result<Value, String> {
    match expr {
        Expr::Lit(lit) => Ok(match lit {
            Literal::Str(s) => Value::Text(s.clone()),
            Literal::Int(n) => Value::Int(*n),
            Literal::Float(f) => Value::Float(*f),
            Literal::Bool(b) => Value::Bool(*b),
            Literal::Null => Value::Null,
            Literal::Size { value, unit } => Value::Text(format!("{value} {unit}")),
            Literal::Typed { raw, .. } => Value::Text(raw.clone()),
        }),
        other => Err(format!(
            "/server config values must be literals, got {}",
            expr_kind(other)
        )),
    }
}

/// A short, secret-free label for a non-literal expression (for the structured error).
fn expr_kind(expr: &Expr) -> &'static str {
    match expr {
        Expr::Lit(_) => "literal",
        Expr::Col(_) => "column reference",
        Expr::Path(_) => "path",
        Expr::Fn(_) => "function call",
        Expr::Binary { .. } => "binary op",
        Expr::Unary { .. } => "unary op",
        Expr::In { .. } => "IN",
        Expr::Between { .. } => "BETWEEN",
        Expr::Like { .. } => "LIKE",
        Expr::AnyOp { .. } => "ANY",
    }
}

/// Render an optional inner statement (`AS <query>` / `DO <plan>`) back to canonical source
/// text — the plan body the binding (E7) re-parses and lowers when it fires. Stored as text
/// so [`ServerState`] stays `Serialize`/snapshot-stable.
fn statement_src(stmt: Option<&Statement>) -> String {
    stmt.map(render_statement).unwrap_or_default()
}

/// A minimal, deterministic source rendering of a statement body. This is intentionally a
/// small, stable projection (not a full pretty-printer): the E7 bindings re-parse it; the
/// CREATE-vs-INSERT equivalence test compares the *plan node*, where the equivalent INSERT
/// supplies the same source text literally, so the two match.
fn render_statement(stmt: &Statement) -> String {
    // The parser does not ship a Display/round-trip renderer; for the bodies the server
    // stores (`AS <query>` / `DO <plan>`) we keep the structured debug as a stable,
    // deterministic textual key. Equivalence tests supply the same literal on both sides.
    format!("{stmt:?}")
}

/// Split an endpoint `ON` token of the form `'METHOD /route'` into `(method, route)`.
/// A bare route (`/route`) yields an empty method.
fn split_method_route(on: &str) -> (String, String) {
    let trimmed = on.trim();
    match trimmed.split_once(char::is_whitespace) {
        Some((method, route)) => (method.trim().to_uppercase(), route.trim().to_string()),
        None => (String::new(), trimmed.to_string()),
    }
}
