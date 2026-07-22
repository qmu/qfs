//! The markdown **tree interpretation** — the `documents` and `links` named relations of the
//! `md` codec (blueprint §13b; mission
//! `a-file-collection-is-a-declared-set-over-any-blob-source`). This is the pure parser rehomed
//! from `qfs-driver-markdown` into the codec layer as the **second named relation of the same
//! format**: `decode md.documents` and `decode md.links` address these, while bare `decode md`
//! keeps yielding the flat front-matter+body relation.
//!
//! **Pure string work — no I/O.** [`parse_document`] takes a document's root-relative path (its
//! provenance/join id — needed to normalize `target_doc` against the source's directory) and its
//! text, and returns the `documents` record + the `links` records. The row-equivalence test
//! (`crates/codec/tests/codecs.rs`) pins this against the compiled `qfs-driver-markdown` parser
//! on shared fixtures, so the rehomed interpretation reproduces the driver's rows exactly.
//!
//! ## What is recognised (documented slice-1 scope)
//! - **Frontmatter**: an opening `---` at the very start, closed by the next `---`; parsed as
//!   YAML into one `serde_json::Value`; an unparseable block degrades to no frontmatter.
//! - **Headings**: ATX only (`#`–`######` + whitespace), outside fenced code blocks; trailing
//!   closing hashes trimmed; setext headings not recognised.
//! - **Links**: inline `[text](target)`, outside fenced code blocks, nested brackets/parens
//!   counted; images excluded; autolinks and reference-style links not recognised.
//! - **`section_path`**: the full nested ATX-heading stack at the link's line, top-level first
//!   (never collapsed to the nearest heading); empty before any heading.
//! - **`target_doc`**: the joinable root-relative form of an in-tree target; `None` for a URL
//!   scheme, protocol-relative `//…`, or a root-escaping target; a pure `#fragment` normalizes
//!   to the source document itself.

use qfs_types::{Column, ColumnType, Row, Schema, Value};

/// One addressable named relation of the `md` codec's tree interpretation (blueprint §13b). A
/// **closed set**; a new relation adds a variant here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkdownRelation {
    /// `documents` — one row per file: `path` (root-relative join id), `title`
    /// (frontmatter `title`, else the first ATX heading, else NULL), `frontmatter` (the whole
    /// parsed YAML as one Json column, NULL when absent).
    Documents,
    /// `links` — one row per inline link: `source_doc`, `section_path` (the full nested heading
    /// path, a lossless `Array(Text)`, `[]` before any heading), `target`, `target_doc`
    /// (normalized root-relative, NULL for external/escaping), `line`.
    Links,
}

impl MarkdownRelation {
    /// The relation name the codec declares and a `decode md.<name>` addresses.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Documents => "documents",
            Self::Links => "links",
        }
    }

    /// Resolve a relation name (`documents`/`links`) to its variant.
    #[must_use]
    pub fn from_label(label: &str) -> Option<Self> {
        match label {
            "documents" => Some(Self::Documents),
            "links" => Some(Self::Links),
            _ => None,
        }
    }
}

