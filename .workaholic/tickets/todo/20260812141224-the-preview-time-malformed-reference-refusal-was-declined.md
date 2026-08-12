---
created_at: 2026-08-12T14:12:24+09:00
author: a@qmu.jp
assignees: [a@qmu.jp]
depends_on:
mission:
merge_policy: review
---

# The blueprint promises a preview-time malformed-reference refusal the implementation deliberately declined

## Overview

Minted from the open concern `the-preview-time-malformed-reference-refusal` (feedback
`20260804212551-the-preview-time-malformed-reference-refusal.md`, severity moderate, raised out of
the G9 implementation and never turned into work until now).

Blueprint §13.1 **G9** — the rule that lets a declared write resolve a name against a collection —
states its timing as: *"COMMIT, in the confined applier, immediately before the effect leg; PREVIEW
additionally refuses a **malformed** reference with no I/O."*

The implementation declined that second clause, for a stated reason. "Malformed" presupposes a
**shape rule** — a way to tell a legal-but-unknown reference from an illegal one without asking the
service. The shipped resolution deliberately matches against **data** instead (`WHERE name ==
row.channel OR id == row.channel`), precisely so the engine never has to know that a Slack channel id
looks like `C0…`; that is what let the declared twin reproduce the compiled driver's behaviour
without importing its `C`/`G`/`D` prefix heuristic. With data-based matching there is no
malformed-versus-unknown line to draw at preview time without putting service-specific knowledge back
into a generic engine.

So the blueprint currently promises a refusal that does not exist, and the truthful property — that
PREVIEW performs **zero** I/O for a name-addressed CALL, and the resolution failure surfaces at
COMMIT before the effect leg fires — is asserted only in tests. A reader of §13.1 is misled about
what PREVIEW does.

This is a **developer ruling first**, implementation second.

## Scope

Close the gap between the ruling text and the shipped behaviour, in whichever direction the developer
rules.

**Option A — confirm the deviation (expected).** Correct §13.1 G9's timing clause to state what is
true: PREVIEW performs no I/O and therefore cannot resolve or reject a reference; an unresolvable or
ambiguous reference is a structured refusal at COMMIT, raised before the effect leg is constructed,
so no wire write is ever issued with a guessed id. Record *why* the malformed check was declined — a
shape rule is service-specific knowledge in a generic engine — so the clause is not re-proposed
blind. Cost: a blueprint edit; no code.

**Option B — define what a generic engine can check.** Say what preview can validate *without*
service knowledge, and implement exactly that. The honest candidates are all structural rather than
semantic: the referenced binding exists (a body naming an undeclared lookup), the lookup's source is
a declared view of this driver, the referenced `row.<field>` is a declared CALL parameter, an empty
or `Null` reference value. Note that the first three are declaration-time facts already checked at
extraction; only the last is per-row, and it is arguably the one worth surfacing early.

**Out of scope.** Making PREVIEW perform I/O to resolve names — §6 records that PREVIEW structurally
cannot reach the executor, and changing that is its own mission (already noted as deliberately not
taken in the G9 ruling).

## Key Files

- `docs/blueprint.md` — §13.1 G9, the timing bullet ("Time — COMMIT, in the confined applier…").
- `packages/qfs/crates/exec/src/declared.rs` — `map_body_lookups` (the pure, declaration-time shape
  and confinement checks) and `resolve_lookup` (the per-row match, and where zero/ambiguous matches
  are refused).
- `packages/qfs/crates/qfs/src/apply_facets.rs` — the confined applier that fetches the collection
  once per statement, immediately before the effect leg.
- `packages/qfs/crates/qfs/src/declared_driver.rs` — the twin tests that assert the true property
  (zero I/O at preview; refusal before the effect leg).

## Implementation Steps

1. Rule A or B and record it in the blueprint beside G9, with the rejected option and its reason.
2. If A: edit the G9 timing clause; add nothing to the code. Check whether any other document repeats
   the retired promise (search the docs for the malformed-reference wording) and correct those too.
3. If B: implement exactly the enumerated checks, each with a structured refusal naming what was
   wrong, and cover each with a hermetic test.
4. Either way, add or point to the test that pins the truthful property, so the document and the
   binary are checkable against each other rather than only consistent by intention.

## Policies

- `workaholic:implementation` / objective-documentation — a design document that promises a refusal
  the binary does not perform is worse than silence: it is the reference a reader trusts.
- `workaholic:design` / 「推測するな、宣言して拒否せよ」 — the reason the check was declined is itself
  an application of this rule (a shape heuristic would be the engine guessing at a service's id
  convention), and that reasoning belongs in the record.

## Quality Gate

1. **Acceptance:** §13.1 G9's timing statement and the shipped behaviour agree, and the rejected
   option is recorded with its reason.
2. **Acceptance:** if Option B is ruled, every check it names is implemented and refuses with a
   structured, secret-free error; if Option A, no behavioural change ships.
3. **Verification:** a test pins the truthful preview property (zero wire requests for a
   name-addressed CALL at preview), and — under B — one test per enumerated check.
4. **Gate:** `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`,
   `cargo fmt --all --check`, `gen-docs --check`, `gen-skills --check` all exit 0.

## Considerations

- Option A is the expected ruling, but it must be *chosen*: silently deleting the clause would lose
  the reason it was written, and the next author would re-propose the same check.
- The same tension will recur for every declared driver that resolves a name (drive's path→id, mail's
  label→id). Whatever is ruled here should be phrased generally enough to answer those too.
