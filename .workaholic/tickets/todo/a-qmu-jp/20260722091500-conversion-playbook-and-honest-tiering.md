---
created_at: 2026-07-22T09:15:00+09:00
author: a@qmu.jp
type: enhancement
layer: [Domain]
effort:
commit_hash:
category:
depends_on: [20260722091200-rule-the-semantic-gaps-in-blueprint-13.md]
mission: the-declared-driver-dsl-covers-the-compiled-drivers-concisely
---

# The conversion playbook exists and honest tiering is restated

## Overview

Mission acceptance items 5 and 6, one document each way:

1. **The conversion playbook** (in the blueprint or beside it, per the mission's Experience)
   names the four downstream twin-conversion missions in order — `slack` → `github` →
   `drive` → `mail` (ascending service-quirk difficulty; Gmail last because the MIME/batch
   rulings must exist first) — and for each records:
   - its **entry condition**: which §13 rulings it needs landed;
   - its **fixture/row-equivalence bar**: compiled stays until the declared twin is
     row-equivalent on shared fixtures;
   - its **retirement steps**: compiled deletion, gen-docs/gen-skills regeneration, plugin
     version bump per CLAUDE.md.
   The playbook states plainly that **none of the four starts before this mission's rulings
   land** — the downstream missions are named here, not created here.
2. **Honest tiering restated, not eroded**: `/git`, `/claude`, `/cf`'s queue pull and
   Artifacts (as far as still compiled when this ticket runs), the `/local`/`/s3`-class blob
   primitives, and the `/sql` engines are recorded as **named structural exceptions with
   reasons**, so "declared is the normal way" keeps its honest boundary and no silent
   exception rides the conversions.

## Policies

- The playbook is a gate document: it must be sufficient for a fresh session to start the
  slack conversion mission without re-deriving any ruling.
- No conversion work in this ticket — naming, ordering, and entry conditions only.
- The exception list must match reality at the time of writing (re-verify what is still
  compiled; the read-over-POST ticket may have moved the /cf line).

## Quality Gate

- The playbook exists with all four missions, each carrying entry condition, equivalence
  bar, and retirement steps, plus the explicit none-starts-early statement.
- The structural-exception list is complete against the compiled driver registry at HEAD and
  each entry carries its reason.
- `gen-docs --check` green; no generated file hand-edited.
