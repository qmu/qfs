//! Fixtures shared by the `qfs-runtime` integration tests (`txn_commit`, `observability`).
//!
//! Each integration test file is its own binary and compiles this module separately, so a
//! fixture used by only one of them is dead code in the other — hence the blanket allow.

#![allow(dead_code)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::HashSet;
use std::sync::Arc;
use std::sync::Mutex;

use qfs_plan::{EffectKind, EffectNode, NodeId, Target, VfsPath};
use qfs_runtime::{ApplyCx, ApplyDriver, DriverRegistry, EffectError, EffectInput, EffectOutput};
use qfs_types::{Column, ColumnType, DriverId, Row, RowBatch, Schema, Value};

/// A mock driver that records which node ids it actually applied (so idempotent resume is
/// observable) and can fail specific nodes terminally / with a "conflict" reason.
#[derive(Default)]
pub struct TxnMock {
    applied: Mutex<Vec<NodeId>>,
    fail_terminal: HashSet<NodeId>,
    fail_conflict: HashSet<NodeId>,
}

impl TxnMock {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn failing_terminal(mut self, id: NodeId) -> Self {
        self.fail_terminal.insert(id);
        self
    }
    pub fn failing_conflict(mut self, id: NodeId) -> Self {
        self.fail_conflict.insert(id);
        self
    }
    pub fn applied_ids(&self) -> Vec<NodeId> {
        self.applied.lock().unwrap().clone()
    }
}

#[async_trait::async_trait]
impl ApplyDriver for TxnMock {
    async fn apply_one(&self, e: &EffectInput, _cx: &ApplyCx) -> Result<EffectOutput, EffectError> {
        if self.fail_conflict.contains(&e.id) {
            // The driver carries the version the world ACTUALLY holds (t12), not the expected
            // token — the bridge threads this real coordinate into a typed `Conflict`.
            return Err(EffectError::conflict("v2-world"));
        }
        if self.fail_terminal.contains(&e.id) {
            return Err(EffectError::terminal("mock terminal failure"));
        }
        self.applied.lock().unwrap().push(e.id);
        Ok(EffectOutput::new(e.id, 1))
    }
}

pub fn write_node(id: u32, driver: &str, kind: EffectKind) -> EffectNode {
    let schema = Schema::new(vec![Column::new("v", ColumnType::Int, false)]);
    let batch = RowBatch::new(schema, vec![Row::new(vec![Value::Int(i64::from(id))])]);
    EffectNode::new(
        NodeId(id),
        kind,
        Target::new(
            DriverId::new(driver),
            VfsPath::new(format!("/{driver}/{id}")),
        ),
    )
    .with_args(batch)
}

pub fn registry(driver: Arc<TxnMock>, id: &str) -> DriverRegistry {
    DriverRegistry::new().with(DriverId::new(id), driver)
}

/// Build a node whose payload row carries a "secret" value, so the secret-free assertion has
/// real secret material to NOT find in the audit output.
pub fn secret_bearing_node(id: u32, driver: &str, kind: EffectKind) -> EffectNode {
    let schema = Schema::new(vec![Column::new("secret", ColumnType::Text, false)]);
    let batch = RowBatch::new(
        schema,
        vec![Row::new(vec![Value::Text("PASSWORD-12345".into())])],
    );
    EffectNode::new(
        NodeId(id),
        kind,
        Target::new(
            DriverId::new(driver),
            VfsPath::new(format!("/{driver}/{id}")),
        ),
    )
    .with_args(batch)
}
