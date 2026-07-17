---
created_at: 2026-07-17T02:11:00+09:00
author: a@qmu.jp
type: enhancement
layer: [Domain]
effort:
commit_hash:
category: Changed
depends_on: [20260717021000-rule-the-markdown-path-shape-and-root-declaration.md]
mission: markdown-trees-are-queryable-as-documents-and-links-tables
---

# The markdown scanner driver crate: documents + links with section paths

## Overview

Mission acceptance items 2 (documents), 3 (links with section context), 4 (no typing by
construction) and the parser half of 7 (hermetic tests). A new leaf crate
`packages/qfs/crates/driver-markdown` (`qfs-driver-markdown`) modelled on
`qfs-driver-directory` / `qfs-driver-claude`'s pure-introspective pattern:

- **Node model** (`schema.rs`): mount `/markdown`; `MarkdownNode::{Documents, Links}` resolved
  from `/markdown/<name>/documents` / `/markdown/<name>/links` (per the design brief); static
  typed schemas:
  - `documents`: `path` Text (root-relative, the join key), `title` Text nullable (frontmatter
    `title`, else first ATX heading, else NULL), `frontmatter` Json nullable (the whole parsed
    YAML frontmatter map).
  - `links`: `source_doc` Text, `source_section_path` Array(Text) NOT NULL (`[]` pre-heading),
    `target` Text (as written), `target_doc` Text nullable (normalized root-relative target,
    joinable against `documents.path`; NULL for external/escaping targets), `line` Int.
  - **No relation-type column** — the vocabulary mission layers on `source_section_path` later;
    stated in the crate docs.
- **Pure parser** (`parse.rs`): `parse_document(rel_path, text)` → one document record + link
  records. Frontmatter split (mirror `codec/src/codecs/markdown.rs` fence semantics), ATX
  heading stack (pop ≥ level, push; skipped levels fine), fenced-code exclusion (``` / ~~~),
  inline `[text](target)` links with image (`![`) exclusion, nested brackets/parens, `<url>`
  stripping and title-after-space stripping. 1-based line numbers over the WHOLE file
  (frontmatter lines counted). Documented limitations: ATX only (no setext), inline links only
  (no reference-style / autolinks) — honest, in the crate docs.
- **Driver** (`lib.rs`): `MarkdownDriver` — `describe` (RelationalTable + the static schema),
  `capabilities` = SELECT only on both tables (writes rejected at parse time),
  `PushdownProfile::None`, `NoopApplier` (read-only slice; no qfs-runtime dep — the crate stays
  a pure leaf like `qfs-driver-directory`).

## Implementation Steps

1. `Cargo.toml`: deps `qfs-driver`, `qfs-plan`, `qfs-types`, `thiserror`, `serde_json`,
   `serde_yaml_ng` (the codec crate's YAML choice). Workspace lints.
2. `schema.rs`: `MARKDOWN_MOUNT`, `MarkdownNode`, `node_for_path`, `tree_name`,
   `markdown_node_schema`.
3. `parse.rs`: the pure scanner (no I/O — filesystem walking lives in the binary ticket).
4. `lib.rs`: `MarkdownDriver` + capabilities + NoopApplier + crate docs stating the no-typing
   rule and the parser limitations.
5. Hermetic unit tests: nested-heading section path (multi-level, in order), pre-heading link
   `[]`, heading-stack pop on sibling/higher heading, fenced code ignored (links AND headings),
   frontmatter → Json + title precedence, image exclusion, target normalization (`./`, `../`,
   root-`/`, `#fragment` → source_doc, external scheme → NULL, escape-root → NULL), purity
   (describe reads nothing), read-only capability gate, object safety.

## Key Files

- `packages/qfs/crates/driver-directory/src/lib.rs` — the pure read-only driver pattern.
- `packages/qfs/crates/driver-claude/src/schema.rs` — the node-model pattern.
- `packages/qfs/crates/codec/src/codecs/markdown.rs` — the frontmatter fence semantics to mirror.

## Policies

- `workaholic:implementation` / `type-driven-design` — the schema is the single source of truth
  describe and the scan both read; the no-typing rule is structural (no column to misuse).
- `workaholic:implementation` / `coding-standards` + `test` — workspace lints (no unwrap/expect
  outside tests), hermetic tests only.

## Quality Gate

1. A fixture document with a link under two nested headings yields
   `source_section_path = ["top", "nested"]` in order; a pre-heading link yields `[]`.
2. Fenced code containing `# fake heading` and `[fake](x.md)` contributes neither.
3. `describe` of both tables returns the static schemas with no I/O; `INSERT`/`UPDATE`/`REMOVE`
   are structurally rejected.
4. `cargo test -p qfs-driver-markdown`, clippy `-D warnings`, fmt all green.
