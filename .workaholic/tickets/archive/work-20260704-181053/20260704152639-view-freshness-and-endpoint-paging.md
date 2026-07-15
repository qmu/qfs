---
created_at: 2026-07-04T15:26:39+09:00
author: a@qmu.jp
type: enhancement
layer: [Domain, Infrastructure]
effort:
commit_hash: f152459
category: Added
depends_on:
---

# Freshness as data + bounded endpoint paging (blueprint §14 contracts 2 and 3)

## Overview

Implement the two remaining infra contracts blueprint §14 (the console face, approved
2026-07-04) says qfs owes its screen — both useful to any client, with the console as the first
consumer:

1. **Freshness as data.** A materialized view's `last_run` / staleness must be readable through
   the language — the "updated 5 minutes ago" primitive. The daemon already persists `LAST_RUN`
   durably (the fsync'd durable store); this surfaces it as ordinary rows (e.g. columns on the
   `/server` views listing, or a `/sys` surface — decide with DESCRIBE honesty: pure, local, no
   network).
2. **Bounded result paging on endpoints.** A dashboard widget renders bounded slices: an
   endpoint result must be requestable in pages with the truncation flag set honestly.
   **Dialect settled (2026-07-04, owner-approved; blueprint §14): `limit`/`offset`** — sharing
   exactly the envelope's `meta.{limit,offset,truncated}` vocabulary (ticket 20260703150300); no
   second pagination dialect. Cursor was rejected: qfs sources cannot generally guarantee the
   stable sort key an honest cursor requires.

## Policies

- `workaholic:implementation` / `policies/directory-structure.md` — conventional layout
- `workaholic:implementation` / `policies/coding-standards.md` — style conventions
- `workaholic:implementation` / `policies/observability.md` — freshness is observation output surfaced as data, not a bespoke UI feed
- `workaholic:implementation` / `policies/objective-documentation.md` — staleness reporting must be honest (a never-run view says so; no fake timestamps)

## Key Files

- `packages/qfs/crates/host/src/` - the durable store where `LAST_RUN` persists
- `packages/qfs/crates/server/src/` - materialized view state + the `/server` listing surface
- `packages/qfs/crates/http/src/` - endpoint result production (where paging binds)
- `.workaholic/tickets/todo/a-qmu-jp/20260703150300-agent-facing-doc-gaps.md` - the envelope whose truncation metadata paging must agree with
- `docs/blueprint.md` §14 - the authority

## Implementation Steps

1. Surface `last_run`/staleness on the materialized-view listing (columns readable via the
   language; pure DESCRIBE).
2. Add paging to endpoint results, agreeing with the envelope's truncation metadata; document
   the parameter names on the endpoint surface.
3. Hermetic tests: a refreshed view reports its `last_run`; a never-run view reports honestly;
   a paged endpoint returns bounded slices with the truncation flag set exactly.
4. Cookbook/skills regeneration where the surfaces are taught.

## Quality Gate

**Acceptance criteria:**

- `last_run`/staleness readable through the language for materialized views (hermetic test);
  a never-run view is honest.
- An endpoint result pages with bounded slices and exact truncation flags (hermetic test).
- Paging vocabulary is consistent with the result envelope (one shape, no second pagination
  dialect).

**Verification method:** `cargo test --workspace`; `clippy --workspace --all-targets -- -D
warnings`; `fmt --all --check`; `gen-docs --check`; `gen-skills --check`.

**Gate:** all green including the named tests.

## Considerations

- Freshness must not turn DESCRIBE impure — the value comes from the local durable store, never
  a network probe
- Paging interacts with pushed-down LIMIT (§6 residual honesty): the pushed limit and the page
  bound must compose without double-truncation surprises
