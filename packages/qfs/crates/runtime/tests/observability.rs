//! Observability surface of the transactional COMMIT (blueprint §7 spans/ids, §8 secret-free).
//!
//! **Why this test has its own integration-test binary.** Capturing `tracing` output means
//! touching process-global state, and `tracing`'s per-callsite `Interest` cache is exactly that:
//! the first thread to reach a callsite decides — for the whole process — whether it is live.
//! When this test shared a binary with `txn_commit`'s parallel siblings, a sibling that reached
//! the interpreter's `info_span!` before any subscriber was registered cached
//! `Interest::never()` process-wide, and this test's own commit then emitted nothing (an empty
//! buffer failing every assertion); when it emitted first, the siblings' commits landed in the
//! shared buffer instead and the whole-buffer assertions read a sibling's output. Both are the
//! same defect — one capture, shared by tests that run concurrently.
//!
//! A separate binary is the fix rather than a workaround: this file's process contains exactly
//! one emitting test, so the capture cannot be polluted and its callsites cannot be decided by
//! anyone else. The subscriber is still installed scoped (`with_default`) rather than globally,
//! so the isolation does not silently depend on this file staying single-test — the isolation
//! assertion below fails loudly if that ever stops being true.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;

use qfs_plan::{EffectKind, Plan};
use qfs_runtime::{
    CapabilitySet, InMemoryLedger, Interpreter, Preconditions, TransactionalDrivers,
};
use qfs_types::DriverId;

mod common;
use common::{registry, secret_bearing_node, TxnMock};

/// A minimal capturing subscriber: records each event's fields (and its span's fields) as a
/// flat `key=value` string so the observability test can assert id presence + secret-freedom.
#[derive(Default)]
struct Capture {
    lines: std::sync::Mutex<Vec<String>>,
}
struct FieldVisitor(String);
impl tracing::field::Visit for FieldVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        self.0.push_str(&format!(" {}={value:?}", field.name()));
    }
}
impl tracing::Subscriber for Capture {
    fn enabled(&self, _m: &tracing::Metadata<'_>) -> bool {
        true
    }
    fn max_level_hint(&self) -> Option<tracing::level_filters::LevelFilter> {
        // Raise the max level so the interpreter's `info_span!`/`info!` callsites are not
        // short-circuited by tracing's default `OFF` filter.
        Some(tracing::level_filters::LevelFilter::TRACE)
    }
    fn new_span(&self, attrs: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        let mut v = FieldVisitor(format!("span:{}", attrs.metadata().name()));
        attrs.record(&mut v);
        self.lines.lock().unwrap().push(v.0);
        tracing::span::Id::from_u64(1)
    }
    fn record(&self, _s: &tracing::span::Id, _v: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _s: &tracing::span::Id, _f: &tracing::span::Id) {}
    fn event(&self, event: &tracing::Event<'_>) {
        let mut v = FieldVisitor(format!("event:{}", event.metadata().name()));
        event.record(&mut v);
        self.lines.lock().unwrap().push(v.0);
    }
    fn enter(&self, _s: &tracing::span::Id) {}
    fn exit(&self, _s: &tracing::span::Id) {}
}

/// Drive `f` to completion on a private current-thread runtime with `cap` installed as this
/// thread's default subscriber, and return the lines it captured. The runtime is
/// `current_thread`, so every poll of the future runs on this thread and inherits the default.
fn capture_lines<F: std::future::Future>(f: F) -> Vec<String> {
    let cap = Arc::new(Capture::default());
    let rt = tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("current-thread runtime");
    tracing::subscriber::with_default(cap.clone(), || rt.block_on(f));
    let lines = cap.lines.lock().unwrap().clone();
    lines
}

/// Every applied-effect log line carries `trace_id`, `plan_id`, and `effect.id` (blueprint §7),
/// and no line carries payload material (blueprint §8).
#[test]
fn observability_spans_carry_ids_and_are_secret_free() {
    let lines = capture_lines(async {
        let mock = Arc::new(TxnMock::new());
        let interp = Interpreter::with_defaults(registry(mock, "db"));
        let plan = Plan::leaf(secret_bearing_node(0, "db", EffectKind::Insert));
        let txnal = TransactionalDrivers::none().with(DriverId::new("db"));
        let ledger = InMemoryLedger::new();

        interp
            .commit_txn(
                &plan,
                &CapabilitySet::allow_all(),
                "plan-obs-unique",
                &Preconditions::new(),
                &txnal,
                &ledger,
            )
            .await
            .unwrap();
    });
    let all = lines.join("\n");

    // The capture must hold this commit's emissions and nothing else. Only the root
    // `commit_txn` span carries `plan_id` as a field — the per-leg span/event correlate by
    // trace id through span context, which this minimal subscriber does not track — so
    // isolation, not per-line filtering, is what makes the whole-buffer assertions below sound.
    // A root span for any other plan means the capture leaked and they are reading someone else.
    let foreign: Vec<&String> = lines
        .iter()
        .filter(|l| l.starts_with("span:commit_txn") && !l.contains("plan_id=plan-obs-unique"))
        .collect();
    assert!(foreign.is_empty(), "capture not isolated: {foreign:?}");
    // An empty buffer would satisfy every negative assertion below for the wrong reason.
    assert!(!lines.is_empty(), "capture recorded nothing");

    // The root span carries trace_id + plan_id.
    assert!(all.contains("plan_id=plan-obs-unique"), "plan_id: {all}");
    assert!(
        all.contains("trace_id=t:plan-obs-unique"),
        "trace_id: {all}"
    );
    // The per-leg event carries effect.id and the outcome (it inherits the root trace id).
    assert!(all.contains("leg applied"), "leg event present: {all}");
    assert!(all.contains("effect.id"), "effect id present: {all}");
    // Secret-free: no payload value in any captured span/event line (blueprint §8).
    assert!(
        !all.contains("PASSWORD-12345"),
        "no secret in observability: {all}"
    );
}
