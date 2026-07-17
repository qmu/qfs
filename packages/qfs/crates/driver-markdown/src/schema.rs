//! The `/markdown/*` node model: the [`MarkdownNode`] sum type, its path↔node mapping, and the
//! single-source-of-truth [`markdown_node_schema`].
//!
//! This is the **pure, credential-free** introspective surface (blueprint §3 purity / §6),
//! mirroring the `/claude` and `/sys` drivers' node-model pattern: `DESCRIBE
//! /markdown/<name>/documents` returns a stable typed [`Schema`] with **no filesystem root
//! bound**, so describe (and the parse-time capability gate) read one source of truth that can
//! never drift from the rows the binary's tree walk later scans. NOTHING here reads a file.
//!
//! ## The path shape (design brief, mission acceptance item 1)
//! One declared root `<name>` (a `CONNECT /markdown/<name> TO markdown AT '<root>'` binding)
//! resolves exactly two relational tables — `/markdown/<name>/documents` and
//! `/markdown/<name>/links`, keeping the strategy vocabulary one-to-one. The mount itself and
//! the bare `/markdown/<name>` are not nodes.
//!
//! ## No typing, by construction (mission acceptance item 4)
//! The `links` schema carries **no relation-type column**: the closed relation vocabulary
//! (`parent`/`concerns`/`references`/… — "declare and reject, never guess") is a later,
//! separate mission layered on the preserved `source_section_path`. This slice records the
//! section context losslessly and infers nothing from it.

use qfs_types::{Column, ColumnType, Schema};

/// The reserved mount point for the markdown collection-path driver.
pub const MARKDOWN_MOUNT: &str = "/markdown";

/// One addressable `/markdown/<name>/<table>` relation. A **closed set**; a new view adds a
/// variant here, never a side-channel API (the one-engine constraint).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkdownNode {
    /// `/markdown/<name>/documents` — one row per `.md` file under the declared root:
    /// the root-relative `path` (the canonical id `links.target_doc` joins on), the document
    /// `title` (frontmatter `title`, else the first ATX heading, else NULL), and the whole
    /// parsed YAML `frontmatter` as one Json column (NULL when the file has none).
    Documents,
    /// `/markdown/<name>/links` — one row per inline markdown link: `source_doc` (the linking
    /// document's `documents.path`), `source_section_path` (the FULL nested heading path of the
    /// section containing the link, top-level first, as a lossless Array(Text); empty for a
    /// pre-heading link — the column the later vocabulary mission types), `target` (as
    /// written), `target_doc` (the normalized root-relative form, joinable against
    /// `documents.path`; NULL for external or root-escaping targets), and the 1-based `line`.
    Links,
}

impl MarkdownNode {
    /// The path segment naming this table (`documents`, `links`).
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Documents => "documents",
            Self::Links => "links",
        }
    }
}

/// Resolve a `/markdown/...` path to its [`MarkdownNode`], if it names a known table.
/// Recognised shapes:
/// - `/markdown/<name>/documents` → [`MarkdownNode::Documents`];
/// - `/markdown/<name>/links` → [`MarkdownNode::Links`].
///
/// Returns `None` for `/markdown` itself, a bare `/markdown/<name>` (the tree is read through
/// its two tables, never addressed as one node), or any other shape.
#[must_use]
pub fn node_for_path(path: &str) -> Option<MarkdownNode> {
    let (_name, node) = tree_and_node_for_path(path)?;
    Some(node)
}

/// Resolve a `/markdown/<name>/<table>` path to `(name, node)` — the declared-tree name the
/// binary's read facet looks up a root by, plus the table. Pure string work; no I/O.
#[must_use]
pub fn tree_and_node_for_path(path: &str) -> Option<(String, MarkdownNode)> {
    let rest = path
        .strip_prefix("/markdown/")
        .or_else(|| path.strip_prefix("markdown/"))?;
    let segments: Vec<&str> = rest.split('/').filter(|s| !s.is_empty()).collect();
    match segments.as_slice() {
        [name, "documents"] => Some(((*name).to_string(), MarkdownNode::Documents)),
        [name, "links"] => Some(((*name).to_string(), MarkdownNode::Links)),
        _ => None,
    }
}

/// The typed [`Schema`] of a `/markdown/<name>/<table>` relation — the **canonical** source of
/// truth `DESCRIBE` and the binary's tree scan both read. Pure data; no root, no creds.
#[must_use]
pub fn markdown_node_schema(node: MarkdownNode) -> Schema {
    let col = |name: &str, ty: ColumnType, nullable: bool| Column::new(name, ty, nullable);
    match node {
        // One row per .md file: listing + detail-header columns (mission acceptance item 2).
        MarkdownNode::Documents => Schema::new(vec![
            col("path", ColumnType::Text, false),
            col("title", ColumnType::Text, true),
            col("frontmatter", ColumnType::Json, true),
        ]),
        // One row per inline markdown link, with the section context preserved (item 3). NO
        // relation-type column, by construction (item 4).
        MarkdownNode::Links => Schema::new(vec![
            col("source_doc", ColumnType::Text, false),
            col(
                "source_section_path",
                ColumnType::Array(Box::new(ColumnType::Text)),
                false,
            ),
            col("target", ColumnType::Text, false),
            col("target_doc", ColumnType::Text, true),
            col("line", ColumnType::Int, false),
        ]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_for_path_resolves_known_shapes() {
        assert_eq!(
            node_for_path("/markdown/docs/documents"),
            Some(MarkdownNode::Documents)
        );
        assert_eq!(
            node_for_path("/markdown/docs/links"),
            Some(MarkdownNode::Links)
        );
        assert_eq!(
            node_for_path("/markdown/docs/documents/"),
            Some(MarkdownNode::Documents)
        );
        assert_eq!(
            tree_and_node_for_path("/markdown/knowledge/links"),
            Some(("knowledge".to_string(), MarkdownNode::Links))
        );
        // The mount, a bare tree, and unknown shapes are not nodes.
        assert_eq!(node_for_path("/markdown"), None);
        assert_eq!(node_for_path("/markdown/docs"), None);
        assert_eq!(node_for_path("/markdown/docs/nope"), None);
        assert_eq!(node_for_path("/markdown/docs/documents/deeper"), None);
    }

    /// The no-typing rule is structural (mission acceptance item 4): the links schema has no
    /// column a relation type could ride in, and no secret-shaped column exists anywhere.
    #[test]
    fn links_schema_carries_no_relation_type_and_no_secret_column() {
        let links = markdown_node_schema(MarkdownNode::Links);
        for forbidden in ["relation", "relation_type", "type", "kind", "verb"] {
            assert!(
                links.column(forbidden).is_none(),
                "links must never expose `{forbidden}` in the untyped slice"
            );
        }
        for node in [MarkdownNode::Documents, MarkdownNode::Links] {
            let schema = markdown_node_schema(node);
            for forbidden in ["token", "api_key", "secret", "password", "bearer"] {
                assert!(
                    schema.column(forbidden).is_none(),
                    "/markdown {} must never expose `{forbidden}`",
                    node.label()
                );
            }
        }
    }

    /// The section-path column is the lossless Array(Text) the design brief rules — never a
    /// flat delimited string, never only the nearest heading.
    #[test]
    fn source_section_path_is_an_array_of_text() {
        let links = markdown_node_schema(MarkdownNode::Links);
        let col = links
            .column("source_section_path")
            .expect("source_section_path present");
        assert_eq!(col.ty, ColumnType::Array(Box::new(ColumnType::Text)));
        assert!(!col.nullable, "empty path is [], never NULL");
    }
}
