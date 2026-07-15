//! [`DriveApplier`] — the Drive driver's synchronous apply leg (blueprint §7). It is the lone
//! impure seam the introspective [`crate::GDriveDriver`] hands back via `applier()`, and the
//! [`qfs_runtime::SharedApplier`] the runtime's [`qfs_runtime::PlanApplierBridge`] drives under
//! `COMMIT`.
//!
//! Stateless across the call: it holds the [`GDriveClient`] behind an `Arc` and performs fresh
//! Drive API I/O on every call — so it implements `SharedApplier` (`&self` apply), the
//! statelessness contract the bridge requires. Each effect is decoded to a [`DriveEffect`] and
//! dispatched to the client; the token is wholly behind the client (t19), never here.
//!
//! ## Idempotency / recovery (blueprint §7)
//! `UPSERT` is the retry-safe write: a content replace by id (PATCH-by-media) is idempotent, and
//! a resumable create resumes on the same session URI rather than duplicating a file. `REMOVE`
//! defaults to **trash** (recoverable) — a permanent `Delete` requires an explicit `hard_delete`
//! flag and is irreversible, so the runtime never auto-retries it. `mv` is the planner's
//! copy→verify→delete DAG; this leg applies a single metadata move (`Move`) or the server-side
//! `Copy`, each an irreducible step the ledger records.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use qfs_plan::{AppliedEffect, ApplyError, EffectNode, PlanApplier};
use qfs_runtime::{EffectError, EffectOutput, SharedApplier};

use crate::client::GDriveClient;
use crate::effect::{DriveEffect, WriteResolver};
use crate::error::DriveError;
use crate::path::DrivePath;
use crate::schema::FileMeta;

/// The synchronous Drive apply leg. Holds the [`GDriveClient`] (the real auth-bearing client in
/// production, an in-memory mock in tests) behind an `Arc` so the leg is cheap to clone for the
/// runtime bridge and safe to share across blocking apply threads.
#[derive(Clone)]
pub struct DriveApplier {
    client: Arc<dyn GDriveClient>,
}

impl DriveApplier {
    /// Build an applier over `client`.
    #[must_use]
    pub fn new(client: Arc<dyn GDriveClient>) -> Self {
        Self { client }
    }

    /// Apply one decoded [`DriveEffect`], returning the affected count. The single place Drive
    /// API write I/O happens.
    fn apply_effect(&self, effect: &DriveEffect) -> Result<u64, DriveError> {
        match effect {
            DriveEffect::Upload {
                parent,
                name,
                mime,
                bytes,
            } => {
                self.client.upload(parent, name, mime, bytes)?;
                Ok(1)
            }
            DriveEffect::Update { id, mime, bytes } => {
                self.client.update_content(id, mime, bytes)?;
                Ok(1)
            }
            DriveEffect::Move {
                id,
                new_name,
                add_parents,
                remove_parents,
            } => {
                self.client
                    .modify_file(id, new_name.as_deref(), add_parents, remove_parents)?;
                Ok(1)
            }
            DriveEffect::Copy { id, parent, name } => {
                self.client.copy_file(id, parent, name)?;
                Ok(1)
            }
            DriveEffect::Trash { id } => {
                self.client.trash(id)?;
                Ok(1)
            }
            DriveEffect::Delete { id } => {
                self.client.delete(id)?;
                Ok(1)
            }
        }
    }
}

impl DriveApplier {
    /// Decode `node` with LIVE name→id resolution: a path-addressed write (no snapshotted
    /// `parent_id`/`file_id` — the planner snapshots ids only for effects born from a scan)
    /// resolves them here through the same walk the read path uses, at the moment of apply.
    /// A multi-row upload decodes to one effect PER ROW (ticket 20260712005000); the resolver is
    /// memoized so the batch's shared destination walk runs once, not once per row.
    fn decode_rows(&self, node: &EffectNode) -> Result<Vec<DriveEffect>, DriveError> {
        let resolver = MemoResolver::new(crate::read::ClientResolver {
            client: self.client.as_ref(),
        });
        DriveEffect::from_node_rows_with(node, &resolver)
    }

