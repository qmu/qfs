---
created_at: 2026-07-25T14:30:00+09:00
author: a@qmu.jp
assignees: [a@qmu.jp]
type: bugfix
layer: [Domain]
effort:
commit_hash:
category:
depends_on:
mission: a-where-predicate-is-honored-or-refused-never-dropped
---

# The cookbook ratchet only parses — it must typecheck against the describe registry

## Overview

`packages/qfs/crates/test/tests/cookbook_skills.rs` is the "verified-true" ratchet over every
`docs/cookbook/*.md` article: it extracts each ` ```qfs ` fenced statement and asserts it **parses**.
Its own doc says a skill "can never teach an agent a statement the binary rejects" — but parsing is
the weakest possible reading of that promise, and it let a whole article ship false.

On 2026-07-25 the release pass on branch `work-20260724-011029` found `docs/cookbook/github.md`
documenting a **fabricated** PR/issue column set: `author`, `merged_at`, `mergeable`,
`review_decision`, `checks_status`, `additions`, `deletions`, `requested_reviewers`, `reviews`,
`assignee`, `milestone`, `label`, `closed_at` — none of which exist in
`packages/qfs/crates/driver-github/src/dto.rs`. Eleven recipes named them. Every one **parsed**, so
the ratchet was green the entire time. Six of them additionally became hard **exit-2 refusals** the
moment this mission made an unpushed `where` honest, because `/github` declares `where=true` and
`GitHubReadDriver` does not declare `honors_pushed_filter` — so the executor re-applies the
predicate through the checked seam and refuses the unknown column. The article was fixed by hand;
the ratchet that should have caught it was not.

## Scope

**In scope:** raise the ratchet from *parses* to *typechecks against the compiled describe
registry*. For each extracted statement whose source path resolves to a driver in the binary's own
cred-free describe registry (`packages/qfs/crates/qfs/src/describe.rs`, the same registry
`gen-docs` renders from), resolve the node's schema and assert that every column named by
`where` / `select` / `group by` / `order by` / `expand` / `aggregate` exists in it.

**Out of scope:**

- Recipes over paths with no compiled describe surface (`/sql/<conn>/<table>`, `/git/<repo>`,
  declared `/chatwork` and `/cloudflare` mounts) — those need a live registration, not a compiled
  type. Skip them **explicitly and loudly**: keep a floor count of how many statements were actually
  typechecked, the way `MIN_STATEMENTS` guards the extractor today, so "skipped everything, all
  pass" cannot masquerade as green.
- Procedure/argument checking (`call github.merge(number => …)` names a param the `ProcSig` does not
  declare — a real second false-teaching class, but a separate resolve-layer check).
- Changing any article. `github.md` was corrected in the same run this ticket was minted in.

## Key Files

- `packages/qfs/crates/test/tests/cookbook_skills.rs` — the ratchet; `extract_statements` already
  isolates real runnable source, so the new work is entirely in what happens after extraction.
- `packages/qfs/crates/qfs/src/describe.rs` — `cred_free_driver` / the compiled describe registry.
  Note the standing constraint: the catalog renders from `compiled_describe_registry` only, never
  from live CONNECT-ed mounts, so this check stays deterministic and credential-free.
- `packages/qfs/crates/driver-github/src/dto.rs` — the schemas the false article contradicted.
- `packages/qfs/crates/core/src/eval.rs` — the fold that already refuses an unknown column; the
  check here should agree with it rather than re-derive a second, divergent rule.

## Policies

- `workaholic:implementation` / `objective-documentation` — a taught surface that the binary
  rejects is worse than no documentation: an agent runs it and reports the failure as the user's.
- `workaholic:development` / `qa-engineering` — the ratchet exists precisely so this class cannot
  ship; a ratchet that passes on false content is a false green.

## Quality Gate

1. A deliberately-false recipe (e.g. `/github/acme/web/pulls |> where author == 'x'`) added to a
   scratch article **fails** the test — demonstrated red before green.
2. The typechecked-statement floor is asserted, and its value is stated in the commit body.
3. Skipped paths are reported by path, not silently dropped.
4. Workspace gates green with raw exit codes: `cargo test --workspace`, `cargo clippy --workspace
   --all-targets -- -D warnings`, `cargo fmt --all --check`, `gen-docs --check`,
   `gen-skills --check`.

## Considerations

- Minted by the `/monitor` drive of this mission (run 20260725-101714) while fixing the article the
  gap let through. The article fix is done; this is the ratchet that should have prevented it.
- The check is cheap: the describe registry is already linked into the test workspace for the
  `gen-docs` anti-drift, and `Schema::column` is the whole lookup.
