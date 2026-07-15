//! `qfs-driver-transform` — the **transform-definition driver** (blueprint §15, decision W).
//!
//! A transform predicate is DATA (declare → store → activate). [`TransformDriver`] exposes the
//! definition registry as the ordinary qfs relation `/transform`: `SELECT`/`ls /transform` lists
//! definitions, `DESCRIBE /transform` reports the registry schema, `INSERT INTO /transform` creates
//! (the desugar target of `CREATE TRANSFORM`), and `REMOVE /transform/<name>` deletes (the desugar
//! target of `REMOVE TRANSFORM <name>`, inherently irreversible) — **one engine, three faces**.
//!
//! ## The same split as the `/sys` administration driver
//! [`TransformDriver`]'s **introspective** half ([`Driver::describe`]/[`Driver::capabilities`]) is
//! **pure** — a stable, credential-free schema (see [`transform_node_schema`]) with NO DB and NO
//! secrets — and its `applier()` is a [`NoopApplier`]. The real mutation lands in a runtime
//! [`TransformApplier`] over the injected [`TransformBackend`] (binary-side rusqlite over the
//! System DB's `sys_transforms` table), bridged via [`transform_apply_driver`]. The crate stays
//! tokio-free and DB-free; the binary leaf is the one place that opens a real DB path (decision F).
//!
//! ## Safety floor
//! - The `secret_ref` column is a REFERENCE (`env:`/`vault:`), never a value — resolved lazily at
//!   COMMIT by the executor (a later ticket), never here, never at DESCRIBE.
//! - The cardinality `mode` is DERIVED from the declared INPUT (never a stored flag).
//! - `REMOVE /transform/<name>` is inherently irreversible (an `EffectKind::Remove` plan node), so
//!   it rides the standard irreversible acknowledgement gate with no per-driver flag.

#![forbid(unsafe_code)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

mod applier;
mod backend;
mod provider;
mod schema;

use std::sync::Arc;

use qfs_driver::{Archetype, Capabilities, Driver, NodeDesc, Path, ProcSig, PushdownProfile, Verb};
use qfs_plan::PlanApplier;
use qfs_runtime::PlanApplierBridge;

pub use applier::TransformApplier;
pub use backend::{TransformBackend, TransformError};
pub use provider::{
    call_model, CallProof, ModelError, ModelProvider, ModelRequest, UnconfiguredProvider,
};
pub use schema::{
    name_from_path, node_for_path, transform_node_schema, TransformNode, TRANSFORM_MOUNT,
};

/// The transform-definition driver. Pure introspection only — it owns NO state and NO backend (the
/// read source + the mutation applier are injected from the binary). Construct with
/// [`TransformDriver::new`].
pub struct TransformDriver {
    // The definition registry is a small System-DB table read in-engine; it pushes nothing down.
    pushdown: PushdownProfile,
    procs: Vec<ProcSig>,
}

impl Default for TransformDriver {
    fn default() -> Self {
        Self::new()
    }
}

impl TransformDriver {
    /// Construct the (pure) transform-definition driver.
    #[must_use]
    pub fn new() -> Self {
        Self {
            pushdown: PushdownProfile::None,
            procs: Vec::new(),
        }
    }
}

/// The `/transform` capability set (blueprint §6): SELECT to list/describe, INSERT to create
/// (upsert-on-name), REMOVE to delete (irreversible). No UPDATE — a definition is install/uninstall
/// (remove and re-create to change one), mirroring the declared-driver registry. Single source of
/// truth shared by [`Driver::capabilities`] and the parse-time verb gate.
#[must_use]
pub fn transform_node_capabilities(_node: TransformNode) -> Capabilities {
    Capabilities::from_verbs(&[Verb::Select, Verb::Insert, Verb::Remove])
}

impl Driver for TransformDriver {
    fn mount(&self) -> &str {
        TRANSFORM_MOUNT
    }

