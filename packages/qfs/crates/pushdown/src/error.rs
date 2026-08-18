//! [`PlanError`] — the structured, AI-consumable error of the pushdown planner (blueprint §6).
//!
//! Capability/policy denial fails **at plan time** (blueprint §6 parse-time rejection mirror),
//! never as a partial scan; an unknown source is its own arm. No credentials appear.

use crate::logical::SourceId;

/// A structured pushdown-planning error. `#[non_exhaustive]` so later epics can add
/// arms (cost-model rejections, policy denials) without breaking exhaustive matches.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[non_exhaustive]
pub enum PlanError {
    /// A subtree resolved to a [`SourceId`] that is not registered. The planner cannot
    /// negotiate pushdown with an unknown source, so it rejects at plan time rather than
    /// emitting a scan that would fail at execution.
    UnknownSource {
        /// The unregistered source.
        source: String,
    },
    /// The source's capabilities deny an operation in its subtree (blueprint §6/§8). Rejected
    /// at plan time — never a partial scan. Carries the source and the denied op label.
    CapabilityDenied {
        /// The source whose capabilities denied the op.
        source: String,
        /// A stable label for the denied operation (e.g. `SELECT`, `aggregate`).
        op: &'static str,
    },
    /// A caller-written `SELECT` named a column the relation's **described, non-empty** schema
    /// does not carry (ticket 20260725113000). Refused at plan time because a name-only
    /// projection is normally *pushed* to the driver, which narrows to what it can find and
    /// therefore has no channel to report the miss: the query used to answer rows of nothing at
    /// exit 0. A relation whose schema is empty (late-bound, post-decode) is never refused here —
    /// the engine refuses it at runtime over the batch the driver actually delivered.
    UnknownColumn {
        /// The stage that named it — `select` today, the only plan-time projection.
        stage: &'static str,
        /// The column the stage named.
        name: String,
        /// The columns the relation does carry, for the operator to correct against.
        available: Vec<String>,
    },
}

impl PlanError {
    /// A stable, machine-readable code an AI-facing caller branches on (blueprint §6).
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            PlanError::UnknownSource { .. } => "unknown_source",
            PlanError::CapabilityDenied { .. } => "capability_denied",
            // The SAME code the engine's runtime refusal carries: one mistake, one identity,
            // whichever side of the plan/execute line the operator meets it on.
            PlanError::UnknownColumn { .. } => "unknown_column",
        }
    }

    pub(crate) fn unknown_source(source: &SourceId) -> Self {
        PlanError::UnknownSource {
            source: source.as_str().to_string(),
        }
    }

    pub(crate) fn capability_denied(source: &SourceId, op: &'static str) -> Self {
        PlanError::CapabilityDenied {
            source: source.as_str().to_string(),
            op,
        }
    }

    pub(crate) fn unknown_column(stage: &'static str, name: &str, available: Vec<String>) -> Self {
        PlanError::UnknownColumn {
            stage,
            name: name.to_string(),
            available,
        }
    }
}

impl std::fmt::Display for PlanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PlanError::UnknownSource { source } => {
                write!(f, "unknown source `{source}`")
            }
            PlanError::CapabilityDenied { source, op } => {
                write!(f, "source `{source}` denies operation `{op}`")
            }
            // Word-for-word the engine's `EngineError::UnknownColumn` sentence: the operator
            // must not be able to tell which seam refused, only what to fix.
            PlanError::UnknownColumn {
                stage,
                name,
                available,
            } => write!(
                f,
                "`{stage}` names column '{name}', which this relation does not carry; \
                 available: [{}]",
                available.join(", ")
            ),
        }
    }
}

impl std::error::Error for PlanError {}
