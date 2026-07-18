//! Integration tests for the t11 transactional COMMIT path (`Interpreter::commit_txn`). All
//! use an in-memory mock [`ApplyDriver`] + the in-memory [`InMemoryLedger`] — **no live
//! credentials, no network**, fully deterministic. The pure orchestration policy (saga/ACID
//! executors, key derivation, strategy selection) is unit-tested inside `qfs-txn`; these
//! prove the async interpreter wiring: strategy dispatch, idempotent resume through the
//! ledger, optimistic-concurrency conflict mapping, and a deterministic `RecoveryReport`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;

use qfs_plan::{EffectKind, EffectNode, NodeId, Plan, PlanBuilder, Target, VfsPath};
use qfs_runtime::{
    AuditLedger, CapabilitySet, CommitStrategy, DriverRegistry, InMemoryLedger, Interpreter,
    LegOutcome, Precondition, Preconditions, TransactionalDrivers, Version,
};
use qfs_types::DriverId;

mod common;
use common::{registry, secret_bearing_node, write_node, TxnMock};

/// Single transactional source → ACID strategy; a clean run applies every leg and the report
/// is clean (not rolled back).
#[tokio::test]
async fn acid_strategy_clean_commit() {
    let mock = Arc::new(TxnMock::new());
    let interp = Interpreter::with_defaults(registry(mock.clone(), "db"));

    let mut b = PlanBuilder::new();
    b.push(write_node(0, "db", EffectKind::Insert));
    b.push(write_node(1, "db", EffectKind::Update));
    let plan = b.build();

    let txnal = TransactionalDrivers::none().with(DriverId::new("db"));
    let ledger = InMemoryLedger::new();
    let (strategy, report) = interp
        .commit_txn(
            &plan,
            &CapabilitySet::allow_all(),
            "plan-1",
            &Preconditions::new(),
            &txnal,
            &ledger,
        )
        .await
        .unwrap();

    assert_eq!(strategy.code(), "single_source_acid");
    assert!(report.is_clean());
    assert!(!report.rolled_back);
    assert_eq!(report.applied_count(), 2);
    assert_eq!(mock.applied_ids(), vec![NodeId(0), NodeId(1)]);
}

/// ACID mid-plan failure → rolled_back flag set; the failing + subsequent legs do not commit.
#[tokio::test]
async fn acid_strategy_rolls_back_on_failure() {
    let mock = Arc::new(TxnMock::new().failing_terminal(NodeId(1)));
    let interp = Interpreter::with_defaults(registry(mock.clone(), "db"));

    let mut b = PlanBuilder::new();
    b.push(write_node(0, "db", EffectKind::Insert));
    b.push(write_node(1, "db", EffectKind::Insert));
    b.push(write_node(2, "db", EffectKind::Insert));
    let plan = b.build();

    let txnal = TransactionalDrivers::none().with(DriverId::new("db"));
    let ledger = InMemoryLedger::new();
    let (strategy, report) = interp
        .commit_txn(
            &plan,
            &CapabilitySet::allow_all(),
            "plan-1",
            &Preconditions::new(),
            &txnal,
            &ledger,
        )
        .await
        .unwrap();

    assert!(matches!(strategy, CommitStrategy::SingleSourceAcid { .. }));
    assert!(report.rolled_back, "ACID failure rolls back");
    assert_eq!(report.failure_at, Some(NodeId(1)));
    // Leg 2 was never attempted (skipped after the rollback boundary).
    assert_eq!(mock.applied_ids(), vec![NodeId(0)]);
}

/// Multi-source plan → saga strategy; idempotent resume: a re-run over the SAME ledger applies
/// nothing (every leg AlreadyApplied), the driver is not called again.
#[tokio::test]
async fn saga_strategy_idempotent_resume() {
    let mock_a = Arc::new(TxnMock::new());
    let mock_b = Arc::new(TxnMock::new());
    let registry = DriverRegistry::new()
        .with(DriverId::new("a"), mock_a.clone())
        .with(DriverId::new("b"), mock_b.clone());
    let interp = Interpreter::with_defaults(registry);

    let mut b = PlanBuilder::new();
    b.push(write_node(0, "a", EffectKind::Upsert));
    b.push(write_node(1, "b", EffectKind::Upsert));
    let plan = b.build();

    let txnal = TransactionalDrivers::none();
    let ledger = InMemoryLedger::new();

    let (strategy, r1) = interp
        .commit_txn(
            &plan,
            &CapabilitySet::allow_all(),
            "p",
            &Preconditions::new(),
            &txnal,
            &ledger,
        )
        .await
        .unwrap();
    assert_eq!(strategy.code(), "cross_source_saga");
    assert_eq!(r1.applied_count(), 2);
    assert_eq!(mock_a.applied_ids(), vec![NodeId(0)]);
    assert_eq!(mock_b.applied_ids(), vec![NodeId(1)]);

    // Re-run over the SAME ledger: every leg is a no-op (idempotent at-least-once redelivery).
    let (_s, r2) = interp
        .commit_txn(
            &plan,
            &CapabilitySet::allow_all(),
            "p",
            &Preconditions::new(),
            &txnal,
            &ledger,
        )
        .await
        .unwrap();
    assert_eq!(r2.already_applied_count(), 2);
    assert_eq!(r2.applied_count(), 0);
    // The drivers were NOT called a second time.
    assert_eq!(
        mock_a.applied_ids(),
        vec![NodeId(0)],
        "no re-apply on resume"
    );
    assert_eq!(
        mock_b.applied_ids(),
        vec![NodeId(1)],
        "no re-apply on resume"
    );
}

