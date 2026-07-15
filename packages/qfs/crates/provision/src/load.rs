//! The **desired-state loader** (blueprint §16, Decision X): parse a `.qfs` source-of-truth
//! document into a desired [`ConfigState`] config projection covering both stores.
//!
//! The `/server` half is the exact path boot uses — no new desugar logic: each statement runs
//! `parse_statement → lower_statement → commit` through a [`ServerConfigApplier`] over a fresh
//! `ServerState`. The `/sys` half decodes the write twins (`INSERT/UPSERT INTO /sys/<node>
//! VALUES …`) — including the `CONNECT` sugar, which the **parser itself** desugars to the same
//! `/sys/paths` write — into the owned [`SysState`] rows. The document is split with the same
//! [`qfs_server::statements`] splitter boot uses, so cosmetic differences (whitespace, comments)
//! collapse away and an emitted document reloads to the identical config projection.
//!
//! ## The exclusions hold at the door (blueprint §16, amended)
//! A document naming a **secretish setting** is rejected (secretish settings are outside the
//! universe — not silently dropped, so a hand-edited document gets a crisp refusal), and a
//! statement against an excluded or unknown `/sys` node (billing, accounts, audit, …) is a
//! structured [`LoadError::Universe`] — the desired state can never smuggle an excluded
//! collection into the diff.

use std::sync::{Arc, RwLock};

use qfs_core::{commit, secretish_setting_key};
use qfs_parser::{
    parse_statement, EffectBody, EffectStmt, EffectVerb, Expr, Literal, Statement, Values,
};
use qfs_server::{lower_statement, statements, ServerConfigApplier, ServerState};

use crate::state::{
    ConfigState, PathBindingRow, SysDriverRow, SysPolicyRow, SysState, TransformRow,
};

/// A structured, secret-free failure from loading a source-of-truth document. Each variant is
/// line-located so a malformed document points at the offending statement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadError {
    /// A statement failed to parse.
    Parse {
        /// The 1-based document line.
        line: usize,
        /// The parser's stable machine code.
        code: String,
        /// A secret-free parser message.
        message: String,
    },
    /// A statement parsed but did not lower to a `/server` config write.
    Lower {
        /// The 1-based document line.
        line: usize,
        /// A secret-free detail.
        detail: String,
    },
    /// A statement parsed but writes neither `/server` nor `/sys`.
    NotServerConfig {
        /// The 1-based document line.
        line: usize,
    },
    /// The lowered write failed to apply to the desired state.
    Commit {
        /// The 1-based document line.
        line: usize,
        /// A secret-free reason.
        reason: String,
    },
    /// A statement targets a collection outside the provisioning universe (billing, accounts,
    /// a secretish setting, an unknown `/sys` node) or uses a non-document verb.
    Universe {
        /// The 1-based document line.
        line: usize,
        /// A secret-free detail.
        detail: String,
    },
    /// The desired-state lock was poisoned (a bug, never a user error).
    Poisoned,
}

impl core::fmt::Display for LoadError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Parse {
                line,
                code,
                message,
            } => write!(f, "line {line}: parse error [{code}]: {message}"),
            Self::Lower { line, detail } => write!(f, "line {line}: cannot lower: {detail}"),
            Self::NotServerConfig { line } => {
                write!(f, "line {line}: statement does not write /server or /sys")
            }
            Self::Commit { line, reason } => write!(f, "line {line}: apply failed: {reason}"),
            Self::Universe { line, detail } => {
                write!(
                    f,
                    "line {line}: outside the provisioning universe: {detail}"
                )
            }
            Self::Poisoned => write!(f, "desired-state lock poisoned"),
        }
    }
}

impl std::error::Error for LoadError {}

/// Load a `.qfs` source-of-truth document into the desired [`ConfigState`] config projection.
///
/// # Errors
/// [`LoadError`] — line-located — on a parse, lower, universe, or apply failure.
pub fn load(document: &str) -> Result<ConfigState, LoadError> {
    let server = Arc::new(RwLock::new(ServerState::new()));
    let mut sys = SysState::default();
    for (line, src) in statements(document) {
        let stmt = parse_statement(&src).map_err(|e| LoadError::Parse {
            line,
            code: e.code.as_str().to_string(),
            message: e.message.clone(),
        })?;
        // A /sys write twin (including the CONNECT sugar, which the parser already desugared
        // into `INSERT INTO /sys/paths`) decodes into the owned SysState rows.
        if let Statement::Effect(effect) = &stmt {
            if targets_sys(effect) {
                decode_sys_effect(&mut sys, line, effect)?;
                continue;
            }
            // §15: an `UPSERT INTO /transform` twin decodes into the owned transform definitions.
            if targets_transform(effect) {
                decode_transform_effect(&mut sys, line, effect)?;
                continue;
            }
        }
        apply_server(&server, line, &stmt)?;
    }
    let server = match Arc::try_unwrap(server) {
        Ok(lock) => lock.into_inner().map_err(|_| LoadError::Poisoned)?,
        // Unreachable: the loop dropped every transient applier borrow before returning.
        Err(_) => return Err(LoadError::Poisoned),
    };
    Ok(ConfigState { server, sys })
}

