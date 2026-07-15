---
created_at: 2026-07-09T10:42:58+09:00
author: a@qmu.jp
type: refactoring
layer: [Domain]
effort: 1h 30m
commit_hash:
category: Changed
depends_on: [20260709104254-blueprint-type-system-chapter.md]
mission: language-design-review-layering-principles-and-semantic-gaps
---

# Stdlib naming/resolution policy + LIKE double-spelling + Op::Eq doc drift

## Overview

Resolve three related semantic-consistency gaps the mission found, all in the function/operator
surface, all verified against HEAD (2026-07-09):

1. **Stdlib name-resolution splits from keyword policy.** Keywords are lowercase-canonical,
   recognised case-insensitively (decision S/t74). Builtins are a **case-sensitive** exact
   `HashMap` lookup (`crates/core/src/stdlib/registry.rs`, `builtin`/`is_builtin` over
   `self.core.get(name)`) with a mixed convention — `UPPER`/`COUNT` uppercase,
   `map`/`filter`/`reduce`/`env`/`http.get` lowercase. So `upper(x)` resolves to
   `unknown_function` today. The in-tree doc-comment at `crates/core/src/lambda.rs` even writes
   `concat(x, suffix)`, which does not resolve. Adopt **one** naming/recognition policy aligned
   with the keyword rule (recommended: canonical lowercase, case-insensitive recognition) and
   enforce it with a test.
2. **`LIKE` is spelled twice** — a frozen operator (`expr LIKE pat`, `Op::Like`) **and** a
   registered scalar builtin (`LIKE(s, pat)`, `crates/core/src/stdlib/scalar.rs:50`). One meaning,
   two grammars; anti-compression and a precedent eroding the operator freeze. Remove the builtin
   duplication (keep the operator), or record why both must exist.
3. **`Op::Eq` doc drift** — `crates/parser/src/ast.rs` documents `Op::Eq` as `` `=` ``, but
   decision O reserves `=` for binding and `Token::EqEq` (`==`) is what maps to `Op::Eq`
   (`crates/parser/src/grammar.rs:380`). Fix the doc-comment.

Grounded in the type-system chapter (`depends_on`) because the naming/recognition policy is part
of "one vocabulary and grammar everywhere," and `FnSig` is the type surface of the registry.

## Policies

- `workaholic:implementation` / `policies/directory-structure.md` — the resolution policy lives in
  `qfs-core` stdlib; the recognition rule must not leak backend-specific casing.
- `workaholic:implementation` / `policies/coding-standards.md` — one canonical spelling, enforced
  by a test rather than convention.
- `workaholic:implementation` / `policies/type-driven-design.md` — the registry's `FnSig` is the
  function surface's type; the recognition policy should make an unresolvable name a crisp,
  structured `unknown_function`, consistently.
- `workaholic:implementation` / `policies/objective-documentation.md` — the `Op::Eq` doc-comment
  and the `lambda.rs` `concat` example must be made true (both currently misdocument the surface).

## Key Files

- `packages/qfs/crates/core/src/stdlib/registry.rs` - `builtin`/`is_builtin` lookup; where the
  case policy is enforced (normalise on insert + lookup, or assert canonical case in a test).
- `packages/qfs/crates/core/src/stdlib/scalar.rs` - the `LIKE` builtin (line ~50) to remove; the
  `UPPER`/`LOWER`/… names whose case the policy pins.
- `packages/qfs/crates/core/src/stdlib/higher_order.rs` - `map`/`filter`/`reduce` (lowercase),
  the other pole of the current inconsistency.
- `packages/qfs/crates/core/src/lambda.rs` - the module doc-comment `concat(x, suffix)` example to
  make resolvable (or correct).
- `packages/qfs/crates/parser/src/ast.rs` - the `Op::Eq` doc-comment (line ~571).

## Related History

