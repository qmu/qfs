//! Production **Postgres** and **MySQL** [`SqlBackend`] implementations (t-203060), confined to the
//! terminal binary exactly like the SQLite backend (`crate::sql`). The pure-Rust `postgres` /
//! `mysql` engine clients dead-end HERE; `qfs-driver-sql` is the vendor-free trait + dialect
//! compiler and never sees an engine type. No vendor type crosses the [`SqlBackend`] boundary —
//! only owned qfs DTOs ([`Param`] in, [`Row`]/[`Catalog`] out). The clients are sync, held behind a
//! `Mutex` to satisfy the `Send + Sync` trait; `NoTls` because the dev compose stack is local.
//!
//! Guarantees mirror the SQLite backend: every value is a **bound** parameter (injection-safe), and
//! a multi-op commit is one **ACID** transaction (`BEGIN → ops → COMMIT`, auto-`ROLLBACK` on error).
//!
//! Type coverage targets the common column set the dev stack seeds plus rich Postgres values
//! (NUMERIC / TIMESTAMP / UUID / JSON) normalized to qfs's canonical text values. An unmapped
//! column type falls back to its text rendering.

use std::error::Error;
use std::fmt::Write as _;
use std::io;
use std::str::FromStr;
use std::sync::mpsc;
use std::sync::Mutex;

use mysql::prelude::Queryable;
use qfs_driver_sql::{
    render_dml, Catalog, ColumnDef, Dialect, DmlOp, Param, RelationKind, SqlBackend, SqlError,
    TableCatalog,
};
use qfs_types::{Row, Value};

// ---------------------------------------------------------------------------------------------
// Postgres
// ---------------------------------------------------------------------------------------------

/// One request to the Postgres worker thread. The reply rides back on a per-request channel.
enum PgReq {
    Introspect(mpsc::Sender<Result<Catalog, SqlError>>),
    Read {
        sql: String,
        params: Vec<Param>,
        reply: mpsc::Sender<Result<Vec<Row>, SqlError>>,
    },
    Commit {
        ops: Vec<DmlOp>,
        reply: mpsc::Sender<Result<u64, SqlError>>,
    },
}

/// A live Postgres backend. The sync `postgres` client wraps `tokio-postgres` and drives its OWN
/// tokio runtime, which panics ("runtime within a runtime") if called from inside qfs's runtime
/// (the async read executor). So the client lives on a DEDICATED OS thread — which has no outer
/// runtime — and the sync [`SqlBackend`] methods talk to it over channels. This fully isolates the
/// engine's runtime from qfs's; no `postgres` type crosses the channel (owned qfs DTOs only).
pub struct PostgresBackend {
    req: Mutex<mpsc::Sender<PgReq>>,
}

impl PostgresBackend {
    /// Connect to Postgres at `locator` (a libpq/URL connection string), injecting `password` when
    /// the locator does not already carry one (resolved from the connection's `SECRET 'ref'`). The
    /// connection is established ON the worker thread; this returns once it succeeds or fails.
    pub fn connect(locator: &str, password: Option<&str>) -> Result<Self, SqlError> {
        let locator = locator.to_string();
        let password = password.map(str::to_string);
        let (setup_tx, setup_rx) = mpsc::channel::<Result<(), SqlError>>();
        let (req_tx, req_rx) = mpsc::channel::<PgReq>();
        std::thread::Builder::new()
            .name("qfs-postgres".into())
            .spawn(move || {
                let connect = || -> Result<postgres::Client, SqlError> {
                    let mut cfg = postgres::Config::from_str(&locator)
                        .map_err(|e| SqlError::backend("postgres", "config", e.to_string()))?;
                    if let Some(pw) = &password {
                        cfg.password(pw);
                    }
                    cfg.connect(postgres::NoTls)
                        .map_err(|e| SqlError::backend("postgres", "connect", e.to_string()))
                };
                match connect() {
                    Ok(mut client) => {
                        let _ = setup_tx.send(Ok(()));
                        pg_worker(&mut client, &req_rx);
                    }
                    Err(e) => {
                        let _ = setup_tx.send(Err(e));
                    }
                }
            })
            .map_err(|e| SqlError::backend("postgres", "spawn", e.to_string()))?;
        setup_rx
            .recv()
            .map_err(|_| SqlError::backend("postgres", "setup", "worker thread exited"))??;
        Ok(Self {
            req: Mutex::new(req_tx),
        })
    }

