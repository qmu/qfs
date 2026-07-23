//! The `/transform` composition root: the System-DB-backed [`TransformBackend`] implementation +
//! the async [`TransformReadDriver`] read facet, both hosted in the **`qfs` binary crate**.
//!
//! Like `/sys` (see [`crate::sys`]), `qfs-driver-transform` is a vendor-free `qfs-runtime` consumer
//! and therefore a leaf: only the terminal `qfs` binary depends onto it, and the binary is the ONE
//! place that opens a real DB path (decision F). So the real `rusqlite` reads/writes over the
//! System DB's `sys_transforms` table dead-end here; no `rusqlite` type crosses the
//! [`TransformBackend`] boundary (owned qfs DTOs only). The audit + `ddl_event` hash chains are the
//! SAME primitives `/sys` uses (`crate::sys::{append_audit_tx, append_ddl_event_tx}`), so a
//! transform mutation self-audits exactly like a `/sys` one.
//!
//! ## Safety floor
//! - The `secret_ref` column is a REFERENCE (`env:`/`vault:`), never a value — it is stored and
//!   listed as-is and NEVER resolved here (no vault read, no network): DESCRIBE/list stay pure.
//! - The cardinality `mode` is DERIVED from the stored INPUT on every scan (never a stored flag).
//! - A write is re-validated through [`TransformDef::from_stored`] (empty input/output, inline
//!   secret, malformed schema are refused) — defence beyond the parse-time desugar.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use qfs_core::ddl::transform::TransformDef;
use qfs_core::{CfsError, RequestContext, RowBatch};
use qfs_driver_transform::{
    node_for_path, transform_node_schema, ModelProvider, ModelRequest, TransformBackend,
    TransformError, TransformNode, TRANSFORM_MOUNT,
};
use qfs_exec::{ReadDriver, TransformCall, TransformExecutor};
use qfs_pushdown::ScanNode;
use qfs_store::audit::AuditEvent;
use qfs_types::{ResolvedTransform, Schema, TransformDefs, TransformMode};
use qfs_types::{Row, Value};
use rusqlite::Connection;

use crate::sys::{
    append_audit_tx, append_ddl_event_tx, ddl_event, now_rfc3339, nullable_text, optional_text,
};

/// The acting principal recorded on a `/transform` mutation's audit event — a label, never a
/// credential (mirrors `crate::sys`'s `ACTOR_CLI`).
const ACTOR_CLI: &str = "cli";

/// The System-DB-backed [`TransformBackend`]: the real rusqlite reads/writes over `sys_transforms`.
/// The connection is held behind a `Mutex` (rusqlite is `!Sync`; the mutex provides `Send + Sync`).
pub struct TransformDbBackend {
    system: Mutex<Connection>,
}

impl TransformDbBackend {
    /// Build a backend over an already-migrated System-DB connection (the test + composition seam).
    #[must_use]
    pub fn new(system: Connection) -> Self {
        Self {
            system: Mutex::new(system),
        }
    }

    /// Open the default System DB and build the backend. Returns `None` when no config home
    /// resolves (the `/transform` write/read surface is simply not wired, never a CLI failure —
    /// the same best-effort posture as `SystemDbBackend::open_default`).
    #[must_use]
    pub fn open_default() -> Option<Self> {
        match crate::store::open_system_db() {
            Ok(Some(sys)) => Some(Self::new(sys.into_db().into_connection())),
            _ => None,
        }
    }

    /// Load every stored definition into a plan-time [`TransformDefs`] map (name → resolved
    /// INPUT/OUTPUT + derived mode + the NON-SECRET provider/model/effort selectors, which the
    /// PREVIEW consent node renders for spend legibility). Best-effort: a row that fails to
    /// decode is skipped (never a panic, never a plan failure — a bad row simply can't be resolved).
    #[must_use]
    pub fn load_defs(&self) -> TransformDefs {
        let mut defs = TransformDefs::new();
        for def in self.load_full_defs() {
            if let Ok(resolved) = ResolvedTransform::new(def.input.clone(), def.output.clone()) {
                defs.insert(
                    def.name.clone(),
                    resolved.with_model_meta(def.provider, def.model, def.effort),
                );
            }
        }
        defs
    }

