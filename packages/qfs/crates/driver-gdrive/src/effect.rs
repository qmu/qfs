//! [`DriveEffect`] — the owned effect the driver realises a plan leaf as (blueprint §7), and the
//! decode from a runtime [`EffectNode`] onto it. The applier ([`crate::applier`]) interprets one
//! of these against the Drive API under `COMMIT`.
//!
//! ## Why an explicit effect enum
//! The closed core [`EffectKind`] (`Insert`/`Upsert`/`Update`/`Remove`/`Call`) is universal. The
//! Drive driver maps each onto a concrete Drive op via the `(kind, path, args)` triple:
//! - `INSERT INTO /drive/...`   → [`DriveEffect::Upload`] (a fresh file under a resolved parent)
//! - `UPSERT INTO /drive/...`   → [`DriveEffect::Update`] (retry-safe content replace by id) or
//!   [`DriveEffect::Upload`] when no `file_id` key is present (create)
//! - `UPDATE /drive/...`        → [`DriveEffect::Move`] (rename and/or re-parent)
//! - `REMOVE id:<file>`         → [`DriveEffect::Trash`] (default; irreversible) or
//!   [`DriveEffect::Delete`] when the `hard_delete` flag column is set (irreversible)
//! - `CALL drive.copy`          → [`DriveEffect::Copy`] (server-side copy; the `cp` apply)
//!
//! The well-known row columns carry the resolved ids/bytes the planner snapshotted at plan time
//! (blueprint §6 snapshot-resolution). No vendor type appears here. `Trash`/`Delete` carry
//! `irreversible = true` for blueprint §7 PREVIEW gating.

use qfs_plan::{EffectKind, EffectNode};
use qfs_types::Value;

use crate::error::DriveError;
use crate::path::DrivePath;
use crate::schema::{FileMeta, FOLDER_MIME};

/// The live name→id resolution seam the effect decode consults when a well-known id column is
/// ABSENT from the effect row (the planner snapshots ids only for effects born from a scan —
/// a path-addressed write never had one). The applier supplies a client-backed implementation
/// ([`crate::read::ClientResolver`]); the pure [`DriveEffect::from_node`] supplies a refusing
/// one, preserving the fail-closed decode for callers with no client.
pub(crate) trait WriteResolver {
    /// Resolve the FOLDER a path names — the upload/mkdir destination. The My Drive corpus
    /// root resolves to Drive's reserved `root` alias. `raw` is the effect's original path
    /// string (for error context). Returns `(folder_id, drive_id)`.
    ///
    /// # Errors
    /// [`DriveError`] when the path names no folder (missing, or a file), or on API failure.
    fn folder_id(
        &self,
        path: &DrivePath,
        raw: &str,
    ) -> Result<(String, Option<String>), DriveError>;

    /// Resolve the EXISTING node a path names, or `None` when nothing is there (the
    /// create-vs-replace branch of `UPSERT`, and the id source for path-addressed
    /// `REMOVE`/`UPDATE`).
    ///
    /// # Errors
    /// [`DriveError`] on API failure (a missing node is `Ok(None)`, not an error).
    fn existing(&self, path: &DrivePath, raw: &str) -> Result<Option<FileMeta>, DriveError>;

    /// Whether a child named `name` already exists directly under the already-resolved
    /// `parent_id` folder — the create-only `INSERT` probe (ticket 20260708000100). Returns the
    /// existing child's id (any one, if several share the name — a create refuses regardless), or
    /// `None` when the name is free.
    ///
    /// # Errors
    /// [`DriveError`] on API failure.
    fn child_id(&self, parent_id: &str, name: &str) -> Result<Option<String>, DriveError>;
}

/// The refusing resolver behind the pure [`DriveEffect::from_node`]: every resolution request
/// fails exactly like the pre-resolution decode did, so callers with no live client keep the
/// fail-closed contract (and the original error texts).
struct NoResolve;

impl WriteResolver for NoResolve {
    fn folder_id(
        &self,
        _path: &DrivePath,
        raw: &str,
    ) -> Result<(String, Option<String>), DriveError> {
        Err(DriveError::MalformedEffect {
            verb: "INSERT",
            path: raw.to_string(),
            reason: format!("upload needs the resolved `{PARENT_ID_COL}`"),
        })
    }

