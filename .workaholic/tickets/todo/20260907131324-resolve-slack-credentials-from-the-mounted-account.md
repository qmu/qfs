---
created_at: 2026-09-07T13:13:24+09:00
author: a@qmu.jp
assignees: [a@qmu.jp]
depends_on:
---

# Resolve Slack credentials from the mounted account

## Overview

The developer wants a coding agent to post as their own user in several Slack workspaces by choosing a connection path. After registering two tokens and connecting each path with `--account`, both reads and writes must use that path's account without an extra `--secret` or mutable active-profile setting. The current Slack AUTH BEARER declaration does not consume the mount's account label.

## Policies

- `workaholic:implementation` / `policies/directory-structure.md`: keep the repair in the existing QFS credential boundary.
- `workaholic:implementation` / `policies/coding-standards.md`: follow the repository's Rust conventions; no TypeScript changes needed.
- `workaholic:implementation` / `policies/test.md`: verify outbound authentication using hermetic requests, not real Slack posts.
- `workaholic:design` / `policies/modeless-design.md`: a path selects the account without global selection state.
- `workaholic:design` / `policies/defense-in-depth.md`: an unavailable selected account must never fall back to another account.

## Key Files

- `packages/qfs/crates/qfs/src/declared_driver.rs`: shared credential resolution for reads and writes.
- `packages/qfs/crates/qfs/src/commit.rs`: per-mount apply registration.
- `packages/qfs/crates/qfs/src/shell.rs`: per-mount read registration.
- `packages/qfs/crates/skill/assets/examples/slack_driver.qfs`: shipped Slack definition.
- `docs/cookbook/slack.md`: connection and user-token instructions.

## Related History

The Slack implementation moved from a compiled driver to a declared driver, while its documented multi-account connection contract stayed in place.

- `.workaholic/tickets/archive/work-20260803-213737/20260724014200-retire-the-compiled-slack-driver.md`
- `.workaholic/tickets/archive/work-20260711-121525/20260711010500-docs-slack-user-token-posting-guide.md`

## Implementation Steps

1. Reproduce and localize account-label loss using the shipped Slack declaration and distinct test credentials.
2. Repair the shared authentication boundary so existing stored declarations and new connections both honor account binding; preserve deliberate legacy secret references.
3. Prove separate paths choose separate bearer tokens for reads and writes, including missing/revoked credentials and a populated default account.
4. Clarify path selection in the cookbook, regenerate agent skills, and update affected binary/plugin versions.

## Quality Gate

**Acceptance criteria**

- A user connects `/slack-a` with account `work-a` and `/slack-b` with `work-b`, then chooses only the path for subsequent reads and posts.
- The recorded HTTP requests carry exactly the selected account's token.
- Missing/revoked selected credentials produce errors before an outbound request, even when a default credential exists.
- Existing explicit secret references retain their documented behavior.

**Verification method**

- Decided: hermetic regression tests with isolated vaults and mock HTTP, avoiding real Slack messages.
- Run Cargo workspace tests, the serialized config-home isolation suite, clippy, formatting, generated documentation/skill checks, and migration check as applicable.

**Gate**

- The focused regression fails before the fix and passes afterward; required checks pass or unrelated environmental limitations are reported with evidence.

## Considerations

- Changing only the example AUTH declaration may leave already-stored driver definitions broken; inspect the runtime boundary before choosing the repair (`declared_driver.rs`).
- The token determines Slack workspace and sender; the path's workspace segment does not independently select credentials (`slack_driver.qfs`).
- Scope is this requested fix only; existing unrelated queued work is not part of this unit.