    /// Send a request to the worker and block on its reply (the sync side of the channel actor).
    fn dispatch<T>(
        &self,
        req: PgReq,
        reply_rx: &mpsc::Receiver<Result<T, SqlError>>,
    ) -> Result<T, SqlError> {
        self.req
            .lock()
            .map_err(|_| SqlError::backend("postgres", "lock", "poisoned request channel"))?
            .send(req)
            .map_err(|_| SqlError::backend("postgres", "send", "worker thread gone"))?;
        reply_rx
            .recv()
            .map_err(|_| SqlError::backend("postgres", "recv", "worker thread gone"))?
    }
}

/// The worker loop: own the `postgres::Client` on this runtime-free thread and serve requests until
/// the channel closes (the backend dropped).
fn pg_worker(client: &mut postgres::Client, rx: &mpsc::Receiver<PgReq>) {
    while let Ok(req) = rx.recv() {
        match req {
            PgReq::Introspect(reply) => {
                let _ = reply.send(pg_introspect(client));
            }
            PgReq::Read { sql, params, reply } => {
                let _ = reply.send(pg_read(client, &sql, &params));
            }
            PgReq::Commit { ops, reply } => {
                let _ = reply.send(pg_commit(client, &ops));
            }
        }
    }
}

/// A `postgres` bind value that adapts a qfs [`Param`] to whatever SQL type the server INFERS for
/// the placeholder. rust-postgres is otherwise strict: a bare `i64` is rejected against an `int4`
/// column even though Postgres itself compares them fine. Adapting the encoding to the inferred
/// `Type` keeps every value BOUND (injection-safe) while accepting any integer/float width.
#[derive(Debug)]
struct PgBind(Param);

impl postgres::types::ToSql for PgBind {
    fn to_sql(
        &self,
        ty: &postgres::types::Type,
        out: &mut bytes::BytesMut,
    ) -> Result<postgres::types::IsNull, Box<dyn std::error::Error + Sync + Send>> {
        use postgres::types::{IsNull, Type};
        match &self.0 {
            Param::Null => Ok(IsNull::Yes),
            Param::Bool(b) => b.to_sql(ty, out),
            Param::Int(n) => match *ty {
                Type::INT2 => i16::try_from(*n)?.to_sql(ty, out),
                Type::INT4 => i32::try_from(*n)?.to_sql(ty, out),
                _ => n.to_sql(ty, out),
            },
            Param::Float(f) => {
                if *ty == Type::FLOAT4 {
                    #[allow(clippy::cast_possible_truncation)]
                    let narrowed = *f as f32;
                    narrowed.to_sql(ty, out)
                } else {
                    f.to_sql(ty, out)
                }
            }
            Param::Text(s) => s.to_sql(ty, out),
            Param::Bytes(b) => b.to_sql(ty, out),
        }
    }

    fn accepts(_ty: &postgres::types::Type) -> bool {
        true
    }

    postgres::types::to_sql_checked!();
}

/// Wrap each [`Param`] in the type-adapting [`PgBind`] (every value BOUND, never interpolated).
fn pg_params(params: &[Param]) -> Vec<PgBind> {
    params.iter().cloned().map(PgBind).collect()
}

/// Text-normalized Postgres NUMERIC. rust-postgres has no built-in arbitrary-decimal adapter in
/// this version, so decode the stable binary NUMERIC wire format locally and keep the qfs boundary
/// as `Value::Text`.
struct PgNumericText(String);

impl<'a> postgres::types::FromSql<'a> for PgNumericText {
    fn from_sql(
        _ty: &postgres::types::Type,
        raw: &'a [u8],
    ) -> Result<Self, Box<dyn Error + Sync + Send>> {
        pg_numeric_text(raw).map(Self)
    }

    fn accepts(ty: &postgres::types::Type) -> bool {
        *ty == postgres::types::Type::NUMERIC
    }
}