    fn existing(&self, _path: &DrivePath, _raw: &str) -> Result<Option<FileMeta>, DriveError> {
        Ok(None)
    }

    fn child_id(&self, _parent_id: &str, _name: &str) -> Result<Option<String>, DriveError> {
        Ok(None)
    }
}

/// Row column carrying the resolved parent folder id (the upload destination).
pub const PARENT_ID_COL: &str = "parent_id";
/// Row column carrying the destination folder PATH for a copy — resolved to its id live at
/// apply time (the `cp`-parity form, so a recipe names a folder path, not an opaque id).
pub const PARENT_PATH_COL: &str = "parent_path";
/// Row column carrying the resolved file id (the UPSERT/UPDATE/REMOVE key).
pub const FILE_ID_COL: &str = "file_id";
/// Row column carrying the file name (upload / rename).
pub const NAME_COL: &str = "name";
/// Row column carrying the MIME type for an upload.
pub const MIME_COL: &str = "mime_type";
/// Row column carrying the file content bytes for an upload/update.
pub const BYTES_COL: &str = "bytes";
/// Row column carrying parent ids to add (comma-separated) for a move.
pub const ADD_PARENTS_COL: &str = "add_parents";
/// Row column carrying parent ids to remove (comma-separated) for a move.
pub const REMOVE_PARENTS_COL: &str = "remove_parents";
/// Row column flagging an irreversible **permanent** delete instead of the default trash.
pub const HARD_DELETE_COL: &str = "hard_delete";

/// One fully-decoded Drive effect — what the apply leg executes against the API. Owned DTOs; no
/// google type appears here. `Trash`, `Delete` are irreversible (blueprint §8).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum DriveEffect {
    /// Create a new file under `parent` (`INSERT`, or `UPSERT` with no `file_id`).
    Upload {
        /// The resolved parent folder id.
        parent: String,
        /// The new file name.
        name: String,
        /// The MIME type.
        mime: String,
        /// The file content bytes.
        bytes: Vec<u8>,
    },
    /// Replace an existing file's content by id (`UPSERT` with a `file_id`) — retry-safe.
    Update {
        /// The file id to replace.
        id: String,
        /// The MIME type.
        mime: String,
        /// The new content bytes.
        bytes: Vec<u8>,
    },
    /// Rename and/or re-parent a file (`UPDATE`) — the metadata-only move.
    Move {
        /// The file id to move/rename.
        id: String,
        /// The new name, if renamed.
        new_name: Option<String>,
        /// Parent ids to add.
        add_parents: Vec<String>,
        /// Parent ids to remove.
        remove_parents: Vec<String>,
    },
    /// Server-side copy a file (`CALL drive.copy` / the `cp` apply).
    Copy {
        /// The source file id.
        id: String,
        /// The destination parent id.
        parent: String,
        /// The copy's name.
        name: String,
    },
    /// Trash a file (`REMOVE` default) — irreversible but recoverable from trash, **not** a
    /// permanent delete.
    Trash {
        /// The file id to trash.
        id: String,
    },
    /// Permanently delete a file (`REMOVE` with `hard_delete = true`) — irreversible.
    Delete {
        /// The file id to permanently delete.
        id: String,
    },
}

impl DriveEffect {
    /// Decode a runtime [`EffectNode`] into the concrete Drive operation, PURELY — no I/O. An
    /// effect whose id columns were never snapshotted (a path-addressed write) fails closed
    /// exactly as before; the applier uses [`DriveEffect::from_node_with`] instead, which
    /// resolves those ids live.
    ///
    /// # Errors
    /// [`DriveError`] if the `(kind, path)` pair is not one the Drive driver services, or the
    /// row args carry no usable payload.
    pub fn from_node(node: &EffectNode) -> Result<Self, DriveError> {
        Self::from_node_with(node, &NoResolve)
    }

    /// Decode a runtime [`EffectNode`] into the concrete Drive operation, resolving a missing
    /// `parent_id`/`file_id` from the effect's PATH through `res` (the live name→id walk). The
    /// snapshotted id columns, when present, always win — resolution runs only for the columns
    /// the planner could not know.
    ///
    /// # Errors
    /// [`DriveError`] if the `(kind, path)` pair is not serviced, the row args carry no usable
    /// payload, or the path resolution fails (missing parent, API failure).
    pub(crate) fn from_node_with(
        node: &EffectNode,
        res: &dyn WriteResolver,
    ) -> Result<Self, DriveError> {
        Self::decode_row(node, 0, res)
    }

