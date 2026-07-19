---
created_at: 2026-07-19T00:00:00+09:00
author: a@qmu.jp
type: deferred
layer: [Domain, Infrastructure]
effort:
commit_hash:
category: Changed
depends_on: []
blocks: [20260718203326-cf-surface-from-committed-declaration.md]
mission: declared-drivers-are-the-normal-way-to-add-a-service
---

# DESIGN RULING NEEDED — the declared `/cf` D1 mount shape (nested mount vs composite facet, and the D1 path placement)

## Why this is deferred (not built)

This is the **owner ruling** that gates Stage 2+ of
`20260718203326-cf-surface-from-committed-declaration.md`, the last open item of this mission
(6/7 acceptance). Four `/monitor` drive leaves have now worked the ticket:

- Stage 1 (the declared `CREATE SQL` sql-resource *shape* + `DeclaredSqlResource` read model) is
  landed and gate-green (`bb089fe`).
- Stages 2–6 (the D1 bridge that actually serves a mount, KV/queues REST, the conformance twin,
  deleting compiled `/cf`, docs) are NOT built.

Every leaf independently reached the same wall: Stage 2 wired to serve a real mount **cannot be
made gate-green in one leaf without first fabricating an owner-facing design decision** that the
older brief (`20260716214300`) explicitly reserves as *Fable-tier judgment* — "Do not start
implementation before the brief is ruled." An unattended drive leaf cannot make that call, so it is
escalated here as a discrete decision artifact rather than left buried in the implementation
ticket's drive-status notes.

## The decision (two coupled questions)

### Q1 — Mount routing shape: nested mount, or composite facet?

The declared D1 surface (`/…/d1/{database}/{table}`, a sqlite-dialect SQL endpoint) must reach a
`CfDriver`+`D1Database`, while everything else under the same declared Cloudflare mount stays a
plain `RestDriver`. Two structures can carry that:

- **(A) Nested mount** — register the D1 surface as its own mount (id e.g. `cloudflare/d1`),
  distinct from the `cloudflare` REST mount. The plan/describe registry already routes by
  **longest-prefix path** (`core/src/registry.rs:594 resolve_service_path`), so `/cloudflare/d1/…`
  would prefer the nested mount over `/cloudflare`. `DriverId` is an **unvalidated String**
  (`types/src/schema.rs:19`) and `MountRemap::outer_id` keeps the full outer path minus the leading
  `/` (`mount_adapter.rs:113`), so a `/cloudflare/d1` mount naturally yields the slash-bearing id
  `"cloudflare/d1"`, distinct from `"cloudflare"`. If a slash-bearing id flows cleanly through
  plan-lowering → the `DriverId`-keyed **read** funnel (`exec/src/read.rs:57`, resolved by
  `.get(id)`) → the apply funnel, then (A) needs **no** composite facet — a much smaller build.

- **(B) Composite facet** — keep one `cloudflare` mount whose driver internally dispatches
  `/cloudflare/d1/…` → CfDriver and everything else → RestDriver, across read, apply, AND a merged
  plan/pushdown profile. Substantially larger; forced only if (A)'s slash-bearing id breaks
  somewhere.

**Unverified seam (a ~30-min spike settles it, no design input needed):** register a dummy nested
mount at id `a/b` and assert a SELECT read resolves to `ReadRegistry.get("a/b")`, not `"a"`. Read
drivers today all register under **single-segment** ids (`shell.rs`: `local`, `sys`, `transform`,
`type`, …), so whether the `DriverId`-keyed read/apply funnels and plan-lowering tolerate a
slash-bearing id is confirmed only by running it. Green spike ⇒ ruling can be **(A)**; if any stage
(plan lowering, capability qualification, `CALL` proc routing, the `mount_adapter` proc/effect
rewrites) assumes a single-segment id ⇒ ruling is forced to **(B)**.

### Q2 — Address placement (owner UX call, independent of Q1's outcome)

Does the declared D1 surface stay **nested under `/cloudflare/d1/{database}`** (co-located with the
plain-REST Cloudflare mount), or move to **its own top-level segment** (e.g. `/cf-d1/…` or a
renamed `/cf`)? This is a pure product-UX decision — it sets the `cloudflare.qfs` asset shape, the
`qfs-cloudflare` skill's taught surface (and thus whether the four plugin `version` fields bump),
and the operator's mental model of "one Cloudflare mount vs two." No spike resolves it; the owner
picks.

## What is already de-risked (so the ruling is the only true blocker)

- **The wire backend is NOT a blocker.** `HttpApiBackend::new(exchange, account_id, token)`
  (`driver-cf/src/backend.rs:611`) already speaks the declared D1 endpoint shape; build it from
  declared inputs (`account_id` = mount `at_locator`, `token` = resolved `AUTH ACCOUNT 'cf'` bearer,
  `exchange` = `transport::cf_exchange()`) instead of compiled discovery.
  `D1Database::discovered(backend, uuid, catalog)` (`registry.rs:48`) is the exact lift;
  `DeclaredSqlResource::catalog()` (Stage 1) supplies the `catalog` — no mount-time `introspect_d1`.
- **Blocker-3 (wildcard D1 resolution) is small and design-independent.** `CfRegistry::d1`
  (`driver-cf/src/registry.rs:280`) is exact-match `HashMap.get(db)`; the wildcard declared path
  `/…/d1/{database}` needs a template fallback (e.g. `with_d1_template(catalog, backend)` + a `d1()`
  template arm) resolving an arbitrary key to a `D1Database` carrying the declared catalog. Forced by
  the no-introspection model, unit-testable in `driver-cf/src/tests.rs`, shape-independent — build it
  in whichever stage, either ruling.

## Build order once ruled (for the implementing session)

1. **2.0 spike** (Q1) — confirm nested-id routing; commit nothing.
2. **2a** — the wildcard-D1 `CfRegistry` capability (blocker-3) + a declared→`CfDriver` composition
   helper (backend from declared inputs + `CfRegistry` from `resource.catalog()` → `CfDriver`),
   unit-tested hermetically over `MockCfBackend`/`MockExchange` (assert ZERO introspection at build).
3. **2b** — wire into the three declared-mount facets (`describe.rs:173`, `commit.rs:355-396`,
   `shell.rs:418-448`) per the Q1 ruling.
4. **Stages 3–6** — KV/queues declared REST; the conformance twin over `MockCfBackend`/`MockHttpClient`
   + a describe-purity (no-network) test; delete compiled `/cf` (`cf.rs:153-242`, `cloud_mounts.rs:54`)
   only once the twin is green (§13 ratchet forbids deleting first); `cloudflare.qfs` asset extension
   + cookbook + `gen-skills`; plugin version bump if `qfs-cloudflare`'s taught surface changes; binary
   patch bump; then archive + tick mission acceptance line 142.

## Resolution

When ruled: record the Q1 shape (A/B) and the Q2 placement in the implementation ticket
`20260718203326`, then a normal drive builds Stages 2–6. This deferred ticket is retired at that
point (its only content is the ruling request).
