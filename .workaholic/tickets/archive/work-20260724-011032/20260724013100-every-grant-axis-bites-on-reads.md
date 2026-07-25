---
created_at: 2026-07-24T01:31:00+09:00
author: a@qmu.jp
type: enhancement
layer: [Domain]
effort: 1h
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

## Final Report

Development completed as planned. Every t57 grant axis now has a read-side both-directions proof,
built on the write-side rule and context builders rather than a parallel read-side fixture family.

### Discovered Insights

- **Insight**: the read-side matrices belong at the `evaluate_reads_with_context` level, not at the
  `crates/http` seam, because the HTTP principal seam (`decision_for`) carries the USER axis only —
  it deliberately does not convert an identity role/group label into a grant. Roles, groups and
  memberships therefore cannot be injected through a request at all today, so an http-level role
  test could only ever prove the denial direction.
  **Context**: when a later mission wires roles into the request principal, `decision_for` is the
  single place to change, and these axis tests become reachable end to end.
- **Insight**: a federated read has SEVERAL scan targets, and the gate is only as permissive as the
  weakest leg — `evaluate_reads_with_context` denies on the first ungranted target. A policy that
  grants one driver does not open a join that also reads another.
  **Context**: pinned by `every_scanned_target_must_be_granted_not_just_the_first`; it is the
  property that keeps a cross-service JOIN from becoming a way around a per-driver grant.
- **Insight**: a broad `ALLOW ALL` DOES now open reads (SELECT is reversible), which makes the
  irreversible-strictness rule more load-bearing than before: the same token that grants every read
  still grants no REMOVE/CALL. The regression assertion covers both halves in one test.