    /// Decode EVERY row of the effect into its own Drive operation — one file per row for the
    /// upload-shaped kinds. This is the honest-count decode (ticket 20260712005000): a multi-row
    /// `INSERT INTO /drive/<folder>` previously collapsed to the first row's upload while the
    /// committed summary reported every source row as affected. Single-row (and row-less) nodes
    /// decode exactly as before. The id-addressed kinds (`UPDATE`/`REMOVE`/`CALL`) target one
    /// node per statement, so a multi-row batch fails closed rather than silently dropping the
    /// extra rows. Decoding validates the WHOLE batch (payload presence, destination resolution,
    /// create-only probes) before the applier writes anything.
    ///
    /// # Errors
    /// [`DriveError`] as [`Self::from_node_with`], for whichever row fails to decode.
    pub(crate) fn from_node_rows_with(
        node: &EffectNode,
        res: &dyn WriteResolver,
    ) -> Result<Vec<Self>, DriveError> {
        let rows = node.args.rows.len();
        if rows <= 1 {
            return Ok(vec![Self::decode_row(node, 0, res)?]);
        }
        match &node.kind {
            EffectKind::Insert | EffectKind::Upsert => (0..rows)
                .map(|row| Self::decode_row(node, row, res))
                .collect(),
            other => Err(DriveError::MalformedEffect {
                verb: "EFFECT",
                path: node.target.path.as_str().to_string(),
                reason: format!(
                    "{rows} rows for a single-target {} — this verb addresses exactly one node \
                     per statement",
                    other.label()
                ),
            }),
        }
    }

    /// Decode the operation row `row` of the effect carries.
    fn decode_row(
        node: &EffectNode,
        row: usize,
        res: &dyn WriteResolver,
    ) -> Result<Self, DriveError> {
        let path = DrivePath::parse_str(node.target.path.as_str())?;
        match &node.kind {
            EffectKind::Insert => Self::decode_insert(node, row, &path, res),
            EffectKind::Upsert => Self::decode_upsert(node, row, &path, res),
            EffectKind::Update => Self::decode_move(node, row, &path, res),
            EffectKind::Remove => Self::decode_remove(node, row, &path, res),
            EffectKind::Call(proc) => Self::decode_call(proc.as_str(), node, row, &path, res),
            other => Err(DriveError::MalformedEffect {
                verb: "EFFECT",
                path: node.target.path.as_str().to_string(),
                reason: format!("{} is not serviced by the Drive driver", other.label()),
            }),
        }
    }

    /// The upload destination folder: the snapshotted `parent_id` when present; else resolved
    /// from the path. With an explicit `name` column the TARGET path is the parent collection
    /// (`INSERT INTO /drive/my/Reports VALUES (name, …)` creates inside Reports — and the
    /// corpus root works the same way); without one the path is the blob itself, so its
    /// CONTAINING folder is the parent and the terminal segment is the name.
    fn upload_destination(
        node: &EffectNode,
        row: usize,
        path: &DrivePath,
        res: &dyn WriteResolver,
    ) -> Result<(String, String), DriveError> {
        let raw = node.target.path.as_str();
        if let Some(parent) = text_col(node, row, PARENT_ID_COL) {
            return Ok((parent, upload_name(node, row, path)?));
        }
        if let Some(name) = text_col(node, row, NAME_COL) {
            let (parent, _) = res.folder_id(path, raw)?;
            return Ok((parent, name));
        }
        let leaf =
            path.leaf_name()
                .map(str::to_string)
                .ok_or_else(|| DriveError::MalformedEffect {
                    verb: "INSERT",
                    path: raw.to_string(),
                    reason: format!("upload needs a `{NAME_COL}` or a named path segment"),
                })?;
        let parent_path = path.parent().ok_or_else(|| DriveError::MalformedEffect {
            verb: "INSERT",
            path: raw.to_string(),
            reason: "this address has no containing folder to upload into".to_string(),
        })?;
        let (parent, _) = res.folder_id(&parent_path, raw)?;
        Ok((parent, leaf))
    }

