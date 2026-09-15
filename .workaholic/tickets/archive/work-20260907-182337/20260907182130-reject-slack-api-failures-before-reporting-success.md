---
created_at: 2026-09-07T18:21:30+09:00
status: done
author: a@qmu.jp
assignees: [a@qmu.jp]
depends_on:
---

# Reject Slack API failures before reporting success

## Overview

A live thread reply returned HTTP 200 with `{"ok":false,"error":"missing_post_type"}`. QFS printed `committed: true` despite no message being posted. JSON request bodies lacked Content-Type, and the generic REST boundary treated every HTTP 2xx as application success. Reads also hid the Slack error behind a generic declared-view evaluation failure.

## Key Files

- packages/qfs/crates/driver-http/src/applier.rs
- packages/qfs/crates/driver-http/src/config.rs
- packages/qfs/crates/driver-http/src/error.rs
- packages/qfs/crates/driver-http/src/lib.rs
- packages/qfs/crates/qfs/src/declared_driver.rs
- packages/qfs/crates/qfs/src/shell.rs
- packages/qfs/crates/qfs/src/declared_driver/mounted_account_tests.rs

## Related History

- 20260907131324-resolve-slack-credentials-from-the-mounted-account.md: per-path bearer selection.
- 20260727214856-declared-rest-drivers-cannot-post-form-encoded-bodies.md: wire encoding and Content-Type.

## Implementation Steps

1. Reproduce HTTP-success/application-failure with a hermetic Slack response and verify the current false success.
2. Add an opt-in JSON boolean response contract to the REST boundary; compose it for the exact Slack Web API origin/path. Reject false, missing, wrongly typed or malformed success envelopes before decoding, counting effects, or following pagination. Keep other REST responses unchanged.
3. Include application/json on generated JSON bodies, sharing the helper for writes and read-over-POST. Preserve form/multipart headers.
4. Propagate a bounded, secret-free service failure through reads and commits without automatic POST retries.
5. Prove CLI failed commits exit nonzero without committed:true; prove successful writes still work and preview sends nothing. Bump binary patch version and document behavior.

## Policies

- implementation/test: regress the observed failure through the execution path, not just a shape assertion.
- implementation/observability: failures must remain observable to humans and agents.
- design/defense-in-depth: HTTP success is insufficient proof of application acceptance.
- implementation/anti-corruption-structure: vendor selection stays at composition; generic REST validation is opt-in.

## Quality Gate

### Acceptance Criteria

- A Slack `ok:false` cannot report a successful commit or affected row; the CLI exits nonzero and exposes a safe error code.
- Generated JSON requests carry Content-Type; form/multipart and generic REST semantics are preserved.
- Successful envelopes pass; malformed Slack responses fail closed; POST failures are not retried.
- Read failures retain the actionable service error and stop pagination.

### Verification Method

Decided: hermetic transport and CLI regression tests plus workspace checks; no additional real Slack messages are authorized by this fix request.

### Gate

Targeted regressions, cargo test --workspace, serialized qfs lib tests, cargo fmt, clippy, generated documentation/skills and migration checks pass. Review source and tests before installing the fixed binary.

## Considerations

A generic REST response may legitimately contain ok:false as business data; response validation must be opt-in. Existing persisted Slack declarations must gain the fix without credential changes or reinstalling definitions. The scope is truthful error handling and request encoding, not a new Slack feature set.

## Final Report

Implemented opt-in JSON response contracts at the REST boundary and enabled them for the exact Slack Web API base URL. Slack application failures now stop reads and commits, preserve bounded error codes, and never count as affected rows. Generated JSON bodies carry application/json for writes and read-over-POST. Unknown error text is sanitized and failed posts are not retried. Existing persisted Slack definitions inherit the fix.

Verification: all 2,800 workspace tests passed (2 ignored); serialized qfs lib tests passed 529 (1 ignored). Targeted mounted-account and CLI tests prove failed commits exit 5 without committed:true, preview sends nothing, and successful account-isolated writes remain valid. Format, clippy, generated docs and skills checks plus migration integrity passed; GitHub CI passed all jobs. Live read-only checks using the release candidate read internal_with_yosan successfully and returned channel_not_found with exit 5 for a nonexistent channel. No additional Slack messages were posted.

Decision: select the response contract at composition for https://slack.com/api (optional trailing slash), rather than by driver label or for all REST services. Custom Slack proxies are outside this selection; generic REST business data containing ok:false retains its existing semantics. Binary patch version is 0.0.130 and both plugin manifests are 0.22.2.
