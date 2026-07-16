//! The pure markdown scanner: one document's text → its `documents` record + `links` records.
//!
//! **Pure string work — no I/O.** The qfs binary's read facet walks the declared root
//! (`std::fs` lives there, decision F) and feeds each file's root-relative path + text into
//! [`parse_document`]; this module never opens a file, so every behavior is pinned by hermetic
//! unit tests over string fixtures.
//!
//! ## What is recognised (the documented slice-1 scope, per the design brief)
//! - **Frontmatter**: an opening `---` line at the very start, closed by the next `---` line
//!   (the same fence semantics as the `md` codec). Parsed as YAML into ONE
//!   [`serde_json::Value`]; an unparseable block degrades to no frontmatter (robust, never an
//!   error — a broken file still lists).
//! - **Headings**: ATX only (`#`–`######` followed by whitespace), outside fenced code blocks.
//!   Trailing closing hashes (`## title ##`) are trimmed. Setext (underline) headings are NOT
//!   recognised — a documented limitation, not a promise.
//! - **Links**: inline `[text](target)` links, outside fenced code blocks, with nested-bracket
//!   text and nested-paren targets handled by counting. Images (`![alt](src)`) are excluded;
//!   autolinks (`<https://…>`) and reference-style links (`[text][ref]`) are NOT recognised —
//!   documented limitations for the minimal slice.
//! - **`source_section_path`**: the stack of enclosing ATX headings, top-level first, at the
//!   line the link is written on — the FULL nested path, never collapsed to the nearest heading
//!   (the whole point of the minimal version; the later vocabulary mission types this column).
//!   A link before any heading carries the empty path.
//! - **`target_doc` normalization**: the joinable root-relative form of an in-tree target —
//!   fragment/query stripped, `<…>` unwrapped, `./`/`../` resolved against the source
//!   document's directory, a leading `/` treated as root-relative. `None` for a target with a
//!   URL scheme (`https:`, `mailto:`, …), a protocol-relative `//…`, or one that escapes the
//!   root; a pure `#fragment` link normalizes to the source document itself.

use qfs_types::{Row, Value};

use crate::schema::{markdown_node_schema, MarkdownNode};

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
    /// This document's `documents` row, in [`markdown_node_schema`] column order.
    #[must_use]
    pub fn document_row(&self) -> Row {
        Row::new(vec![
            Value::Text(self.path.clone()),
            self.title.clone().map_or(Value::Null, Value::Text),
            self.frontmatter.clone().map_or(Value::Null, Value::Json),
        ])
    }

    /// This document's `links` rows, in file order, in [`markdown_node_schema`] column order.
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

