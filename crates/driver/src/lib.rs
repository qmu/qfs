//! `cfs-driver` — the driver contract (RFD-0001 §5).
//!
//! A driver declares its namespace, per-node archetype + schema, capabilities,
//! procedures, and prelude — and that declaration is everything the engine and the
//! AI need. This crate defines the **consumer-side narrow trait** every E4 driver
//! fills, plus the owned-DTO conventions that keep vendor SDK types from crossing
//! the boundary (§9, boundary B3 — the direct generalisation of the Go
//! `internal/gmail` SDK quarantine).
//!
//! ## Purity invariant at the type level (fidelity guard G3, boundary B4)
//! Every method on [`Driver`] returns **data** — a [`NodeSchema`], a
//! [`Capabilities`], a slice of [`ProcedureDecl`]/[`AliasFn`]. **No method takes
//! `&mut self`, returns a future, or performs I/O.** The lone impure seam
//! (`COMMIT : Plan -> World`) is *deliberately absent* from this trait (reserved
//! for E2). This makes it structurally impossible for a driver to do I/O at
//! describe/capability time; the in-crate test [`tests::dummy_driver_is_pure`]
//! proves it by instantiating a no-I/O dummy driver.
//!
//! ## Shared primitives
//! This crate owns [`CfsError`] and [`Path`] (decision D1 — see [`error`]) because
//! the trait signatures need them and the acyclic spine forbids reaching up into
//! `cfs-core`.
//!
//! ## wasm-friendliness (boundary guard B7)
//! No threads, no `std::fs`, no sockets. I/O lives in (future) driver *impls*, never
//! in this contract crate.

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

mod error;
mod path;

pub use error::CfsError;
pub use path::Path;

use cfs_plan::Plan;

/// How a node maps onto cfs's uniform model (RFD-0001 §5, "Four archetypes").
///
/// A single driver may expose multiple archetypes on different sub-paths (git is
/// all three: versioned-blob FS, relational history, mutable pointers).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Archetype {
    /// `ls cp mv rm` — local FS, S3/R2, Drive, repo files, Slack files.
    BlobNamespace,
    /// `SELECT JOIN INSERT UPDATE` — SQL DBs, D1, Notion DB.
    RelationalTable,
    /// `SELECT(tail) INSERT(append)` — Slack, mail, CF Queues, comments, webhooks.
    AppendLog,
    /// CRUD + `CALL` procs — GitHub, Linear, K8s.
    ObjectGraphWorkflow,
}

/// The set of universal verbs a node supports (RFD-0001 §5). Unsupported verbs are
/// rejected **at parse time** with a structured [`CfsError`] — important for AI.
///
/// E0 ships the shape; per-verb gating logic lands with real drivers (E4) and
/// enforcement with capability checks (E5). The fields are owned booleans — no
/// vendor type, no I/O.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Capabilities {
    /// Supports `SELECT` (read).
    pub select: bool,
    /// Supports `INSERT INTO`.
    pub insert: bool,
    /// Supports `UPDATE`.
    pub update: bool,
    /// Supports `UPSERT INTO`.
    pub upsert: bool,
    /// Supports `REMOVE`.
    pub remove: bool,
}

/// The schema of a node: its archetype plus its columns (powers `DESCRIBE`, §5).
///
/// E0 ships a minimal owned shape; typed column descriptors land in E1/E3.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct NodeSchema {
    /// How this node maps onto the uniform model.
    pub archetype: Archetype,
    /// Column names exposed at this node (owned; typed columns come later).
    pub columns: Vec<String>,
}

impl NodeSchema {
    /// Construct a node schema. Provided because the struct is `#[non_exhaustive]`,
    /// so out-of-crate driver impls (E4) cannot use a struct literal.
    #[must_use]
    pub fn new(archetype: Archetype, columns: Vec<String>) -> Self {
        Self { archetype, columns }
    }
}

/// Declaration of a domain procedure callable via `CALL driver.action(...)`
/// (RFD-0001 §3/§5 — the irreducible state transitions).
///
/// `CALL` only resolves procedures a driver declares (capability). Owned data only.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ProcedureDecl {
    /// The unqualified action name, e.g. `send`, `merge`. Qualified at the call site
    /// by the driver mount (`mail.send` vs `git.merge`).
    pub name: String,
}

impl ProcedureDecl {
    /// Construct a procedure declaration. Provided because the struct is
    /// `#[non_exhaustive]` (out-of-crate driver impls cannot use a struct literal).
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into() }
    }
}