/// Convert the `i`-th column of a Postgres row into the canonical qfs [`Value`] (the owned-DTO
/// boundary — no `postgres` type crosses past here). Branches on the column's runtime type; rich
/// scalar types are decoded through rust-postgres' feature adapters and normalized to text so
/// NUMERIC / TIMESTAMP / UUID / JSON round-trip without leaking vendor values.
fn pg_value(row: &postgres::Row, i: usize) -> Result<Value, SqlError> {
    use postgres::types::Type;
    let decode = |e: postgres::Error| SqlError::backend("postgres", "decode", e.to_string());
    let v = match *row.columns()[i].type_() {
        Type::BOOL => row
            .try_get::<_, Option<bool>>(i)
            .map(|o| o.map_or(Value::Null, Value::Bool)),
        Type::INT2 => row
            .try_get::<_, Option<i16>>(i)
            .map(|o| o.map_or(Value::Null, |n| Value::Int(i64::from(n)))),
        Type::INT4 => row
            .try_get::<_, Option<i32>>(i)
            .map(|o| o.map_or(Value::Null, |n| Value::Int(i64::from(n)))),
        Type::INT8 => row
            .try_get::<_, Option<i64>>(i)
            .map(|o| o.map_or(Value::Null, Value::Int)),
        Type::FLOAT4 => row
            .try_get::<_, Option<f32>>(i)
            .map(|o| o.map_or(Value::Null, |n| Value::Float(f64::from(n)))),
        Type::FLOAT8 => row
            .try_get::<_, Option<f64>>(i)
            .map(|o| o.map_or(Value::Null, Value::Float)),
        Type::BYTEA => row
            .try_get::<_, Option<Vec<u8>>>(i)
            .map(|o| o.map_or(Value::Null, Value::Bytes)),
        Type::NUMERIC => row
            .try_get::<_, Option<PgNumericText>>(i)
            .map(|o| o.map_or(Value::Null, |n| Value::Text(n.0))),
        Type::DATE => row
            .try_get::<_, Option<time::Date>>(i)
            .map(|o| o.map_or(Value::Null, |d| Value::Text(d.to_string()))),
        Type::TIMESTAMP => row
            .try_get::<_, Option<time::PrimitiveDateTime>>(i)
            .map(|o| o.map_or(Value::Null, |ts| Value::Text(ts.to_string()))),
        Type::TIMESTAMPTZ => row
            .try_get::<_, Option<time::OffsetDateTime>>(i)
            .map(|o| o.map_or(Value::Null, |ts| Value::Text(ts.to_string()))),
        Type::UUID => row
            .try_get::<_, Option<uuid::Uuid>>(i)
            .map(|o| o.map_or(Value::Null, |id| Value::Text(id.to_string()))),
        Type::JSON | Type::JSONB => row
            .try_get::<_, Option<serde_json::Value>>(i)
            .map(|o| o.map_or(Value::Null, |json| Value::Text(json.to_string()))),
        _ => row
            .try_get::<_, Option<String>>(i)
            .map(|o| o.map_or(Value::Null, Value::Text)),
    };
    v.map_err(decode)
}

fn pg_numeric_text(raw: &[u8]) -> Result<String, Box<dyn Error + Sync + Send>> {
    let ndigits = usize::from(read_u16(raw, 0)?);
    let weight = read_i16(raw, 2)?;
    let sign = read_u16(raw, 4)?;
    let dscale = usize::from(read_u16(raw, 6)?);
    let mut digits = Vec::with_capacity(ndigits);
    for idx in 0..ndigits {
        digits.push(read_u16(raw, 8 + idx * 2)?);
    }

    match sign {
        0xC000 => return Ok("NaN".to_string()),
        0xD000 => return Ok("Infinity".to_string()),
        0xF000 => return Ok("-Infinity".to_string()),
        0x0000 | 0x4000 => {}
        _ => return Err(invalid_data("unknown postgres numeric sign")),
    }

    let groups_before_decimal = i32::from(weight) + 1;
    let mut int = String::new();
    if groups_before_decimal <= 0 {
        int.push('0');
    } else {
        for group in 0..usize::try_from(groups_before_decimal).unwrap_or(0) {
            let digit = digits.get(group).copied().unwrap_or(0);
            if int.is_empty() {
                int.push_str(&digit.to_string());
            } else {
                let _ = write!(&mut int, "{digit:04}");
            }
        }
        let trimmed = int.trim_start_matches('0');
        int = if trimmed.is_empty() {
            "0".to_string()
        } else {
            trimmed.to_string()
        };
    }

    let mut frac = String::new();
    if dscale > 0 {
        if groups_before_decimal < 0 {
            for _ in 0..usize::try_from(-groups_before_decimal).unwrap_or(0) {
                frac.push_str("0000");
            }
        }
        let start = usize::try_from(groups_before_decimal.max(0)).unwrap_or(0);
        for digit in digits.iter().skip(start) {
            let _ = write!(&mut frac, "{digit:04}");
        }
        while frac.len() < dscale {
            frac.push('0');
        }
        frac.truncate(dscale);
    }

    let mut out = String::new();
    if sign == 0x4000 {
        out.push('-');
    }
    out.push_str(&int);
    if dscale > 0 {
        out.push('.');
        out.push_str(&frac);
    }
    Ok(out)
}

