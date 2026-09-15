---
created_at: 2026-09-16T06:56:47+09:00
author: a@qmu.jp
assignees: [a@qmu.jp]
depends_on:
feedback: [20260916065638-fb-slack-worked-example-in-qfs-skill-posts-an-empty-message-and-the-correct-spelling-is-ungrammatical.md]
merge_policy:
verification_handoff: 
---

# Make Slack message examples bind text explicitly

## Overview

Repair the executable Slack posting contract so the shipped example reaches chat.postMessage with its intended text. diagnosis_first:true. The reported pre-VALUES column forms are not evidence that the existing VALUES (text) form is unavailable: declared_driver.rs already tests that form.

## Policies

- Follow CLAUDE.md Rust workspace, generated documentation/skills and release version rules.
- Preserve selected-account isolation and avoid credential disclosure.

## Key Files

- packages/qfs/crates/skill/assets/SKILL.md
- packages/qfs/crates/skill/assets/examples/slack.qfs
- packages/qfs/crates/skill/assets/examples/slack_driver.qfs
- packages/qfs/crates/qfs/src/declared_driver.rs
- docs/cookbook/slack.md

## Implementation Steps

1. Reproduce the shipped positional example and existing VALUES (text) spelling with the current parser and a recording Slack fixture; localize which examples construct a null text field.
2. Use the supported explicit-column spelling in executable examples, describe guidance and cookbook. Add parser support only if reproduction proves a necessary supported column name remains inaccessible; do not adopt an unnecessary quoting design from the report.
3. Exercise the example through COMMIT against a recording transport and assert channel/text payload and successful response, including thread replies and a schema column-order change. Preview-only validation is insufficient.
4. Regenerate affected docs/skills and bump all four plugin version fields when taught surfaces change, following CLAUDE.md.

## Quality Gate

Every shipped Slack posting example parses and commits its intended text on fixtures. Explicit binding is independent of schema ordering. Run targeted parser/declared-driver/skill tests and generated-doc/skill checks.

## Considerations

Error classification and local missing-text diagnostics are already queued by issue #108 / PR #112; do not duplicate that work. Current source demonstrates VALUES (text), so the reporter-proposed quoting mechanism is only a hypothesis.
