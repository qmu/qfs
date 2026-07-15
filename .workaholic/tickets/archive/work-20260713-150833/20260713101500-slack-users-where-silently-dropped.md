---
created_at: 2026-07-13T10:15:00+09:00
author: a@qmu.jp
type: bugfix
layer: [Domain]
effort:
commit_hash:
category:
depends_on:
mission: qfs-capability-tryout-file-handling-transformation-and-platform-hardening
---

# Slack `/users` read silently drops the WHERE stage (returns all rows)

## Problem (found live, round 3 of the owner-attended rounds, v0.0.59)

Against the live `/slack-me/qmu/users` listing (user-token mount, same driver as `/slack`):

- `|> where id == 'U0BFLKVB66N' |> select id, name, real_name, is_bot` → **all 31 workspace
  users**, filter ignored
- `|> where is_bot == true |> select id, name` → all 31 rows
- `|> where name == 'slackbot' |> select id, name` → all 31 rows

The `select` projection in the same pipe IS applied (requested columns only), so the pipe runs —
only the WHERE stage vanishes. Silent, not fail-loud: the caller gets a full result set shaped
exactly like a filtered one. The Slack cookbook explicitly teaches
`/slack/acme/users |> where name == 'alice' |> select id` (docs/cookbook/slack.md), so the taught
surface returns wrong rows.

## Scope to establish while fixing

- Which Slack nodes drop WHERE: `users` confirmed; check `messages`, `files`, `dms/...` too.
- Whether other drivers' read facets share the pattern (the engine should apply WHERE post-read
  even where pushdown is absent — find where the stage is lost: driver pushdown claim vs engine
  residual filter).
- drivers.md claims `where=true` pushdown for `/slack` — reconcile the claim with reality
  (gen-docs renders from the compiled registry, so the registry itself may be overclaiming).

## Fix

Apply the residual WHERE engine-side when the driver does not (or mis-)push it down, or make the
driver honor it; either way a WHERE that cannot be applied must refuse, never silently return the
unfiltered set. Add hermetic locks: a mocked Slack users listing with each literal-type filter
(text ==, bool ==) asserting filtered row counts, plus a cookbook-shape regression.

## Key files

- `packages/qfs/crates/driver-slack/` — users/messages/files read facets
- the read-path planner that decides pushdown vs residual filtering
- `docs/cookbook/slack.md` — the taught `where name ==` recipe (must become true)

## Resolution (2026-07-13, branch work-20260713-150833)

Root cause established. The Slack (and GitHub) read facet dropped the residual: `read_rows` computes
a truthful residual (`ReadPlan::list` keeps the whole predicate residual for `users`/`files`, which
have no server-side filter param) but returns only the `RowBatch`. And the driver's
`PushdownProfile { where_: true }` makes the planner push the whole WHERE into `scan.pushed.filter`
with no residual `CombineOp`, so the engine re-filters nothing either. Result: the predicate was
honored nowhere → all rows. (The gdrive/gmail facets look identical but happen to work because their
BACKENDS filter server-side — Drive `q`, Gmail `q` — so their over-return is already narrowed;
Slack `users.list` / GitHub list APIs have no such param, exposing the gap.)

Fix (matches the proven S3/SQL facets, which DO apply their residual at the seam): both the Slack
and GitHub facets now enforce the pushed `WHERE` over the returned rows via a shared
`apply_pushed_filter` (→ `qfs_exec::apply_residual`), the t20 over-fetch-then-filter invariant. Every
returned row must satisfy the predicate; a filter can never silently return the unfiltered set. It
is idempotent where the backend already narrowed, so it is safe for every Slack/GitHub node
(`users`/`files`/`messages`/`dms` and all GitHub collections), not just `users`.

- Scope established: the fix is node-agnostic (applied at the facet seam), so `messages`/`files`/
  `dms` are covered too, and the sibling GitHub facet (same latent gap for non-param filters like
  `number == 5`) is fixed in the same change.
- `drivers.md` `where=true` for `/slack` is now honest: WHERE is genuinely honored (locally at the
  seam where the backend can't push), so the taught `docs/cookbook/slack.md` `where name ==` recipe
  returns the right rows. The `PushdownProfile` is unchanged, so gen-docs output is unchanged.

New hermetic locks (`read_facets` tests): `slack_users_facet_applies_a_text_where_not_the_whole_directory`
(text `id ==`) and `slack_users_facet_applies_a_bool_where` (bool `is_bot ==`), each asserting the
filtered row count against a two-user mock directory.
