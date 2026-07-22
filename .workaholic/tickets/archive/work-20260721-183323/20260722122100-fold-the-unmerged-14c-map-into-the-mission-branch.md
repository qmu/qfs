---
created_at: 2026-07-22T12:21:00+09:00
author: a@qmu.jp
type: enhancement
layer: [Domain]
effort:
commit_hash:
category: Changed
depends_on:
mission: a-walk-extends-one-trail-one-column-at-a-time
---

# Fold the unmerged §14c map into the mission branch

## Overview

Mission acceptance item 1 — the foundation every later ticket edits. §14c ("The viewer,
reconsidered — the design space (open; nothing settled)") was written earlier in the same
2026-07-21 design conversation and sits on branch `work-20260721-031401` at commit `d0218aa`,
**not on main**. It is checked out read-only at `.worktrees/viewer-reconsideration/`. Bring that
section into `docs/blueprint.md` on this mission branch so the later tickets rewrite it in place
rather than re-derive it.

- **Preferred:** cherry-pick `d0218aa` onto this mission branch. If it does not apply cleanly
  (blueprint moved on main since), rebase-and-tidy: reproduce §14c's content faithfully from the
  read-only worktree into `docs/blueprint.md` right after the shipped §14b.
- If `d0218aa` has already merged to main by drive time, `rebase-and-tidy` degenerates to
  "confirm §14c is present and tidy it"; do not duplicate it.
- **Never write** the `.worktrees/viewer-reconsideration/` checkout — it is a read-only source.
- Land it tidied but do not yet rewrite the rulings (tickets #20260722122300 / #20260722122400
  own that); this ticket only makes §14c present as the base.

## Policies

- Documentation only — no viewer code, no grammar, no generated reference docs hand-edited.
- qfs is experimental — where §14c's text is folded, it is recorded as-is for the later tickets
  to rule; no compatibility/deprecation framing is introduced.
- The `viewer-reconsideration` worktree is read-only and must not be modified.

## Quality Gate

- `docs/blueprint.md` on the mission branch carries a §14c section immediately after §14b.
- `git status` shows the `viewer-reconsideration` worktree untouched.
- The diff touches only `docs/blueprint.md` (and mission bookkeeping); no code file changes.
- `cargo run -p xtask -- gen-docs --check` still passes (a design-only change leaves the
  generated reference docs in sync).
