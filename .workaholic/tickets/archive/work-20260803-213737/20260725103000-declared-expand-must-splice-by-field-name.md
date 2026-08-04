---
created_at: 2026-07-25T10:30:00+09:00
author: a@qmu.jp
type: bugfix
layer: [Domain]
effort: 2h
commit_hash:
category: Changed
depends_on:
mission: a-declared-write-resolves-a-name-the-way-a-query-does
---

# Declared EXPAND must splice by field NAME, and empty text must fold like the DTOs

## Overview

Found while proving the declared Slack twin row-equivalent to `driver-slack`
(ticket 20260724014000). Two tier-2 evaluator behaviours force the equivalence fixtures to be
*homogeneous and fully populated*, which is not what a real service returns. Both are declared-side
correctness gaps, not test inconvenience.

**1. `EXPAND` splices a struct's values POSITIONALLY.** `qfs_engine::eval::expand` computes the
output schema once (from the first row / the column's declared type) and then, per element, calls
`expand_item` → `fields.into_values()` — an ordered value list with no reference to the field names.
A JSON array whose elements have DIFFERENT key sets (exactly what Slack returns: `thread_ts` and
`subtype` are present only on threaded/subtyped messages) therefore shifts every later column. In
the fixture that surfaced it, a two-message array where the second message omitted two optional keys
delivered `[Bool(true), Null, Text("2"), Text("U2"), Text("yo")]` — the envelope's own `ok` value
had slid into the `ts` column. The compiled driver is immune because its DTO decode reads each field
BY NAME.

This is a silent wrong-rows defect, not an error: a declared driver over any real optional-field API
delivers scrambled columns with no diagnostic.

**2. Empty text does not fold to `Null`.** The compiled DTOs map an empty string to `Value::Null`
(`MessageDto` → `Row::from`), so a compiled read of `"subtype": ""` yields `Null`. The declared `OF`
shaping delivers `Text("")`. Both are defensible in isolation, but they are not the same row, so the
twin's equivalence bar cannot cover the empty-value case until one rule is chosen.

## Policies

- workaholic:implementation / honest-surfaces — a declared read that silently mis-assigns columns is
  the sharpest possible violation: the delivered rows do not match the advertised contract, and
  nothing says so.
- Blueprint §13 tier-2 ("a declared view IS its stored query") — the fix belongs in the shared engine
  operator, not in a per-driver workaround, or every future twin re-hits it.
- experimental-no-backward-compat — changing `EXPAND`'s splice rule is a sanctioned hard break.

## Quality Gate

1. `EXPAND <field>` over an array of structs with DIFFERING key sets delivers each element's values
   under the CORRECT column names (a field absent from an element is `Null`), proven by a test whose
   fixture is deliberately ragged.
2. The output schema is the UNION of the elements' fields (or the declared `OF` type's columns where
   one is declared), in a deterministic order.
3. A ruling is recorded for empty-text folding — either the declared `OF` shaping folds `""` to
   `Null` like the DTOs, or the compiled DTO rule is retired — and the declared Slack twin's
   equivalence fixtures are relaxed to include an element with omitted optional keys and an empty
   string, with both sides still equal.
4. Workspace gates green: `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`,
   `cargo fmt --all --check`.

## Considerations

- The equivalence tests in `crates/qfs/src/declared_driver.rs` (`slack_twin_*`) currently carry a
  comment naming this ticket and pinning the fully-populated fixtures; relax them here.
- `crates/engine/src/eval.rs::expand` + `Schema::expand` are the two places the positional
  assumption lives.

## Final Report

Both halves landed, and the second one is a recorded ruling rather than a code change on both sides.

**1. `EXPAND` splices by field name.** `qfs_engine::eval::expand` now derives the replacement column
names once and reads each element's fields by name, so a key an element omits is `Null` in that
element's row instead of shifting every later column. Two sources for those names: the declared
element type where the column has one, and — for a late-bound (`Unknown`) column, which is what a
`DECODE json` body produces — the union of the field names the rows actually carry, in first-seen
order. A non-struct value where struct columns were expected now widens to the right number of
`Null`s, which the old code got wrong in a way that produced rows narrower than their own schema.

**2. Empty text: the declaration is right, and the compiled side cannot follow it.** An omitted key
is `Null`; a key present with `""` is `Text("")`. These are different facts on the wire and the
surviving side must not conflate them. The compiled `MessageDto` cannot express the distinction at
all — its fields are `String`, so the empty value doubles as the absent sentinel — and making it
express it means changing the DTOs to `Option<String>` and re-cutting every golden, in a crate this
same mission deletes two tickets later (20260724014200). So the shared equivalence fixture was
relaxed to the RAGGED case both sides can agree on (omitted optional keys → `Null` on both), and the
present-but-empty case is pinned declared-side in
`declared_of_shaping_keeps_an_empty_string_distinct_from_an_absent_field`.

### Discovered Insights

- **Insight**: The shared Slack equivalence fixture was homogeneous and fully populated not by
  convenience but because the positional splice made anything else fail — the fixture's own comment
  said so and named this ticket. Relaxing it was therefore the actual proof the fix works end to
  end, through the real tier-2 evaluator against the compiled driver, and it is worth more than the
  unit test.
  **Context**: When a test fixture carries a comment explaining what it cannot contain, that comment
  is a defect report. The fixture is the bar, and a bar written down to what the code can currently
  do stops measuring anything.

- **Insight**: `EXPAND` over a late-bound column had no notion of a replacement schema at all — it
  kept the input schema and spliced values in, so a struct element silently produced rows wider than
  the schema. The union-of-observed-fields path is new behavior, not a repair of existing behavior.
  **Context**: Every declared driver reads through `DECODE json`, which produces exactly this
  late-bound shape, so this path — not the declared-element-type one — is what a declared view
  actually exercises.
