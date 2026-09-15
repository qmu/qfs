---
created_at: 2026-08-19T06:15:00+00:00
status: done
author: a@qmu.jp
assignees: [a@qmu.jp]
depends_on:
mission:
merge_policy: review
verification_handoff:
claim: work-20260818-210119
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

## Final Report

Development completed as planned. The `chatwork/room` type now names every field `GET /rooms`
delivers, so the five previously-unreachable columns can be queried.

- **Widened the type** (`packages/qfs/crates/skill/assets/examples/chatwork.qfs`): added `sticky`
  (bool) and `icon_path` (text) as room metadata, and `mytask_num` / `file_num` / `task_num` (int)
  among the counts, keeping the identity → metadata → counts → timestamp order convention. A comment
  above the type records WHY every delivered field must be named (the `OF` projection drops any the
  type omits before the rows leave the view).
- **No view change needed.** `/chatwork/rooms` is `… |> DECODE json` with no explicit `SELECT`, so
  the widened `OF chatwork/room` contract is exactly what shapes the output — the new columns flow
  through automatically. (Step 2's concern applies only to a view with an explicit projection; this
  one has none.)
- **Added the recipe the value is behind** (`docs/cookbook/chatwork.md`): "Find the rooms waiting on
  a task of yours" over `mytask_num` — the query that was unanswerable before. Regenerated the skill
  (`gen-skills`) and bumped all four plugin `version` fields (0.21.1 → 0.21.2, patch: additive
  recipe, not a break) since the taught surface changed.
- **Did not port #65's view rewrite** (its `SELECT … AS` → `|> EXTEND` restyle), per the ticket's
  Considerations — `main` carries the `SELECT` form from #46 under the cookbook ratchet, and mixing
  a style change into a column addition would make it hard to review.

### Discovered Insights

- **Insight**: `/chatwork/rooms` carries no explicit projection, so widening its `OF` type is the
  ENTIRE change needed to surface new columns — there is no `SELECT` list to keep in sync. A view
  WITH an explicit projection (the message views) would need both edited or the new column surfaces
  as drift; knowing which shape a view has decides whether a type widening is one edit or two.
  **Context**: the declared-driver `OF` contract is the projection when the view body does not carry
  its own `SELECT`.

### Verification caveat (not a blocker)

The five field names are read from #65's closed-PR declaration, NOT from the live Chatwork API. The
declared-driver design reconciles the `OF` contract against what the API actually delivers, so a
misnamed field surfaces as drift rather than silent data at first live read — it fails safe. A live
`GET /rooms` response was not reachable from this environment (no Chatwork account/network), so the
names should be confirmed against a real response when convenient; the change itself is additive and
low-risk either way.
