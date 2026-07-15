---
created_at: 2026-07-09T05:45:42+09:00
author: a@qmu.jp
type: bugfix
layer: [Domain, Infrastructure]
effort:
commit_hash: 8f063e6
category: Changed
depends_on:
mission:
---

# Resume transform epic: apply the T1+T2 review findings, then T3 → T4

## Outcome (2026-07-09 night drive, commit 8f063e6)

All 7 findings applied. The `struct<>` fix accepts the lexer's single `Ne` token in the STRUCT
branch (nested form covered by an encoder round-trip test). The pushdown INPUT check threads a
name-availability fold (`available_columns`) over the lowered subtree — `None` under `EXPAND`
(names indeterminate at lowering; the eval fold still catches those) — and fails with the same
`transform_input_missing` code as eval. Fix #6 took the comment-cross-reference option: the parser
deliberately depends only on qfs-lang, so the scheme list stays duplicated with sync comments on
both sides (moving it to qfs-types would still not be reachable without a new parser dep). One
System-DB open at shell startup (`load_transform_defs()` deleted; the read backend feeds the
registry). T3 and T4 continue as their own tickets in this same drive.

## Why this ticket exists (carry checkpoint, 2026-07-09)

A `/drive` session implemented **Transform T1** (definition DDL/storage/lifecycle) and **T2** (plan
spine), then ran a high-effort `/code-review` on `main..HEAD`. This is a **capture-only handoff** so
a fresh `/drive` continues without relying on compaction. Nothing here is implemented yet.

## Position (verified state)

- Branch `work-20260709-023822`. Commits (all with the full gate green — per-crate tests,
  `clippy --workspace --all-targets -D warnings`, `fmt --all --check`,
  gen-docs/gen-skills/check-migrations, `dep_direction`):
  - `30dd2bb` cf-artifacts owner decision recorded (Option A).
  - `5c22fe7` **T1** — CREATE TRANSFORM definition DDL, storage (v16 `sys_transforms`), `/transform`
    driver (`crates/qfs/crates/driver-transform`), binary backend (`crates/qfs/src/transform.rs`),
    containment, provision SoT `Transforms` collection. Archived ticket:
    `.workaholic/tickets/archive/work-20260709-023822/20260708192730-transform-definition-ddl-storage.md`.
  - `1a2462f` **T2** — plan spine: `LogicalPlan::Transform` / `CombineOp::Transform`, lowering arm,
    forced-local, `PlanSource::Transform` OUTPUT fold, the single `EngineError::TransformNotExecutable`
    exec refusal. Archived ticket: `…/20260708192731-transform-plan-spine.md`.
- Working tree clean at `1a2462f`. **No `crates/qfs/Cargo.toml` patch bump yet** (CLAUDE.md: bump on
  the shipped PR; do it before `/report`→`/ship`). The branch also carries an unrelated pre-existing
  SQLite-flake fix (2 commits) — a mixed-concern PR; the owner may want to split at ship time.

## Remaining work, in order

### 1. Apply the T1+T2 code-review findings (NEW — not yet ticketed)

A high-effort review confirmed **2 correctness + 5 cleanup** findings (2 others were refuted and are
correct as-is: the eval INPUT-column check is right for relation-wise; the ddl_event name-only payload
is fine). Fix these first (they are in the already-landed T1/T2 code):

**Correctness:**
1. **`struct<>` (empty struct) never parses** — `packages/qfs/crates/parser/src/grammar.rs:1470`
   (`transform_type` STRUCT branch). The lexer tokenizes adjacent `<>` as a single `Token::Ne`
   (lex.rs `two('<','>')`), so `opt(punct(Token::Lt))` never matches and
   `CREATE TRANSFORM t INPUT (x struct<>) …` fails at the `)`. `struct<>` is the exact canonical
   string `ColumnType::parse` accepts and that appears in stored JSON — encoder/decoder disagree.
   `struct<a struct<>>` breaks the same way. **Fix:** in the STRUCT branch, accept a leading
   `Token::Ne` as the empty field-list (`struct<>`), else the `Lt … Gt` form. Add a grammar
   round-trip test (encoder side) — the existing qfs-types test only covers the decoder.
