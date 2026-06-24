//! The real `qfs run --commit` apply path: drives the `qfs-runtime` [`Interpreter`] over a live
//! driver registry to apply an effect `Plan` to the World. Injected into `qfs-cmd` as the
//! [`qfs_exec::WorldApply`] hook.
//!
//! `qfs-cmd` and `qfs-exec` are deliberately confined off `qfs-runtime` (the interpreter is the
//! sole impure stage). The terminal binary is the allowlisted runtime leaf that owns the
//! interpreter + the live drivers, so the real commit composition lives here — exactly like the
//! shell / serve / account launchers.
//!
//! Today the registry carries the **local filesystem** driver (no credentials needed), which
//! proves the commit path is real end to end: `qfs run "UPSERT INTO /local/… " --commit` actually
//! writes the file. Credentialed / networked drivers register here behind their live clients as
//! they land (the execution+auth ticket).

use std::sync::Arc;

use qfs_exec::{ErrorKind, ExecError};
use qfs_runtime::{CapabilitySet, DriverRegistry, Interpreter, LegStatus};
use qfs_types::DriverId;

/// Apply `plan` to the World via the runtime interpreter. Returns `Ok(())` once every leg applied,
/// or an [`ExecError`] (kind `commit_failed`) if a leg failed or was skipped. Builds a fresh
/// current-thread tokio runtime to drive the async interpreter (tokio dead-ends here, in the
/// terminal binary leaf). Never panics.
pub fn apply_plan(plan: &qfs_core::Plan) -> Result<(), ExecError> {
    let interp = Interpreter::with_defaults(live_registry());
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| {
            ExecError::new(
                ErrorKind::Internal,
                "runtime_init",
                format!("failed to start the commit runtime: {e}"),
            )
        })?;
    // Capability gating already ran at parse time; allow-all is the apply-time re-check for the
    // CLI one-shot (a server run gates with its POLICY instead).
    let outcome = rt
        .block_on(interp.commit(plan.clone(), &CapabilitySet::allow_all()))
        .map_err(|e| ExecError::new(ErrorKind::CommitFailed, "commit_failed", format!("{e:?}")))?;
    if !outcome.is_complete() {
        // Surface the per-leg failure reasons (structured, secret-free) so a commit failure is
        // diagnosable rather than an opaque count.
        let reasons: Vec<String> = outcome
            .ledger
            .iter()
            .filter_map(|e| match &e.status {
                LegStatus::Failed { error, .. } => Some(format!("{error:?}")),
                LegStatus::Skipped { cause } => {
                    Some(format!("skipped (dependency {cause:?} failed)"))
                }
                LegStatus::Applied { .. } => None,
                _ => Some("unknown leg status".to_string()),
            })
            .collect();
        return Err(ExecError::new(
            ErrorKind::CommitFailed,
            "commit_failed",
            reasons.join("; "),
        ));
    }
    Ok(())
}

/// The live apply-driver registry. **Local filesystem only** today (cred-free): rooted at `/` so a
/// VFS path `/local/<p>` maps to the host path `/<p>` within the driver's sandbox; real
/// `UPSERT`/`REMOVE` legs apply through its `LocalApplier`. Other drivers register here behind
/// their live clients as they land.
fn live_registry() -> DriverRegistry {
    let local = qfs_driver_local::LocalFsDriver::new("/");
    DriverRegistry::new().with(
        DriverId::new("local"),
        Arc::new(qfs_driver_local::local_apply_driver(&local)),
    )
}
