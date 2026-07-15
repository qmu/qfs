//! The **injected** model-provider seam (blueprint §15, decision W) — the analogue of
//! `qfs-driver-claude`'s `SessionSource`: a pure driver crate declares the trait, the `qfs`
//! binary leaf implements the live provider, and hermetic tests inject a deterministic mock. The
//! crate stays tokio-free, network-free, and credential-free; nothing here calls a model.
//!
//! ## The one-seam lock (blueprint §15: transform is the ONLY model-call seam)
//! A model call is confined to a SINGLE invocation funnel by construction AND by the type system:
//! - **The engine cannot reach a provider.** The pure evaluator drives the model through
//!   `qfs_exec::TransformExecutor` (name / mode / OUTPUT schema only) — that seam never mentions a
//!   [`ModelProvider`], so no general read/write/CALL/codec/DDL path can obtain one. The one
//!   `TransformExecutor` impl (`BinaryTransformExecutor`, the transform applier, binary-side) is
//!   the sole holder.
//! - **The trait method cannot be INVOKED off that path.** [`ModelProvider::call`] takes a
//!   crate-private witness [`CallProof`] whose only constructor lives in this crate. The sole
//!   public funnel that mints it is [`call_model`], so every model invocation in the whole
//!   workspace flows through that one function — an arbitrary driver or code path holding a
//!   `&dyn ModelProvider` still cannot call it, because it cannot forge the witness.
//! - The trait stays `pub` on purpose: §15 requires the binary LEAF to implement the live provider
//!   (and tests a mock). Implementors RECEIVE the witness; they can never FORGE one. So the seam is
//!   open to implement and sealed to invoke — sealing implementation is deliberately NOT done, as
//!   it would forbid the very live provider §15 mandates.
//!
//! ```compile_fail
//! // An external crate cannot mint the call witness (its field is private), so it cannot invoke
//! // `ModelProvider::call` outside this crate's `call_model` funnel — the one-seam lock, enforced.
//! let _forged = qfs_driver_transform::CallProof::new();
//! ```
//!
//! ## Safety floor
//! - The resolved secret rides a SEPARATE non-`Debug` parameter, never a field of the
//!   [`ModelRequest`] DTO — so no `{req:?}` log line can ever carry a credential.
//! - [`ModelError`] is structured and secret-free by contract: an implementor maps its transport
//!   error to a reason string naming WHAT failed, never a token or a prompt payload.

use qfs_types::{RowBatch, Schema, TransformMode};

/// A crate-private witness proving a [`ModelProvider::call`] is being made through THE model-call
/// funnel ([`call_model`]) and nowhere else (blueprint §15: transform is the only model-call seam).
///
/// The type is `pub` because it appears in [`ModelProvider::call`]'s signature (an external
/// implementor must be able to name the parameter). Its field is PRIVATE and its only constructor
/// ([`CallProof::new`]) is `pub(crate)`, so no code outside this crate can construct one — a model
/// call is therefore *invocable* only from [`call_model`], even by a holder of a `&dyn
/// ModelProvider`. Implementors receive the witness; they never forge it.
#[derive(Debug)]
pub struct CallProof(());

impl CallProof {
    /// Mint the witness. Crate-private: only [`call_model`] (the sole model-call funnel) calls it.
    pub(crate) fn new() -> Self {
        Self(())
    }
}

/// The SOLE model-call funnel (blueprint §15, the one-seam lock): the only path that invokes a
/// [`ModelProvider`]. It mints the crate-private [`CallProof`] witness and performs the call, so
/// every model invocation in the workspace goes through here. The transform applier
/// (`BinaryTransformExecutor`, binary-side) calls this; no other code path can, because
/// [`ModelProvider::call`] requires a witness only this crate can mint.
///
/// # Errors
/// [`ModelError`] — structured and secret-free — on a provider/transport failure, propagated
/// verbatim from the provider.
pub fn call_model(
    provider: &dyn ModelProvider,
    req: &ModelRequest<'_>,
    secret: Option<&str>,
) -> Result<RowBatch, ModelError> {
    provider.call(req, secret, &CallProof::new())
}

/// One model invocation the executor asks the provider for: the definition's non-secret
/// selectors, the derived cardinality mode, the declared OUTPUT schema the rows must satisfy,
/// and the input rows for THIS call (the executor chunks by mode — one row per call for
/// row-wise/extraction, the whole relation for relation-wise).
#[derive(Debug)]
pub struct ModelRequest<'a> {
    /// The transform definition name (a label for diagnostics/audit).
    pub name: &'a str,
    /// The provider selector (e.g. `claude`) — a label, never a credential.
    pub provider: &'a str,
    /// The model name/id the provider is asked for.
    pub model: &'a str,
    /// The optional effort/budget hint.
    pub effort: Option<&'a str>,
    /// The derived cardinality mode this call runs under.
    pub mode: TransformMode,
    /// The declared OUTPUT schema the returned rows must satisfy.
    pub output: &'a Schema,
    /// The input rows for this single call.
    pub input: &'a RowBatch,
}

/// The model-call seam the binary implements (live) and tests mock (deterministic). The resolved
/// credential is passed OUT-OF-BAND of the `Debug`-printable request — an implementor uses it for
/// transport auth and must never echo it into an error or the returned rows.
///
/// **Invoke via [`call_model`] only.** `call` takes a crate-private [`CallProof`] witness that only
/// this crate can mint, so an implementor cannot invoke its own (or another's) `call` — every model
/// invocation is funnelled through [`call_model`] (blueprint §15: transform is the only model-call
/// seam). The trait stays open to *implement* (the binary leaf's live provider, a test mock); it is
/// sealed to *invoke*.
pub trait ModelProvider: Send + Sync {
    /// Perform one model call, returning rows shaped for the declared OUTPUT schema. The
    /// [`CallProof`] witness proves the call arrived through the [`call_model`] funnel; an
    /// implementor simply accepts it (it cannot be forged, so a non-funnel call cannot exist).
    ///
    /// # Errors
    /// [`ModelError`] — structured and secret-free — on a provider/transport failure.
    fn call(
        &self,
        req: &ModelRequest<'_>,
        secret: Option<&str>,
        proof: &CallProof,
    ) -> Result<RowBatch, ModelError>;
}

/// A structured, **secret-free** model-provider error (blueprint §6, AI-consumable).
#[derive(Debug, thiserror::Error)]
pub enum ModelError {
    /// No live provider is configured for the requested provider selector. The fail-closed
    /// default: a transform COMMIT without a configured provider refuses rather than pretending.
    #[error("no model provider is configured for provider '{provider}'")]
    Unconfigured {
        /// The requested provider selector (a label).
        provider: String,
    },
    /// The provider/transport failed. The reason is secret-free by the implementor's contract.
    #[error("model provider failed: {reason}")]
    Provider {
        /// A secret-free failure reason.
        reason: String,
    },
}

/// The fail-closed default provider: every call refuses with [`ModelError::Unconfigured`]. The
/// binary composition registers this until a live provider is wired, so a transform COMMIT
/// without one fails closed with an actionable, secret-free error — never a silent no-op.
#[derive(Debug, Default, Clone, Copy)]
pub struct UnconfiguredProvider;

impl ModelProvider for UnconfiguredProvider {
    fn call(
        &self,
        req: &ModelRequest<'_>,
        _secret: Option<&str>,
        _proof: &CallProof,
    ) -> Result<RowBatch, ModelError> {
        Err(ModelError::Unconfigured {
            provider: req.provider.to_string(),
        })
    }
}
