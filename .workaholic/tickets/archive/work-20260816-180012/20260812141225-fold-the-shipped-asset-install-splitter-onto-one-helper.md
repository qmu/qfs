---
created_at: 2026-08-12T14:12:25+09:00
status: done
author: a@qmu.jp
assignees: [a@qmu.jp]
depends_on:
mission:
merge_policy: review
claim: work-20260816-180012
---

# Fold the shipped-asset install-splitter onto one helper, so a test cannot drift from the install path

## Overview

Minted from the open concern `the-shipped-asset-install-splitter-is` (feedback
`20260804205042-the-shipped-asset-install-splitter-is.md`, severity low).

Several tests in `packages/qfs/crates/qfs/src/declared_driver.rs` verify a **shipped declaration
asset** (`crates/skill/assets/examples/*.qfs`) by splitting it into statements the way an install
does: strip `--` comments, split on `;`. Each carries its own inline copy of that splitter. The
Chatwork tests were folded onto a shared `shipped_statements` helper (PR #32); at least four inline
copies remain (`declared_driver.rs`, around lines 2749, 2811, 2849 and 3396 on `main`), including the
Slack twin harness's per-kind extractors and the cloudflare / github_account checks.

Why it matters beyond tidiness: the splitter has to stay **identical to the config install path**, or
a test proves something about a statement list the installer would never produce. Independent copies
are independent places for that to drift. This bit once already — when the Slack CALL maps moved
their channel binding to a shared `CREATE LOOKUP` (PR #36), the twin harness had no extractor for the
new statement kind and four equivalence tests silently drove maps whose lookup was simply absent;
they failed loudly only because the *assertions* were strong. A single splitter is one place to teach
a new statement kind.

## Scope

**In scope.** One helper that turns a shipped asset into its statement list, used by every test that
reads a shipped asset; the per-kind extractors (`CREATE VIEW` / `MAP` / `TYPE` / `LOOKUP` for the
Slack twin harness) built on top of it rather than each re-splitting.

**Out of scope.** Changing what any test asserts, and unifying the harness with the production
install path itself (the tests deliberately construct an in-memory model rather than writing a System
DB; that is a separate question).

## Key Files

- `packages/qfs/crates/qfs/src/declared_driver.rs` — `shipped_statements` (the existing helper) and
  the remaining inline copies.
- `packages/qfs/crates/skill/src/lib.rs` — the embedded assets under test.

## Implementation Steps

1. Move every remaining inline splitter onto `shipped_statements`.
2. Express the Slack twin harness's per-kind extractors over that one statement list.
3. Confirm no assertion changed: the suite passes with no test edits beyond the mechanical fold.

## Policies

- `workaholic:implementation` / one-source-of-truth — a rule copied into N places is N places to
  drift; the copies here are copies of the *install semantics*, which is exactly the thing the tests
  exist to be faithful to.

## Quality Gate

1. **Acceptance:** no inline comment-strip-and-split remains in `declared_driver.rs`; every shipped
   asset test reads its statements through the one helper.
2. **Verification:** `cargo test --workspace` passes with assertions unchanged (the diff is
   extraction only).
3. **Gate:** `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`,
   `cargo fmt --all --check` all exit 0.

## Considerations

- Purely mechanical and safe to drive alongside anything else; it touches tests only.
- If a future statement kind is added to a shipped asset, the helper is where it should become
  visible — consider making an unrecognised statement kind loud rather than skipped, so the next
  `CREATE …` addition cannot be silently ignored by the harness.

## Final Report

Development completed as planned.

Every remaining inline copy of the install splitter in `declared_driver.rs` is gone. Three tests
(`shipped_slack_script_installs_statement_for_statement`,
`shipped_cloudflare_script_installs_statement_for_statement`,
`shipped_github_account_script_installs_credential_free_with_account_auth`) each carried a
character-for-character duplicate of the 24-line splitter and now call `shipped_statements(script)`
— the helper the Chatwork tests already used. The Slack twin harness's four per-kind extractors
(`shipped_slack_maps` / `_views` / `_lookups` / `_types`) and the G2 pushdown test's driver-statement
lookup each re-split the bytes with their own strip-and-split; they now go through one new helper,
`shipped_statements_of_kind(script, "CREATE <KIND> ")`, expressed over `shipped_statements`. Eight
call sites, one splitter.

No assertion changed and no test was added or removed — the diff is extraction only, which is the
ticket's own definition of done.

### Verification

- `cargo test -p qfs --lib declared_driver` — 66 passed, 0 failed (the tests that read a shipped
  asset are all in this set).
- `cargo test --workspace` — 2719 passed, 0 failed.
- `cargo clippy --workspace --all-targets -- -D warnings` — `CLIPPY=0`.
- `cargo fmt --all --check` — `FMT=0`.
- `gen-docs --check` / `gen-skills --check` / `check-migrations` — all exit 0.
- Acceptance re-checked by grep: `grep -n "split(';')" crates/qfs/src/declared_driver.rs` now
  returns nothing. The two surviving `.lines()` uses are line *counters* — the §13.2 conciseness
  measurement and the `IRREVERSIBLE` marking count — not statement splitters, and are out of scope.

### Discovered Insights

- **Insight**: The inline copies were not merely duplicates of the helper, they were *stale* ones:
  the helper strips `#` whole-line comments and the five Slack extractors did not. No shipped asset
  currently uses `#` (`grep -c '^\s*#'` returns 0 across all eleven `.qfs` assets), so the fold was
  behaviour-preserving — but the first asset to use one would have split differently in the tests
  than in the installer, silently.
  **Context**: This is the concrete form of the drift the ticket was written against, and it was
  already present before anyone added a new statement kind. The divergence was in comment handling,
  not in statement kinds, which is the direction the ticket did not anticipate.
- **Insight**: `shipped_statements` lives at the bottom of a 5,500-line `mod tests`, roughly 2,300
  lines below the copies that should have used it.
  **Context**: Rust's order-independent item resolution means the copies never failed to compile
  for lack of it — the helper was in scope the whole time. Distance, not visibility, is what kept
  them separate, so a future helper of this kind is worth placing near its first caller.