    /// Decode and apply EVERY row's operation, returning the count of operations that actually
    /// ran (honest counts: `affected` equals files written, never the source row count). The
    /// whole batch decodes — payloads, destinations, create-only probes — before the first write,
    /// so a malformed row aborts with nothing applied; an API failure mid-batch surfaces as
    /// [`DriveError::PartialApply`] naming exactly how far the batch got, never as full success.
    fn apply_all(&self, node: &EffectNode) -> Result<u64, DriveError> {
        let effects = self.decode_rows(node)?;
        let total = effects.len();
        let mut applied = 0u64;
        for (row, effect) in effects.iter().enumerate() {
            match self.apply_effect(effect) {
                Ok(n) => applied += n,
                Err(source) if total > 1 => {
                    return Err(DriveError::PartialApply {
                        applied,
                        total,
                        row,
                        reason: source.to_string(),
                    });
                }
                Err(source) => return Err(source),
            }
        }
        Ok(applied)
    }
}

/// Memoizes the [`WriteResolver`] walks across a multi-row decode: every row of a batched upload
/// names the same destination folder, so the parent walk (and the UPSERT existing-node probe)
/// runs once per distinct path instead of once per row. The create-only `child_id` probe is NOT
/// cached — each row probes its own leaf name. Single-use per apply call (interior mutability
/// only lives for the decode), so no staleness outlives the statement.
struct MemoResolver<R> {
    inner: R,
    folders: RefCell<HashMap<String, (String, Option<String>)>>,
    existing: RefCell<HashMap<String, Option<FileMeta>>>,
}

impl<R> MemoResolver<R> {
    fn new(inner: R) -> Self {
        Self {
            inner,
            folders: RefCell::new(HashMap::new()),
            existing: RefCell::new(HashMap::new()),
        }
    }
}

impl<R: WriteResolver> WriteResolver for MemoResolver<R> {
    fn folder_id(
        &self,
        path: &DrivePath,
        raw: &str,
    ) -> Result<(String, Option<String>), DriveError> {
        let key = format!("{path:?}");
        if let Some(hit) = self.folders.borrow().get(&key) {
            return Ok(hit.clone());
        }
        let resolved = self.inner.folder_id(path, raw)?;
        self.folders.borrow_mut().insert(key, resolved.clone());
        Ok(resolved)
    }

    fn existing(&self, path: &DrivePath, raw: &str) -> Result<Option<FileMeta>, DriveError> {
        let key = format!("{path:?}");
        if let Some(hit) = self.existing.borrow().get(&key) {
            return Ok(hit.clone());
        }
        let resolved = self.inner.existing(path, raw)?;
        self.existing.borrow_mut().insert(key, resolved.clone());
        Ok(resolved)
    }

    fn child_id(&self, parent_id: &str, name: &str) -> Result<Option<String>, DriveError> {
        self.inner.child_id(parent_id, name)
    }
}

impl SharedApplier for DriveApplier {
    fn apply_shared(&self, node: &EffectNode) -> Result<EffectOutput, EffectError> {
        let affected = self.apply_all(node)?;
        Ok(EffectOutput::new(node.id, affected))
    }
}

impl PlanApplier for DriveApplier {
    /// The introspective `qfs_driver::Driver::applier()` seam (t09): a synchronous, `&mut self`
    /// apply leg. The Drive applier is stateless, so this delegates to the same `&self` core as
    /// [`SharedApplier::apply_shared`]. The structured [`DriveError`] is reduced to the plan
    /// crate's owned `(id, reason)` shape — secret-free by construction — so no driver type leaks
    /// into `qfs-plan`.
    fn apply(&mut self, node: &EffectNode) -> Result<AppliedEffect, ApplyError> {
        let affected = self
            .apply_all(node)
            .map_err(|e| ApplyError::new(node.id, e.to_string()))?;
        Ok(AppliedEffect::new(node.id, affected))
    }
}
