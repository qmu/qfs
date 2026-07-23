---
created_at: 2026-07-22T12:24:00+09:00
author: a@qmu.jp
type: enhancement
layer: [Domain]
effort:
commit_hash:
category: Changed
depends_on: 20260722122100-fold-the-unmerged-14c-map-into-the-mission-branch.md
mission: a-walk-extends-one-trail-one-column-at-a-time
---

# Record the AI-letter rulings (5–8) in the blueprint

## Overview

Mission acceptance item 4. Record rulings 5–8 from the mission's Goal as design rulings in
`docs/blueprint.md` — a §14c subsection or a section §14c points to. The AI-letter concept rides
the **same column UI** as the rest of the viewer; its qfs mapping is ruled:

5. **Envelope = context + interactivity.** A letter is an envelope carrying context and its own
   interactivity, riding the same column strip.
6. **Inward confinement is the same principle as declared-driver host-confinement.** A letter's
   reach is confined inward, named explicitly as the same confinement principle a declared driver
   applies to its host — not a new mechanism.
7. **Single typed egress.** The only way out is a typed reply: a reply is a typed INSERT into the
   sender's inbox — one egress, typed, no side channel.
8. **Type-derived interactivity; form-filling is a walk.** Interactivity is derived from the type;
   free modality lands on the fixed type. Form-filling is a walk — a partial struct is a valid
   intermediate value, and the **effect happens only at the terminal column** (no I/O until
   COMMIT). A condition-split is a declared, checkable path-branch that keeps walks linear. And
   **"who drives is not the design axis"** — the same surface serves human and agent.

## Policies

- Documentation only — blueprint prose; no code, no grammar, no ASK/split implemented.
- Depends on #20260722122100 (§14c present as the anchor).
- These are recorded rulings; the *grammar spellings* they imply (ASK, split) stay open and go on
  the open list (#20260722122500), not implemented here.

## Quality Gate

- Rulings 5–8 appear as design rulings in §14c (or a subsection §14c points to), each with its
  pinned content (envelope, inward confinement = host-confinement, single typed egress = typed
  INSERT into the sender's inbox, type-derived interactivity, form-filling-as-a-walk with effect
  only at the terminal column, condition-split as a linear-preserving path-branch, "who drives is
  not the design axis").
- The diff touches only `docs/blueprint.md` (and mission bookkeeping).
- `cargo run -p xtask -- gen-docs --check` still passes.