fn read_u16(raw: &[u8], offset: usize) -> Result<u16, Box<dyn Error + Sync + Send>> {
    let bytes = raw
        .get(offset..offset + 2)
        .ok_or_else(|| invalid_data("truncated postgres numeric"))?;
    Ok(u16::from_be_bytes([bytes[0], bytes[1]]))
}

fn read_i16(raw: &[u8], offset: usize) -> Result<i16, Box<dyn Error + Sync + Send>> {
    let bytes = raw
        .get(offset..offset + 2)
        .ok_or_else(|| invalid_data("truncated postgres numeric"))?;
    Ok(i16::from_be_bytes([bytes[0], bytes[1]]))
}

fn invalid_data(message: &'static str) -> Box<dyn Error + Sync + Send> {
    Box::new(io::Error::new(io::ErrorKind::InvalidData, message))
}

/// Introspect the `public` schema into a [`Catalog`] (runs ON the worker thread).
fn pg_introspect(client: &mut postgres::Client) -> Result<Catalog, SqlError> {
    let err = |e: postgres::Error| SqlError::backend("postgres", "introspect", e.to_string());
    // Base tables + views in the public schema.
    let rels = client
        .query(
            "SELECT table_name, table_type FROM information_schema.tables \
             WHERE table_schema = 'public' ORDER BY table_name",
            &[],
        )
        .map_err(err)?;
    let mut tables = Vec::new();
    for rel in &rels {
        let name: String = rel.get(0);
        let kind: String = rel.get(1);
        // Primary-key columns for this table (for capability gating + UPSERT key).
        let pk_rows = client
            .query(
                "SELECT kcu.column_name FROM information_schema.table_constraints tc \
                 JOIN information_schema.key_column_usage kcu \
                   ON kcu.constraint_name = tc.constraint_name \
                  AND kcu.table_schema = tc.table_schema \
                 WHERE tc.constraint_type = 'PRIMARY KEY' AND tc.table_schema = 'public' \
                   AND tc.table_name = $1",
                &[&name],
            )
            .map_err(err)?;
        let pks: Vec<String> = pk_rows.iter().map(|r| r.get(0)).collect();
        let col_rows = client
            .query(
                "SELECT column_name, data_type, is_nullable FROM information_schema.columns \
                 WHERE table_schema = 'public' AND table_name = $1 ORDER BY ordinal_position",
                &[&name],
            )
            .map_err(err)?;
        let mut cols = Vec::new();
        for c in &col_rows {
            let col_name: String = c.get(0);
            let data_type: String = c.get(1);
            let nullable: String = c.get(2);
            let is_pk = pks.contains(&col_name);
            cols.push(ColumnDef::new(
                col_name,
                Dialect::Postgres.map_type(&data_type),
                nullable.eq_ignore_ascii_case("yes"),
                is_pk,
                is_pk,
            ));
        }
        let relkind = if kind.eq_ignore_ascii_case("view") {
            RelationKind::View
        } else {
            RelationKind::Table
        };
        tables.push(TableCatalog::new(name, relkind, cols));
    }
    Ok(Catalog::new(tables))
}

/// Run one parameterized `SELECT` into [`Row`]s (runs ON the worker thread).
fn pg_read(
    client: &mut postgres::Client,
    sql: &str,
    params: &[Param],
) -> Result<Vec<Row>, SqlError> {
    let owned = pg_params(params);
    let refs: Vec<&(dyn postgres::types::ToSql + Sync)> = owned
        .iter()
        .map(|p| p as &(dyn postgres::types::ToSql + Sync))
        .collect();
    let rows = client
        .query(sql, &refs)
        .map_err(|e| SqlError::backend("postgres", "select", e.to_string()))?;
    let mut out = Vec::with_capacity(rows.len());
    for row in &rows {
        let mut values = Vec::with_capacity(row.len());
        for i in 0..row.len() {
            values.push(pg_value(row, i)?);
        }
        out.push(Row::new(values));
    }
    Ok(out)
}

