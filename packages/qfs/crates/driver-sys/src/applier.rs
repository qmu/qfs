//! [`SysApplier`] — the `/sys` driver's apply leg (blueprint §7). It lowers a write effect node
//! into the one gated System-DB mutation this slice ships: `INSERT INTO /sys/policies`. Every
//! other write is rejected here (belt-and-suspenders over the parse-time capability gate):
//! `/sys/audit` is append-only and the remaining admin views are read-only.
//!
//! The real I/O happens in the injected [`SysBackend`] (binary-side rusqlite); the applier is a
//! pure router over the owned effect node, so it is stateless and `&self`-applies through the
//! runtime's [`SharedApplier`] bridge.
//!
//! The backend appends the t76 audit row transactionally with the policy write — so the audit
//! emission is NOT duplicated by the CLI commit path's best-effort emitter (which skips `/sys`
//! legs precisely because they self-audit at the source of truth).

use qfs_plan::{AppliedEffect, ApplyError, EffectKind, EffectNode, PlanApplier};
use qfs_runtime::{EffectError, EffectOutput, SharedApplier};
use qfs_types::{RowBatch, Value};

use std::sync::Arc;

use crate::backend::{SysBackend, SysError};
use crate::schema::{node_for_path, SysNode};

/// The synchronous `/sys` apply leg. Holds the injected backend behind an `Arc` (so the leg is
/// cheap to clone and `&self`-apply). Stateless across calls.
#[derive(Clone)]
pub struct SysApplier {
    backend: Arc<dyn SysBackend>,
}

impl SysApplier {
    /// Build an applier over an injected [`SysBackend`] (the binary's System-DB implementation).
    #[must_use]
    pub fn new(backend: Arc<dyn SysBackend>) -> Self {
        Self { backend }
    }

