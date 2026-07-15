//! Restore qfs JSONL state dumps.
//!
//! Restore is preview-only by default. A committed restore replays supported current-state records
//! into the local System/Project DBs and records new local audit/DDL events through the `/sys`
//! backend. Dumped historical `ddl_event` rows are treated as external provenance, not imported
//! into the local hash chain.

use qfs_cmd::RestoreAction;
use qfs_core::RowBatch;
use qfs_driver_sys::{SysBackend, SysNode};
use qfs_types::{Column, ColumnType, Row, Schema, Value};
use serde_json::Value as JsonValue;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct RestoreReport {
    parsed: usize,
    applied: usize,
    skipped_existing: usize,
    skipped_events: usize,
    /// Secretish `sys_settings` records skipped on replay (blueprint §16, amended): the dump
    /// carries `<redacted>` for a secretish key, so writing it back would CLOBBER the live
    /// secret value with the literal marker. Restore never writes a secretish setting.
    skipped_secretish: usize,
}

/// Run the injected `qfs restore` command. Returns the process exit code.
#[must_use]
pub fn run_restore(action: &RestoreAction) -> i32 {
    let input = if action.input == "-" {
        let mut buf = String::new();
        if let Err(e) = std::io::Read::read_to_string(&mut std::io::stdin(), &mut buf) {
            eprintln!("qfs: error: reading stdin: {e}");
            return 1;
        }
        buf
    } else {
        match std::fs::read_to_string(&action.input) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("qfs: error: reading {}: {e}", action.input);
                return 1;
            }
        }
    };
    match restore_jsonl(&input, action.commit) {
        Ok(report) => {
            if action.commit {
                println!(
                    "qfs: restore committed: applied={}, skipped_existing={}, skipped_events={}, \
                     skipped_secretish={}",
                    report.applied,
                    report.skipped_existing,
                    report.skipped_events,
                    report.skipped_secretish
                );
            } else {
                println!(
                    "qfs: restore preview: parsed={} restorable={} skipped_events={} (no changes; rerun with --commit)",
                    report.parsed,
                    report.parsed.saturating_sub(report.skipped_events + 1),
                    report.skipped_events
                );
            }
            0
        }
        Err(e) => {
            eprintln!("qfs: error: {e}");
            1
        }
    }
}

