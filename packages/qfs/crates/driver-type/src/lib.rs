//! `qfs-driver-type` — the **declared-type catalog driver** (blueprint §5.4/§5.5).
//!
//! A declared type is DATA (declare → store → reference). [`TypeDriver`] exposes the type namespace
//! as the ordinary qfs relation `/type`: `SELECT`/`ls /type` **is SHOW TYPES**, and `DESCRIBE
//! /type/<name>` teaches a declared type's shape — the inspection surface an operator (or an agent)
//! reads before writing `of customer`.
//!
//! ## The catalog is not the reference (§5.5)
//! `/type` is the **catalog/shell face**, never a reference site: a type is referenced by NAME
//! (`of customer`, a column type `email email`, `create type customer`), and the pipe and the DDL
//! never apply a path. The Unix analogy is exact — definitions are *stored* at catalog paths
//! (`/usr/bin/grep`) and *invoked* by name (`grep`). So this mount answers `ls`/`describe`/`SELECT`
//! and nothing else.
//!
//! ## Read-only, and why
//! Unlike `/transform` (whose registry IS its own table, so `INSERT`/`REMOVE` land there), a
//! declared type's rows live in the declared-driver registry `/sys/drivers` (`kind='type'`) — the
//! one table `CREATE TYPE` desugars into. Install/update/remove therefore stay ordinary previewed
//! writes to **that** path, and `/type` is the read face over the same rows. Exposing `INSERT INTO
//! /type` would mint a second, competing write path to one table, so [`Capabilities`] here is
//! `SELECT` only and [`Driver::applier`] is a [`NoopApplier`] that is never reached.
//!
//! ## Purity
//! The introspective half ([`Driver::describe`]/[`Driver::capabilities`]) is **pure** — a stable,
//! credential-free schema (see [`type_node_schema`]) with NO DB and NO secrets — mirroring the
//! `/transform` and `/sys` split. The System-DB read source (the `sys_drivers` scan) is INJECTED
//! from the binary, the one place that opens a real DB path (decision F), so this crate stays
//! tokio-free and DB-free.

#![forbid(unsafe_code)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

mod schema;

use qfs_driver::{Archetype, Capabilities, Driver, NodeDesc, Path, ProcSig, PushdownProfile, Verb};
use qfs_plan::PlanApplier;

pub use schema::{name_from_path, node_for_path, type_node_schema, TypeNode, TYPE_MOUNT};

/// The declared-type catalog driver. Pure introspection only — it owns NO state and NO backend (the
/// read source is injected from the binary). Construct with [`TypeDriver::new`].
pub struct TypeDriver {
    // A small System-DB-backed catalog read in-engine; it pushes nothing down.
    pushdown: PushdownProfile,
    procs: Vec<ProcSig>,
}

impl Default for TypeDriver {
    fn default() -> Self {
        Self::new()
    }
}

impl TypeDriver {
    /// Construct the (pure) declared-type catalog driver.
    #[must_use]
    pub fn new() -> Self {
        Self {
            pushdown: PushdownProfile::None,
            procs: Vec::new(),
        }
    }
}

/// The `/type` capability set (blueprint §6): **SELECT only** — the catalog is a read face. A type is
/// installed/removed by a previewed write to `/sys/drivers` (`kind='type'`), which is where `CREATE
/// TYPE` desugars; `/type` never mints a second write path to those rows (§5.5). Single source of
/// truth shared by [`Driver::capabilities`] and the parse-time verb gate — so `INSERT INTO /type …`
/// and `REMOVE /type/<name>` are refused structurally, at parse time, with a capability error.
#[must_use]
pub fn type_node_capabilities(_node: TypeNode) -> Capabilities {
    Capabilities::from_verbs(&[Verb::Select])
}

impl Driver for TypeDriver {
    fn mount(&self) -> &str {
        TYPE_MOUNT
    }