    /// Route one effect node to the backend: resolve the `/sys` node, gate the verb, and apply.
    /// Only `INSERT INTO /sys/policies` is permitted; everything else is a structured rejection.
    fn apply_node(&self, node: &EffectNode) -> Result<u64, SysError> {
        let path = node.target.path.as_str();
        let sys_node = node_for_path(path).ok_or_else(|| SysError::UnknownNode {
            path: path.to_string(),
        })?;

        match (&node.kind, sys_node) {
            // The gated writes: a policy grant, or a deployment setting (the safety mode — t59,
            // upsert-on-`key`). Both land in the System DB + append a t76 audit row in one txn.
            (EffectKind::Insert, SysNode::Policies) => self.backend.insert_policy(&node.args),
            (EffectKind::Insert | EffectKind::Upsert, SysNode::Settings) => {
                self.backend.set_setting(&node.args)
            }
            // t67: record/grant a team's billing tier (upsert-on-`team_id`). The gate later reads
            // this plan state; the write is a /sys mutation (previewed, committed, self-audited).
            (EffectKind::Insert | EffectKind::Upsert, SysNode::Billing) => {
                self.backend.set_billing(&node.args)
            }
            // t100020 (the CONNECT model): bind / re-bind a defined path — `INSERT/UPSERT INTO
            // /sys/paths` (upsert on `path`) — into the Project DB `path_binding` table.
            (EffectKind::Insert | EffectKind::Upsert, SysNode::Paths) => {
                self.backend.upsert_binding(&node.args)
            }
            // §13: install a declared-driver declaration — `INSERT INTO /sys/drivers` (the desugar
            // target of a `CREATE DRIVER`/`TYPE`/`VIEW`/`MAP` script) — into the System DB
            // `sys_drivers` table. The row carries declaration text + selectors only, never a secret.
            (EffectKind::Insert | EffectKind::Upsert, SysNode::Drivers) => {
                self.backend.insert_driver(&node.args)
            }
            // 20260703040000 (the CREATE ACCOUNT model): declare a service account — `INSERT INTO
            // /sys/accounts` — by recording consent (gated on a signed-in operator). The token stays
            // out-of-band, never in this row.
            (EffectKind::Insert | EffectKind::Upsert, SysNode::Accounts) => {
                self.backend.record_account(&node.args)
            }
            // `REMOVE /sys/accounts/<provider>/<account>`: the `<provider>` and `<account>` ride as
            // the two segments AFTER `accounts`; reconstruct them and delete the account (token +
            // consent). A path-safe label (github/slack/… label) round-trips cleanly here.
            (EffectKind::Remove, SysNode::Accounts) => match account_from_target(path) {
                // Path-addressed: `REMOVE /sys/accounts/<provider>/<account>` — a path-safe label
                // (github/slack/… label) round-trips as the two segments after `accounts`.
                Some((provider, account)) => self.backend.remove_account(&provider, &account),
                // Filter-addressed: `REMOVE /sys/accounts WHERE account == '<value>' [AND provider
                // == '<p>']`. A Google account whose label is an EMAIL cannot ride a path (`@` is a
                // version coordinate the router drops), so it rides SAFELY as a string literal in
                // the WHERE — carried here on the SELECTOR channel (§7). Resolve (provider, account).
                None => {
                    let (provider, account) = self.account_from_filter(node)?;
                    self.backend.remove_account(&provider, &account)
                }
            },
            // t100020: `DISCONNECT` — `REMOVE /sys/paths/<path>`. The user path rides as the path
            // segments AFTER `paths` (a multi-segment defined path is `/sys/paths/a/b`); reconstruct
            // it and remove the binding (its aliases cascade).
            (EffectKind::Remove, SysNode::Paths) => {
                let user_path =
                    defined_path_from_target(path).ok_or_else(|| SysError::MalformedEffect {
                        reason: "DISCONNECT needs a path, e.g. REMOVE /sys/paths/work/orders"
                            .into(),
                    })?;
                self.backend.remove_binding(&user_path)
            }
            // Provisioning reconcile (blueprint §16): the authoritative UPDATE/REMOVE the
            // `qfs apply` batch carries. The row/segment names the key; the backend appends the
            // audit + ddl_event transactionally, exactly like the INSERT writers.
            (EffectKind::Update, SysNode::Policies) => self.backend.update_policy(&node.args),
            (EffectKind::Remove, SysNode::Policies) => {
                let name = required_key(&node.args, "name", "REMOVE /sys/policies")?;
                self.backend.remove_policy(&name)
            }
            (EffectKind::Remove, SysNode::Settings) => {
                let key = required_key(&node.args, "key", "REMOVE /sys/settings")?;
                self.backend.remove_setting(&key)
            }
            (EffectKind::Remove, SysNode::Drivers) => {
                let name = required_key(&node.args, "name", "REMOVE /sys/drivers")?;
                self.backend.remove_driver(&name)
            }
            // Declared drivers are install/uninstall only: a driver *edit* is refused (remove and
            // re-add to change one). An honest structured rejection, never a silent duplicate.
            (EffectKind::Update, SysNode::Drivers) => Err(SysError::MalformedEffect {
                reason: "declared drivers are install/uninstall only; \
                         remove and re-add to change a driver"
                    .into(),
            }),
            // /sys/audit is append-only; the other admin views are read-only. Reject every other
            // write at the applier too (so even a hand-built plan that bypassed the parse-time
            // capability gate cannot mutate them).
            (kind, n) => Err(SysError::AppendOnly {
                node: n.segment(),
                verb: static_verb_label(kind),
            }),
        }
    }

    /// Resolve `(provider, account)` for a filter-addressed `REMOVE /sys/accounts WHERE …`. The
    /// account (required) rides as a string literal in the WHERE row, so an email's `@` survives
    /// intact (a path would drop it). An explicit `provider == '<p>'` disambiguates; without it the
    /// provider is resolved by matching the account in the `/sys/accounts` registry (its scan
    /// collapses the Google trio to one email-keyed row, so an email names a single `google`
    /// provider). Zero or ambiguous matches are honest, secret-free rejections.
    /// Resolve `(provider, account)` from the REMOVE's **WHERE-selector** (blueprint §7) — the one
    /// channel a filter travels on. A REMOVE writes nothing, so its `args` is empty; the selector is
    /// where `WHERE account == '<v>' [AND provider == '<p>']` lands.
    fn account_from_filter(&self, node: &EffectNode) -> Result<(String, String), SysError> {
        let account = node
            .selector_text("account")
            .ok_or_else(|| SysError::MalformedEffect {
                reason:
                    "REMOVE /sys/accounts needs a path (/sys/accounts/<provider>/<account>) or \
                         a filter (REMOVE /sys/accounts WHERE account == '<value>')"
                        .into(),
            })?;
        if let Some(provider) = node.selector_text("provider") {
            return Ok((provider, account));
        }
        let providers = self.providers_for_account(&account)?;
        match providers.as_slice() {
            [one] => Ok((one.clone(), account)),
            [] => Err(SysError::MalformedEffect {
                reason: format!("no /sys/accounts row has account == '{account}'"),
            }),
            many => Err(SysError::MalformedEffect {
                reason: format!(
                    "account '{account}' is ambiguous across providers {many:?} — disambiguate \
                     with REMOVE /sys/accounts WHERE provider == '<p>' AND account == '{account}'"
                ),
            }),
        }
    }