2. **pushdown lowering skips the INPUT-column presence check** —
   `packages/qfs/crates/pushdown/src/lower.rs:331` (the `PipeOp::Transform` arm). The eval schema
   fold (`crates/core/src/eval.rs:695`) rejects a missing declared INPUT column with
   `transform_input_missing`, but the pushdown path folds to OUTPUT without checking, so the same
   query gives different diagnostics by planning surface and the execution path never surfaces the
   real cause. **Fix:** thread the incoming relation schema to the transform lowering arm and apply
   the same by-name INPUT-column check (surplus incoming columns ignored; missing = structured
   lower error). Note: lowering does not currently track the running schema — the fix needs schema
   availability at that point (the input `LogicalPlan`'s resolved schema).

**Cleanup:**
3. **System DB opened twice on shell startup** — `packages/qfs/crates/qfs/src/shell.rs:240`. The read
   backend opens at :226 (moved into the read driver), then `load_transform_defs()` at :240 opens a
   second time. **Fix:** call `backend.load_defs()` on the first backend before moving it (one open).
4. **Wrong SQL comment on stored format** — `packages/qfs/crates/store/src/schema/system_transforms.sql:22`.
   The `input`/`output` comment says `qfs_types::Schema` serde JSON, but the real stored value is the
   flat descriptor array `[{"name","type","nullable"}]` (what the grammar emits and
   `TransformDef::from_stored` decodes). `state.rs::TransformRow` documents it correctly. **Fix:** the
   comment.
5. **Redundant OUTPUT parse in mode derivation** — `packages/qfs/crates/qfs/src/transform.rs:107`
   (`derived_mode`). `TransformDef::from_stored` parses+validates BOTH input and output per scanned
   row, but the mode is a pure function of `input`. **Fix:** decode only `input` and call
   `qfs_types::derive_mode` for the `mode` column.
6. **Duplicated secret-ref prefix rule** — `packages/qfs/crates/parser/src/grammar.rs:1385` inlines
   `starts_with("env:") || starts_with("vault:")`; `core::ddl::transform::is_secret_reference` is the
   reusable rule. **Fix:** the parser can't reach qfs-core (dep direction), so EITHER move the scheme
   list to a shared leaf (qfs-types) and have both call it, OR leave a comment cross-referencing the
   two and keep them in sync. (Confirm the dep direction before deciding.)
7. **Over-widened `pub(crate) fn text`** — `packages/qfs/crates/qfs/src/sys.rs:1160`. `text` was
   widened alongside the other helpers but `transform.rs` never imports it and nothing else references
   `crate::sys::text`. **Fix:** return `text` to private (keep the others `pub(crate)`).

### 2. Implement T3 — execution/routing (ticket already in todo)

`.workaholic/tickets/todo/a-qmu-jp/20260708192732-transform-execution-routing.md`. Depends on T1+T2
(both landed). The `ModelProvider` seam + injected applier (copy `driver-claude`'s
`SessionSource`/`FakeSource` shape), the whole-tree classifier (generalize `exec/src/lib.rs:206`
terminal-`CALL` → any `PipeOp::Transform` anywhere), exec-layer orchestration, the three cardinality
modes against a mock, the irreversible gate with a **model-free PREVIEW**, and the committed-read
rows+`meta.affected` envelope. **T3 deletes the single `EngineError::TransformNotExecutable` refusal
in `crates/qfs/crates/engine/src/combine.rs`** and replaces it with the applier. Fully hermetic (mock
provider); the live run is T4. This dev host has LIVE cloud accounts — never verify against a real
provider here.

### 3. Implement T4 — docs / skills / versions / Decision-K sweep / live run (ticket in todo)

`.workaholic/tickets/todo/a-qmu-jp/20260708192733-transform-docs-versioning-live-run.md`. Depends on
T3. Adds the TRANSFORM EBNF rule (regenerate `docs/language.md`), one parse-checked cookbook recipe,
regenerates skills, the **minor** plugin bump (four fields) + qfs patch bump, the ten Decision-K
citation re-points, and the single owner-approved live-provider run.

## Considerations

- Grammar surface reminder for whoever fixes #1 / touches T3/T4: model/provider/effort values with
  non-ident chars must be quoted (`MODEL 'claude-sonnet-5'`) — `-` is not a lexer token.
- The resolver rides `MountRegistry.transform_defs` (installed by
  `crate::transform::load_transform_defs()` in `shell.rs::register_cloud_and_sys_mounts`) — the pure
  planner/evaluator read it; the binary is the only DB reader.
- Experimental / no backward compat: fix definitively; no shims.
- Commit via `workaholic:commit` `commit.sh` with explicit file args; never `git add -A`
  (shared-tree concurrent sessions). Run the full gate set before archiving each ticket.
