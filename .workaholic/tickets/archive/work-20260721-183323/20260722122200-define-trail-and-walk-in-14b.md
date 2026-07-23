---
created_at: 2026-07-22T12:22:00+09:00
author: a@qmu.jp
type: enhancement
layer: [Domain]
effort:
commit_hash:
category: Changed
depends_on: 20260722122100-fold-the-unmerged-14c-map-into-the-mission-branch.md
mission: a-walk-extends-one-trail-one-column-at-a-time
---

# Define trail and walk as domain terms in §14b

## Overview

Mission acceptance item 2 — the core deliverable. Write the two domain-term definitions into
`docs/blueprint.md` §14b (which already carries the term *trail* loosely) and cross-reference them
from §14c. The definitions carry **all** pinned distinctions from the mission's Goal:

- **trail** — a NOUN, STATIC, a RESULT: one written path within the path concept — the canonical
  containment backbone plus segments beyond bare containment (selection `@A`, declared-relation
  `/client`, derived-reverse `~projects`); "where you have walked, recorded."
- **walk** — a VERB, DYNAMIC, an ACT: extending a trail one step — one column — at a time; **the
  walk produces the trail** (the trail is the walk's trace); **always linear** — a walk traverses
  exactly ONE trail, never a graph, and a DAG's non-linearity never appears in a walk.
- **The sharpened definition** (ruling 8): *"choose one of the steps the current trail admits, and
  extend"* — for reads the admitted steps are describe's declared relations and keys; for writes
  the next input type dependent on the values bound so far. **One definition covers both.**
- **The containment chain:** address/path (canonical backbone) ⊆ trail (backbone + relation
  segments); walk = the act that builds/traverses a trail.

## Policies

- Documentation only — §14b/§14c prose in `docs/blueprint.md`; no code, no grammar.
- Depends on #20260722122100 (§14c must be folded in before it can be cross-referenced).
- Definitions are recorded rulings, not open questions — state them, do not hedge.

## Quality Gate

- §14b defines both `trail` and `walk` with every distinction listed above (noun/verb,
  static/dynamic, result/act, linearity, address ⊆ trail, the reads+writes "admitted steps"
  definition).
- §14c cross-references the §14b definitions.
- The diff touches only `docs/blueprint.md` (and mission bookkeeping).
- `cargo run -p xtask -- gen-docs --check` still passes.
