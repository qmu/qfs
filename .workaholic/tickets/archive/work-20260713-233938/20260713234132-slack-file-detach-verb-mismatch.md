---
created_at: 2026-07-13T23:41:33+09:00
author: a@qmu.jp
type: bugfix
layer: [Domain]
effort: 0.5h
commit_hash: d828abe
category: Changed
depends_on:
mission: qfs-capability-tryout-file-handling-transformation-and-platform-hardening
---

# Slack file detach is unreachable: file node lacks REMOVE, cookbook form is capability-rejected

## Overview

Found live (L60 Slack-bytes round, owner-attended): a Slack file **cannot be deleted** through any
`qfs run` form. The cookbook teaches `remove /slack/<ws>/files/<id>`, but the file node
`SlackNode::File` advertises only `Verb::Select`, so REMOVE is capability-rejected; the workspace
`SlackNode::Files` namespace advertises `Verb::Rm` (not `Verb::Remove`), so `remove /slack/<ws>/files
…` is also rejected; and `rm` is not a `qfs run` grammar verb (parse error). Meanwhile the effect
decoder `decode_remove` already produces `SlackEffect::DeleteFile` for `SlackNode::Files` (reading an
`id` column) — a ready decoder with no reachable capability gate. The upload/list/download half is
live-proven; only detach is blocked.

## Key Files

- `packages/qfs/crates/driver-slack/src/lib.rs` — `caps_for`: `SlackNode::File` is `[Select]`.
- `packages/qfs/crates/driver-slack/src/effect.rs` — `decode_remove`: handles `SlackNode::Files`
  (id from a column) but not the path-addressed `SlackNode::File { id }`.
- `packages/qfs/crates/driver-slack/src/tests.rs` — capability + decode tests.
- `docs/cookbook/slack.md` — the taught `remove /slack/<ws>/files/<id>` detach recipe.

## Implementation Steps

1. `caps_for`: give `SlackNode::File { .. }` the capabilities `[Verb::Select, Verb::Remove]` so the
   path-addressed `remove /slack/<ws>/files/<id>` (the cookbook form) passes the capability gate.
2. `decode_remove`: add a `SlackNode::File { id }` arm → `SlackEffect::DeleteFile { id: id.clone() }`
   (the id comes from the path, no column needed). Keep the existing `SlackNode::Files` (id-column)
   arm.
3. Add a hermetic test: a `remove /slack/<ws>/files/<id>` statement decodes to `DeleteFile { id }`
   and the file node advertises `Verb::Remove`; assert the mock `files.delete` call fires.
4. Confirm the cookbook `remove /slack/<ws>/files/<id>` recipe is accurate (it now works); regenerate
   skills if any wording changes.

## Considerations

- `DeleteFile` stays irreversible (needs `--commit-irreversible`) — unchanged.
- Leave the namespace `SlackNode::Files` `Verb::Rm` capability as-is (it serves the shell `cp`/`rm`
  blob shorthands and the upload `cp`); this ticket makes the **path-addressed `remove`** work.

## Policies

`layer: [Domain]` → `workaholic:implementation` (capability/decoder consistency; a declared
capability must be invocable).

## Quality Gate

- **Acceptance**: `remove /slack/<ws>/files/<id>` previews as an irreversible file delete and, on
  `--commit-irreversible`, fires `files.delete`; the file node describes `Verb::Remove`.
- **Verify**: `cargo test -p qfs-driver-slack`, `cargo clippy -p qfs-driver-slack --all-targets -- -D
  warnings`, `cargo fmt`, `gen-docs --check`, `gen-skills --check`; then the live detach of the
  round's uploaded file.

## Final Report

Development completed as planned. The file node `/slack/<ws>/files/<id>` now advertises
`[Select, Remove]` and `decode_remove` handles the path-addressed `SlackNode::File { id }` →
`DeleteFile { id }` (id from the path). Verified: `cargo test -p qfs-driver-slack` (56 lib + 30
tests, incl. the new `remove_file_by_path_decodes_to_delete_file…` lock), clippy/fmt clean, gen-docs
+ gen-skills in sync. **Live-proven (owner-attended, v0.0.62):** `remove /slack-me/qmu/files/F0BH1A78P9P
--commit --commit-irreversible` deleted the round's uploaded file; a read-back confirmed it was gone.
Completes the L60 attach/detach acceptance together with the (earlier-proven) upload/list/download
half.

### Discovered Insights

- **Insight**: the capability enum distinguishes `Verb::Rm` from `Verb::Remove`, and the Slack Files
  namespace advertised `Rm` while the effect decoder and the cookbook used `Remove` — so a declared
  capability was never invocable via `qfs run` (`rm` is not a query verb; only the shell has it).
  **Context**: a capability that no grammar can invoke is dead. When a driver adds a blob-delete,
  assert an actual `remove <path>` statement decodes end-to-end, not just `check_capability(Verb::Rm)`
  — the latter passed for two versions while every real delete was rejected.