/// Whether an effect statement targets the `/sys/...` mount.
fn targets_sys(effect: &EffectStmt) -> bool {
    effect
        .target
        .segments
        .first()
        .is_some_and(|s| s.name == "sys")
}

/// Whether an effect statement targets the top-level `/transform` mount (blueprint §15).
fn targets_transform(effect: &EffectStmt) -> bool {
    effect
        .target
        .segments
        .first()
        .is_some_and(|s| s.name == "transform")
}

/// Decode one `UPSERT INTO /transform` document twin into the desired [`SysState::transforms`]. Only
/// the declarative `INSERT`/`UPSERT` verbs are document forms (`UPDATE`/`REMOVE` are not).
fn decode_transform_effect(
    sys: &mut SysState,
    line: usize,
    effect: &EffectStmt,
) -> Result<(), LoadError> {
    if !matches!(effect.verb, EffectVerb::Insert | EffectVerb::Upsert) {
        return Err(LoadError::Universe {
            line,
            detail: "a desired-state document declares rows (INSERT/UPSERT); \
                     UPDATE/REMOVE are not document forms"
                .to_string(),
        });
    }
    let cols = values_columns(line, &effect.body)?;
    let name = require_text(line, &cols, "name")?;
    sys.transforms.insert(
        name.clone(),
        TransformRow {
            name,
            input: require_text(line, &cols, "input")?,
            output: require_text(line, &cols, "output")?,
            provider: require_text(line, &cols, "provider")?,
            model: require_text(line, &cols, "model")?,
            effort: get_text(&cols, "effort"),
            secret_ref: get_text(&cols, "secret_ref"),
        },
    );
    Ok(())
}

/// Parse → lower → COMMIT one `/server` statement into the shared desired state (the boot unit).
fn apply_server(
    state: &Arc<RwLock<ServerState>>,
    line: usize,
    stmt: &Statement,
) -> Result<(), LoadError> {
    let plan = lower_statement(stmt)
        .map_err(|detail| LoadError::Lower { line, detail })?
        .ok_or(LoadError::NotServerConfig { line })?;
    let mut applier = ServerConfigApplier::new(state);
    let report = commit(&plan, &mut applier, |_| {});
    if let Some(err) = report.failed {
        return Err(LoadError::Commit {
            line,
            reason: err.reason,
        });
    }
    Ok(())
}

/// Decode one `/sys` write twin into the desired [`SysState`]. Only the declarative document
/// verbs (`INSERT`/`UPSERT`) and the four universe collections are accepted; everything else is
/// a structured [`LoadError::Universe`].
fn decode_sys_effect(
    sys: &mut SysState,
    line: usize,
    effect: &EffectStmt,
) -> Result<(), LoadError> {
    if !matches!(effect.verb, EffectVerb::Insert | EffectVerb::Upsert) {
        return Err(LoadError::Universe {
            line,
            detail: "a desired-state document declares rows (INSERT/UPSERT); \
                     UPDATE/REMOVE are not document forms"
                .to_string(),
        });
    }
    let node = effect
        .target
        .segments
        .get(1)
        .map(|s| s.name.as_str())
        .unwrap_or_default();
    let cols = values_columns(line, &effect.body)?;
    match node {
        "settings" => {
            let key = require_text(line, &cols, "key")?;
            if secretish_setting_key(&key) {
                return Err(LoadError::Universe {
                    line,
                    detail: format!(
                        "setting `{key}` is secretish — excluded from the provisioning \
                         universe (never emitted, never diffed, never destroyed)"
                    ),
                });
            }
            let value = require_text(line, &cols, "value")?;
            sys.settings.insert(key, value);
        }
        "policies" => {
            let name = require_text(line, &cols, "name")?;
            sys.policies.insert(
                name.clone(),
                SysPolicyRow {
                    name,
                    allow: get_text(&cols, "allow"),
                    target: get_text(&cols, "target"),
                },
            );
        }
        "drivers" => {
            let name = require_text(line, &cols, "name")?;
            let kind = require_text(line, &cols, "kind")?;
            sys.drivers.insert(
                name.clone(),
                SysDriverRow {
                    name,
                    kind,
                    base_url: get_text(&cols, "base_url"),
                    auth: get_text(&cols, "auth"),
                    pagination: get_text(&cols, "pagination"),
                    of_type: get_text(&cols, "of_type"),
                    verb: get_text(&cols, "verb"),
                    body: get_text(&cols, "body"),
                    irreversible: get_bool(&cols, "irreversible"),
                },
            );
        }
        "paths" => {
            let path = require_text(line, &cols, "path")?;
            sys.bindings.insert(
                path.clone(),
                PathBindingRow {
                    path,
                    driver: get_text(&cols, "driver"),
                    at: get_text(&cols, "at"),
                    secret_ref: get_text(&cols, "secret_ref"),
                    alias_of: get_text(&cols, "alias_of"),
                    host: get_text(&cols, "host"),
                    account: get_text(&cols, "account"),
                    app: get_text(&cols, "app"),
                },
            );
        }
        // Billing, accounts, audit, users, … : outside the universe entirely (blueprint §16) —
        // never loadable, so never diffable, so never destroyable.
        other => {
            return Err(LoadError::Universe {
                line,
                detail: format!(
                    "/sys/{other} is not a provisioning collection (universe: drivers, \
                     policies, settings, paths)"
                ),
            });
        }
    }
    Ok(())
}