    /// The providers whose `/sys/accounts` registry row matches `account`. Read side (`scan`), so it
    /// sees the same collapsed, secret-free view a user does.
    fn providers_for_account(&self, account: &str) -> Result<Vec<String>, SysError> {
        let rows = self.backend.scan(SysNode::Accounts)?;
        let (Some(ai), Some(pi)) = (col_index(&rows, "account"), col_index(&rows, "provider"))
        else {
            return Ok(Vec::new());
        };
        Ok(rows
            .rows
            .iter()
            .filter(|r| matches!(r.values.get(ai), Some(Value::Text(a)) if a == account))
            .filter_map(|r| match r.values.get(pi) {
                Some(Value::Text(p)) => Some(p.clone()),
                _ => None,
            })
            .collect())
    }
}

/// Reconstruct the user-defined path from a `REMOVE /sys/paths/<path…>` target (t100020). The
/// defined path rides as every segment AFTER `paths`, so `/sys/paths/work/orders` → `/work/orders`.
/// Returns `None` for a bare `/sys/paths` (no path named).
fn defined_path_from_target(target: &str) -> Option<String> {
    let rest = target
        .strip_prefix("/sys/paths/")
        .or_else(|| target.strip_prefix("sys/paths/"))?;
    let rest = rest.trim_matches('/');
    (!rest.is_empty()).then(|| format!("/{rest}"))
}

/// Reconstruct `(provider, account)` from a `REMOVE /sys/accounts/<provider>/<account…>` target
/// (20260703040000). The provider is the segment after `accounts`; the account is EVERYTHING after
/// it (joined) — so a label carrying `/` still round-trips. Returns `None` for a bare
/// `/sys/accounts` or a provider with no account segment.
fn account_from_target(target: &str) -> Option<(String, String)> {
    let rest = target
        .strip_prefix("/sys/accounts/")
        .or_else(|| target.strip_prefix("sys/accounts/"))?;
    let rest = rest.trim_matches('/');
    let (provider, account) = rest.split_once('/')?;
    (!provider.is_empty() && !account.is_empty())
        .then(|| (provider.to_string(), account.to_string()))
}

/// The single-row payload's key column (`name`/`key`) as a non-empty string, or a structured
/// malformed-effect error naming the operation — the reconcile REMOVE ops carry a key-only row.
fn required_key(args: &RowBatch, col: &str, op: &str) -> Result<String, SysError> {
    arg_text(args, col).ok_or_else(|| SysError::MalformedEffect {
        reason: format!("{op} requires a non-empty `{col}`"),
    })
}

/// The single-row write payload's value for `col` as a non-empty string, if the batch carries it.
fn arg_text(args: &RowBatch, col: &str) -> Option<String> {
    let idx = col_index(args, col)?;
    match args.rows.first().and_then(|r| r.values.get(idx)) {
        Some(Value::Text(s)) if !s.is_empty() => Some(s.clone()),
        _ => None,
    }
}

/// The column index of `col` in a batch's schema, if present.
fn col_index(batch: &RowBatch, col: &str) -> Option<usize> {
    batch
        .schema
        .columns
        .iter()
        .position(|c| c.name.as_str() == col)
}

/// The stable `&'static str` label for an effect kind (the structured-error field is `&'static`).
fn static_verb_label(kind: &EffectKind) -> &'static str {
    match kind {
        EffectKind::Read => "READ",
        EffectKind::List => "LIST",
        EffectKind::Insert => "INSERT",
        EffectKind::Upsert => "UPSERT",
        EffectKind::Update => "UPDATE",
        EffectKind::Remove => "REMOVE",
        EffectKind::Call(_) => "CALL",
        _ => "WRITE",
    }
}