/// Apply a multi-op DML commit as ONE ACID transaction (runs ON the worker thread).
fn pg_commit(client: &mut postgres::Client, ops: &[DmlOp]) -> Result<u64, SqlError> {
    let mut tx = client
        .transaction()
        .map_err(|e| SqlError::backend("postgres", "begin", e.to_string()))?;
    let mut affected = 0u64;
    for op in ops {
        let (sql, params) = render_dml(Dialect::Postgres, op);
        let owned = pg_params(&params);
        let refs: Vec<&(dyn postgres::types::ToSql + Sync)> = owned
            .iter()
            .map(|p| p as &(dyn postgres::types::ToSql + Sync))
            .collect();
        // On ANY error `tx` is dropped without commit → automatic ROLLBACK (all-or-nothing).
        let n = tx
            .execute(sql.as_str(), &refs)
            .map_err(|e| SqlError::backend("postgres", "dml", e.to_string()))?;
        affected += n;
    }
    tx.commit()
        .map_err(|e| SqlError::backend("postgres", "commit", e.to_string()))?;
    Ok(affected)
}

impl SqlBackend for PostgresBackend {
    fn dialect(&self) -> Dialect {
        Dialect::Postgres
    }

    fn introspect(&self) -> Result<Catalog, SqlError> {
        let (reply, reply_rx) = mpsc::channel();
        self.dispatch(PgReq::Introspect(reply), &reply_rx)
    }

    fn execute_read(&self, sql: &str, params: &[Param]) -> Result<Vec<Row>, SqlError> {
        let (reply, reply_rx) = mpsc::channel();
        self.dispatch(
            PgReq::Read {
                sql: sql.to_string(),
                params: params.to_vec(),
                reply,
            },
            &reply_rx,
        )
    }

    fn commit_transaction(&self, ops: &[DmlOp]) -> Result<u64, SqlError> {
        let (reply, reply_rx) = mpsc::channel();
        self.dispatch(
            PgReq::Commit {
                ops: ops.to_vec(),
                reply,
            },
            &reply_rx,
        )
    }
}

// ---------------------------------------------------------------------------------------------
// MySQL
// ---------------------------------------------------------------------------------------------

/// A live MySQL backend wrapping a sync `mysql::Conn` behind a `Mutex`. `schema` is the connected
/// database name, used to scope `information_schema` introspection.
pub struct MysqlBackend {
    conn: Mutex<mysql::Conn>,
    schema: String,
}

impl MysqlBackend {
    /// Connect to MySQL at `locator` (a `mysql://` URL), injecting `password` when provided.
    pub fn connect(locator: &str, password: Option<&str>) -> Result<Self, SqlError> {
        let opts = mysql::Opts::from_url(locator)
            .map_err(|e| SqlError::backend("mysql", "config", e.to_string()))?;
        let schema = opts.get_db_name().unwrap_or_default().to_string();
        let builder = mysql::OptsBuilder::from_opts(opts);
        let builder = match password {
            Some(pw) => builder.pass(Some(pw.to_string())),
            None => builder,
        };
        let conn = mysql::Conn::new(builder)
            .map_err(|e| SqlError::backend("mysql", "connect", e.to_string()))?;
        Ok(Self {
            conn: Mutex::new(conn),
            schema,
        })
    }
}

/// Box each [`Param`] as a positional `mysql` value (bound, never interpolated). MySQL has no
/// boolean — a `Bool` binds as its `0`/`1` integer (the `TINYINT(1)` convention).
fn my_params(params: &[Param]) -> mysql::Params {
    let values: Vec<mysql::Value> = params
        .iter()
        .map(|p| match p {
            Param::Null => mysql::Value::NULL,
            Param::Bool(b) => mysql::Value::Int(i64::from(*b)),
            Param::Int(n) => mysql::Value::Int(*n),
            Param::Float(f) => mysql::Value::Double(*f),
            Param::Text(s) => mysql::Value::Bytes(s.clone().into_bytes()),
            Param::Bytes(b) => mysql::Value::Bytes(b.clone()),
        })
        .collect();
    if values.is_empty() {
        mysql::Params::Empty
    } else {
        mysql::Params::Positional(values)
    }
}