    /// Decode a create-only `INSERT` (ticket 20260708000100). An `INSERT` never replaces: if the
    /// target name already resolves to a Drive node, refuse with [`DriveError::TargetExists`] so an
    /// inferred copy cannot silently overwrite an operator's file. The explicit content replace is
    /// `UPSERT`. The existence probe runs through the resolver, so the pure PREVIEW path
    /// ([`NoResolve`], which reports nothing existing) still decodes a plain create — the guard is a
    /// fail-closed COMMIT-time check, not a preview-time network read (preview stays side-effect free).
    fn decode_insert(
        node: &EffectNode,
        row: usize,
        path: &DrivePath,
        res: &dyn WriteResolver,
    ) -> Result<Self, DriveError> {
        // Resolve the destination once (parent folder + leaf name), then probe the leaf under that
        // parent: if a child of this name already exists, refuse (create-only). The probe reuses the
        // resolved parent id, so it is one leaf lookup — no second parent walk. The pure PREVIEW
        // resolver ([`NoResolve::child_id`] → None) skips the probe, so preview stays side-effect free.
        let (parent, name) = Self::upload_destination(node, row, path, res)?;
        if let Some(id) = res.child_id(&parent, &name)? {
            return Err(DriveError::TargetExists {
                path: node.target.path.as_str().to_string(),
                id,
            });
        }
        Self::build_upload(node, row, parent, name)
    }

    fn decode_upload(
        node: &EffectNode,
        row: usize,
        path: &DrivePath,
        res: &dyn WriteResolver,
    ) -> Result<Self, DriveError> {
        let (parent, name) = Self::upload_destination(node, row, path, res)?;
        Self::build_upload(node, row, parent, name)
    }

    /// Build the `Upload` effect from an already-resolved `(parent, name)` — the shared tail of
    /// `INSERT` (create-only) and an `UPSERT` that converged to a create.
    fn build_upload(
        node: &EffectNode,
        row: usize,
        parent: String,
        name: String,
    ) -> Result<Self, DriveError> {
        let mime =
            text_col(node, row, MIME_COL).unwrap_or_else(|| "application/octet-stream".to_string());
        let bytes = upload_bytes(node, row, &mime, "INSERT")?;
        Ok(DriveEffect::Upload {
            parent,
            name,
            mime,
            bytes,
        })
    }

    fn decode_upsert(
        node: &EffectNode,
        row: usize,
        path: &DrivePath,
        res: &dyn WriteResolver,
    ) -> Result<Self, DriveError> {
        // UPSERT keyed by a resolved file id replaces content (retry-safe); without one, an
        // EXISTING node at the path converges to a content replace; else it creates.
        if let Some(id) = text_col(node, row, FILE_ID_COL) {
            return Ok(DriveEffect::Update {
                id,
                mime: text_col(node, row, MIME_COL)
                    .unwrap_or_else(|| "application/octet-stream".to_string()),
                bytes: replace_bytes(node, row)?,
            });
        }
        // Per-row NAMED upsert into a FOLDER — INSERT parity (ticket 20260712150000). A row carrying
        // a `name` targets `<folder>/<name>`: replace its content if that child exists, else create
        // it. INSERT decodes this exact shape create-only (`upload_destination` → the folder is the
        // parent); UPSERT is the replace-or-create twin, so the INSERT-collision error's advice
        // ("use UPSERT to replace its content") actually works. Without this, the folder-target row
        // fell through to the single-blob path below and refused "bytes cannot replace a folder".
        if text_col(node, row, NAME_COL).is_some() {
            let (parent, name) = Self::upload_destination(node, row, path, res)?;
            if let Some(id) = res.child_id(&parent, &name)? {
                return Ok(DriveEffect::Update {
                    id,
                    mime: text_col(node, row, MIME_COL)
                        .unwrap_or_else(|| "application/octet-stream".to_string()),
                    bytes: replace_bytes(node, row)?,
                });
            }
            return Self::build_upload(node, row, parent, name);
        }
        if let Some(meta) = res.existing(path, node.target.path.as_str())? {
            if meta.is_folder() {
                return Err(DriveError::MalformedEffect {
                    verb: "UPSERT",
                    path: node.target.path.as_str().to_string(),
                    reason: "the path names a folder — bytes cannot replace a folder".to_string(),
                });
            }
            return Ok(DriveEffect::Update {
                id: meta.id,
                mime: text_col(node, row, MIME_COL).unwrap_or(meta.mime_type),
                bytes: replace_bytes(node, row)?,
            });
        }
        Self::decode_upload(node, row, path, res)
    }

