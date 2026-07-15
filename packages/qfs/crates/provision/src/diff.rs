//! The **diff engine** (blueprint §16, Decision X; §5 drift-is-set-difference): desired-vs-current
//! reduced to a Terraform-style add/change/destroy plan across **both** config stores.
//!
//! Per collection the diff is the set difference on the **config projection** (runtime fields and
//! secretish settings excluded, so a materialized-view refresh — or a secret rotation — is never
//! drift): a name present only in `desired` is an [`ServerWriteOp::Insert`], a name present in
//! both whose projection differs is an [`ServerWriteOp::Update`], a name present only in
//! `current` is an [`ServerWriteOp::Remove`], and an equal name is no op. Identical states diff
//! to an **empty** plan (idempotent apply).
//!
//! Policies are keyed by name **within their store**: `/server/policies` and `sys_policies` are
//! two independent collections ([`ReconcileNode::Server`]`(Policies)` vs
//! [`ReconcileNode::Sys`]`(Policies)`) — the same name diffs independently in each. The excluded
//! collections (billing, `sys_ddl_events`, secretish settings) are outside the diff universe
//! entirely, so an authoritative destroy can never touch them.

use std::collections::{BTreeMap, BTreeSet};

use qfs_core::{RowBatch, ServerNode, ServerWriteOp};

use crate::proj::{collection_projs, name_only, ProjRow, SERVER_NODES};
use crate::state::{sys_collection_projs, ConfigState, SysCollection, SYS_COLLECTIONS};

/// Which configuration store a [`ReconcileOp`] targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ConfigStore {
    /// The running daemon's `/server` self-config store.
    Server,
    /// The system/project-DB `/sys` store.
    Sys,
}

/// The store+collection coordinate of a reconcile op — the two policy collections are distinct
/// variants here, so a policy name can never be conflated across stores.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ReconcileNode {
    /// A `/server/<node>` collection.
    Server(ServerNode),
    /// A `/sys` collection (dump-covered; the exclusions have no variant).
    Sys(SysCollection),
}

impl ReconcileNode {
    /// The store this coordinate belongs to.
    #[must_use]
    pub const fn store(self) -> ConfigStore {
        match self {
            Self::Server(_) => ConfigStore::Server,
            Self::Sys(_) => ConfigStore::Sys,
        }
    }
}

/// One reconcile step: the store/collection coordinate, the write op, the row key, and the
/// desired config projection to write. For a [`ServerWriteOp::Remove`] the projection is the
/// row key only (all a delete needs).
#[derive(Debug, Clone, PartialEq)]
pub struct ReconcileOp {
    /// The store+collection coordinate.
    pub node: ReconcileNode,
    /// The precise write verb (`Insert`/`Update`/`Remove` — never `Upsert`; the diff is exact).
    pub op: ServerWriteOp,
    /// The affected row key (`name` / setting `key` / binding `path`).
    pub name: String,
    /// The desired config projection to write (key-only for a `Remove`).
    pub proj: ProjRow,
}

impl ReconcileOp {
    /// The store this op targets.
    #[must_use]
    pub const fn store(&self) -> ConfigStore {
        self.node.store()
    }

    /// Build the write payload for this op — what the plan builder feeds the effect node.
    /// `/server` payloads are schema-ordered ([`ProjRow::to_row_batch`]); `/sys` payloads are
    /// name-addressed ([`ProjRow::to_named_batch`], the backend reads columns by name).
    ///
    /// # Errors
    /// A secret-free string if a `/server` projection carries an undeclared column (unreachable
    /// for diffs produced by [`diff`], which project through the schema-faithful converters).
    pub fn row_batch(&self) -> Result<RowBatch, String> {
        match self.node {
            ReconcileNode::Server(node) => self.proj.to_row_batch(node),
            ReconcileNode::Sys(_) => Ok(self.proj.to_named_batch()),
        }
    }
}

/// A reconcile plan: the ordered ops plus the add/change/destroy summary the CLI renders.
/// Walk order is fixed: the `/sys` collections first (the foundation a `/server` binding may
/// reference), then the `/server` collections — each in fixed collection order, then row-key
/// order. Deterministic.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ReconcilePlan {
    ops: Vec<ReconcileOp>,
}

impl ReconcilePlan {
    /// The reconcile ops, in deterministic order.
    #[must_use]
    pub fn ops(&self) -> &[ReconcileOp] {
        &self.ops
    }

