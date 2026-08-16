---
created_at: 2026-07-25T14:32:00+09:00
status: done
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

## Final Report

Development completed as planned.

### The decision, and why (Quality Gate item 1)

The ticket names two defensible answers and forbids assuming the first. **Lenient** is implemented.
The reason is not preference but consistency evidence already in the tree: four sibling folds —
`select`, `where`, `expand`, and the transform-input membership check — were all made lenient over
an empty schema by ticket `20260717180300`, and this ticket's own `## Policies` section requires
that "the four folds and this one should state the same rule about an undescribable relation, in
one place, rather than diverge silently". `check_of_assertion` was the one that was missed, not the
one that was ruled differently.

The second option — refusing `decode |> of` explicitly — is the one the ticket says "must be a
deliberate ruling, not a fallback". An unattended run does not hold that ruling: it removes a shape
the ticket itself calls exactly what an operator wants. So the choice available here was between
matching what already shipped and inventing a new refusal, and only the first is defensible without
a developer. **Recorded as a deferred decision available to reverse**: if the developer prefers the
explicit refusal, this branch is the one place to change and the test below pins both directions.

**Scope kept narrow deliberately:** the leniency is applied *after* type resolution, so an
unresolvable name is still `of_type_unresolved`. That error does not depend on the incoming schema,
and letting an empty schema suppress it would have traded one false claim for another.

### Verification (Quality Gate item 2 — both directions, measured)

Pre-fix, with the new empty-schema arm reverted and the binary rebuilt:

```
$ qfs run "/local/tmp/qfsdemo/a.json |> decode json |> of (id int, title text)"
{"error":{"code":"of_assertion_failed","kind":"usage","message":"OfAssertionFailed { ty: \"(inline)\", missing: [\"id\", \"title\"], unexpected: [], mismatched: [] }"}}
EXIT=2
```

Every asserted column reported missing from a schema that is empty by construction — the false
claim, reproduced.

Post-fix, same command:

```
$ qfs run "/local/tmp/qfsdemo/a.json |> decode json |> of (id int, title text)"
{"schema":[{"name":"path","type":"text"},{"name":"id","type":"int"},{"name":"title","type":"text"}],"rows":[{"path":"/local/tmp/qfsdemo/a.json","id":1,"title":"x"}],...}
EXIT=0
```

And the control — `of` over a **described** relation still refuses a genuine mismatch, so the
leniency is scoped to undescribable rather than switched on globally:

```
$ qfs run "/local/tmp/qfsdemo |> of (nope text)"
{"error":{"code":"of_assertion_failed","kind":"usage","message":"OfAssertionFailed { ty: \"(inline)\", missing: [\"nope\"], unexpected: [\"name\", \"path\", \"size\", \"modified\", \"is_dir\", \"mode\", \"content\"], mismatched: [] }"}}
EXIT=2
```

`of_after_a_codec_is_late_bound_but_still_resolves_its_type_name` pins all four facts (named
lenient, inline lenient, unresolved name still refused, described relation still refused) beside the
existing `a_stage_after_a_codec_does_not_get_refused_against_the_pre_decode_columns`, and it was
confirmed to **fail** with the arm reverted — so it is a regression test, not a tautology.

Gate: `cargo test --workspace` 2720 passed / 0 failed, `cargo clippy --workspace --all-targets --
-D warnings` `CLIPPY=0`, `cargo fmt --all --check` `FMT=0`, `gen-docs --check` /
`gen-skills --check` / `check-migrations` all exit 0. Blueprint §5.6 updated to say the structural
half stays late-bound at a codec seam (Quality Gate item 3).

### Discovered Insights

- **Insight**: The five folds are lenient for the same reason but say so in five places. Four carry
  a hand-written `if …columns.is_empty()` guard with its own comment; the fifth was missed for a
  month precisely because nothing links them.
  **Context**: The ticket's own Policies section asks for "one place". A shared
  `Schema::is_undescribable()` predicate — or a single guard at the fold dispatcher — would make the
  next fold's omission a compile-time question rather than a review-time one. Out of scope here (the
  ticket scopes the `of` arm), but it is the durable fix.
- **Insight**: `of_assertion_failed`'s message is the Rust `Debug` form of the error struct
  (`OfAssertionFailed { ty: "(inline)", missing: [...] }`), not prose.
  **Context**: The same defect class this run minted `20260816175149` for on the post-decode path.
  That ticket's acceptance criterion — "no error message reaching an operator contains Rust `Debug`
  struct syntax" — already covers this site, so it is recorded here rather than re-ticketed.