    fn decode_move(
        node: &EffectNode,
        row: usize,
        path: &DrivePath,
        res: &dyn WriteResolver,
    ) -> Result<Self, DriveError> {
        let new_name = text_col(node, row, NAME_COL);
        let add_parents = list_col(node, row, ADD_PARENTS_COL);
        let remove_parents = list_col(node, row, REMOVE_PARENTS_COL);
        if new_name.is_none() && add_parents.is_empty() && remove_parents.is_empty() {
            return Err(DriveError::MalformedEffect {
                verb: "UPDATE",
                path: node.target.path.as_str().to_string(),
                reason: format!(
                    "UPDATE changes nothing (set `{NAME_COL}`/`{ADD_PARENTS_COL}`/`{REMOVE_PARENTS_COL}`)"
                ),
            });
        }
        let id = match text_col(node, row, FILE_ID_COL) {
            // A snapshotted `file_id` names the exact node — the caller (or a preceding read)
            // chose it deliberately, so trust it.
            Some(id) => id,
            // An id-addressed path (`/drive/id:<id>`) is likewise unambiguous: this is the
            // sanctioned way to rename a FOLDER itself.
            None if matches!(path, DrivePath::ById { .. }) => {
                resolve_existing_id(node, path, res, "UPDATE")?
            }
            // A NAME path: resolve it AND guard the wrong-node write. A row-filtered
            // `UPDATE … SET name … WHERE name == …` collapses to a bare `SET name` on the FOLDER
            // path — the WHERE key is dropped when it shares the SET column (see
            // `setwhere_row_batch`), so what reaches the driver is indistinguishable from renaming
            // the container. Renaming a folder reached by NAME path is therefore refused loudly
            // (round-5 defect: it silently renamed the folder itself). File renames by path, and
            // folder moves (add/remove parents), are unaffected.
            None => {
                let meta = res
                    .existing(path, node.target.path.as_str())?
                    .ok_or_else(|| DriveError::MalformedEffect {
                        verb: "UPDATE",
                        path: node.target.path.as_str().to_string(),
                        reason: format!(
                            "needs the resolved `{FILE_ID_COL}` (nothing found at this path)"
                        ),
                    })?;
                if meta.is_folder() && new_name.is_some() {
                    // A row-filtered folder UPDATE with a single `name` selector renames the
                    // MATCHING CHILD (blueprint §7, ticket 20260713195008): the `WHERE` now survives
                    // to the applier via `node.selector` distinct from the `SET name` payload, so a
                    // same-column `SET name='X' WHERE name='Y'` is representable. Resolve that child
                    // under the folder ambiguity-safe (via `existing`/`resolve_node`, which refuses
                    // `AmbiguousTarget` on ≥2 same-named children — never a first-hit probe) and
                    // rename it. Without such a single-`name` selector, the name-path folder UPDATE
                    // stays the safe loud refusal (it would otherwise rename the container).
                    match selector_single_name(node) {
                        Some(child_name) => {
                            let child = path.child(&child_name).ok_or_else(|| {
                                DriveError::CapabilityDenied {
                                    path: node.target.path.as_str().to_string(),
                                    verb: "UPDATE",
                                }
                            })?;
                            res.existing(&child, node.target.path.as_str())?
                                .ok_or_else(|| DriveError::NotFound {
                                    path: node.target.path.as_str().to_string(),
                                    segment: child_name.clone(),
                                    reason: "no child of this name under the folder to rename",
                                })?
                                .id
                        }
                        None => {
                            return Err(DriveError::MalformedEffect {
                                verb: "UPDATE",
                                path: node.target.path.as_str().to_string(),
                                reason:
                                    "the path names a folder; a bare name-path UPDATE would rename \
                                     the folder itself. To rename a CHILD, filter it: `UPDATE \
                                     /drive/<folder> SET name='X' WHERE name='Y'` (a single `name` \
                                     selector). To rename the folder itself, address it by id \
                                     (/drive/id:<id>). A filter with keys other than a single \
                                     `name` is not resolvable to one child and is refused."
                                        .to_string(),
                            });
                        }
                    }
                } else {
                    meta.id
                }
            }
        };
        Ok(DriveEffect::Move {
            id,
            new_name,
            add_parents,
            remove_parents,
        })
    }

