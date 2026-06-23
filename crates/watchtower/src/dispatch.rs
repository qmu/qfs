//! [`Dispatcher`] — match an [`Event`] against the registered triggers, gate it through the
//! optional `WHERE NEW.*` predicate, bind `NEW.*` into the handler plan, pass the policy gate hook,
//! and `COMMIT` through the injected [`Committer`] — acking ONLY after a successful commit.
//!
//! ## At-least-once + idempotency (the hard part)
//! Delivery is at-least-once: the bus redelivers an un-acked event after a crash. Correctness comes
//! from idempotent handlers (`UPSERT`/`@version`) PLUS the [`Event::dedup_key`] the dispatcher
//! tracks in an **idempotency ledger** ([`Dispatcher::seen`]): the FIRST successful commit of a
//! dedup_key records it; a re-delivered event with the same dedup_key is a NO-OP (acked without a
//! second commit). So delivering the same Event twice yields ONE net effect.
//!
//! NOTE: a non-idempotent proc (`CALL mail.send`) still needs an explicit dedupe guard in the
//! plan — the ledger collapses *redelivery* of the same dedup_key, but two genuinely-distinct
//! events (distinct dedup_keys) each fire once, as intended.
//!
//! ## Purity
//! `handle` builds the bound handler `Statement` and evaluates the WHERE guard with NO I/O and NO
//! mutation; the ONLY effect is the injected `Committer::commit` (the COMMIT boundary). A failing
//! guard / a policy denial / a build error fires NOTHING (zero commit, zero driver call).

use std::collections::HashSet;
use std::sync::Mutex;

use cfs_core::{Row, StatementSpec};
use cfs_server::{AuditEntry, AuditSink, TriggerDef};

use crate::bind::{bind_new, NewBindings};
use crate::commit::{Committer, FireError, FireOutcome, PolicyGate};
use crate::event::Event;
use crate::predicate::guard_matches;

/// The result of dispatching one event (for tests + the bus ack decision).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Dispatched {
    /// One or more handlers fired and committed; the event may be acked. Carries the count of
    /// committed plans (for the audit / observability assertion).
    Fired(usize),
    /// The event was a redelivery of an already-committed dedup_key — a no-op; ack it.
    Duplicate,
    /// No registered trigger matched the event (ack it; nothing to do).
    NoMatch,
    /// Every matching handler was gated out (WHERE failed / policy denied) — zero fires; ack it.
    Gated,
}

impl Dispatched {
    /// Whether the event should be acked (every terminal outcome acks; a commit FAILURE returns an
    /// `Err` from `handle`, NOT a `Dispatched`, so it is NOT acked and is redelivered).
    #[must_use]
    pub fn should_ack(&self) -> bool {
        true
    }
}

/// The pure dispatch core. Holds the policy gate + the idempotency ledger; the trigger set is
/// passed per-call (a cloned `ServerState` snapshot, never a held lock). The committer + audit are
/// passed by reference so the same dispatcher serves many events.
pub struct Dispatcher<G: PolicyGate> {
    gate: G,
    /// The idempotency ledger: dedup_keys that have COMMITTED at least once. A redelivery of a
    /// recorded key is a no-op (the at-least-once → effectively-once collapse for idempotent
    /// handlers). In-memory now; a durable store is the E7/DO carry-over.
    seen: Mutex<HashSet<String>>,
}

impl<G: PolicyGate> Dispatcher<G> {
    /// Construct a dispatcher over a policy gate (the t34 default is [`crate::AllowAllGate`]).
    #[must_use]
    pub fn new(gate: G) -> Self {
        Self {
            gate,
            seen: Mutex::new(HashSet::new()),
        }
    }

    /// Whether `dedup_key` has already committed (test/observability aid).
    #[must_use]
    pub fn has_seen(&self, dedup_key: &str) -> bool {
        self.seen
            .lock()
            .map(|s| s.contains(dedup_key))
            .unwrap_or(false)
    }

