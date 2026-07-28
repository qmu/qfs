//! The fixed typed [`Schema`]s for the two non-D1 Cloudflare archetypes (blueprint §4/§6). D1's
//! schema is the catalog-derived `qfs_types::Schema` from the reused t17 catalog; KV and Queues
//! have a fixed shape:
//!
//! - **KV** is the degenerate two-column `(key, value)` table view (so `SELECT`/`UPSERT` work
//!   over a key-value namespace as a relation).
//! - **Queues** is the append/log message row `(id, body, attempts)` DESCRIBE reports.
//! - **Artifacts** is the account-scoped repository table. It exposes remote metadata, never the
//!   repo token returned by Cloudflare create.

use qfs_types::{Column, ColumnType, Schema};

/// The KV-as-table view schema: `(key TEXT, value TEXT)`. The degenerate two-column relation a
/// `SELECT`/`UPSERT` over `/cf/kv/<ns>` reads/writes.
#[must_use]
pub fn kv_table_schema() -> Schema {
    Schema::new(vec![
        Column::new("key", ColumnType::Text, false),
        Column::new("value", ColumnType::Text, true),
    ])
}

/// The Queues **append** schema: `(id TEXT, body TEXT, attempts INT)`. The message shape the
/// append log describes. Consumer PULL retired to the declared `cloudflare.qfs` read-over-POST view
/// (blueprint §13.3), so this is a DESCRIBE contract only — the compiled `/cf` queue serves
/// `INSERT` (append) and no longer reads.
#[must_use]
pub fn queue_append_schema() -> Schema {
    Schema::new(vec![
        Column::new("id", ColumnType::Text, false),
        Column::new("body", ColumnType::Text, true),
        Column::new("attempts", ColumnType::Int, false),
    ])
}

/// The Artifacts repo table schema. `token` is intentionally absent: create seals it into the vault.
#[must_use]
pub fn artifacts_repos_schema() -> Schema {
    Schema::new(vec![
        Column::new("namespace", ColumnType::Text, false),
        Column::new("name", ColumnType::Text, false),
        Column::new("id", ColumnType::Text, false),
        Column::new("remote_url", ColumnType::Text, false),
        Column::new("created_at", ColumnType::Text, true),
        Column::new("updated_at", ColumnType::Text, true),
        Column::new("last_push_at", ColumnType::Text, true),
        Column::new("description", ColumnType::Text, true),
        Column::new("default_branch", ColumnType::Text, true),
        Column::new("source", ColumnType::Text, true),
        Column::new("read_only", ColumnType::Bool, false),
    ])
}
