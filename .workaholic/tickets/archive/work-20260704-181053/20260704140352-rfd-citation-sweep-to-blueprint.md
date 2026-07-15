---
created_at: 2026-07-04T14:03:52+09:00
author: a@qmu.jp
type: housekeeping
layer: [Config]
effort:
commit_hash: 6868f87
category: Changed
depends_on:
---

# Sweep RFD §-citations in crate docs to blueprint anchors, then delete RFD-0001

## Overview

The blueprint (`docs/blueprint.md`) superseded RFD-0001 and the ADR pile (2026-07-04, owner
directive: one living design document; git holds history). The ADR files are already deleted.
RFD-0001 remains **only** as a frozen citation anchor because crate doc-comments cite its section
numbers heavily (`RFD §3`, `RFD-0001 §5/§9/§10`, …). This ticket finishes the retirement:

1. Sweep every `RFD` citation in `packages/qfs/crates/**` doc-comments (and any remaining doc/
   README references) to the corresponding blueprint chapter anchor (`blueprint §3 The language`,
   `§6 Driver contract`, `§7 Runtime`, `§8 Authorization`, …). Purely mechanical: the mapping is
   RFD §1→bp §1, §2→§2, §3→§3, §4→§4, §5→§6, §6→§7, §7→§9, §8→§10, §9→§11, §10→§8.
   Also sweep the handful of `docs/adr/000N` citations in crate docs (e.g. `crates/host` "PARKED
   per docs/adr/0005", `crates/http` "see docs/adr/0004") to blueprint §10/§11 — those files are
   already deleted.
2. Delete `.workaholic/RFDs/0001-qfs-architecture.md` (its superseded banner already announces
   this) and the now-empty `RFDs/` directory if nothing else remains.

Mechanical Opus-class work; no design decisions. Do NOT reword the surrounding sentences —
replace only the citation tokens, so the diff stays reviewable.

## Policies

- `workaholic:implementation` / `policies/directory-structure.md` — conventional layout (doc hygiene)
- `workaholic:implementation` / `policies/coding-standards.md` — doc-comments are code surface
- `workaholic:implementation` / `policies/objective-documentation.md` — citations must point at a live document, not a deleted one

## Key Files

- `packages/qfs/crates/*/src/*.rs` - the RFD § citations in crate/module doc-comments (grep `RFD`)
- `.workaholic/RFDs/0001-qfs-architecture.md` - delete after the sweep
- `docs/blueprint.md` - the target anchors (verify each mapped section exists before rewriting)
- `README.md` - remaining `RFD-0001 §…` textual references

## Quality Gate

**Acceptance criteria:**

- `grep -rn "RFD" packages/qfs/crates --include='*.rs'` returns zero design citations (test
  fixture strings excepted only if a golden depends on them — then re-bless).
- `.workaholic/RFDs/0001-qfs-architecture.md` is deleted; no dangling links to it remain in
  README/docs.
- No functional change: docs-only diff (comments/markdown).

**Verification method:**

- `cd packages/qfs && cargo test --workspace` green (comments don't change behavior; goldens
  re-blessed if any embed the old citation), `clippy --workspace --all-targets -- -D warnings`,
  `gen-docs --check`, `gen-skills --check`.

**Gate:** the grep is clean, the workspace checks are green.

## Considerations

- Some tests may pin doc output containing `RFD` tokens (goldens); re-bless via `QFS_BLESS=1`
  rather than hand-editing fixtures
- `docs/language.md`/`drivers.md`/`server.md` are generated — if the generator emits RFD
  citations, fix the generator source, never the output (`packages/qfs/xtask`)
