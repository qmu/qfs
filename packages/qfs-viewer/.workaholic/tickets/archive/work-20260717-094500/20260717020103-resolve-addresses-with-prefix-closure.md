---
created_at: 2026-07-17T02:01:03+09:00
author: a@qmu.jp
type: enhancement
layer: [Domain, UX]
effort: 4h
commit_hash:
category: Added
depends_on: [20260717020102-describe-generic-browsing-as-default-columns.md]
mission: qfs-viewer-mvp
---

# /resolve addresses: prefix closure, click = segment append

## Overview

Mission acceptance item 4 (demo leg 4). The view's truth becomes a path under
`/resolve`: column i is the resolution of the address's prefix i (接頭辞閉包
— every prefix of a valid trail is a valid trail), a click is a segment
appended to the address, and pasting a `/resolve` address into a fresh
session reproduces the same columns. Display state (folding, sort,
highlights) is provably absent from the address.

## Policies

- `workaholic:design` / `policies/modeless-design.md` — the address IS the
  state; nothing navigable lives outside it, and display state never enters
  it (the plan's 境界 paragraph is explicit on this split).
- `workaholic:design` / `policies/sacrificial-architecture.md` — one lowering
  from trail to columns, in one place; `/resolve` and `?cols=` must not
  become two driftable serializations of the same idea.
- `workaholic:implementation` / `policies/type-driven-design.md` — the trail
  grammar is a closed vocabulary; segments the grammar does not know are
  skipped at the boundary, never guessed at.

## Constraints from the mission

- Containment segments and simple row selection ONLY. The `@selection`
  composite-key grammar and derived reverse-edge naming (`~projects`) are
  strategy-owned open questions — do not invent local answers.
- The existing `?cols=` trail is today's encoding of the same idea; this
  ticket decides whether `/resolve/<trail>` subsumes it or redirects to it,
  and records the decision.

## Quality Gate

- Acceptance: `GET /resolve/<trail>` renders one column per prefix; a click
  navigates to the same address plus one segment; a fresh session at a
  copied address reproduces the columns (modulo live row data).
- Verification: unit specs over the prefix-closure rendering; a spec
  asserting no display-state parameter participates in the address; a live
  paste-the-address check.
- Gate: `./scripts/check-all.sh` exits 0.