    /// Load every stored definition as the full typed [`TransformDef`] (INCLUDING the secret
    /// REFERENCE) — the COMMIT-boundary executor's resolution source. The secret_ref is a
    /// reference (`env:`/`vault:`), never a value; it is resolved lazily at the model call, never
    /// here. Best-effort: an undecodable row is skipped.
    #[must_use]
    pub fn load_full_defs(&self) -> Vec<TransformDef> {
        let mut out = Vec::new();
        let Ok(conn) = self.system.lock() else {
            return out;
        };
        let Ok(mut stmt) = conn.prepare(
            "SELECT name, input, output, provider, model, effort, secret_ref \
             FROM sys_transforms ORDER BY name",
        ) else {
            return out;
        };
        let Ok(rows) = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, Option<String>>(5)?,
                r.get::<_, Option<String>>(6)?,
            ))
        }) else {
            return out;
        };
        for (name, input, output, provider, model, effort, secret_ref) in rows.flatten() {
            if let Ok(def) = TransformDef::from_stored(
                &name, &input, &output, &provider, &model, effort, secret_ref,
            ) {
                out.push(def);
            }
        }
        out
    }

    /// The derived cardinality-mode token for a stored row's INPUT, best-effort: a malformed stored
    /// row (should never happen — writes are validated) reports `"unknown"` rather than failing the
    /// whole scan. The mode is a pure function of INPUT, so nothing else is decoded.
    fn derived_mode(input: &str) -> String {
        qfs_core::ddl::transform::derived_mode_of_stored_input(input)
            .map_or_else(|| "unknown".to_string(), |m| m.token().to_string())
    }
}

impl TransformBackend for TransformDbBackend {
    fn scan(&self) -> Result<RowBatch, TransformError> {
        let schema = transform_node_schema(TransformNode::Registry);
        let conn = self.system.lock().map_err(|_| poisoned())?;
        let mut stmt = conn
            .prepare(
                "SELECT name, input, output, provider, model, effort, secret_ref, created_at \
                 FROM sys_transforms ORDER BY name",
            )
            .map_err(backend)?;
        let mapped = stmt
            .query_map([], |r| {
                // Read the raw text columns, then splice in the DERIVED mode (never stored).
                let name: String = r.get(0)?;
                let input: String = r.get(1)?;
                let output: String = r.get(2)?;
                let provider: String = r.get(3)?;
                let model: String = r.get(4)?;
                let mode = Self::derived_mode(&input);
                Ok(Row::new(vec![
                    Value::Text(name),
                    Value::Text(input),
                    Value::Text(output),
                    Value::Text(provider),
                    Value::Text(model),
                    nullable_text(r, 5)?,
                    Value::Text(mode),
                    nullable_text(r, 6)?,
                    nullable_text(r, 7)?,
                ]))
            })
            .map_err(backend)?;
        let mut rows = Vec::new();
        for r in mapped {
            rows.push(r.map_err(backend)?);
        }
        Ok(RowBatch::new(schema, rows))
    }

    fn insert(&self, row: &RowBatch) -> Result<u64, TransformError> {
        // Create / re-create a definition (upsert on `name`). Every field is validated through the
        // typed constructor first (empty input/output, inline secret, malformed schema are refused).
        let name = required(row, "name")?;
        let input = required(row, "input")?;
        let output = required(row, "output")?;
        let provider = required(row, "provider")?;
        let model = required(row, "model")?;
        let effort = optional_text(row, "effort");
        let secret_ref = optional_text(row, "secret_ref");
        // Re-validate: a bad definition never reaches the table (defence beyond the parse desugar).
        TransformDef::from_stored(
            &name,
            &input,
            &output,
            &provider,
            &model,
            effort.clone(),
            secret_ref.clone(),
        )
        .map_err(|e| TransformError::MalformedEffect {
            reason: e.to_string(),
        })?;

        let conn = self.system.lock().map_err(|_| poisoned())?;
        let tx = conn.unchecked_transaction().map_err(backend)?;
        tx.execute(
            "INSERT INTO sys_transforms (name, input, output, provider, model, effort, secret_ref) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7) \
             ON CONFLICT(name) DO UPDATE SET \
                 input = ?2, output = ?3, provider = ?4, model = ?5, effort = ?6, secret_ref = ?7",
            rusqlite::params![name, input, output, provider, model, effort, secret_ref],
        )
        .map_err(backend)?;
        self.audit_and_event(&tx, "INSERT", &name)?;
        tx.commit().map_err(backend)?;
        Ok(1)
    }

