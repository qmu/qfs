---
created_at: 2026-08-19T06:15:00+00:00
author: a@qmu.jp
assignees: [a@qmu.jp]
depends_on:
mission:
merge_policy: review
verification_handoff:
---

# The chatwork/room type drops five fields the endpoint delivers

## Overview

Salvaged from PR #65 ("Describe a declared driver's real surface") when it was closed on
2026-08-19. #65 was a third, independent implementation of ticket
`20260728085253-declared-driver-undiscoverable-through-describe.md`, which #46 had already
landed on `main`. Its describe work is therefore redundant, but one part of it is not: its
`chatwork/room` type carries fields the shipped one omits, and its reasoning for carrying them
is the part worth keeping.

## The argument #65 made

> The types carry EVERY field the endpoint delivers, not a convenient subset: a column the type
> omits is a column no caller can reach, because the `OF` projection drops it before the rows
> leave the view.

That is a real property of the declared-driver design, not a preference. `GET /rooms` returns
these five fields and the shipped `chatwork/room` in
`packages/qfs/crates/skill/assets/examples/chatwork.qfs` does not name them, so no query can
reach them:

| Column | Type | What it answers |
| --- | --- | --- |
| `sticky` | bool | is the room pinned |
| `icon_path` | text | the room's icon |
| `mytask_num` | int | how many tasks in the room are assigned to me |
| `file_num` | int | how many files the room holds |
| `task_num` | int | how many tasks the room holds |

`mytask_num` is the one with a query behind it — "which rooms are waiting on me" is currently
unanswerable through qfs.

## Implementation

1. Add the five columns to `CREATE TYPE chatwork/room` in
   `packages/qfs/crates/skill/assets/examples/chatwork.qfs`, keeping the shipped column order
   convention (identity, then metadata, then counts).
2. Check whether the `/chatwork/rooms` view's projection needs to name them (the shipped view
   uses an explicit `SELECT`; a widened type with an unchanged projection would surface the new
   columns as drift rather than data).
3. Update `docs/cookbook/chatwork.md` if a recipe becomes worth teaching from the new columns —
   a "rooms waiting on me" recipe over `mytask_num` is the obvious one — and regenerate the
   skill with `cargo run -p xtask -- gen-skills`.
4. Bump the plugin version in all four fields if a taught surface changes (CLAUDE.md's rule).

## Considerations

- **Do not port #65's view rewrite.** #65 also restyled the message views from an explicit
  `SELECT … AS …` projection to `|> EXTEND account_id = account.account_id, …`. `main` carries
  the `SELECT` form from #46 and it is under the cookbook ratchet; changing the style is a
  separate decision with no behavioural gain, and mixing it into this change would make the
  column addition hard to review.
- **Conformance is the check.** The declaration reconciles its `OF` contract against what the
  live API delivers, so a field named wrongly surfaces as drift rather than silently. Verify
  against the real Chatwork endpoint's response shape before assuming the five names above are
  exactly right — they are read from #65's declaration, not from the API docs.
- #65's branch is `work-20260817-023958`; its full diff stays readable on the closed pull
  request if more of it turns out to be worth salvaging.

## Key Files

- `packages/qfs/crates/skill/assets/examples/chatwork.qfs`
- `docs/cookbook/chatwork.md`
- `plugins/qfs/skills/qfs-chatwork/SKILL.md` (generated)
