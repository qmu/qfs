//! [`TransformApplier`] — the `/transform` driver's apply leg. It routes a write effect node to the
//! injected [`TransformBackend`]: `INSERT INTO /transform` (create, upsert-on-name) and
//! `REMOVE /transform[/<name>]` (delete, inherently irreversible). Every other write is a
//! structured rejection (belt-and-suspenders over the parse-time capability gate).
//!
//! The real I/O happens in the injected backend (binary-side rusqlite over `sys_transforms`); the
//! applier is a stateless `&self` router bridged to the runtime via [`SharedApplier`].

use std::sync::Arc;

use qfs_plan::{AppliedEffect, ApplyError, EffectKind, EffectNode, PlanApplier};
use qfs_runtime::{EffectError, EffectOutput, SharedApplier};
use qfs_types::{RowBatch, Value};

use crate::backend::{TransformBackend, TransformError};
use crate::schema::{name_from_path, node_for_path};

/// The synchronous `/transform` apply leg. Holds the injected backend behind an `Arc` so it is
/// cheap to clone and `&self`-apply. Stateless across calls.
#[derive(Clone)]
pub struct TransformApplier {
    backend: Arc<dyn TransformBackend>,
}

impl TransformApplier {
    /// Build an applier over an injected [`TransformBackend`] (the binary's System-DB impl).
    #[must_use]
    pub fn new(backend: Arc<dyn TransformBackend>) -> Self {
        Self { backend }
    }

    /// Route one effect node to the backend: resolve the `/transform` node, gate the verb, apply.
    fn apply_node(&self, node: &EffectNode) -> Result<u64, TransformError> {
        let path = node.target.path.as_str();
        node_for_path(path).ok_or_else(|| TransformError::UnknownNode {
            path: path.to_string(),
        })?;
        match &node.kind {
            // Create / re-create a definition (upsert on `name`).
            EffectKind::Insert | EffectKind::Upsert => self.backend.insert(&node.args),
            // Delete a definition. The name rides as the `/transform/<name>` path segment (the
            // `REMOVE TRANSFORM <name>` sugar desugars to `REMOVE /transform WHERE name == '<name>'`,
            // whose evaluator lowers the filter onto the WHERE-SELECTOR, §7) — accept either the
            // path or the selector. A REMOVE's `args` is empty now: it writes nothing.
            EffectKind::Remove => {
                let name = name_from_path(path)
                    .or_else(|| node.selector_text("name"))
                    .ok_or_else(|| TransformError::MalformedEffect {
                        reason: "REMOVE /transform needs a name (REMOVE TRANSFORM <name> or \
                                 REMOVE /transform/<name>)"
                            .into(),
                    })?;
                self.backend.remove(&name)
            }
            // The §15 transform-RUN consent/audit leg (`CALL transform.<name>`): the model call
            // already ran exec-side at the commit boundary; this ledgers that it did. The exact
            // produced-row count was refined onto the node's estimate by the orchestrator.
            EffectKind::Call(proc) if proc.as_str().starts_with("transform.") => {
                let name = name_from_path(path)
                    .or_else(|| arg_text(&node.args, "transform"))
                    .unwrap_or_else(|| proc.as_str().trim_start_matches("transform.").to_string());
                let affected = match node.est_affected {
                    qfs_plan::Affected::Exact(n) => n,
                    _ => 0,
                };
                self.backend.record_run(&name, affected)?;
                Ok(affected)
            }
            other => Err(TransformError::UnsupportedVerb {
                verb: static_verb_label(other),
            }),
        }
    }
}

