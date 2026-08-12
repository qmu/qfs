---
created_at: 2026-08-12T14:12:25+09:00
author: a@qmu.jp
assignees: [a@qmu.jp]
depends_on:
mission:
merge_policy: review
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
