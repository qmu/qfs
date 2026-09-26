---
created_at: 2026-09-19T05:00:00+09:00
author: a@qmu.jp
assignees: [a@qmu.jp]
depends_on:
feedback: [20260918222801-a-slack-file-detach-reports-success-and-deletes-nothing.md]
merge_policy:
verification_handoff: 
---

# A declared write addressed at its target reports success and sends nothing

## Overview

`remove /slack/<ws>/files/<id>` passes the irreversible gate, reports `committed: true`, and the
file is still there. Measured live on 2026-09-18 and again on a freshly uploaded file: two deletes,
no change, content still readable. Tracing the run at `qfs_driver_http=debug` showed **no wire
request at all** — the effect was "applied" without anything being sent.

Two defects, stacked, both in the declared-map write path:

1. **A rowless write evaluates zero bodies.** `eval_map_body` builds one wire body per incoming row
   (`for r in &incoming.rows`). A write ADDRESSED at its target carries no row — `remove
   /slack/<ws>/files/<id>` names the file in the path — so the loop runs zero times, the applier's
   `for body in &write.bodies` runs zero times, and the effect reports success having sent nothing.
   Silence on an irreversible verb is the worst place this could land.
2. **The wire leg used the mount's verb, not the body's.** `apply_facets` overrode the wire effect
   kind only when a CALL map matched (`if called.is_some()`), so a `CREATE MAP REMOVE … AS INSERT
   INTO /http/slack/files.delete` went out as `DELETE /files.delete` instead of the declared POST.
   With defect 1 fixed the request finally left, and Slack answered `invalid_arguments` — the
   declaration's own verb had never been honoured.

The response contract added in `c9a956d` was working the whole time; it never got a response to
judge.

## Policies

- `workaholic:implementation` / `policies/error-handling.md` — an unconfirmed outcome is never
  reported as a completed one
- `workaholic:implementation` / `policies/coding-standards.md` — style and structure conventions
- `workaholic:safety` — an irreversible verb must never claim an effect it did not perform

## Key Files

- `packages/qfs/crates/exec/src/declared.rs` — `eval_map_body`'s per-row loop (defect 1) and the
  `MapWrite.wire_kind` it already computes from the body's own verb.
- `packages/qfs/crates/qfs/src/apply_facets.rs` — `RestApplyDriver::apply_one`, where `wire.kind`
  was set only for a CALL (defect 2) and where an empty `write.bodies` silently applies nothing.
- `packages/qfs/crates/skill/assets/examples/slack_driver.qfs` — the `CREATE MAP REMOVE` this was
  measured on; the same shape is the chatwork/cloudflare file and queue maps' shape.
- `packages/qfs/crates/qfs/src/declared_driver.rs` — the Slack response contract (`ok`/`error`),
  which is what turns Slack's HTTP-200 refusal into a failed effect once a request is actually made.

## Implementation Steps

1. In `eval_map_body`, evaluate the body ONCE when the write carries no row, with `path.<param>`
   bound and an empty `row` struct.
2. Refuse — structurally, before the wire — a rowless write whose body reads `row.<field>`: putting
   nulls on the wire and calling it success is the same defect in a different mask.
3. In `apply_facets`, always set the wire effect kind from the map body's declared verb.
4. Regression tests at the `declared.rs` seam: an address-only body yields exactly one wire body
   with its `path.<param>` bound, and a rowless body that reads a row is refused.
5. Bump the patch.

## Quality Gate

**Acceptance criteria**

- `remove /slack/<ws>/files/<id>` issues exactly one `POST https://slack.com/api/files.delete` and
  the file is gone; a Slack refusal fails the effect instead of reporting success.
- A declared map's wire leg uses the verb its body declares, for every map kind.
- A rowless write whose body needs a row is refused with a structured error.

**Verification method**

- `cd packages/qfs && cargo test --workspace`
- `cd packages/qfs && cargo clippy --workspace --all-targets -- -D warnings`
- `cd packages/qfs && cargo fmt --all --check`
- Live: upload a file to a channel, delete it, confirm the listing no longer returns it.

## Considerations

- **Why the mount's verb was ever used.** The override was written for CALL maps, where the
  procedure kind cannot reach the wire; universal-verb maps happened to work because the two verbs
  agreed (`INSERT` → `INSERT`). The Slack detach is the first shipped map where they differ, so the
  gap had no way to show until a REMOVE map existed.
- **Scope.** Both fixes are in the declared write path and apply to every declared driver, not to
  Slack. The `qfs.file-upload` primitive is unaffected (it intercepts before the method matters).
