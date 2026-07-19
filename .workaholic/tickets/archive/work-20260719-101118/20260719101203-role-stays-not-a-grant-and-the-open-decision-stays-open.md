---
created_at: 2026-07-19T10:12:03+09:00
author: a@qmu.jp
type: housekeeping
layer: [Domain]
effort: 2h
commit_hash:
category: Changed
depends_on:
mission: a-request-resolves-to-a-principal-the-query-path-can-read
---

# `Role` stays not a grant, and the open decision stays open

Satisfies mission acceptance: **"`Role` is still not a grant, and the open decision is still
open."** This is an OUTCOME statement, not a conversion — an anti-regression ticket. The mission
threads a *user* principal only; it must not, as a side effect, turn `identity::Role` into an
authorization grant or settle the t55-vs-t53 taxonomy.

## What to assert (do NOT convert Role into a grant)

- Confirm, after the seam ticket lands, that:
  - `identity::Role` (`crates/identity/src/invite.rs:141-149`) is unchanged — still
    `Owner | Admin | Member`, still a label on a `Membership`, `Role::Admin` still "not
    privileged yet".
  - The open-decision flags still stand verbatim: `identity/src/invite.rs:135-139` (role
    taxonomy is an OPEN PRODUCT DECISION) and `qfs/src/sys.rs:24-27` (super-admin vs
    project-admin split recorded as open, not baked in).
  - No consumer or gate introduced by the seam ticket treats `Role::Admin` as "is an admin".
    The `RequestContext`/`DecisionContext` mapping carries the *user id* only; no role is read
    from `identity::Role` into an authorization decision.
- Add a **regression test** (in `qfs-identity` or `qfs-server`) that pins the invariant: a
  `DecisionContext` built for a user with a `Role::Admin` membership grants nothing beyond what
  the same user without the label grants — i.e. `Role` contributes no authorization. If a later
  change wires `identity::Role` into `evaluate`, this test fails.

## Policies

**設計 / `workaholic:design`**
- `access-control` — identity ≠ authorization (§4.1). A membership label is not a capability;
  the ACL (t57 `POLICY`) is the only grant path. This ticket keeps that boundary intact.

**実装 / `workaholic:implementation`**
- `machine-checkable-domain` — the invariant becomes a test, not a comment, so a future
  accidental conversion is caught by CI rather than by review.

**House rules (`CLAUDE.md`)**
- Experimental: no back-compat concerns; the point here is a NEGATIVE guarantee, held by a test.

## Quality Gate

**Acceptance criteria.** `identity::Role` is not a grant after the mission's changes; the two
open-decision flags stand unedited (or edited only to record a ruling the developer actually
made — none was); a regression test pins that a `Role::Admin` label grants nothing.

**Verification method.** Read the two flag sites; run the new regression test.

**Gate that must pass.** `cargo fmt` on touched crates, `cargo build --workspace`,
`cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings` — all exit 0.
