//! `qfs view refresh` — the explicit materialized-view refresh entrypoint.
//!
//! A materialized view is stored server configuration plus an internal row snapshot. Refresh is a
//! read operation followed by a server-state metadata update: execute the saved query through the
//! normal read registry, cache the returned rows, then stamp `last_run`. There is no internal
//! scheduler here; operators or external schedulers invoke this command when they want freshness.

use std::time::{SystemTime, UNIX_EPOCH};

use qfs_cmd::{ViewAction, ViewRequest};
use qfs_core::{RowBatch, StatementSpec};

/// Route a parsed `qfs view <verb>` request. Returns a process exit code; never panics.
#[must_use]
pub fn run_view_request(req: &ViewRequest) -> i32 {
    match req.action {
        ViewAction::Refresh => refresh(req),
    }
}

fn refresh(req: &ViewRequest) -> i32 {
    let (engine, reads, _safety) = crate::shell::run_engine_and_reads();
    let now = current_epoch_ms();
    let report =
        qfs_host::refresh_materialized_view_from_config(&req.config, &req.name, now, |query| {
            let spec = StatementSpec::from_canonical(query)
                .map_err(|e| format!("saved query not rehydratable: {e}"))?;
            qfs_exec::block_on_read(spec.statement(), &engine.mounts, &reads)
                .map(|rows| RowBatch::new(rows.schema, rows.rows))
                .map_err(|e| e.to_string())
        });

    match report {
        Ok(report) => {
            if !req.quiet {
                if req.json {
                    println!(
                        "{}",
                        serde_json::json!({
                            "refreshed": {
                                "name": report.name,
                                "rows": report.rows,
                                "last_run": report.last_run,
                            }
                        })
                    );
                } else {
                    println!(
                        "REFRESHED view '{}' ({} row(s), last_run {})",
                        report.name, report.rows, report.last_run
                    );
                }
            }
            0
        }
        Err(e) => render_error(req.json, "view_refresh", format!("qfs view refresh: {e}")),
    }
}

fn render_error(json: bool, code: &str, message: String) -> i32 {
    if json {
        eprintln!(
            "{}",
            serde_json::json!({
                "error": {
                    "code": code,
                    "message": message,
                }
            })
        );
    } else {
        eprintln!("{message}");
    }
    1
}

fn current_epoch_ms() -> i64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(d) => i64::try_from(d.as_millis()).unwrap_or(i64::MAX),
        Err(_) => 0,
    }
}
