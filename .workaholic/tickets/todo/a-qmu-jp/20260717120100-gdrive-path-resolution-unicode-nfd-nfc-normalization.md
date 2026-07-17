---
created_at: 2026-07-17T12:01:00+09:00
author: a@qmu.jp
type: bugfix
layer: [Domain]
effort:
commit_hash:
category: Changed
depends_on:
mission:
---

# gdrive path resolution: absorb Unicode NFD/NFC normalization differences

## Overview

Observed live (2026-07-17, same session as ticket 20260717102000): a Drive file whose stored name
is **NFD-normalized** (e.g. a Japanese name with decomposed dakuten, as macOS clients often
upload) cannot be addressed by the name shown in a listing. The listing renders the name; the
operator (or an agent) pastes it into a single-file path — the terminal typically produces the
**NFC** form — and the walk's exact-match Drive query (`name = '<seg>'`,
`resolve_node` in `packages/qfs/crates/driver-gdrive/src/read.rs`) finds nothing: `not_found`
for a file that is plainly in the listing. The same asymmetry affects the write walk
(`ClientResolver::existing`/`folder_id`) and the `where name == '…'` selector resolution, since
they ride the same walk.

## Expected behavior

Path-segment (and name-selector) resolution absorbs the NFC/NFD difference: a segment that is
canonically equivalent (Unicode NFC == NFC(stored name)) resolves to the node. Suggested shape:

1. Query Drive with the segment as written; on no hit, retry with the other normalization form
   (or query broadly and compare NFC-normalized names locally).
2. Keep the ambiguity guard: if normalization makes TWO children equivalent to the segment,
   refuse as `ambiguous_target` (never first-hit).
3. Apply uniformly to `resolve_node`, `ClientResolver::child_id`, and the REMOVE/UPDATE
   name-selector child resolution, so read and write paths never disagree.

## Key Files

- `packages/qfs/crates/driver-gdrive/src/read.rs` — `resolve_node`, `ClientResolver`.
- `packages/qfs/crates/driver-gdrive/src/effect.rs` — name-selector child resolution.

## Policies

- `workaholic:implementation` / honest-surfaces — a name a listing shows must be addressable;
  `not_found` for a visible file is a machine-checkable domain gap.
- `workaholic:design` — normalization must not weaken the ambiguity fail-closed guard.

## Quality Gate

1. Hermetic tests over the mock client: an NFD-stored name resolves from an NFC-written path
   (and vice versa); two normalization-equivalent children refuse as ambiguous.
2. The REMOVE `where name == '…'` selector resolves an NFD-stored child from an NFC value.
3. `cargo test --workspace`, clippy `-D warnings`, fmt, gen-docs/gen-skills checks all pass.