    /// Whether the plan is a no-op (desired already equals current).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }

    /// The number of `Insert` (add) ops.
    #[must_use]
    pub fn add_count(&self) -> usize {
        self.count(ServerWriteOp::Insert)
    }

    /// The number of `Update` (change) ops.
    #[must_use]
    pub fn change_count(&self) -> usize {
        self.count(ServerWriteOp::Update)
    }

    /// The number of `Remove` (destroy) ops.
    #[must_use]
    pub fn destroy_count(&self) -> usize {
        self.count(ServerWriteOp::Remove)
    }

    /// Whether the plan contains **any** destroy — the flag `apply` gates behind
    /// `--commit-irreversible` (increment 3).
    #[must_use]
    pub fn has_destroy(&self) -> bool {
        self.ops.iter().any(|o| o.op == ServerWriteOp::Remove)
    }

    /// Split the plan into its two store halves `(sys, server)`, each preserving the
    /// deterministic op order — the apply path drives the `/sys` half through the local
    /// dispatching applier and the `/server` half statement-by-statement through the daemon's
    /// statement bridge (blueprint §16 "The face, named").
    #[must_use]
    pub fn split_stores(&self) -> (ReconcilePlan, ReconcilePlan) {
        let (sys, server) = self
            .ops
            .iter()
            .cloned()
            .partition(|o| o.store() == ConfigStore::Sys);
        (ReconcilePlan { ops: sys }, ReconcilePlan { ops: server })
    }

    fn count(&self, op: ServerWriteOp) -> usize {
        self.ops.iter().filter(|o| o.op == op).count()
    }
}

/// Diff the desired config projection against the current one, per collection across both
/// stores, into a [`ReconcilePlan`]. Equality is on the config projection (runtime fields and
/// secretish settings excluded), so cosmetic source differences, view refreshes, and secret
/// rotations plan to **zero** changes.
#[must_use]
pub fn diff(current: &ConfigState, desired: &ConfigState) -> ReconcilePlan {
    let mut ops = Vec::new();
    for coll in SYS_COLLECTIONS {
        diff_collection(
            ReconcileNode::Sys(coll),
            &sys_collection_projs(&current.sys, coll),
            &sys_collection_projs(&desired.sys, coll),
            coll.key_column(),
            &mut ops,
        );
    }
    for node in SERVER_NODES {
        diff_collection(
            ReconcileNode::Server(node),
            &collection_projs(&current.server, node),
            &collection_projs(&desired.server, node),
            "name",
            &mut ops,
        );
    }
    ReconcilePlan { ops }
}

/// Diff one collection into `ops`, walking the union of row keys in sorted order. `key_column`
/// names the column a `Remove` op's key-only projection carries.
fn diff_collection(
    node: ReconcileNode,
    cur: &BTreeMap<String, ProjRow>,
    des: &BTreeMap<String, ProjRow>,
    key_column: &str,
    ops: &mut Vec<ReconcileOp>,
) {
    let names: BTreeSet<&String> = cur.keys().chain(des.keys()).collect();
    for name in names {
        match (cur.get(name), des.get(name)) {
            // New in desired ⇒ Insert.
            (None, Some(want)) => ops.push(ReconcileOp {
                node,
                op: ServerWriteOp::Insert,
                name: name.clone(),
                proj: want.clone(),
            }),
            // Present in both but the projection drifted ⇒ Update.
            (Some(have), Some(want)) if have != want => ops.push(ReconcileOp {
                node,
                op: ServerWriteOp::Update,
                name: name.clone(),
                proj: want.clone(),
            }),
            // Present in both and equal ⇒ no op.
            (Some(_), Some(_)) => {}
            // Absent in desired ⇒ Remove (authoritative destroy). Key-only payload.
            (Some(_), None) => ops.push(ReconcileOp {
                node,
                op: ServerWriteOp::Remove,
                name: name.clone(),
                proj: key_only(key_column, name),
            }),
            // Unreachable: a name in the union came from at least one side.
            (None, None) => {}
        }
    }
}

/// A key-only projection under the collection's key column (`name`/`key`/`path`).
fn key_only(key_column: &str, name: &str) -> ProjRow {
    if key_column == "name" {
        return name_only(name);
    }
    let mut row = ProjRow::default();
    row.set_text(key_column, name);
    row
}