/// Convert a `mysql::Value` into the canonical qfs [`Value`]. MySQL's text protocol returns most
/// columns as `Bytes`; valid UTF-8 becomes [`Value::Text`], otherwise [`Value::Bytes`]. Temporal
/// values render to their text form.
fn my_value(v: &mysql::Value) -> Value {
    match v {
        mysql::Value::NULL => Value::Null,
        mysql::Value::Int(n) => Value::Int(*n),
        mysql::Value::UInt(n) => Value::Int(i64::try_from(*n).unwrap_or(i64::MAX)),
        mysql::Value::Float(f) => Value::Float(f64::from(*f)),
        mysql::Value::Double(f) => Value::Float(*f),
        mysql::Value::Bytes(b) => match String::from_utf8(b.clone()) {
            Ok(s) => Value::Text(s),
            Err(e) => Value::Bytes(e.into_bytes()),
        },
        other => Value::Text(format!("{other:?}")),
    }
}

impl SqlBackend for MysqlBackend {
    fn dialect(&self) -> Dialect {
        Dialect::Mysql
    }

    fn introspect(&self) -> Result<Catalog, SqlError> {
        let err = |e: mysql::Error| SqlError::backend("mysql", "introspect", e.to_string());
        let mut conn = self
            .conn
            .lock()
            .map_err(|_| SqlError::backend("mysql", "lock", "poisoned connection mutex"))?;
        let rels: Vec<(String, String)> = conn
            .exec(
                "SELECT table_name, table_type FROM information_schema.tables \
                 WHERE table_schema = ? ORDER BY table_name",
                (self.schema.clone(),),
            )
            .map_err(err)?;
        let mut tables = Vec::new();
        for (name, kind) in rels {
            let cols_raw: Vec<(String, String, String, String)> = conn
                .exec(
                    "SELECT column_name, data_type, is_nullable, column_key \
                     FROM information_schema.columns \
                     WHERE table_schema = ? AND table_name = ? ORDER BY ordinal_position",
                    (self.schema.clone(), name.clone()),
                )
                .map_err(err)?;
            let mut cols = Vec::new();
            for (col_name, data_type, nullable, key) in cols_raw {
                let is_pk = key.eq_ignore_ascii_case("PRI");
                cols.push(ColumnDef::new(
                    col_name,
                    Dialect::Mysql.map_type(&data_type),
                    nullable.eq_ignore_ascii_case("yes"),
                    is_pk,
                    is_pk,
                ));
            }
            let relkind = if kind.eq_ignore_ascii_case("view") {
                RelationKind::View
            } else {
                RelationKind::Table
            };
            tables.push(TableCatalog::new(name, relkind, cols));
        }
        Ok(Catalog::new(tables))
    }

    fn execute_read(&self, sql: &str, params: &[Param]) -> Result<Vec<Row>, SqlError> {
        let mut conn = self
            .conn
            .lock()
            .map_err(|_| SqlError::backend("mysql", "lock", "poisoned connection mutex"))?;
        let rows: Vec<mysql::Row> = conn
            .exec(sql, my_params(params))
            .map_err(|e| SqlError::backend("mysql", "select", e.to_string()))?;
        let mut out = Vec::with_capacity(rows.len());
        for row in &rows {
            let mut values = Vec::with_capacity(row.len());
            for i in 0..row.len() {
                values.push(row.as_ref(i).map_or(Value::Null, my_value));
            }
            out.push(Row::new(values));
        }
        Ok(out)
    }

