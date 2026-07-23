---
created_at: 2026-07-22T09:11:00+09:00
author: a@qmu.jp
type: enhancement
layer: [Domain]
effort:
commit_hash:
category: Added
depends_on:
mission: the-declared-driver-dsl-covers-the-compiled-drivers-concisely
---

# The coverage inventory of the compiled driver surfaces, disposed and evidenced

## Overview

Mission acceptance item 1. Enumerate **every node, verb, and `CALL` procedure** the compiled
slack, github, gdrive, and gmail drivers expose — from the compiled describe registry (the
`gen-docs` source, which is connection-free by design), never from prose — and dispose each
surface into exactly one of:

- **expressible today** — with the working declaration snippet as evidence (parse-checked at
  minimum; the snippet must be a declaration the current binary accepts);
- **needs a ruled semantic** — with the concrete missing primitive named (read-over-POST,
  declared pushdown, MIME assembly, batch/subrequest fan-out, …) and what the surface needs
  from it;
- **named park** — with why waiting is honest (push/watch channels, GraphQL, websockets stay
  parks unless found load-bearing for a coverage-bar surface).

The deliverable is an inventory document in the mission directory
(`.workaholic/missions/active/the-declared-driver-dsl-covers-the-compiled-drivers-concisely/`),
one row per surface, machine-checkable in structure (a table or front-matter-parsed list), so
the downstream rulings ticket can consume it without re-enumeration.

Known gaps going in are listed in the mission's Goal — verify, extend, and price each; none is
pre-solved.

## Policies

- The registry is the source of truth: if the inventory and `docs/drivers.md` disagree, read
  the registry code, not the rendered page.
- Do not silently drop a surface: every node/verb/CALL of the four drivers appears with a
  disposition. Silent truncation reads as coverage and is the failure mode this ticket exists
  to prevent.
- Structural exceptions (/git, /claude, queue-pull/Artifacts, blob primitives, /sql engines)
  are out of the coverage bar but are NOT silently skipped — they are restated by the playbook
  ticket; this inventory only covers slack/github/gdrive/gmail.

## Quality Gate

- The inventory document exists, covers all four drivers' complete describe-registry surface
  (spot-checkable by counting registry entries vs inventory rows), and every row carries a
  disposition with its evidence.
- Every "expressible today" snippet parses against the current binary (verified in a scratch
  run or a test; note the method in the document).
- `cargo test --workspace` untouched and green; no production code changes expected.
