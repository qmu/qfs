---
type: Feedback
title: [FB] Make Slack write failures actionable instead of collapsing upstream errors to service_rejected
kind: instruction
source: discussion
subject: person:tamurayoshiya
created_at: 2026-09-16T06:54:07+09:00
author: a@qmu.jp
supersedes: 
---

# [FB] Make Slack write failures actionable instead of collapsing upstream errors to service_rejected

kind: instruction / source: discussion / subject: person:tamurayoshiya

# Make failed Slack writes explain the cause instead of collapsing it to service_rejected

The user asks for actionable errors: a write that fails without exposing why is an inadequate error implementation. In a live Slack thread-reply operation, the same mounted account, thread, and message failed with `INSERT INTO /slack-example/workspace/CHANNEL/messages/ROOT/replies VALUES ('hello')`, but succeeded when the column was explicit: `VALUES (text) ('hello')`. Both previews reported one INSERT and one exact affected row. The first commit exited 5 with `{"error":{"code":"commit_failed","kind":"commit_failed","message":"Terminal { reason: \"service response failed: service_rejected\" }"}}`; the explicit-column commit returned `committed: true`, and reading the thread confirmed the message and its correct parent. The operator had unlocked QFS before both attempts, so the failure reproduced after unlock and the changed column binding was sufficient to make this attempt work. Coordinates and message content above are synthetic.

Source inspection found that `packages/qfs/crates/driver-http/src/applier.rs::validate_response` maps a Slack `ok: false` response to a small allowlist of error codes and collapses every other value to `service_rejected`; `no_text`, for example, is not in that list. The actual upstream code for the failed attempt was not retained or observed, so `no_text` is a hypothesis, not a confirmed Slack response. This loss of information prevented distinguishing request construction, account locking, permissions, and upstream rejection until a successful comparison was tried.

Please retain safe, actionable upstream error classification and operation context without exposing tokens or arbitrary sensitive response bodies, and give invalid or missing write-field bindings a local diagnostic before sending when they can be validated. Cover the thread-reply positional versus explicit `text` case, and ensure an unsupported upstream code still yields useful diagnostic evidence rather than an unexplained `service_rejected`. The error should let an operator identify and correct the cause without guessing or repeatedly attempting a write.

Source: https://github.com/qmu/qfs/issues/108
