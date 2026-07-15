---
created_at: 2026-07-08T00:00:00+09:00
author: a@qmu.jp
type: enhancement
layer: [Infrastructure, Domain]
effort:
commit_hash: d45889b
category: Changed
depends_on:
mission:
---

# Slack file-share channel/DM linkage

## Overview

The Slack driver can now list the workspace file namespace (`/slack/<ws>/files`) and download one
file by id (`/slack/<ws>/files/<file_id>`), and file rows carry a `created` epoch-millis timestamp.
What it still CANNOT do is prove **which channel or DM a file was shared in**. A file row exposes the
uploader (`user`) and `created` time, but not the `channels`/`ims`/`groups` share targets Slack
attaches to a file. So a request like "grab the latest file someone dropped in my DM" cannot be
answered objectively — inferring "latest in my DM" from uploader + created time is a guess, and a
prior session used that guess to overwrite the wrong Drive file (see the carry origin in
`20260707175424-resume-qfs-slack-drive-safety.md`).

This ticket adds the file-share linkage so a DM/channel file lookup is backed by real Slack metadata,
not inference.

## Policies

- `workaholic:design` — a qfs path must be self-explanatory; a path that cannot prove "latest in this
  DM" must not be presented as if it did.
- `workaholic:implementation` / `policies/coding-standards.md` — keep the Slack SDK/vendor JSON behind
  the driver DTO/read/client seams; expose only owned columns.
- `workaholic:safety` — a file lookup that feeds a live cloud write must resolve from the actual
  file-share event, not from uploader + created heuristics.

## Key Files

- `packages/qfs/crates/driver-slack/src/dto.rs` — `FileDto`; add the share targets (channel/im/group
  ids) as owned columns.
- `packages/qfs/crates/driver-slack/src/read.rs` — `decode_files`; decode `channels`/`ims`/`groups`.
- `packages/qfs/crates/driver-slack/src/client.rs` — `files.list` already pages; consider a
  `channel=` / `user=` filter param, or a per-DM file listing that resolves the IM channel first.
- `packages/qfs/crates/driver-slack/src/path.rs` — a possible `/slack/<ws>/dms/<user>/files` node.

## Implementation Steps

1. Decide the addressable surface: either a filter column on `/slack/<ws>/files` (`where channel ==`)
   or a dedicated `/slack/<ws>/dms/<user>/files` / `/slack/<ws>/<#channel>/files` node.
2. Decode the file-share targets from `files.list` / `files.info` into owned `FileDto` columns.
3. For a DM file lookup, resolve the IM channel via `conversations.open` (already implemented) and
   filter `files.list` to that channel, so "latest in my DM" is provably that DM's newest share.
4. Cover the decode + the DM-file resolution with hermetic mock tests.
5. Only after the surface is real, document it in the Slack cookbook (`docs/cookbook/slack.md`) — and
   regenerate the skill. Until then the cookbook must NOT teach a DM-file lookup.

## Quality Gate

- A DM/channel file lookup returns only files actually shared in that DM/channel, proven by a mock
  test with two files in different channels.
- No doc or skill claims "latest file in my DM" until this lands.
- `cargo test -p qfs-driver-slack`.
