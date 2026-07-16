//! Secret-free qfs state dump.
//!
//! `qfs dump` is a backup/review surface over qfs configuration state, not a credential export.
//! It reads current-state tables plus optional DDL event metadata and never selects vault secret
//! columns (`nonce`, `ciphertext`) or plaintext account tokens.

use qfs_cmd::{DumpAction, DumpFormat};
use rusqlite::OptionalExtension;
use serde_json::{json, Value as JsonValue};

/// Run the injected `qfs dump` command. Returns the process exit code.
#[must_use]
pub fn run_dump(action: &DumpAction) -> i32 {
    match action.format {
        DumpFormat::Jsonl => match dump_jsonl_now(action.include_events) {
            Ok(out) => {
                print!("{out}");
                0
            }
            Err(e) => {
                eprintln!("qfs: error: {e}");
                1
            }
        },
    }
}

fn dump_jsonl_now(include_events: bool) -> Result<String, String> {
    dump_jsonl(include_events, &now_rfc3339())
}

pub(crate) fn dump_jsonl(include_events: bool, generated_at: &str) -> Result<String, String> {
    let system = crate::store::open_system_db()
        .map_err(|e| format!("opening the system database: {e}"))?
        .ok_or("cannot determine the system database path (set HOME or XDG_CONFIG_HOME)")?;
    let sys_migrations = qfs_store::applied_migrations(system.db())
        .map_err(|e| format!("reading migrations: {e}"))?;
    let sys = system.db().conn();

    let project = crate::store::open_project_db()
        .map_err(|e| format!("opening the project database: {e}"))?;
    let project_migration_count = project
        .as_ref()
        .map(|p| qfs_store::applied_migrations(p.db()).map(|m| m.len()))
        .transpose()
        .map_err(|e| format!("reading project migrations: {e}"))?;

    let event_head = sys
        .query_row(
            "SELECT seq, hash FROM sys_ddl_events ORDER BY seq DESC LIMIT 1",
            [],
            |r| {
                Ok(json!({
                    "seq": r.get::<_, i64>(0)?,
                    "hash": r.get::<_, String>(1)?,
                }))
            },
        )
        .optional()
        .map_err(|e| format!("reading DDL event head: {e}"))?;

    let mut out = String::new();
    push_line(
        &mut out,
        json!({
            "record": "header",
            "format": "qfs-state-jsonl-v1",
            "generated_at": generated_at,
            "qfs_version": crate::version::VERSION,
            "system_migrations": sys_migrations.len(),
            "project_migrations": project_migration_count,
            "ddl_event_head": event_head,
            "credential_boundary": "credential values are excluded; restore the encrypted vault separately",
        }),
    )?;

    dump_sys_drivers(&mut out, sys)?;
    dump_sys_settings(&mut out, sys)?;
    dump_sys_policies(&mut out, sys)?;
    dump_sys_billing(&mut out, sys)?;
    // Both re-homed declarative tables (20260716143641) dump from the System DB — including the
    // previously-missing accounts/consent section (selectors + metadata, never a token).
    dump_path_bindings(&mut out, sys)?;
    dump_accounts(&mut out, sys)?;
    if include_events {
        dump_ddl_events(&mut out, sys)?;
    }
    Ok(out)
}

fn dump_sys_drivers(out: &mut String, conn: &rusqlite::Connection) -> Result<(), String> {
    let mut stmt = conn
        .prepare(
            "SELECT kind, name, base_url, auth, pagination, of_type, verb, body, irreversible, \
                    created_at \
             FROM sys_drivers ORDER BY id",
        )
        .map_err(|e| format!("reading sys_drivers: {e}"))?;
    let rows = stmt
        .query_map([], |r| {
            Ok(json!({
                "record": "sys_driver",
                "kind": r.get::<_, String>(0)?,
                "name": r.get::<_, String>(1)?,
                "base_url": r.get::<_, Option<String>>(2)?,
                "auth": json_text_or_string(r.get::<_, Option<String>>(3)?),
                "pagination": json_text_or_string(r.get::<_, Option<String>>(4)?),
                "of_type": r.get::<_, Option<String>>(5)?,
                "verb": r.get::<_, Option<String>>(6)?,
                "body": json_text_or_string(r.get::<_, Option<String>>(7)?),
                "irreversible": r.get::<_, i64>(8)? != 0,
                "created_at": r.get::<_, Option<String>>(9)?,
            }))
        })
        .map_err(|e| format!("reading sys_drivers: {e}"))?;
    push_rows(out, rows)
}