    fn commit_transaction(&self, ops: &[DmlOp]) -> Result<u64, SqlError> {
        let mut conn = self
            .conn
            .lock()
            .map_err(|_| SqlError::backend("mysql", "lock", "poisoned connection mutex"))?;
        let mut tx = conn
            .start_transaction(mysql::TxOpts::default())
            .map_err(|e| SqlError::backend("mysql", "begin", e.to_string()))?;
        let mut affected = 0u64;
        for op in ops {
            let (sql, params) = render_dml(Dialect::Mysql, op);
            // On ANY error `tx` is dropped without commit → automatic ROLLBACK (all-or-nothing).
            tx.exec_drop(&sql, my_params(&params))
                .map_err(|e| SqlError::backend("mysql", "dml", e.to_string()))?;
            affected += tx.affected_rows();
        }
        tx.commit()
            .map_err(|e| SqlError::backend("mysql", "commit", e.to_string()))?;
        Ok(affected)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn pg_and_my_params_bind_every_param_positionally() {
        let params = [
            Param::Null,
            Param::Bool(true),
            Param::Int(7),
            Param::Float(1.5),
            Param::Text("x".into()),
            Param::Bytes(vec![1, 2]),
        ];
        // Postgres: one type-adapting bind per param, in order.
        assert_eq!(pg_params(&params).len(), params.len());
        // MySQL: a positional bind list of the same arity; an empty param set is `Empty`.
        match my_params(&params) {
            mysql::Params::Positional(v) => assert_eq!(v.len(), params.len()),
            other => panic!("expected positional params, got {other:?}"),
        }
        assert!(matches!(my_params(&[]), mysql::Params::Empty));
    }

    #[test]
    fn my_value_maps_engine_values_to_canonical_values() {
        assert_eq!(my_value(&mysql::Value::NULL), Value::Null);
        assert_eq!(my_value(&mysql::Value::Int(42)), Value::Int(42));
        assert_eq!(my_value(&mysql::Value::UInt(42)), Value::Int(42));
        assert_eq!(my_value(&mysql::Value::Double(2.5)), Value::Float(2.5));
        // The text protocol returns most columns as Bytes: valid UTF-8 becomes Text…
        assert_eq!(
            my_value(&mysql::Value::Bytes(b"hello".to_vec())),
            Value::Text("hello".to_string())
        );
        // …non-UTF-8 stays Bytes (never a lossy/incorrect Text).
        assert_eq!(
            my_value(&mysql::Value::Bytes(vec![0xff, 0xfe])),
            Value::Bytes(vec![0xff, 0xfe])
        );
    }

    #[test]
    fn my_value_maps_rich_type_columns_to_canonical_text() {
        // MySQL's text protocol returns DATETIME (TIMESTAMP), CHAR(36) (UUID), and JSON columns as
        // UTF-8 byte strings, so the value-mapping seam for those three rich types is the
        // Bytes→Text branch. Each round-trips to `Value::Text` intact (the NUMERIC path is covered
        // by `pg_numeric_wire_format_renders_canonical_text`; MySQL DECIMAL likewise arrives as text
        // bytes here).
        assert_eq!(
            my_value(&mysql::Value::Bytes(b"2026-07-18 00:00:00".to_vec())),
            Value::Text("2026-07-18 00:00:00".to_string())
        );
        assert_eq!(
            my_value(&mysql::Value::Bytes(
                b"550e8400-e29b-41d4-a716-446655440000".to_vec()
            )),
            Value::Text("550e8400-e29b-41d4-a716-446655440000".to_string())
        );
        assert_eq!(
            my_value(&mysql::Value::Bytes(b"{\"a\": 1}".to_vec())),
            Value::Text("{\"a\": 1}".to_string())
        );
        assert_eq!(
            my_value(&mysql::Value::Bytes(b"12345.67".to_vec())),
            Value::Text("12345.67".to_string())
        );
    }

    #[test]
    fn pg_numeric_wire_format_renders_canonical_text() {
        assert_eq!(
            pg_numeric_text(&numeric_wire(1, 0x0000, 2, &[1, 2345, 6700])).unwrap(),
            "12345.67"
        );
        assert_eq!(
            pg_numeric_text(&numeric_wire(-1, 0x0000, 4, &[12])).unwrap(),
            "0.0012"
        );
        assert_eq!(
            pg_numeric_text(&numeric_wire(0, 0x4000, 0, &[42])).unwrap(),
            "-42"
        );
        assert_eq!(
            pg_numeric_text(&numeric_wire(0, 0xC000, 0, &[])).unwrap(),
            "NaN"
        );
    }

    fn numeric_wire(weight: i16, sign: u16, dscale: u16, digits: &[u16]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&u16::try_from(digits.len()).unwrap().to_be_bytes());
        out.extend_from_slice(&weight.to_be_bytes());
        out.extend_from_slice(&sign.to_be_bytes());
        out.extend_from_slice(&dscale.to_be_bytes());
        for digit in digits {
            out.extend_from_slice(&digit.to_be_bytes());
        }
        out
    }
}
