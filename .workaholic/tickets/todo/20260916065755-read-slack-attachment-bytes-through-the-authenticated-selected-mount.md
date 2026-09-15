---
created_at: 2026-09-16T06:57:55+09:00
author: a@qmu.jp
assignees: [a@qmu.jp]
depends_on:
feedback: [20260916065746-fb-support-authenticated-slack-attachment-downloads-through-the-selected-qfs-mount.md]
merge_policy:
verification_handoff: 
---

# Read Slack attachment bytes through the authenticated selected mount

## Overview

Add a discoverable Slack-scoped file-byte read using the selected mount account so an agent can retrieve an attached PDF without requesting a second upload. This is one end-to-end capability; mid_term_plan:no.

## Policies

- Follow CLAUDE.md Rust workspace, generated documentation/skills and release version rules.
- Preserve selected-account isolation and avoid credential disclosure.

## Key Files

- packages/qfs/crates/skill/assets/examples/slack_driver.qfs
- packages/qfs/crates/qfs/src/declared_driver.rs
- packages/qfs/crates/driver-http/src
- docs/cookbook/slack.md
- packages/qfs/crates/skill/assets/SKILL.md

## Implementation Steps

1. Trace current file listings, declared wire confinement, connection credential selection (277504f), byte transport and generic FOLLOW isolation. Confirm the missing read surface with a fixture before extending it.
2. Implement a Slack-scoped byte-read operation selected by mount/account and file identity. Preserve Slack file/channel access checks and keep credentials inside the transport.
3. Constrain authenticated destinations and redirect behavior: reject arbitrary hosts and ensure credentials cannot cross an untrusted redirect. Generic FOLLOW continues forwarding no credentials.
4. Expose the read through DESCRIBE and an executable Slack cookbook/skill example that lists an attached PDF then downloads intact bytes through the same mount.
5. Test PDF byte equality, binary content, inaccessible/missing files, two-account isolation, malicious URL and redirect cases with recording fixtures. Regenerate docs/skills and update plugin versions when taught surfaces change.

## Quality Gate

Listing and authenticated read yield the exact PDF fixture bytes using the same account; forbidden files produce clear errors. No arbitrary-host or redirect token forwarding occurs and FOLLOW isolation remains unchanged. Run driver-http and declared-driver tests plus generated-doc/skill checks. A live account demonstration may supplement fixtures only where credentials and a suitable file are available.

## Considerations

The declared driver currently documents downloads as unsupported because FOLLOW is credential-free. Keep that boundary and add a scoped capability; do not treat arbitrary private URLs as authorization. No active mission covers this independent capability.