/// Optimistic concurrency: a conditional write whose driver reports a precondition/412 failure
/// is surfaced as a typed `Conflict` (not a generic failure), proving no lost update.
#[tokio::test]
async fn optimistic_conflict_surfaces_typed() {
    let mock = Arc::new(TxnMock::new().failing_conflict(NodeId(0)));
    let interp = Interpreter::with_defaults(registry(mock, "s3"));

    let plan = Plan::leaf(write_node(0, "s3", EffectKind::Update));
    let mut pre = Preconditions::new();
    pre.insert(NodeId(0), Precondition::IfVersion(Version::new("v1")));

    let txnal = TransactionalDrivers::none().with(DriverId::new("s3"));
    let ledger = InMemoryLedger::new();
    let (_strategy, report) = interp
        .commit_txn(
            &plan,
            &CapabilitySet::allow_all(),
            "p",
            &pre,
            &txnal,
            &ledger,
        )
        .await
        .unwrap();

    assert_eq!(
        report.conflict_count(),
        1,
        "typed conflict surfaced: {report:?}"
    );
    assert!(!report.is_clean());
    match &report.legs[0].outcome {
        // The bridge surfaces the REAL world version the driver reported ("v2-world"), NOT the
        // precondition's expected token ("v1") — the t12 Conflict{version} threading.
        LegOutcome::Conflict { version } => assert_eq!(version, &Version::new("v2-world")),
        other => panic!("expected Conflict, got {other:?}"),
    }
}

/// A capability denial on a transactional leg is a terminal leg failure (defense in depth),
/// and on the ACID path it triggers rollback.
#[tokio::test]
async fn capability_denied_leg_fails_and_rolls_back() {
    let mock = Arc::new(TxnMock::new());
    let interp = Interpreter::with_defaults(registry(mock.clone(), "db"));

    let plan = Plan::leaf(write_node(0, "db", EffectKind::Remove));
    // Grant nothing → the REMOVE is denied at apply time.
    let caps = CapabilitySet::none();
    let txnal = TransactionalDrivers::none().with(DriverId::new("db"));
    let ledger = InMemoryLedger::new();
    let (_strategy, report) = interp
        .commit_txn(&plan, &caps, "p", &Preconditions::new(), &txnal, &ledger)
        .await
        .unwrap();

    assert!(!report.is_clean());
    assert!(report.rolled_back);
    assert!(
        mock.applied_ids().is_empty(),
        "denied leg never reaches the driver"
    );
}

/// PREVIEW-equivalent purity: `select_strategy` (exposed via the runtime re-export) chooses the
/// strategy with no driver calls — a plan can be inspected without executing.
#[tokio::test]
async fn read_only_plan_is_saga_and_applies_no_legs() {
    let mock = Arc::new(TxnMock::new());
    let interp = Interpreter::with_defaults(registry(mock.clone(), "ga"));

    // A pure read plan: no write legs at all.
    let plan = Plan::leaf(EffectNode::new(
        NodeId(0),
        EffectKind::Read,
        Target::new(DriverId::new("ga"), VfsPath::new("/ga/x")),
    ));
    let ledger = InMemoryLedger::new();
    let (strategy, report) = interp
        .commit_txn(
            &plan,
            &CapabilitySet::allow_all(),
            "p",
            &Preconditions::new(),
            &TransactionalDrivers::none(),
            &ledger,
        )
        .await
        .unwrap();
    assert_eq!(strategy.code(), "cross_source_saga");
    assert!(report.legs.is_empty(), "no write legs");
    assert!(mock.applied_ids().is_empty());
}

// --- t12: has_intent crash-window reconcile -----------------------------------------------

