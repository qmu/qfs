---
created_at: 2026-07-09T10:42:57+09:00
author: a@qmu.jp
type: enhancement
layer: [Domain]
effort: 3h
commit_hash:
category: Changed
depends_on: [20260709104254-blueprint-type-system-chapter.md]
mission: language-design-review-layering-principles-and-semantic-gaps
---

# Arithmetic operators — decide, and if adopted ship with precedence and freeze-test update

## Overview

Decide the deliberate question the mission raised: qfs has **no arithmetic operators**. `Op` is
comparison/logical/LIKE/regex only; `+ - * /` do not exist, and `*` lexes solely as the projection
star (`crates/lang/src/token.rs:80` `Star`, consumed at `crates/parser/src/grammar.rs:607` as
`Projection::Star`). So `extend total = price * qty` is unwritable while `SUM`/`DATE_DIFF`/`ABS`
exist — the gap real transform+join queries hit first (the mission's invoice example had to write
`unit_price <> list_price` where it wanted `abs(unit_price - list_price) > 5.0`).

Per the owner's quality-gate answer, this is a **decide-and-if-adopted-implement-in-one-ticket**
(project policy: no risk-splitting). The decision is grounded in the type-system chapter
(`depends_on`), because introducing `+ - * /` requires the numeric type rules — int/float
promotion, the result type of mixed operands, and division semantics — that the chapter defines.

If adopted, the implementation spans the full stack: lexer tokens, the frozen `Op` enum + the
`qfs-lang` operator-freeze test, parser precedence (multiplicative over additive over comparison),
the typeck result-typing rules, the eval arithmetic, and pushdown lowering (arithmetic in a
`where`/`extend` that targets a SQL backend should push down). If rejected, the reasoning is
recorded in the blueprint so the question is closed, not merely unimplemented.

## Policies

- `workaholic:implementation` / `policies/directory-structure.md` — tokens in `qfs-lang`, `Op` +
  freeze test in `qfs-lang`/`qfs-parser`, eval in `qfs-core`, lowering in `qfs-pushdown` — respect
  the spine.
- `workaholic:implementation` / `policies/coding-standards.md` — arithmetic eval must be total and
  return structured errors (overflow, divide-by-zero), never panic.
- `workaholic:implementation` / `policies/type-driven-design.md` — the result type of an
  arithmetic expression must follow the chapter's numeric rules; a mixed-type misuse is a
  plan-time type error, not a runtime surprise.
- `workaholic:implementation` / `policies/functional-programming.md` — operators are pure total
  functions over values; the expression layer's totality invariant is preserved (divide-by-zero
  is a structured error arm, keeping evaluation total).

## Key Files

- `packages/qfs/crates/lang/src/token.rs` - add arithmetic tokens (guarding the `*`
  projection-vs-multiply ambiguity by position).
- `packages/qfs/crates/parser/src/ast.rs` - extend `Op` (line ~570); this trips the operator
  freeze — intentional, MINOR per §12.
- `packages/qfs/crates/lang/src/*` - the operator-freeze count-lock test updated with the decision.
- `packages/qfs/crates/parser/src/grammar.rs` - precedence-aware expression parsing (multiplicative
  > additive > comparison); disambiguate `*` from `Projection::Star`.
- `packages/qfs/crates/core/src/typeck.rs` / `eval.rs` - result-typing + total arithmetic eval.
- `packages/qfs/crates/pushdown/src/lower.rs` - lower arithmetic to backend SQL where pushable.

## Related History

- [20260626103000-t70-operator-equals-binds-eqeq-compares.md](.workaholic/tickets/archive/work-20260628-000332/20260626103000-t70-operator-equals-binds-eqeq-compares.md) - the last deliberate operator decision (`=` binds, `==` compares); same freeze-test discipline
- [20260622214650-t08-stdlib-and-driver-preludes.md](.workaholic/tickets/archive/work-20260622-230954/20260622214650-t08-stdlib-and-driver-preludes.md) - the `ABS`/`ROUND` numeric builtins that exist while operators do not