impl SharedApplier for SysApplier {
    fn apply_shared(&self, node: &EffectNode) -> Result<EffectOutput, EffectError> {
        let affected = self
            .apply_node(node)
            .map_err(|e| EffectError::terminal(e.to_string()))?;
        Ok(EffectOutput::new(node.id, affected))
    }
}

impl PlanApplier for SysApplier {
    /// The introspective `qfs_driver::Driver::applier()` seam (t09). Stateless, so it delegates to
    /// the same `&self` core as [`SharedApplier::apply_shared`]; the structured [`SysError`] is
    /// reduced to the plan crate's owned `(id, reason)` shape — secret-free by construction.
    fn apply(&mut self, node: &EffectNode) -> Result<AppliedEffect, ApplyError> {
        let affected = self
            .apply_node(node)
            .map_err(|e| ApplyError::new(node.id, e.to_string()))?;
        Ok(AppliedEffect::new(node.id, affected))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use qfs_plan::{DriverId, NodeId, Target, VfsPath};
    use qfs_types::{Column, ColumnType, RowBatch, Schema, Value};
    use std::sync::Mutex;

    use qfs_types::Row;

    /// An in-memory fake backend (no DB, no creds): records the policy rows it was asked to
    /// insert, so the applier's ROUTING can be proven without the binary's rusqlite impl.
    #[derive(Default)]
    struct FakeBackend {
        inserted: Mutex<Vec<RowBatch>>,
        removed: Mutex<Vec<String>>,
        /// (provider, account) rows that `scan(Accounts)` returns — the registry a filter-based
        /// `REMOVE /sys/accounts WHERE account == '<value>'` resolves the provider against.
        accounts: Mutex<Vec<(String, String)>>,
    }

    impl SysBackend for FakeBackend {
        fn scan(&self, node: SysNode) -> Result<RowBatch, SysError> {
            if matches!(node, SysNode::Accounts) {
                let schema = Schema::new(vec![
                    Column::new("provider", ColumnType::Text, false),
                    Column::new("account", ColumnType::Text, false),
                ]);
                let rows = self
                    .accounts
                    .lock()
                    .unwrap()
                    .iter()
                    .map(|(p, a)| Row::new(vec![Value::Text(p.clone()), Value::Text(a.clone())]))
                    .collect();
                return Ok(RowBatch::new(schema, rows));
            }
            Ok(RowBatch::new(Schema::new(vec![]), vec![]))
        }
        fn insert_policy(&self, row: &RowBatch) -> Result<u64, SysError> {
            self.inserted.lock().unwrap().push(row.clone());
            Ok(1)
        }
        fn set_setting(&self, row: &RowBatch) -> Result<u64, SysError> {
            self.inserted.lock().unwrap().push(row.clone());
            Ok(1)
        }
        fn set_billing(&self, row: &RowBatch) -> Result<u64, SysError> {
            self.inserted.lock().unwrap().push(row.clone());
            Ok(1)
        }
        fn upsert_binding(&self, row: &RowBatch) -> Result<u64, SysError> {
            self.inserted.lock().unwrap().push(row.clone());
            Ok(1)
        }
        fn remove_binding(&self, path: &str) -> Result<u64, SysError> {
            self.removed.lock().unwrap().push(path.to_string());
            Ok(1)
        }
        fn insert_driver(&self, row: &RowBatch) -> Result<u64, SysError> {
            self.inserted.lock().unwrap().push(row.clone());
            Ok(1)
        }
        fn record_account(&self, row: &RowBatch) -> Result<u64, SysError> {
            self.inserted.lock().unwrap().push(row.clone());
            Ok(1)
        }
        fn remove_account(&self, provider: &str, account: &str) -> Result<u64, SysError> {
            self.removed
                .lock()
                .unwrap()
                .push(format!("{provider}/{account}"));
            Ok(1)
        }
        fn update_policy(&self, row: &RowBatch) -> Result<u64, SysError> {
            self.inserted.lock().unwrap().push(row.clone());
            Ok(1)
        }
        fn remove_policy(&self, name: &str) -> Result<u64, SysError> {
            self.removed.lock().unwrap().push(format!("policy:{name}"));
            Ok(1)
        }
        fn remove_setting(&self, key: &str) -> Result<u64, SysError> {
            self.removed.lock().unwrap().push(format!("setting:{key}"));
            Ok(1)
        }
        fn remove_driver(&self, name: &str) -> Result<u64, SysError> {
            self.removed.lock().unwrap().push(format!("driver:{name}"));
            Ok(1)
        }
    }

    fn policy_row() -> RowBatch {
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

    fn effect(kind: EffectKind, path: &str, args: RowBatch) -> EffectNode {
        EffectNode::new(
            NodeId(0),
            kind,
            Target::new(DriverId::new("sys"), VfsPath::new(path)),
        )
        .with_args(args)
    }

    #[test]
    fn insert_into_sys_policies_routes_to_the_backend() {
        let backend = Arc::new(FakeBackend::default());
        let applier = SysApplier::new(backend.clone());
        let node = effect(EffectKind::Insert, "/sys/policies", policy_row());
        let out = applier.apply_shared(&node).expect("policy insert applies");
        assert_eq!(out.affected, 1);
        assert_eq!(
            backend.inserted.lock().unwrap().len(),
            1,
            "row reached backend"
        );
    }

    #[test]
    fn insert_into_sys_settings_routes_to_the_backend() {
        // t59: `INSERT INTO /sys/settings` (the safety-mode setter) routes to set_setting.
        let backend = Arc::new(FakeBackend::default());
        let applier = SysApplier::new(backend.clone());
        let schema = Schema::new(vec![
            Column::new("key", ColumnType::Text, false),
            Column::new("value", ColumnType::Text, false),
        ]);
        let row = RowBatch::new(
            schema,
            vec![Row::new(vec![
                Value::Text("safety_mode".into()),
                Value::Text("policy-only".into()),
            ])],
        );
        let node = effect(EffectKind::Insert, "/sys/settings", row);
        let out = applier
            .apply_shared(&node)
            .expect("settings upsert applies");
        assert_eq!(out.affected, 1);
        assert_eq!(
            backend.inserted.lock().unwrap().len(),
            1,
            "row reached backend"
        );
    }

    #[test]
    fn insert_into_sys_billing_routes_to_the_backend() {
        // t67: `INSERT INTO /sys/billing` (the tier recorder) routes to set_billing.
        let backend = Arc::new(FakeBackend::default());
        let applier = SysApplier::new(backend.clone());
        let schema = Schema::new(vec![
            Column::new("team_id", ColumnType::Text, false),
            Column::new("tier", ColumnType::Text, false),
            Column::new("status", ColumnType::Text, false),
        ]);
        let row = RowBatch::new(
            schema,
            vec![Row::new(vec![
                Value::Text("team-acme".into()),
                Value::Text("paid-team".into()),
                Value::Text("active".into()),
            ])],
        );
        let node = effect(EffectKind::Insert, "/sys/billing", row);
        let out = applier.apply_shared(&node).expect("billing upsert applies");
        assert_eq!(out.affected, 1);
        assert_eq!(
            backend.inserted.lock().unwrap().len(),
            1,
            "row reached backend"
        );
    }

    #[test]
    fn upsert_into_sys_paths_routes_to_the_binding_backend() {
        // t100020: `CONNECT` desugars to `UPSERT INTO /sys/paths` — routes to upsert_binding.
        let backend = Arc::new(FakeBackend::default());
        let applier = SysApplier::new(backend.clone());
        let schema = Schema::new(vec![
            Column::new("path", ColumnType::Text, false),
            Column::new("driver", ColumnType::Text, true),
        ]);
        let row = RowBatch::new(
            schema,
            vec![Row::new(vec![
                Value::Text("/work/orders".into()),
                Value::Text("postgres".into()),
            ])],
        );
        let node = effect(EffectKind::Upsert, "/sys/paths", row);
        let out = applier.apply_shared(&node).expect("binding upsert applies");
        assert_eq!(out.affected, 1);
        assert_eq!(
            backend.inserted.lock().unwrap().len(),
            1,
            "row reached backend"
        );
    }

    #[test]
    fn remove_on_sys_paths_reconstructs_the_multi_segment_path() {
        // t100020: `DISCONNECT /work/orders` desugars to `REMOVE /sys/paths/work/orders` — the
        // applier reconstructs the user path from the segments AFTER `paths`.
        let backend = Arc::new(FakeBackend::default());
        let applier = SysApplier::new(backend.clone());
        let node = effect(
            EffectKind::Remove,
            "/sys/paths/work/orders",
            RowBatch::new(Schema::new(vec![]), vec![]),
        );
        applier.apply_shared(&node).expect("binding remove applies");
        assert_eq!(
            backend.removed.lock().unwrap().as_slice(),
            &["/work/orders".to_string()]
        );
    }

    #[test]
    fn insert_into_sys_accounts_routes_to_record_account() {
        // 20260703040000: `CREATE ACCOUNT` desugars to `INSERT INTO /sys/accounts` — routes to
        // record_account (which records consent, gated, sharing the CLI writer).
        let backend = Arc::new(FakeBackend::default());
        let applier = SysApplier::new(backend.clone());
        let schema = Schema::new(vec![
            Column::new("provider", ColumnType::Text, false),
            Column::new("account", ColumnType::Text, false),
        ]);
        let row = RowBatch::new(
            schema,
            vec![Row::new(vec![
                Value::Text("github".into()),
                Value::Text("work".into()),
            ])],
        );
        let node = effect(EffectKind::Insert, "/sys/accounts", row);
        let out = applier
            .apply_shared(&node)
            .expect("account declare applies");
        assert_eq!(out.affected, 1);
        assert_eq!(
            backend.inserted.lock().unwrap().len(),
            1,
            "row reached backend"
        );
    }

    #[test]
    fn remove_on_sys_accounts_reconstructs_provider_and_account() {
        // 20260703040000: `REMOVE /sys/accounts/github/work` — the applier reconstructs
        // `(provider, account)` from the segments after `accounts`.
        let backend = Arc::new(FakeBackend::default());
        let applier = SysApplier::new(backend.clone());
        let node = effect(
            EffectKind::Remove,
            "/sys/accounts/github/work",
            RowBatch::new(Schema::new(vec![]), vec![]),
        );
        applier.apply_shared(&node).expect("account remove applies");
        assert_eq!(
            backend.removed.lock().unwrap().as_slice(),
            &["github/work".to_string()]
        );
    }

    /// Build a `REMOVE … WHERE` row: each `(col, value)` is an equality the evaluator lowers into
    /// the effect's single-row payload (via `setwhere_row_batch`).
    fn filter_row(cols: &[(&str, &str)]) -> RowBatch {
        let schema = Schema::new(
            cols.iter()
                .map(|(n, _)| Column::new(*n, ColumnType::Text, true))
                .collect(),
        );
        let vals = cols.iter().map(|(_, v)| Value::Text((*v).into())).collect();
        RowBatch::new(schema, vec![Row::new(vals)])
    }

    /// A filter-addressed REMOVE: the `WHERE` rides the SELECTOR channel (§7) and `args` stays
    /// empty, because a REMOVE writes nothing.
    fn filter_effect(path: &str, cols: &[(&str, &str)]) -> EffectNode {
        EffectNode::new(
            NodeId(0),
            EffectKind::Remove,
            Target::new(DriverId::new("sys"), VfsPath::new(path)),
        )
        .with_selector(filter_row(cols))
    }

    #[test]
    fn remove_via_filter_resolves_the_provider_from_the_registry() {
        // The concern's edge case: a Google account whose label is an email cannot ride a path
        // (`@` is a version coordinate the router drops), so `REMOVE /sys/accounts WHERE account ==
        // '<email>'` carries the email as DATA. The applier resolves provider=google from the
        // registry and removes it — the email survives intact.
        let backend = Arc::new(FakeBackend {
            accounts: Mutex::new(vec![("google".into(), "you@example.com".into())]),
            ..Default::default()
        });
        let applier = SysApplier::new(backend.clone());
        let node = filter_effect("/sys/accounts", &[("account", "you@example.com")]);
        applier.apply_shared(&node).expect("filter remove applies");
        assert_eq!(
            backend.removed.lock().unwrap().as_slice(),
            &["google/you@example.com".to_string()]
        );
    }

    #[test]
    fn remove_via_filter_with_explicit_provider_skips_resolution() {
        // `WHERE provider == 'github' AND account == 'work'` names the pair directly — no registry
        // scan needed (and none available: the accounts fixture is empty).
        let backend = Arc::new(FakeBackend::default());
        let applier = SysApplier::new(backend.clone());
        let node = filter_effect(
            "/sys/accounts",
            &[("provider", "github"), ("account", "work")],
        );
        applier.apply_shared(&node).expect("filter remove applies");
        assert_eq!(
            backend.removed.lock().unwrap().as_slice(),
            &["github/work".to_string()]
        );
    }

    #[test]
    fn remove_via_filter_fails_closed_when_ambiguous_absent_or_unmatched() {
        // Ambiguous (one label under two providers), no account column, and no matching row all
        // fail closed with a secret-free, actionable error — never a wrong-row delete.
        let backend = Arc::new(FakeBackend {
            accounts: Mutex::new(vec![
                ("github".into(), "x".into()),
                ("slack".into(), "x".into()),
            ]),
            ..Default::default()
        });
        let applier = SysApplier::new(backend.clone());

        let ambiguous = effect(
            EffectKind::Remove,
            "/sys/accounts",
            filter_row(&[("account", "x")]),
        );
        assert!(
            applier.apply_shared(&ambiguous).is_err(),
            "an account under multiple providers is rejected, not guessed"
        );

        let unmatched = effect(
            EffectKind::Remove,
            "/sys/accounts",
            filter_row(&[("account", "ghost")]),
        );
        assert!(
            applier.apply_shared(&unmatched).is_err(),
            "an account with no registry row is rejected"
        );

        let no_account = effect(
            EffectKind::Remove,
            "/sys/accounts",
            RowBatch::new(Schema::new(vec![]), vec![]),
        );
        assert!(
            applier.apply_shared(&no_account).is_err(),
            "a bare REMOVE with no path and no account filter is rejected"
        );

        assert!(
            backend.removed.lock().unwrap().is_empty(),
            "nothing was removed on any rejected filter"
        );
    }

    #[test]
    fn reconcile_update_and_remove_route_to_the_new_sys_seams() {
        // The provisioning reconcile (blueprint §16) drives UPDATE/REMOVE against the /sys
        // config collections; each routes to its dedicated backend seam.
        let backend = Arc::new(FakeBackend::default());
        let applier = SysApplier::new(backend.clone());

        // UPDATE /sys/policies (a drifted grant) → update_policy.
        applier
            .apply_shared(&effect(EffectKind::Update, "/sys/policies", policy_row()))
            .expect("policy update applies");
        assert_eq!(backend.inserted.lock().unwrap().len(), 1);

        // REMOVE /sys/{policies,settings,drivers} — the key rides in the row.
        let key_row = |col: &str, val: &str| {
            RowBatch::new(
                Schema::new(vec![Column::new(col, ColumnType::Text, false)]),
                vec![Row::new(vec![Value::Text(val.into())])],
            )
        };
        applier
            .apply_shared(&effect(
                EffectKind::Remove,
                "/sys/policies",
                key_row("name", "analysts"),
            ))
            .expect("policy remove applies");
        applier
            .apply_shared(&effect(
                EffectKind::Remove,
                "/sys/settings",
                key_row("key", "theme"),
            ))
            .expect("setting remove applies");
        applier
            .apply_shared(&effect(
                EffectKind::Remove,
                "/sys/drivers",
                key_row("name", "chatwork"),
            ))
            .expect("driver remove applies");
        assert_eq!(
            backend.removed.lock().unwrap().as_slice(),
            &[
                "policy:analysts".to_string(),
                "setting:theme".to_string(),
                "driver:chatwork".to_string(),
            ]
        );
    }

    #[test]
    fn update_or_remove_on_audit_is_rejected_in_the_applier() {
        // Belt-and-suspenders over the parse-time gate: even a hand-built plan cannot mutate the
        // append-only audit log (or any read-only admin view).
        let applier = SysApplier::new(Arc::new(FakeBackend::default()));
        for (kind, path) in [
            (EffectKind::Update, "/sys/audit"),
            (EffectKind::Remove, "/sys/audit"),
            (EffectKind::Insert, "/sys/users"),
            (EffectKind::Insert, "/sys/connections"),
        ] {
            let node = effect(kind, path, RowBatch::new(Schema::new(vec![]), vec![]));
            assert!(
                applier.apply_shared(&node).is_err(),
                "{path} must reject a write in the applier"
            );
        }
    }
}