/// A crash between `record_intent` and `mark_applied` on a **non-idempotent** leg (an
/// unconditional `Insert`) is detected on resume via `has_intent` and surfaced as
/// `Indeterminate` — the bridge refuses to silently re-apply it (apply-once, blueprint §7/§8). The
/// driver is NOT called a second time.
#[tokio::test]
async fn crash_between_intent_and_apply_is_indeterminate_for_insert() {
    let mock = Arc::new(TxnMock::new());
    let interp = Interpreter::with_defaults(registry(mock.clone(), "db"));
    let plan = Plan::leaf(write_node(0, "db", EffectKind::Insert));
    let txnal = TransactionalDrivers::none().with(DriverId::new("db"));
    let ledger = InMemoryLedger::new();

    // Simulate the crash window: the prior run recorded the intent but died before sealing the
    // apply, so the ledger holds an unsealed intent (has_intent = true, applied = None).
    let node = plan.node(NodeId(0)).unwrap();
    let leg = qfs_runtime::EffectLeg::from_node("p", node, Precondition::None);
    ledger.record_intent(&leg.key, &leg.descriptor);
    assert!(ledger.has_intent(&leg.key));
    assert!(ledger.applied(&leg.key).is_none());

    let (_strategy, report) = interp
        .commit_txn(
            &plan,
            &CapabilitySet::allow_all(),
            "p",
            &Preconditions::new(),
            &txnal,
            &ledger,
        )
        .await
        .unwrap();

    assert_eq!(report.indeterminate_count(), 1, "{report:?}");
    assert_eq!(report.applied_count(), 0);
    assert!(!report.is_clean());
    assert_eq!(report.failure_at, Some(NodeId(0)));
    match &report.legs[0].outcome {
        LegOutcome::Indeterminate { key } => assert_eq!(key, &leg.key),
        other => panic!("expected Indeterminate, got {other:?}"),
    }
    // The non-idempotent Insert was NEVER blindly replayed.
    assert!(
        mock.applied_ids().is_empty(),
        "no silent replay of a non-idempotent leg"
    );
}

/// The same crash window on a **replay-safe** leg (an `UPSERT`) is reconciled by re-applying:
/// `UPSERT` is convergent, so the resume applies it (sealing the ledger) rather than flagging
/// it `Indeterminate`.
#[tokio::test]
async fn crash_window_upsert_is_replayed_not_indeterminate() {
    let mock = Arc::new(TxnMock::new());
    let interp = Interpreter::with_defaults(registry(mock.clone(), "db"));
    let plan = Plan::leaf(write_node(0, "db", EffectKind::Upsert));
    let txnal = TransactionalDrivers::none().with(DriverId::new("db"));
    let ledger = InMemoryLedger::new();

    let node = plan.node(NodeId(0)).unwrap();
    let leg = qfs_runtime::EffectLeg::from_node("p", node, Precondition::None);
    ledger.record_intent(&leg.key, &leg.descriptor);

    let (_strategy, report) = interp
        .commit_txn(
            &plan,
            &CapabilitySet::allow_all(),
            "p",
            &Preconditions::new(),
            &txnal,
            &ledger,
        )
        .await
        .unwrap();

    assert_eq!(report.indeterminate_count(), 0, "{report:?}");
    assert_eq!(
        report.applied_count(),
        1,
        "upsert is convergent, re-applied"
    );
    assert!(report.is_clean());
    assert_eq!(mock.applied_ids(), vec![NodeId(0)]);
    // The resume sealed the ledger, so a further re-run is a no-op.
    assert!(ledger.applied(&leg.key).is_some());
}

/// A conditionally-guarded write is also replay-safe in the crash window: a stale re-apply
/// would be caught as a `Conflict`, never a silent double-apply — so it is re-applied, not
/// flagged `Indeterminate`.
#[tokio::test]
async fn crash_window_conditional_write_is_replay_safe() {
    let mock = Arc::new(TxnMock::new());
    let interp = Interpreter::with_defaults(registry(mock.clone(), "s3"));
    let plan = Plan::leaf(write_node(0, "s3", EffectKind::Update));
    let txnal = TransactionalDrivers::none().with(DriverId::new("s3"));
    let ledger = InMemoryLedger::new();

    let pre = Precondition::IfVersion(Version::new("v1"));
    let node = plan.node(NodeId(0)).unwrap();
    let leg = qfs_runtime::EffectLeg::from_node("p", node, pre.clone());
    ledger.record_intent(&leg.key, &leg.descriptor);

    let mut preconditions = Preconditions::new();
    preconditions.insert(NodeId(0), pre);
    let (_strategy, report) = interp
        .commit_txn(
            &plan,
            &CapabilitySet::allow_all(),
            "p",
            &preconditions,
            &txnal,
            &ledger,
        )
        .await
        .unwrap();

    assert_eq!(report.indeterminate_count(), 0, "{report:?}");
    assert_eq!(report.applied_count(), 1);
    assert_eq!(mock.applied_ids(), vec![NodeId(0)]);
}

