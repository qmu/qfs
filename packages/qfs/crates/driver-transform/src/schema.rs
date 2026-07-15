//! The `/transform` node model: the mount, the path↔node resolution, and the single-source-of-truth
//! [`transform_node_schema`] the describe surface and the backend scan both read.
//!
//! This is the **pure, credential-free** introspective surface (blueprint §3 purity / §6). It
//! mirrors `qfs-driver-sys`'s `sys_node_schema`: `DESCRIBE /transform` returns a stable typed
//! [`Schema`] with **no DB and no secrets**. The `secret_ref` column is a REFERENCE
//! (`env:`/`vault:`), never a value — there is structurally no column an inline secret could ride
//! in. The `mode` column is the DERIVED cardinality (row-wise/relation-wise/extraction), computed
//! by the backend from the definition's INPUT; the pure schema only declares that the column exists.

use qfs_types::{Column, ColumnType, Schema};

/// The reserved mount point for the transform-definition registry (a top-level driver, alongside
/// `/local`, `/sys`, `/git`).
pub const TRANSFORM_MOUNT: &str = "/transform";

/// The `/transform` relation node — the definition registry. A single node: the collection
/// `/transform` (list) and the item `/transform/<name>` (one definition, the `REMOVE` target) both
/// resolve here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransformNode {
    /// `/transform` — the definition registry relation.
    Registry,
}

/// Resolve a `/transform...` path to its [`TransformNode`]. `/transform`, `/transform/<name>`, and
/// `/transform/<name>/…` all resolve to the one registry node. Returns `None` for a foreign prefix.
#[must_use]
pub fn node_for_path(path: &str) -> Option<TransformNode> {
    if path == TRANSFORM_MOUNT
        || path == "transform"
        || path.starts_with("/transform/")
        || path.starts_with("transform/")
    {
        return Some(TransformNode::Registry);
    }
    None
}

/// Reconstruct the definition `<name>` from a `/transform/<name>` path (the `REMOVE` item form).
/// Returns `None` for the bare collection `/transform` (no name named). The name is the single
/// segment after `transform` (a definition name has no `/`).
#[must_use]
pub fn name_from_path(path: &str) -> Option<String> {
    let rest = path
        .strip_prefix("/transform/")
        .or_else(|| path.strip_prefix("transform/"))?;
    let name = rest.split('/').next().unwrap_or(rest).trim();
    (!name.is_empty()).then(|| name.to_string())
}

/// The typed [`Schema`] of the `/transform` relation — the canonical source of truth
/// `DESCRIBE /transform` and the backend scan both read. Pure data; no live backend, no creds.
///
/// Definition text + selectors + a DERIVED `mode` + a secret REFERENCE only. There is structurally
/// no column an inline secret value could ride in: `secret_ref` names WHERE the secret lives
/// (`env:`/`vault:`), never the secret.
#[must_use]
pub fn transform_node_schema(node: TransformNode) -> Schema {
    let col = |name: &str, ty: ColumnType, nullable: bool| Column::new(name, ty, nullable);
    match node {
        TransformNode::Registry => Schema::new(vec![
            col("name", ColumnType::Text, false),
            // The declared INPUT/OUTPUT schemas as column-descriptor JSON (the definition shape).
            col("input", ColumnType::Text, false),
            col("output", ColumnType::Text, false),
            col("provider", ColumnType::Text, false),
            col("model", ColumnType::Text, false),
            col("effort", ColumnType::Text, true),
            // The DERIVED cardinality mode (row-wise/relation-wise/extraction) — never a stored
            // flag; the backend computes it from `input` on every read.
            col("mode", ColumnType::Text, false),
            // A secret REFERENCE (`env:`/`vault:`), NEVER a value; NULL when the provider needs none.
            col("secret_ref", ColumnType::Text, true),
            col("created_at", ColumnType::Text, true),
        ]),
    }
}