    fn remove(&self, name: &str) -> Result<u64, TransformError> {
        let conn = self.system.lock().map_err(|_| poisoned())?;
        let tx = conn.unchecked_transaction().map_err(backend)?;
        let affected = tx
            .execute("DELETE FROM sys_transforms WHERE name = ?1", [name])
            .map_err(backend)? as u64;
        self.audit_and_event(&tx, "REMOVE", name)?;
        tx.commit().map_err(backend)?;
        Ok(affected)
    }

    fn record_run(&self, name: &str, affected: u64) -> Result<(), TransformError> {
        // The model call already ran exec-side (blueprint §15): this is the consent/audit leg.
        // Append the metadata-only audit event (verb `RUN`, the definition path, the affected
        // count) transactionally — never rows, never a secret. No table row changes.
        let conn = self.system.lock().map_err(|_| poisoned())?;
        let tx = conn.unchecked_transaction().map_err(backend)?;
        let ts = now_rfc3339();
        let path = format!("{TRANSFORM_MOUNT}/{name}");
        append_audit_tx(
            &tx,
            AuditEvent {
                actor: ACTOR_CLI.to_string(),
                connection: "default".to_string(),
                verb: "RUN".to_string(),
                path,
                committed: true,
                ts: ts.clone(),
            },
        )
        .map_err(backend)?;
        let payload = serde_json::json!({ "name": name, "affected": affected }).to_string();
        append_ddl_event_tx(&tx, ddl_event(TRANSFORM_MOUNT, "RUN", payload, ts))
            .map_err(backend)?;
        tx.commit().map_err(backend)?;
        Ok(())
    }
}

impl TransformDbBackend {
    /// Append the t76 audit row + the `ddl_event` for a `/transform` mutation INSIDE the write's
    /// transaction (administration observes itself; a torn write can never leave the change
    /// un-audited). Metadata only — the definition name + verb, never a secret.
    fn audit_and_event(
        &self,
        tx: &rusqlite::Transaction<'_>,
        verb: &str,
        name: &str,
    ) -> Result<(), TransformError> {
        let ts = now_rfc3339();
        append_audit_tx(
            tx,
            AuditEvent {
                actor: ACTOR_CLI.to_string(),
                connection: "default".to_string(),
                verb: verb.to_string(),
                path: TRANSFORM_MOUNT.to_string(),
                committed: true,
                ts: ts.clone(),
            },
        )
        .map_err(backend)?;
        let payload = serde_json::json!({ "name": name }).to_string();
        append_ddl_event_tx(tx, ddl_event(TRANSFORM_MOUNT, verb, payload, ts)).map_err(backend)?;
        Ok(())
    }
}

/// The single-row write payload's value for `col` as a non-empty string, or a structured
/// malformed-effect error.
fn required(row: &RowBatch, col: &str) -> Result<String, TransformError> {
    optional_text(row, col).ok_or_else(|| TransformError::MalformedEffect {
        reason: format!("INSERT INTO /transform requires a non-empty `{col}`"),
    })
}

/// Map a rusqlite error to the secret-free [`TransformError::Backend`] (a DB path is infra).
fn backend(e: rusqlite::Error) -> TransformError {
    TransformError::Backend(e.to_string())
}

/// The lock-poisoned error (a poisoned mutex is an internal fault, secret-free).
fn poisoned() -> TransformError {
    TransformError::Backend("system db lock poisoned".to_string())
}

/// The async read facet (the analogue of [`crate::sys::SysReadDriver`]): adapts the synchronous
/// [`TransformBackend::scan`] to qfs-exec's [`ReadDriver`] seam. Lives in the binary because
/// `ReadDriver` is a qfs-exec type the driver crate must stay off (dep direction).
pub struct TransformReadDriver {
    backend: Arc<dyn TransformBackend>,
}

impl TransformReadDriver {
    /// Build the read adapter over an injected backend.
    #[must_use]
    pub fn new(backend: Arc<dyn TransformBackend>) -> Self {
        Self { backend }
    }
}