/// The single-row write payload's value for `col` as a non-empty string, if the batch carries it.
fn arg_text(args: &RowBatch, col: &str) -> Option<String> {
    let idx = args
        .schema
        .columns
        .iter()
        .position(|c| c.name.as_str() == col)?;
    match args.rows.first().and_then(|r| r.values.get(idx)) {
        Some(Value::Text(s)) if !s.is_empty() => Some(s.clone()),
        _ => None,
    }
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

impl SharedApplier for TransformApplier {
    fn apply_shared(&self, node: &EffectNode) -> Result<EffectOutput, EffectError> {
        let affected = self
            .apply_node(node)
            .map_err(|e| EffectError::terminal(e.to_string()))?;
        Ok(EffectOutput::new(node.id, affected))
    }
}

impl PlanApplier for TransformApplier {
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
    use qfs_types::{Column, ColumnType, Row, Schema};
    use std::sync::Mutex;

    #[derive(Default)]
    struct FakeBackend {
        inserted: Mutex<Vec<RowBatch>>,
        removed: Mutex<Vec<String>>,
        runs: Mutex<Vec<(String, u64)>>,
    }

    impl TransformBackend for FakeBackend {
        fn scan(&self) -> Result<RowBatch, TransformError> {
            Ok(RowBatch::new(Schema::new(vec![]), vec![]))
        }
        fn insert(&self, row: &RowBatch) -> Result<u64, TransformError> {
            self.inserted.lock().unwrap().push(row.clone());
            Ok(1)
        }
        fn remove(&self, name: &str) -> Result<u64, TransformError> {
            self.removed.lock().unwrap().push(name.to_string());
            Ok(1)
        }
        fn record_run(&self, name: &str, affected: u64) -> Result<(), TransformError> {
            self.runs.lock().unwrap().push((name.to_string(), affected));
            Ok(())
        }
    }

    fn effect(kind: EffectKind, path: &str, args: RowBatch) -> EffectNode {
        EffectNode::new(
            NodeId(0),
            kind,
            Target::new(DriverId::new("transform"), VfsPath::new(path)),
        )
        .with_args(args)
    }

    fn name_row(name: &str) -> RowBatch {
        RowBatch::new(
            Schema::new(vec![Column::new("name", ColumnType::Text, false)]),
            vec![Row::new(vec![Value::Text(name.into())])],
        )
    }

    #[test]
    fn insert_into_transform_routes_to_the_backend() {
        let backend = Arc::new(FakeBackend::default());
        let applier = TransformApplier::new(backend.clone());
        let node = effect(EffectKind::Insert, "/transform", name_row("classify"));
        let out = applier.apply_shared(&node).expect("insert applies");
        assert_eq!(out.affected, 1);
        assert_eq!(backend.inserted.lock().unwrap().len(), 1);
    }

    #[test]
    fn remove_by_path_segment_resolves_the_name() {
        let backend = Arc::new(FakeBackend::default());
        let applier = TransformApplier::new(backend.clone());
        let node = effect(
            EffectKind::Remove,
            "/transform/classify",
            RowBatch::new(Schema::new(vec![]), vec![]),
        );
        applier.apply_shared(&node).expect("remove applies");
        assert_eq!(
            backend.removed.lock().unwrap().as_slice(),
            &["classify".to_string()]
        );
    }

    #[test]
    fn remove_by_name_filter_row_resolves_the_name() {
        // `REMOVE TRANSFORM classify` desugars to `REMOVE /transform WHERE name == 'classify'`,
        // whose filter the evaluator lowers onto the WHERE-SELECTOR (blueprint §7). A REMOVE writes
        // nothing, so its `args` is empty — the selector is the only channel the name travels on.
        let backend = Arc::new(FakeBackend::default());
        let applier = TransformApplier::new(backend.clone());
        let node = effect(EffectKind::Remove, "/transform", RowBatch::default())
            .with_selector(name_row("classify"));
        applier.apply_shared(&node).expect("remove applies");
        assert_eq!(
            backend.removed.lock().unwrap().as_slice(),
            &["classify".to_string()]
        );
    }

    #[test]
    fn transform_run_consent_leg_ledgers_via_record_run() {
        // The §15 model-run consent node (`CALL transform.<name>`) ledgers the run with the
        // orchestrator-refined affected count; it never touches insert/remove.
        let backend = Arc::new(FakeBackend::default());
        let applier = TransformApplier::new(backend.clone());
        let node = EffectNode::new(
            NodeId(0),
            EffectKind::Call(qfs_plan::ProcId::new("transform.classify")),
            Target::new(
                DriverId::new("transform"),
                VfsPath::new("/transform/classify"),
            ),
        )
        .with_affected(qfs_plan::Affected::Exact(3));
        let out = applier.apply_shared(&node).expect("consent leg applies");
        assert_eq!(out.affected, 3);
        assert_eq!(
            backend.runs.lock().unwrap().as_slice(),
            &[("classify".to_string(), 3)]
        );
        assert!(backend.inserted.lock().unwrap().is_empty());
        assert!(backend.removed.lock().unwrap().is_empty());
    }

    #[test]
    fn update_is_rejected_in_the_applier() {
        let applier = TransformApplier::new(Arc::new(FakeBackend::default()));
        let node = effect(
            EffectKind::Update,
            "/transform",
            RowBatch::new(Schema::new(vec![]), vec![]),
        );
        assert!(applier.apply_shared(&node).is_err());
    }
}
