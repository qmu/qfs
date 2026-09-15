---
created_at: 2026-09-16T06:56:48+09:00
author: a@qmu.jp
assignees: [a@qmu.jp]
depends_on:
feedback: [20260916065638-fb-slack-worked-example-in-qfs-skill-posts-an-empty-message-and-the-correct-spelling-is-ungrammatical.md]
merge_policy:
verification_handoff: 
---

# Make advertised Slack procedures callable on selected named mounts

## Overview

Ensure a caller can execute the procedures advertised by DESCRIBE for a hyphenated Slack mount using that same account. diagnosis_first:true. This is separate from message example correction.

## Policies

- Follow CLAUDE.md Rust workspace, generated documentation/skills and release version rules.
- Preserve selected-account isolation and avoid credential disclosure.

## Key Files

- packages/qfs/crates/parser/src
- packages/qfs/crates/qfs/src/declared_driver.rs
- packages/qfs/crates/exec/src/declared.rs
- packages/qfs/crates/skill/assets/SKILL.md
- docs/cookbook/slack.md

## Implementation Steps

1. Reproduce the advertised CALL forms on two mounts such as /slack-a and /slack-b using separate recording accounts. Inspect the parser, namespace remapping and procedure resolution before choosing a fix.
2. Make at least one discoverable CALL spelling resolve the procedure on the selected source mount, including hyphens, without routing to an unrelated default account.
3. Ensure DESCRIBE and executable skill examples use the reachable spelling. Preserve typed arguments, lookup semantics and irreversible gates.
4. Test positive and negative parsing, two-account isolation, missing mount, and every advertised procedure using fixture request assertions; regenerate generated guidance and apply required version updates.

## Quality Gate

Each advertised procedure is reachable on a named/hyphenated mount and uses only its credential. Existing default-mount forms remain valid or receive a documented migration. Run targeted parser, declared-driver and executor tests plus affected generated checks.

## Considerations

Source history 277504f binds authentication by connection path; preserve that boundary. Do not silently omit useful procedures if the existing language can express them. Error handling overlaps #108 and remains in its ticket.
