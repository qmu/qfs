---
created_at: 2026-07-24T01:31:00+09:00
author: a@qmu.jp
type: enhancement
layer: [Domain]
effort:
commit_hash:
category: Added
depends_on: [20260724013000-enforce-policy-on-the-serve-read-path.md]
mission: what-a-principal-can-see-and-do-is-granted-by-policy
---

# Every grant axis bites on reads, both directions

## Overview

With SELECT evaluated on the serve read path (ticket 20260724013000), prove each t57 grant axis
admits the matching actor and denies the non-matching one **on reads**:

- **FOR** — `Subject::User` / `Subject::Role` (through `RoleGraph` inheritance: an `owner` holds
  what `member` is granted) / `Subject::Group`.
- **AT** — `ScopeGlob` path scope: `ALLOW SELECT AT /members/alice/**` admits Alice's subtree read
  and denies a read outside the scope.
- **WHERE** — `Condition::MemberOf('/directories/...')` through the `MembershipResolver` seam
  (`crates/server/src/policy/context.rs` — `resolve_memberships` runs up front; use the existing
  resolver seam with a test resolver, exactly as the write-side tests do).

The machinery all exists in `crates/server/src/policy/model.rs` and is proven for writes; this
ticket is the read-side both-directions matrix.

## Policies

- workaholic:implementation / machine-checkable-domain-gaps — each axis gets an admit test AND a
  deny test; a single-direction proof is not a proof.
- workaholic:design / アクセス制御 — RBAC (roles bundle subjects) and PBAC (grant triples)
  combined; humans and AI are peer principals, so the tests use plain named users, not special
  cases.

## Quality Gate

1. Per axis (FOR user, FOR role incl. one inherited-role case, FOR group, AT scope, WHERE
   member_of): one hermetic test that the matching actor's read succeeds and one that the
   non-matching actor's read is denied — at the `evaluate_with_context` level or the
   `crates/http` seam, consistent with where ticket 20260724013000 put its tests.
2. The irreversible-strictness invariant stays: a bare `ALLOW ALL` still never grants
   REMOVE/CALL (`Rule::matches`), untouched by the read work — one regression assertion.
3. Workspace gates green: `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`,
   `cargo fmt --all --check`.

## Considerations

- Reuse the write-side test fixtures/builders for policies and contexts rather than a parallel
  read-side fixture family — the point is that reads ride the same rules.
