---
type: Mission
title: A where predicate is honored or refused, never dropped
slug: a-where-predicate-is-honored-or-refused-never-dropped
status: active
created_at: 2026-07-24T01:10:44+09:00
author: a@qmu.jp
assignee: a@qmu.jp
strategy: integrations-are-declared-not-compiled
drive_authorized: true
predicted_hours:
actual_hours:
tickets: []
stories: []
concerns: []
gate_type:
gate_target:
gate_assert:
---

# A where predicate is honored or refused, never dropped

## Goal

**A query stage that cannot be honored must never be silently ignored.** On 2026-07-23 the
developer hit the sharpest form of this in production use: a `where id == '<fileId>'` on a
`/drive` folder listing returned the **complete unfiltered listing at exit 0** — the predicate
had no effect, and the answer read as "these rows matched". The same seam admits three sibling
defects already measured and ticketed: a `where` on an **unknown column** returns 0 rows at exit
0 (a typo and "nothing matched" are indistinguishable), `expand` **silently no-ops** on json and
unknown columns, and a codec source error names the **pre-decode** columns. All four share one
root: the engine lets a stage mean nothing while the query still answers.

This matters doubly under the `integrations-are-declared-not-compiled` strategy: the declared
driver model routes ever more surfaces through the one generic engine and its pushdown planner.
An engine that silently drops what a driver cannot push down poisons **every** declared driver at
once — and conversely, one honest seam fixes them all at once. Consumers are scripts branching on
`row_count` and agents in the describe→preview→commit loop; a wrong answer at exit 0 is consumed
as a right one, and an unfiltered listing over-discloses rows the caller never asked for.

## Scope

**Done when** every acceptance item below is ticked: no unpushed `where` predicate is ever
dropped (evaluated locally or refused with a structured error, across all drivers), an unknown
column in `where` is a structured error distinct from an empty result, `expand` refuses what it
cannot expand, codec errors name post-decode columns, and `describe`'s pushdown flags are honest.

**Out of scope — do not do these in passing:**

- **Drive id-based lookup (`/drive/by-id` or `files.get` pushdown) and corpus-wide search** —
  ruled to the **drive twin** mission (blueprint §13.3 #3, G4 machinery) by the developer,
  2026-07-24. This mission makes the gdrive listing honest (0 rows for an absent id, never the
  unfiltered listing); resolving a share-link id to a path is the twin's feature work.
- **New pushdown capabilities** — this mission makes the existing planner honest, it does not
  teach drivers to push more down.
- **The slack/github/drive/mail twin conversions** — separate missions per the playbook.

## Experience

1. **An unpushed predicate always filters or always refuses.** For any driver and any `where`
   shape the driver does not push down, the engine either evaluates the predicate locally over
   the listed rows or fails the query with a structured "unsupported predicate" error naming the
   predicate and the driver. `/drive/<folder> |> where id == '<absent>'` returns 0 rows — never
   the whole listing.
2. **A typo is not an empty result.** `where nosuchcol == 'x'` on any source returns a
   structured `unknown_column` error naming the column and the schema it was checked against —
   not `rows: []` at exit 0. "Nothing matched" and "malformed question" are distinguishable by
   exit code and error shape.
3. **`expand` refuses what it cannot expand** instead of passing rows through unchanged.
4. **A codec error names the columns the caller can see** (post-decode), so the message points at
   something actionable in the query the caller wrote.
5. **`describe` is truthful about pushdown**: the flags reported for a node match what the driver
   actually pushes down after the change; no flag advertises a pushdown the engine then works
   around.

## Acceptance

- [ ] Unpushed `where` predicates get a local Filter or a structured refusal at the planner seam, across all drivers; the gdrive listing defect is gone in both directions and `describe` pushdown flags are honest (#20260723020055-gdrive-where-pushdown-silent-drop.md)
- [ ] `where` on an unknown column returns a structured `unknown_column` error at non-zero exit, general across drivers, distinguishable from an empty result (#20260717180100-where-on-an-unknown-column-returns-zero-rows-at-exit-0.md)
- [ ] `expand` on a non-expandable or unknown column is a structured error, never a silent pass-through (#20260717180200-expand-silently-no-ops-on-json-and-unknown-columns.md)
- [ ] A codec source error names the post-decode columns the caller queried (#20260717180300-codec-source-error-names-the-pre-decode-columns.md)

## Changelog

- 2026-07-24 — mission created from the bare /mission planning session (developer-selected candidate; strategy gap after the DSL and file-collection missions closed) — mission.md
- 2026-07-24 — strategy linked — integrations-are-declared-not-compiled
- 2026-07-24 — ruling recorded: P2 id-lookup and P3 corpus search deferred to the drive twin mission (blueprint §13.3 #3); this mission is the engine-seam honesty fix only — 20260723020055-gdrive-where-pushdown-silent-drop.md
- 2026-07-24 — ticket adopted (restored from 7cd91ec sweep) — 20260717180100-where-on-an-unknown-column-returns-zero-rows-at-exit-0.md
- 2026-07-24 — ticket adopted (restored from 7cd91ec sweep) — 20260717180200-expand-silently-no-ops-on-json-and-unknown-columns.md
- 2026-07-24 — ticket adopted (restored from 7cd91ec sweep) — 20260717180300-codec-source-error-names-the-pre-decode-columns.md
- 2026-07-24 — ticket adopted (moved from the root queue, mission-stamped) — 20260723020055-gdrive-where-pushdown-silent-drop.md
- 2026-07-24 — drive_authorized stamped after the creation interrogation (rulings above; per-ticket Policies and Quality Gate pre-answered in the adopted tickets) — mission.md
