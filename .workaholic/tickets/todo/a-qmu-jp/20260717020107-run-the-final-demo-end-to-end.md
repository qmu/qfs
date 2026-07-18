---
created_at: 2026-07-17T02:01:07+09:00
author: a@qmu.jp
type: enhancement
layer: [UX, Infrastructure]
effort: 1h
commit_hash:
category: Added
depends_on: [20260717020105-markdown-browsing-over-the-qfs-collection-path.md, 20260717020106-keep-npx-distribution-true-through-the-ui-replacement.md]
mission: qfs-viewer-mvp
---

# The final demo, end to end, at the qmu/strategy root

## Overview

Mission acceptance item 7 (all four legs, run as one): at the root of
`qmu/strategy`, `npx qfs-viewer` starts the viewer → the markdown under
`docs/` browses as horizontal column strips → a connected qfs resource
browses via the generic describe view → a `/resolve` address copied and
revisited reproduces the same columns. Each leg already has its own item;
this ticket proves they compose, records the run (commands, addresses,
observed columns) in the mission changelog, and closes the mission if
everything holds.

## Policies

- `workaholic:planning` / `policies/verify-before-building.md` — the demo is
  run with real components at the real target repository, not simulated.
- `workaholic:operation` / `policies/observability.md` — the run's evidence
  (structured logs, the exact addresses) is captured, so "it worked" is
  checkable later.

## Quality Gate

- Acceptance: all four legs pass in one session at the qmu/strategy root;
  every mission acceptance box is ticked with its ticket marker.
- Verification: the recorded transcript of the run (addresses + curl
  outputs) appended to the mission changelog.
- Gate: `./scripts/check-all.sh` exits 0, and the mission's Definition of
  done reads true.