/// The decoded `(column, literal)` pairs of a single-row `VALUES (cols) (vals)` body. `NULL`
/// columns are dropped (absent optional == `NULL`, the projection convention — this is what
/// makes the `CONNECT` desugar, which emits explicit `NULL`s, load equal to a present-only row).
fn values_columns(
    line: usize,
    body: &EffectBody,
) -> Result<Vec<(String, LiteralValue)>, LoadError> {
    let EffectBody::Values(Values {
        columns: Some(columns),
        rows,
    }) = body
    else {
        return Err(LoadError::Universe {
            line,
            detail: "a /sys document row needs explicit columns: VALUES (col, …) (val, …)"
                .to_string(),
        });
    };
    let Some(first) = rows.first() else {
        return Err(LoadError::Universe {
            line,
            detail: "a /sys document row needs one VALUES row".to_string(),
        });
    };
    if columns.len() != first.len() {
        return Err(LoadError::Universe {
            line,
            detail: format!(
                "column/value count mismatch ({} cols, {} vals)",
                columns.len(),
                first.len()
            ),
        });
    }
    let mut out = Vec::with_capacity(columns.len());
    for (col, expr) in columns.iter().zip(first) {
        match literal_of(expr) {
            // An explicit NULL is an absent optional — dropped, never stored.
            Some(LiteralValue::Null) => {}
            Some(v) => out.push((col.clone(), v)),
            None => {
                return Err(LoadError::Universe {
                    line,
                    detail: format!("column `{col}` must be a scalar literal"),
                });
            }
        }
    }
    Ok(out)
}

/// A decoded scalar literal from a document `VALUES` cell.
enum LiteralValue {
    /// A text literal.
    Text(String),
    /// A boolean literal.
    Bool(bool),
    /// An explicit `NULL` (dropped — absent optional).
    Null,
}

fn literal_of(expr: &Expr) -> Option<LiteralValue> {
    match expr {
        Expr::Lit(Literal::Str(s)) => Some(LiteralValue::Text(s.clone())),
        Expr::Lit(Literal::Bool(b)) => Some(LiteralValue::Bool(*b)),
        Expr::Lit(Literal::Int(n)) => Some(LiteralValue::Text(n.to_string())),
        Expr::Lit(Literal::Null) => Some(LiteralValue::Null),
        _ => None,
    }
}

fn get_text(cols: &[(String, LiteralValue)], name: &str) -> Option<String> {
    cols.iter().find_map(|(c, v)| match v {
        LiteralValue::Text(s) if c == name && !s.is_empty() => Some(s.clone()),
        _ => None,
    })
}

fn get_bool(cols: &[(String, LiteralValue)], name: &str) -> bool {
    cols.iter()
        .any(|(c, v)| c == name && matches!(v, LiteralValue::Bool(true)))
}

fn require_text(
    line: usize,
    cols: &[(String, LiteralValue)],
    name: &str,
) -> Result<String, LoadError> {
    get_text(cols, name).ok_or_else(|| LoadError::Universe {
        line,
        detail: format!("a non-empty `{name}` column is required"),
    })
}
