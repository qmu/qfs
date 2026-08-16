---
created_at: 2026-07-25T14:32:00+09:00
author: a@qmu.jp
assignees: [a@qmu.jp]
type: bugfix
layer: [Domain]
effort:
commit_hash:
category:
depends_on:
mission:
claim: work-20260816-181224
---

# `of` compares against the empty codec schema — the one fold this mission left strict

## Overview

This mission made a codec seam report `Schema::empty()` (commit `be3a05c`): a `decode`'s columns are
undescribable until the bytes are read, so `PlanSource::Codec` carries its own always-empty schema
instead of falsely reporting the pre-decode blob columns. Every downstream fold was made **lenient**
for that empty schema so it raises no substitute false claim — `select`, `where`, `expand`, and the
transform-input check all stay late-bound and let the runtime make the honest refusal over the
decoded batch.

`check_of_assertion` was not. `packages/qfs/crates/core/src/eval.rs` (~984, ~998) does
column-name-**set equality** between the asserted type and the relation's computed schema, with no
empty-schema leniency. So:

```
… |> decode md |> of article
```

compares `article`'s columns against an **empty** schema and reports every one of them as missing —
a plan-time `of_assertion_failed` whose content is false, fired before the decode that would have
produced exactly those columns. This is the same defect class the codec-schema ticket
(`20260717180300`) existed to remove, surviving in the one fold it did not touch.

## Scope

**In scope:** decide and implement how `|> of <type>` behaves when the computed schema is empty
(undescribable), consistent with the four folds already made lenient.

The two candidate answers, both defensible — this ticket must not assume the first:

- **Be lenient like the others** — skip the structural check on an empty schema and let the
  assertion ride to the next materialising boundary, where the rows (and their real columns) exist.
  This is §5.4's "honest split" applied to structure as well as refinement, and it is consistent
  with `select`/`where`/`expand`.
- **Refuse the combination explicitly** — a structured error saying `of` cannot be asserted directly
  after a codec because there is nothing to assert against, naming `decode` as the reason. Honest,
  but it removes a shape that reads natural (`decode md |> of article` is exactly the assertion an
  operator wants), so it must be a deliberate ruling, not a fallback.

What is **not** acceptable is the status quo: an assertion that fails at plan time listing every
asserted column as missing from a schema that is empty by design.

**Out of scope:**

- The blueprint §5.6 `of` semantics for a normal, described relation — unchanged either way.
- The refinement/membership half of `of` — already split honestly.
- Re-opening the empty-schema decision itself; `Schema::empty()` at the codec seam is settled.

## Key Files

- `packages/qfs/crates/core/src/eval.rs` — `check_of_assertion` (~984 onward): the
  `OfTarget::Inline` / named-type resolution and the name-set equality that has no empty-schema arm.
  The lenient folds this must match are in the same file.
- `packages/qfs/crates/core/src/eval.rs` (~103, ~171) — `PlanSource::Codec` and the arm that reports
  its always-empty schema.
- `packages/qfs/crates/core/src/eval/tests.rs` (~1596) —
  `a_stage_after_a_codec_does_not_get_refused_against_the_pre_decode_columns`, the test that pins the
  leniency for the other three stages; the `of` case belongs beside it either way.
- `docs/blueprint.md` (~374) — the §5.6 `of` paragraph, corrected in this run to say the codec seam
  is the one place plan time cannot honestly prove anything. Whichever way this lands, that
  paragraph must be updated to match.

## Policies

- `workaholic:design` — 「推測するな、宣言して拒否せよ」: an error naming columns as missing from a
  schema that is empty by construction is a guess dressed as a refusal.
- `workaholic:implementation` — the four folds and this one should state the same rule about an
  undescribable relation, in one place, rather than diverge silently.

## Quality Gate

1. The decision is recorded in the ticket outcome and the commit body, with its reason.
2. A both-directions test: `… |> decode <fmt> |> of <type>` behaves as decided (green, or refused
   with the new structured reason), while `of` over a **described** relation still refuses a genuine
   mismatch naming the differing columns — the leniency is scoped to undescribable, not global.
3. `docs/blueprint.md` §5.6 agrees with the binary.
4. Workspace gates green with raw exit codes.

## Considerations

- Minted by the `/monitor` drive of this mission (run 20260725-101714) from the release-readiness
  pass. It is a real inconsistency introduced by this mission's own work, not pre-existing: before
  `be3a05c` the codec seam reported the (false) input schema, so `of` compared against something
  non-empty.

## Queue provenance — the `mission:` stamp was cleared on 2026-08-12

This ticket was minted under the mission **`a-where-predicate-is-honored-or-refused-never-dropped`**, which closed `achieved` while the ticket
itself stayed unfinished. `plan-units.sh` excludes any mission-stamped ticket from the developer's
backlog **without checking whether that mission is still active** (`plan-units.sh:432` — a non-empty
mission relation is excluded as `mission_member`), and only *active* missions are offered as mission
units. A ticket stamped with a closed mission is therefore reachable by neither path, and this one
had been invisible to every `/drive` survey since the close.

The stamp is cleared so the ticket returns to the ordinary backlog — the same correction
`20260804173000` received when its own mission closed. The provenance lives here in prose instead.

**Still-open evidence (verified 2026-08-12, read-only):** Still open: `check_of_assertion` in `crates/core/src/eval.rs` carries no empty-schema leniency, so a `|> decode md |> of article` still compares against `Schema::empty()`.
