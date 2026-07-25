---
created_at: 2026-07-25T10:30:00+09:00
author: a@qmu.jp
type: bugfix
layer: [Domain]
effort:
commit_hash:
category: Changed
depends_on:
mission: the-declared-slack-twin-retires-the-compiled-driver
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
