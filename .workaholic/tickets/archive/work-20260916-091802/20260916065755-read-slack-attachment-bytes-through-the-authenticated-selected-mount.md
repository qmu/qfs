---
created_at: 2026-09-16T06:57:55+09:00
status: done
author: a@qmu.jp
assignees: [a@qmu.jp]
depends_on:
feedback: [20260916065746-fb-support-authenticated-slack-attachment-downloads-through-the-selected-qfs-mount.md]
merge_policy:
verification_handoff: 
claim: work-20260916-091802
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

## Final Report

Added the discoverable `/slack/<workspace>/files/<file>/content` view, including named-mount remapping and a typed `content: bytes` result. Its explicit QFS wire primitive resolves the file ID with `files.info`, validates the returned HTTPS `files.slack.com/files-pri/` destination, and downloads through the same selected account. Both exchanges refuse every redirect. The transport capability fails closed when an injected client does not implement no-redirect sends; generic FOLLOW remains unchanged and credential-free.

The initial source trace confirmed that the shipped declaration exposed file metadata and deletion but no byte read. The implementation preserves the existing selected-account credential adapter rather than adding a credential store or allowing arbitrary authenticated URLs. External/remote file hosts and redirects remain unsupported, and existing installations must reinstall the current Slack declarations to discover the new view. The cookbook and shipped operating instructions now show listing a PDF and copying its content through one mount. Binary version is 0.0.132; all four plugin version fields are 0.22.4.

### Verification

- HTTP tests: 57 unit tests and 3 real-loopback wire tests passed, including a bearer-bearing same-host redirect that reached no second socket.
- Named-mount fixtures prove listing → file identity → exact binary PDF bytes through two separate accounts, missing-account refusal before HTTP, inaccessible-file errors, and DESCRIBE's bytes schema. Malicious host, HTTP, userinfo, port, path traversal and redirect fixtures fail closed; response error text and tokens do not escape.
- `cargo test --workspace`: 2810 passed, 2 existing ignored. XDG-unset serial qfs library suite: 532 passed, 1 existing ignored. Existing generic FOLLOW isolation tests passed within the workspace suite.
- Workspace clippy with warnings denied, formatting, generated-doc and generated-skill checks, the plugin checker and its 11 negative fixtures, and the VitePress documentation build passed.
- The host's full `/tmp` was isolated using a private user/mount namespace bound to project-owned temporary storage; no host mount or system configuration changed. No live account demonstration or deployment was performed.

### Discovered Insights

- **Insight:** A provider-scoped transport capability can extend a declared view without widening the generic FOLLOW credential boundary or adding query grammar.
  **Context:** The existing mount credential adapter and declared-type descriptions supply account isolation and discoverability; tests must exercise both the mounted read and the real redirect policy rather than only a mocked download response.
