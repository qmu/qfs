---
created_at: 2026-09-26T14:27:46+09:00
status: done
author: a@qmu.jp
assignees: [a@qmu.jp]
depends_on:
feedback: [20260926142729-slack-insert-drops-thread-ts-so-a-thread-reply-lands-in-the-channel.md, 20260926142734-ship-and-document-the-slack-thread-reply-write-replies-insert-map.md]
merge_policy:
verification_handoff: 
claim: work-20260926-143118
---

# Preserve Slack thread reply intent and reject dropped INSERT columns

## Overview

Resolve qmu/qfs#139 and #140 together: a Slack thread reply must preserve its parent through both the messages INSERT and the shipped replies INSERT, and a declared INSERT must never silently discard a supplied column. This is one bounded repair of message write intent, not a mid-term mission.

## Policies

- `workaholic:implementation` / `policies/directory-structure.md` — conventional project layout.
- `workaholic:implementation` / `policies/coding-standards.md` — follow existing Rust and declaration conventions.

## Key Files

- `packages/qfs/crates/skill/assets/examples/slack_driver.qfs` — messages map currently forwards only channel/text; replies is SELECT-only.
- `plugins/qfs/skills/qfs-slack/SKILL.md` — replace the current thread-write limitation with runnable read/reply/read-back examples.
- `packages/qfs/crates/core` — declared map expansion and INSERT preview validation.
- Existing declaration and HTTP wire tests — prove payloads and rejection before side effects.

## Implementation Steps

1. diagnosis_first: true. Reproduce the messages INSERT dropping thread_ts and replies INSERT lacking a write map using existing offline fixtures. History: the declared-driver migration removed the compiled replies writer; current map forwards only channel/text.
2. Forward a supplied non-null thread_ts through messages INSERT while preserving root posts without a parent. Include reply_broadcast only if supported consistently.
3. Add the shipped replies INSERT map with thread_ts from the path. Preserve normal connection routing.
4. Detect supplied INSERT columns the declaration does not consume, refusing them before commit or explicitly warning during preview. Prefer refusal to prevent false success; ensure row-object forwarding and valid non-Slack declarations remain supported.
5. Document reading a thread, writing a reply and verifying the returned parent. Remove claims that threads require an undiscoverable local map. If declaration refresh is required, document the supported refresh workflow.
6. Run targeted parser/core/declaration/HTTP tests and the repository's relevant plugin validation.

## Quality Gate

**Acceptance criteria**

- messages INSERT with thread_ts sends the exact parent; absent or null thread_ts retains root-post behavior.
- replies INSERT works from the shipped declaration without a local CREATE MAP.
- A supplied, unconsumed column cannot silently commit successfully.
- Skill examples cover thread read, reply and read-back confirmation.

**Verification method**

- Offline tests inspect outbound Slack payloads for root, messages-thread and replies-thread variants.
- Negative test verifies unconsumed columns are rejected before HTTP side effects (or are explicitly warned in preview if refusal proves incompatible).
- Existing declaration tests remain green, including non-Slack and whole-row maps.

**Gate**

- All targeted tests and plugin validation pass; report live Slack verification separately, because this session has not authorized posting messages.

## Considerations

Generic map field-use analysis must account for row-object forwarding and nested expressions; avoid a Slack-only allowlist masquerading as generic validation. The reporter's suggested map is a hypothesis to validate against actual executor semantics. The two feedback records are carried together because both write surfaces share the same regression and test fixture.

## Final Report

Implemented messages INSERT with optional `thread_ts` and a shipped replies INSERT whose parent
comes from `path.ts`. Account routing remains covered by mocked wire tests. Named unconsumed
columns now fail with `unused_map_columns` before any wire write; the diagnostic includes column
names and excludes values. Nested expressions, whole-row forwarding and lookup-consumed fields
are covered by the generic evaluator and existing non-Slack/lookup regressions.

The cookbook and generated Slack skill include thread read, preview, reply commit, read-back and
`qfs declare slack --commit` refresh examples. Binary version is 0.0.140; plugin version is 0.24.1.
No live Slack messages were sent. Deployment and live Slack verification are not claimed.

Verification: qfs-exec 123 tests; qfs declared-driver 97 tests; parser/core/driver-http/xtask suites
479 tests; plugin distribution validation and 12 negative fixtures; generated-doc and generated-skill
checks. Root, null-parent, explicit-parent, replies-path, account binding and unused-column
zero-HTTP regression cases pass.

### Discovered Insights

- Absent map row fields already evaluate to null. Slack explicitly documents that JSON null takes
  the argument's default (https://docs.slack.dev/apis/web-api/), so forwarding optional thread_ts
  preserves root posts without adding a service-specific evaluator rule.
- The lowered scalar reference walker already covers nested objects, arrays and whole-row use.
  Reusing it avoids maintaining a second expression grammar for column-use analysis.