// --- t12: EffectError::Conflict{version} threading -----------------------------------------

/// A `Conflict` reported on an **unconditional** write (a driver-contract anomaly: there is no
/// precondition to reconcile against) maps to a terminal failure that preserves the world
/// version in its reason — NOT a typed `Conflict` (which only makes sense for a guarded write).
#[tokio::test]
async fn conflict_on_unconditional_write_is_terminal() {
    let mock = Arc::new(TxnMock::new().failing_conflict(NodeId(0)));
    let interp = Interpreter::with_defaults(registry(mock, "s3"));
    let plan = Plan::leaf(write_node(0, "s3", EffectKind::Update));
    // No precondition for NodeId(0) → unconditional write.
    let txnal = TransactionalDrivers::none().with(DriverId::new("s3"));
    let ledger = InMemoryLedger::new();
    let (_strategy, report) = interp
        .commit_txn(
            &plan,
            &CapabilitySet::allow_all(),
            "p",
            &Preconditions::new(),
            &txnal,
            &ledger,
        )
        .await
        .unwrap();

    assert_eq!(report.conflict_count(), 0);
    match &report.legs[0].outcome {
        LegOutcome::Failed(e) => assert!(
            format!("{e:?}").contains("v2-world"),
            "world version preserved in terminal reason: {e:?}"
        ),
        other => panic!("expected terminal Failed, got {other:?}"),
    }
}

// --- t12: mixed-plan audit ledger (deterministic order + secret-free) ----------------------

/// A mixed cross-source saga plan (applied / failed / skipped) produces a `RecoveryReport`
/// whose legs are in deterministic plan (topological) order, and whose serialized form is
/// **secret-free** — no payload row value, no credential token, no `@version` literal leaks
/// (blueprint §8). The committed prefix matches the intended prefix up to the failure boundary.
#[tokio::test]
async fn mixed_plan_audit_is_ordered_and_secret_free() {
    // Three legs across two sources; the middle one fails terminally, so the third is skipped.
    let mock_a = Arc::new(TxnMock::new().failing_terminal(NodeId(1)));
    let mock_b = Arc::new(TxnMock::new());
    let registry = DriverRegistry::new()
        .with(DriverId::new("a"), mock_a.clone())
        .with(DriverId::new("b"), mock_b.clone());
    let interp = Interpreter::with_defaults(registry);

    let mut b = PlanBuilder::new();
    b.push(secret_bearing_node(0, "a", EffectKind::Insert)); // applies
    b.push(secret_bearing_node(1, "a", EffectKind::Insert)); // fails terminally
    b.push(secret_bearing_node(2, "b", EffectKind::Insert)); // skipped after failure
    let plan = b.build();

    let ledger = InMemoryLedger::new();
    let (strategy, report) = interp
        .commit_txn(
            &plan,
            &CapabilitySet::allow_all(),
            "plan-mix",
            &Preconditions::new(),
            &TransactionalDrivers::none(),
            &ledger,
        )
        .await
        .unwrap();

    assert_eq!(strategy.code(), "cross_source_saga");
    // Deterministic plan order: legs are emitted [0, 1, 2] regardless of completion timing.
    let ids: Vec<_> = report.legs.iter().map(|l| l.id).collect();
    assert_eq!(ids, vec![NodeId(0), NodeId(1), NodeId(2)]);
    // Committed prefix == intended prefix up to the failure: 0 applied, 1 failed, 2 skipped.
    assert_eq!(report.legs[0].outcome.code(), "applied");
    assert_eq!(report.legs[1].outcome.code(), "failed");
    assert_eq!(report.legs[2].outcome.code(), "failed"); // skipped is modelled as a terminal
    assert_eq!(report.failure_at, Some(NodeId(1)));
    assert_eq!(report.applied_count(), 1);
    // Only the first leg actually reached its driver.
    assert_eq!(mock_a.applied_ids(), vec![NodeId(0)]);
    assert!(mock_b.applied_ids().is_empty());

    // Secret-free audit: the serialized report carries identity + shape + counts only — never
    // the secret payload value, the credential, or the `@version` literal (blueprint §8).
    let json = serde_json::to_string(&report).unwrap();
    assert!(
        !json.contains("super-secret-token"),
        "no credential leaks: {json}"
    );
    assert!(
        !json.contains("PASSWORD-12345"),
        "no payload value leaks: {json}"
    );
    // The report DOES carry the secret-free shape: the idempotency key, kind, and target path.
    assert!(json.contains("k:plan-mix:0:"), "key present: {json}");
    assert!(json.contains("\"insert\""), "kind present: {json}");
}