    fn decode_remove(
        node: &EffectNode,
        row: usize,
        path: &DrivePath,
        res: &dyn WriteResolver,
    ) -> Result<Self, DriveError> {
        let id = match path {
            DrivePath::ById { id, .. } => id.clone(),
            _ => match text_col(node, row, FILE_ID_COL) {
                Some(id) => id,
                None => Self::remove_target_id(node, path, res)?,
            },
        };
        // `hard_delete = true` selects the irreversible permanent delete; default is trash.
        if bool_col(node, row, HARD_DELETE_COL) {
            Ok(DriveEffect::Delete { id })
        } else {
            Ok(DriveEffect::Trash { id })
        }
    }

    /// Resolve WHICH node a path-addressed `REMOVE` trashes, honestly. The filter is read from the
    /// WHERE-**selector** (blueprint §7) — a REMOVE writes nothing, so it carries no `args` row to
    /// index into, which is why this takes no row index:
    /// - **no filter columns at all** — the path itself names the node (`remove
    ///   /drive/my/old.txt`, or a folder path — trashing a folder trashes its subtree).
    /// - **exactly one `name` filter key** — the child of that name under the path (`remove
    ///   /drive/my where name == 'old.txt'`, the cookbook's trash-by-name recipe).
    /// - **anything else** — fail closed. A richer filter cannot be resolved to ids here, and
    ///   trashing the folder while silently ignoring the filter would over-delete.
    fn remove_target_id(
        node: &EffectNode,
        path: &DrivePath,
        res: &dyn WriteResolver,
    ) -> Result<String, DriveError> {
        let raw = node.target.path.as_str();
        // The `WHERE` rides the SELECTOR channel (§7) — a REMOVE writes nothing, so its `args` is
        // empty and the selector is the only place a filter lives. `decode_move` already reads it.
        let filter_cols: Vec<&str> = node
            .selector
            .as_ref()
            .map(|s| s.schema.columns.iter().map(|c| c.name.as_str()).collect())
            .unwrap_or_default();
        let target = match filter_cols.as_slice() {
            [] => res.existing(path, raw)?,
            [only] if *only == NAME_COL => {
                let name = node.selector_text(NAME_COL).unwrap_or_default();
                let child = path
                    .child(&name)
                    .ok_or_else(|| DriveError::CapabilityDenied {
                        path: raw.to_string(),
                        verb: "REMOVE",
                    })?;
                res.existing(&child, raw)?
            }
            _ => {
                // A filter richer than one name key: refusing beats trashing the wrong node.
                return Err(DriveError::CapabilityDenied {
                    path: raw.to_string(),
                    verb: "REMOVE",
                });
            }
        };
        target.map(|meta| meta.id).ok_or(DriveError::NotFound {
            path: raw.to_string(),
            segment: String::new(),
            reason: "nothing to remove at this address",
        })
    }

    fn decode_call(
        proc: &str,
        node: &EffectNode,
        row: usize,
        path: &DrivePath,
        res: &dyn WriteResolver,
    ) -> Result<Self, DriveError> {
        if proc != "drive.copy" {
            return Err(DriveError::UnknownProcedure(proc.to_string()));
        }
        let id = match text_col(node, row, FILE_ID_COL) {
            Some(id) => id,
            None => resolve_existing_id(node, path, res, "CALL")?,
        };
        let parent = Self::copy_destination(node, row, res)?;
        let name = text_col(node, row, NAME_COL).ok_or_else(|| DriveError::MalformedEffect {
            verb: "CALL",
            path: node.target.path.as_str().to_string(),
            reason: format!("drive.copy needs the copy `{NAME_COL}`"),
        })?;
        Ok(DriveEffect::Copy { id, parent, name })
    }

    /// The copy destination folder id. A snapshotted `parent_id` (a raw folder id, e.g. from a
    /// scan) wins; otherwise a `parent_path` folder PATH is resolved live to its id — the
    /// `cp`-parity form, mirroring the upload destination walk ([`Self::upload_destination`]),
    /// so a recipe names a `/drive` folder path instead of an opaque id. Fails closed when
    /// neither is supplied.
    fn copy_destination(
        node: &EffectNode,
        row: usize,
        res: &dyn WriteResolver,
    ) -> Result<String, DriveError> {
        if let Some(parent) = text_col(node, row, PARENT_ID_COL) {
            return Ok(parent);
        }
        if let Some(dest) = text_col(node, row, PARENT_PATH_COL) {
            let dest_path = DrivePath::parse_str(&dest)?;
            let (parent, _) = res.folder_id(&dest_path, &dest)?;
            return Ok(parent);
        }
        Err(DriveError::MalformedEffect {
            verb: "CALL",
            path: node.target.path.as_str().to_string(),
            reason: format!(
                "drive.copy needs the destination folder — `{PARENT_PATH_COL}` (a /drive folder \
                 path) or `{PARENT_ID_COL}` (a resolved id)"
            ),
        })
    }

