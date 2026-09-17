---
created_at: 2026-09-18T02:25:38+09:00
author: a@qmu.jp
assignees: [a@qmu.jp]
depends_on:
feedback: [20260918022343-diagnose-declared-slack-read-failures-when-the-same-account-succeeds-through-the-web-api.md]
merge_policy:
verification_handoff: 
claim: work-20260918-023508
---

# Say what the declared view body evaluation actually failed at, and fix the Slack replies read it is masking

## Overview

A live read of a private Slack channel and one of its threads fails through `qfs` while the
**same account's credential** succeeds through the Slack Web API (`auth.test` ok,
`conversations.info` confirms membership, `conversations.replies` returns `ok: true`, all 18
messages, `has_more: false`). `qfs describe` advertises SELECT for the same replies path, and the
query still fails:

```
{"error":{"code":"invalid_path","kind":"usage",
 "message":"invalid path \"/slack/…/messages/…/replies\": declared view body evaluation failed"}}
```

The reported message is produced at `packages/qfs/crates/exec/src/declared.rs:1356`, where
`run_body_ops` discards the evaluator's own error:

```rust
MiniEvaluator::new()
    .execute(&physical, ScanResults::new(vec![fetched]))
    .map_err(|_| "declared view body evaluation failed")
```

All three call sites (lines 652, 670, 714) then wrap that one `&'static str` in
`CfsError::InvalidPath`, which renders as `code: invalid_path`, `kind: usage` — i.e. the CLI tells
the operator *their path is wrong* when in fact an internal evaluation stage failed on a path the
binary itself resolved and described. Two defects are stacked here and the second hides the first:

1. **The diagnostic is destroyed.** `EngineError` is already a structured, secret-free enum —
   `UnknownColumn { … }`, `MissingScanResult`, `Arity`, `TransformOutputMismatch`, and the
   `ExpandError::{Unknown(MissingColumn), NotExpandable}` family reachable through
   `qfs_engine::eval::expand` — and every one of them collapses into the same eleven words.
   `lower_query` and `partition_by_source` are discarded the same way two lines above.
2. **The category is wrong.** `CfsError::InvalidPath` is documented as *a virtual path was
   malformed at the driver boundary (empty, or not absolute)*. An evaluator failure over a
   correctly-resolved declared view is not that, and classifying it as `kind: usage` sends every
   reader — human or agent — to check the path, the credential and the channel membership, which
   is exactly the wasted round the reporter already ran by hand.

The ask is explicit that the internal cause must be captured **before** a fix is chosen. So this
ticket's first deliverable is the diagnostic, and the second is whatever the diagnostic proves.

## Policies

- `workaholic:implementation` / `policies/directory-structure.md` — conventional project layout
- `workaholic:implementation` / `policies/coding-standards.md` — style and structure conventions
- `workaholic:implementation` / `policies/error-handling.md` — a failure must say what failed;
  errors are structured for machine consumption (blueprint §6: parseable by an AI, not prose)
- `workaholic:safety` — no credential, no private response body and no message content may reach
  an error string, a log line or a test fixture

## Key Files

- `packages/qfs/crates/exec/src/declared.rs` — `run_body_ops` (line 1339) discards three errors
  (`lower_query`, `partition_by_source`, `MiniEvaluator::execute`); the three call sites at 652,
  670 and 714 wrap the survivor in `CfsError::InvalidPath`. The change is here.
- `packages/qfs/crates/driver/src/error.rs` — `CfsError`, `#[non_exhaustive]`. `InvalidPath.reason`
  is `&'static str`, so it structurally cannot carry a stage or a detail; `Decode { fmt, detail:
  String }` is the precedent for an arm that does.
- `packages/qfs/crates/engine/src/combine.rs` — `EngineError` (line 48) and
  `MiniEvaluator::execute` (line 225): the structured error being thrown away.
- `packages/qfs/crates/engine/src/eval.rs` — `expand` (line 666), `expand_item` (line 799),
  `observed_struct_fields`: where a real Slack envelope with optional/nested fields is most likely
  to fail, and where the `None`-names positional fallback still exists.
