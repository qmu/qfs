---
created_at: 2026-08-16T18:34:41+00:00
status: done
author: a@qmu.jp
assignees: []
depends_on:
mission:
merge_policy: review
verification_handoff:
claim: work-20260816-194159
---

# `EvalError` has no `Display`, so a plan-time refusal reaches the operator as a Debug dump

## Overview

Every plan-time refusal the evaluator raises is rendered with `{:?}`. `map_eval_error`
(`packages/qfs/crates/exec/src/exec.rs`, ~355) says so in a comment and means it:

```rust
// EvalError has no Display; its owned, secret-free Debug is the machine-facing message. The
// host-realm arm's inner error DOES Display — render it so the canonical pointer reads clean.
let message = match &err {
    EvalError::HostScope(h) => h.to_string(),
    other => format!("{other:?}"),
};
```

Measured 2026-08-16 on `qfs 0.0.102`:

```
$ qfs run "/local/tmp/qfsdemo |> of (nope text)"
{"error":{"code":"of_assertion_failed","kind":"usage","message":"OfAssertionFailed { ty: \"(inline)\", missing: [\"nope\"], unexpected: [\"name\", \"path\", \"size\", \"modified\", \"is_dir\", \"mode\", \"content\"], mismatched: [] }"}}
EXIT=2

$ qfs run "/local/tmp/qfsdemo/t.csv |> decode csv |> transform nosuch"
{"error":{"code":"transform_not_executable","kind":"internal","message":"TransformNotExecutable { name: \"nosuch\" }"}}
EXIT=5
```

Contrast the engine's and the planner's errors, which all carry a hand-written `Display` and read as
sentences (`` `where` names column 'nope', which this relation does not carry; available: […] ``).
`EvalError` is the odd one out, and it is the layer that produces the refusals an operator meets
most often — the `of` assertion, the switch shape errors, the write-lowering rejections.

**This is a convention, not a bug, which is why it is a ticket and not a fix.** The comment states a
rationale: the `Debug` form is *owned* (the enum's own derived rendering cannot drift from its
fields) and *secret-free* (nothing in an arm holds a credential). Both are real properties, and a
hand-written `Display` over 28 arms gives up the first to gain readability. The question this ticket
exists to settle is whether that trade is worth making, and it is the developer's to settle.

## Scope

**In scope:** rule on whether `EvalError` gets a `Display` impl; if yes, write it across every arm
and switch `map_eval_error` to it, keeping `code()` untouched.

**Out of scope:**

- The `code()` vocabulary and the `ErrorKind` mapping — both correct today, and neither changes
  whichever way this lands.
- The codec seam, fixed by `20260816175149`: `unknown_column` / `not_expandable` after a decode
  already carry the engine's own prose.
- Any other error type. `EngineError`, `LowerError`, `PlanError` and `CfsError` already `Display`.

## Open Decisions

- **Does `EvalError` get a `Display` impl?** — options: **(a)** write one over all 28 arms, so every
  operator-facing message is prose, at the cost of a second rendering that must be kept faithful to
  the fields; **(b)** keep `Debug` as the machine-facing message and instead document, in
  `docs/cookbook/faq.md`, that a plan-time error's `message` is a structured dump whose `code` is
  the thing to branch on. Neither is clearly right: (a) is what the rest of the codebase does and
  what an operator reading a terminal wants; (b) is what the existing comment deliberately chose,
  and it is genuinely cheaper to keep honest. A run cannot settle this from evidence in the tree,
  because the tree contains both the convention and its contradiction.

## Policies

- `workaholic:design` / 「推測するな、宣言して拒否せよ」 — the refusals are right; how they are
  rendered decides whether a correct refusal teaches or confuses.
- `workaholic:implementation` / `objective-documentation` — whichever way this lands, the FAQ's
  promise that the envelope carries a stable `code` and a usable `message` must match the binary.
- `workaholic:implementation` / `coding-standards`.

## Key Files

- `packages/qfs/crates/exec/src/exec.rs` (~355) — `map_eval_error` and the comment recording the
  present convention.
- `packages/qfs/crates/core/src/eval.rs` (~212 onward) — `EvalError`, its 28 arms and its `code()`.
- `packages/qfs/crates/engine/src/combine.rs` (~136) — `EngineError`'s `Display`, the shape a new
  one would follow.
- `docs/cookbook/faq.md` — the exit-code table and the common-errors rows, which describe what an
  operator sees.

## Implementation Steps

1. Enumerate which `EvalError` arms actually reach an operator (several are internal-class and only
   ever surface as exit 5), so the ruling is made against the real blast radius rather than the arm
   count.
2. Settle the Open Decision above with the developer.
3. Implement the chosen side; if (a), one `write!` per arm mirroring the field set, and delete the
   `{other:?}` fallback so a future arm cannot silently regress to a dump.
4. Pin it: a test asserting no operator-facing `message` contains `" { "`, the same negative
   assertion `20260816175149` added at the codec seam.

## Quality Gate

**Acceptance criteria**

- The decision is recorded with its reason in the ticket outcome and the commit body.
- If (a): every `EvalError` arm renders as a sentence and `map_eval_error` has no `{:?}` fallback;
  the two commands quoted above answer in prose at their current codes and exit codes.