    /// Whether this effect is irreversible (blueprint §8): both the trash and the hard delete.
    #[must_use]
    pub const fn is_irreversible(&self) -> bool {
        matches!(self, DriveEffect::Trash { .. } | DriveEffect::Delete { .. })
    }

    /// The stable verb label (for the audit ledger / capability-denied errors).
    #[must_use]
    pub const fn verb_label(&self) -> &'static str {
        match self {
            DriveEffect::Upload { .. } => "INSERT",
            DriveEffect::Update { .. } => "UPSERT",
            DriveEffect::Move { .. } => "UPDATE",
            DriveEffect::Copy { .. } => "CALL",
            DriveEffect::Trash { .. } | DriveEffect::Delete { .. } => "REMOVE",
        }
    }
}

/// Resolve the file id a path-addressed effect targets through the live resolver, failing
/// closed with a verb-tagged malformed-effect error when the path names nothing (or no
/// resolver is available — the pure decode).
fn resolve_existing_id(
    node: &EffectNode,
    path: &DrivePath,
    res: &dyn WriteResolver,
    verb: &'static str,
) -> Result<String, DriveError> {
    match res.existing(path, node.target.path.as_str())? {
        Some(meta) => Ok(meta.id),
        None => Err(DriveError::MalformedEffect {
            verb,
            path: node.target.path.as_str().to_string(),
            reason: format!("needs the resolved `{FILE_ID_COL}` (nothing found at this path)"),
        }),
    }
}

/// The upload file name: the explicit `name` column, else the path's terminal segment.
fn upload_name(node: &EffectNode, row: usize, path: &DrivePath) -> Result<String, DriveError> {
    if let Some(name) = text_col(node, row, NAME_COL) {
        return Ok(name);
    }
    path.leaf_name()
        .map(str::to_string)
        .ok_or_else(|| DriveError::MalformedEffect {
            verb: "INSERT",
            path: node.target.path.as_str().to_string(),
            reason: format!("upload needs a `{NAME_COL}` or a named path segment"),
        })
}

/// Read a `Text` value from the node's row `row` by column name.
fn text_col(node: &EffectNode, row: usize, name: &str) -> Option<String> {
    let idx = node
        .args
        .schema
        .columns
        .iter()
        .position(|c| c.name == name)?;
    match node.args.rows.get(row).and_then(|r| r.values.get(idx)) {
        Some(Value::Text(t)) if !t.is_empty() => Some(t.clone()),
        _ => None,
    }
}

/// Read a `Text` value from the node's `WHERE`-selector by column name (blueprint §7). The selector
/// is a single-row equality batch, so it always reads row 0. `None` when there is no selector, the
/// column is absent, or the value is not non-empty text.
fn selector_text(node: &EffectNode, name: &str) -> Option<String> {
    let sel = node.selector.as_ref()?;
    let idx = sel.schema.columns.iter().position(|c| c.name == name)?;
    match sel.rows.first().and_then(|r| r.values.get(idx)) {
        Some(Value::Text(t)) if !t.is_empty() => Some(t.clone()),
        _ => None,
    }
}

/// When the `WHERE`-selector is **exactly** one `name = '<value>'` key, return that value — the
/// single matching child a folder-path rename resolves against. A richer selector (any extra key)
/// returns `None`, so the caller falls back to the safe refusal rather than guessing which child.
fn selector_single_name(node: &EffectNode) -> Option<String> {
    let sel = node.selector.as_ref()?;
    match sel.schema.columns.as_slice() {
        [only] if only.name == NAME_COL => selector_text(node, NAME_COL),
        _ => None,
    }
}