/// A pure alias function shipped in a driver's prelude (RFD-0001 §3, e.g.
/// `fn SEND(d) = d |> CALL mail.send`).
///
/// Aliases are **pure functions in the registry**, never keywords; they desugar to
/// a `CALL` and are in scope only for plans whose driver provides them
/// (receiver-typed resolution). Owned data only.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct AliasFn {
    /// The alias surface name, e.g. `SEND`.
    pub name: String,
    /// The qualified procedure it desugars to, e.g. `mail.send`.
    pub desugars_to: String,
}

impl AliasFn {
    /// Construct an alias function. Provided because the struct is
    /// `#[non_exhaustive]` (out-of-crate driver impls cannot use a struct literal).
    #[must_use]
    pub fn new(name: impl Into<String>, desugars_to: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            desugars_to: desugars_to.into(),
        }
    }
}

/// The consumer-side narrow driver trait (RFD-0001 §5, §9).
///
/// Every method returns **data** (or a `Plan` node) — see the purity invariant in
/// the crate docs. There is intentionally no I/O-capable method and no `COMMIT`
/// here; the interpreter (the one impure op) lives in E2's `cfs-plan` interpreter,
/// not on this trait.
pub trait Driver: Send + Sync {
    /// The mount point this driver answers for, e.g. `/mail`, `/s3`.
    fn mount(&self) -> &str;

    /// Describe a node's schema (powers `DESCRIBE`). Pure: returns data, no I/O.
    ///
    /// # Errors
    /// Returns [`CfsError`] if the path does not resolve to a describable node.
    fn describe(&self, path: &Path) -> Result<NodeSchema, CfsError>;

    /// The capability set for a node — used to gate verbs at parse time (§5). Pure.
    fn capabilities(&self, path: &Path) -> Capabilities;

    /// The `CALL` targets this driver declares. Pure: returns owned data.
    fn procedures(&self) -> &[ProcedureDecl];

    /// Optional pure alias functions shipped with the driver (e.g. `SEND`). Pure.
    fn prelude(&self) -> &[AliasFn] {
        &[]
    }
}

/// Reserved seam (do not call at E0): the only impure operation in cfs is the
/// interpreter that applies a [`Plan`] to the world (`COMMIT : Plan -> World`,
/// RFD §3 purity invariant). It is **deliberately not a `Driver` method** and is
/// reserved for E2. This zero-sized marker documents that the seam is reserved, not
/// forgotten, and keeps `cfs_plan::Plan` referenced from the contract crate.
#[doc(hidden)]
pub const fn _commit_seam_reserved_for_e2(_plan: &Plan) {
    // TODO(E2): the effect-plan interpreter applies a Plan to the world. It belongs
    // to the runtime, never to the pure Driver trait above.
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A dummy driver that performs **no I/O** — exists only to prove the seam
    /// instantiates and that the trait's signatures permit a purely in-memory impl.
    struct DummyDriver {
        procs: Vec<ProcedureDecl>,
    }

    impl Driver for DummyDriver {
        fn mount(&self) -> &str {
            "/dummy"
        }

        fn describe(&self, _path: &Path) -> Result<NodeSchema, CfsError> {
            // Pure: builds data in memory; touches no filesystem, network, or clock.
            Ok(NodeSchema {
                archetype: Archetype::BlobNamespace,
                columns: vec!["name".to_string()],
            })
        }

        fn capabilities(&self, _path: &Path) -> Capabilities {
            Capabilities {
                select: true,
                ..Capabilities::default()
            }
        }

        fn procedures(&self) -> &[ProcedureDecl] {
            &self.procs
        }
    }

    /// G3 — the purity proof. If a `Driver` method *could* do I/O it would need
    /// `&mut self`, a future, or an executor; none are in the signatures, so this
    /// no-I/O impl compiling and round-tripping data IS the type-level proof.
    #[test]
    fn dummy_driver_is_pure() {
        let d = DummyDriver {
            procs: vec![ProcedureDecl {
                name: "noop".to_string(),
            }],
        };
        let p = Path::new("/dummy/x");
        let schema = d.describe(&p).unwrap();
        assert_eq!(schema.archetype, Archetype::BlobNamespace);
        assert!(d.capabilities(&p).select);
        assert_eq!(d.procedures().len(), 1);
        assert!(d.prelude().is_empty());
        assert_eq!(d.mount(), "/dummy");
    }

    /// The driver is object-safe (`dyn Driver`) — required because the registries
    /// store `Arc<dyn Driver>` (G2: registries generic over the trait object).
    #[test]
    fn driver_is_object_safe() {
        let d: std::sync::Arc<dyn Driver> = std::sync::Arc::new(DummyDriver { procs: vec![] });
        assert_eq!(d.mount(), "/dummy");
    }
}
