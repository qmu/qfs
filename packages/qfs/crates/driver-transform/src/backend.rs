//! The **injected** transform-registry seam (the analogue of `qfs-driver-sys`'s `SysBackend`). The
//! introspective driver half is pure; the impure read/write half — the rusqlite I/O over the
//! System DB's `sys_transforms` table — is provided by the `qfs` binary leaf through this trait, so
//! this crate stays tokio-free and DB-free.
//!
//! No vendor type crosses this boundary — only owned qfs DTOs (`RowBatch` and the structured
//! [`TransformError`]).

use qfs_types::RowBatch;

/// The read/write seam the binary implements over the `sys_transforms` System-DB table. The driver
/// holds only `Arc<dyn TransformBackend>`; the concrete rusqlite implementation lives binary-side.
pub trait TransformBackend: Send + Sync {
    /// Scan all transform definitions into a [`RowBatch`] shaped by
    /// [`transform_node_schema`](crate::transform_node_schema). The `mode` column is DERIVED from
    /// each row's `input` (never a stored flag); the `secret_ref` column is a REFERENCE, never a
    /// resolved value (no network, no vault read — the DESCRIBE/list purity contract).
    ///
    /// # Errors
    /// [`TransformError::Backend`] on an I/O / decode failure.
    fn scan(&self) -> Result<RowBatch, TransformError>;

    /// Apply a single-row `INSERT INTO /transform` — create a definition, an **upsert on `name`**
    /// (re-creating replaces the definition) — appending the audit + ddl_event in the SAME
    /// transaction. Returns 1 on success. The row carries definition text + selectors + a secret
    /// REFERENCE only; the implementor persists no secret value.
    ///
    /// # Errors
    /// [`TransformError::MalformedEffect`] if the row lacks a non-empty `name`/`input`/`output`/
    /// `provider`/`model`, or carries a non-reference `secret_ref`; [`TransformError::Backend`] on I/O.
    fn insert(&self, row: &RowBatch) -> Result<u64, TransformError>;

    /// Apply a `REMOVE /transform/<name>` — delete the definition `name`, appending the audit +
    /// ddl_event transactionally. Idempotent (removing an absent definition affects 0 rows).
    ///
    /// # Errors
    /// [`TransformError::Backend`] on an I/O failure.
    fn remove(&self, name: &str) -> Result<u64, TransformError>;

    /// Ledger a committed §15 transform RUN — the consent/audit leg of a `|> transform <name>`
    /// commit (the model call itself already ran exec-side; this records that it did). Appends
    /// the audit event only: metadata (name + affected count), never rows, never a secret.
    ///
    /// # Errors
    /// [`TransformError::Backend`] on an I/O failure.
    fn record_run(&self, name: &str, affected: u64) -> Result<(), TransformError>;
}

/// A structured, **secret-free** error from the transform backend (AI-consumable). Names a redacted
/// detail — never a credential, never a DB path-secret.
#[derive(Debug, thiserror::Error)]
pub enum TransformError {
    /// The path did not resolve to the `/transform` registry.
    #[error("`{path}` is not a /transform node")]
    UnknownNode {
        /// The offending path (carries no secret).
        path: String,
    },
    /// A write verb is not supported at this node (only SELECT/INSERT/REMOVE).
    #[error("{verb} is not supported on /transform")]
    UnsupportedVerb {
        /// The rejected verb label.
        verb: &'static str,
    },
    /// The effect payload was malformed (missing a required column, or an inline secret).
    #[error("malformed /transform write effect: {reason}")]
    MalformedEffect {
        /// A secret-free reason.
        reason: String,
    },
    /// An underlying System-DB I/O failure (the binary maps its rusqlite error to a secret-free
    /// string — a DB path is infra, never a credential).
    #[error("system db: {0}")]
    Backend(String),
}
