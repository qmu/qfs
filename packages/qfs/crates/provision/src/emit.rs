//! The **canonical emitter** (blueprint §16, Decision X): a [`ConfigState`] — both config stores
//! — rendered as the `.qfs` "as code" source-of-truth document.
//!
//! The document is a deterministic, normalized list of config-write statements — the `/sys`
//! collections first, then the `/server` collections, each walked in a fixed order and, within
//! each collection, in [`BTreeMap`](std::collections::BTreeMap) key order — prefixed by a
//! generation-stamp comment header. Two identical states emit **byte-identical** documents.
//!
//! ## Config projection only (never runtime fields, never secrets)
//! The emitter renders the [`crate::ProjRow`] of each row, so the runtime freshness fields
//! ([`ViewDef::last_run`]/`cache_json`, `JobDef::last_run`) never appear, **secretish settings
//! are excluded entirely** (blueprint §16, amended — never emitted, not redacted), and every
//! credential field is a *reference* (`secret_ref`, an auth scheme name) — never a value.
//!
//! ## Emission form (the CREATE ≡ INSERT twin)
//! A `/server` binding's deferred body (`AS <query>`/`DO <plan>`) is stored in `ServerState` as
//! its **canonical span-normalised spec** (serde JSON — see [`qfs_core::StatementSpec::canonical`]),
//! not as re-parseable source, and no AST→source renderer exists. So the five scalar `/server`
//! collections are emitted as their `UPSERT INTO /server/<node>` twin (the exact desugar target
//! of a `CREATE`, [`qfs_core::CREATE_WRITE_OP`]), carrying each column value verbatim: the
//! canonical JSON body contains `"` (which the qfs lexer rejects), so on reload it is kept
//! literal and the projection round-trips exactly. `/server/policies` — whose `allow` array
//! cannot ride a `VALUES` literal — is emitted as `CREATE POLICY`, whose desugar rebuilds the
//! identical rule-string array. The `/sys` collections emit as their write twins against the
//! `/sys/<node>` paths (`INSERT INTO /sys/policies` — the one verb that node accepts — and
//! `UPSERT INTO` for drivers/settings/paths); the loader also accepts the `CONNECT` sugar, which
//! the parser itself desugars to the identical `/sys/paths` write.

use std::collections::BTreeMap;

use qfs_core::{ServerNode, ServerWriteOp, Value};
use qfs_server::{PolicyDef, ServerState};

use crate::proj::{collection_projs, ProjRow, SERVER_NODES};
use crate::state::{sys_collection_projs, ConfigState, SysCollection, SYS_COLLECTIONS};

/// The generation stamp of a source-of-truth document (blueprint §16): the migration counts plus
/// the `ddl_event` hash-chain head, so a later `apply` can detect a moved base. The pure core
/// takes the stamp as a caller-supplied value (no DB read); increment 3's fetch computes it live.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GenerationStamp {
    /// Applied system-DB migration count.
    pub system_migrations: usize,
    /// Applied project-DB migration count (`None` when no project DB is present).
    pub project_migrations: Option<usize>,
    /// The head of the `sys_ddl_events` hash chain (`None` when the chain is empty).
    pub ddl_event_head: Option<DdlEventHead>,
}

/// The `sys_ddl_events` chain head — the audit-spine anchor a stale-base check compares against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DdlEventHead {
    /// The head event sequence number.
    pub seq: i64,
    /// The head event content hash.
    pub hash: String,
}

impl GenerationStamp {
    /// Parse the generation stamp back out of an emitted document's comment header (the inverse
    /// of [`emit_header`]). Returns `None` when the document carries no `# generation:` lines (a
    /// hand-written document with no stamp — the caller treats that as "no base to compare").
    ///
    /// The `apply` stale-base gate compares this against the live stamp; `plan` renders whether
    /// the base moved.
    #[must_use]
    pub fn parse_from_document(document: &str) -> Option<Self> {
        let mut system_migrations = None;
        let mut project_migrations = None;
        let mut ddl_event_head = None;
        let mut saw_any = false;
        for line in document.lines() {
            let Some(rest) = line.trim_start().strip_prefix('#') else {
                // Statements begin below the header; stop scanning at the first non-comment.
                if line.trim().is_empty() {
                    continue;
                }
                break;
            };
            let Some(rest) = rest.trim_start().strip_prefix("generation:") else {
                continue;
            };
            saw_any = true;
            for token in rest.split_whitespace() {
                if let Some(v) = token.strip_prefix("system_migrations=") {
                    system_migrations = v.parse::<usize>().ok();
                } else if let Some(v) = token.strip_prefix("project_migrations=") {
                    project_migrations = parse_opt_usize(v);
                } else if let Some(v) = token.strip_prefix("ddl_event_head=") {
                    ddl_event_head = parse_ddl_head(v);
                }
            }
        }
        if !saw_any {
            return None;
        }
        Some(Self {
            system_migrations: system_migrations.unwrap_or(0),
            project_migrations,
            ddl_event_head,
        })
    }
}

