---
created_at: 2026-08-16T17:51:49+00:00
author: a@qmu.jp
assignees: []
depends_on:
mission:
merge_policy: review
verification_handoff:
claim: work-20260816-182531
---

# A post-decode refusal leaks the Rust debug struct instead of the prose sentence

## Overview

`unknown_column` and `not_expandable` are the project's two structured stage refusals, and before a
`decode` they emit a prose sentence naming the column and listing the available ones. **After** a
`decode` the same two failures are re-wrapped under `codec_then_query` and the message carries the
Rust `Debug` form of the error enum instead:

```
$ qfs run "/local/tmp/qfsdemo/t.csv |> decode csv |> where nope == 1"
{"error":{"code":"codec_then_query","kind":"usage","message":"post-decode evaluation failed: UnknownColumn { stage: \"where\", name: \"nope\", available: [\"path\", \"a\", \"b\"] }"}}
EXIT=2

$ qfs run "/local/tmp/qfsdemo/t.csv |> decode csv |> expand a"
{"error":{"code":"codec_then_query","kind":"usage","message":"post-decode evaluation failed: NotExpandable { field: \"a\", ty: \"a scalar (int)\" }"}}
EXIT=2
```

against the pre-decode shape for the identical mistake:

```
$ qfs run "/local/tmp/qfsdemo |> where nope == 1"
{"error":{"code":"unknown_column","kind":"usage","message":"`where` names column 'nope', which this relation does not carry; available: [name, path, size, modified, is_dir, mode, content]"}}
EXIT=2
```

Measured 2026-08-16 on `qfs 0.0.99`, branch `work-20260816-174228`, while driving
`20260725143100-faq-under-describes-exit-2-and-the-new-refusals.md`.

Three things are wrong with the second shape, in ascending order of cost:

1. **It is a leaked internal representation.** `UnknownColumn { stage: "where", … }` is a Rust type
   name and its field syntax, printed at an operator. The project's own documentation policy is
   that an error tells the operator what to do; this one tells them what the enum is called.
2. **The stable `code` differs for the same mistake.** An agent branching on `code` —
   which the FAQ explicitly promises it can do, "exit codes are stable so an agent can branch on
   them" and "the JSON error envelope carries a stable `code`" — sees `unknown_column` on one path
   and `codec_then_query` on the other. The FAQ's new `unknown_column` row is therefore true only
   before a `decode`.
3. **The remedy is unstated.** The pre-decode message ends with the available columns, which is the
   whole fix. The debug form does carry them, but inside a `Vec<String>` literal a human has to
   parse out of a struct dump.

## Scope

**In scope:**

- Re-raise the post-decode failure with the *same* `code` and the *same* prose message the
  pre-decode path emits, so one mistake has one identity whichever side of a `decode` it lands on.
- Keep `codec_then_query` for what it actually names — a post-decode evaluation failure that is not
  one of the structured stage refusals.
- A test pinning both paths to the same `code` for the same mistake, so the two cannot drift again.

**Out of scope:**

- Changing any exit code. Both paths are `kind: usage` → 2 and that is correct.
- The late-bound `PlanSource::Codec` design itself. Late binding is deliberate (`decode`'s columns
  are undescribable until the bytes are read) and is not the defect — the defect is how the
  late-bound refusal is rendered on its way out.
- `select`'s silent drop after a decode, which is the separate open ticket
  `20260725113000-select-on-an-unknown-column-is-silently-dropped.md`.

## Policies

- `workaholic:design` / 「推測するな、宣言して拒否せよ」 — the refusal is right; rendering it as a
  debug dump is what turns a correct refusal into a confusing one.
- `workaholic:implementation` / `objective-documentation` — the doc and the binary must agree, and
  `docs/cookbook/faq.md` now documents a `code` this path does not produce.
- `workaholic:implementation` / `coding-standards`.

## Key Files

- `packages/qfs/crates/core/src/eval.rs` — the post-decode evaluation seam that produces
  `post-decode evaluation failed: {:?}`; where the re-wrap happens.
- `packages/qfs/crates/engine/src/lib.rs` — `apply_where` / `check_where_columns`, the native
  `unknown_column` / `not_expandable` producers whose prose the codec path should reuse.
- `packages/qfs/crates/exec/src/error.rs` — `ErrorKind::Usage`, the `code` vocabulary, and the
  exit-code mapping.
- `docs/cookbook/faq.md` — the common-errors rows added 2026-08-16, currently true only on the
  pre-decode path.

## Implementation Steps

1. Reproduce both shapes exactly as quoted above and localize the `{:?}` formatting site — the
   report above is a measurement, and the fix hypothesis below is a hypothesis, not an adopted
   design.
2. Establish whether the structured refusal survives the codec seam as a typed value or only as a
   rendered string. If typed, re-raise it unchanged; if it has already been flattened, carry the
   `code` and message through rather than re-deriving them at the boundary.
3. Leave `codec_then_query` in place for genuinely post-decode-only failures, and confirm nothing
   else was relying on the debug-formatted message.
4. Pin both paths in a test: the same query mistake before and after a `decode` yields the same
   `code` and a message naming the available columns.
5. Re-check the FAQ rows against the fixed binary; correct them if the fix changes the emitted text.

## Quality Gate

**Acceptance criteria**

- `… |> decode csv |> where <unknown> == 1` returns `code: unknown_column` (not `codec_then_query`)
  with the same prose sentence the pre-decode path emits, at exit 2.
- `… |> decode csv |> expand <scalar>` returns `code: not_expandable` with the prose sentence, at
  exit 2.
- No error message reaching an operator contains Rust `Debug` struct syntax (`Ident { field: … }`).
- A post-decode failure that is *not* a structured stage refusal still reports `codec_then_query`.

**Verification method**

- The new test pinning pre- and post-decode `code` equality for both refusals.
- The four commands above run against the built binary with `echo "EXIT=$?"` pasted into the ticket
  outcome, not paraphrased.

**Gate**

- `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo fmt --all --check`, and `gen-docs --check` / `gen-skills --check` / `check-migrations` all
  exit 0.

## Considerations

- Minted mid-drive by the `[Implement]` routine on 2026-08-16 while driving
  `20260725143100`, whose Final Report records the same measurement
  (`packages/qfs/crates/core/src/eval.rs`).
- The FAQ's new `unknown_column` and `not_expandable` rows are the surface that makes this visible
  to an agent: they promise a `code` the post-decode path does not emit
  (`docs/cookbook/faq.md`, the common-errors table).