fn dump_sys_settings(out: &mut String, conn: &rusqlite::Connection) -> Result<(), String> {
    let mut stmt = conn
        .prepare("SELECT key, value, updated_at FROM sys_settings ORDER BY key")
        .map_err(|e| format!("reading sys_settings: {e}"))?;
    let rows = stmt
        .query_map([], |r| {
            let key = r.get::<_, String>(0)?;
            Ok(json!({
                "record": "sys_setting",
                "key": key,
                "value": setting_dump_value(&key, r.get::<_, String>(1)?),
                "updated_at": r.get::<_, Option<String>>(2)?,
            }))
        })
        .map_err(|e| format!("reading sys_settings: {e}"))?;
    push_rows(out, rows)
}

fn dump_sys_policies(out: &mut String, conn: &rusqlite::Connection) -> Result<(), String> {
    let mut stmt = conn
        .prepare("SELECT name, allow, target, created_at FROM sys_policies ORDER BY id")
        .map_err(|e| format!("reading sys_policies: {e}"))?;
    let rows = stmt
        .query_map([], |r| {
            Ok(json!({
                "record": "sys_policy",
                "name": r.get::<_, String>(0)?,
                "allow": r.get::<_, Option<String>>(1)?,
                "target": r.get::<_, Option<String>>(2)?,
                "created_at": r.get::<_, Option<String>>(3)?,
            }))
        })
        .map_err(|e| format!("reading sys_policies: {e}"))?;
    push_rows(out, rows)
}

fn dump_sys_billing(out: &mut String, conn: &rusqlite::Connection) -> Result<(), String> {
    let mut stmt = conn
        .prepare(
            "SELECT team_id, tier, status, current_period_end, updated_at \
             FROM billing_subscriptions ORDER BY team_id",
        )
        .map_err(|e| format!("reading billing_subscriptions: {e}"))?;
    let rows = stmt
        .query_map([], |r| {
            Ok(json!({
                "record": "sys_billing",
                "team_id": r.get::<_, String>(0)?,
                "tier": r.get::<_, String>(1)?,
                "status": r.get::<_, String>(2)?,
                "current_period_end": r.get::<_, Option<String>>(3)?,
                "updated_at": r.get::<_, Option<String>>(4)?,
            }))
        })
        .map_err(|e| format!("reading billing_subscriptions: {e}"))?;
    push_rows(out, rows)
}

fn dump_path_bindings(out: &mut String, conn: &rusqlite::Connection) -> Result<(), String> {
    let mut stmt = conn
        .prepare(
            "SELECT path, driver_id, at_locator, secret_ref, alias_of, host, account, app, created_at \
             FROM path_binding ORDER BY path",
        )
        .map_err(|e| format!("reading path_binding: {e}"))?;
    let rows = stmt
        .query_map([], |r| {
            Ok(json!({
                "record": "path_binding",
                "path": r.get::<_, String>(0)?,
                "driver_id": r.get::<_, Option<String>>(1)?,
                "at_locator": r.get::<_, Option<String>>(2)?,
                "secret_ref": r.get::<_, Option<String>>(3)?,
                "alias_of": r.get::<_, Option<String>>(4)?,
                "host": r.get::<_, String>(5)?,
                "account": r.get::<_, Option<String>>(6)?,
                "app": r.get::<_, Option<String>>(7)?,
                "created_at": r.get::<_, Option<String>>(8)?,
                "credential_note": "secret_ref is a reference; credential value is excluded",
            }))
        })
        .map_err(|e| format!("reading path_binding: {e}"))?;
    push_rows(out, rows)
}

