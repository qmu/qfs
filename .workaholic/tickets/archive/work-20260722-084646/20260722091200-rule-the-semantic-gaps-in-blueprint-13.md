---
created_at: 2026-07-22T09:12:00+09:00
author: a@qmu.jp
type: enhancement
layer: [Domain]
effort:
commit_hash:
category: Changed
depends_on: [20260722091100-coverage-inventory-of-compiled-driver-surfaces.md]
mission: the-declared-driver-dsl-covers-the-compiled-drivers-concisely
---

# Every "needs a ruled semantic" gap gets its ruling in blueprint §13

## Overview

Mission acceptance item 2. For each gap the inventory disposed as *needs a ruled semantic*,
record the ruling in blueprint §13 — stated as a **redefinition** (hard breaks sanctioned, no
migration or deprecation framing), each with its **declaration-cost note** (what the semantic
adds to a declaration's length; a device that makes declarations longer than the compiled
driver's docs is the wrong device).

Ruled explicitly, by name (they gate the slack/github conversions and full /cf retirement):

- **read-over-POST** — a read whose wire shape is a POST (queue pull, GraphQL-shaped search,
  body-carried queries). Rule the declared spelling.
- **declared pushdown** — which predicates/params push to the wire as query/body parameters
  (Slack oldest/latest, Gmail q=, GitHub list filters), with residual honesty surviving the
  declaration.
- **MIME/body assembly** (Gmail send/draft: RFC 2822 + base64url multipart) — codec, prelude
  function, or named park: rule it.
- **batch and subrequest shapes** (Gmail batch; per-row fan-out beyond bytes-oriented FOLLOW)
  — rule or park with reason.
- **the non-REST arm** — whether the declared shape grows one; rule it or park it with a
  reason (this answers what the declared-drivers mission left as "reopens if the shape grows
  one").

Owner authorization (2026-07-22): **autonomous recording is authorized** — the driving session
records these as real rulings in §13, per the mission's "the driving session executes or
overturns with cause" framing; no draft-and-wait step.

## Policies

- Redefinition, not migration: no compatibility shims, no deprecation periods, no
  backward-compat framing anywhere in the rulings.
- Consume the sibling mission's codec rulings (per-row decode, multi-relation codecs) — a
  declared driver decoding a collected response set rides the same rule; do NOT fork those
  semantics here.
- Every ruling carries its declaration-cost note; conciseness is a measured property.
- Rulings land in blueprint §13 text itself, not a side document.

## Quality Gate

- Blueprint §13 contains a ruling (or an explicit named park with reason) for every gap the
  inventory disposed as needing one — cross-checkable one-to-one against the inventory.
- read-over-POST and declared pushdown have explicit ruled spellings; the non-REST-arm
  question is ruled or parked in writing.
- Each ruling includes its declaration-cost note.
- `gen-docs --check` stays green (blueprint is hand-authored; generated docs untouched).
