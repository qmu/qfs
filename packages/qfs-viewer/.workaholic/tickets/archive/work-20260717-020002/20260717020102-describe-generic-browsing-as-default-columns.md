---
created_at: 2026-07-17T02:01:02+09:00
author: a@qmu.jp
type: enhancement
layer: [Domain, UX]
effort: 4h
commit_hash:
category: Added
depends_on: [20260717020101-qfs-connection-seam-with-swappable-issuance-forms.md]
mission: qfs-viewer-mvp
---

# describe generic browsing: any qfs path lowers to a default column view

## Overview

Mission acceptance item 2 (demo leg 3), the plan's step 1 (汎用ロワリング).
Any path qfs can `describe` renders as a default column view with NO
per-resource code: the describe schema names the columns, a read
(`<path> |> limit N`) supplies the rows, and a row that names a contained
child path is a click that appends a segment to the trail. Deterministic and
thin — one generator function from (describe, rows) to view data, kept as an
explicit seam that later manifest generators sit beside (the mission's
配管は一本 invariant).

## Policies

- `workaholic:design` / `policies/sacrificial-architecture.md` — the
  lowering is ONE deterministic generator feeding the same view pipeline
  richer manifests will feed; keep it a named function, not a special case
  wired into the renderer.
- `workaholic:design` / `policies/modeless-design.md` — the trail (and only
  the trail) holds the navigable state; a qfs column is a pure function of
  the URL, reproduced on revisit.
- `workaholic:implementation` / `policies/type-driven-design.md` — qfs
  paths, describe answers and rows are untrusted boundary input read through
  validating casters; the path charset is what makes statement embedding
  injection-proof.
- `workaholic:safety` / `policies/information-security-basic.md` — generic
  browsing is read-only by construction (describe is pure; `run` is never
  passed `--commit`), and the capability stays an argument the composition
  root grants.

## Scope honesty

This ticket renders the default view with the EXISTING plgg-view column UI.
Re-rendering it through the plggmatic engine's strip is the strip-UI ticket's
work (with the ADR 0002 amendment); the lowering function this ticket lands
is the input that work consumes. Navigation is CONTAINMENT ONLY — row
selection (`@selection`) rides the /resolve ticket, and its grammar is a
strategy-owned open question this repo must not pre-empt.

## Implementation

1. `src/domain/model/Describe.ts`: `asQfsPath` (absolute, closed charset, no
   dot-segments), `asResourceDescribe` (path, archetype, columns from
   `describe --json`), and the containment-link rule: a row links when its
   `path` column extends the current path by one-or-more segments.
2. `Trail.ts`: a third `Stop` variant `Qfs{path}` with URL prefix `qfs:` —
   same skip-and-continue parsing as the other two.
3. `columns.ts`: a qfs column — describe header (path, archetype), row table,
   containment links appending `qfs:` stops; qfs's own error words on
   failure. The corpus column gains a small GET form for entering a qfs path.
4. `api.ts`: `GET /qfs?path=…&cols=…` validates and 303-redirects to the
   trail URL with the qfs stop appended (forms cannot write `?cols=`
   themselves).
5. `serve.ts` grants the runner unconditionally: the mission's Experience
   (items 1 and 5) makes every describable path browsable on the local serve
   surface with zero config; declared `resources` remain the curated list on
   the root column.

## Quality Gate

- Acceptance: with a qfs binary on PATH, `/?cols=qfs:/local/<abs-dir>`
  renders that directory as a column and clicking a child row appends a
  column; revisiting the URL reproduces the columns; no per-archetype code
  anywhere in the change.
- Verification: unit specs against a fake runner (describe + rows), a codec
  round-trip spec for the `qfs:` trail segment, and a live `curl` against a
  served real directory.
- Gate: `./scripts/check-all.sh` exits 0.
