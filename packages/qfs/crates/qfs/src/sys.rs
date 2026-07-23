//! The `/sys` administration composition root (ticket t53): the System-DB-backed [`SysBackend`]
//! implementation + the async [`SysReadDriver`] read facet, both hosted in the **`qfs` binary
//! crate**.
//!
//! ## Why the backend lives in the binary (not the leaf driver crate)
//! `qfs-driver-sys` is the vendor-free administration driver (the pure introspective half + the
//! `SysApplier` over the `SysBackend` seam) and is a **`qfs-runtime` consumer**, so the
//! dep-direction guard requires it to be a **leaf** — only the terminal `qfs` binary may depend
//! onto it. The binary IS that leaf and the ONE place that opens a real DB path (decision F), so
//! the real `rusqlite` reads/writes over the System DB (and the Project DB's connection registry)
//! dead-end here, exactly like the SQL driver's `SqliteBackend`. No `rusqlite` type crosses the
//! `SysBackend` boundary (owned qfs DTOs only).
//!
//! ## Safety floor (roadmap §3.2 / §4.6)
//! - `/sys/connections` reads the connection REGISTRY (`secret_store`'s `driver`/`connection`/
//!   `created_at` columns) — names + metadata only. The `nonce`/`ciphertext` columns are NEVER
//!   selected, so no secret material can surface (the vault is read only by `qfs-secrets`).
//! - `INSERT INTO /sys/policies` lands a grant row **and** appends a t76 audit row in ONE
//!   transaction — a torn write can never leave a policy un-audited (administration observes
//!   itself). Because the `/sys` legs self-audit at the source of truth, the CLI commit path's
//!   best-effort emitter SKIPS `/sys` legs (see `commit.rs`) so the chain is not double-written.
//!
//! ## Authorization (flagged, not baked in)
//! `/sys/*` writes are high-privilege. They are gated by the SAME default-deny policy engine as
//! any other driver (the path is the authorization subject). Until the super-admin vs.
//! project-admin split is settled (roadmap §3.4), the binary wires this loopback / local-CLI
//! super-admin only — the split is recorded as an open decision rather than baked into a model.

use std::sync::{Arc, Mutex};

use qfs_core::{CfsError, RequestContext, RowBatch};
use qfs_driver_sys::{node_for_path, sys_node_schema, SysBackend, SysError, SysNode};
use qfs_exec::ReadDriver;
use qfs_pushdown::ScanNode;
use qfs_store::audit::{AuditEvent, ChainHead, GENESIS_PREV_HASH};
use qfs_store::ddl_events::{DdlEvent, GENESIS_PREV_HASH as DDL_GENESIS_PREV_HASH};
use qfs_types::{Row, Value};
use rusqlite::{Connection, OptionalExtension};
use serde_json::{Map as JsonMap, Value as JsonValue};

/// The bounded audit live-tail cap (mirrors `crate::audit::TAIL_CAP`) — `/sys` mutations append
/// to the same t76 chain and trim to this many rows.
const TAIL_CAP: i64 = 256;

/// The acting principal recorded on a `/sys` mutation's audit event — a label, never a credential
/// (t76 / §4.6). The local CLI invocation is the actor today; a request-derived identity replaces
/// this once the super-admin session model lands.
const ACTOR_CLI: &str = "cli";

/// The acting principal recorded when a PROVIDER WEBHOOK (t67) updates a billing plan — a label, not
/// a credential. The provider's signing secret was HMAC-verified upstream (`qfs-watchtower`) and
/// never reaches here; this names the actor as the payment provider, not a human operator.
const ACTOR_PROVIDER: &str = "provider";

/// The System-DB-backed [`SysBackend`]: the real rusqlite reads/writes over the System DB (and the
/// Project DB's connection registry). Each connection is held behind a `Mutex` (rusqlite is
/// `!Sync`; the mutex provides the `Send + Sync` the trait requires).
pub struct SystemDbBackend {
    /// The System DB (users / projects / audit_tail / sys_policies + the audit chain head).
    system: Mutex<Connection>,
    /// The Project DB connection registry (`secret_store`), if a Project DB resolved. `None`
    /// leaves `/sys/connections` empty rather than failing (best-effort, like the audit emitter).
    project: Option<Mutex<Connection>>,
}

impl SystemDbBackend {
    /// Build a backend over already-migrated connections (the test + composition seam).
    #[must_use]
    pub fn new(system: Connection, project: Option<Connection>) -> Self {
        Self {
            system: Mutex::new(system),
            project: project.map(Mutex::new),
        }
    }

    /// Open the default System DB (+ best-effort Project DB) and build the backend. Returns `None`
    /// when no config home resolves (HOME/XDG unset) — the `/sys` surface is simply not wired
    /// rather than failing the CLI (the same best-effort posture as the audit emitter).
    #[must_use]
    pub fn open_default() -> Option<Self> {
        let system = match crate::store::open_system_db() {
            Ok(Some(sys)) => sys.into_db().into_connection(),
            _ => return None,
        };
        // The connection registry lives in the Project DB; a missing/locked one leaves
        // /sys/connections empty (never a failure).
        let project = match crate::store::open_project_db() {
            Ok(Some(proj)) => Some(proj.into_db().into_connection()),
            _ => None,
        };
        Some(Self::new(system, project))
    }

    fn declared_type_body(&self, path: &str) -> Option<String> {
        let conn = self.system.lock().ok()?;
        conn.query_row(
            "SELECT body FROM sys_drivers WHERE kind = 'type' AND name = ?1 ORDER BY id DESC \
             LIMIT 1",
            [path],
            |r| r.get::<_, Option<String>>(0),
        )
        .optional()
        .ok()
        .flatten()
        .flatten()
    }
}

impl SysBackend for SystemDbBackend {
    fn scan(&self, node: SysNode) -> Result<RowBatch, SysError> {
        let schema = sys_node_schema(node);
        let rows = match node {
            SysNode::Users => self.scan_system(
                "SELECT id, primary_email, status, created_at FROM users ORDER BY id",
                |r| {
                    Ok(Row::new(vec![
                        int(r, 0)?,
                        text(r, 1)?,
                        text(r, 2)?,
                        nullable_text(r, 3)?,
                    ]))
                },
            )?,
            SysNode::Projects => self.scan_system(
                "SELECT id, slug, created_at FROM projects ORDER BY id",
                |r| {
                    Ok(Row::new(vec![
                        int(r, 0)?,
                        text(r, 1)?,
                        nullable_text(r, 2)?,
                    ]))
                },
            )?,
            SysNode::Audit => self.scan_system(
                "SELECT seq, actor, connection, verb, path, committed, ts \
                 FROM audit_tail ORDER BY seq",
                |r| {
                    Ok(Row::new(vec![
                        int(r, 0)?,
                        text(r, 1)?,
                        text(r, 2)?,
                        text(r, 3)?,
                        text(r, 4)?,
                        Value::Bool(r.get::<_, i64>(5)? != 0),
                        text(r, 6)?,
                    ]))
                },
            )?,
            // Names + metadata ONLY — `nonce`/`ciphertext` are NEVER selected (the redaction
            // contract, §3.2). The registry is the Project DB's `secret_store`.
            SysNode::Connections => self.scan_project(
                "SELECT driver, connection, created_at FROM secret_store \
                 ORDER BY driver, connection",
                |r| {
                    Ok(Row::new(vec![
                        text(r, 0)?,
                        text(r, 1)?,
                        nullable_text(r, 2)?,
                    ]))
                },
            )?,
            // t100020 (the CONNECT model): the DEFINED-PATH binding registry, from the System DB
            // (re-homed by 20260716143641). Metadata only — `secret_ref` is a REFERENCE
            // (`env:`/`vault:`), never a secret value.
            SysNode::Paths => self.scan_system(
                "SELECT path, driver_id, at_locator, secret_ref, alias_of, host, account, app, \
                        created_at \
                 FROM path_binding ORDER BY path",
                |r| {
                    Ok(Row::new(vec![
                        text(r, 0)?,
                        nullable_text(r, 1)?,
                        nullable_text(r, 2)?,
                        nullable_text(r, 3)?,
                        nullable_text(r, 4)?,
                        text(r, 5)?,
                        nullable_text(r, 6)?,
                        nullable_text(r, 7)?,
                        nullable_text(r, 8)?,
                    ]))
                },
            )?,
            SysNode::Policies => self.scan_system(
                "SELECT name, allow, target, created_at FROM sys_policies ORDER BY id",
                |r| {
                    Ok(Row::new(vec![
                        text(r, 0)?,
                        nullable_text(r, 1)?,
                        nullable_text(r, 2)?,
                        nullable_text(r, 3)?,
                    ]))
                },
            )?,
            // t77: the telemetry counter live view — read the in-process metrics registry (NOT the
            // System DB; qfs emits + does not store the stream, decision V). Metadata only:
            // instrument name + kind + integer counter value.
            SysNode::Metrics => crate::telemetry::metrics_snapshot()
                .into_iter()
                .map(|m| {
                    #[allow(clippy::cast_possible_truncation)]
                    Row::new(vec![
                        Value::Text(m.name),
                        Value::Text(m.kind.as_str().to_string()),
                        Value::Int(m.value as i64),
                    ])
                })
                .collect(),
            // t59: the deployment settings key/value (the safety-mode home). Metadata only.
            SysNode::Settings => self.scan_system(
                "SELECT key, value, updated_at FROM sys_settings ORDER BY key",
                |r| {
                    Ok(Row::new(vec![
                        text(r, 0)?,
                        text(r, 1)?,
                        nullable_text(r, 2)?,
                    ]))
                },
            )?,
            // t67: the per-team billing plan (the entitlement gate's authority). Metadata only —
            // team id + tier/status labels + period end. NEVER a payment secret (the schema has no
            // column for one; the provider keys live envelope-encrypted in the vault).
            SysNode::Billing => self.scan_system(
                "SELECT team_id, tier, status, current_period_end, updated_at \
                 FROM billing_subscriptions ORDER BY team_id",
                |r| {
                    Ok(Row::new(vec![
                        text(r, 0)?,
                        text(r, 1)?,
                        text(r, 2)?,
                        nullable_text(r, 3)?,
                        nullable_text(r, 4)?,
                    ]))
                },
            )?,
            // §13: the declared-driver registry. Declaration text + selectors only — the `auth`
            // descriptor names a SCHEME, never a token (the credential-free-script contract).
            SysNode::Drivers => self.scan_system(
                "SELECT kind, name, base_url, auth, pagination, of_type, verb, body, irreversible, \
                        created_at \
                 FROM sys_drivers ORDER BY id",
                |r| {
                    Ok(Row::new(vec![
                        text(r, 0)?,
                        text(r, 1)?,
                        nullable_text(r, 2)?,
                        nullable_text(r, 3)?,
                        nullable_text(r, 4)?,
                        nullable_text(r, 5)?,
                        nullable_text(r, 6)?,
                        nullable_text(r, 7)?,
                        Value::Bool(r.get::<_, i64>(8)? != 0),
                        nullable_text(r, 9)?,
                    ]))
                },
            )?,
            // 20260703040000 (the CREATE ACCOUNT model): the service-account consent registry, from
            // the System DB `connection_consent` ledger (re-homed by 20260716143641). USER-FACING
            // provider grain: Google's three driver rows (gmail/gdrive/ga, one consent many drivers)
            // COLLAPSE to one `google` row per email (scope = the union); a cloud account is one row
            // as-is. SELECTORS + METADATA only — there is structurally no token column (the
            // credential is sealed out-of-band).
            SysNode::Accounts => self.scan_system(
                "SELECT 'google' AS provider, connection AS account, MIN(subject) AS subject, \
                        group_concat(DISTINCT scope) AS scope, MIN(app) AS app, \
                        MIN(secret_ref) AS secret_ref, MIN(granted_at) AS created_at \
                   FROM connection_consent WHERE driver IN ('gmail','gdrive','ga') \
                  GROUP BY connection \
                 UNION ALL \
                 SELECT driver AS provider, connection AS account, subject, scope, \
                        app, secret_ref, granted_at AS created_at \
                   FROM connection_consent WHERE driver NOT IN ('gmail','gdrive','ga') \
                 ORDER BY provider, account",
                |r| {
                    Ok(Row::new(vec![
                        text(r, 0)?,
                        text(r, 1)?,
                        nullable_text(r, 2)?,
                        nullable_text(r, 3)?,
                        nullable_text(r, 4)?,
                        nullable_text(r, 5)?,
                        nullable_text(r, 6)?,
                    ]))
                },
            )?,
            // `/sys/whoami` is resolved from the request principal in the read facet
            // (`SysReadDriver::scan`), never from the backend — the backend has no request
            // context. Unreachable here by construction; an honest structured rejection if reached.
            SysNode::Whoami => {
                return Err(SysError::MalformedEffect {
                    reason: "/sys/whoami is resolved from the request principal, not the backend"
                        .into(),
                })
            }
        };
        Ok(RowBatch::new(schema, rows))
    }

