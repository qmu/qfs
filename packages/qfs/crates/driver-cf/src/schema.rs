//! The fixed typed [`Schema`]s for the two non-D1 Cloudflare archetypes (blueprint §4/§6). D1's
//! schema is the catalog-derived `qfs_types::Schema` from the reused t17 catalog; KV and Queues
//! have a fixed shape:
//!
//! - **KV** is the degenerate two-column `(key, value)` table view (so `SELECT`/`UPSERT` work
//!   over a key-value namespace as a relation).
//! - **Queues** is the append/log tail row `(id, body, attempts)` a bounded `SELECT` yields.
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

/// The Queues tail schema: `(id TEXT, body TEXT, attempts INT)`. The row shape a bounded-tail
/// `SELECT … LIMIT n` over `/cf/queue/<name>` yields (consumer pull / recent messages).
#[must_use]
pub fn queue_tail_schema() -> Schema {
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