/// Parse a `project_migrations=<n|->` value: `-` is `None`, a number is `Some(n)`.
fn parse_opt_usize(v: &str) -> Option<usize> {
    if v == "-" {
        None
    } else {
        v.parse().ok()
    }
}

/// Parse a `ddl_event_head=<seq>:<hash>` value (`-` = no head).
fn parse_ddl_head(v: &str) -> Option<DdlEventHead> {
    if v == "-" {
        return None;
    }
    let (seq, hash) = v.split_once(':')?;
    Some(DdlEventHead {
        seq: seq.parse().ok()?,
        hash: hash.to_string(),
    })
}

/// Emit `state` as the canonical `.qfs` source-of-truth document under `stamp`. Deterministic:
/// identical `(state, stamp)` inputs produce a byte-identical string.
#[must_use]
pub fn emit(state: &ConfigState, stamp: &GenerationStamp) -> String {
    let mut out = String::new();
    emit_header(&mut out, stamp);
    // The /sys store first (the foundation a /server binding may reference), then /server —
    // the same fixed order the diff engine walks.
    for coll in SYS_COLLECTIONS {
        emit_write_collection(
            &mut out,
            sys_collection_verb(coll),
            &coll.path(),
            &sys_collection_projs(&state.sys, coll),
        );
    }
    for node in SERVER_NODES {
        if node == ServerNode::Policies {
            emit_policies(&mut out, &state.server);
        } else {
            emit_write_collection(
                &mut out,
                "UPSERT INTO",
                &node.path(),
                &collection_projs(&state.server, node),
            );
        }
    }
    out
}

/// The document verb of a `/sys` collection: `INSERT INTO` for `/sys/policies` (the one write
/// verb that node accepts — `sys_node_capabilities`), `UPSERT INTO` everywhere else (settings /
/// paths / drivers apply upsert-on-key semantics, the replay-safe declarative verb).
fn sys_collection_verb(coll: SysCollection) -> &'static str {
    match coll {
        SysCollection::Policies => "INSERT INTO",
        SysCollection::Drivers
        | SysCollection::Settings
        | SysCollection::Paths
        // `/transform` accepts INSERT (upsert-on-name in the applier) — the replay-safe declarative
        // verb, so a reconcile re-emit is idempotent, like drivers/settings/paths.
        | SysCollection::Transforms => "UPSERT INTO",
    }
}

/// Render the generation-stamp comment header. Every line is a whole-line `#` comment, so the
/// loader's statement splitter strips it (it is metadata, never a statement).
fn emit_header(out: &mut String, stamp: &GenerationStamp) {
    out.push_str("# qfs config — source of truth (blueprint §16, Decision X)\n");
    out.push_str(
        "# config projection only: runtime fields and secretish settings are never emitted\n",
    );
    let project = stamp
        .project_migrations
        .map_or_else(|| "-".to_string(), |n| n.to_string());
    out.push_str(&format!(
        "# generation: system_migrations={} project_migrations={}\n",
        stamp.system_migrations, project,
    ));
    let head = stamp
        .ddl_event_head
        .as_ref()
        .map_or_else(|| "-".to_string(), |h| format!("{}:{}", h.seq, h.hash));
    out.push_str(&format!("# generation: ddl_event_head={head}\n"));
    out.push('\n');
}

/// Emit one scalar collection as `<verb> <path> VALUES (cols) (vals)` statements, one per row
/// in key order. Columns follow the projection's deterministic (name) order; absent optional
/// columns are simply not emitted (they round-trip as absent).
fn emit_write_collection(
    out: &mut String,
    verb: &str,
    path: &str,
    rows: &BTreeMap<String, ProjRow>,
) {
    for row in rows.values() {
        let mut cols = Vec::new();
        let mut vals = Vec::new();
        for (col, value) in row.columns() {
            cols.push(col.clone());
            vals.push(render_value(value));
        }
        out.push_str(&format!(
            "{verb} {path} VALUES ({}) ({});\n",
            cols.join(", "),
            vals.join(", "),
        ));
    }
}