/// Read a `Bool` value from the node's row `row` by column name (default `false`).
fn bool_col(node: &EffectNode, row: usize, name: &str) -> bool {
    let Some(idx) = node.args.schema.columns.iter().position(|c| c.name == name) else {
        return false;
    };
    matches!(
        node.args.rows.get(row).and_then(|r| r.values.get(idx)),
        Some(Value::Bool(true))
    )
}

/// Locate the upload payload in row `row`: the drive-native `bytes` column, else the engine's
/// well-known blob column `content` (the planner lowers a blob write's payload there — exactly how
/// a cross-driver `cp /local/x /drive/y` materializes the source file's bytes, and how the `/local`
/// / `/fs` blob writes carry theirs), else — when the row carries exactly ONE column — that single
/// positional value (the `upsert into /drive/.../file values ('…')` shape).
///
/// Returns `Some(bytes)` when a payload CHANNEL is present — its bytes, possibly **empty** for a
/// genuinely empty source file (an explicit empty `content`/`bytes` value is a valid zero-byte
/// upload). Returns `None` when NO payload channel exists at all — the effect never carried the
/// source bytes. A payload the decoder cannot see must never silently become an empty upload; the
/// callers ([`upload_bytes`] / [`replace_bytes`]) turn a `None` into a fail-closed error
/// (ticket 20260707181404) rather than truncating the Drive object to zero bytes.
fn payload_bytes(node: &EffectNode, row: usize) -> Option<Vec<u8>> {
    for name in [BYTES_COL, "content"] {
        if let Some(idx) = node.args.schema.columns.iter().position(|c| c.name == name) {
            if let Some(bytes) =
                value_as_bytes(node.args.rows.get(row).and_then(|r| r.values.get(idx)))
            {
                return Some(bytes);
            }
        }
    }
    // The single-positional-column fallback: an unambiguous one-value row IS the payload.
    if node.args.schema.columns.len() == 1 {
        if let Some(bytes) = value_as_bytes(node.args.rows.get(row).and_then(|r| r.values.first()))
        {
            return Some(bytes);
        }
    }
    None
}

/// The bytes for a **file upload** (`INSERT`, or an `UPSERT` that creates), failing closed when the
/// effect carries no payload channel (blueprint §7 fail-closed; ticket 20260707181404): a byteless
/// file upload is a silent-truncation data-integrity bug, so refuse rather than create an empty
/// object. The one legitimate byteless upload is the **metadata-only folder create**
/// (`mime == FOLDER_MIME`, the gmail-ftp `mkdir`), which is allowed through.
fn upload_bytes(
    node: &EffectNode,
    row: usize,
    mime: &str,
    verb: &'static str,
) -> Result<Vec<u8>, DriveError> {
    match payload_bytes(node, row) {
        Some(bytes) => Ok(bytes),
        None if mime == FOLDER_MIME => Ok(Vec::new()),
        None => Err(missing_payload(node, verb)),
    }
}

/// The bytes for a **content replace** (`UPSERT` onto an existing file / a resolved `file_id`),
/// failing closed when the effect carries no payload channel (ticket 20260707181404). Unlike an
/// upload there is no folder exception — replacing a file's content with "nothing found" would
/// silently truncate it to empty, which is exactly the bug this guard prevents.
fn replace_bytes(node: &EffectNode, row: usize) -> Result<Vec<u8>, DriveError> {
    payload_bytes(node, row).ok_or_else(|| missing_payload(node, "UPSERT"))
}

/// The fail-closed error for a write whose source bytes never reached the effect row.
fn missing_payload(node: &EffectNode, verb: &'static str) -> DriveError {
    DriveError::MalformedEffect {
        verb,
        path: node.target.path.as_str().to_string(),
        reason:
            "the write carries no content bytes — the copy source produced no `content`/`bytes` \
                 payload; refusing to create or replace the Drive file with an empty body"
                .to_string(),
    }
}

/// A `Bytes` value verbatim, or `Text` as UTF-8 bytes; `None` for anything else.
fn value_as_bytes(value: Option<&Value>) -> Option<Vec<u8>> {
    match value {
        Some(Value::Bytes(b)) => Some(b.clone()),
        Some(Value::Text(t)) => Some(t.clone().into_bytes()),
        _ => None,
    }
}

/// Read a comma-separated `Text` column into a list of trimmed, non-empty items.
fn list_col(node: &EffectNode, row: usize, name: &str) -> Vec<String> {
    text_col(node, row, name)
        .map(|s| {
            s.split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}