/// Parse one markdown document (pure; see the module docs for the recognised scope).
/// `rel_path` is the document's root-relative path — it becomes `documents.path` /
/// `links.source_doc` and anchors relative-target normalization.
#[must_use]
pub fn parse_document(rel_path: &str, text: &str) -> ParsedDocument {
    let (frontmatter_lines, frontmatter) = parse_frontmatter(text);

    let mut heading_stack: Vec<(u8, String)> = Vec::new();
    let mut first_heading: Option<String> = None;
    let mut links: Vec<ParsedLink> = Vec::new();
    let mut in_fence = false;

    for (idx, line) in text.lines().enumerate() {
        let line_no = (idx + 1) as u64;
        // Skip the frontmatter block (its lines still count toward line numbers).
        if (idx as u64) < frontmatter_lines {
            continue;
        }
        let trimmed = line.trim_start();
        // Fenced code toggling: a ``` or ~~~ fence line opens/closes a block; headings and
        // links inside contribute nothing. (Fence-length matching is deliberately simple —
        // any fence line toggles — a documented simplification.)
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        if let Some((level, heading_text)) = parse_atx_heading(line) {
            // Pop to the parent level, push this heading: the stack IS the section path.
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

/// The `documents` batch schema (re-exported for the binary's read facet).
#[must_use]
pub fn documents_schema() -> qfs_types::Schema {
    markdown_node_schema(MarkdownNode::Documents)
}

/// The `links` batch schema (re-exported for the binary's read facet).
#[must_use]
pub fn links_schema() -> qfs_types::Schema {
    markdown_node_schema(MarkdownNode::Links)
}

/// Split + parse the YAML frontmatter block. Returns `(lines_consumed, value)`:
/// `lines_consumed` counts the opening fence through the closing fence inclusive, so the
/// caller can skip those lines while keeping true file line numbers. An unparseable YAML block
/// (or a non-map) degrades to `None` with the lines still consumed — a broken frontmatter
/// never hides the body's links.
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
    // No closing fence: not frontmatter — treat the whole text as body.
    (0, None)
}

/// Parse an ATX heading line: `#{1,6}` + whitespace + text (CommonMark requires the space;
/// `#hashtag` is not a heading). Trailing closing hashes are trimmed.
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

/// Extract every inline `[text](target)` link target on one line, in order, excluding images
/// (`![alt](src)`) and text inside backtick code spans. Nested brackets in the text and nested
/// parens in the target are handled by counting; the target has any `<…>` wrapper removed and a
/// space-separated title (`(url "title")`) stripped.
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
                // An image's `[` is preceded by `!` — skip the whole image link.
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

/// Try to read one `[text](target)` starting at the `[` at `open`. Returns the raw target and
/// the index just past the closing `)`.
fn link_at(bytes: &[u8], open: usize) -> Option<(String, usize)> {
    // Match the closing `]` with bracket counting (link text may nest brackets).
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
    // The target must open immediately after the `]`.
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
    // `<url>` wrapper, then a space-separated `"title"` tail.
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

/// Normalize a link target into the root-relative `target_doc` join key (see the module docs).
fn normalize_target(source_doc: &str, target: &str) -> Option<String> {
    // A URL scheme (`https:`, `mailto:`, …) or protocol-relative `//…` is external.
    if target.starts_with("//") || has_url_scheme(target) {
        return None;
    }
    // Strip the fragment, then the query.
    let no_frag = target.split_once('#').map_or(target, |(p, _)| p);
    let path_part = no_frag.split_once('?').map_or(no_frag, |(p, _)| p);
    // A pure `#fragment` (or empty) target references the source document itself.
    if path_part.is_empty() {
        return Some(source_doc.to_string());
    }
    // A leading `/` is root-relative; otherwise resolve against the source's directory.
    let mut segments: Vec<&str> = if let Some(abs) = path_part.strip_prefix('/') {
        Vec::from_iter(abs.split('/'))
    } else {
        let mut base: Vec<&str> = source_doc.split('/').collect();
        base.pop(); // the source file name
        base.extend(path_part.split('/'));
        base
    };
    // Resolve `.` / `..`; escaping the root makes the target un-joinable (None).
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

/// Whether the target starts with a URL scheme per RFC 3986 (`ALPHA *( ALPHA / DIGIT / "+" /
/// "-" / "." ) ":"`). A Windows-drive-like `C:` matches too — acceptable, since a drive path is
/// not an in-tree reference either.
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

#[cfg(test)]
mod tests {
    use super::*;

    fn targets(doc: &ParsedDocument) -> Vec<&str> {
        doc.links.iter().map(|l| l.target.as_str()).collect()
    }

    /// The load-bearing behavior (mission acceptance item 3): a link under nested headings
    /// carries the FULL heading path in order — both levels, never only the nearest.
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
        // The sibling `##` popped 懸念; the `###` nests under 次の一歩.
        assert_eq!(
            doc.links[2].section_path,
            vec!["全体の振り返り", "次の一歩", "詳細"]
        );
    }

    /// A pre-heading link carries the EMPTY path (item 3), and heading levels may skip.
    #[test]
    fn pre_heading_link_is_empty_and_skipped_levels_stack() {
        let text = "\
[before any heading](top.md)

# a

### deep-under-a

[x](x.md)

## shallower

[y](y.md)
";
        let doc = parse_document("d.md", text);
        assert_eq!(doc.links[0].section_path, Vec::<String>::new());
        assert_eq!(doc.links[1].section_path, vec!["a", "deep-under-a"]);
        // `##` pops the `###` but stays under `#`.
        assert_eq!(doc.links[2].section_path, vec!["a", "shallower"]);
    }

    /// Fenced code contributes neither headings nor links (item 7's fixture behavior).
    #[test]
    fn fenced_code_is_excluded() {
        let text = "\
# real

```
# fake heading
[fake](fake.md)
```

[real](real.md)

~~~
[also fake](nope.md)
~~~
";
        let doc = parse_document("d.md", text);
        assert_eq!(targets(&doc), vec!["real.md"]);
        assert_eq!(doc.links[0].section_path, vec!["real"]);
    }

    /// Frontmatter parses into the Json column; `title` prefers frontmatter over the first
    /// heading; line numbers count the frontmatter lines (true file coordinates).
    #[test]
    fn frontmatter_parses_and_title_prefers_it() {
        let text = "\
---
title: The Declared Title
status: active
---

# A Heading

[l](l.md)
";
        let doc = parse_document("d.md", text);
        assert_eq!(doc.title.as_deref(), Some("The Declared Title"));
        let fm = doc.frontmatter.as_ref().expect("frontmatter present");
        assert_eq!(fm.get("status").and_then(|v| v.as_str()), Some("active"));
        assert_eq!(doc.links[0].line, 8);
    }

    /// Without frontmatter the first ATX heading is the title; with neither, NULL.
    #[test]
    fn title_falls_back_to_first_heading_then_none() {
        let doc = parse_document("d.md", "# First\n\n## Second\n");
        assert_eq!(doc.title.as_deref(), Some("First"));
        assert!(doc.frontmatter.is_none());
        let doc = parse_document("d.md", "no headings here\n");
        assert!(doc.title.is_none());
    }

    /// Broken YAML frontmatter degrades to no frontmatter — the body's links still index.
    #[test]
    fn broken_frontmatter_never_hides_the_body() {
        let text = "---\n: : not yaml : :\n---\n\n[l](l.md)\n";
        let doc = parse_document("d.md", text);
        assert!(doc.frontmatter.is_none());
        assert_eq!(targets(&doc), vec!["l.md"]);
    }

    /// Images are excluded; code spans hide links; nested brackets/parens are handled;
    /// `<url>` unwraps; a `"title"` tail is stripped.
    #[test]
    fn link_extraction_edge_cases() {
        let text = "\
![an image](img.png) and [a [nested] link](a.md) and `[in code](x.md)`
[wrapped](<b c.md>) [titled](t.md \"the title\") [parens](p(1).md)
";
        let doc = parse_document("d.md", text);
        assert_eq!(
            targets(&doc),
            vec!["a.md", "b", "t.md", "p(1).md"],
            "images and code-span links excluded; wrappers/titles stripped"
        );
    }

    /// `#hashtag` is not a heading; `## closed ##` trims its closing hashes.
    #[test]
    fn atx_heading_shapes() {
        assert_eq!(parse_atx_heading("# a"), Some((1, "a".to_string())));
        assert_eq!(
            parse_atx_heading("###### deep"),
            Some((6, "deep".to_string()))
        );
        assert_eq!(
            parse_atx_heading("## closed ##"),
            Some((2, "closed".to_string()))
        );
        assert_eq!(parse_atx_heading("#hashtag"), None);
        assert_eq!(parse_atx_heading("####### seven"), None);
        assert_eq!(parse_atx_heading("plain"), None);
    }

    /// `target` stays as written; `target_doc` is the normalized join key (item 3): `./`,
    /// `../`, root-`/`, fragment and external forms.
    #[test]
    fn target_normalization() {
        let n = |src: &str, t: &str| normalize_target(src, t);
        assert_eq!(n("a/b.md", "c.md").as_deref(), Some("a/c.md"));
        assert_eq!(n("a/b.md", "./c.md").as_deref(), Some("a/c.md"));
        assert_eq!(n("a/b.md", "../c.md").as_deref(), Some("c.md"));
        assert_eq!(n("a/b.md", "/root.md").as_deref(), Some("root.md"));
        assert_eq!(n("a/b.md", "c.md#section").as_deref(), Some("a/c.md"));
        assert_eq!(n("a/b.md", "#section").as_deref(), Some("a/b.md"));
        assert_eq!(n("a/b.md", "https://example.com/x"), None);
        assert_eq!(n("a/b.md", "mailto:x@example.com"), None);
        assert_eq!(n("a/b.md", "//cdn.example.com/x"), None);
        assert_eq!(n("a/b.md", "../../escapes.md"), None);
    }

    /// Rows conform to the canonical schemas (no drift between describe and the scan).
    #[test]
    fn rows_match_the_described_schemas() {
        let doc = parse_document("d.md", "# t\n\n[l](l.md)\n");
        let drow = doc.document_row();
        assert_eq!(drow.values.len(), documents_schema().columns.len());
        let lrows = doc.link_rows();
        assert_eq!(lrows.len(), 1);
        assert_eq!(lrows[0].values.len(), links_schema().columns.len());
        assert!(matches!(&lrows[0].values[1], Value::Array(_)));
    }
}