fn restore_jsonl(input: &str, commit: bool) -> Result<RestoreReport, String> {
    let records = parse_records(input)?;
    let mut report = RestoreReport {
        parsed: records.len(),
        ..RestoreReport::default()
    };
    validate_header(records.first())?;
    for record in &records {
        if record["record"] == "ddl_event" {
            report.skipped_events += 1;
        }
    }
    if !commit {
        return Ok(report);
    }

    let system = crate::store::open_system_db()
        .map_err(|e| format!("opening the system database: {e}"))?
        .ok_or("cannot determine the system database path (set HOME or XDG_CONFIG_HOME)")?;
    let project = crate::store::open_project_db()
        .map_err(|e| format!("opening the project database: {e}"))?
        .map(|p| p.into_db().into_connection());
    let backend = crate::sys::SystemDbBackend::new(system.into_db().into_connection(), None);
    let project_conn = project.as_ref();

    for record in records {
        match record_type(&record)? {
            "header" | "ddl_event" => {}
            "sys_setting" => {
                let key = required_string(&record, "key")?;
                let value = required_string(&record, "value")?;
                // Secretish settings are excluded, not restored (blueprint §16, amended): the
                // dump redacts them to the literal `<redacted>`, so replaying the record would
                // overwrite the LIVE secret value with the marker. Skip (never write), counted —
                // both by the shared key predicate and by the redaction marker itself (belt and
                // suspenders for a hand-edited dump).
                if qfs_core::secretish_setting_key(&key) || value == "<redacted>" {
                    report.skipped_secretish += 1;
                    continue;
                }
                backend
                    .set_setting(&row_batch(
                        &[("key", ColumnType::Text), ("value", ColumnType::Text)],
                        vec![Value::Text(key), Value::Text(value)],
                    ))
                    .map_err(|e| format!("restoring sys_setting: {e}"))?;
                report.applied += 1;
            }
            "sys_billing" => {
                backend
                    .set_billing(&row_batch(
                        &[
                            ("team_id", ColumnType::Text),
                            ("tier", ColumnType::Text),
                            ("status", ColumnType::Text),
                        ],
                        vec![
                            Value::Text(required_string(&record, "team_id")?),
                            Value::Text(required_string(&record, "tier")?),
                            Value::Text(required_string(&record, "status")?),
                        ],
                    ))
                    .map_err(|e| format!("restoring sys_billing: {e}"))?;
                report.applied += 1;
            }
            "sys_policy" => {
                let batch = row_batch(
                    &[
                        ("name", ColumnType::Text),
                        ("allow", ColumnType::Text),
                        ("target", ColumnType::Text),
                    ],
                    vec![
                        Value::Text(required_string(&record, "name")?),
                        optional_text_value(&record, "allow"),
                        optional_text_value(&record, "target"),
                    ],
                );
                if row_exists(
                    &backend,
                    SysNode::Policies,
                    &["name", "allow", "target"],
                    &batch,
                )? {
                    report.skipped_existing += 1;
                } else {
                    backend
                        .insert_policy(&batch)
                        .map_err(|e| format!("restoring sys_policy: {e}"))?;
                    report.applied += 1;
                }
            }
            "sys_driver" => {
                let batch = row_batch(
                    &[
                        ("kind", ColumnType::Text),
                        ("name", ColumnType::Text),
                        ("base_url", ColumnType::Text),
                        ("auth", ColumnType::Text),
                        ("pagination", ColumnType::Text),
                        ("of_type", ColumnType::Text),
                        ("verb", ColumnType::Text),
                        ("body", ColumnType::Text),
                        ("irreversible", ColumnType::Bool),
                    ],
                    vec![
                        Value::Text(required_string(&record, "kind")?),
                        Value::Text(required_string(&record, "name")?),
                        optional_text_value(&record, "base_url"),
                        jsonish_text_value(&record, "auth"),
                        jsonish_text_value(&record, "pagination"),
                        optional_text_value(&record, "of_type"),
                        optional_text_value(&record, "verb"),
                        jsonish_text_value(&record, "body"),
                        Value::Bool(record["irreversible"].as_bool().unwrap_or(false)),
                    ],
                );
                if row_exists(
                    &backend,
                    SysNode::Drivers,
                    &["kind", "name", "base_url"],
                    &batch,
                )? {
                    report.skipped_existing += 1;
                } else {
                    backend
                        .insert_driver(&batch)
                        .map_err(|e| format!("restoring sys_driver: {e}"))?;
                    report.applied += 1;
                }
            }
            "path_binding" => {
                let Some(conn) = project_conn else {
                    return Err("cannot restore path_binding without a project database".into());
                };
                let path = required_string(&record, "path")?;
                if let Some(alias_of) = optional_string(&record, "alias_of") {
                    crate::path_binding::db_upsert_alias(conn, &path, &alias_of)
                        .map_err(|e| format!("restoring path_binding alias: {e}"))?;
                } else {
                    crate::path_binding::db_upsert_binding(
                        conn,
                        &path,
                        &required_string(&record, "driver_id")?,
                        optional_string(&record, "at_locator").as_deref(),
                        optional_string(&record, "secret_ref").as_deref(),
                        optional_string(&record, "host").as_deref(),
                        optional_string(&record, "account").as_deref(),
                        optional_string(&record, "app").as_deref(),
                    )
                    .map_err(|e| format!("restoring path_binding: {e}"))?;
                }
                report.applied += 1;
            }
            other => return Err(format!("unsupported dump record type `{other}`")),
        }
    }
    Ok(report)
}

fn parse_records(input: &str) -> Result<Vec<JsonValue>, String> {
    let mut out = Vec::new();
    for (idx, line) in input.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        out.push(
            serde_json::from_str::<JsonValue>(line)
                .map_err(|e| format!("invalid JSONL at line {}: {e}", idx + 1))?,
        );
    }
    if out.is_empty() {
        return Err("restore input is empty".into());
    }
    Ok(out)
}

