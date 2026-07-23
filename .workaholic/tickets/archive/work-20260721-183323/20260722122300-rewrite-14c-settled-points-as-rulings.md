---
created_at: 2026-07-22T12:23:00+09:00
author: a@qmu.jp
type: enhancement
layer: [Domain]
effort:
commit_hash:
category: Changed
depends_on: 20260722122100-fold-the-unmerged-14c-map-into-the-mission-branch.md
mission: a-walk-extends-one-trail-one-column-at-a-time
---

# Rewrite §14c's settled points (rulings 1–4) from open to ruled

## Overview

Mission acceptance item 3. In `docs/blueprint.md` §14c, rewrite the points the 2026-07-21
conversation settled so they read as **rulings**, not open questions, and remove the retracted
framing. The four rulings (from the mission's Goal):

1. **Column layout is a display pattern for post-execution semantics** — "display the semantics
   after a query is exercised, in columns." Kept simple; must not be over-abstracted. **Remove**
   the earlier "higher-abstracted container / design-pattern" framing wherever §14c states it —
   the owner explicitly RETRACTED it.
2. **Linear-vs-graph is dissolved by placement, not by choosing one.** The strip stays linear
   (columns are a linear walk); a define-time DAG (e.g. a React-Flow-like pipeline editor) lives
   INSIDE a single column, not across columns. Non-linearity is confined to a column's interior.
3. **Intension/extension is welded by path = query = set.** A path is simultaneously a query
   (intension; describe = schema/keys/relations) and resolves to a set (extension; read = rows);
   every prefix carries both aspects of ONE object. **Retain the caveat as open:** the unity is
   cleanest for reads; writes/effects reintroduce a distinct preview/commit aspect (§7).
4. **100% viewer/language parity is deliberately given up.** The viewer is a faithful
   representation of the subset it covers, not a lossy projection of the whole; fidelity is the
   CONTENT's responsibility (e.g. the DAG inside a column), not the container's.

## Policies

- Documentation only — §14c prose in `docs/blueprint.md`; no code, no grammar.
- Depends on #20260722122100 (§14c must be present to rewrite).
- The write-edge caveat under ruling 3 stays **open** — do not close it here; it belongs to the
  open list (#20260722122500).
- The retracted higher-abstraction framing is removed, not merely softened.

## Quality Gate

- §14c states rulings 1–4 as rulings (not open questions).
- The "higher-abstracted container/design-pattern" framing is absent from §14c.
- The intension/extension write-edge caveat is retained as an open item.
- The diff touches only `docs/blueprint.md` (and mission bookkeeping).
- `cargo run -p xtask -- gen-docs --check` still passes.