/// The typed [`Schema`] of a markdown named relation — the canonical source of truth `DESCRIBE`
/// and a `decode md.<relation>` both read. Pure data; no root, no creds.
#[must_use]
pub fn relation_schema(relation: MarkdownRelation) -> Schema {
    let col = |name: &str, ty: ColumnType, nullable: bool| Column::new(name, ty, nullable);
    match relation {
        MarkdownRelation::Documents => Schema::new(vec![
            col("path", ColumnType::Text, false),
            col("title", ColumnType::Text, true),
            col("frontmatter", ColumnType::Json, true),
        ]),
        MarkdownRelation::Links => Schema::new(vec![
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

/// One parsed document: the `documents` row's fields + its `links` rows' fields.
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedDocument {
    /// Root-relative path of the document (the canonical join id), as handed in.
    pub path: String,
    /// Frontmatter `title` (a YAML string), else the first ATX heading, else `None`.
    pub title: Option<String>,
    /// The whole parsed YAML frontmatter map, `None` when absent or unparseable.
    pub frontmatter: Option<serde_json::Value>,
    /// Every recognised inline link, in file order.
    pub links: Vec<ParsedLink>,
}

/// One inline markdown link with its section context (the load-bearing column).
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedLink {
    /// The FULL nested heading path of the section containing the link, top-level first.
    /// Empty for a link written before any heading.
    pub section_path: Vec<String>,
    /// The link target exactly as written between the parentheses (title stripped).
    pub target: String,
    /// The normalized root-relative target joinable against `documents.path`, when in-tree.
    pub target_doc: Option<String>,
    /// 1-based line number in the file (frontmatter lines counted).
    pub line: u64,
}

impl ParsedDocument {
    /// This document's `documents` row, in [`relation_schema`] column order.
    #[must_use]
    pub fn document_row(&self) -> Row {
        Row::new(vec![
            Value::Text(self.path.clone()),
            self.title.clone().map_or(Value::Null, Value::Text),
            self.frontmatter.clone().map_or(Value::Null, Value::Json),
        ])
    }

    /// This document's `links` rows, in file order, in [`relation_schema`] column order.
    #[must_use]
    pub fn link_rows(&self) -> Vec<Row> {
        self.links
            .iter()
            .map(|l| {
                Row::new(vec![
                    Value::Text(self.path.clone()),
                    Value::Array(
                        l.section_path
                            .iter()
                            .map(|s| Value::Text(s.clone()))
                            .collect(),
                    ),
                    Value::Text(l.target.clone()),
                    l.target_doc.clone().map_or(Value::Null, Value::Text),
                    #[allow(clippy::cast_possible_wrap)]
                    Value::Int(l.line as i64),
                ])
            })
            .collect()
    }
}

/// Parse one markdown document (pure; see the module docs for the recognised scope). `rel_path`
/// is the document's root-relative path — it becomes `documents.path` / `links.source_doc` and
/// anchors relative-target normalization.
#[must_use]
pub fn parse_document(rel_path: &str, text: &str) -> ParsedDocument {
    let (frontmatter_lines, frontmatter) = parse_frontmatter(text);

    let mut heading_stack: Vec<(u8, String)> = Vec::new();
    let mut first_heading: Option<String> = None;
    let mut links: Vec<ParsedLink> = Vec::new();
    let mut in_fence = false;

    for (idx, line) in text.lines().enumerate() {
        let line_no = (idx + 1) as u64;
        if (idx as u64) < frontmatter_lines {
            continue;
        }
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        if let Some((level, heading_text)) = parse_atx_heading(line) {
            while heading_stack.last().is_some_and(|(l, _)| *l >= level) {
                heading_stack.pop();
            }
            heading_stack.push((level, heading_text.clone()));
            if first_heading.is_none() {
                first_heading = Some(heading_text);
            }
            continue;
        }
        for (target, _span_start) in inline_link_targets(line) {
            let target_doc = normalize_target(rel_path, &target);
            links.push(ParsedLink {
                section_path: heading_stack.iter().map(|(_, t)| t.clone()).collect(),
                target,
                target_doc,
                line: line_no,
            });
        }
    }

    let title = frontmatter
        .as_ref()
        .and_then(|fm| fm.get("title"))
        .and_then(|t| t.as_str())
        .map(str::to_string)
        .or(first_heading);

    ParsedDocument {
        path: rel_path.to_string(),
        title,
        frontmatter,
        links,
    }
}

/// The `documents` relation schema.
#[must_use]
pub fn documents_schema() -> Schema {
    relation_schema(MarkdownRelation::Documents)
}

/// The `links` relation schema.
#[must_use]
pub fn links_schema() -> Schema {
    relation_schema(MarkdownRelation::Links)
}

/// Split + parse the YAML frontmatter block. Returns `(lines_consumed, value)`.
fn parse_frontmatter(text: &str) -> (u64, Option<serde_json::Value>) {
    let mut lines = text.lines();
    match lines.next() {
        Some(first) if first.trim_end() == "---" => {}
        _ => return (0, None),
    }
    let mut consumed: u64 = 1;
    let mut block = String::new();
    for line in lines {
        consumed += 1;
        if line.trim_end() == "---" {
            let parsed = serde_yaml_ng::from_str::<serde_json::Value>(&block)
                .ok()
                .filter(serde_json::Value::is_object);
            return (consumed, parsed);
        }
        block.push_str(line);
        block.push('\n');
    }
    (0, None)
}

/// Parse an ATX heading line.
fn parse_atx_heading(line: &str) -> Option<(u8, String)> {
    let trimmed = line.trim_start();
    let hashes = trimmed.chars().take_while(|c| *c == '#').count();
    if hashes == 0 || hashes > 6 {
        return None;
    }
    let rest = &trimmed[hashes..];
    if !rest.is_empty() && !rest.starts_with(' ') && !rest.starts_with('\t') {
        return None;
    }
    let text = rest.trim().trim_end_matches('#').trim_end().to_string();
    #[allow(clippy::cast_possible_truncation)]
    Some((hashes as u8, text))
}

/// Extract every inline `[text](target)` link target on one line, in order, excluding images and
/// text inside backtick code spans.
fn inline_link_targets(line: &str) -> Vec<(String, usize)> {
    let bytes = line.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    let mut in_code_span = false;
    while i < bytes.len() {
        match bytes[i] {
            b'`' => {
                in_code_span = !in_code_span;
                i += 1;
            }
            b'[' if !in_code_span => {
                let is_image = i > 0 && bytes[i - 1] == b'!';
                if let Some((target, next)) = link_at(bytes, i) {
                    if !is_image {
                        out.push((target, i));
                    }
                    i = next;
                } else {
                    i += 1;
                }
            }
            _ => i += 1,
        }
    }
    out
}

/// Try to read one `[text](target)` starting at the `[` at `open`.
fn link_at(bytes: &[u8], open: usize) -> Option<(String, usize)> {
    let mut depth = 0usize;
    let mut close = None;
    for (off, &b) in bytes[open..].iter().enumerate() {
        match b {
            b'[' => depth += 1,
            b']' => {
                depth -= 1;
                if depth == 0 {
                    close = Some(open + off);
                    break;
                }
            }
            _ => {}
        }
    }
    let close = close?;
    if bytes.get(close + 1) != Some(&b'(') {
        return None;
    }
    let mut depth = 1usize;
    let mut end = None;
    for (off, &b) in bytes[close + 2..].iter().enumerate() {
        match b {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    end = Some(close + 2 + off);
                    break;
                }
            }
            _ => {}
        }
    }
    let end = end?;
    let raw = std::str::from_utf8(&bytes[close + 2..end]).ok()?.trim();
    let unwrapped = raw
        .strip_prefix('<')
        .and_then(|s| s.strip_suffix('>'))
        .unwrap_or(raw);
    let target = unwrapped
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_string();
    if target.is_empty() {
        return None;
    }
    Some((target, end + 1))
}

/// Normalize a link target into the root-relative `target_doc` join key.
fn normalize_target(source_doc: &str, target: &str) -> Option<String> {
    if target.starts_with("//") || has_url_scheme(target) {
        return None;
    }
    let no_frag = target.split_once('#').map_or(target, |(p, _)| p);
    let path_part = no_frag.split_once('?').map_or(no_frag, |(p, _)| p);
    if path_part.is_empty() {
        return Some(source_doc.to_string());
    }
    let mut segments: Vec<&str> = if let Some(abs) = path_part.strip_prefix('/') {
        Vec::from_iter(abs.split('/'))
    } else {
        let mut base: Vec<&str> = source_doc.split('/').collect();
        base.pop();
        base.extend(path_part.split('/'));
        base
    };
    let mut resolved: Vec<&str> = Vec::new();
    for seg in segments.drain(..) {
        match seg {
            "" | "." => {}
            ".." => {
                resolved.pop()?;
            }
            s => resolved.push(s),
        }
    }
    if resolved.is_empty() {
        return None;
    }
    Some(resolved.join("/"))
}

/// Whether the target starts with a URL scheme per RFC 3986.
fn has_url_scheme(target: &str) -> bool {
    let Some(colon) = target.find(':') else {
        return false;
    };
    let scheme = &target[..colon];
    let mut chars = scheme.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
}

/// Decode a document's bytes into the rows of one named relation (`documents` or `links`), given
/// the source's root-relative `rel_path` provenance (needed for `target_doc` normalization).
/// Returns the relation's [`RowBatch`] — one row for `documents`, zero-or-more for `links`.
///
/// # Errors
/// [`crate::CfsError`] if the bytes are not valid UTF-8.
pub fn decode_relation(
    relation: MarkdownRelation,
    bytes: &[u8],
    rel_path: &str,
) -> Result<crate::RowBatch, crate::CfsError> {
    let text = std::str::from_utf8(bytes).map_err(|e| crate::CfsError::Decode {
        fmt: "md",
        detail: format!("invalid utf-8: {e}"),
    })?;
    let doc = parse_document(rel_path, text);
    let (schema, rows) = match relation {
        MarkdownRelation::Documents => (documents_schema(), vec![doc.document_row()]),
        MarkdownRelation::Links => (links_schema(), doc.link_rows()),
    };
    Ok(crate::RowBatch::new(schema, rows))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn targets(doc: &ParsedDocument) -> Vec<&str> {
        doc.links.iter().map(|l| l.target.as_str()).collect()
    }

    #[test]
    fn nested_headings_yield_the_full_section_path() {
        let text = "\
# 全体の振り返り

intro [pre](a.md)

## 懸念

- [the concern](../other/b.md)

## 次の一歩

### 詳細

deep [deep](c.md)
";
        let doc = parse_document("notes/review.md", text);
        assert_eq!(targets(&doc), vec!["a.md", "../other/b.md", "c.md"]);
        assert_eq!(doc.links[0].section_path, vec!["全体の振り返り"]);
        assert_eq!(doc.links[1].section_path, vec!["全体の振り返り", "懸念"]);
        assert_eq!(
            doc.links[2].section_path,
            vec!["全体の振り返り", "次の一歩", "詳細"]
        );
    }

    #[test]
    fn target_normalization() {
        let n = |src: &str, t: &str| normalize_target(src, t);
        assert_eq!(n("a/b.md", "c.md").as_deref(), Some("a/c.md"));
        assert_eq!(n("a/b.md", "../c.md").as_deref(), Some("c.md"));
        assert_eq!(n("a/b.md", "/root.md").as_deref(), Some("root.md"));
        assert_eq!(n("a/b.md", "#section").as_deref(), Some("a/b.md"));
        assert_eq!(n("a/b.md", "https://example.com/x"), None);
        assert_eq!(n("a/b.md", "../../escapes.md"), None);
    }

    #[test]
    fn decode_relation_documents_and_links() {
        let bytes = b"---\ntitle: T\nstatus: todo\n---\n# H\n\n[l](../x.md)\n";
        let docs = decode_relation(MarkdownRelation::Documents, bytes, "dir/d.md").unwrap();
        assert_eq!(docs.rows.len(), 1);
        assert_eq!(docs.schema.columns[0].name.as_str(), "path");
        let links = decode_relation(MarkdownRelation::Links, bytes, "dir/d.md").unwrap();
        assert_eq!(links.rows.len(), 1);
        // target_doc normalized against dir/d.md → x.md
        let td = links
            .schema
            .columns
            .iter()
            .position(|c| c.name == "target_doc")
            .unwrap();
        assert!(matches!(&links.rows[0].values[td], Value::Text(s) if s == "x.md"));
    }
}
