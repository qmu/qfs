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

## Blocker — npm min-release-age hold (recorded 2026-07-19, night /monitor)

The demo could **not** be run: the viewer cannot be built or installed
today because the developer's supply-chain policy (`~/.npmrc`
`min-release-age=7`, which npm enforces as `before=2026-07-12T02:34Z`)
excludes the two newest plgg-family releases the build/run requires.

- **Command:** `packages/qfs-viewer/scripts/npm-install.sh` (the same
  `npm install` `scripts/check-all.sh` and the npx smoke run).
- **First hard failure:** `npm error code ETARGET — No matching version
  found for plgg-bundle@^0.0.6 with a date before 2026/7/12`.
  `plgg-bundle 0.0.6` (plggmatic's build dep) was published
  **2026-07-13T02:38Z** → clears the 7-day hold ~**2026-07-20T02:38Z**.
- **Binding runtime failure:** `qfs-viewer` depends on `plggmatic ^0.2.0`
  — the UI engine the strip renders through, so it is required to *run*
  the viewer at all, not merely to build it. `plggmatic 0.2.0` was
  published **2026-07-17T02:26Z** → clears the hold ~**2026-07-24T02:26Z**.
  Only `0.1.0` (2026-07-04) is old enough, it does not satisfy `^0.2.0`,
  and its tarball is empty (mission changelog 2026-07-17).

Nothing was reusable: there is **no installed `node_modules` anywhere on
this machine** (not in the main checkout at
`/home/ec2-user/projects/qfs/packages/qfs-viewer`, no other worktree, no
global install, no packed `.tgz`, no prebuilt `dist/`), so the mission's
"reuse the main checkout's deps" fallback had nothing to copy. Pinning
stale versions or bypassing `min-release-age` is out of scope by the
mission's own Considerations ("route the request through HQ rather than
pinning stale versions"); the developer lifts the hold, not the drive.

**Action:** ticket left in `todo`, mission acceptance item 7 **not**
ticked. The final demo is runnable once the hold clears (binding date
**2026-07-24**, or earlier if the developer lifts it). No code change was
needed or made.