/// The accounts/consent section (20260716143641 — previously missing from the dump entirely):
/// one `connection_consent` row per record. Selectors + metadata only — subject/scope/app are
/// labels; the token lives ENCRYPTED in the Project-DB vault and is never here (the same
/// secret-free discipline as every other section).
fn dump_accounts(out: &mut String, conn: &rusqlite::Connection) -> Result<(), String> {
    let mut stmt = conn
        .prepare(
            "SELECT driver, connection, subject, scope, app, granted_at \
             FROM connection_consent ORDER BY driver, connection",
        )
        .map_err(|e| format!("reading connection_consent: {e}"))?;
    let rows = stmt
        .query_map([], |r| {
            Ok(json!({
                "record": "connection_consent",
                "driver": r.get::<_, String>(0)?,
                "connection": r.get::<_, String>(1)?,
                "subject": r.get::<_, String>(2)?,
                "scope": r.get::<_, String>(3)?,
                "app": r.get::<_, Option<String>>(4)?,
                "granted_at": r.get::<_, Option<String>>(5)?,
                "credential_note": "consent metadata only; the token is excluded",
            }))
        })
        .map_err(|e| format!("reading connection_consent: {e}"))?;
    push_rows(out, rows)
}

fn dump_ddl_events(out: &mut String, conn: &rusqlite::Connection) -> Result<(), String> {
    let mut stmt = conn
        .prepare(
            "SELECT seq, tx_id, actor, ts, target_path, verb, source_text, payload_json, \
                    content_hash, prev_hash, hash \
             FROM sys_ddl_events ORDER BY seq",
        )
        .map_err(|e| format!("reading sys_ddl_events: {e}"))?;
    let rows = stmt
        .query_map([], |r| {
            let payload: String = r.get(7)?;
            Ok(json!({
                "record": "ddl_event",
                "seq": r.get::<_, i64>(0)?,
                "tx_id": r.get::<_, String>(1)?,
                "actor": r.get::<_, String>(2)?,
                "ts": r.get::<_, String>(3)?,
                "target_path": r.get::<_, String>(4)?,
                "verb": r.get::<_, String>(5)?,
                "source_text": r.get::<_, Option<String>>(6)?,
                "payload": serde_json::from_str::<JsonValue>(&payload).unwrap_or(JsonValue::String(payload)),
                "content_hash": r.get::<_, String>(8)?,
                "prev_hash": r.get::<_, String>(9)?,
                "hash": r.get::<_, String>(10)?,
            }))
        })
        .map_err(|e| format!("reading sys_ddl_events: {e}"))?;
    push_rows(out, rows)
}

fn push_rows(
    out: &mut String,
    rows: rusqlite::MappedRows<'_, impl FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<JsonValue>>,
) -> Result<(), String> {
    for row in rows {
        push_line(out, row.map_err(|e| format!("rendering dump row: {e}"))?)?;
    }
    Ok(())
}

fn push_line(out: &mut String, value: JsonValue) -> Result<(), String> {
    out.push_str(&serde_json::to_string(&value).map_err(|e| format!("rendering JSONL: {e}"))?);
    out.push('\n');
    Ok(())
}

fn json_text_or_string(value: Option<String>) -> JsonValue {
    match value {
        Some(s) => serde_json::from_str(&s).unwrap_or(JsonValue::String(s)),
        None => JsonValue::Null,
    }
}

fn setting_dump_value(key: &str, value: String) -> JsonValue {
    // The ONE shared secretish predicate (`qfs_core::secretish_setting_key`): dump redacts,
    // restore skips on replay, and the provisioning universe excludes — all off the same list.
    if qfs_core::secretish_setting_key(key) {
        JsonValue::String("<redacted>".to_string())
    } else {
        JsonValue::String(value)
    }
}

