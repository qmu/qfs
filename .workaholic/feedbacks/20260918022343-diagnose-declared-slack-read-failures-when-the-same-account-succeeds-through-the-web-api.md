---
type: Feedback
title: Diagnose declared Slack read failures when the same account succeeds through the Web API
kind: instruction
source: discussion
subject: person:tamurayoshiya
created_at: 2026-09-18T02:23:43+09:00
author: a@qmu.jp
supersedes: 
---

# Diagnose declared Slack read failures when the same account succeeds through the Web API

Source: https://github.com/qmu/qfs/issues/129

# Diagnose declared Slack read failures when the same account succeeds through the Web API

A live read failure in qfs 0.0.128 (commit fac8d4d, aarch64-unknown-linux-musl): a named
Slack account reads a private channel and its thread through the Slack Web API, but the
same read through qfs fails with `invalid_path` / `declared view body evaluation failed`.

What the reporter established, using exactly the mounted account's credential:

- `auth.test` succeeded.
- `conversations.info` confirmed membership of the private channel.
- `conversations.replies` returned `ok: true`, all 18 messages, `has_more: false`.
- `qfs describe` advertises SELECT for the same replies path.
- The query fails (coordinates synthetic):

```sh
qfs run '/slack-example/workspace/CHANNEL/messages/ROOT/replies |> select ts, text |> limit 100' --json
# exit 2
# {"error":{"code":"invalid_path","kind":"usage","message":"invalid path \"/slack/workspace/CHANNEL/messages/ROOT/replies\": declared view body evaluation failed","path":"/slack/workspace/CHANNEL/messages/ROOT/replies"}}
```

The channel messages query fails the same way. No Slack writes or account changes were
performed. The direct API comparison establishes that the selected credential and thread
access work; it does **not** establish which credential or response the failing qfs
execution actually used, nor the underlying evaluator error.

Source inspection in checkout 4643f44: `packages/qfs/crates/exec/src/declared.rs::run_body_ops`
discards the `MiniEvaluator::execute` error with `.map_err(|_| "declared view body evaluation
failed")` (line 1356). Callers then classify that string as `CfsError::InvalidPath`. The
installed views decode JSON and expand `messages`; the existing
`eval_view_body_unwraps_the_envelope_and_shapes_to_the_of_type` test covers only a small
uniform two-message envelope. The exact live trigger is unconfirmed.

This is a read/evaluator diagnostic issue, distinct from the upstream write-rejection
diagnostics in #108.

What the reporter asks for:

- Preserve a safe error category and the failing evaluation stage, without leaking
  credentials or private response/message contents.
- Verify named-mount account propagation (which credential the failing execution used).
- Add regression coverage using sanitized realistic Slack envelopes carrying
  optional/nested message fields.

Success means these valid history/replies reads return their rows, and that actual
decoding/evaluation failures explain what failed instead of masquerading as an invalid
user path. Capture the internal cause before deciding on a fix; do not assume
authentication, vault locking, or missing channel membership from this generic error.