    fn describe(&self, path: &Path) -> Result<NodeDesc, qfs_driver::CfsError> {
        // Pure: returns static schema data; never touches a DB or a credential.
        let node =
            node_for_path(path.as_str()).ok_or_else(|| qfs_driver::CfsError::UnsupportedVerb {
                path: path.as_str().to_string(),
                verb: "DESCRIBE",
                supported: Vec::new(),
            })?;
        // The registry ROOT is a navigable interior (§9): its children are the definitions —
        // locations. An ITEM (`/transform/<name>`) is a row leaf. Both carry `RelationalTable`, so
        // the archetype cannot tell them apart; this per-node fact is what the `cd` gate reads.
        Ok(
            NodeDesc::new(Archetype::RelationalTable, transform_node_schema(node))
                .navigable(name_from_path(path.as_str()).is_none())
                // §5.5: these rows ARE definitions — a `|> transform triage` names one, never a
                // path. Copying data rows in here is a category error, not a schema mismatch.
                .category(qfs_driver::NodeCategory::Definition),
        )
    }

    fn capabilities(&self, path: &Path) -> Capabilities {
        match node_for_path(path.as_str()) {
            Some(node) => transform_node_capabilities(node),
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
        // Like SysDriver: the real apply path is the runtime's TransformApplier (holding the
        // injected backend). The introspective driver does not own that impure seam.
        &NoopApplier
    }
}

/// A no-op applier for the `Driver::applier()` contract slot (mirrors `SysDriver`'s). The real
/// `/transform` apply path is the runtime [`TransformApplier`]; this exists only so the driver
/// satisfies the introspective trait without pretending to own the impure seam.
struct NoopApplier;

impl PlanApplier for NoopApplier {
    fn apply(
        &mut self,
        node: &qfs_plan::EffectNode,
    ) -> Result<qfs_plan::AppliedEffect, qfs_plan::ApplyError> {
        Ok(qfs_plan::AppliedEffect::new(node.id, 0))
    }
}

/// Wrap a [`TransformApplier`] in the runtime [`PlanApplierBridge`], yielding the async
/// `ApplyDriver` ready to `register` under the driver id `transform`.
#[must_use]
pub fn transform_apply_driver(applier: &TransformApplier) -> PlanApplierBridge<TransformApplier> {
    PlanApplierBridge::new(Arc::new(applier.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use qfs_driver::check_capability;

    #[test]
    fn describe_transform_is_pure_no_db_no_creds() {
        let d = TransformDriver::new();
        assert_eq!(d.mount(), "/transform");

        let reg = d.describe(&Path::new("/transform")).unwrap();
        assert_eq!(reg.archetype, Archetype::RelationalTable);
        assert!(reg.schema.column("name").is_some());
        assert!(reg.schema.column("mode").is_some());
        // The schema carries NO secret-VALUE column — only a `secret_ref` REFERENCE.
        assert!(reg.schema.column("secret").is_none());
        assert!(reg.schema.column("token").is_none());
        assert!(reg.schema.column("secret_ref").is_some());

        // /transform/<name> also resolves (the item/DESCRIBE/REMOVE form).
        assert!(d.describe(&Path::new("/transform/classify")).is_ok());
        // A foreign path is not describable (no panic).
        assert!(d.describe(&Path::new("/sys/nope")).is_err());
    }

    #[test]
    fn capabilities_allow_select_insert_remove_not_update() {
        let d = TransformDriver::new();
        let p = Path::new("/transform");
        assert!(check_capability(&d, &p, Verb::Select).is_ok());
        assert!(check_capability(&d, &p, Verb::Insert).is_ok());
        assert!(check_capability(&d, &p, Verb::Remove).is_ok());
        assert!(check_capability(&d, &p, Verb::Update).is_err());
    }

    #[test]
    fn the_registry_root_is_navigable_but_a_definition_is_a_leaf() {
        // §9 enumerable-children conformance: `cd /transform` enters the registry (its children are
        // the definitions — locations); `/transform/classify` is a row leaf. Both report the SAME
        // `RelationalTable` archetype — this is the exact pair the scope finding verified against
        // the running binary, and the reason the `cd` gate reads `navigable`, not the archetype.
        let d = TransformDriver::new();
        let root = d.describe(&Path::new("/transform")).unwrap();
        let item = d.describe(&Path::new("/transform/classify")).unwrap();
        assert_eq!(root.archetype, item.archetype);
        assert!(root.navigable, "cd /transform must enter the registry");
        assert!(!item.navigable, "a definition is a leaf, not an interior");
    }

    #[test]
    fn driver_is_object_safe() {
        let d: Arc<dyn Driver> = Arc::new(TransformDriver::new());
        assert_eq!(d.mount(), "/transform");
        let _seam: &dyn PlanApplier = d.applier();
    }
}
