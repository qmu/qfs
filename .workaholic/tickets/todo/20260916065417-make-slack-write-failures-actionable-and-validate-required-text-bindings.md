---
created_at: 2026-09-16T06:54:17+09:00
author: a@qmu.jp
assignees: [a@qmu.jp]
depends_on:
feedback: [20260916065407-fb-make-slack-write-failures-actionable-instead-of-collapsing-upstream-errors-to-service-rejected.md]
merge_policy:
verification_handoff: 
---

# Make Slack write failures actionable and validate required text bindings

## Overview

Make failed Slack writes identify a safe cause and operation, and reject locally invalid text bindings before issuing the request. This is one atomic diagnostic repair from issue #108; diagnosis_first: true.

## Policies

- Follow CLAUDE.md: Rust workspace conventions, generated docs and skills, and release version policy.
- Preserve response secrecy: no tokens, arbitrary bodies, or request text in errors.

## Key Files

- packages/qfs/crates/driver-http/src/applier.rs — application response validation.
- packages/qfs/crates/driver-http/src/response_contract_tests.rs — allowlist and response fixtures.
- packages/qfs/crates/skill/assets/examples/slack_driver.qfs — message and reply write maps.
- packages/qfs/crates/qfs/src/declared_driver.rs — schema and write routing.
- docs/cookbook/slack.md — user-visible error contract; regenerate skills when changed.

## Implementation Steps

1. Reproduce and localize positional versus explicit text binding using a recording HTTP fixture for both message INSERT and thread replies; show the exact outgoing body before changing the design.
2. Trace the 51bcbbb response-validation repair and its tests. Keep safe allowlisted distinctions, add bounded actionable classification and operation context for unsupported codes without copying arbitrary response content.
3. Validate required write fields where declared map/schema evidence permits, so missing/null text is rejected before network I/O while valid explicit text continues to work.
4. Test success, no_text, missing_scope, unsupported/malformed errors, secret-like values, and both positional/explicit reply forms. Update the documented diagnostic contract and generated assets if affected.

## Quality Gate

- Invalid missing text produces a local diagnostic and zero HTTP requests; valid explicit text produces the intended reply payload.
- Unsupported upstream errors remain actionable without leaking tokens, message bodies, or arbitrary response values.
- Run targeted driver-http and declared-driver tests, cargo fmt --all --check, and applicable generated doc/skill checks from CLAUDE.md.

## Considerations

The report did not observe the upstream no_text response: it is a hypothesis, not a proven cause. Existing commit 51bcbbb already handles API refusals; extend its safe contract. No active mission covers this atomic repair (mid_term_plan:no).
