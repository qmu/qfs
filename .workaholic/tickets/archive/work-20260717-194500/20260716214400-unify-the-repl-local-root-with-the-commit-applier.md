---
created_at: 2026-07-16T21:44:00+09:00
author: a@qmu.jp
type: bugfix
layer: [Domain]
effort:
commit_hash:
category: Changed
depends_on:
mission:
---

# Unify the REPL's /local root with the commit applier's

## Overview

Concern `the-interactive-shell-s-local-reads` (moderate — the only live wrong-write-target class
in the corpus). Verified against source this session:

- The **interactive shell** roots `/local` reads and plans at the **process cwd**:
  `run_interactive_shell` (`shell.rs:106`) computes `root = std::env::current_dir()` (:110) and
  builds both the plan mount `LocalFsDriver::new(root)` and the read facet
  `LocalReadDriver::new(Sandbox::new(root))` via `local_engine_and_reads(root)` (:132-142).
- The **commit applier** the same REPL invokes (`run_repl` :659 → `apply_plan` :666-671)
  hardcodes the filesystem root: `live_registry()` builds `LocalFsDriver::new("/")`
  (`commit.rs:246`, doc :235-237 "a VFS path /local/<p> maps to host /<p>").

So an interactive `cp`/`mv`/`UPSERT` whose preview showed a cwd-relative target COMMITS against
`/` — a mis-targeted write to the filesystem root, as whatever user runs the shell. Only the
interactive path diverges: the one-shot `qfs run` deliberately roots BOTH sides at `/`
(`shell.rs:150-159`, `job.rs:225`), and the golden REPL tests never reach the commit leg
(`run_repl_with_history` passes `apply = None`, `shell.rs:688`), so the divergence is unpinned
by any test.

## Implementation Steps

1. **One root, constructed once per launch context.** Parameterize the commit side — a
   `live_registry(root)` / `apply_plan`-with-root variant — and have `run_interactive_shell`
   thread its cwd `root` into BOTH `local_engine_and_reads` and the applier it passes to the
   REPL. The one-shot and job paths keep passing `/` explicitly and stay behavior-identical.
2. Keep the existing `apply_plan(plan)` signature for the one-shot callers (delegating to the
   rooted variant with `/`), so the change is confined to the interactive wiring.
3. **Pin it.** A test that runs a REPL cp/UPSERT through the real apply hook inside a temp cwd
   and asserts the write lands under the temp root, never under `/`. Write it against current
   code first — it must fail by targeting `/` (assert on the planned/applied host path, not by
   actually writing to `/`).
4. Document the sandbox rule where the shell comment already lives (`shell.rs:112-116`): the
   interactive session's `/local` is cwd-rooted on BOTH faces; one-shot and jobs are `/`-rooted
   on both.

## Key Files

- `packages/qfs/crates/qfs/src/shell.rs:106-160,659-690` — the root computation and the REPL
  apply wiring.
- `packages/qfs/crates/qfs/src/commit.rs:235-250` — `live_registry`'s hardcoded `/`.
- `packages/qfs/crates/qfs/src/job.rs:225` — the job path that must stay `/`-rooted.
- `packages/qfs/crates/qfs/src/shell.rs:815-850` — the golden REPL test fixtures to extend
  through the apply leg.

## Policies

- `workaholic:design` / data handling — a preview that shows one target and a commit that hits
  another is precisely the silent wrong-node-write class the product's safety loop exists to
  prevent.
- `workaholic:implementation` / `type-driven-design` — the root becomes a constructor argument
  owned by the launch context, not a per-side literal.
- `workaholic:implementation` / `test` — the commit leg of the REPL gains its first real-apply
  pin.

## Quality Gate

1. Both-directions: the new REPL-commit test fails on current code (target resolves under `/`)
   and passes after (target under the session cwd).
2. One-shot `qfs run` and JOB behavior byte-identical (existing suites stay green — they pin the
   `/`-rooted mapping).
3. Preview and commit of the same interactive statement name the same host path (assert the
   ledger/audit path equals the previewed one).
4. Baseline gates + patch bump.

## Considerations

- Do NOT change what one-shot `/local` means; the divergence is interactive-only and so is the
  fix.
- The cwd sandbox comment (`shell.rs:112`) currently oversells a "sandbox boundary" the commit
  side never honored — the fix makes the comment true rather than deleting it.