- `packages/qfs/crates/skill/assets/examples/slack_driver.qfs` — the failing views, lines 48–55:
  `… |> DECODE json |> EXPAND messages` under `OF slack/message`.
- `packages/qfs/crates/exec/src/declared.rs` test
  `eval_view_body_unwraps_the_envelope_and_shapes_to_the_of_type` (line 1478) — the existing
  coverage, a small uniform two-message envelope; this is the test whose narrowness the reporter
  names.
- `.workaholic/tickets/archive/work-20260803-213737/20260725103000-declared-expand-must-splice-by-field-name.md`
  — the prior, landed work on exactly this axis (Slack's `thread_ts` / `subtype` appear only on
  some messages). Read it before designing: it records what was already fixed and what was not.

## Implementation Steps

**Diagnosis first — the fix is not designed until step 3 has an answer.**

1. **Reproduce hermetically.** Capture a sanitized `conversations.replies` envelope shaped like the
   reported one — ~18 messages, mixed presence of `thread_ts`, `subtype`, `bot_id`, `edited`,
   `blocks` (nested), `reactions` (array of objects), `files`, plus at least one message whose text
   is empty and one join/subtype message. Invent the contents; carry over only the *shape*. Drive it
   through the `/slack/{ws}/{channel}/messages/{ts}/replies` view body
   (`… |> DECODE json |> EXPAND messages`, `OF slack/message`) in a unit test at the
   `declared.rs` seam, and assert the current failure. If it does not fail, widen the envelope
   (top-level `response_metadata`, `has_more`, an `ok: true` sibling key order change) until it
   does — the reproduction is the deliverable of this step, not a formality.
2. **Localize.** With the error preserved locally (a temporary `dbg!`/`unwrap_err()` is enough
   here), name *which* of the three discarded results failed and *which* `EngineError` /
   `ExpandError` variant it carried. Record the answer in the Final Report — that answer is the
   thing the reporter asked for.
3. **Verify named-mount account propagation** — the second half of the ask, and it is checkable
   without the live account. The reported error's `path` field reads `/slack/…` while the query
   addressed `/slack-example/…`. Establish whether `view_path` is reconstructed from the
   declaration's own driver name rather than the mount alias the user addressed, and whether the
   credential the failing execution resolved is the named mount's or a default. Trace
   `driver_name` / `params` / `view_path` through `eval_view_body` and its `fetch` closure and
   state, in the Final Report, which credential a named mount actually reaches. If they diverge,
   that is a second defect in this ticket's scope; if they do not, say so — the reporter explicitly
   notes the live comparison could **not** establish it.
4. **Carry the failure through.** Stop discarding the three errors in `run_body_ops`. Preserve at
   minimum a **stage** (`lower` / `plan` / `evaluate`) and a **secret-free detail** taken from the
   structured error's own fields — a column name, an op label, an arity — never a response body, a
   message text or a token.
5. **Classify it honestly.** An evaluator failure over a resolved declared view stops being
   `InvalidPath` / `kind: usage`. Add the arm the failure deserves (see Considerations for the
   recommendation) and route the three call sites through it. Update any test or golden that
   pinned the old string.
6. **Fix what step 2 found**, in the same change. The diagnosis is not the deliverable on its own:
   the ask's success condition is that these valid history/replies reads return their rows.
7. **Lock it with the envelope from step 1.** Keep the sanitized fixture as permanent regression
   coverage — the optional/nested-field envelope this seam had no test for — and add a negative
   test asserting that a genuinely broken body reports its stage and detail rather than
   `invalid_path`.
8. Bump the patch in `packages/qfs/crates/qfs/Cargo.toml`. If the error surface the skills teach
   changes, bump all four plugin `version` fields per CLAUDE.md.

## Quality Gate

**Acceptance criteria** — the checkable conditions that must hold:

- A declared view body whose evaluation fails reports the **stage** it failed at and a
  secret-free **detail** from the underlying structured error, and no longer reports
  `code: invalid_path` / `kind: usage` for an internal evaluation failure.
- A sanitized, realistic Slack `conversations.replies` envelope — 18 messages, optional and nested
  fields, mixed key sets — is read through the declared replies view and **returns its rows**.
- The Final Report names the internal cause found at step 2 and the named-mount credential answer
  found at step 3, in words, whether or not either turned out to be a defect.
- No credential, no token, no live response body and no real message content appears in any fixture,
  test name, error string or log line added by this change.

**Verification method** — the commands/tests/probes that prove them:

- `cd packages/qfs && cargo test --workspace` — including the new `declared.rs` positive
  (realistic envelope returns rows) and negative (broken body reports stage + detail) tests.
- `cd packages/qfs && env -u XDG_CONFIG_HOME cargo test -p qfs --lib -- --test-threads=1`
- `cd packages/qfs && cargo clippy --workspace --all-targets -- -D warnings`
- `cd packages/qfs && cargo fmt --all --check`
- `cd packages/qfs && cargo run -p xtask -- gen-docs --check` and `gen-skills --check` if the
  error surface moved.
- `git grep -nE '(xox[baprs]-|Bearer )' -- <the files this change adds>` returns nothing.

**Gate** — what must pass before approval:

- All of the above green, and the Final Report carries the step-2 and step-3 answers.

## Considerations

- **Which error arm to add — recommended, not left open.** `CfsError::InvalidPath.reason` is
  `&'static str` and cannot carry a detail, and widening it would keep an evaluator failure
  labelled a path error. Add a dedicated `#[non_exhaustive]` arm — shape
  `ViewBodyEval { path: String, stage: &'static str, detail: String }`, matching the existing
  `Decode { fmt, detail: String }` precedent — and give it its own `code`. qfs is experimental and
  takes hard breaks, so no compatibility shim for the old string is wanted; update the tests that
  pinned it. Do not add a second reason string to `InvalidPath`.
- **The reporter's own hypothesis is a hypothesis, not the design.** The ask attributes the failure
  to the discarded `map_err` at line 1356. That line is certainly a *defect* — it is step 4 — but
  it is a **masking** defect, and whether it is also the *cause* is unknown until step 2 answers.
  Do not start from the assumption that restoring the error makes the read work.
- **The strongest prior on the cause**, for step 1's fixture design and for no other purpose:
  archived ticket `20260725103000-declared-expand-must-splice-by-field-name` recorded that Slack
  returns messages with differing key sets (`thread_ts`, `subtype` on some only), and
  `eval.rs::expand` still has a branch where `observed_struct_fields` yields `None` and splicing
  falls back to positional (`expand_item`, line 805). `ExpandError::Unknown(MissingColumn)`,
  `ExpandError::NotExpandable` and `EngineError::UnknownColumn` are all reachable and all collapse
  into the same eleven words today. Treat these as the shapes to *try to reproduce*, never as the
  answer.
- **Do not infer auth, vault locking or membership from this error.** The reporter's direct API
  comparison already ruled all three out for that account, and the generic message is precisely
  what makes them look plausible.
- **Live confirmation against the reporter's own private channel is the operator's**, and it is
  not this ticket's gate. Everything above is provable hermetically with a sanitized envelope,
  which is why no `verification_handoff` is declared: a handoff would park the unit at an open
  pull request for a decision nobody needs to make. The cost of that choice, stated plainly: the
  hermetic fixture is a *reconstruction* of the live envelope, so a fix can pass this gate and
  still miss a shape the reconstruction did not guess. If step 1 cannot make the failure
  reproduce at all, say so in the Final Report rather than shipping a speculative fix — an
  unreproduced failure is a report back to the operator, not a patch.
- **Scope boundary.** This is the read/evaluator path. The upstream write-rejection diagnostics in
  #108 (`service_rejected` collapsing Slack write errors) are a separate defect on a separate seam
  and are not touched here.
