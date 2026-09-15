---
created_at: 2026-09-16T06:54:17+09:00
status: done
author: a@qmu.jp
assignees: [a@qmu.jp]
depends_on:
feedback: [20260916065407-fb-make-slack-write-failures-actionable-instead-of-collapsing-upstream-errors-to-service-rejected.md]
merge_policy:
verification_handoff: 
claim: work-20260916-071056
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

## Final Report

Implemented the diagnostic repair. Recording HTTP fixtures drove the real CLI spelling through
the mounted declared driver for both channel messages and a locally installed reply map. Before
enabling validation, `VALUES ('hello')` produced `{"channel":"C1","text":null}`; explicit
`VALUES (text) ('hello')` produced `{"channel":"C1","text":"hello"}`. Reply bodies additionally
contained `"thread_ts":"123.456789"` in both cases. Explicit null also produced null text.
These are synthetic fixtures; the original live upstream error remains unobserved.

The exact Slack API composition now configures a JSON content requirement for `chat.postMessage`.
Missing/null/empty text-only payloads fail before credential lookup or HTTP, naming the operation
and explicit-column correction. Nonempty blocks, attachments and markdown_text are alternatives;
other endpoints and non-JSON encodings keep their existing behavior. This follows
[Slack's documented contract](https://docs.slack.dev/reference/methods/chat.postMessage/).

The existing response allowlist now includes no_text, msg_too_long and invalid_blocks. Application
errors include a closed operation label and corrective guidance. Unknown string codes remain
service_rejected with an explicit withheld-code explanation; malformed error fields are distinct.
Response bodies, unknown error values, request content and URL parameters never enter these errors.
No retry behavior changed. The cookbook and generated Slack skill describe the diagnostic;
plugin 0.22.4 and qfs 0.0.132 carry the change.

Validation: 54 driver-http tests and 87 declared-driver tests passed, including the CLI
message/reply positional-versus-explicit regression with zero HTTP requests for invalid text.
The pre-validation recording test also passed independently. cargo fmt --all --check,
xtask gen-docs --check, xtask gen-skills --check, and scripts/check-plugin.py passed.
No live Slack message was sent and no deployment was performed.

### Discovered Insights

- A read view for replies does not supply a reply write map. The fixture installs its own map
  and verifies its parent timestamp; this repair does not introduce a shipped reply surface.
- A required-text check alone would reject valid rich messages. Validation uses alternative
  content fields and is limited to the JSON POST contract selected at the Slack composition seam.
