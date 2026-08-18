---
created_at: 2026-07-25T14:30:00+09:00
status: done
author: a@qmu.jp
assignees: [a@qmu.jp]
type: bugfix
layer: [Domain]
effort:
commit_hash:
category:
depends_on:
mission:
claim: work-20260816-184034
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

## Queue provenance — the `mission:` stamp was cleared on 2026-08-12

This ticket was minted under the mission **`a-where-predicate-is-honored-or-refused-never-dropped`**, which closed `achieved` while the ticket
itself stayed unfinished. `plan-units.sh` excludes any mission-stamped ticket from the developer's
backlog **without checking whether that mission is still active** (`plan-units.sh:432` — a non-empty
mission relation is excluded as `mission_member`), and only *active* missions are offered as mission
units. A ticket stamped with a closed mission is therefore reachable by neither path, and this one
had been invisible to every `/drive` survey since the close.

The stamp is cleared so the ticket returns to the ordinary backlog — the same correction
`20260804173000` received when its own mission closed. The provenance lives here in prose instead.

**Still-open evidence (verified 2026-08-12, read-only):** Still open: `crates/test/tests/cookbook_skills.rs` calls `parse_statement` and asserts nothing beyond parsing — no typecheck against the describe registry.

## Final Report

Development completed as planned, with **two scope calls recorded below** rather than made silently.

The ratchet is now two checks over one extractor and one floor: every recipe **parses**, and every
column it names **exists** on the node it addresses, resolved through the binary's own cred-free
`compiled_describe_registry` — the same registry `gen-docs` renders from. 62 recipes are
typechecked; 19 source paths are skipped and **named** in the test output.

### Where it lives, and why it moved twice

The ticket assumed the new work was "entirely in what happens after extraction", i.e. inside
`crates/test/tests/cookbook_skills.rs`. It could not stay there: `qfs-test` is the wasm-clean pure
harness and must not link the binary, and `compiled_describe_registry` is in the binary crate.
Moving it into `crates/qfs/tests/` then tripped the architecture guard
`crates/cmd/tests/dep_direction.rs` — the binary may not depend on `qfs-parser`, and a dev-dependency
counts. The guard is right and was left alone.

It landed in **`xtask/tests/`**: xtask already links `qfs` for `gen-docs`, already owns the
anti-drift family this check belongs to, and ships in nothing. One file still holds both ratchets,
so there is still one extractor and one `MIN_STATEMENTS`.

### The check agrees with the fold rather than re-deriving a rule

Running each recipe through `Evaluator::eval` was tried first and does **not** work for the class
this ticket exists to catch: measured, `/github/acme/web/pulls |> where author == 'x'` evaluates
**Ok** (the predicate is late-bound at plan time), while `|> select author` refuses. So the check
walks the pipeline's *checkable prefix* explicitly, as the Scope describes. Two properties keep it
from ever producing a false red:

- The known-name set starts as the node's columns and only ever **grows** (`EXTEND`/`SET`/`AS`/a
  projection alias adds a name), so an alias can never read as missing.
- It stops at the first stage after which the schema is no longer knowable from describe alone —
  `decode`/`encode`, `join`, the set ops, `call`, `transform`, `switch`, `follow`, `post`.

### Scope call 1 — two articles were changed, which the ticket lists as out of scope

The Out-of-scope line reads "Changing any article. `github.md` was corrected in the same run this
ticket was minted in" — i.e. *you need not re-fix github.md*. It cannot mean *a recipe the new
ratchet catches must stay false*, because then the ratchet could never land green. Turning it on
found **two genuinely false recipes** in `docs/cookbook/cross-service.md`, both against `/mail/inbox`
(real columns: `id, thread_id, date, from, subject, snippet, label_ids, attachments`):

- `select … received_at as at` — no `received_at`; corrected to `date`.
- `select subject, body` (×3, plus the `create transform triage` input declaration) — no `body`;
  corrected to `snippet`.

These are exactly the class the ticket was minted for, found in a second article nobody had checked.
`qfs-cross-service/SKILL.md` was regenerated and all four plugin `version` fields went
`0.19.3 → 0.19.4` (a taught surface changed).

### Scope call 2 — the ratchet is blind wherever `describe` is

Three of the first five hits were **false positives** traced to an existing defect, not to false
articles: the first implementation passed `resolve_path`'s mount-stripped remainder to
`Driver::describe`, and the mail driver ignores its path argument in that form — reporting the
*message* schema for `/mail` (really `name`) and for an attachment node (really
`filename, mime, size, content`). Passing the **full** path, as `catalog.rs` does, fixed all three
and raised the typechecked count 48 → 62 by making `/github` resolvable at all. This is the live
concern `what-describe-says-is-not-what` (defect 1); the ratchet is only ever as true as `describe`,
which is worth stating because a future describe regression turns this green test into a silent one.

### Verification

**Red before green** (Quality Gate item 1), with the ticket's own example added to a scratch article:

```
$ cargo test -p xtask --test cookbook_skills every_cookbook_skill_recipe_names_columns
    names ["author"] — not columns of /github/acme/web/pulls
test result: FAILED. 0 passed; 1 failed; ...
```

The scratch article was removed and the suite is green. Floors (item 2): `MIN_STATEMENTS = 45`
unchanged, new `MIN_TYPECHECKED = 20` against an actual 62. Skips (item 3) print by path:
`/chatwork/*`, `/cloudflare/*`, `/git/myrepo/*`, `/server/*`, `/slack/*`, `/sql/*` — 19 sources, all
needing a live registration rather than a compiled type, exactly as the ticket scoped.

Gate (item 4): `cargo test --workspace` 2722 passed / 0 failed, `cargo clippy --workspace
--all-targets -- -D warnings` `CLIPPY=0`, `cargo fmt --all --check` `FMT=0`, `gen-docs --check` /
`gen-skills --check` / `check-migrations` all exit 0.

### Discovered Insights

- **Insight**: `Driver::describe` takes the **full** path, not the mount-stripped remainder
  `MountRegistry::resolve_path` returns, and the two forms fail differently per driver: `/github`
  errors loudly on the stripped form while `/mail` silently answers with the wrong node's schema.
  **Context**: The loud failure is survivable; the silent one is what makes the stripped form
  dangerous for any new consumer. `catalog.rs` (gen-docs) is the correct model to copy.
- **Insight**: The evaluator refuses an unknown `select` column but not an unknown `where` column at
  plan time, because `where` is late-bound for pushdown.
  **Context**: This is why "just run it through the evaluator" cannot be the ratchet, and it is the
  same asymmetry the open ticket `20260725113000` is about from the other direction.
