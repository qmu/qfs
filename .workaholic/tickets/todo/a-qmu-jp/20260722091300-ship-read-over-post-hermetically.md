---
created_at: 2026-07-22T09:13:00+09:00
author: a@qmu.jp
type: enhancement
layer: [Domain]
effort:
commit_hash:
category: Added
depends_on: [20260722091200-rule-the-semantic-gaps-in-blueprint-13.md]
mission: the-declared-driver-dsl-covers-the-compiled-drivers-concisely
---

# read-over-POST ships and is proven hermetically end-to-end

## Overview

Mission acceptance item 3, with the proof target fixed by the owner (2026-07-22):
**read-over-POST** — the sharpest known wall, gating the slack/github conversions and full
/cf retirement.

Implement the semantic exactly as ruled in blueprint §13 (depends_on), then prove it the way
`cloudflare.qfs`'s SQL arm was proven: a declared driver using the new semantic **installs,
describes, plans, and reads against hermetic wire fixtures through the real tier-2 evaluator**
— so the ruling is demonstrated, not speculative.

The natural proof body is the Cloudflare queue-pull surface (today the compiled `/cf`
holdout): if the ruled spelling can declare it, extend `cloudflare.qfs` (or a test-local
declaration) and cover it with fixture-backed tests. If the driving session finds a cleaner
proof body, it may choose one — the requirement is a real declared driver over hermetic wire
fixtures, not a unit test of the parser.

Full /cf retirement (deleting the compiled queue-pull) is NOT required by this ticket; if the
proof happens to make it mechanical, record that in the mission changelog for a follow-up
rather than widening this ticket.

## Policies

- Implement what §13 ruled; overturn only with cause recorded in the mission changelog and
  the §13 text corrected in the same change.
- Hermetic only: wire fixtures, no live Cloudflare (or any live service) traffic, no
  credentials.
- Structural host confinement holds: the declared driver's hosts stay confined at load time
  and plan time exactly as the existing declaration surface enforces.

## Quality Gate

- A declared driver using read-over-POST passes an end-to-end hermetic suite: install,
  DESCRIBE, plan, and read against wire fixtures through the real evaluator.
- The declaration snippet is concise — measured against the conciseness bar and noted in the
  mission directory (feeding the conciseness ticket).
- `cargo test --workspace`, clippy `-D warnings`, `cargo fmt --all --check`,
  `gen-docs --check` / `gen-skills --check` all pass (grammar surface changes regenerate,
  never hand-edit).
