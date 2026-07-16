---
created_at: 2026-07-17T02:12:00+09:00
author: a@qmu.jp
type: enhancement
layer: [Infrastructure]
effort:
commit_hash:
category: Changed
depends_on: [20260717021100-markdown-scanner-driver-crate-documents-and-links.md]
mission: markdown-trees-are-queryable-as-documents-and-links-tables
---

# Mount declared markdown roots through the engine (describe true AND reachable)

## Overview

Mission acceptance items 5 (describe true AND reachable), 6 (rescan) and the engine half of 2/3/7.
The binary composition root (`packages/qfs/crates/qfs/src/markdown.rs`) binds `path_binding` rows
to the live surface — learning from the `/claude` driver's documented mistake: `/claude` shipped
with ONLY a read facet registered (`shell.rs:328`) and **no `engine.mounts.register`**, so
`DESCRIBE` and `docs/drivers.md` were true while every SELECT raised `unknown_source`
(`pushdown/src/planner.rs` resolves against the MOUNT registry; the claude mission's findings).
So `/markdown` registers **both** facets, and an **engine-level SELECT test** — through
`qfs_exec::parse` + `block_on_read`, never the scanner struct directly — guards it.

- `path_binding_markdown_connections()` — System-DB bindings with `driver_id = "markdown"`,
  `(name, at_locator)` per `/markdown/<name>` row (mirror `git.rs:162`). `has_connections()`.
- `MarkdownReadDriver` (qfs-exec `ReadDriver`): resolve the node, look up the declared root,
  walk it (std::fs — recursive, `.md` files only, dot-entries skipped, symlinks not followed),
  parse via the driver crate's pure parser, return the `documents` / `links` batch in
  deterministic order. **Stateless read-through**: every scan re-walks, so results can never be
  stale (the rescan ruling in the design brief).
- `shell.rs register_cloud_and_sys_mounts`: when connections exist, register the mount
  (`engine.mounts.register(MarkdownDriver)`) AND the read facet
  (`reads.with(DriverId::new("markdown"), …)`) — one function serves shell + one-shot run.
- `describe.rs compiled_describe_registry`: add the (pure, rootless) `MarkdownDriver` so
  `qfs describe` and gen-docs surface it; `catalog.rs representative_path`:
  `"/markdown" => "/markdown/tree/documents"`. Regenerate `docs/drivers.md`.

## Implementation Steps

1. New module `crates/qfs/src/markdown.rs` (binding loader + read facet + tests); wire in
   `lib.rs`; add the `qfs-driver-markdown` dep to `crates/qfs/Cargo.toml`.
2. Register mount + read facet in `shell.rs` (comment citing the /claude lesson).
3. Describe registry + catalog arm + `cargo run -p xtask -- gen-docs`.
4. Tests (hermetic, tempdir fixture tree):
   - engine-level: mounts + reads registered, `parse("/markdown/docs/links |> …")` +
     `block_on_read` returns real rows (documents AND links) — the reachability guard;
   - a `path_binding`-seeded test (HomeGuard, mirror
     `qfs_connect_git_binding_converges_run_and_describe`) proving a persisted
     `CONNECT /markdown/docs TO markdown AT '<root>'` wires `has_connections` + describe;
   - rescan: add/edit/remove a file under the root → the NEXT engine SELECT reflects it;
   - non-`.md` ignored; join-ability: every in-tree `links.target_doc` value equals some
     `documents.path` value.

## Key Files

- `packages/qfs/crates/qfs/src/{git.rs,claude.rs,shell.rs,describe.rs,catalog.rs}` — patterns.
- `packages/qfs/crates/exec/src/exec.rs` — `parse` / `block_on_read`.
- `packages/qfs/crates/exec/tests/oneshot.rs` — the engine-level test shape.

## Policies

- `workaholic:implementation` / `multi-path-reachability` — the same facts reach the shell, the
  one-shot run, describe, and gen-docs from one registration seam.
- `workaholic:implementation` / `test` — the engine-level SELECT is the regression guard the
  /claude driver lacked.

## Quality Gate

1. With a binding declared and a fixture tree on disk, an engine-level
   `/markdown/<name>/documents |> LIMIT …` and `…/links |> WHERE …` both return real rows;
   without any binding, nothing registers (fail-closed).
2. `DESCRIBE` reports both schemas from the compiled registry (no connection needed) — and the
   SELECT test proves reachability is not describe-only.
3. Baseline gates: `cargo test --workspace`, clippy `-D warnings`, `fmt --check`,
   `gen-docs --check`, `gen-skills --check`, patch bump on the shipped PR.
