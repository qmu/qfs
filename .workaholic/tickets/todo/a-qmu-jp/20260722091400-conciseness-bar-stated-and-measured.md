---
created_at: 2026-07-22T09:14:00+09:00
author: a@qmu.jp
type: enhancement
layer: [Domain]
effort:
commit_hash:
category:
depends_on: [20260722091100-coverage-inventory-of-compiled-driver-surfaces.md, 20260722091200-rule-the-semantic-gaps-in-blueprint-13.md]
mission: the-declared-driver-dsl-covers-the-compiled-drivers-concisely
---

# The conciseness bar is stated and measured

## Overview

Mission acceptance item 4. Conciseness is a measured property, not a vibe:

1. **State the bar in the blueprint**: a tier-1/tier-2 REST service ≈ one screen of
   statements, with `chatwork.qfs` (~30 statement lines for a full tier-1 service including
   file transfer) as the calibration point.
2. **Measure the inventory's "expressible today" dispositions against it** — statement-line
   counts per surface family, recorded in the inventory document.
3. **Terseness devices**: for each device adopted (driver-level defaults with per-view
   override, shared pipeline fragments via §5.9 pipeline-valued lambdas, declared prelude
   aliases, OF-type inference/shorthand), show a **before/after on a real declaration**. A
   device is adopted only if its ruling landed (depends_on) and its after is shorter without
   hiding the contract; a device that saves nothing measurable is rejected in writing.

Implementation of a device is in scope where its ruling landed and its cost is modest;
otherwise the device's ruling records it as future work with its measured expected saving.

## Policies

- A device that makes declarations longer than the compiled driver's docs is the wrong
  device — reject it, do not tune it.
- Measurements go next to the dispositions in the inventory document so a later conversion
  mission reads bar and evidence in one place.
- Hermetic only; any implemented device gets the same test/clippy/fmt/gen-docs gates as any
  grammar change.

## Quality Gate

- The blueprint states the bar with the chatwork calibration.
- Every "expressible today" family has its measurement recorded.
- Every adopted device shows a real before/after; every rejected device has its written
  reason.
- Full workspace gates green if any code changed; docs regenerated, never hand-edited.