#[async_trait::async_trait]
impl ReadDriver for TransformReadDriver {
    async fn scan(&self, scan: &ScanNode, _ctx: &RequestContext) -> Result<RowBatch, CfsError> {
        node_for_path(&scan.path).ok_or_else(|| CfsError::InvalidPath {
            path: scan.path.clone(),
            reason: "not a /transform path",
        })?;
        self.backend.scan().map_err(|_| CfsError::InvalidPath {
            path: scan.path.clone(),
            reason: "transform_read_failed",
        })
    }
}

/// The COMMIT-boundary transform executor (blueprint §15, decision W): the binary-side
/// [`TransformExecutor`] that drives an injected [`ModelProvider`] over a `|> transform <name>`
/// stage's upstream rows. It holds the full definitions (INCLUDING the secret REFERENCE) so it can
/// resolve the credential lazily at the model call — never at PREVIEW, never logged. The pure
/// engine calls this seam; the model call itself dead-ends here in the binary leaf.
///
/// ## Mode chunking (from the derived mode)
/// - **row-wise / extraction:** one model call per upstream row (each call sees a single-row batch
///   projected to the declared INPUT columns);
/// - **relation-wise:** one model call for the whole upstream relation.
///
/// The OUTPUT-schema membership check + column reordering happen in the engine, over what the
/// provider returns — the model's output is untrusted.
/// A `vault:<path>` secret resolver the binary injects (the binary owns the vault). Given the
/// path portion of a `vault:` reference, returns the resolved secret VALUE or a secret-free error
/// string. Held behind an `Arc` so the executor is cheap to clone.
pub type VaultResolver = Arc<dyn Fn(&str) -> Result<String, String> + Send + Sync>;

pub struct BinaryTransformExecutor {
    provider: Arc<dyn ModelProvider>,
    defs: BTreeMap<String, TransformDef>,
    /// Optional `vault:<path>` resolver, injected by the composition root. `None` leaves a
    /// `vault:` reference unresolvable (a structured COMMIT error); an `env:` reference always
    /// resolves inline.
    vault_resolver: Option<VaultResolver>,
}

impl BinaryTransformExecutor {
    /// Build the executor over the injected provider and the full stored definitions.
    #[must_use]
    pub fn new(provider: Arc<dyn ModelProvider>, defs: Vec<TransformDef>) -> Self {
        let defs = defs.into_iter().map(|d| (d.name.clone(), d)).collect();
        Self {
            provider,
            defs,
            vault_resolver: None,
        }
    }

    /// Attach the `vault:<path>` secret resolver (the binary's vault seam). An `env:` reference
    /// resolves without it.
    #[must_use]
    pub fn with_vault_resolver(mut self, resolver: VaultResolver) -> Self {
        self.vault_resolver = Some(resolver);
        self
    }

    /// Resolve a definition's secret REFERENCE to its value at COMMIT — `env:<VAR>` inline,
    /// `vault:<path>` via the injected resolver. `None` reference ⇒ `None` secret (a provider that
    /// needs no per-definition credential). NEVER logged; the value is returned only to the
    /// provider call.
    fn resolve_secret(&self, secret_ref: Option<&str>) -> Result<Option<String>, String> {
        let Some(reference) = secret_ref else {
            return Ok(None);
        };
        if let Some(var) = reference.strip_prefix("env:") {
            return std::env::var(var)
                .map(Some)
                .map_err(|_| format!("secret reference env:{var} is not set"));
        }
        if let Some(path) = reference.strip_prefix("vault:") {
            let resolver = self.vault_resolver.as_ref().ok_or_else(|| {
                "a vault: secret reference cannot be resolved (no vault is configured)".to_string()
            })?;
            return resolver(path).map(Some);
        }
        // `TransformDef` validation guarantees the scheme, so this is unreachable; stay total.
        Err("unrecognized secret reference scheme".to_string())
    }
}

/// Project a batch to the declared INPUT columns in declared order (surplus incoming columns are
/// dropped; a missing declared column — already rejected at plan time — degrades to `Null`).
fn project_to_input(input: &Schema, batch: &RowBatch) -> RowBatch {
    let idx: Vec<Option<usize>> = input
        .columns
        .iter()
        .map(|c| batch.schema.columns.iter().position(|bc| bc.name == c.name))
        .collect();
    let rows = batch
        .rows
        .iter()
        .map(|row| {
            let values = idx
                .iter()
                .map(|i| {
                    i.and_then(|i| row.values.get(i).cloned())
                        .unwrap_or(Value::Null)
                })
                .collect();
            Row::new(values)
        })
        .collect();
    RowBatch::new(input.clone(), rows)
}