    fn insert_policy(&self, row: &RowBatch) -> Result<u64, SysError> {
        let name = required_text(row, "name")?;
        let allow = optional_text(row, "allow");
        let target = optional_text(row, "target");

        let conn = self.system.lock().map_err(poisoned)?;
        let tx = conn.unchecked_transaction().map_err(backend)?;

        // The grant row.
        tx.execute(
            "INSERT INTO sys_policies (name, allow, target) VALUES (?1, ?2, ?3)",
            rusqlite::params![name, allow, target],
        )
        .map_err(backend)?;

        // Administration observes itself: append the t76 audit row in the SAME transaction so a
        // torn write can never leave the policy un-audited. Metadata only (verb + path), never the
        // grant's row data — the boundary `describe` enforces.
        let ts = now_rfc3339();
        append_audit_tx(
            &tx,
            AuditEvent {
                actor: ACTOR_CLI.to_string(),
                connection: "default".to_string(),
                verb: "INSERT".to_string(),
                path: SysNode::Policies.path(),
                committed: true,
                ts: ts.clone(),
            },
        )
        .map_err(backend)?;
        append_ddl_event_tx(
            &tx,
            ddl_event(
                &SysNode::Policies.path(),
                "INSERT",
                row_payload_json("policy", row),
                ts,
            ),
        )
        .map_err(backend)?;

        tx.commit().map_err(backend)?;
        Ok(1)
    }

    fn set_setting(&self, row: &RowBatch) -> Result<u64, SysError> {
        let key = required_text(row, "key")?;
        let value = required_text(row, "value")?;

        let conn = self.system.lock().map_err(poisoned)?;
        let tx = conn.unchecked_transaction().map_err(backend)?;

        // Upsert on `key`: a setting is single-valued, so re-setting it replaces the prior value
        // (and bumps `updated_at`). The safety mode is one such row (`key = 'safety_mode'`).
        tx.execute(
            "INSERT INTO sys_settings (key, value, updated_at) \
             VALUES (?1, ?2, strftime('%Y-%m-%dT%H:%M:%SZ','now')) \
             ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
            rusqlite::params![key, value],
        )
        .map_err(backend)?;

        // Administration observes itself: append the t76 audit row in the SAME transaction (a torn
        // write can never leave the setting un-audited). Metadata only (verb + path), never the
        // value — the boundary `describe` enforces.
        let ts = now_rfc3339();
        append_audit_tx(
            &tx,
            AuditEvent {
                actor: ACTOR_CLI.to_string(),
                connection: "default".to_string(),
                verb: "INSERT".to_string(),
                path: SysNode::Settings.path(),
                committed: true,
                ts: ts.clone(),
            },
        )
        .map_err(backend)?;
        append_ddl_event_tx(
            &tx,
            ddl_event(
                &SysNode::Settings.path(),
                "UPSERT",
                setting_payload_json(&key, &value),
                ts,
            ),
        )
        .map_err(backend)?;

        tx.commit().map_err(backend)?;
        Ok(1)
    }

    fn set_billing(&self, row: &RowBatch) -> Result<u64, SysError> {
        let team_id = required_text(row, "team_id")?;
        let tier = required_text(row, "tier")?;
        let status = required_text(row, "status")?;
        let current_period_end = optional_text(row, "current_period_end");

        let conn = self.system.lock().map_err(poisoned)?;
        let tx = conn.unchecked_transaction().map_err(backend)?;
        upsert_billing_tx(&tx, &team_id, &tier, &status, current_period_end.as_deref())
            .map_err(backend)?;
        // Administration observes itself: append the t76 audit row in the SAME transaction. Metadata
        // only (verb + path), never the plan row — the boundary `describe` enforces.
        let ts = now_rfc3339();
        append_audit_tx(
            &tx,
            AuditEvent {
                actor: ACTOR_CLI.to_string(),
                connection: "default".to_string(),
                verb: "INSERT".to_string(),
                path: SysNode::Billing.path(),
                committed: true,
                ts: ts.clone(),
            },
        )
        .map_err(backend)?;
        append_ddl_event_tx(
            &tx,
            ddl_event(
                &SysNode::Billing.path(),
                "UPSERT",
                row_payload_json("billing", row),
                ts,
            ),
        )
        .map_err(backend)?;
        tx.commit().map_err(backend)?;
        Ok(1)
    }

    fn upsert_binding(&self, row: &RowBatch) -> Result<u64, SysError> {
        // t100020 (the CONNECT model): bind / re-bind a defined path in the SYSTEM DB `path_binding`
        // table (upsert on `path`; re-homed by ticket 20260716143641). A row carrying `alias_of` is
        // an ALIAS (reuse another defined path's connection); otherwise it is a FULL connect and
        // MUST name a driver. The binding row, its t76 audit row, and its `ddl_event` land in ONE
        // transaction — the `insert_driver` pattern (administration observes itself, atomically).
        let path = required_text(row, "path")?;
        let alias_of = optional_text(row, "alias_of");
        let conn = self.system.lock().map_err(poisoned)?;
        let tx = conn.unchecked_transaction().map_err(backend)?;
        let affected = if let Some(target) = &alias_of {
            crate::path_binding::db_upsert_alias(&tx, &path, target).map_err(binding_err)?
        } else {
            let driver = required_text(row, "driver").map_err(|_| SysError::MalformedEffect {
                reason: "a full CONNECT needs a driver (or a `/path` alias target)".into(),
            })?;
            let at = optional_text(row, "at");
            let secret_ref = optional_text(row, "secret_ref");
            // ADR 0008: the mount coordinate — an absent HOST clause means the implicit embedded
            // `local` host (defaulted in the binding I/O); `account` is a label, never a token.
            let host = optional_text(row, "host");
            let account = optional_text(row, "account");
            let app = optional_text(row, "app");
            crate::path_binding::db_upsert_binding(
                &tx,
                &path,
                &driver,
                at.as_deref(),
                secret_ref.as_deref(),
                host.as_deref(),
                account.as_deref(),
                app.as_deref(),
            )
            .map_err(binding_err)?
        };
        ledgered_paths_write_tx(&tx, "INSERT", &path, row_payload_json("binding", row))
            .map_err(backend)?;
        tx.commit().map_err(backend)?;
        Ok(affected)
    }

    fn remove_binding(&self, path: &str) -> Result<u64, SysError> {
        // t100020: `DISCONNECT` — remove the defined path from the SYSTEM DB (aliases cascade),
        // with the audit row + `ddl_event` in the same transaction (20260716143641).
        let conn = self.system.lock().map_err(poisoned)?;
        let tx = conn.unchecked_transaction().map_err(backend)?;
        let affected = crate::path_binding::db_remove_binding(&tx, path).map_err(binding_err)?;
        ledgered_paths_write_tx(
            &tx,
            "REMOVE",
            path,
            remove_payload_json("binding", "path", path),
        )
        .map_err(backend)?;
        tx.commit().map_err(backend)?;
        Ok(affected)
    }

    fn insert_driver(&self, row: &RowBatch) -> Result<u64, SysError> {
        // §13 / §5.4: install one declaration (driver/type/view/map) or table contract into the
        // System DB. Declaration text + selectors only — `auth` names a SCHEME, never a token.
        let kind = optional_text(row, "kind").ok_or_else(|| SysError::MalformedEffect {
            reason: "INSERT INTO /sys/drivers requires a non-empty `kind`".into(),
        })?;
        let name = optional_text(row, "name").ok_or_else(|| SysError::MalformedEffect {
            reason: "INSERT INTO /sys/drivers requires a non-empty `name`".into(),
        })?;
        let base_url = optional_text(row, "base_url");
        let auth = optional_text(row, "auth");
        let pagination = optional_text(row, "pagination");
        let of_type = optional_text(row, "of_type");
        let verb = optional_text(row, "verb");
        let body = optional_text(row, "body");
        let irreversible = optional_bool(row, "irreversible");

        // §5.4: a `CREATE TYPE` refinement is well-formedness-checked HERE (the store/commit seam),
        // mirroring how a `CREATE TRANSFORM` re-validates its INPUT/OUTPUT before the row lands — so
        // a malformed refinement (non-boolean, impure builtin, unknown/`unknown`-typed column, …)
        // fails at CREATE and never at first write.
        if kind == "type" {
            if qfs_core::ddl::types::type_name_shadows_base(&name) {
                return Err(SysError::MalformedEffect {
                    reason: qfs_core::ddl::types::TypeDefError::TypeNameShadowsBase {
                        name: name.clone(),
                    }
                    .to_string(),
                });
            }
            if let Some(body_json) = &body {
                qfs_core::ddl::types::validate_type_def_with_catalog(
                    body_json,
                    &qfs_core::StdlibRegistry::with_core(),
                    |path| self.declared_type_body(path),
                )
                .map_err(|e| SysError::MalformedEffect {
                    reason: e.to_string(),
                })?;
            }
        }

        let conn = self.system.lock().map_err(poisoned)?;
        let tx = conn.unchecked_transaction().map_err(backend)?;

        // Re-install REPLACES (owner ruling 2026-07-16). A declaration's identity is
        // `(kind, name, verb)` — the key the read paths group by — so the superseded row is
        // deleted in the SAME transaction the new one lands in, converging the store to one row
        // per key instead of appending forever. This is a SUPERSEDE, not a destroy: the same
        // replace-by-key `/sys/settings` and `/sys/paths` upserts already perform, so it carries
        // no irreversibility gate. The delete leg is audited exactly like `remove_system_row` —
        // administration observes itself, deletes included — and a first install (nothing
        // superseded) emits no delete event.
        let superseded = tx
            .execute(
                "DELETE FROM sys_drivers WHERE kind = ?1 AND name = ?2 AND verb IS ?3",
                rusqlite::params![kind, name, verb],
            )
            .map_err(backend)?;
        if superseded > 0 {
            let ts = now_rfc3339();
            let mut key = JsonMap::new();
            key.insert("kind".to_string(), JsonValue::String(kind.clone()));
            key.insert("name".to_string(), JsonValue::String(name.clone()));
            key.insert(
                "verb".to_string(),
                verb.clone().map_or(JsonValue::Null, JsonValue::String),
            );
            key.insert(
                "superseded_rows".to_string(),
                JsonValue::from(superseded as u64),
            );
            append_audit_tx(
                &tx,
                AuditEvent {
                    actor: ACTOR_CLI.to_string(),
                    connection: "default".to_string(),
                    verb: "REMOVE".to_string(),
                    path: SysNode::Drivers.path(),
                    committed: true,
                    ts: ts.clone(),
                },
            )
            .map_err(backend)?;
            append_ddl_event_tx(
                &tx,
                ddl_event(
                    &SysNode::Drivers.path(),
                    "REMOVE",
                    JsonValue::Object(key).to_string(),
                    ts,
                ),
            )
            .map_err(backend)?;
        }

        tx.execute(
            "INSERT INTO sys_drivers \
                 (kind, name, base_url, auth, pagination, of_type, verb, body, irreversible) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            rusqlite::params![
                kind,
                name,
                base_url,
                auth,
                pagination,
                of_type,
                verb,
                body,
                i64::from(irreversible),
            ],
        )
        .map_err(backend)?;

