//! `cfs-plan` — the effect substrate (RFD-0001 §3 purity invariant, §6 runtime).
//!
//! cfs's central safety property is **effects-as-data**: write operators
//! (`cp/mv/INSERT/UPSERT/UPDATE/REMOVE/CALL`) do not execute — they evaluate to a
//! [`Plan`], a typed DAG of [`Effect`]s. The only impure operation is the
//! interpreter (`COMMIT : Plan -> World -> World`), which is **deliberately absent
//! from E0** (reserved for E2). The existence of this crate as the sole effect type
//! is what anchors the purity invariant: a `Driver`'s methods return data /
//! `Plan` nodes, never a future or a unit-with-side-effects (fidelity guard G3,
//! boundary B4).
//!
//! E0 ships construction-only placeholders. There is no `apply`/`commit` here.
//!
//! ## wasm-friendliness (boundary guard B7)
//! Pure data: no threads, no `std::fs`, no sockets.

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

/// A single effect node in a [`Plan`].
///
/// The variants mirror the RFD §3 effect verbs. The set is `#[non_exhaustive]` and
/// intentionally minimal at E0 — E2 fills in the payloads (target path, rows,
/// procedure reference, …) and the interpreter that applies them.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Effect {
    /// `INSERT INTO <path>` — create rows / objects at a path.
    Insert,
    /// `UPSERT INTO <path>` — idempotent create-or-update (retry-safe, §6).
    Upsert,
    /// `UPDATE <path>` — modify existing rows / objects.
    Update,
    /// `REMOVE <path>` — delete rows / objects.
    Remove,
    /// `CALL driver.action(...)` — an irreducible namespaced procedure (§3).
    Call,
}

/// A typed DAG of effects: the value a write statement evaluates to (RFD §6).
///
/// `nodes` are the effects; `deps` are edges `(from, to)` meaning the effect at
/// index `from` must be applied before the effect at index `to`. Independent
/// subgraphs may be auto-parallelised by the (future) interpreter (Haxl-style §6).
///
/// `irreversible` is the seam reserved per §6/§10: a plan containing an
/// irreversible effect (`CALL mail.send`, a hard delete) is where `PREVIEW` +
/// `POLICY` earn their keep. E0 only carries the flag; enforcement lands later.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Plan {
    nodes: Vec<Effect>,
    deps: Vec<(usize, usize)>,
    irreversible: bool,
}

impl Plan {
    /// An empty plan (no effects, reversible).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The effect nodes of this plan.
    #[must_use]
    pub fn nodes(&self) -> &[Effect] {
        &self.nodes
    }

    /// The dependency edges `(from, to)` of this plan.
    #[must_use]
    pub fn deps(&self) -> &[(usize, usize)] {
        &self.deps
    }

    /// Whether this plan contains an irreversible effect (§6/§10 seam).
    #[must_use]
    pub fn is_irreversible(&self) -> bool {
        self.irreversible
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_plan_is_reversible_and_has_no_nodes() {
        let p = Plan::new();
        assert!(p.nodes().is_empty());
        assert!(p.deps().is_empty());
        assert!(!p.is_irreversible());
    }

    #[test]
    fn effect_variants_are_constructible() {
        // Construction-only: E0 proves the type exists; E2 adds the interpreter.
        let effects = [
            Effect::Insert,
            Effect::Upsert,
            Effect::Update,
            Effect::Remove,
            Effect::Call,
        ];
        assert_eq!(effects.len(), 5);
    }
}
