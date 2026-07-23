# Design brief — the codec relation surface, per-row decode, and provenance

*(Mission `a-file-collection-is-a-declared-set-over-any-blob-source`, ticket
`20260722090100`. This brief rules the design the implementation tickets `090200`/`090300`
build against. Ruled 2026-07-22 against the source of `crates/codec`, `crates/exec`, the
`crates/pushdown` planner, and `crates/driver-markdown`; the mission delegated the codec
relation-surface pick to this brief, ## Goal ruling 4.)*

## What is being ruled

The markdown interpretation yields **two named relations of the same format**: the flat
per-document relation, and the link relation (the full nested `section_path` graph). Three
things must be written down before code exists:

1. **How the second relation is named and reached** — the codec relation surface (the delegated
   pick).
2. **The per-row decode contract** — `DECODE` over a multi-row content-bearing set.
3. **The provenance contract** — the canonical `path` join id carried through every decode.

## Ruling 1 — the codec relation surface: a relation-qualified format token, backed by
codec-declared named relations

**Ruled: `DECODE <fmt>[.<relation>]`.** A codec *declares* its named relations (the mechanism);
a decode stage *addresses* one of them by a **relation-qualified format token** (the spelling).

- `decode md` — the codec's **primary** relation. For the `md` codec this is the flat
  per-document relation it yields today (front-matter keys as columns + a `body` column),
  **unchanged** (the mission's fixed constraint: "`decode md` keeps yielding the flat
  per-document relation").
- `decode md.documents` — the `documents` relation: `path`, `title` (front-matter `title`, else
  the first ATX heading, else null), `frontmatter` (the whole parsed YAML as one `json` value).
- `decode md.links` — the `links` relation: `source_doc`, `section_path` (the full nested
  ATX-heading path, a lossless `Array(Text)`, `[]` before any heading), `target`, `target_doc`
  (normalized root-relative, null for external/escaping), `line`.

A codec exposes its relation set through the registry: `Codec::relations() -> &[&str]` (the
primary named first) and `Codec::decode_relation(&self, relation: Option<&str>, bytes) ->
RowBatch`. `decode md` resolves `relation = None` → the primary; `decode md.links` resolves
`relation = Some("links")`. An unknown relation (`decode md.nope`) is a usage error listing the
codec's declared relations. A format with no `.` and a codec that declares only one relation
(json/yaml/toml/csv) behaves exactly as today — zero behavior change off the markdown path.

**Grammar impact — bounded, and not the §3 "no new grammar" the mission protects.** The
mission's "zero new grammar" rule governs *registration* (`CREATE <noun>` desugars to an
`INSERT` into a registry path, §3) — that is untouched. The codec *stage* gains one optional
suffix: the `codec` parser rule (`crates/parser/src/grammar.rs`, `fn codec`) reads the format
`ident`, then optionally a `.` punct + a relation `ident`. The `Codec` AST node
(`crates/parser/src/ast.rs`) gains `relation: Option<Ident>`. This is a leaf addition to one
pipe stage, fully backward-compatible with every existing `decode <fmt>` / `encode <fmt>`
statement, and it is regenerated into `docs/language.md` by `gen-docs` (never hand-edited).

**Rejected alternatives.**

- *A relation argument to `decode` (`decode md links`, two positional tokens).* Rejected: a
  bare second token reads as a second *operand*, not a sub-selector, and forecloses the natural
  future of codec *options* (`decode csv delimiter='\t'`), which will want a keyword/paren
  form. Two positional idents with no punctuation between them is also the hardest shape to read
  and the easiest to mis-lower (is `links` a relation, a column, a codec?). The dotted token
  keeps "one format, one of its relations" in a single lexeme.
- *Codec-declared named outputs reached by a NEW addressing form* (e.g. a `/codec/md/links`
  pseudo-path, or a `relation:` clause). Rejected: it invents a second way to say "which
  relation" alongside the path grammar the whole system already uses, for no gain over a
  qualified token. (The *mechanism* — a codec declaring named relations — is kept; only this
  spelling of the *reach* is rejected.)
- *`encode` gaining the same suffix now.* Deferred, not adopted: `encode` collapses rows→bytes
  and the markdown write round-trip (`set … |> encode md |> upsert`) is explicitly out of
  mission scope. `encode <fmt>` stays single-relation; the parser accepts the suffix uniformly
  (one rule) but a relation-qualified `encode` is a usage error until a writer needs it.

## Ruling 2 — `DECODE` runs per row over a collected set

**Ruled.** `DECODE <fmt>` applies the codec's `bytes → rows` contract to **each row** of a
multi-row content-bearing batch, and the per-file relations **union**. Concretely, in the codec
application (`crates/exec/src/codec.rs`, `apply_codecs`):

- For each input row, take its `content` bytes and run `decode_relation` → a per-file
  `RowBatch` of zero-or-more rows.
- **Union** every per-file batch into one output batch. The union is **schema-widening**: the
  output schema is the ordered union of all per-file columns; a column absent from one file's
  relation reads as **null** in that file's rows (not an error). This is what makes a `WHERE`
  over a front-matter key that only some files carry return the right rows (ruling below), and
  it is the direct mechanism behind acceptance Requirement 3's "sparse keys read as null."
- The **single-file case is the one-row instance** of the same rule. The
  `decode_needs_single_blob` refusal (`exec/src/codec.rs:111-140`) **retires**; there is no
  single-blob special case left. A content-free row (null `content` — e.g. a directory entry
  caught by a glob) contributes **no** rows (it is skipped, not an error), so a decode over a
  mixed listing is robust.
- `decode md.documents` yields **one** row per file; `decode md.links` yields **zero-or-more**
  rows per file (one per link). Both ride the identical per-row-then-union machinery — the
  cardinality difference is the codec's, not the application's.

**Materialization is plan-driven.** A glob/directory `/local` listing carries `content = null`
today (`driver-local/src/read.rs`), because a listing must not pay to read every file's bytes.
When a `DECODE` follows the collect, the engine knows the bytes are needed and asks the scan to
**materialize `content` per row**. The chosen mechanism: a `materialize_content: bool` on
`ScanNode` (`crates/pushdown/src/physical.rs`), set by the read executor
(`crates/exec/src/exec.rs`) when the statement carries a `DECODE` stage, honored by the local
read facet (`crates/qfs/src/shell.rs` / `driver-local/src/read.rs`), which reads each file
entry's bytes into `content` only when the flag is set. A plain listing (`/local/*.md |> select
name`) keeps `content = null` and pays nothing — the null-content listing behavior stays the
default; materialization is the decode-driven exception.

## Ruling 3 — provenance rides the decode application, not the codec

**Ruled.** Every decoded row carries the **root-relative `path`** column — the canonical join id
— and that column is owned by the **decode application**, not by any codec. A codec's
`bytes → rows` contract knows only bytes; it never sees a filename. `apply_codecs`, which holds
each source row, **prepends** that row's address as a `path` column onto every row the codec
produced from its bytes. Consequences fixed here:

- `documents.path`-style joins, backlink derivation (`links.target_doc = documents.path`), and
  "which file said this" survive the decode uniformly, for **every** format (json/yaml/csv/md
  alike), because the application adds `path` regardless of codec.
- The source address column is the collect segment's canonical id. For `/local` that is the
  listing's existing `path` value (the VFS path, e.g. `/local/notes/a.md`). The **root-relative**
  form the `/markdown` driver emits (e.g. `notes/a.md`) is produced by the registration/codec
  layer stripping the mount+root prefix; this brief fixes only that a single column named `path`
  is the join id and the application owns it. If a source row already carries a `path` column
  (the `/local` case), the provenance `path` is that value; the codec must not also emit `path`
  (the `md` codec does not).
- Because provenance is the application's, the `links` relation's `source_doc` is the **same
  value** as the provenance `path` of that file — the equivalence tickets assert both are the
  file's root-relative path.

## The two contracts the implementation tickets consume (restated for the record)

1. **Per-row decode over a content-bearing set** — ruling 2. The single-file case is the one-row
   instance; per-file relations union with schema-widening (missing column → null); null-content
   rows contribute nothing.
2. **Provenance** — ruling 3. The root-relative `path` column is carried through every decode as
   the canonical join id, owned by the decode application, uniform across codecs.

## Post-decode relational ops (consequence of the mission's Requirement 3)

Acceptance Requirement 3 (`… *.md |> decode md |> where <key> == …`) requires a relational op
**after** a decode. Today `apply_codecs`'s `codec_chain` rejects any relational op positioned
after a codec (`codec_then_query`, `exec/src/codec.rs:76-84`). This brief rules that the codec
tail **evaluates trailing relational ops locally over the decoded relation**: after the decode
produces its (data-dependent) schema, `WHERE`/`SELECT`/`LIMIT`/`ORDER BY`/`DISTINCT` following
the decode run in-process over the decoded batch (the batch is already fully materialized
locally — a straight in-memory filter/project, reusing the engine's residual evaluation).
`codec_then_query` is lifted for these ops; a post-decode op the local evaluator cannot yet run
still returns a clear, named error rather than silently mis-ordering. This keeps the decoded
schema late-bound (only known after the decode runs) while making the taught pipeline execute.

## Consistency with the sibling DSL mission

Where a **declared** driver (the sibling mission
`the-declared-driver-dsl-covers-the-compiled-drivers-concisely`) decodes a collected response
set, it rides **this** per-row rule and **this** provenance contract — the semantics are ruled
once, here, and not forked. A declared driver that returns a set of blobs and decodes them gets
per-row decode + `path` provenance for free; it does not define its own.