        // Administration observes itself: append the t76 audit row in the SAME transaction (a torn
        // write can never leave the declaration un-audited). Metadata only (verb + path), never the
        // declaration body — the boundary `describe` enforces.
        let ts = now_rfc3339();
        append_audit_tx(
            &tx,
            AuditEvent {
                actor: ACTOR_CLI.to_string(),
                connection: "default".to_string(),
                verb: "INSERT".to_string(),
                path: SysNode::Drivers.path(),
                committed: true,
                ts: ts.clone(),
            },
        )
        .map_err(backend)?;
        append_ddl_event_tx(
            &tx,
            ddl_event(
                &SysNode::Drivers.path(),
                "INSERT",
                row_payload_json("driver", row),
                ts,
            ),
        )
        .map_err(backend)?;

        tx.commit().map_err(backend)?;
        Ok(1)
    }

    fn record_account(&self, row: &RowBatch) -> Result<u64, SysError> {
        // 20260703040000 (the CREATE ACCOUNT model): declare a service account by RECORDING CONSENT.
        // Delegate to the SHARED account logic (`crate::account::declare_account`) — it enforces the
        // signed-in-operator gate and writes the SAME `connection_consent` state the CLI
        // `qfs account add` does (one writer, one gate). It opens its OWN Project DB connection (not
        // `self.project`), so there is no double-lock. The token VALUE is never in this row.
        let provider = required_text(row, "provider")?;
        let account = required_text(row, "account")?;
        let app = optional_text(row, "app");
        // 20260718203325: an optional `SECRET '<ref>'` reference rides as the `secret_ref` column —
        // a selector (`env:`/`vault:`) resolved lazily at bind time, never a token in this row.
        let secret_ref = optional_text(row, "secret_ref");
        crate::account::declare_account(&provider, &account, app.as_deref(), secret_ref.as_deref())
            .map_err(SysError::Backend)?;
        // The consent write is ledgered INSIDE `declare_account`'s own System-DB transaction
        // (audit + ddl_event; ticket 20260716143641) — nothing to append here.
        Ok(1)
    }

    fn remove_account(&self, provider: &str, account: &str) -> Result<u64, SysError> {
        // Complete deletion (token + consent) via the shared `crate::account::remove_account` — the
        // same path `qfs account remove` takes (data sovereignty: deletion is first-class + complete).
        // The consent delete is ledgered inside that shared writer's System-DB transaction.
        crate::account::remove_account(provider, account).map_err(SysError::Backend)?;
        Ok(1)
    }

    fn update_policy(&self, row: &RowBatch) -> Result<u64, SysError> {
        // Provisioning reconcile UPDATE (blueprint §16): replace a sys policy grant's allow/target
        // by name, in one transaction with the t76 audit + ddl_event (administration observes itself).
        let name = required_text(row, "name")?;
        let allow = optional_text(row, "allow");
        let target = optional_text(row, "target");
        let conn = self.system.lock().map_err(poisoned)?;
        let tx = conn.unchecked_transaction().map_err(backend)?;
        let affected = tx
            .execute(
                "UPDATE sys_policies SET allow = ?2, target = ?3 WHERE name = ?1",
                rusqlite::params![name, allow, target],
            )
            .map_err(backend)? as u64;
        self.audit_and_event_tx(
            &tx,
            SysNode::Policies,
            "UPDATE",
            row_payload_json("policy", row),
        )?;
        tx.commit().map_err(backend)?;
        Ok(affected)
    }

    fn remove_policy(&self, name: &str) -> Result<u64, SysError> {
        self.remove_system_row(
            SysNode::Policies,
            "DELETE FROM sys_policies WHERE name = ?1",
            name,
            remove_payload_json("policy", "name", name),
        )
    }

    fn remove_setting(&self, key: &str) -> Result<u64, SysError> {
        // Belt-and-suspenders: a secretish setting is excluded from the provisioning universe, so
        // the reconcile never produces this — but never destroy a live secret value on a bad plan.
        if secretish_key(key) {
            return Err(SysError::MalformedEffect {
                reason: format!("refusing to remove secretish setting `{key}` (excluded)"),
            });
        }
        self.remove_system_row(
            SysNode::Settings,
            "DELETE FROM sys_settings WHERE key = ?1",
            key,
            remove_payload_json("setting", "key", key),
        )
    }

    fn remove_driver(&self, name: &str) -> Result<u64, SysError> {
        self.remove_system_row(
            SysNode::Drivers,
            "DELETE FROM sys_drivers WHERE name = ?1",
            name,
            remove_payload_json("driver", "name", name),
        )
    }
}

impl SystemDbBackend {
    /// Delete one System-DB row keyed by a single value, in one transaction with the t76 audit +
    /// `ddl_event` (administration observes itself). The provisioning reconcile REMOVE path for
    /// `/sys/{policies,settings,drivers}` — idempotent (a missing row affects 0 rows). Returns the
    /// affected count.
    fn remove_system_row(
        &self,
        node: SysNode,
        sql: &str,
        key: &str,
        payload_json: String,
    ) -> Result<u64, SysError> {
        let conn = self.system.lock().map_err(poisoned)?;
        let tx = conn.unchecked_transaction().map_err(backend)?;
        let affected = tx.execute(sql, rusqlite::params![key]).map_err(backend)? as u64;
        self.audit_and_event_tx(&tx, node, "REMOVE", payload_json)?;
        tx.commit().map_err(backend)?;
        Ok(affected)
    }

    /// Append the t76 audit row + the `ddl_event` for a `/sys/<node>` mutation inside the caller's
    /// transaction (metadata only — verb + node path, never row data). Shared by the reconcile
    /// UPDATE/REMOVE writers so a torn write can never leave the mutation un-audited.
    fn audit_and_event_tx(
        &self,
        tx: &rusqlite::Transaction<'_>,
        node: SysNode,
        verb: &str,
        payload_json: String,
    ) -> Result<(), SysError> {
        let ts = now_rfc3339();
        append_audit_tx(
            tx,
            AuditEvent {
                actor: ACTOR_CLI.to_string(),
                connection: "default".to_string(),
                verb: verb.to_string(),
                path: node.path(),
                committed: true,
                ts: ts.clone(),
            },
        )
        .map_err(backend)?;
        append_ddl_event_tx(tx, ddl_event(&node.path(), verb, payload_json, ts))
            .map_err(backend)?;
        Ok(())
    }

    /// Resolve a team's recorded **billing plan** (t67) from `/sys/billing` — the authority the
    /// ENTITLEMENT GATE reads. **Fail-closed (default-deny toward the free floor):** a missing row, a
    /// read error, or a garbled tier/status all resolve to the FREE plan
    /// ([`qfs_identity::BillingPlan::free`]) — an unpaid/unknown team never gains paid entitlements.
    /// The labels are decoded through the pure model, so a corrupted value can only LOSE entitlements.
    #[must_use]
    pub fn get_billing_plan(&self, team_id: &str) -> qfs_identity::BillingPlan {
        let Ok(conn) = self.system.lock() else {
            return qfs_identity::BillingPlan::free();
        };
        conn.query_row(
            "SELECT tier, status FROM billing_subscriptions WHERE team_id = ?1",
            rusqlite::params![team_id],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
        )
        .optional()
        .ok()
        .flatten()
        .map_or_else(qfs_identity::BillingPlan::free, |(tier, status)| {
            qfs_identity::BillingPlan::decode(&tier, &status)
        })
    }

    /// Apply a provider subscription event to a team's plan, **idempotently** (t67, the at-least-once
    /// webhook update path). The provider's `event_id` is inserted into the `billing_events` dedup
    /// ledger inside the SAME transaction as the upsert: a REPLAYED event (same id) is a no-op (the
    /// plan is not double-applied), so a re-delivered "subscription cancelled" cannot fight a later
    /// "renewed". Returns `true` when this call applied the event, `false` when it was a deduped
    /// replay. The plan row + the audit row + the ledger row commit atomically.
    ///
    /// This is the SEAM the binary's webhook handler (`crate::billing`) calls after
    /// `qfs-watchtower` has HMAC-verified the request — no payment secret crosses into this method
    /// (only the already-verified, decoded plan labels).
    ///
    /// # Errors
    /// [`SysError::Backend`] on an I/O failure.
    pub fn apply_provider_event(
        &self,
        event_id: &str,
        team_id: &str,
        tier: &str,
        status: &str,
        current_period_end: Option<&str>,
    ) -> Result<bool, SysError> {
        let conn = self.system.lock().map_err(poisoned)?;
        let tx = conn.unchecked_transaction().map_err(backend)?;
        // Dedup FIRST (the ledger PK is the provider event id): INSERT OR IGNORE — a 0-row result
        // means this exact event was already applied, so we apply NOTHING and report a deduped replay.
        let inserted = tx
            .execute(
                "INSERT OR IGNORE INTO billing_events (event_id, team_id) VALUES (?1, ?2)",
                rusqlite::params![event_id, team_id],
            )
            .map_err(backend)?;
        if inserted == 0 {
            // A replay: roll back (nothing changed) and report "not applied".
            tx.rollback().map_err(backend)?;
            return Ok(false);
        }
        upsert_billing_tx(&tx, team_id, tier, status, current_period_end).map_err(backend)?;
        let ts = now_rfc3339();
        append_audit_tx(
            &tx,
            AuditEvent {
                actor: ACTOR_PROVIDER.to_string(),
                connection: "default".to_string(),
                verb: "INSERT".to_string(),
                path: SysNode::Billing.path(),
                committed: true,
                ts: ts.clone(),
            },
        )
        .map_err(backend)?;
        append_ddl_event_tx(
            &tx,
            DdlEvent {
                tx_id: event_id.to_string(),
                actor: ACTOR_PROVIDER.to_string(),
                ts,
                target_path: SysNode::Billing.path().to_string(),
                verb: "UPSERT".to_string(),
                source_text: None,
                payload_json: billing_event_payload_json(team_id, tier, status, current_period_end),
            },
        )
        .map_err(backend)?;
        tx.commit().map_err(backend)?;
        Ok(true)
    }

    /// Read a single setting `value` by `key` from the System DB (best-effort: a missing row or a
    /// read error yields `None`). The READ side of the `/sys/settings` round-trip.
    #[must_use]
    pub fn get_setting(&self, key: &str) -> Option<String> {
        let conn = self.system.lock().ok()?;
        conn.query_row(
            "SELECT value FROM sys_settings WHERE key = ?1",
            rusqlite::params![key],
            |r| r.get::<_, String>(0),
        )
        .optional()
        .ok()
        .flatten()
    }

