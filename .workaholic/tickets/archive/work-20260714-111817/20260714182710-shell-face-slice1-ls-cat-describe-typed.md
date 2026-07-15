---
created_at: 2026-07-14T18:27:10+09:00
author: a@qmu.jp
type: enhancement
layer: [UX, Domain]
effort: 4h
commit_hash: 97e90e3
category: Changed
depends_on: []
mission: language-design-review-layering-principles-and-semantic-gaps
needs_design_brief: false
---

# Shell face slice 1 — `ls`/`cat`/`describe` become path-typed; mount `/type`; re-found blueprint §9

## Overview

The shell face (interactive REPL, verbs as line-head idents, cwd session state, desugar-to-core,
gate inheritance) **already shipped in ticket t28** and is running code. The owner ruled **adopt**
(2026-07-14) on the design brief at
`.workaholic/missions/active/language-design-review-layering-principles-and-semantic-gaps/shell-face-design-brief.md`.
This first slice is pure **navigation** (no gate involvement) and closes the two places the shipped
shell **contradicts blueprint §5.1's "one type, three faces"**, plus records the ruled architecture
in the blueprint.

The concrete defect: `ls`'s desugar hardcodes the blob-listing projection
`SELECT name, size, is_dir, modified` (`crates/exec/src/shell/desugar.rs`), and `Schema::project`
errors on an unknown column — so **`ls /mail/inbox` and `ls /transform` fail with `unknown column`
today**. §5.1 clause (c) ("the type … shapes what `ls p` enumerates") is currently false for every
non-blob path.

## Design (settled — shell-face brief §4 slice 1, no new design)

- **`ls p` desugars by the path's ENTRY KIND** (its `describe`: archetype + schema). A **blob
  namespace** keeps the `name, size, is_dir, modified` projection; **every other entry kind lowers
  to the bare read `p`** — the path's rows ARE its enumeration (relational table = the rows,
  definition catalog = the defs = SHOW TYPES/SHOW TRANSFORMS, append log = the tail, object graph =
  the entities). The desugar already resolves the absolute path; give it the describe registry (the
  `Session` holds `engine.mounts`) so the projection choice is a **lookup, not a frozen guess**.
- **`cat p`** stays the bare read (degenerate `ls` for a leaf) — already correct.
- **`describe` becomes a pure REPL builtin** reusing `run_describe`'s machinery
  (`crates/exec/src/lib.rs` `run_describe`), so an operator can read a path's contract without
  leaving the session (today `describe` is one-shot CLI only).
- **Mount `/type` as a read-only catalog** mirroring `/transform`'s split: a pure describe facet
  (cred-free) plus a System-DB-injected read facet over `sys_drivers kind='type'` rows (the loader
  `load_declared_types` / `load_declared_type_defs` in `crates/qfs/src/declared_driver.rs` already
  reads them). Then `ls /type` = SHOW TYPES and `describe /type/<name>` teach the shape — the
  blueprint §5.4 claim that is currently ahead of the binary.
- **Blueprint re-founding**: record the ruled shell-face architecture as a §9 subsection — "the
  shell face is a REPL-layer desugar over the typed-path space; verb semantics derive from the
  path's entry kind; pure navigation builds no plan; gated mutation lowers to the closed core and
  rides preview→commit; zero new keywords; cwd is session state absolutized before parse." Also fix
  the stale §5.4 caveat "until that seam carries rows" — the pipeline-sourced membership seam
  **carries rows now** (`materialize_pipeline_source` → `check_table_membership`).

## Key Files

- `crates/exec/src/shell/desugar.rs` — `ls`/`cat` desugar keyed on the destination path's describe;
  thread the describe registry in (from `Session`).
- `crates/exec/src/shell/session.rs` — add the `describe` builtin path; supply the registry to the
  desugar.
- `crates/exec/src/shell/complete.rs` — the completer already `ls`-scans; confirm it still works
  once `ls` is entry-kind-typed.
- A new `/type` catalog mount: a describe facet (mirror `crates/driver-transform/`) + a binary read
  facet (`crates/qfs/src/…`, injected from the System DB like the transform read facet in
  `shell.rs`).
- `docs/blueprint.md` — §9 shell-face ruling subsection; §5.4 stale-caveat fix; §5.4/§5.1 "`ls /type`
  = SHOW TYPES" now true.

## Considerations

- **Pure navigation only** — this slice builds no effect plan and must touch nothing (describe is
  cred-free). Keep `ls`/`cat`/`describe` off the preview/commit gate entirely.
- **Blob projection stays** for blob namespaces — do not regress `ls /local`/`ls /drive`.
- **`/type` read facet fails closed** when no System DB resolves (like the transform facet): the
  mount still plans (describe is cred-free) but a scan surfaces a structured read error.
- Experimental repo — hard breaks are correct; no back-compat framing.

## Delivered scope (re-sliced by capability boundary)

Implementation revealed the ticket bundled work of very different sizes. **This slice ships the
core §5.1 defect fix + the ruling record**; the two *additive* pieces are split to a focused
follow-up (a `/type` catalog mount is a full read-only driver comparable to `/transform`'s crate,
and `describe`-as-a-REPL-builtin is Outcome/REPL-render plumbing — neither is a footnote):

- **Shipped here**: `ls` becomes **entry-kind-typed** — the desugar takes the resolved target's
  `describe` archetype (supplied by the session, which holds the registry) and keeps the blob
  `name/size/is_dir/modified` projection ONLY for `BlobNamespace`; every other kind lowers to the
  bare read (its rows ARE the enumeration). This closes the live `ls /mail/inbox` / `ls /transform`
  → `unknown column` defect. Blueprint §9 records the ruled shell-face architecture; §5.4's stale
  "until that seam carries rows" caveat is retired (the pipeline-sourced membership seam carries
  rows). `ls /transform` = SHOW TRANSFORMS now works (transform is already mounted).
- **Split to follow-up** (`#20260714182740-shell-face-type-mount-and-describe-builtin.md`): mount
  `/type` as a read-only catalog (so `ls /type` = SHOW TYPES resolves) and add `describe` as a REPL
  builtin.

## Quality Gate

- `cargo test/clippy/fmt/gen-docs/gen-skills/check-migrations` all green.
- New tests: `ls /mail/inbox`, `ls /transform`, `ls /type`, `describe /type/<name>` all answer
  inside one session; `ls /local` keeps the blob projection.
- Blueprint §9 records the shell-face ruling; §5.4 stale caveat removed.
- Plugin re-versioned if the shell verbs' taught surface changes (patch; navigation is additive).
