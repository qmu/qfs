---
type: Concern
concern_id: the-carried-create-account-ships-the
mission:
tickets: [20260713195008-effect-selector-channel-folder-rename.md, 20260714120000-effect-selector-uniform-migration.md, 20260714154144-general-of-type-assertion.md, 20260714182710-shell-face-slice1-ls-cat-describe-typed.md, 20260714182720-shell-face-slice2-cd-gate-enumerable-children.md, 20260714182730-shell-face-slice3-mutation-verbs-per-kind.md, 20260714182740-shell-face-type-mount-and-describe-builtin.md, 20260714220213-resume-shell-face-slices-and-report.md]
origin_pr: 41
origin_pr_url: https://github.com/qmu/qfs/pull/41
origin_branch: work-20260714-111817
origin_commit: 7752cb3
created_at: 2026-07-15T16:35:34+09:00
first_seen: 2026-07-15T16:35:34+09:00
last_seen: 2026-07-15T16:35:34+09:00
severity: low
status: resolved
resolved_by_pr:
resolved_by_commit:
---

# The carried `create-account-ships-the-core-two` concern is now half-stale

## Description

Its sub-item 2 names "`EffectNode` carries no filter" as the blocker for a filter-addressed `REMOVE /sys/accounts WHERE account == '<email>'`. That blocker is **retired** — the selector channel exists and `driver-sys` resolves the filter off it, with a test covering the Google-email case that motivated it. Sub-item 1 (the `SECRET '<ref>'` clause on `CREATE ACCOUNT`) is untouched: `create_account_stmt` still parses only `CREATE ACCOUNT <provider> '<label>' [APP …]` (see [7b72cab](https://github.com/qmu/qfs/commit/7b72cab)).

## How to Fix

Re-scope that concern's body to the `SECRET` edge alone, so its stale blocker note stops misleading readers into thinking the filter work is still pending. It stays `active` because it is only partially resolved.

## Resolution

**Done 2026-07-15** by the missions/tickets reframing (branch `work-20260715-205333`). Both claims
above were re-verified against the **v0.0.71** binary before acting, rather than taken from the
record:

- Sub-item 2's blocker is retired — `remove /sys/accounts where account == '<an email>'` previews
  with `selector: ["account"]`, stopping only at the standard destructive-set-wide commit gate.
- Sub-item 1 is untouched — `create account <provider> '<label>' secret '<ref>'` returns
  `parse_error` / `UNEXPECTED_TOKEN`; `create_account_stmt` (`parser/src/grammar.rs:2364`) reads
  only provider + label + optional `APP`.

`create-account-ships-the-core-two` was rescoped to the `SECRET` edge alone, keeps `status: active`
(that edge is real), and now hangs off the `declared-drivers-are-the-normal-way-to-add-a-service`
mission. The retired edge is recorded there with its verification rather than silently dropped, so
this concern has nothing left to carry.
