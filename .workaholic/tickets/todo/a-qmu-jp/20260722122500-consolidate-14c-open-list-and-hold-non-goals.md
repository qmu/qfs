---
created_at: 2026-07-22T12:25:00+09:00
author: a@qmu.jp
type: enhancement
layer: [Domain]
effort:
commit_hash:
category:
depends_on: 20260722122200-define-trail-and-walk-in-14b.md, 20260722122300-rewrite-14c-settled-points-as-rulings.md, 20260722122400-record-the-ai-letter-rulings.md
mission: a-walk-extends-one-trail-one-column-at-a-time
---

# Consolidate §14c's open list (naming downstream missions) and hold the non-goals

## Overview

Mission acceptance items 5 and 6 — the closing ticket. Two deliverables:

1. **Consolidate §14c's open list.** After the settled points became rulings (#20260722122300 /
   #20260722122400), the genuinely-open items must remain listed as open, each attaching its
   named-but-not-created downstream mission. The list keeps at least:
   - the ASK-grammar / predicate- and merge-column spellings (candidates, unsettled);
   - the split primitive + in-column DAG editor;
   - the request-principal seam / empty-home root;
   - the enumerate-root plumbing;
   - the per-viewport projections;
   - the intension/extension write edge (the caveat retained from ruling 3).
   Attach each open item's downstream mission by name as recorded in the mission's ## Scope.
   **Create none of those missions here.**

2. **Hold the non-goals (acceptance item 6).** Verify, before the mission branch reaches
   `/report`, that the whole diff touches only design/knowledge documents: no grammar shipped, no
   viewer code changed, no ASK/split implemented. This is the mission's closing check.

## Policies

- Documentation only — blueprint prose plus mission bookkeeping; nothing else.
- Depends on the three content tickets landing first (terms defined, rulings written).
- Downstream missions are **named, not created** — creating one is out of scope and would violate
  the mission's non-goals.

## Quality Gate

- §14c's consolidated open list contains at least the six items above, each naming its downstream
  mission; none of those missions is created by this ticket.
- The mission branch's cumulative diff (via the shipped PR's file list) touches only
  design/knowledge documents — `docs/blueprint.md` and `.workaholic/` bookkeeping — with no
  `.rs`, grammar, or viewer-code changes.
- `cargo run -p xtask -- gen-docs --check` passes.