    fn describe(&self, path: &Path) -> Result<NodeDesc, qfs_driver::CfsError> {
        // Pure: returns static schema data; never touches a DB or a credential.
        let node =
            node_for_path(path.as_str()).ok_or_else(|| qfs_driver::CfsError::UnsupportedVerb {
                path: path.as_str().to_string(),
                verb: "DESCRIBE",
                supported: Vec::new(),
            })?;
        // The catalog ROOT is a navigable interior (§9): its children are the declared types —
        // locations you can enter. An ITEM (`/type/<name>`) is a row leaf: `describe` it, don't
        // enter it. Both carry `RelationalTable`, so the archetype cannot tell them apart — this
        // per-node fact is what the `cd` gate reads.
        Ok(
            NodeDesc::new(Archetype::RelationalTable, type_node_schema(node))
                .navigable(name_from_path(path.as_str()).is_none())
                // §5.5: these rows ARE definitions — referenced by name (`of customer`), never by
                // this path. Copying data rows in here is a category error, not a schema mismatch.
                .category(qfs_driver::NodeCategory::Definition),
        )
    }

    fn capabilities(&self, path: &Path) -> Capabilities {
        match node_for_path(path.as_str()) {
            Some(node) => type_node_capabilities(node),
            None => Capabilities::none(),
        }
    }

    fn procedures(&self) -> &[ProcSig] {
        &self.procs
    }

    fn pushdown(&self) -> &PushdownProfile {
        &self.pushdown
    }

    fn applier(&self) -> &dyn PlanApplier {
        // The catalog is read-only: no verb reaches an apply. This exists only so the driver
        // satisfies the introspective trait without pretending to own an impure seam.
        &NoopApplier
    }
}

/// A no-op applier for the `Driver::applier()` contract slot (mirrors `TransformDriver`'s). `/type`
/// exposes no mutating verb, so this is unreachable in practice.
struct NoopApplier;

impl PlanApplier for NoopApplier {
    fn apply(
        &mut self,
        node: &qfs_plan::EffectNode,
    ) -> Result<qfs_plan::AppliedEffect, qfs_plan::ApplyError> {
        Ok(qfs_plan::AppliedEffect::new(node.id, 0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use qfs_driver::check_capability;
    use std::sync::Arc;

    #[test]
    fn describe_type_is_pure_no_db_no_creds() {
        let d = TypeDriver::new();
        assert_eq!(d.mount(), "/type");

        let cat = d.describe(&Path::new("/type")).unwrap();
        assert_eq!(cat.archetype, Archetype::RelationalTable);
        assert!(cat.schema.column("name").is_some());
        assert!(cat.schema.column("columns").is_some());
        assert!(cat.schema.column("refinement").is_some());
        // A declared type is declarative data — no column a credential could ride in.
        assert!(cat.schema.column("secret").is_none());
        assert!(cat.schema.column("secret_ref").is_none());
        assert!(cat.schema.column("token").is_none());

        // /type/<name> also resolves (the item/DESCRIBE form that teaches a shape).
        assert!(d.describe(&Path::new("/type/customer")).is_ok());
        // A foreign path is not describable (no panic).
        assert!(d.describe(&Path::new("/sys/nope")).is_err());
    }

    #[test]
    fn capabilities_are_select_only_the_catalog_is_not_a_write_path() {
        let d = TypeDriver::new();
        let p = Path::new("/type");
        assert!(check_capability(&d, &p, Verb::Select).is_ok());
        // §5.5: install/update/remove are previewed writes to /sys/drivers (kind='type'), never a
        // second write path through the catalog mount.
        assert!(check_capability(&d, &p, Verb::Insert).is_err());
        assert!(check_capability(&d, &p, Verb::Update).is_err());
        assert!(check_capability(&d, &p, Verb::Remove).is_err());
    }

    #[test]
    fn the_catalog_root_is_navigable_but_an_item_is_a_leaf() {
        // §9 enumerable-children conformance: `cd /type` enters the catalog (its children are the
        // declared types — locations); `/type/customer` is a row leaf you describe, not enter. Both
        // report the SAME archetype, so this per-node fact is the only thing that separates them.
        let d = TypeDriver::new();
        let root = d.describe(&Path::new("/type")).unwrap();
        let item = d.describe(&Path::new("/type/customer")).unwrap();
        assert_eq!(root.archetype, item.archetype);
        assert!(root.navigable, "cd /type must enter the catalog");
        assert!(
            !item.navigable,
            "a declared type is a leaf, not an interior"
        );
        // A qualified name is an item too — not a catalog to enter.
        assert!(
            !d.describe(&Path::new("/type/chatwork/message"))
                .unwrap()
                .navigable
        );
    }

    #[test]
    fn driver_is_object_safe() {
        let d: Arc<dyn Driver> = Arc::new(TypeDriver::new());
        assert_eq!(d.mount(), "/type");
        let _seam: &dyn PlanApplier = d.applier();
    }
}
