//! The effect node: [`EffectKind`] (the closed-core write verbs) and [`EffectNode`]
//! (one effect, fully described as data). RFD-0001 §3 (closed core) / §6 (runtime).

use cfs_types::RowBatch;
use serde::Serialize;

use crate::ids::{Affected, NodeId, ProcId, Target};

/// The kind of an effect — a **closed set** mirroring the frozen core write verbs
/// (RFD §3). A new backend adds **zero** variants here; it routes through a `Target`
/// driver id and (for [`EffectKind::Call`]) a [`ProcId`] string instead.
///
/// `Read`/`List` are pure data-acquisition nodes the evaluator may emit as plan
/// dependencies of a write (e.g. an `UPDATE … FROM <query>`); they are reversible.
/// `Insert`/`Upsert`/`Update`/`Remove`/`Call` are the write verbs. `Upsert` is modelled
/// **distinctly** from `Insert` so retry-safe (idempotent) effects are first-class
/// (RFD §6 recovery).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum EffectKind {
    /// Read rows from the target (a pure dependency of a downstream write).
    Read,
    /// List entries at the target path (a pure dependency of a downstream write).
    List,
    /// `INSERT INTO <path>` — create rows / objects.
    Insert,
    /// `UPSERT INTO <path>` — idempotent create-or-update (retry-safe, §6).
    Upsert,
    /// `UPDATE <path>` — modify existing rows / objects.
    Update,
    /// `REMOVE <path>` — delete rows / objects (irreversible, §10).
    Remove,
    /// `CALL <driver>.<action>(...)` — an irreducible namespaced procedure (§3). The
    /// [`ProcId`] is the registry name; irreversibility is carried per-node.
    Call(ProcId),
}

impl EffectKind {
    /// Whether this kind is *inherently* irreversible regardless of the procedure
    /// declaration. `Remove` always destroys data (RFD §10). `Call` irreversibility is
    /// declared per-procedure by the planner (e.g. `mail.send`) and recorded on the
    /// node, so it is **not** decided here.
    #[must_use]
    pub fn is_inherently_irreversible(&self) -> bool {
        matches!(self, EffectKind::Remove)
    }

    /// A short, stable label for previews / golden snapshots.
    #[must_use]
    pub fn label(&self) -> &str {
        match self {
            EffectKind::Read => "READ",
            EffectKind::List => "LIST",
            EffectKind::Insert => "INSERT",
            EffectKind::Upsert => "UPSERT",
            EffectKind::Update => "UPDATE",
            EffectKind::Remove => "REMOVE",
            EffectKind::Call(_) => "CALL",
        }
    }
}

/// One fully-described effect: what it does ([`EffectKind`]), where it lands
/// ([`Target`]), the data it carries ([`RowBatch`] from `cfs-types`), whether it is
/// irreversible, and how many rows it is estimated to touch ([`Affected`]).
///
/// `#[non_exhaustive]` so representation can gain internal fields without a breaking
/// grammar change (the *grammar* stays frozen; the plan representation may evolve).
/// Carries **no secrets** — safe to render in a preview and log (RFD §10).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[non_exhaustive]
pub struct EffectNode {
    /// The plan-local node identity (dependency endpoint, topo tie-breaker).
    pub id: NodeId,
    /// What this effect does.
    pub kind: EffectKind,
    /// Where the effect lands (driver + virtual path).
    pub target: Target,
    /// The rows the effect writes (empty for `Read`/`List`/`Remove`-by-filter).
    pub args: RowBatch,
    /// Whether applying this effect cannot be undone (`Remove`, declared-irreversible
    /// `Call`). Surfaced explicitly in [`preview`](crate::preview).
    pub irreversible: bool,
    /// The estimated row count this effect touches (honest: `Exact`/`AtMost`/`Unknown`).
    pub est_affected: Affected,
}

impl EffectNode {
    /// Construct a node, deriving `irreversible` for inherently-irreversible kinds
    /// (`Remove`) and leaving `Call` to the explicit builder below.
    #[must_use]
    pub fn new(id: NodeId, kind: EffectKind, target: Target) -> Self {
        let irreversible = kind.is_inherently_irreversible();
        Self {
            id,
            kind,
            target,
            args: RowBatch::default(),
            irreversible,
            est_affected: Affected::Unknown,
        }
    }

    /// Builder: attach the row batch the effect writes, refining `est_affected` to the
    /// exact row count when it was previously `Unknown` (an explicit estimate set via
    /// [`EffectNode::with_affected`] is preserved).
    #[must_use]
    pub fn with_args(mut self, args: RowBatch) -> Self {
        if matches!(self.est_affected, Affected::Unknown) {
            // A literal row batch gives an exact count; saturating cast is fine since
            // a usize row count fits u64 on every supported target.
            self.est_affected = Affected::Exact(args.rows.len() as u64);
        }
        self.args = args;
        self
    }

    /// Builder: set the affected-row estimate explicitly (honest bounds for
    /// filter-driven effects whose count is not known until apply).
    #[must_use]
    pub fn with_affected(mut self, est: Affected) -> Self {
        self.est_affected = est;
        self
    }

    /// Builder: mark this node irreversible explicitly (e.g. a declared-irreversible
    /// `Call` such as `mail.send`). Inherently-irreversible kinds are already flagged.
    #[must_use]
    pub fn irreversible(mut self, yes: bool) -> Self {
        self.irreversible = self.irreversible || yes;
        self
    }
}