fn validate_header(first: Option<&JsonValue>) -> Result<(), String> {
    let Some(header) = first else {
        return Err("restore input is empty".into());
    };
    if header["record"] != "header" || header["format"] != "qfs-state-jsonl-v1" {
        return Err("restore expects a qfs-state-jsonl-v1 header record".into());
    }
    Ok(())
}

fn record_type(record: &JsonValue) -> Result<&str, String> {
    record["record"]
        .as_str()
        .ok_or_else(|| "dump record is missing string field `record`".to_string())
}

fn row_batch(cols: &[(&str, ColumnType)], values: Vec<Value>) -> RowBatch {
    RowBatch::new(
        Schema::new(
            cols.iter()
                .map(|(name, ty)| Column::new(*name, ty.clone(), true))
                .collect(),
        ),
        vec![Row::new(values)],
    )
}

fn row_exists(
    backend: &crate::sys::SystemDbBackend,
    node: SysNode,
    keys: &[&str],
    candidate: &RowBatch,
) -> Result<bool, String> {
    let rows = backend
        .scan(node)
        .map_err(|e| format!("scanning existing state: {e}"))?;
    Ok(rows.rows.iter().any(|row| {
        keys.iter().all(|key| {
            let Some(existing) = cell(&rows, row, key) else {
                return false;
            };
            let Some(wanted) = candidate_cell(candidate, key) else {
                return false;
            };
            existing == wanted
        })
    }))
}

fn cell<'a>(batch: &'a RowBatch, row: &'a Row, col: &str) -> Option<&'a Value> {
    let idx = batch.schema.columns.iter().position(|c| c.name == col)?;
    row.values.get(idx)
}

fn candidate_cell<'a>(batch: &'a RowBatch, col: &str) -> Option<&'a Value> {
    let idx = batch.schema.columns.iter().position(|c| c.name == col)?;
    batch.rows.first()?.values.get(idx)
}

fn required_string(record: &JsonValue, key: &str) -> Result<String, String> {
    optional_string(record, key).ok_or_else(|| format!("record missing required string `{key}`"))
}

fn optional_string(record: &JsonValue, key: &str) -> Option<String> {
    record.get(key)?.as_str().map(str::to_string)
}

fn optional_text_value(record: &JsonValue, key: &str) -> Value {
    optional_string(record, key).map_or(Value::Null, Value::Text)
}

