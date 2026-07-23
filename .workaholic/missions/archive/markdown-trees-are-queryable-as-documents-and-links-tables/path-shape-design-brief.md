# Design brief — the markdown collection path's shape and root declaration

*Mission `markdown-trees-are-queryable-as-documents-and-links-tables`, acceptance item 1.
Raised 2026-07-17 (ticket `20260717021000`). Deliverable: a recorded ruling the implementation
tickets follow — path shape, root declaration, section-path encoding, the no-typing rule, and
the rescan mechanism.*

## 1. State — what exists today

- The canonical way a local resource becomes a qfs mount is a **`path_binding` row** in the
  System DB: `qfs connect <path> --driver <id> --at '<locator>'` (CLI,
  `connection.rs:462 run_connect`) or the language `CONNECT <path> TO <driver> AT '<locator>'`
  (via the `/sys/paths` backend) both upsert the row in one transaction with its audit row +
  `ddl_event` (re-homed to the System DB by `20260716143641`). Composition roots load bindings
  per driver id — the model is `git.rs:162 path_binding_git_connections()`, which mounts
  `CONNECT /git/app TO git AT '<path>'` at `/git/app/...`.
- The deprecated seams — `QFS_SQL_*`/`QFS_GIT_*` env vars and the `connections.qfs` file — are
  warned shims being retired (ticket `20260716214200`). The declared-drivers mission's rule:
  **no name-shaped `QFS_*` env var as the working path**; a committed, reviewable declaration is
  the normal way. (`/claude`'s `QFS_CLAUDE_SESSIONS` env var is the counter-example, not the
  pattern.)
- The engine needs BOTH registrations for a surface to work: the mount registry (planning —
  `engine.mounts.register`) and the read registry (execution — `reads.with(DriverId, facet)`).
  `/claude` shipped with only the read facet, so `DESCRIBE` was true while every SELECT raised
  `unknown_source` (the claude mission's findings). Any ruling here must be verified by an
  engine-level SELECT, not by describe alone.

## 2. Ruling

1. **Path shape.** The driver mounts at **`/markdown`**. One declared root `<name>` resolves
   exactly two relational tables:
   - `/markdown/<name>/documents`
   - `/markdown/<name>/links`

   The table names keep the strategy vocabulary one-to-one (`documents` / `links`). The mount
   itself and `/markdown/<name>` are not nodes; only the two tables describe and scan.
2. **Root declaration** rides the declared-drivers convention on the canonical registry:

   ```
   qfs connect /markdown/<name> --driver markdown --at '/abs/path/to/root'
   CONNECT /markdown/<name> TO markdown AT '/abs/path/to/root'
   ```

   Both land the same ledgered `path_binding` row (`driver_id = "markdown"`, `at_locator` = the
   root directory). Multiple roots on one box = multiple bindings with distinct `<name>`s —
   which is how qfs-viewer points at arbitrary repository checkouts. **No env var ships at
   all**, and the deprecated `connections.qfs` seam is not extended. Markdown roots carry no
   secret, so no `SECRET` clause applies.
3. **`source_section_path` encoding** is a **JSON array of heading texts**
   (`ColumnType::Array(Text)`, e.g. `["全体の振り返り", "懸念"]`), ordered top-level heading
   first, nearest enclosing heading last. A link written before any heading carries the empty
   array `[]`. An array is lossless by construction — heading text can contain any delimiter,
   so no flat string encoding is used. This is the documented contract the later vocabulary
   mission types.
4. **No typing, by construction.** The `links` schema carries **no relation-type column** and
   the indexer infers nothing from heading text. The closed relation vocabulary
   (`parent`/`concerns`/`references`/… — "declare and reject, never guess") is a later,
   separate mission layered on the preserved `source_section_path`.
5. **Rescan.** Slice 1 ships a **stateless read-through scanner**: every engine scan re-walks
   and re-parses the declared root, so `documents`/`links` can never be stale — the rescan
   entry point is the scan itself, and this is pinned by a test (modify the tree, the next
   engine SELECT reflects it). If a later slice introduces a persistent index or cache, an
   explicit `CALL markdown.rescan` procedure becomes the entry point. Hot reload / file
   watching stays unshipped without penalty.

## 3. Consequences

- The composition root mirrors git's: `path_binding_markdown_connections()` +
  `has_connections()` gate; registration happens once in `shell.rs
  register_cloud_and_sys_mounts` (serving both the interactive shell and one-shot `qfs run`),
  registering **both** the mount and the read facet — with an engine-level SELECT test as the
  regression guard the `/claude` driver lacked.
- `DESCRIBE` stays pure and connection-free: the compiled describe registry carries a rootless
  `MarkdownDriver`, so `docs/drivers.md` documents `/markdown` deterministically and qfs-viewer's
  generic describe-lowering needs no markdown-specific code.
- Parser scope for slice 1 (documented in the crate): ATX headings only, inline
  `[text](target)` links only (images, autolinks, and reference-style links excluded), fenced
  code blocks excluded, dot-entries under the root skipped, symlinks not followed. `target` is
  kept as written; `target_doc` adds the normalized root-relative form for joins against
  `documents.path` (NULL for external or root-escaping targets; a pure `#fragment` link
  normalizes to the source document itself).