fn now_rfc3339() -> String {
    use time::format_description::well_known::Rfc3339;
    time::OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testenv::HomeGuard;
    use qfs_store::{FileSource, ProjectDb, SystemDb};

    #[test]
    fn jsonl_dump_is_deterministic_and_secret_free() {
        let home = HomeGuard::with_passphrase("test-passphrase");
        let sys = SystemDb::open(&FileSource::new(home.system_db_path())).unwrap();
        let conn = sys.db().conn();
        conn.execute(
            "INSERT INTO sys_drivers (kind, name, base_url, auth, irreversible) \
             VALUES ('driver', 'chatwork', 'https://api.chatwork.com/v2', \
                     '{\"kind\":\"header\",\"name\":\"x-chatworktoken\"}', 0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO sys_settings (key, value) VALUES ('safety_mode', 'policy-only')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO sys_settings (key, value) VALUES ('api_token', 'PLAINTEXT-TOKEN-CANARY')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO sys_policies (name, allow, target) VALUES ('analysts', 'SELECT', '/sql/*')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO sys_ddl_events \
             (seq, tx_id, actor, ts, target_path, verb, payload_json, content_hash, prev_hash, hash) \
             VALUES (1, 'tx-1', 'cli', '2026-07-07T00:00:00Z', '/sys/settings', 'UPSERT', \
                     '{\"kind\":\"setting\",\"key\":\"safety_mode\",\"value\":\"policy-only\"}', \
                     'c', 'p', 'h')",
            [],
        )
        .unwrap();
        // The re-homed declarative tables (20260716143641) dump from the System DB.
        conn.execute(
            "INSERT INTO path_binding (path, driver_id, at_locator, secret_ref, host, account) \
             VALUES ('/chat', 'chatwork', 'https://api.chatwork.com/v2', 'vault:chatwork/work', \
                     'local', 'work')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO path_binding (path, driver_id, at_locator, secret_ref, host, account) \
             VALUES ('/cf', 'cf', 'cloudflare-account', NULL, 'local', 'mycf')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO connection_consent (driver, connection, subject, scope, app) \
             VALUES ('github', 'work', 'op@example.com', '', NULL)",
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
                "INSERT INTO secret_store (driver, connection, nonce, ciphertext) \
                 VALUES ('github', 'work', x'00', ?1)",
                rusqlite::params![b"PLAINTEXT-TOKEN-CANARY".to_vec()],
            )
            .unwrap();
        drop(project);

        let a = dump_jsonl(true, "2026-07-07T00:00:00Z").unwrap();
        let b = dump_jsonl(true, "2026-07-07T00:00:00Z").unwrap();
        assert_eq!(a, b);
        assert!(a.contains(r#""record":"header""#));
        assert!(a.contains(r#""record":"sys_driver""#));
        assert!(a.contains(r#""record":"path_binding""#));
        assert!(a.contains(r#""record":"connection_consent""#));
        assert!(a.contains(r#""subject":"op@example.com""#));
        assert!(a.contains(r#""record":"ddl_event""#));
        assert!(a.contains("vault:chatwork/work"));
        assert!(a.contains(r#""driver_id":"cf""#));
        assert!(a.contains(r#""at_locator":"cloudflare-account""#));
        assert!(a.contains(r#""account":"mycf""#));
        assert!(!a.contains("PLAINTEXT-TOKEN-CANARY"));
        assert!(!a.contains("ciphertext"));

        for line in a.lines() {
            serde_json::from_str::<JsonValue>(line).unwrap();
        }
    }

    #[test]
    fn jsonl_dump_can_omit_events() {
        let _home = HomeGuard::new();
        let out = dump_jsonl(false, "2026-07-07T00:00:00Z").unwrap();
        assert!(out.contains(r#""record":"header""#));
        assert!(!out.contains(r#""record":"ddl_event""#));
    }
}