impl TransformExecutor for BinaryTransformExecutor {
    fn execute(&self, call: &TransformCall<'_>, input: RowBatch) -> Result<RowBatch, String> {
        let def = self
            .defs
            .get(call.name)
            .ok_or_else(|| format!("transform '{}' is not installed", call.name))?;
        let secret = self.resolve_secret(def.secret_ref.as_deref())?;
        let projected = project_to_input(&def.input, &input);

        // Chunk the upstream relation by mode, call the provider per chunk, concatenate the
        // OUTPUT rows. Relation-wise hands the whole relation at once; row-wise/extraction call
        // once per row. The engine enforces OUTPUT membership over the concatenated result.
        let call_batches: Vec<RowBatch> = match call.mode {
            TransformMode::RelationWise => vec![projected],
            TransformMode::RowWise | TransformMode::Extraction => projected
                .rows
                .into_iter()
                .map(|row| RowBatch::new(def.input.clone(), vec![row]))
                .collect(),
        };

        let mut out_rows = Vec::new();
        let mut out_schema: Option<Schema> = None;
        for chunk in &call_batches {
            let req = ModelRequest {
                name: &def.name,
                provider: &def.provider,
                model: &def.model,
                effort: def.effort.as_deref(),
                mode: call.mode,
                output: call.output,
                input: chunk,
            };
            // The one-seam lock (blueprint §15): every model invocation flows through the single
            // `call_model` funnel — the ONLY path that can mint the `CallProof` witness `call`
            // requires. This transform applier is the sole caller.
            let produced =
                qfs_driver_transform::call_model(self.provider.as_ref(), &req, secret.as_deref())
                    .map_err(|e| e.to_string())?;
            if out_schema.is_none() {
                out_schema = Some(produced.schema.clone());
            }
            out_rows.extend(produced.rows);
        }
        let schema = out_schema.unwrap_or_else(|| call.output.clone());
        Ok(RowBatch::new(schema, out_rows))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;
    use qfs_store::{MemorySource, SystemDb};
    use qfs_types::{Column, ColumnType, Schema};

    fn backend() -> TransformDbBackend {
        let sys = SystemDb::open(&MemorySource).unwrap();
        TransformDbBackend::new(sys.into_db().into_connection())
    }

    fn def_row() -> RowBatch {
        let cols = [
            "name",
            "input",
            "output",
            "provider",
            "model",
            "effort",
            "secret_ref",
        ];
        let schema = Schema::new(
            cols.iter()
                .map(|c| Column::new(*c, ColumnType::Text, true))
                .collect(),
        );
        RowBatch::new(
            schema,
            vec![Row::new(vec![
                Value::Text("classify".into()),
                Value::Text("[{\"name\":\"body\",\"type\":\"text\",\"nullable\":true}]".into()),
                Value::Text("[{\"name\":\"label\",\"type\":\"text\",\"nullable\":true}]".into()),
                Value::Text("claude".into()),
                Value::Text("claude-sonnet-5".into()),
                Value::Text("medium".into()),
                Value::Text("vault:models/key".into()),
            ])],
        )
    }

    #[test]
    fn insert_then_scan_lists_the_definition_with_derived_mode_and_no_secret_value() {
        let b = backend();
        assert_eq!(b.insert(&def_row()).unwrap(), 1);
        let batch = b.scan().unwrap();
        assert_eq!(batch.rows.len(), 1);
        let idx = |name: &str| {
            batch
                .schema
                .columns
                .iter()
                .position(|c| c.name == name)
                .unwrap()
        };
        let row = &batch.rows[0];
        assert_eq!(row.values[idx("name")], Value::Text("classify".into()));
        // The DERIVED mode column (single text column ⇒ row-wise).
        assert_eq!(row.values[idx("mode")], Value::Text("row-wise".into()));
        // The secret REFERENCE is listed as-is (a reference names WHERE, never the secret VALUE).
        assert_eq!(
            row.values[idx("secret_ref")],
            Value::Text("vault:models/key".into())
        );
        // No column named `secret`/`token` exists — structurally cred-free.
        assert!(batch.schema.column("secret").is_none());
        assert!(batch.schema.column("token").is_none());
    }

    #[test]
    fn insert_is_upsert_on_name() {
        let b = backend();
        b.insert(&def_row()).unwrap();
        // Re-inserting the same name replaces it (upsert), not a UNIQUE-violation error.
        b.insert(&def_row()).unwrap();
        assert_eq!(b.scan().unwrap().rows.len(), 1);
    }

    #[test]
    fn remove_deletes_the_definition_idempotently() {
        let b = backend();
        b.insert(&def_row()).unwrap();
        assert_eq!(b.remove("classify").unwrap(), 1);
        assert_eq!(b.scan().unwrap().rows.len(), 0);
        // Removing an absent definition affects 0 rows (idempotent), never an error.
        assert_eq!(b.remove("classify").unwrap(), 0);
    }

    #[test]
    fn a_malformed_definition_is_refused_at_write() {
        let b = backend();
        let mut row = def_row();
        // Empty INPUT JSON ⇒ EmptyInput ⇒ MalformedEffect.
        row.rows[0].values[1] = Value::Text("[]".into());
        assert!(b.insert(&row).is_err());
        assert_eq!(b.scan().unwrap().rows.len(), 0, "nothing was written");
    }

    // ---- §15 COMMIT executor (BinaryTransformExecutor) ----

    use qfs_driver_transform::{CallProof, ModelError, ModelProvider, ModelRequest};
    use std::sync::Mutex;

    /// A deterministic mock provider: records each call's (mode, input-row-count, secret) and
    /// returns one OUTPUT row per input row with `label` = "L". No model, no network.
    #[derive(Default)]
    struct MockProvider {
        calls: Mutex<Vec<(TransformMode, usize, Option<String>)>>,
    }

    impl ModelProvider for MockProvider {
        fn call(
            &self,
            req: &ModelRequest<'_>,
            secret: Option<&str>,
            _proof: &CallProof,
        ) -> Result<RowBatch, ModelError> {
            self.calls.lock().unwrap().push((
                req.mode,
                req.input.rows.len(),
                secret.map(str::to_string),
            ));
            let rows = req
                .input
                .rows
                .iter()
                .map(|_| Row::new(vec![Value::Text("L".into())]))
                .collect();
            Ok(RowBatch::new(
                Schema::new(vec![Column::new("label", ColumnType::Text, true)]),
                rows,
            ))
        }
    }

    fn text_def(name: &str, secret_ref: Option<&str>) -> TransformDef {
        TransformDef::from_stored(
            name,
            "[{\"name\":\"body\",\"type\":\"text\",\"nullable\":true}]",
            "[{\"name\":\"label\",\"type\":\"text\",\"nullable\":true}]",
            "claude",
            "claude-sonnet-5",
            Some("medium".into()),
            secret_ref.map(str::to_string),
        )
        .unwrap()
    }

    fn input_batch(rows: usize) -> RowBatch {
        let schema = Schema::new(vec![
            Column::new("body", ColumnType::Text, true),
            Column::new("surplus", ColumnType::Int, true),
        ]);
        RowBatch::new(
            schema,
            (0..rows)
                .map(|i| Row::new(vec![Value::Text(format!("m{i}")), Value::Int(i as i64)]))
                .collect(),
        )
    }

    fn call<'a>(name: &'a str, mode: TransformMode, output: &'a Schema) -> TransformCall<'a> {
        TransformCall { name, mode, output }
    }

    #[test]
    fn row_wise_calls_the_provider_once_per_row_and_projects_to_declared_input() {
        let provider = Arc::new(MockProvider::default());
        let exec = BinaryTransformExecutor::new(provider.clone(), vec![text_def("classify", None)]);
        let output = Schema::new(vec![Column::new("label", ColumnType::Text, true)]);
        let out = exec
            .execute(
                &call("classify", TransformMode::RowWise, &output),
                input_batch(3),
            )
            .unwrap();
        assert_eq!(out.rows.len(), 3, "one OUTPUT row per input row");
        let calls = provider.calls.lock().unwrap();
        assert_eq!(calls.len(), 3, "one model call per row");
        // Each call sees a single row projected to the declared INPUT (the surplus column is gone).
        assert!(calls
            .iter()
            .all(|(m, n, _)| *m == TransformMode::RowWise && *n == 1));
    }

    #[test]
    fn relation_wise_calls_the_provider_once_for_the_whole_relation() {
        // A single `array<struct<…>>` INPUT column ⇒ relation-wise: one call, the whole relation.
        let def = TransformDef::from_stored(
            "rollup",
            "[{\"name\":\"rows\",\"type\":\"array<struct<sku:text,qty:int>>\",\"nullable\":true}]",
            "[{\"name\":\"n\",\"type\":\"int\",\"nullable\":true}]",
            "claude",
            "claude-sonnet-5",
            None,
            None,
        )
        .unwrap();
        let provider = Arc::new(MockProvider::default());
        let exec = BinaryTransformExecutor::new(provider.clone(), vec![def]);
        let output = Schema::new(vec![Column::new("n", ColumnType::Int, true)]);
        let input = RowBatch::new(
            Schema::new(vec![Column::new("rows", ColumnType::Text, true)]),
            vec![
                Row::new(vec![Value::Text("a".into())]),
                Row::new(vec![Value::Text("b".into())]),
            ],
        );
        let _ = exec.execute(&call("rollup", TransformMode::RelationWise, &output), input);
        let calls = provider.calls.lock().unwrap();
        assert_eq!(calls.len(), 1, "one call for the whole relation");
        assert_eq!(calls[0].1, 2, "the call sees all rows");
    }

    #[test]
    fn env_secret_reference_resolves_and_reaches_the_provider() {
        // Hermetic: an `env:` reference resolves via the process env (no vault, no network).
        std::env::set_var("QFS_TEST_TRANSFORM_KEY", "sekret");
        let provider = Arc::new(MockProvider::default());
        let exec = BinaryTransformExecutor::new(
            provider.clone(),
            vec![text_def("classify", Some("env:QFS_TEST_TRANSFORM_KEY"))],
        );
        let output = Schema::new(vec![Column::new("label", ColumnType::Text, true)]);
        exec.execute(
            &call("classify", TransformMode::RowWise, &output),
            input_batch(1),
        )
        .unwrap();
        std::env::remove_var("QFS_TEST_TRANSFORM_KEY");
        let calls = provider.calls.lock().unwrap();
        assert_eq!(
            calls[0].2.as_deref(),
            Some("sekret"),
            "the resolved secret reached the provider"
        );
    }

    #[test]
    fn a_missing_env_secret_fails_closed_before_any_provider_call() {
        let provider = Arc::new(MockProvider::default());
        let exec = BinaryTransformExecutor::new(
            provider.clone(),
            vec![text_def("classify", Some("env:QFS_TEST_ABSENT_KEY"))],
        );
        let output = Schema::new(vec![Column::new("label", ColumnType::Text, true)]);
        let err = exec
            .execute(
                &call("classify", TransformMode::RowWise, &output),
                input_batch(1),
            )
            .unwrap_err();
        assert!(err.contains("env:QFS_TEST_ABSENT_KEY"), "{err}");
        assert!(
            provider.calls.lock().unwrap().is_empty(),
            "no model call on a failed secret"
        );
    }

    #[test]
    fn a_vault_reference_without_a_resolver_fails_closed() {
        let provider = Arc::new(MockProvider::default());
        let exec = BinaryTransformExecutor::new(
            provider.clone(),
            vec![text_def("classify", Some("vault:models/key"))],
        );
        let output = Schema::new(vec![Column::new("label", ColumnType::Text, true)]);
        let err = exec
            .execute(
                &call("classify", TransformMode::RowWise, &output),
                input_batch(1),
            )
            .unwrap_err();
        assert!(err.contains("vault"), "{err}");
        assert!(provider.calls.lock().unwrap().is_empty());
    }

    #[test]
    fn the_unconfigured_provider_fails_closed_with_an_actionable_error() {
        // The binary's default until T4 wires a live provider: a transform COMMIT refuses.
        let exec = BinaryTransformExecutor::new(
            Arc::new(qfs_driver_transform::UnconfiguredProvider),
            vec![text_def("classify", None)],
        );
        let output = Schema::new(vec![Column::new("label", ColumnType::Text, true)]);
        let err = exec
            .execute(
                &call("classify", TransformMode::RowWise, &output),
                input_batch(1),
            )
            .unwrap_err();
        assert!(err.contains("provider"), "{err}");
    }
}
