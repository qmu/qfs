---
created_at: 2026-07-14T22:02:13+09:00
author: a@qmu.jp
type: housekeeping
layer: [UX, Domain]
effort: 0.1h
commit_hash:
category: Changed
depends_on: []
mission: language-design-review-layering-principles-and-semantic-gaps
needs_design_brief: false
---

# RESUME — shell-face slices 2/3 (+ /type mount) and reporting the current branch

**This is a `/carry` resumption checkpoint, not a work item.** It captures where the
`language-design-review-layering-principles-and-semantic-gaps` mission stands so a fresh
`/drive` (or `/report`) continues without relying on compaction. Read this first, then act.

## Position (verify before acting)

- **Branch**: `work-20260714-111817`, HEAD `dbcd6a5`, **working tree clean**. Not merged to `main`.
- **This session shipped four commits on top of the branch** (all committed + archived, gates green):
  - `ba06534` — general mid-pipe `of <type>` assertion (20th `PipeOp`); qfs 0.0.65, plugin 0.11.5.
  - `62a67ec` — shell-face **design brief + owner ADOPT ruling** + three slice tickets filed.
  - `02c6a66` — shell-face **Slice 1**: `ls` entry-kind-typed + blueprint §9 record; qfs 0.0.66.
  - `dbcd6a5` — recorded the Slice 2 scope finding (below).
- The branch also carries prior, unmerged commits (`daa1d02` capability-tryout mission close;
  `4e98295` effect-selector channel) — a `/report`/`/ship` would bundle the whole branch.

## Mission status

Both of the mission's remaining acceptance items are now **closed**:
- ✅ General mid-pipe `of <type>` assertion — **implemented + shipped** (`#20260714154144`, archived).
- ✅ Shell face — **decided: adopt** (owner, 2026-07-14); design note is
  `.workaholic/missions/active/language-design-review-.../shell-face-design-brief.md`.

**The mission's design questions are exhausted.** What remains is implementation of the adopted
shell-face plan, already ticketed.

## First decision on resume

The owner paused implementation here (2026-07-14) and asked for slices 2/3 in a **fresh session**.
My standing recommendation: **run `/report` first** to turn this branch's shipped work (the `of`
assertion + the shell-face ruling + Slice 1) into a PR, THEN pick up the slices below. Confirm the
owner still wants that ordering before implementing.

## Remaining work — the shell-face slices (dependency order; all in `todo/a-qmu-jp/`)

1. **`20260714182740-shell-face-type-mount-and-describe-builtin.md`** — mount `/type` as a
   read-only catalog (so `ls /type` = SHOW TYPES) + a `describe` REPL builtin. Split off Slice 1;
   `depends_on` Slice 1 (done). A full read-only driver like `/transform` (~700-line template) +
   an `Outcome`/REPL-render change. ~4h.
2. **`20260714182720-shell-face-slice2-cd-gate-enumerable-children.md`** — the `cd` gate becomes
   "enumerable-children" instead of an archetype pair. **SCOPE GREW (verified against the binary):**
   `describe /transform` (a navigable catalog interior) reports the **same `relational_table`
   archetype** as a leaf table (`/sys/drivers`), so archetype cannot distinguish them — the fix
   needs a **per-node navigability signal**: add a field to the `NodeDesc` describe contract
   (`crates/driver/src/lib.rs`, `#[non_exhaustive]` — add a `navigable(bool)` builder defaulting
   from the archetype so blob/object-graph nodes are unchanged) and update each driver's `describe`
   to set it per path where the default is wrong (`/sql/<conn>` catalog vs its table leaves;
   `/transform`/`/type`/`/sys`/`/server` roots vs leaf rows; mail/slack label/channel trees). This
   is a **driver-contract change across many drivers** — re-estimated to ~4h. Also fix the gmail
   root describe (currently `AppendLog`, should be a navigable label tree).
3. **`20260714182730-shell-face-slice3-mutation-verbs-per-kind.md`** — `cp`/`mv`/`rm` per
   entry-kind ruling (behind the existing preview/commit gate; no new gate/machinery). `cp` verb by
   destination kind (blob→UPSERT, else INSERT; membership-checked into an `OF` table via the shipped
   `materialize_pipeline_source` seam); `mv` same-kind-only (blob rename / def rename / else refuse
   naming the honest spelling — the mail copy+delete = send trap); def-catalog clone/rename/drop;
   data-row → def-catalog `cp` is `category_error`; `mkdir` deferred. ~4h.

Not part of this mission: `20260714120000-effect-selector-uniform-migration.md` belongs to the
**already-achieved** `qfs-capability-tryout` mission (closed 13/13 by another session). Driving it
reopens scope the owner closed — leave it for an explicit owner decision.

## Environment / gate notes (save a rediscovery)

- `cargo` is not on the default PATH: `export PATH="$HOME/.cargo/bin:$PATH"`.
- Redirect the small tmpfs `/tmp`: `export TMPDIR=<session scratchpad>` for cargo runs.
- Build/test from repo root with `--manifest-path packages/qfs/Cargo.toml`. `cargo fmt --all` has NO
  commit-time gate — apply it before committing or the ship fmt gate fails.
- Full gate: `cargo test --workspace` (~1240+ tests), `clippy --workspace --all-targets -D warnings`
  (NOT `--all-features`), `fmt --all --check`, and `xtask gen-docs --check` / `gen-skills --check` /
  `check-migrations`. All were green at `dbcd6a5`.
- Interactive-shell e2e: `printf 'ls /transform\n' | qfs` drives the REPL (no `exit` builtin — it
  parses as a query, harmless).

## Final Report

**Checkpoint consumed 2026-07-15** — archived because every item it tracked is now closed, not
because it was "implemented" (it is a `/carry` checkpoint, not a work item). Leaving it in `todo`
would have made the next `/drive` read a stale map claiming work remains.

Its **first decision** was put to the owner and answered: *continue implementing the slices now, and
`/report` afterwards* (the branch is still unmerged and now carries eight commits). The
effect-selector caveat was also raised and overruled deliberately — the owner chose to implement
`20260714120000` despite it belonging to the already-achieved `qfs-capability-tryout` mission.

All four remaining tickets landed in this session, in the dependency order this checkpoint fixed:
- `c20b6c4` — `/type` catalog mount + `describe` REPL builtin (qfs 0.0.67, plugin 0.11.6)
- `fb664b5` — the enumerable-children `cd` gate (qfs 0.0.68)
- `fc99572` — `cp`/`mv` per-entry-kind ruling (qfs 0.0.69, plugin 0.11.7)
- `7b72cab` — effect-selector uniform lowering (qfs 0.0.70)

### Discovered Insights

- **Insight**: The checkpoint's environment notes paid for themselves immediately (cargo PATH, the
  `TMPDIR` redirect, the full gate list, the `printf … | qfs` REPL driver) — but the one hazard it
  did NOT record bit twice: `/` fills from the shared host's many worktree target dirs, and
  `cargo test --workspace` dies at link with "No space left on device". Reclaiming only this tree's
  `target/debug/incremental` freed 17G (100% → 90%).
  **Context**: Worth adding to any future resume checkpoint's environment section alongside the
  TMPDIR note — it presents as a mystifying link failure, not a disk error, and the safe fix is
  narrow (never touch another worktree's `target/`).
