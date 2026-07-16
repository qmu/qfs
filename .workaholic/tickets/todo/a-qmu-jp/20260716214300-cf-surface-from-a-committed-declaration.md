---
created_at: 2026-07-16T21:43:00+09:00
author: a@qmu.jp
type: enhancement
layer: [Domain, Infrastructure]
effort:
commit_hash:
category:
depends_on:
mission: declared-drivers-are-the-normal-way-to-add-a-service
---

# /cf's D1/KV/queues surface comes from a committed declaration

## Overview

Mission acceptance item 2 (concern `cf-live-203090-unimplemented-cf-and`, rescoped 2026-07-16).
`/cf` is the mission's counter-example: a working cloud service reachable *because* it was
compiled in. This ticket makes the declaration shape carry what a per-resource cloud driver
needs, so the compiled `/cf` stops being the way Cloudflare's D1/KV/queues are reached.

**Starts with a design brief — the implementation gap is a declaration-format gap.** Verified
against source this session, the declared REST lift (`declared_driver.rs:266 rest_config`,
`:291 resources`) can express only a flat, static per-leading-segment verb map. Three
capabilities separate it from `/cf`:

1. **Mount-bound account segment.** `/cf` resolves the account id once from the mount's
   `at_locator` (`CloudflareAccountId::from_mount`, `cf.rs:23`); a declaration must hardcode
   `{account}` as a template variable the caller supplies per query (see `cloudflare.qfs`'s
   account-scoped listing views). Needed: a declaration-level "bind this path segment to the
   connection's account coordinate".
2. **Live sub-resource enumeration.** `/cf` discovers the actual D1 databases / KV namespaces /
   queues at mount time (`cf.rs:153 driver_from_backend_*`, `list_d1_databases`,
   `introspect_d1` `cf.rs:309` running `sqlite_master` + `pragma_table_info` over the wire).
   A declared driver's surface is exactly its `CREATE VIEW/MAP` paths — no enumeration hook.
3. **Typed, nested sub-surfaces.** `/cf/d1/<db>` carries a full SQL catalog with pushdown
   (`Dialect::Sqlite.map_type`, `cf.rs:363`); KV and queues carry typed sub-schemas
   (`schema.rs kv_table_schema`, `queue_tail_schema`). A declared resource is one flat
   `ResourceMap`; `OF <type>` gives a single flat row shape (`type_column_names`, :588), and
   the declared verb set has no UPDATE/CALL (`map_verb`, :338).

Context: `/cloudflare` (the DECLARED plain-REST driver from `cloudflare.qfs`) already covers
zones/DNS and account-scoped **listings** of KV/queues/D1 — but only listings; D1 SQL, KV
get/put, and queue send/pull stay compiled. The archived concern
`26-cloudflare-declaration-design-remains-partial` (resolved_by_pr b9e1137) already named this
target: "a per-resource Cloudflare declaration format" without losing fail-closed behavior.

## Implementation Steps

1. **Design brief first (owner ruling).** Options to weigh, at minimum:
   - (a) extend the declaration grammar with the three capabilities (account-segment binding,
     an enumeration view that materializes child mounts, a `CATALOG`-style typed sub-surface
     clause), keeping one generic lift;
   - (b) keep enumeration/introspection compiled but move the per-resource *config* into
     declaration rows the compiled driver reads (smaller grammar, hybrid);
   - (c) declare D1 only (the SQL surface, where pushdown pays) and leave KV/queues compiled.
   The brief names the ruled shape, its grammar, and what `/cf` retires.
2. Implement the ruled shape in the lift (`declared_driver.rs`) and, if grammar moves, the
   parser + `/sys/drivers` row shape (append-only migration if a column is needed).
3. Port the Cloudflare surface: extend `cloudflare.qfs` (or a new declaration) until D1
   query, KV read/write, and queue send/pull run declared; the compiled `/cf` demotes to
   whatever the ruling leaves it (possibly nothing — hard breaks are fine).
4. Hermetic conformance tests against a mock backend mirroring `driver-cf`'s
   (`backend.rs:463 CfBackend` trait is the template); the live round hands over to the
   owner-attended backlog.
5. Docs/skills: `qfs-cloudflare` skill and cookbook article update if the taught surface moves
   (plugin version bump per CLAUDE.md).

## Key Files

- `packages/qfs/crates/qfs/src/declared_driver.rs:233-390,473-590` — the REST lift to extend.
- `packages/qfs/crates/qfs/src/cf.rs:23,86,153,193,209,309,338,363` — what the declaration must
  be able to say; the retirement target.
- `packages/qfs/crates/driver-cf/src/backend.rs:463,594,600` — the wire trait / mock seam.
- `packages/qfs/crates/skill/assets/examples/cloudflare.qfs` + `docs/cookbook/cloudflare.md` —
  the declaration to grow.
- `packages/qfs/crates/qfs/src/cloud_mounts.rs:53` — the compiled registration that eventually
  drops.

## Policies

- `workaholic:design` / external dependencies — the declaration shape is the product's public
  contract for every future cloud service; ruled by brief, not grown ad hoc.
- `workaholic:implementation` / `anti-corruption-structure` — one lift for all declared
  drivers; no cf-special-case branch in the generic path.
- `workaholic:implementation` / `coding-standards` + `test`.

## Quality Gate

1. The design brief exists, names the ruled option, and the implementation matches it.
2. Declared D1: `select` against `/…/d1/<db>/<table>` plans with pushdown and round-trips
   hermetically against the mock backend; KV get/put and queue send/pull per the ruling.
3. The account coordinate comes from the mount (no per-query `{account}` hand-off) — a test
   pins that a declared query never asks the caller for the account id.
4. Fail-closed: no token → the declared mount refuses exactly like `/cf` does today (the
   consent + vault gates are unchanged).
5. Both-directions where a defect is claimed; baseline gates + patch bump; plugin bump if the
   taught surface changes.

## Considerations

- This is the largest of the mission items; the design brief is Fable-tier judgment, the
  implementation is not. Do not start implementation before the brief is ruled.
- Sequencing: independent of tickets 20260716214100/20260716214200, but if the ruling adds a
  `/sys/drivers` column, land after any in-flight migration to keep numbering linear.
- `/cloudflare` (plain REST) stays as the broad-surface companion regardless of the ruling.