fn jsonish_text_value(record: &JsonValue, key: &str) -> Value {
    match record.get(key) {
        Some(JsonValue::Null) | None => Value::Null,
        Some(JsonValue::String(s)) => Value::Text(s.clone()),
        Some(v) => Value::Text(v.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testenv::HomeGuard;
    use qfs_store::{FileSource, ProjectDb, SystemDb};

    #[test]
    fn restore_previews_then_commits_a_dump_idempotently() {
        let dump = {
            let home = HomeGuard::with_passphrase("source-pass");
            let sys = SystemDb::open(&FileSource::new(home.system_db_path())).unwrap();
            sys.db()
                .conn()
                .execute(
                    "INSERT INTO sys_settings (key, value) VALUES ('safety_mode', 'policy-only')",
                    [],
                )
                .unwrap();
            sys.db()
                .conn()
                .execute(
                    "INSERT INTO sys_policies (name, allow, target) VALUES ('analysts', 'SELECT', '/sql/*')",
                    [],
                )
                .unwrap();
            sys.db()
                .conn()
                .execute(
                    "INSERT INTO sys_drivers (kind, name, base_url, auth, irreversible) \
                     VALUES ('driver', 'chatwork', 'https://api.chatwork.com/v2', \
                             '{\"kind\":\"header\",\"name\":\"x-chatworktoken\"}', 0)",
                    [],
                )
                .unwrap();
            drop(sys);
            let project = ProjectDb::open(&FileSource::new(
                home.system_db_path().with_file_name("project.db"),
            ))
            .unwrap();
            project
                .db()
                .conn()
                .execute(
                    "INSERT INTO path_binding (path, driver_id, at_locator, secret_ref, host, account) \
                     VALUES ('/chat', 'chatwork', 'https://api.chatwork.com/v2', 'vault:chatwork/work', \
                             'local', 'work')",
                    [],
                )
                .unwrap();
            project
                .db()
                .conn()
                .execute(
                    "INSERT INTO path_binding (path, driver_id, at_locator, secret_ref, host, account) \
                     VALUES ('/cf', 'cf', 'cloudflare-account', NULL, 'local', 'mycf')",
                    [],
                )
                .unwrap();
            drop(project);
            crate::dump::dump_jsonl(true, "2026-07-07T00:00:00Z").unwrap()
        };

        let _dest = HomeGuard::with_passphrase("dest-pass");
        let preview = restore_jsonl(&dump, false).unwrap();
        assert_eq!(preview.applied, 0);
        assert!(crate::dump::dump_jsonl(false, "2026-07-07T00:00:00Z")
            .unwrap()
            .contains(r#""record":"header""#));

        let committed = restore_jsonl(&dump, true).unwrap();
        assert_eq!(committed.applied, 5);
        let second = restore_jsonl(&dump, true).unwrap();
        assert!(second.skipped_existing >= 2);

        let restored = crate::dump::dump_jsonl(false, "2026-07-07T00:00:00Z").unwrap();
        assert!(restored.contains(r#""record":"sys_setting""#));
        assert!(restored.contains(r#""record":"sys_policy""#));
        assert!(restored.contains(r#""record":"sys_driver""#));
        assert!(restored.contains(r#""record":"path_binding""#));
        assert!(restored.contains("vault:chatwork/work"));
        assert!(restored.contains(r#""driver_id":"cf""#));
        assert!(restored.contains(r#""at_locator":"cloudflare-account""#));
        assert!(restored.contains(r#""account":"mycf""#));
        assert!(!restored.contains("PLAINTEXT"));
    }

    #[test]
    fn restore_skips_secretish_settings_never_clobbering_live_values() {
        // The shipped flaw (blueprint §16, amended): the dump redacts a secretish setting to the
        // literal `<redacted>`, and replaying it used to write that marker OVER the live secret.
        // Restore must skip (never write) secretish settings, counted in the report.
        let dump = {
            let home = HomeGuard::with_passphrase("source-pass");
            let sys = SystemDb::open(&FileSource::new(home.system_db_path())).unwrap();
            sys.db()
                .conn()
                .execute(
                    "INSERT INTO sys_settings (key, value) VALUES ('api_token', 'SOURCE-TOKEN')",
                    [],
                )
                .unwrap();
            sys.db()
                .conn()
                .execute(
                    "INSERT INTO sys_settings (key, value) VALUES ('safety_mode', 'policy-only')",
                    [],
                )
                .unwrap();
            drop(sys);
            crate::dump::dump_jsonl(false, "2026-07-08T00:00:00Z").unwrap()
        };
        assert!(dump.contains("<redacted>"), "the dump redacts the token");
        assert!(!dump.contains("SOURCE-TOKEN"));

        // A destination deployment with a LIVE secretish value that must survive the replay.
        let home = HomeGuard::with_passphrase("dest-pass");
        {
            let sys = SystemDb::open(&FileSource::new(home.system_db_path())).unwrap();
            sys.db()
                .conn()
                .execute(
                    "INSERT INTO sys_settings (key, value) VALUES ('api_token', 'LIVE-TOKEN')",
                    [],
                )
                .unwrap();
        }

        let report = restore_jsonl(&dump, true).unwrap();
        assert_eq!(report.skipped_secretish, 1, "the token record is skipped");

        let sys = SystemDb::open(&FileSource::new(home.system_db_path())).unwrap();
        let live: String = sys
            .db()
            .conn()
            .query_row(
                "SELECT value FROM sys_settings WHERE key = 'api_token'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(live, "LIVE-TOKEN", "the live secret value is untouched");
        let mode: String = sys
            .db()
            .conn()
            .query_row(
                "SELECT value FROM sys_settings WHERE key = 'safety_mode'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(mode, "policy-only", "non-secretish settings still restore");
    }

    #[test]
    fn restore_rejects_unknown_format() {
        let err =
            restore_jsonl("{\"record\":\"header\",\"format\":\"nope\"}\n", false).unwrap_err();
        assert!(err.contains("qfs-state-jsonl-v1"));
    }
}
