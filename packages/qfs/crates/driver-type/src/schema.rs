//! The `/type` node model: the mount, the path↔node resolution, and the single-source-of-truth
//! [`type_node_schema`] the describe surface and the binary's read facet both read.
//!
//! This is the **pure, credential-free** introspective surface (blueprint §3 purity / §6). It
//! mirrors `qfs-driver-transform`'s `transform_node_schema`: `DESCRIBE /type` returns a stable typed
//! [`Schema`] with **no DB and no secrets**. A declared type is declarative DATA — a column list plus
//! an optional row-local refinement predicate, both stored as text — so there is structurally no
//! column a credential could ride in.

use qfs_types::{Column, ColumnType, Schema};

/// The reserved mount point for the declared-type catalog (a top-level driver, alongside `/local`,
/// `/sys`, `/transform`).
pub const TYPE_MOUNT: &str = "/type";

/// The `/type` relation node — the declared-type catalog. A single node: the collection `/type`
/// (list = SHOW TYPES) and the item `/type/<name>` (one declared type) both resolve here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeNode {
    /// `/type` — the declared-type catalog relation.
    Catalog,
}

/// Resolve a `/type...` path to its [`TypeNode`]. `/type`, `/type/<name>`, and a nested
/// `/type/<catalog>/<name>` (a qualified name — §5.4 names may be multi-segment) all resolve to the
/// one catalog node. Returns `None` for a foreign prefix.
#[must_use]
pub fn node_for_path(path: &str) -> Option<TypeNode> {
    if path == TYPE_MOUNT
        || path == "type"
        || path.starts_with("/type/")
        || path.starts_with("type/")
    {
        return Some(TypeNode::Catalog);
    }
    None
}

/// Reconstruct the declared type `<name>` from a `/type/<name>` path (the item form). Returns `None`
/// for the bare collection `/type` (no name named). Unlike `/transform`, the name is the WHOLE
/// remainder, not the first segment: a declared type name may be multi-segment where a catalog nests
/// (`chatwork/message` — blueprint §5.4), and the `/type` mount only prefixes it.
#[must_use]
pub fn name_from_path(path: &str) -> Option<String> {
    let rest = path
        .strip_prefix("/type/")
        .or_else(|| path.strip_prefix("type/"))?
        .trim()
        .trim_end_matches('/');
    (!rest.is_empty()).then(|| rest.to_string())
}

/// The typed [`Schema`] of the `/type` relation — the canonical source of truth `DESCRIBE /type` and
/// the binary's read facet both read. Pure data; no live backend, no creds.
///
/// A declared type is its NAME plus its shape: the declared column descriptors and the optional
/// row-local refinement predicate (§5.4). Both are stored as declarative text (a stored, normalised
/// AST rendered as JSON), never opaque executable code.
#[must_use]
pub fn type_node_schema(node: TypeNode) -> Schema {
    let col = |name: &str, ty: ColumnType, nullable: bool| Column::new(name, ty, nullable);
    match node {
        TypeNode::Catalog => Schema::new(vec![
            // The declared type NAME — the reference face (`of customer`), possibly qualified
            // (`chatwork/message`). This is what `ls /type` enumerates.
            col("name", ColumnType::Text, false),
            // The declared column descriptors as JSON (`[{"name":…,"type":…,…}, …]`) — the shape.
            col("columns", ColumnType::Text, false),
            // The optional row-local refinement predicate (§5.4's `WHERE <pred>`) as its stored,
            // span-normalised AST JSON. NULL for a purely structural type.
            col("refinement", ColumnType::Text, true),
            col("created_at", ColumnType::Text, true),
        ]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_for_path_resolves_the_mount_and_its_items_only() {
        assert_eq!(node_for_path("/type"), Some(TypeNode::Catalog));
        assert_eq!(node_for_path("/type/customer"), Some(TypeNode::Catalog));
        // A qualified (nested-catalog) name still resolves to the one catalog node.
        assert_eq!(
            node_for_path("/type/chatwork/message"),
            Some(TypeNode::Catalog)
        );
        assert_eq!(node_for_path("/transform"), None);
        // `/types` is a foreign mount — the prefix match must not bleed past the segment.
        assert_eq!(node_for_path("/types/customer"), None);
    }

    #[test]
    fn name_from_path_keeps_a_qualified_name_whole() {
        assert_eq!(
            name_from_path("/type/customer").as_deref(),
            Some("customer")
        );
        // §5.4: a nested catalog qualifies the name — the WHOLE remainder is the name, so this must
        // not truncate to `chatwork` the way `/transform`'s first-segment rule would.
        assert_eq!(
            name_from_path("/type/chatwork/message").as_deref(),
            Some("chatwork/message")
        );
        // The bare collection names no type.
        assert_eq!(name_from_path("/type"), None);
        assert_eq!(name_from_path("/type/"), None);
    }
}