## Implementation Steps

1. **Decide** (record in the blueprint): adopt `+ - * /` (and `%`?) or reject with reasoning. If
   rejected, stop after the blueprint note and the mission box.
2. If adopted: add tokens; extend `Op`; update the operator-freeze count-lock test with the new
   count and a comment citing this ticket.
3. Add precedence-aware parsing, disambiguating `*` from the projection star by grammar position.
4. Add typeck result-typing per the chapter's numeric rules and total eval (structured
   overflow/divide-by-zero errors).
5. Add pushdown lowering for arithmetic in pushable stages; add a cookbook recipe exercising
   `extend total = price * qty` (verified-true ratchet).
6. Full suite; tick the mission's arithmetic acceptance box.

## Quality Gate

**Acceptance criteria (if adopted):**
- `extend total = price * qty` and `abs(unit_price - list_price) > 5.0` parse, type-check, and
  evaluate correctly; precedence is `* /` over `+ -` over comparison.
- Divide-by-zero and overflow are structured errors, not panics (evaluation stays total).
- Arithmetic pushes down to a SQL backend where the surrounding stage is pushable.
- The operator-freeze count-lock test reflects the new operator count deliberately.
**Acceptance criteria (if rejected):**
- The blueprint records the rejection reasoning; no code changes; mission box ticked as decided.

**Verification method:**
- `cd packages/qfs && cargo test --workspace` including new precedence/eval/error tests and the
  updated freeze-test.
- `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings`.
- `cargo run -p xtask -- gen-docs --check && cargo run -p xtask -- gen-skills --check` green (the
  new operators appear in the generated grammar; the cookbook recipe is parse-checked).
- The new cookbook arithmetic recipe passes `crates/test/tests/cookbook_skills.rs`.

**Gate:** full suite + freeze-test + cookbook ratchet green; owner reviews the precedence/type
rules against the chapter. Mission box updated.

## Considerations

- The `*` projection-star ambiguity is the sharp edge — resolve strictly by grammar position
  (`select *` vs an operand context) with a test pinning both readings.
- Bump the plugin `version` fields only if the taught surface changes (a new operator in a cookbook
  recipe does — see CLAUDE.md re-versioning rule); patch the binary version per the shipped-PR rule.
- Depends on the type chapter's numeric rules; do not invent promotion semantics here.

## Final Report

Development completed with the owner's explicit no-implicit-conversion ruling. qfs now parses and
evaluates `+`, `-`, `*`, and `/` with normal precedence, while keeping arithmetic monomorphic:
`Int +|-|* Int -> Int`, `Float +|-|*|/ Float -> Float`, mixed numeric arithmetic is rejected, and
`Int / Int` is not treated as implicit float division. Evaluation is total and returns structured
overflow, divide-by-zero, and integer-division errors rather than panicking.

The lexer now disambiguates `/path` from division and keeps `select *` separate from multiply by
grammar position. The generated language reference and operator freeze were updated, the binary and
plugin versions were bumped for the taught surface, and the query cookbook gained a core arithmetic
recipe (`unit_price * qty`) with the parse-ratchet baseline raised to 154. Backend pushdown for
arithmetic predicates is deliberately deferred: the current pushdown IR cannot represent arithmetic
expressions, so lowering rejects them structurally and the blueprint records the IR/driver follow-up.

### Discovered Insights

- **Insight**: Slash disambiguation is a lexer boundary, not just parser precedence. A path can
  legally follow statement words, call signatures, and new lines, while `/` after an operand is
  division; this needed explicit regression tests for `UPSERT INTO /dst /src`, `CREATE MAP CALL`,
  `CONNECT`, and `let` bodies.
- **Insight**: Arithmetic pushdown must wait for expression-capable predicate IR. Rejecting it at
  the lowering boundary is better than overclaiming pushdown, because the local planner remains
  honest about what the backend can execute.