    /// Dispatch `event` against `triggers`, committing every matching, gated-in handler through
    /// `committer` and recording one audit entry per fire. Acks (returns `Ok(Dispatched)`) on any
    /// terminal outcome; returns `Err(FireError)` on a COMMIT failure so the caller does NOT ack
    /// (the event is redelivered — at-least-once).
    ///
    /// # Errors
    /// [`FireError`] if a matching handler's plan build or commit FAILS (the event stays un-acked).
    pub fn handle(
        &self,
        event: &Event,
        triggers: &[TriggerDef],
        committer: &dyn Committer,
        audit: &AuditSink,
    ) -> Result<Dispatched, FireError> {
        // Idempotency ledger: a redelivery of an already-committed dedup_key is a no-op. Checked
        // FIRST so a redelivered event never re-runs a (possibly non-idempotent) handler.
        if self.has_seen(&event.dedup_key) {
            tracing::debug!(
                target: "cfs::watchtower",
                dedup_key = %event.dedup_key,
                "duplicate event (already committed); acking without re-fire"
            );
            return Ok(Dispatched::Duplicate);
        }

        // The NEW.* binding env + the column names (carried with the event payload). Built ONCE.
        let columns = event.columns.clone();
        let binds = NewBindings::from_row(&columns, &event.new.values);

        let matched: Vec<&TriggerDef> = triggers
            .iter()
            .filter(|t| t.on == event.source.as_str() || t.on == event.kind.label())
            .collect();
        if matched.is_empty() {
            return Ok(Dispatched::NoMatch);
        }

        let mut fired = 0usize;
        for trigger in matched {
            // 1. WHERE gating over NEW.* (a failing / malformed guard fires NOTHING).
            if !self.passes_guard(trigger, &columns, &event.new) {
                continue;
            }
            // 2. Build the NEW.*-bound handler statement (pure — no I/O, no commit yet).
            let bound = match self.bound_handler(trigger, &binds) {
                Some(stmt) => stmt,
                None => {
                    // A declared-but-empty / un-rehydratable handler fires nothing (logged).
                    tracing::warn!(
                        target: "cfs::watchtower",
                        trigger = %trigger.name,
                        "trigger handler is empty or could not rehydrate; skipping"
                    );
                    continue;
                }
            };
            // 3. Policy gate hook (RFD §10) — a denial fires nothing.
            if let Err(reason) = self.gate.check(&trigger.name, &bound) {
                tracing::warn!(
                    target: "cfs::watchtower",
                    trigger = %trigger.name,
                    %reason,
                    "policy gate denied fire"
                );
                continue;
            }
            // 4. COMMIT through the injected committer. A FAILURE bubbles up so the event is NOT
            //    acked (redelivered). On success, record the audit entry + advance the ledger.
            let outcome = committer.commit(&bound)?;
            self.record_fire(event, trigger, &outcome, audit);
            fired += 1;
        }

        // Advance the idempotency ledger only AFTER all matching handlers committed (so a partial
        // commit failure above redelivers and re-attempts — at-least-once, the ledger is the
        // effectively-once collapse for the SUCCESSFUL path).
        if fired > 0 {
            if let Ok(mut seen) = self.seen.lock() {
                seen.insert(event.dedup_key.clone());
            }
            Ok(Dispatched::Fired(fired))
        } else {
            // Matched but every handler gated out (WHERE failed / policy denied / empty).
            Ok(Dispatched::Gated)
        }
    }

    /// Evaluate the trigger's optional `WHERE` guard over `NEW.*`. No guard ⇒ always fire. A
    /// malformed guard ⇒ fail-closed (does NOT fire) + logged.
    fn passes_guard(&self, trigger: &TriggerDef, columns: &[String], new: &Row) -> bool {
        let canonical = trigger.predicate.as_str();
        if canonical.is_empty() {
            return true; // no guard
        }
        let spec = match StatementSpec::from_canonical(canonical) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(
                    target: "cfs::watchtower",
                    trigger = %trigger.name,
                    error = %e,
                    "trigger WHERE guard failed to rehydrate; not firing (fail-closed)"
                );
                return false;
            }
        };
        match guard_matches(spec.statement(), columns, new) {
            Ok(m) => m,
            Err(e) => {
                tracing::warn!(
                    target: "cfs::watchtower",
                    trigger = %trigger.name,
                    error = %e,
                    "trigger WHERE guard could not be evaluated; not firing (fail-closed)"
                );
                false
            }
        }
    }

    /// Rehydrate the trigger's handler plan body and bind `NEW.*` into it (pure). `None` if the
    /// handler is empty or could not rehydrate.
    fn bound_handler(
        &self,
        trigger: &TriggerDef,
        binds: &NewBindings,
    ) -> Option<cfs_parser::Statement> {
        let canonical = trigger.plan.as_str();
        if canonical.is_empty() {
            return None;
        }
        let spec = cfs_core::PlanSpec::from_canonical(canonical).ok()?;
        let mut stmt = spec.statement().clone();
        bind_new(&mut stmt, binds);
        Some(stmt)
    }

    /// Record ONE audit ledger entry per fired plan (event id + trigger + outcome). Secret-free.
    fn record_fire(
        &self,
        event: &Event,
        trigger: &TriggerDef,
        outcome: &FireOutcome,
        audit: &AuditSink,
    ) {
        audit.record(AuditEntry::PlanFired {
            cause: format!(
                "trigger:{} event:{} kind:{} plan:{} affected:{}",
                trigger.name,
                event.id.as_str(),
                event.kind.label(),
                outcome.plan_summary,
                outcome.affected,
            ),
        });
        tracing::info!(
            target: "cfs::watchtower",
            trigger = %trigger.name,
            event = %event.id.as_str(),
            kind = %event.kind.label(),
            affected = outcome.affected,
            "trigger fired and committed"
        );
    }
}