    /// Resolve the active selectable **safety mode** (t59) from the persisted `/sys/settings` row,
    /// **failing safe** to the default ([`SafetyMode::AutonomousInPolicy`](qfs_core::SafetyMode)) on
    /// a missing/garbled value (decision: an unset/misconfigured mode falls to the safest sensible
    /// default — irreversible needs approval — never to Policy-only-auto).
    #[must_use]
    pub fn resolve_safety_mode(&self) -> qfs_core::SafetyMode {
        self.get_setting(SAFETY_MODE_KEY)
            .map_or_else(qfs_core::SafetyMode::default, |v| {
                qfs_core::SafetyMode::from_label_or_default(&v)
            })
    }
}

/// The `/sys/settings` key under which the selectable safety mode (t59) is stored.
pub const SAFETY_MODE_KEY: &str = "safety_mode";

/// Resolve the deployment's active selectable **safety mode** (t59) for the binary's commit faces
/// (the CLI one-shot run context + the serve MCP/dashboard engine). Precedence, most-authoritative
/// first, **failing safe** at every step (an unset/garbled source never opens the floor):
///   1. the persisted `/sys/settings` `safety_mode` row (the operator's stored choice, surfaced as
///      data and set via `INSERT INTO /sys/settings`);
///   2. the `QFS_SAFETY_MODE` env (the unattended / no-System-DB config path — CI, agents);
///   3. the safe default [`SafetyMode::AutonomousInPolicy`](qfs_core::SafetyMode).
#[must_use]
pub fn resolve_active_safety_mode() -> qfs_core::SafetyMode {
    if let Some(backend) = SystemDbBackend::open_default() {
        if let Some(value) = backend.get_setting(SAFETY_MODE_KEY) {
            return qfs_core::SafetyMode::from_label_or_default(&value);
        }
    }
    match std::env::var("QFS_SAFETY_MODE") {
        Ok(value) => qfs_core::SafetyMode::from_label_or_default(&value),
        Err(_) => qfs_core::SafetyMode::default(),
    }
}

impl SystemDbBackend {
    /// Run a read over the System DB connection, mapping each row with `f`.
    fn scan_system<F>(&self, sql: &str, f: F) -> Result<Vec<Row>, SysError>
    where
        F: Fn(&rusqlite::Row<'_>) -> rusqlite::Result<Row>,
    {
        let conn = self.system.lock().map_err(poisoned)?;
        query_rows(&conn, sql, f)
    }

    /// Run a read over the Project DB connection registry, mapping each row with `f`. Returns an
    /// empty result when no Project DB is wired (best-effort).
    fn scan_project<F>(&self, sql: &str, f: F) -> Result<Vec<Row>, SysError>
    where
        F: Fn(&rusqlite::Row<'_>) -> rusqlite::Result<Row>,
    {
        let Some(project) = &self.project else {
            return Ok(Vec::new());
        };
        let conn = project.lock().map_err(poisoned)?;
        query_rows(&conn, sql, f)
    }
}

/// Run `sql` and collect each row through `f` into owned [`Row`]s (no vendor type escapes).
fn query_rows<F>(conn: &Connection, sql: &str, f: F) -> Result<Vec<Row>, SysError>
where
    F: Fn(&rusqlite::Row<'_>) -> rusqlite::Result<Row>,
{
    let mut stmt = conn.prepare(sql).map_err(backend)?;
    let mapped = stmt.query_map([], |r| f(r)).map_err(backend)?;
    let mut out = Vec::new();
    for r in mapped {
        out.push(r.map_err(backend)?);
    }
    Ok(out)
}

/// Append one event to the t76 hash chain INSIDE an existing transaction (the policy write's
/// transaction): read the head, compute the next chained event, persist the head, append the
/// bounded tail row, and trim. Mirrors `crate::audit::append_event`, but composed into the caller's
/// transaction so the policy row + its audit row commit atomically.
pub(crate) fn append_audit_tx(
    tx: &rusqlite::Transaction<'_>,
    event: AuditEvent,
) -> rusqlite::Result<()> {
    let head: Option<ChainHead> = tx
        .query_row(
            "SELECT seq, content_hash, prev_hash FROM audit_chain_head WHERE id = 1",
            [],
            |r| {
                Ok(ChainHead {
                    seq: r.get::<_, i64>(0)? as u64,
                    content_hash: r.get(1)?,
                    prev_hash: r.get(2)?,
                })
            },
        )
        .optional()?;

    let (seq, prev_hash) = match head {
        Some(h) => (h.seq + 1, h.hash()),
        None => (1, GENESIS_PREV_HASH.to_string()),
    };
    let chained = event.chain(seq, prev_hash);
    let new_head = chained.head();

    tx.execute(
        "INSERT INTO audit_chain_head (id, seq, content_hash, prev_hash) VALUES (1, ?1, ?2, ?3) \
         ON CONFLICT(id) DO UPDATE SET seq = excluded.seq, content_hash = excluded.content_hash, \
         prev_hash = excluded.prev_hash",
        rusqlite::params![
            new_head.seq as i64,
            new_head.content_hash,
            new_head.prev_hash
        ],
    )?;
    tx.execute(
        "INSERT INTO audit_tail \
         (seq, actor, connection, verb, path, committed, ts, content_hash, prev_hash, hash) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        rusqlite::params![
            chained.seq as i64,
            chained.event.actor,
            chained.event.connection,
            chained.event.verb,
            chained.event.path,
            i64::from(chained.event.committed),
            chained.event.ts,
            chained.content_hash,
            chained.prev_hash,
            chained.hash,
        ],
    )?;
    tx.execute(
        "DELETE FROM audit_tail WHERE seq <= ?1 - ?2",
        rusqlite::params![chained.seq as i64, TAIL_CAP],
    )?;
    Ok(())
}

/// Append one replayable DDL/config event INSIDE an existing System DB transaction. Unlike
/// `/sys/audit`, this stores normalized, secret-free payload JSON for state reconstruction.
pub(crate) fn append_ddl_event_tx(
    tx: &rusqlite::Transaction<'_>,
    event: DdlEvent,
) -> rusqlite::Result<()> {
    let head: Option<(u64, String)> = tx
        .query_row(
            "SELECT seq, hash FROM sys_ddl_events ORDER BY seq DESC LIMIT 1",
            [],
            |r| Ok((r.get::<_, i64>(0)? as u64, r.get::<_, String>(1)?)),
        )
        .optional()?;
    let (seq, prev_hash) = match head {
        Some((seq, hash)) => (seq + 1, hash),
        None => (1, DDL_GENESIS_PREV_HASH.to_string()),
    };
    let chained = event.chain(seq, prev_hash);
    tx.execute(
        "INSERT INTO sys_ddl_events \
         (seq, tx_id, actor, ts, target_path, verb, source_text, payload_json, content_hash, \
          prev_hash, hash) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        rusqlite::params![
            chained.seq as i64,
            chained.event.tx_id,
            chained.event.actor,
            chained.event.ts,
            chained.event.target_path,
            chained.event.verb,
            chained.event.source_text,
            chained.event.payload_json,
            chained.content_hash,
            chained.prev_hash,
            chained.hash,
        ],
    )?;
    Ok(())
}

/// Append the t76 audit row + `ddl_event` for a `/sys/paths` mutation INSIDE the caller's
/// System-DB transaction — the binding row and its ledger entries commit atomically (ticket
/// 20260716143641; the audit `path` records the defined path under the `/sys/paths` node).
/// Shared by the runtime `/sys/paths` backend, the `qfs connect`/`disconnect` CLI, and restore's
/// binding replay, so every config writer lands the same ledger shape. Metadata + references
/// only — never a secret value.
pub(crate) fn ledgered_paths_write_tx(
    tx: &rusqlite::Transaction<'_>,
    verb: &str,
    user_path: &str,
    payload_json: String,
) -> rusqlite::Result<()> {
    let ts = now_rfc3339();
    append_audit_tx(
        tx,
        AuditEvent {
            actor: ACTOR_CLI.to_string(),
            connection: "default".to_string(),
            verb: verb.to_string(),
            path: format!("{}{}", SysNode::Paths.path(), user_path),
            committed: true,
            ts: ts.clone(),
        },
    )?;
    append_ddl_event_tx(
        tx,
        ddl_event(&SysNode::Paths.path(), verb, payload_json, ts),
    )
}

/// Append the t76 audit row + `ddl_event` for a `/sys/accounts` mutation INSIDE the caller's
/// System-DB transaction (ticket 20260716143641) — shared by `qfs account add/remove` and the
/// `CREATE ACCOUNT` / `REMOVE /sys/accounts/…` statement path (one writer, one ledger shape).
/// The payload carries selectors only (provider / account label / app), never a token.
pub(crate) fn ledgered_accounts_write_tx(
    tx: &rusqlite::Transaction<'_>,
    verb: &str,
    provider: &str,
    account: &str,
    payload_json: String,
) -> rusqlite::Result<()> {
    let ts = now_rfc3339();
    append_audit_tx(
        tx,
        AuditEvent {
            actor: ACTOR_CLI.to_string(),
            connection: "default".to_string(),
            verb: verb.to_string(),
            path: format!("{}/{provider}/{account}", SysNode::Accounts.path()),
            committed: true,
            ts: ts.clone(),
        },
    )?;
    append_ddl_event_tx(
        tx,
        ddl_event(&SysNode::Accounts.path(), verb, payload_json, ts),
    )
}

/// The secret-free `ddl_event` payload for a full-connect binding write — selectors, a locator
/// hint, and the secret REFERENCE (`env:`/`vault:` — never a value), mirroring what `qfs dump`
/// emits for the same row.
#[allow(clippy::too_many_arguments)]
pub(crate) fn binding_payload_json(
    path: &str,
    driver: Option<&str>,
    at: Option<&str>,
    secret_ref: Option<&str>,
    alias_of: Option<&str>,
    host: Option<&str>,
    account: Option<&str>,
    app: Option<&str>,
) -> String {
    let mut map = JsonMap::new();
    map.insert("kind".to_string(), JsonValue::String("binding".to_string()));
    map.insert("path".to_string(), JsonValue::String(path.to_string()));
    let opt = |v: Option<&str>| v.map_or(JsonValue::Null, |s| JsonValue::String(s.to_string()));
    map.insert("driver".to_string(), opt(driver));
    map.insert("at".to_string(), opt(at));
    map.insert("secret_ref".to_string(), opt(secret_ref));
    map.insert("alias_of".to_string(), opt(alias_of));
    map.insert("host".to_string(), opt(host));
    map.insert("account".to_string(), opt(account));
    map.insert("app".to_string(), opt(app));
    JsonValue::Object(map).to_string()
}

/// The secret-free `ddl_event` payload for an account consent write — provider + account label +
/// optional app label (the subject rides in the consent row itself; no token exists to leak).
pub(crate) fn account_payload_json(provider: &str, account: &str, app: Option<&str>) -> String {
    let mut map = JsonMap::new();
    map.insert("kind".to_string(), JsonValue::String("account".to_string()));
    map.insert(
        "provider".to_string(),
        JsonValue::String(provider.to_string()),
    );
    map.insert(
        "account".to_string(),
        JsonValue::String(account.to_string()),
    );
    map.insert(
        "app".to_string(),
        app.map_or(JsonValue::Null, |s| JsonValue::String(s.to_string())),
    );
    JsonValue::Object(map).to_string()
}

pub(crate) fn ddl_event(
    target_path: &str,
    verb: &str,
    payload_json: String,
    ts: String,
) -> DdlEvent {
    DdlEvent {
        tx_id: format!("{ACTOR_CLI}:{verb}:{target_path}:{ts}"),
        actor: ACTOR_CLI.to_string(),
        ts,
        target_path: target_path.to_string(),
        verb: verb.to_string(),
        source_text: None,
        payload_json,
    }
}

fn row_payload_json(kind: &str, row: &RowBatch) -> String {
    let mut map = JsonMap::new();
    map.insert("kind".to_string(), JsonValue::String(kind.to_string()));
    if let Some(first) = row.rows.first() {
        for (idx, column) in row.schema.columns.iter().enumerate() {
            let value = first.values.get(idx).unwrap_or(&Value::Null);
            map.insert(column.name.clone(), replay_json_value(&column.name, value));
        }
    }
    JsonValue::Object(map).to_string()
}

/// The `ddl_event` payload for a reconcile REMOVE of a single-keyed `/sys` row — the row's key
/// only (a delete carries no other data), tagged by kind. Secret-free by construction.
pub(crate) fn remove_payload_json(kind: &str, key_col: &str, key: &str) -> String {
    let mut map = JsonMap::new();
    map.insert("kind".to_string(), JsonValue::String(kind.to_string()));
    map.insert(key_col.to_string(), JsonValue::String(key.to_string()));
    JsonValue::Object(map).to_string()
}

fn setting_payload_json(key: &str, value: &str) -> String {
    let mut map = JsonMap::new();
    map.insert("kind".to_string(), JsonValue::String("setting".to_string()));
    map.insert("key".to_string(), JsonValue::String(key.to_string()));
    map.insert(
        "value".to_string(),
        if secretish_key(key) {
            JsonValue::String("<redacted>".to_string())
        } else {
            JsonValue::String(value.to_string())
        },
    );
    JsonValue::Object(map).to_string()
}

fn billing_event_payload_json(
    team_id: &str,
    tier: &str,
    status: &str,
    current_period_end: Option<&str>,
) -> String {
    let mut map = JsonMap::new();
    map.insert("kind".to_string(), JsonValue::String("billing".to_string()));
    map.insert(
        "team_id".to_string(),
        JsonValue::String(team_id.to_string()),
    );
    map.insert("tier".to_string(), JsonValue::String(tier.to_string()));
    map.insert("status".to_string(), JsonValue::String(status.to_string()));
    map.insert(
        "current_period_end".to_string(),
        current_period_end.map_or(JsonValue::Null, |s| JsonValue::String(s.to_string())),
    );
    JsonValue::Object(map).to_string()
}

fn replay_json_value(column: &str, value: &Value) -> JsonValue {
    if secretish_key(column) {
        return JsonValue::String("<redacted>".to_string());
    }
    let mut json = match value {
        Value::Null => JsonValue::Null,
        Value::Bool(v) => JsonValue::Bool(*v),
        Value::Int(v) | Value::Timestamp(v) => JsonValue::Number((*v).into()),
        Value::Float(v) => {
            serde_json::Number::from_f64(*v).map_or(JsonValue::Null, JsonValue::Number)
        }
        Value::Text(v) => JsonValue::String(v.clone()),
        Value::Bytes(_) => JsonValue::String("<bytes>".to_string()),
        Value::Json(v) => v.clone(),
        Value::Struct(_) | Value::Array(_) => {
            serde_json::to_value(value).unwrap_or(JsonValue::Null)
        }
        _ => JsonValue::Null,
    };
    redact_secret_keys(&mut json);
    json
}

fn redact_secret_keys(value: &mut JsonValue) {
    match value {
        JsonValue::Object(map) => {
            for (key, value) in map.iter_mut() {
                if secretish_key(key) {
                    *value = JsonValue::String("<redacted>".to_string());
                } else {
                    redact_secret_keys(value);
                }
            }
        }
        JsonValue::Array(items) => {
            for item in items {
                redact_secret_keys(item);
            }
        }
        _ => {}
    }
}

fn secretish_key(key: &str) -> bool {
    let lower = key.to_ascii_lowercase();
    lower != "secret_ref"
        && (lower.contains("secret")
            || lower.contains("token")
            || lower.contains("password")
            || lower.contains("passphrase")
            || lower.contains("ciphertext")
            || lower.contains("nonce"))
}

/// Upsert one team's billing plan row (t67) INSIDE an existing transaction — an **upsert on
/// `team_id`** (a team has one current plan, so re-recording it replaces the row and bumps
/// `updated_at`). Shared by the gated `/sys/billing` write (`set_billing`) and the provider-webhook
/// apply path (`apply_provider_event`) so both land identical plan state. Metadata only — never a
/// payment secret.
fn upsert_billing_tx(
    tx: &rusqlite::Transaction<'_>,
    team_id: &str,
    tier: &str,
    status: &str,
    current_period_end: Option<&str>,
) -> rusqlite::Result<()> {
    tx.execute(
        "INSERT INTO billing_subscriptions (team_id, tier, status, current_period_end, updated_at) \
         VALUES (?1, ?2, ?3, ?4, strftime('%Y-%m-%dT%H:%M:%SZ','now')) \
         ON CONFLICT(team_id) DO UPDATE SET tier = excluded.tier, status = excluded.status, \
         current_period_end = excluded.current_period_end, updated_at = excluded.updated_at",
        rusqlite::params![team_id, tier, status, current_period_end],
    )?;
    Ok(())
}

/// The current UTC time as an RFC3339 string for an audit event's `ts`. A format failure on an
/// impossible date falls back to the Unix epoch rather than panicking (the audit never breaks the
/// operation).
pub(crate) fn now_rfc3339() -> String {
    use time::format_description::well_known::Rfc3339;
    time::OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string())
}