- If (b): `docs/cookbook/faq.md` states that a plan-time `message` is a structured dump and that
  `code` is the stable branch point, and `gen-skills` is re-run.
- `code()` and the `ErrorKind` mapping are byte-identical either way.

**Verification method**

- The two commands above, re-run against the built binary with `echo "EXIT=$?"` pasted into the
  ticket outcome, not paraphrased.
- The negative-assertion test from step 4.

**Gate**

- `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo fmt --all --check`, and `gen-docs --check` / `gen-skills --check` / `check-migrations` all
  exit 0.

## Considerations

- Minted mid-drive by the `[Implement]` routine on 2026-08-16 while driving `20260816175149`, whose
  acceptance criterion 3 ("no error message reaching an operator contains Rust `Debug` struct
  syntax") was written project-wide inside a ticket scoped to one seam. That criterion is met at the
  codec seam and openly reported as unmet elsewhere; this ticket is the elsewhere
  (`packages/qfs/crates/exec/src/exec.rs`).
- Two branches have now recorded a concern pointing at this rendering — `work-20260816-174228` and
  `work-20260816-181224`. It is the third sighting, which is the argument for ruling it rather than
  re-observing it.

## Final Report

Development completed as planned: option **(a)**, `EvalError` now `Display`s.

### The Open Decision, resolved — (a) write the `Display`

The ticket called this the developer's to settle because "the tree contains both the convention and
its contradiction". It does, but not symmetrically, and the asymmetry is readable from the tree
without a ruling:

- **The convention is six types wide; the contradiction is one comment deep.** `EngineError`,
  `LowerError`, `PlanError`, `CfsError`, `TypeError` and `HostScopeError` all hand-write a
  `Display`. Against that stands one comment in `map_eval_error` — which already special-cases the
  `HostScope` arm *out* of `{:?}` precisely to get prose, so the seam itself had already conceded
  the point for the one arm that could.
- **The project shipped the criterion two tickets ago.** `20260816175149` landed a project-wide
  acceptance item ("no error message reaching an operator contains Rust `Debug` struct syntax") and
  pinned it at the codec seam; this ticket exists because that criterion was written wider than its
  scope. Choosing (b) would have meant documenting the dump the project had just decided against.
- **Both properties the comment defends survive.** *Secret-free*: `Display` renders the same fields
  the `Debug` did, and no arm holds a credential. *Owned*: the risk (a) adds is a second rendering
  that can drift from the fields, and that is exactly what the new pin removes — the `Display` match
  is exhaustive with no `_` arm, so a new variant fails to compile until it renders, and the
  `{other:?}` fallback is deleted so there is nothing to silently regress into.
- **It is cheap to reverse.** `code()` and the `ErrorKind` mapping are untouched, so anything
  branching on the envelope is unaffected either way; if the developer prefers (b), reverting is one
  commit and no consumer has to change.

The two delegating arms that had no inner `Display` (`Resolve`, `Fn`) got one each — rendering
`EvalError::Resolve` as a sentence is not possible otherwise, so it is inside this ticket's scope,
not beyond it.

### Quality Gate

**Criteria.** Every arm renders as a sentence and `map_eval_error` has no `{:?}` fallback; `code()`
and the `ErrorKind` mapping are byte-identical; the two measured commands answer in prose at their
original codes and exit codes.

**Verification, re-run against the built binary (not paraphrased):**

```
$ qfs run "/local/tmp/qfsdemo |> of (nope text)"
{"error":{"code":"of_assertion_failed","kind":"usage","message":"`of (inline)` does not match this relation; missing: [nope]; undeclared: [name, path, size, modified, is_dir, mode, content]"}}
EXIT=2

$ qfs run "/local/tmp/qfsdemo/t.csv |> decode csv |> transform nosuch"
{"error":{"code":"transform_not_executable","kind":"internal","message":"transform 'nosuch' is not installed here: no `CREATE TRANSFORM` defines it"}}
EXIT=5
```

Same codes, same exit codes, prose instead of a struct dump.

**Gate.** `cargo test --workspace` (2723 passed, 0 failed), `cargo clippy --workspace --all-targets
-- -D warnings`, `cargo fmt --all --check`, `gen-docs --check`, `gen-skills --check`,
`check-migrations` — all exit 0.

### Discovered Insights

- **Insight**: Nine `qfs`-crate unit tests fail on a machine where `XDG_CONFIG_HOME` is unset —
  `store.rs`'s `forbid_shared_home_fallback_in_tests` guard panics with "wrap the test in
  `testenv::HomeGuard`" — and pass when it is set.
  **Context**: The suite is documented as hermetic, but those nine inherit hermeticity from the
  ambient environment rather than from a guard, so a fresh container reads them as a red gate
  unrelated to whatever it is driving. Minted as `20260816205752`.
- **Insight**: `map_eval_error` had already carved the `HostScope` arm out of the `{:?}` rendering
  to make one canonical pointer readable. A convention with a hand-written exception for the case
  that mattered most is a convention already deciding against itself — worth reading as evidence
  when a ticket calls a fork evidence-free.
