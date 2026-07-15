//! Materialized-view refresh extraction for the explicit `qfs view refresh` entrypoint.
//!
//! This module keeps the `qfs-server` boot/refresh coupling behind `qfs-host`'s `host-daemon`
//! feature, matching the saved-JOB extraction path. The terminal binary supplies only the live read
//! executor closure; it does not depend on `qfs-server` directly.

use std::path::Path;

/// Boot a `.qfs` config, refresh one materialized view through an injected read executor, and
/// return the secret-free refresh receipt.
///
/// # Errors
/// A line-located boot error, missing/non-materialized view error, executor error, or cache
/// serialization error rendered as a secret-free string.
pub fn refresh_materialized_view_from_config<F>(
    config: &Path,
    name: &str,
    now_epoch_ms: i64,
    execute_query: F,
) -> Result<qfs_server::RefreshReport, String>
where
    F: FnOnce(&str) -> Result<qfs_server::RowBatch, String>,
{
    let mut rt = qfs_server::Runtime::new();
    rt.boot(config).map_err(|e| format!("boot: {e}"))?;
    rt.refresh_materialized_view(name, now_epoch_ms, execute_query)
        .map_err(|e| e.to_string())
}