// --- small typed column getters (keep the row builders legible) ----------------------------------

fn int(r: &rusqlite::Row<'_>, idx: usize) -> rusqlite::Result<Value> {
    Ok(Value::Int(r.get::<_, i64>(idx)?))
}
fn text(r: &rusqlite::Row<'_>, idx: usize) -> rusqlite::Result<Value> {
    Ok(Value::Text(r.get::<_, String>(idx)?))
}
pub(crate) fn nullable_text(r: &rusqlite::Row<'_>, idx: usize) -> rusqlite::Result<Value> {
    Ok(match r.get::<_, Option<String>>(idx)? {
        Some(s) => Value::Text(s),
        None => Value::Null,
    })
}

/// Read a required text column from the single-row write payload by name (the policy `name`).
fn required_text(row: &RowBatch, col: &str) -> Result<String, SysError> {
    match cell(row, col) {
        Some(Value::Text(s)) if !s.is_empty() => Ok(s.clone()),
        _ => Err(SysError::MalformedEffect {
            reason: format!("INSERT INTO /sys/policies requires a non-empty `{col}`"),
        }),
    }
}

/// Read an optional text column (absent/null/empty → `None`).
pub(crate) fn optional_text(row: &RowBatch, col: &str) -> Option<String> {
    match cell(row, col) {
        Some(Value::Text(s)) if !s.is_empty() => Some(s.clone()),
        _ => None,
    }
}

/// Read a boolean effect-arg cell (the §13 `irreversible` flag). Absent / NULL / non-bool → `false`.
fn optional_bool(row: &RowBatch, col: &str) -> bool {
    matches!(cell(row, col), Some(Value::Bool(true)))
}

/// The single write payload's value for `col`, if the batch carries that column.
fn cell<'a>(row: &'a RowBatch, col: &str) -> Option<&'a Value> {
    let idx = row
        .schema
        .columns
        .iter()
        .position(|c| c.name.as_str() == col)?;
    row.rows.first().and_then(|r| r.values.get(idx))
}

fn backend(e: rusqlite::Error) -> SysError {
    SysError::Backend(e.to_string())
}

/// Map a `path_binding` I/O error (t100020). A foreign-key violation means an ALIAS named a target
/// defined path that does not exist — a fail-closed rejection with a clear, secret-free reason
/// (never a fake mount); everything else is a plain backend error.
fn binding_err(e: rusqlite::Error) -> SysError {
    if let rusqlite::Error::SqliteFailure(err, _) = &e {
        if err.code == rusqlite::ErrorCode::ConstraintViolation {
            return SysError::MalformedEffect {
                reason: "the alias target is not a defined path — CONNECT it first".to_string(),
            };
        }
    }
    SysError::Backend(e.to_string())
}
fn poisoned<T>(_: std::sync::PoisonError<T>) -> SysError {
    SysError::Backend("poisoned system db connection mutex".to_string())
}

/// The async read facet (the `/sys` counterpart of `shell.rs`'s `LocalReadDriver`): adapts the
/// synchronous [`SysBackend::scan`] to qfs-exec's [`ReadDriver`] seam. Lives in the binary because
/// `ReadDriver` is a qfs-exec type and the driver crate must stay off qfs-exec (dep direction).
pub struct SysReadDriver {
    backend: Arc<dyn SysBackend>,
}

impl SysReadDriver {
    /// Build the read adapter over an injected backend.
    #[must_use]
    pub fn new(backend: Arc<dyn SysBackend>) -> Self {
        Self { backend }
    }
}

#[async_trait::async_trait]
impl ReadDriver for SysReadDriver {
    async fn scan(&self, scan: &ScanNode, ctx: &RequestContext) -> Result<RowBatch, CfsError> {
        let node = node_for_path(&scan.path).ok_or_else(|| CfsError::InvalidPath {
            path: scan.path.clone(),
            reason: "not a /sys admin path",
        })?;
        // `/sys/whoami` is resolved from the REQUEST PRINCIPAL, not the backend: the scan seam
        // carries `ctx` precisely so this face can read *who is asking*. Credential-free
        // (signed_in + user), and the not-signed-in answer is a first-class row.
        if matches!(node, SysNode::Whoami) {
            return Ok(whoami_batch(ctx));
        }
        self.backend.scan(node).map_err(|e| CfsError::InvalidPath {
            path: scan.path.clone(),
            reason: sys_error_reason(&e),
        })
    }
}

/// The `/sys/whoami` row, resolved from the request principal (NOT the backend — it carries no
/// request context). Credential-free by construction: a `signed_in` flag + the acting user id
/// (`NULL` when anonymous), matching `sys_node_schema(SysNode::Whoami)`. The not-signed-in answer
/// is a first-class row, never an error and never a silent fallback to a sole user.
fn whoami_batch(ctx: &RequestContext) -> RowBatch {
    let (signed_in, user) = match ctx.user() {
        Some(id) => (true, Value::Text(id.to_string())),
        None => (false, Value::Null),
    };
    RowBatch::new(
        sys_node_schema(SysNode::Whoami),
        vec![Row::new(vec![Value::Bool(signed_in), user])],
    )
}

/// A minimal, always-available [`SysBackend`] for the serve face when NO System DB resolves: it
/// backs the credential-free `/sys/whoami` facet (which [`SysReadDriver::scan`] answers from the
/// request principal, NEVER the backend) so the not-signed-in answer stays a first-class row even
/// pre-init. Every backend-touching node — and every write — is fail-closed (`/sys` over serve is
/// read-only anyway), since without a System DB there is nothing to read or mutate.
#[derive(Debug, Default)]
pub struct AnonymousSysBackend;