/// Emit `/server/policies` as `CREATE POLICY` statements — the only writer of the `allow` array.
/// The handler rides the `ON <handler>` operand; each stored rule string is appended verbatim as
/// an `ALLOW …`/`DENY …` clause, so the desugar rebuilds the identical `allow` array.
fn emit_policies(out: &mut String, state: &ServerState) {
    for def in state.policies.values() {
        out.push_str(&render_create_policy(def));
        out.push('\n');
    }
}

/// Render one `CREATE POLICY` statement from a stored [`PolicyDef`].
fn render_create_policy(def: &PolicyDef) -> String {
    let mut s = format!("CREATE POLICY {}", def.name);
    if !def.handler.is_empty() {
        s.push_str(&format!(" ON {}", def.handler));
    }
    for rule in &def.allow {
        s.push(' ');
        s.push_str(rule);
    }
    s.push(';');
    s
}

/// Render one `/server` [`ReconcileOp`](crate::ReconcileOp) as the single statement the
/// reconcile CLI submits through the daemon's statement bridge (blueprint §16 "The face,
/// named" — apply is **statement-by-statement in plan order**, the boot-replay shape):
///
/// - `Insert`/`Update` on a scalar collection ⇒ the `UPSERT INTO /server/<node>` CREATE≡INSERT
///   twin (the exact desugar target, replay-idempotent).
/// - `Insert`/`Update` on `/server/policies` ⇒ the `CREATE POLICY` form (the `allow` rule array
///   cannot ride a `VALUES` literal; the desugar rebuilds the identical row).
/// - `Remove` ⇒ `REMOVE /server/<node>/<name>` (the authoritative destroy; the daemon flags it
///   irreversible, so it commits only with the explicit ack).
///
/// # Errors
/// A secret-free string if `op` does not target the `/server` store.
pub fn server_op_statement(op: &crate::ReconcileOp) -> Result<String, String> {
    let crate::ReconcileNode::Server(node) = op.node else {
        return Err("not a /server reconcile op".to_string());
    };
    if op.op == ServerWriteOp::Remove {
        return Ok(format!("REMOVE /server/{}/{}", node.segment(), op.name));
    }
    if node == ServerNode::Policies {
        // Rebuild the CREATE POLICY form from the projection columns (name/handler/allow).
        let mut name = op.name.clone();
        let mut handler = String::new();
        let mut rules: Vec<String> = Vec::new();
        for (col, value) in op.proj.columns() {
            match (col.as_str(), value) {
                ("name", Value::Text(s)) => name.clone_from(s),
                ("handler", Value::Text(s)) => handler.clone_from(s),
                ("allow", Value::Array(items)) => {
                    rules = items
                        .iter()
                        .filter_map(|v| match v {
                            Value::Text(s) => Some(s.clone()),
                            _ => None,
                        })
                        .collect();
                }
                _ => {}
            }
        }
        let def = PolicyDef {
            name,
            handler,
            allow: rules,
        };
        let mut s = render_create_policy(&def);
        // The bridge submits one statement (no trailing separator needed).
        s.pop();
        return Ok(s);
    }
    let mut cols = Vec::new();
    let mut vals = Vec::new();
    for (col, value) in op.proj.columns() {
        cols.push(col.clone());
        vals.push(render_value(value));
    }
    Ok(format!(
        "UPSERT INTO /server/{} VALUES ({}) ({})",
        node.segment(),
        cols.join(", "),
        vals.join(", "),
    ))
}

/// Render a scalar config value as a qfs literal: text single-quoted with `\\`/`\'` escapes,
/// booleans as bare `true`/`false`. The scalar collections only carry text/bool; any other kind
/// falls back to a quoted debug form (never reached in practice).
fn render_value(value: &Value) -> String {
    match value {
        Value::Text(s) => quote(s),
        Value::Bool(b) => b.to_string(),
        Value::Int(n) => n.to_string(),
        other => quote(&format!("{other:?}")),
    }
}

/// Single-quote a string as a qfs literal, escaping `\` then `'` (the lexer's escape set).
fn quote(s: &str) -> String {
    let escaped = s.replace('\\', "\\\\").replace('\'', "\\'");
    format!("'{escaped}'")
}
