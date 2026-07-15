//! The **diff→plan builder** (blueprint §16, Decision X): one [`ReconcilePlan`] rendered as one
//! batch [`Plan`] of effect nodes, ready for `preview()` and `commit()`.
//!
//! Topology is flat (config writes are independent single-row effects; apply order is the
//! deterministic diff order: `/sys` foundations first, then `/server`). Every **destroy** node
//! carries the `irreversible` flag — a `/sys` `Remove` inherently
//! ([`EffectKind::is_inherently_irreversible`]), a `/server` `Remove` explicitly (the
//! `ServerConfigWrite` kind is reversible by default, but an *authoritative* remove destroys a
//! declared binding) — so [`qfs_core::preview`] calls the destroys out and the
//! [`qfs_core::IrreversibleGuard`] gates the apply behind the ack (increment 3's
//! `--commit-irreversible`).

use qfs_core::{
    Affected, DriverId, EffectKind, EffectNode, Plan, PlanBuilder, ServerWriteOp, Target, VfsPath,
};

use crate::diff::{ReconcileNode, ReconcileOp, ReconcilePlan};
use crate::state::SysCollection;

/// Build the batch [`Plan`] for a reconcile — one effect node per [`ReconcileOp`], in the
/// plan's deterministic order.
///
/// # Errors
/// A secret-free string if an op's payload cannot be built (unreachable for diffs produced by
/// [`crate::diff`]).
pub fn build_plan(plan: &ReconcilePlan) -> Result<Plan, String> {
    let mut builder = PlanBuilder::new();
    for op in plan.ops() {
        let node = effect_node(&mut builder, op)?;
        builder.push(node);
    }
    Ok(builder.build())
}

/// Render one [`ReconcileOp`] as its effect node.
fn effect_node(builder: &mut PlanBuilder, op: &ReconcileOp) -> Result<EffectNode, String> {
    let args = op.row_batch()?;
    match op.node {
        ReconcileNode::Server(snode) => {
            let target = Target::new(DriverId::new("server"), VfsPath::new(snode.path()));
            Ok(EffectNode::new(
                builder.next_id(),
                EffectKind::ServerConfigWrite {
                    node: snode,
                    op: op.op,
                },
                target,
            )
            .with_args(args)
            .with_affected(Affected::Exact(1))
            // A ServerConfigWrite is reversible by default (a removed row can be re-inserted by
            // hand) — but an AUTHORITATIVE remove destroys a declared binding, so the reconcile
            // plan flags it for preview() and the IrreversibleGuard (blueprint §16).
            .irreversible(matches!(op.op, ServerWriteOp::Remove)))
        }
        ReconcileNode::Sys(coll) => {
            let (kind, path) = sys_effect_coordinates(coll, op);
            let target = Target::new(DriverId::new("sys"), VfsPath::new(path));
            Ok(EffectNode::new(builder.next_id(), kind, target)
                .with_args(args)
                .with_affected(Affected::Exact(1)))
        }
    }
}

/// The `(EffectKind, target path)` of a `/sys` reconcile op — matched to the verbs the
/// [`qfs_driver_sys::SysApplier`] routes:
///
/// - Settings / paths / drivers apply upsert-on-key semantics, so an `Update` rides as
///   [`EffectKind::Upsert`] and an `Insert` as [`EffectKind::Insert`] (both reach the same
///   backend seam).
/// - `/sys/policies` `Update` rides as [`EffectKind::Update`] and `Remove` as
///   [`EffectKind::Remove`] — both reach the dedicated sys seams (the reconcile UPDATE/REMOVE
///   writers).
/// - `/sys/drivers` is **install/uninstall only** (matching its capability posture): an `Insert`
///   installs and a `Remove` uninstalls, but a driver *edit* (`Update`) is encoded honestly as
///   [`EffectKind::Update`] and refused by the applier (change a driver by removing + re-adding).
/// - A path-binding `Remove` addresses the binding by path segments
///   (`REMOVE /sys/paths<binding-path>`), the `DISCONNECT` twin the applier reconstructs.
///
/// [`EffectKind::Remove`] is inherently irreversible, so every `/sys` destroy is flagged for
/// `preview()` without an explicit builder call.
fn sys_effect_coordinates(coll: SysCollection, op: &ReconcileOp) -> (EffectKind, String) {
    let kind = match op.op {
        ServerWriteOp::Remove => EffectKind::Remove,
        ServerWriteOp::Update => match coll {
            // Upsert-on-key collections: a config change is a replace-by-key. `/transform` is
            // upsert-on-name in the applier, so a changed definition reconciles as an UPSERT.
            SysCollection::Settings | SysCollection::Paths | SysCollection::Transforms => {
                EffectKind::Upsert
            }
            // Policies have a real UPDATE seam; drivers do not (install/uninstall only) and the
            // applier refuses a driver Update — both encoded honestly as EffectKind::Update.
            SysCollection::Policies | SysCollection::Drivers => EffectKind::Update,
        },
        ServerWriteOp::Insert | ServerWriteOp::Upsert => EffectKind::Insert,
    };
    let path = match (coll, op.op) {
        // DISCONNECT twin: the binding path (`/chat`) rides as segments after `paths`.
        (SysCollection::Paths, ServerWriteOp::Remove) => format!("/sys/paths{}", op.name),
        // REMOVE /transform/<name>: the definition name rides as the segment after `transform`
        // (the applier reconstructs it), so the reconcile destroy addresses the item path.
        (SysCollection::Transforms, ServerWriteOp::Remove) => format!("/transform/{}", op.name),
        _ => coll.path(),
    };
    (kind, path)
}