- [20260627120100-t74-lowercase-keywords.md](.workaholic/tickets/archive/work-20260628-000332/20260627120100-t74-lowercase-keywords.md) - the case-insensitive keyword recognition this ticket aligns the stdlib to
- [20260626103000-t70-operator-equals-binds-eqeq-compares.md](.workaholic/tickets/archive/work-20260628-000332/20260626103000-t70-operator-equals-binds-eqeq-compares.md) - decision O (`=` binds, `==` compares), the source of the `Op::Eq` doc truth
- [20260622214650-t08-stdlib-and-driver-preludes.md](.workaholic/tickets/archive/work-20260622-230954/20260622214650-t08-stdlib-and-driver-preludes.md) - the stdlib registry this ticket makes consistent

## Implementation Steps

1. Record the stdlib naming/recognition policy in the blueprint/type chapter cross-reference
   (canonical lowercase, case-insensitive recognition — matching keywords).
2. Enforce it in `registry.rs`: normalise names on insert and lookup (or canonicalise + assert),
   so `upper(x)`, `UPPER(x)`, `Count(...)` all resolve. Add a test covering mixed-case resolution
   for a representative set (`upper`, `count`, `map`, `http.get`).
3. Remove the `LIKE` scalar builtin (keep the `Op::Like` operator); add a test that `LIKE` as a
   function is rejected while `expr LIKE pat` works — or, if kept, record the justification.
4. Fix the `Op::Eq` doc-comment (`= ` → `==`, citing decision O) and make the `lambda.rs`
   `concat` example resolvable under the new policy.
5. Regenerate docs (the function list in `docs/language.md`/`drivers.md` may reflect casing);
   run the full suite; tick the mission's stdlib acceptance box.

## Quality Gate

**Acceptance criteria:**
- `upper(x)`, `UPPER(x)`, `Map(...)` all resolve identically (case-insensitive), asserted by test.
- `LIKE` is spelled one way (operator); the function form is rejected (or the dual-spelling is
  justified in a recorded note), asserted by test.
- `Op::Eq` doc-comment says `==` per decision O; the `lambda.rs` `concat` example resolves.
- No generated-doc drift.

**Verification method:**
- `cd packages/qfs && cargo test --workspace` (mixed-case resolution + LIKE-removal tests pass).
- `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings`.
- `cargo run -p xtask -- gen-docs --check && cargo run -p xtask -- gen-skills --check` green.

**Gate:** full suite + anti-drift green; owner confirms the naming policy matches the keyword rule.
Mission box updated.

## Considerations

- Case-insensitive resolution must not accidentally collide two distinct names — verify the
  registry has no names differing only by case before normalising (it does not today, but assert).
- `http.get`/`env` are dotted/lowercase already; the policy must accommodate the dotted procedure
  form without forcing it into the scalar-name casing.
- Removing the `LIKE` builtin is a hard break in a pre-release surface — correct per the
  experimental-no-backward-compat posture; no deprecation shim.
- If any cookbook recipe uses uppercase function names that now also accept lowercase, the
  verified-true ratchet still passes (recognition widens, never narrows).

## Final Report

Development completed as planned. The stdlib registry now canonicalizes builtin names to lowercase
and resolves functions case-insensitively, with a collision assertion so two names cannot differ
only by case. Scalar and higher-order builtins keep one resolution policy, `LIKE(...)` was removed
as a function spelling, and `LIKE` remains the single infix operator form.

The refinement checker now distinguishes row-local pure builtins from context, aggregate, and
table-valued builtins. That keeps pure scalar functions valid in type refinements while rejecting
`now()`, `current_date()`, `last_run()`, and `env()` for the right reason. The `Op::Eq` doc comment
now says `==`, matching the frozen grammar decision that `=` is binding syntax.

### Discovered Insights

- **Insight**: Purity is not equivalent to "not aggregate." Context builtins are scalar-shaped but
  statement-context dependent, so a small `row_local_pure` bit gave the refinement checker a
  precise policy without hard-coding every function name.
- **Insight**: Lowercase storage plus case-folded lookup matches the keyword policy and avoids a
  split vocabulary. The registry-level collision assertion is the right guard because it catches
  accidental ambiguous additions at startup/test time.