impl AnonymousSysBackend {
    /// A fresh whoami-only backend.
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

/// The fail-closed error every backend-touching `AnonymousSysBackend` method returns (whoami never
/// reaches the backend, so this is only hit by a real `/sys/<node>` read or a write over serve).
fn no_system_db() -> SysError {
    SysError::Backend("no system database configured".to_string())
}

impl SysBackend for AnonymousSysBackend {
    fn scan(&self, _node: SysNode) -> Result<RowBatch, SysError> {
        Err(no_system_db())
    }
    fn insert_policy(&self, _row: &RowBatch) -> Result<u64, SysError> {
        Err(no_system_db())
    }
    fn set_setting(&self, _row: &RowBatch) -> Result<u64, SysError> {
        Err(no_system_db())
    }
    fn set_billing(&self, _row: &RowBatch) -> Result<u64, SysError> {
        Err(no_system_db())
    }
    fn upsert_binding(&self, _row: &RowBatch) -> Result<u64, SysError> {
        Err(no_system_db())
    }
    fn remove_binding(&self, _path: &str) -> Result<u64, SysError> {
        Err(no_system_db())
    }
    fn update_policy(&self, _row: &RowBatch) -> Result<u64, SysError> {
        Err(no_system_db())
    }
    fn remove_policy(&self, _name: &str) -> Result<u64, SysError> {
        Err(no_system_db())
    }
    fn remove_setting(&self, _key: &str) -> Result<u64, SysError> {
        Err(no_system_db())
    }
    fn remove_driver(&self, _name: &str) -> Result<u64, SysError> {
        Err(no_system_db())
    }
    fn insert_driver(&self, _row: &RowBatch) -> Result<u64, SysError> {
        Err(no_system_db())
    }
    fn record_account(&self, _row: &RowBatch) -> Result<u64, SysError> {
        Err(no_system_db())
    }
    fn remove_account(&self, _provider: &str, _account: &str) -> Result<u64, SysError> {
        Err(no_system_db())
    }
}

/// A stable, secret-free reason code for a `/sys` read failure (the executor maps it to its kind).
fn sys_error_reason(e: &SysError) -> &'static str {
    match e {
        SysError::UnknownNode { .. } => "unknown_sys_node",
        SysError::AppendOnly { .. } => "append_only",
        SysError::MalformedEffect { .. } => "malformed_effect",
        SysError::Backend(_) => "read_failed",
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;
    use qfs_pushdown::{PushedQuery, ScanNode};
    use qfs_store::audit::{verify_chain, GENESIS_PREV_HASH};
    use qfs_store::{FileSource, ProjectDb, SystemDb};
    use qfs_types::{Column, ColumnType, Schema};
    use tempfile::TempDir;

    /// A backend over fresh FILE-backed System + Project DBs (migrated), pre-seeded with fixture
    /// rows. File-backed (not in-memory) so the audit chain can be re-read through a second
    /// `SystemDb` view on the same path (the backend OWNS its connection). The `TempDir` is
    /// returned so the files outlive the test.
    fn fixture_backend() -> (TempDir, SystemDbBackend) {
        let dir = TempDir::new().unwrap();
        let sys_path = dir.path().join("system.db");
        let proj_path = dir.path().join("project.db");

        let sys = SystemDb::open(&FileSource::new(&sys_path))
            .unwrap()
            .into_db()
            .into_connection();
        sys.execute("INSERT INTO projects (slug) VALUES ('alpha')", [])
            .unwrap();
        sys.execute(
            "INSERT INTO users (primary_email, status) VALUES ('a@qmu.jp', 'active')",
            [],
        )
        .unwrap();

        let proj = ProjectDb::open(&FileSource::new(&proj_path))
            .unwrap()
            .into_db()
            .into_connection();
        // A connection-registry row WITH secret material — the scan must never surface it.
        proj.execute(
            "INSERT INTO secret_store (driver, connection, nonce, ciphertext) \
             VALUES ('github', 'work', x'00', ?1)",
            rusqlite::params![b"SUPER-SECRET-TOKEN".to_vec()],
        )
        .unwrap();

        (dir, SystemDbBackend::new(sys, Some(proj)))
    }

    fn texts(batch: &RowBatch, col: &str) -> Vec<String> {
        let idx = batch
            .schema
            .columns
            .iter()
            .position(|c| c.name.as_str() == col)
            .expect("column present");
        batch
            .rows
            .iter()
            .filter_map(|r| match &r.values[idx] {
                Value::Text(s) => Some(s.clone()),
                _ => None,
            })
            .collect()
    }

    #[derive(Debug)]
    struct RecordedDdlEvent {
        target_path: String,
        verb: String,
        payload_json: String,
    }

    fn ddl_events(backend: &SystemDbBackend) -> Vec<RecordedDdlEvent> {
        let conn = backend.system.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT target_path, verb, payload_json FROM sys_ddl_events ORDER BY seq")
            .unwrap();
        stmt.query_map([], |r| {
            Ok(RecordedDdlEvent {
                target_path: r.get(0)?,
                verb: r.get(1)?,
                payload_json: r.get(2)?,
            })
        })
        .unwrap()
        .map(Result::unwrap)
        .collect()
    }

    fn policy_payload() -> RowBatch {
        let schema = Schema::new(vec![
            Column::new("name", ColumnType::Text, false),
            Column::new("allow", ColumnType::Text, true),
            Column::new("target", ColumnType::Text, true),
        ]);
        RowBatch::new(
            schema,
            vec![Row::new(vec![
                Value::Text("analysts".into()),
                Value::Text("SELECT".into()),
                Value::Text("/sql/*".into()),
            ])],
        )
    }

    /// The §13 declared-driver row a `CREATE DRIVER chatwork AT … AUTH HEADER 'x-chatworktoken'`
    /// desugars to — the same column shape the parser emits. The `auth` descriptor names the header,
    /// never a token.
    fn driver_payload() -> RowBatch {
        let schema = Schema::new(vec![
            Column::new("kind", ColumnType::Text, false),
            Column::new("name", ColumnType::Text, false),
            Column::new("base_url", ColumnType::Text, true),
            Column::new("auth", ColumnType::Text, true),
            Column::new("pagination", ColumnType::Text, true),
            Column::new("of_type", ColumnType::Text, true),
            Column::new("verb", ColumnType::Text, true),
            Column::new("body", ColumnType::Text, true),
            Column::new("irreversible", ColumnType::Bool, false),
        ]);
        RowBatch::new(
            schema,
            vec![Row::new(vec![
                Value::Text("driver".into()),
                Value::Text("chatwork".into()),
                Value::Text("https://api.chatwork.com/v2".into()),
                Value::Text(r#"{"kind":"header","name":"x-chatworktoken"}"#.into()),
                Value::Null,
                Value::Null,
                Value::Null,
                Value::Null,
                Value::Bool(false),
            ])],
        )
    }

    fn type_payload(name: &str, body: &str) -> RowBatch {
        let schema = Schema::new(vec![
            Column::new("kind", ColumnType::Text, false),
            Column::new("name", ColumnType::Text, false),
            Column::new("base_url", ColumnType::Text, true),
            Column::new("auth", ColumnType::Text, true),
            Column::new("pagination", ColumnType::Text, true),
            Column::new("of_type", ColumnType::Text, true),
            Column::new("verb", ColumnType::Text, true),
            Column::new("body", ColumnType::Text, true),
            Column::new("irreversible", ColumnType::Bool, false),
        ]);
        RowBatch::new(
            schema,
            vec![Row::new(vec![
                Value::Text("type".into()),
                Value::Text(name.into()),
                Value::Null,
                Value::Null,
                Value::Null,
                Value::Null,
                Value::Null,
                Value::Text(body.into()),
                Value::Bool(false),
            ])],
        )
    }

    /// Ticket 20260716143641 QG1: `CONNECT` / `DISCONNECT` land the binding row, its t76 audit
    /// row, AND its replayable `ddl_event` in ONE System-DB transaction — the history the ledger
    /// could never hold while the registry lived in the Project DB. Written against the pre-move
    /// code first: the binding wrote only a best-effort post-commit AuditEvent and NO DdlEvent, so
    /// the ddl_events assertions below fail there (both-directions proof).
    #[test]
    fn connect_and_disconnect_are_ledger_transactional() {
        let (_d, backend) = fixture_backend();
        let schema = Schema::new(vec![
            Column::new("path", ColumnType::Text, false),
            Column::new("driver", ColumnType::Text, false),
            Column::new("secret_ref", ColumnType::Text, true),
            Column::new("account", ColumnType::Text, true),
        ]);
        let row = RowBatch::new(
            schema,
            vec![Row::new(vec![
                Value::Text("/chat".into()),
                Value::Text("chatwork".into()),
                Value::Text("vault:chatwork/work".into()),
                Value::Text("work".into()),
            ])],
        );
        backend.upsert_binding(&row).unwrap();

        // The registry row is read back from the SYSTEM DB (the re-homed home).
        let paths = backend.scan(SysNode::Paths).unwrap();
        assert_eq!(texts(&paths, "path"), vec!["/chat"]);
        // The ddl_event landed, with the secret REFERENCE (never a value) in the payload.
        let events = ddl_events(&backend);
        let insert = events
            .iter()
            .find(|e| e.target_path == "/sys/paths" && e.verb == "INSERT")
            .expect("CONNECT lands a replayable ddl_event");
        assert!(insert.payload_json.contains("/chat"), "{insert:?}");
        assert!(
            insert.payload_json.contains("vault:chatwork/work"),
            "the payload carries the reference form: {insert:?}"
        );
        // The audit row landed in the same store, naming the defined path under /sys/paths.
        let audit = backend.scan(SysNode::Audit).unwrap();
        assert!(
            texts(&audit, "path").iter().any(|p| p == "/sys/paths/chat"),
            "{audit:?}"
        );

        backend.remove_binding("/chat").unwrap();
        assert!(backend.scan(SysNode::Paths).unwrap().rows.is_empty());
        let events = ddl_events(&backend);
        assert!(
            events
                .iter()
                .any(|e| e.target_path == "/sys/paths" && e.verb == "REMOVE"),
            "DISCONNECT lands a replayable ddl_event: {events:?}"
        );
        // Secret-free discipline across every payload (the planted vault canary never appears).
        for e in &events {
            assert!(!e.payload_json.contains("SUPER-SECRET-TOKEN"), "{e:?}");
        }
    }

    #[test]
    fn scans_users_projects_audit_rows() {
        let (_d, backend) = fixture_backend();
        let users = backend.scan(SysNode::Users).unwrap();
        assert_eq!(texts(&users, "primary_email"), vec!["a@qmu.jp"]);
        let projects = backend.scan(SysNode::Projects).unwrap();
        assert_eq!(texts(&projects, "slug"), vec!["alpha"]);
        // /sys/audit scans (empty before any mutation, but the relation resolves + types).
        let audit = backend.scan(SysNode::Audit).unwrap();
        assert!(audit.rows.is_empty());
        assert!(audit.schema.column("committed").is_some());
    }

    #[test]
    fn connections_project_names_only_never_secrets() {
        let (_d, backend) = fixture_backend();
        let conns = backend.scan(SysNode::Connections).unwrap();
        // The registry row is present (driver + label + metadata).
        assert_eq!(texts(&conns, "driver"), vec!["github"]);
        assert_eq!(texts(&conns, "connection"), vec!["work"]);
        // The schema has NO secret column, and the secret value never appears anywhere in the batch.
        for forbidden in ["nonce", "ciphertext", "secret"] {
            assert!(
                conns.schema.column(forbidden).is_none(),
                "/sys/connections must not expose `{forbidden}`"
            );
        }
        let dump = format!("{conns:?}");
        assert!(
            !dump.contains("SUPER-SECRET-TOKEN") && !dump.contains("SUPER"),
            "no secret material may surface through /sys/connections"
        );
    }

    #[test]
    fn scan_sys_accounts_collapses_the_google_trio_and_exposes_no_token() {
        // 20260703040000: `/sys/accounts` reads the `connection_consent` ledger at USER-FACING
        // provider grain — Google's three driver rows (one consent, many drivers) collapse to one
        // `google` account per email; a cloud account is one row. Selectors + metadata only.
        let dir = TempDir::new().unwrap();
        let sys = SystemDb::open(&FileSource::new(dir.path().join("s.db")))
            .unwrap()
            .into_db()
            .into_connection();
        // The consent ledger lives in the SYSTEM DB (re-homed by 20260716143641).
        for (driver, scope) in [
            ("gmail", "gmail.modify gmail.compose"),
            ("gdrive", "drive"),
            ("ga", "analytics.readonly"),
        ] {
            sys.execute(
                "INSERT INTO connection_consent (driver, connection, subject, scope) \
                 VALUES (?1, 'you@example.com', 'op@example.com', ?2)",
                rusqlite::params![driver, scope],
            )
            .unwrap();
        }
        sys.execute(
            "INSERT INTO connection_consent (driver, connection, subject, scope) \
             VALUES ('github', 'work', 'op@example.com', 'repo')",
            [],
        )
        .unwrap();
        let backend = SystemDbBackend::new(sys, None);

        let accounts = backend.scan(SysNode::Accounts).unwrap();
        // The google trio collapses to ONE `google` account; github is one row → 2 total.
        assert_eq!(
            accounts.rows.len(),
            2,
            "google collapsed + github: {accounts:?}"
        );
        // ORDER BY provider, account → github before google (the user-facing provider, not gmail/…).
        assert_eq!(texts(&accounts, "provider"), vec!["github", "google"]);
        assert_eq!(texts(&accounts, "account"), vec!["work", "you@example.com"]);
        assert_eq!(
            texts(&accounts, "subject"),
            vec!["op@example.com", "op@example.com"]
        );
        // Structurally no token column (the redaction contract, §3.2).
        for forbidden in ["secret", "token", "ciphertext", "nonce", "refresh_token"] {
            assert!(
                accounts.schema.column(forbidden).is_none(),
                "/sys/accounts must not expose `{forbidden}`"
            );
        }
    }

    #[test]
    fn insert_driver_applies_scans_back_and_stays_credential_free() {
        // §13: a declared driver installs into /sys/drivers, reads back through the registry, and
        // appends an audit row — administration observes itself, exactly like the other /sys writes.
        let (_d, backend) = fixture_backend();
        let n = backend.insert_driver(&driver_payload()).unwrap();
        assert_eq!(n, 1);

        let drivers = backend.scan(SysNode::Drivers).unwrap();
        assert_eq!(texts(&drivers, "kind"), vec!["driver"]);
        assert_eq!(texts(&drivers, "name"), vec!["chatwork"]);
        assert_eq!(
            texts(&drivers, "base_url"),
            vec!["https://api.chatwork.com/v2"]
        );
        // The auth descriptor names the SCHEME + header name — never a token.
        let auth = texts(&drivers, "auth");
        assert!(auth[0].contains("header") && auth[0].contains("x-chatworktoken"));

        // Structural credential-free contract: no column a secret value could ride in.
        for forbidden in ["secret", "token", "ciphertext", "nonce"] {
            assert!(
                drivers.schema.column(forbidden).is_none(),
                "/sys/drivers must not expose `{forbidden}`"
            );
        }

        // Administration observed itself: exactly one INSERT audit row against /sys/drivers.
        let audit = backend.scan(SysNode::Audit).unwrap();
        assert_eq!(texts(&audit, "verb"), vec!["INSERT"]);
        assert_eq!(texts(&audit, "path"), vec!["/sys/drivers"]);

        let events = ddl_events(&backend);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].target_path, "/sys/drivers");
        assert_eq!(events[0].verb, "INSERT");
        assert!(events[0].payload_json.contains(r#""kind":"driver""#));
        assert!(events[0].payload_json.contains(r#""name":"chatwork""#));
        assert!(!events[0].payload_json.contains("SUPER-SECRET-TOKEN"));
    }

    /// Re-installing a declaration REPLACES its `(kind, name, verb)` row (owner ruling
    /// 2026-07-16): the superseded row is deleted in the SAME transaction as the new insert, both
    /// legs land in the DDL event log (administration observes itself — deletes included), and a
    /// first install emits no delete. A map differing only in verb is a different declaration and
    /// survives its sibling's re-install.
    #[test]
    fn insert_driver_replaces_the_same_key_row_and_records_both_legs() {
        let (_d, backend) = fixture_backend();

        let decl = |kind: &str, name: &str, base_url: &str, verb: Option<&str>, body: &str| {
            let schema = Schema::new(vec![
                Column::new("kind", ColumnType::Text, false),
                Column::new("name", ColumnType::Text, false),
                Column::new("base_url", ColumnType::Text, true),
                Column::new("auth", ColumnType::Text, true),
                Column::new("pagination", ColumnType::Text, true),
                Column::new("of_type", ColumnType::Text, true),
                Column::new("verb", ColumnType::Text, true),
                Column::new("body", ColumnType::Text, true),
                Column::new("irreversible", ColumnType::Bool, false),
            ]);
            RowBatch::new(
                schema,
                vec![Row::new(vec![
                    Value::Text(kind.into()),
                    Value::Text(name.into()),
                    Value::Text(base_url.into()),
                    Value::Null,
                    Value::Null,
                    Value::Null,
                    verb.map_or(Value::Null, |v| Value::Text(v.into())),
                    Value::Text(body.into()),
                    Value::Bool(false),
                ])],
            )
        };

        // Two installs of one driver key, then a verb-keyed pair of maps, then a re-install of
        // one map — six installs, two of them supersedes.
        for (kind, name, base_url, verb, body) in [
            ("driver", "demo", "https://first.example", None, ""),
            ("driver", "demo", "https://second.example", None, ""),
            ("map", "/demo/things", "", Some("INSERT"), "MAP-V1"),
            ("map", "/demo/things", "", Some("REMOVE"), "MAP-RM"),
            ("map", "/demo/things", "", Some("INSERT"), "MAP-V2"),
        ] {
            backend
                .insert_driver(&decl(kind, name, base_url, verb, body))
                .unwrap();
        }

        // Storage converged: one `demo` driver row carrying the SECOND locator; the two maps are
        // distinct keys, and the INSERT map carries its re-installed body.
        let drivers = backend.scan(SysNode::Drivers).unwrap();
        let names = texts(&drivers, "name");
        assert_eq!(
            names.iter().filter(|n| n.as_str() == "demo").count(),
            1,
            "a re-install must replace, not append: {names:?}"
        );
        let demo_idx = names.iter().position(|n| n == "demo").unwrap();
        assert_eq!(
            texts(&drivers, "base_url")[demo_idx],
            "https://second.example",
            "the newest locator is the one on disk"
        );
        assert_eq!(
            names
                .iter()
                .filter(|n| n.as_str() == "/demo/things")
                .count(),
            2,
            "maps differing in verb are distinct declarations: {names:?}"
        );
        let bodies = texts(&drivers, "body");
        assert!(
            bodies.iter().any(|b| b == "MAP-V2") && !bodies.iter().any(|b| b == "MAP-V1"),
            "the re-installed map body replaced its predecessor: {bodies:?}"
        );
        assert!(
            bodies.iter().any(|b| b == "MAP-RM"),
            "the other-verb map survived its sibling's re-install: {bodies:?}"
        );

        // Both legs of each supersede are in the DDL event log, in operation order; the two
        // first-time installs of each key emitted no delete.
        let events = ddl_events(&backend);
        let verbs: Vec<&str> = events.iter().map(|e| e.verb.as_str()).collect();
        assert_eq!(
            verbs,
            vec![
                "INSERT", // demo v1 (first install: no delete leg)
                "REMOVE", "INSERT", // demo v2 supersedes v1
                "INSERT", // map INSERT v1
                "INSERT", // map REMOVE (different key: no delete leg)
                "REMOVE", "INSERT", // map INSERT v2 supersedes v1
            ],
            "every supersede records its delete leg, first installs record none"
        );
        let supersede = &events[1];
        assert!(
            supersede.payload_json.contains(r#""name":"demo""#),
            "the delete leg names the superseded key: {}",
            supersede.payload_json
        );
    }

    #[test]
    fn insert_type_rejects_an_unknown_declared_column_type() {
        let (_d, backend) = fixture_backend();
        let body = serde_json::json!({
            "columns": [
                {
                    "name": "email",
                    "type": "/type/email",
                    "nullable": true,
                    "primary_key": false,
                    "unique": false
                }
            ],
            "where": null
        })
        .to_string();

        let err = backend
            .insert_driver(&type_payload("/type/customer", &body))
            .expect_err("unknown named type is a declare-time error");
        match err {
            SysError::MalformedEffect { reason } => {
                assert!(reason.contains("email"), "{reason}");
                assert!(reason.contains("/type/email"), "{reason}");
            }
            other => panic!("expected malformed effect, got {other:?}"),
        }
    }

    #[test]
    fn insert_policy_applies_and_appends_a_verifiable_audit_row() {
        let dir = TempDir::new().unwrap();
        let sys_path = dir.path().join("system.db");
        // Build a backend whose System DB is the file at `sys_path` (so we can re-read its audit
        // chain through a second SystemDb view below).
        let sys = SystemDb::open(&FileSource::new(&sys_path))
            .unwrap()
            .into_db()
            .into_connection();
        let backend = SystemDbBackend::new(sys, None);

        let n = backend.insert_policy(&policy_payload()).unwrap();
        assert_eq!(n, 1);

        // The grant row is readable back through /sys/policies.
        let policies = backend.scan(SysNode::Policies).unwrap();
        assert_eq!(texts(&policies, "name"), vec!["analysts"]);
        assert_eq!(texts(&policies, "allow"), vec!["SELECT"]);

        // The mutation appended exactly one audit row (administration observes itself) — visible
        // both through the /sys/audit relation and the durable t76 chain.
        let audit = backend.scan(SysNode::Audit).unwrap();
        assert_eq!(texts(&audit, "verb"), vec!["INSERT"]);
        assert_eq!(texts(&audit, "path"), vec!["/sys/policies"]);

        // Re-open the SAME System DB file to verify the chain via the pure verifier (the head +
        // tail were written transactionally with the policy row).
        let view = SystemDb::open(&FileSource::new(&sys_path)).unwrap();
        let tail = crate::audit::recent_tail(&view).unwrap();
        assert_eq!(tail.len(), 1);
        assert_eq!(verify_chain(&tail, GENESIS_PREV_HASH), None);

        let events = ddl_events(&backend);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].target_path, "/sys/policies");
        assert_eq!(events[0].verb, "INSERT");
        assert!(events[0].payload_json.contains(r#""name":"analysts""#));
        assert!(events[0].payload_json.contains(r#""allow":"SELECT""#));
    }

    fn settings_payload(key: &str, value: &str) -> RowBatch {
        let schema = Schema::new(vec![
            Column::new("key", ColumnType::Text, false),
            Column::new("value", ColumnType::Text, false),
        ]);
        RowBatch::new(
            schema,
            vec![Row::new(vec![
                Value::Text(key.into()),
                Value::Text(value.into()),
            ])],
        )
    }

    /// t59: the selectable safety mode round-trips through `/sys/settings` — `set_setting` (the
    /// `INSERT INTO /sys/settings` backend) persists it, `get_setting` / `resolve_safety_mode` read
    /// it back, the upsert REPLACES on a re-set, and an unset/garbled value resolves SAFE.
    #[test]
    fn safety_mode_round_trips_through_sys_settings() {
        let (_d, backend) = fixture_backend();

        // Unset ⇒ the safe default (autonomous-in-policy).
        assert_eq!(backend.get_setting(SAFETY_MODE_KEY), None);
        assert_eq!(
            backend.resolve_safety_mode(),
            qfs_core::SafetyMode::AutonomousInPolicy
        );

        // Set policy-only, read it back as both the raw value and the resolved mode.
        let n = backend
            .set_setting(&settings_payload(SAFETY_MODE_KEY, "policy-only"))
            .unwrap();
        assert_eq!(n, 1);
        assert_eq!(
            backend.get_setting(SAFETY_MODE_KEY).as_deref(),
            Some("policy-only")
        );
        assert_eq!(
            backend.resolve_safety_mode(),
            qfs_core::SafetyMode::PolicyOnly
        );
        // The mutation is visible as DATA through the /sys/settings relation (one-engine-three-faces).
        let settings = backend.scan(SysNode::Settings).unwrap();
        assert_eq!(texts(&settings, "key"), vec!["safety_mode"]);
        assert_eq!(texts(&settings, "value"), vec!["policy-only"]);

        // Upsert REPLACES (a setting is single-valued) — re-set to approve-everything.
        backend
            .set_setting(&settings_payload(SAFETY_MODE_KEY, "approve-everything"))
            .unwrap();
        assert_eq!(
            backend.resolve_safety_mode(),
            qfs_core::SafetyMode::ApproveEverything
        );
        let settings = backend.scan(SysNode::Settings).unwrap();
        assert_eq!(settings.rows.len(), 1, "upsert replaces, never duplicates");

        // A garbled persisted value fails SAFE to the default (never Policy-only-auto).
        backend
            .set_setting(&settings_payload(SAFETY_MODE_KEY, "nonsense"))
            .unwrap();
        assert_eq!(
            backend.resolve_safety_mode(),
            qfs_core::SafetyMode::AutonomousInPolicy
        );

        // Each set self-audited (administration observes itself): 3 INSERT audit rows on /sys/settings.
        let audit = backend.scan(SysNode::Audit).unwrap();
        assert_eq!(texts(&audit, "path"), vec!["/sys/settings"; 3]);

        let events = ddl_events(&backend);
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].target_path, "/sys/settings");
        assert_eq!(events[0].verb, "UPSERT");
        assert!(events[0].payload_json.contains(r#""key":"safety_mode""#));
        assert!(events[0].payload_json.contains(r#""value":"policy-only""#));
    }

    #[test]
    fn secret_named_setting_event_redacts_value() {
        let (_d, backend) = fixture_backend();
        backend
            .set_setting(&settings_payload("api_token", "PLAINTEXT-TOKEN-CANARY"))
            .unwrap();

        let events = ddl_events(&backend);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].target_path, "/sys/settings");
        assert!(events[0].payload_json.contains(r#""key":"api_token""#));
        assert!(!events[0].payload_json.contains("PLAINTEXT-TOKEN-CANARY"));
        assert!(events[0].payload_json.contains("<redacted>"));
    }

    fn billing_payload(team: &str, tier: &str, status: &str) -> RowBatch {
        let schema = Schema::new(vec![
            Column::new("team_id", ColumnType::Text, false),
            Column::new("tier", ColumnType::Text, false),
            Column::new("status", ColumnType::Text, false),
        ]);
        RowBatch::new(
            schema,
            vec![Row::new(vec![
                Value::Text(team.into()),
                Value::Text(tier.into()),
                Value::Text(status.into()),
            ])],
        )
    }

    /// t67: recording a tier through `INSERT INTO /sys/billing` round-trips as DATA and drives the
    /// ENTITLEMENT GATE — a paid-team active plan permits team connections; a free plan and an
    /// UNRECORDED team both fail closed to free (deny). The upsert replaces on a re-record.
    #[test]
    fn billing_tier_round_trips_and_gates_team_connections() {
        use qfs_identity::Capability;
        let (_d, backend) = fixture_backend();

        // An unrecorded team is the free floor (default-deny toward the lower tier).
        let unknown = backend.get_billing_plan("team-ghost");
        assert!(!unknown.permits(Capability::TeamConnections));

        // Record a PAID team plan; it round-trips through /sys/billing and unlocks the paid capability.
        assert_eq!(
            backend
                .set_billing(&billing_payload("team-acme", "paid-team", "active"))
                .unwrap(),
            1
        );
        let plan = backend.get_billing_plan("team-acme");
        assert_eq!(plan.tier, qfs_identity::Tier::PaidTeam);
        assert!(
            plan.permits(Capability::TeamConnections),
            "an actively-paid team unlocks team connections"
        );
        // Visible as DATA through the /sys/billing relation (one-engine-three-faces).
        let billing = backend.scan(SysNode::Billing).unwrap();
        assert_eq!(texts(&billing, "team_id"), vec!["team-acme"]);
        assert_eq!(texts(&billing, "tier"), vec!["paid-team"]);

        // Upsert REPLACES (one plan per team): downgrade to free ⇒ the gate now DENIES (fail closed).
        backend
            .set_billing(&billing_payload("team-acme", "free-individual", "inactive"))
            .unwrap();
        assert_eq!(backend.scan(SysNode::Billing).unwrap().rows.len(), 1);
        assert!(
            !backend
                .get_billing_plan("team-acme")
                .permits(Capability::TeamConnections),
            "a downgraded plan must lose the paid capability"
        );

        // A garbled stored tier resolves to the free floor (never paid).
        backend
            .set_billing(&billing_payload("team-x", "enterprise-unlimited", "active"))
            .unwrap();
        assert!(!backend
            .get_billing_plan("team-x")
            .permits(Capability::TeamConnections));

        // Each set self-audited (administration observes itself): 3 INSERT rows on /sys/billing.
        let audit = backend.scan(SysNode::Audit).unwrap();
        assert_eq!(texts(&audit, "path"), vec!["/sys/billing"; 3]);

        let events = ddl_events(&backend);
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].target_path, "/sys/billing");
        assert_eq!(events[0].verb, "UPSERT");
        assert!(events[0].payload_json.contains(r#""team_id":"team-acme""#));
        assert!(events[0].payload_json.contains(r#""tier":"paid-team""#));
    }

    /// t67: the at-least-once provider webhook apply is IDEMPOTENT — a replayed event id (the dedup
    /// ledger PK) is a no-op, so a re-delivered "subscription cancelled" cannot double-apply or fight
    /// a later state. A NEW event id does apply (flips the plan).
    #[test]
    fn provider_webhook_apply_is_idempotent() {
        use qfs_identity::Capability;
        let (_d, backend) = fixture_backend();

        // First delivery of evt-1 (upgrade to paid) APPLIES.
        assert!(backend
            .apply_provider_event(
                "evt-1",
                "team-acme",
                "paid-team",
                "active",
                Some("2026-12-31")
            )
            .unwrap());
        assert!(backend
            .get_billing_plan("team-acme")
            .permits(Capability::TeamConnections));

        // A REPLAY of evt-1 (same id) is a deduped no-op (false), state unchanged.
        assert!(!backend
            .apply_provider_event("evt-1", "team-acme", "free-individual", "canceled", None)
            .unwrap());
        assert!(
            backend
                .get_billing_plan("team-acme")
                .permits(Capability::TeamConnections),
            "a replayed event must NOT double-apply / overwrite the plan"
        );

        // A NEW event (evt-2, cancellation) DOES apply ⇒ the gate fails closed to free.
        assert!(backend
            .apply_provider_event("evt-2", "team-acme", "free-individual", "canceled", None)
            .unwrap());
        assert!(!backend
            .get_billing_plan("team-acme")
            .permits(Capability::TeamConnections));

        let events = ddl_events(&backend);
        assert_eq!(events.len(), 2, "deduped provider replay must not append");
        assert_eq!(events[0].target_path, "/sys/billing");
        assert_eq!(events[0].verb, "UPSERT");
        assert!(events[0].payload_json.contains(r#""team_id":"team-acme""#));
    }

    #[test]
    fn insert_policy_requires_a_name() {
        let (_d, backend) = fixture_backend();
        let schema = Schema::new(vec![Column::new("allow", ColumnType::Text, true)]);
        let row = RowBatch::new(schema, vec![Row::new(vec![Value::Text("SELECT".into())])]);
        assert!(backend.insert_policy(&row).is_err());
        assert!(ddl_events(&backend).is_empty());
    }

    #[tokio::test]
    async fn read_driver_scans_through_the_seam() {
        let (_d, backend) = fixture_backend();
        let reader = SysReadDriver::new(Arc::new(backend));
        let scan = ScanNode {
            source: qfs_pushdown::SourceId::new("sys"),
            path: "/sys/users".to_string(),
            pushed: PushedQuery::default(),
            schema: sys_node_schema(SysNode::Users),
            materialize_content: false,
        };
        let batch = reader
            .scan(&scan, &RequestContext::anonymous())
            .await
            .unwrap();
        assert_eq!(texts(&batch, "primary_email"), vec!["a@qmu.jp"]);
        // An unknown /sys segment is a structured invalid-path error (no panic).
        let bad = ScanNode {
            source: qfs_pushdown::SourceId::new("sys"),
            path: "/sys/nope".to_string(),
            pushed: PushedQuery::default(),
            schema: Schema::new(vec![]),
            materialize_content: false,
        };
        assert!(reader
            .scan(&bad, &RequestContext::anonymous())
            .await
            .is_err());
    }

    // ---- /sys/whoami: the "who am I" answer on the scan path (mission acceptance 1/5) ----

    fn whoami_scan() -> ScanNode {
        ScanNode {
            source: qfs_pushdown::SourceId::new("sys"),
            path: "/sys/whoami".to_string(),
            pushed: PushedQuery::default(),
            schema: sys_node_schema(SysNode::Whoami),
        }
    }

    fn bool_at(batch: &RowBatch, col: &str) -> bool {
        let idx = batch
            .schema
            .columns
            .iter()
            .position(|c| c.name.as_str() == col)
            .expect("column present");
        matches!(batch.rows[0].values[idx], Value::Bool(true))
    }

    #[test]
    fn sys_whoami_schema_is_closed_set_and_credential_free() {
        // The answer is data through the ONE engine on the /sys closed set, and carries NO
        // credential column — only `signed_in` + `user` (the /sys/connections redaction contract).
        let schema = sys_node_schema(SysNode::Whoami);
        let names: Vec<&str> = schema.columns.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, vec!["signed_in", "user"]);
        for banned in ["token", "session", "cookie", "password", "hash", "secret"] {
            assert!(
                !names.iter().any(|n| n.contains(banned)),
                "whoami must expose no credential column, found one containing {banned}"
            );
        }
    }

    #[tokio::test]
    async fn sys_whoami_resolves_the_request_principal_both_ways() {
        let (_d, backend) = fixture_backend();
        let reader = SysReadDriver::new(Arc::new(backend));

        // A request carrying a live principal resolves to the named user.
        let signed = reader
            .scan(&whoami_scan(), &RequestContext::for_user("7"))
            .await
            .unwrap();
        assert!(bool_at(&signed, "signed_in"));
        assert_eq!(texts(&signed, "user"), vec!["7"]);

        // A request with no session resolves to an explicit not-signed-in row — a first-class
        // answer (one row), never an error and never a silent fallback to a sole user.
        let anon = reader
            .scan(&whoami_scan(), &RequestContext::anonymous())
            .await
            .unwrap();
        assert_eq!(anon.rows.len(), 1, "not-signed-in is a row, not an absence");
        assert!(!bool_at(&anon, "signed_in"));
        assert!(
            matches!(anon.rows[0].values[1], Value::Null),
            "anonymous user column is NULL, not the sole user"
        );
    }
}
