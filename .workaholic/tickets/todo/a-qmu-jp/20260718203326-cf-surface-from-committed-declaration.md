---
created_at: 2026-07-18T20:33:26+09:00
author: a@qmu.jp
type: enhancement
layer: [Domain, Infrastructure]
effort:
commit_hash:
category: Changed
depends_on: [20260718203325-create-account-secret-ref-bind-time-resolution.md]
mission: declared-drivers-are-the-normal-way-to-add-a-service
---

# The /cf D1/KV/queues surface binds from a committed declaration, not compiled introspection

## Overview

The `/cf` driver's relational and object surfaces are built by compiled introspection today:
`driver_from_backend_with_artifact_sealer` (`packages/qfs/crates/qfs/src/cf.rs:153-242`) discovers
D1/KV/queues by calling `list_*` and `introspect_d1` (`cf.rs:309`) at registry-build time, and
`cloud_mounts.rs:54` hard-codes `cf` as a compiled cloud kind. Per the owner's ruling (2026-07-18)
this surface moves onto a **committed declaration**.

- Grow the declared driver shape with a **sql-resource arm** so a `cf.qfs` declaration — extending
  `packages/qfs/crates/skill/assets/examples/cloudflare.qfs`, which today punts D1 SQL / KV / queues
  to the compiled `/cf` — can declare "a sqlite-dialect SQL endpoint over this REST verb". That arm
  lifts onto the existing driver-sql / driver-cf planner, so D1's relational surface is served from
  the declaration rather than from `introspect_d1`.
- KV and queues ship as **plain declared REST** views/maps first, inside this same ticket (staged):
  they become ordinary declared REST resources while the sql-resource arm carries D1.
- The set of resources a mount serves comes from the committed `/sys_drivers` rows. Compiled `/cf`
  registration (`cf.rs:153-242`, `cloud_mounts.rs:54`) is **deleted only after** the declared twin
  passes the conformance suite — the blueprint §13 self-hosting ratchet.
- The token comes from ticket 1's declared account: `AUTH ACCOUNT 'cf'` resolves through
  `AccountBearerSecrets` (`packages/qfs/crates/qfs/src/declared_driver.rs:768`); `cf` is a
  static-bearer provider, so this already works today. The account id stays the non-secret
  `AT/{account}` locator — never a token.
- The live credentialed round against real Cloudflare is handed to the owner-attended live backlog
  (recorded, not attempted here).

## Policies

- implementation/self-hosting-ratchet — the compiled `/cf` build is deleted only after the declared
  script twin passes the full conformance suite (blueprint §13).
- implementation/honest-surfaces — the declared surface serves exactly what the committed
  declaration says, with no hidden compiled fallback once the twin is live.

## Quality Gate

1. `cargo test --workspace`
2. `cargo clippy --workspace --all-targets -- -D warnings`
3. `cargo fmt --all --check`
4. `cargo run -p xtask -- gen-docs --check`
5. `cargo run -p xtask -- gen-skills --check`
6. Plugin version bump if the `qfs-cloudflare` skill's taught surface changes (minor for a
   taught-surface break, patch otherwise) — all four plugin `version` fields.
7. Acceptance: a committed `.qfs` declaration is the source of the D1/KV/queues surface — no
   `introspect_d1` / `list_*` call at mount time.
8. Acceptance: compiled `/cf` is demoted behind the ratchet and deleted once conformance passes.
9. Acceptance: the declaration carries no token or secret.
10. Acceptance: `DESCRIBE` stays credential-free and network-free (drives `MockHttpClient`).
11. Acceptance: the live credentialed round is handed to the owner-attended backlog (recorded, not
    attempted).
12. Verification: hermetic conformance/twin tests over `MockCfBackend` / `MockHttpClient` (the
    `cf.rs` test pattern at `cf.rs:395-465`); the `cookbook_skills.rs` parse ratchet over the grown
    `cloudflare.qfs`; a describe-purity test asserting no network at DESCRIBE.

## Considerations

- Staging matters: land the plain declared REST KV/queues resources and the D1 sql-resource arm in
  the same ticket, but keep the compiled `/cf` path alive until the conformance twin is green — the
  ratchet forbids deleting first.
- Depends on ticket 1: the declared `cf` account (`AUTH ACCOUNT 'cf'`) supplies the bearer, so this
  ticket must land after the bind-time secret-reference resolution is in place.

## Drive status — 2026-07-18 (/monitor wave-2 leaf, NOT started, still in todo)

Sibling ticket `20260718203327` (path_binding-only sql/git + type round-trips) landed and is
gate-green (commit `23991d5`, archived `d90428e`; binary `0.0.79`, plugins `0.13.0`). This ticket was
investigated but **deliberately not started** — it is an all-or-nothing architecture change that
cannot reach gate-green in one leaf session without risking a broken tree, and no sound self-contained
partial advances its acceptance. Nothing was committed for it; the tree is clean.

**Why it is all-or-nothing (the acceptance can't be split):**
- Acceptance 7 (no `introspect_d1`/`list_*` at mount) and 8 (compiled `/cf` deleted once conformance
  passes) both require the full **sql-resource arm** to exist and be functional. A half-wired arm
  breaks the declared-driver planner and the existing `/cf` tests, so it is not sound to commit.
- The §13 ratchet forbids deleting the compiled `/cf` before the conformance twin is green, so the
  twin + arm must all land together.

**What the sql-resource arm requires (scoping map for the next session):**
1. New declared-driver surface: a way in `cloudflare.qfs` / the grammar to declare "a sqlite-dialect
   SQL endpoint over this REST verb" — a new resource kind alongside `CREATE VIEW`/`CREATE MAP` in
   `packages/qfs/crates/qfs/src/declared_driver.rs` (`resources()` / `ResourceMap`, ~L292-330) and
   its parser support.
2. A bridge lifting that declared resource onto the **driver-sql** planner backed by a **driver-cf**
   D1 backend built from the declared REST config: today `cf.rs:153-242`
   (`driver_from_backend_with_artifact_sealer`) calls `introspect_d1` (`cf.rs:309`) + `list_*` at
   registry-build time; the declared arm must serve `D1Database::discovered(backend, uuid, catalog)`
   (`driver-cf/src/lib.rs`, `registry.rs`) from the committed `/sys_drivers` rows instead — with the
   catalog coming from the declaration, not a mount-time `introspect_d1`.
3. KV/queues as plain declared REST: `cloudflare.qfs` already declares the KV-namespaces / queues /
   d1-database **listings** as REST views (L57-64); the staged step is to make those the served
   surface rather than the compiled `list_*`.
4. Conformance twin: hermetic tests over `MockCfBackend`/`MockHttpClient` (pattern at
   `cf.rs:395-465`; `driver-cf/src/tests.rs`) asserting the declared twin matches the compiled `/cf`
   BEFORE deleting the compiled registration (`cf.rs:153-242`, `cloud_mounts.rs:54`).
5. Then: gen-skills, the `cloudflare.qfs` asset extension, the `cookbook_skills.rs` parse ratchet,
   and a describe-purity test (no network at DESCRIBE). Bump plugin version only if the
   `qfs-cloudflare` skill's taught surface changes.

**No acceptance item is ticked.** Estimated as a multi-session architecture effort (driver-cf ~4600
LOC, declared-driver ~2551 LOC).

## Drive status — 2026-07-19 (STAGE 1 LANDED, gate-green; stages 2–6 remain; acceptance NOT ticked)

Split into gate-green increments per the scoping map above. **Stage 1 — the declared sql-resource
SHAPE — is implemented, gate-green, and committed** (`bb089fe`, still in todo). The remaining stages
(the D1 bridge, KV/queues REST, the conformance twin, deleting compiled `/cf`, docs/version) are the
cross-cutting architecture bulk and are NOT started; the tree is clean and unbroken.

**What Stage 1 landed (`bb089fe`):**
- A new `CREATE SQL /<path> [DIALECT SQLITE] OVER /<wire-endpoint> TABLES ( <table>(<cols>), … )`
  statement: a sqlite-dialect SQL endpoint over a wire query verb with the relation catalog declared
  INLINE (the declared twin of a mount-time D1 introspection). Parser sugar desugaring to a
  `kind='sql'` `/sys/drivers` row (no new `Statement` variant, no new frozen keyword) — the
  `CREATE TRANSFORM`/`CREATE MAP` pattern. `SQL`/`OVER` added to the lexer `path_boundary_word` list
  so `/…` after those nouns lexes as a path, not division (`crates/lang/src/lex.rs`).
- The read-back model `DeclaredSqlResource` + `load_declared_sql_resources()` (mirrors
  `DeclaredType`/`load_declared_types`, so **zero edits to `DeclaredDriver`'s ~15 construction
  sites**), including `DeclaredSqlResource::catalog()` — the `qfs_driver_sql::Catalog` lift the D1
  bridge will hand to `D1Database::discovered` in place of `introspect_d1` — and §13 host confinement
  on the wire endpoint (a foreign `/http/<x>` sql-resource is dropped at load, FAIL CLOSED).
- Unit tests: parser desugar + dialect default/reject; loader round-trip; catalog lift; confinement
  drop. Gates: `cargo test -p qfs` (395), `-p qfs-parser` (137), `-p qfs-lang` (23), `-p qfs-test`
  all pass; `clippy --workspace --all-targets -- -D warnings` clean; `fmt --all --check` clean;
  `gen-docs`/`gen-skills --check` in sync; `check-migrations` clean.

**What remains (stages 2–6), and why they are NOT a single safe increment for one leaf:**
2. **The D1 bridge.** Serve `D1Database::discovered(backend, uuid, catalog)` from the committed row:
   `catalog` = `DeclaredSqlResource::catalog()` (done); `backend` = a `CfBackend`/`HttpApiBackend`
   built from the declared wire config + the resolved `AUTH ACCOUNT 'cf'` bearer + the `{account}`/
   `{database}` path params (no `list_*`/`introspect_d1`). The hard part is **mount composition**:
   the declared mount today builds a `RestDriver` in THREE facets — `describe.rs:173`, `commit.rs:355–387`,
   `shell.rs:418–433`; a `kind='sql'` resource must instead compose a `CfDriver`+`D1Database` there.
   A half-wired branch breaks all three facets at once, so it is not a sound partial commit.
3. **KV/queues as plain declared REST** (the served get/put/push/pull, beyond the listings already in
   `cloudflare.qfs`), staged with the compiled `/cf` still alive.
4. **The conformance twin** over `MockCfBackend`/`MockHttpClient` + a describe-purity (no-network)
   test, proving the declared surface matches the compiled `/cf` BEFORE any deletion.
5. **Delete/demote compiled `/cf`** (`cf.rs:153–242`, `cloud_mounts.rs:54`) only once the twin is
   green (the §13 ratchet forbids deleting first).
6. `cloudflare.qfs` asset extension (a `CREATE SQL` D1 arm) + cookbook + `gen-skills`; plugin version
   bump if `qfs-cloudflare`'s taught surface changes; binary patch bump; then archive + tick mission
   line 142.

Binary stays at `0.0.79`, plugin `0.13.0` (no PR opened, nothing shipped this leaf).

## Drive status — 2026-07-19 (wave-3 leaf: Stage 2 design pinned, NOT built; still at Stage 1)

This leaf resumed at Stage 2 (the D1 bridge) and did a full end-to-end architecture read of the
mount seams. **Conclusion: Stage 2 wired to actually serve a mount cannot be made gate-green in one
leaf without fabricating several interlocking, unspecified design decisions**, so — per the
attempt-first / closed-outcome rule — NOTHING new was committed beyond Stage 1 (`bb089fe`,
recorded `2cfb9ca`); the tree is clean at `2cfb9ca`, binary `0.0.79`, plugins `0.13.0`, no
acceptance ticked. What this leaf adds is the **evidence-pinned design map** below (the prior notes
named the stages; this pins the exact blockers + seams so the next session builds, not rediscovers).

**The three concrete blockers (all read-verified this leaf):**

1. **Registry routing asymmetry — the core problem.** The plan/describe registry
   (`crates/core/src/registry.rs:594` `resolve_service_path`) routes by **longest-prefix path**, so a
   nested D1 mount at `/cloudflare/d1` would route there fine. But the **read** funnel
   (`crates/exec/src/read.rs:57` `ReadRegistry` = `HashMap<DriverId, Arc<dyn ReadDriver>>`, resolved
   by `.get(id)`) and the **apply** funnel are keyed by **DriverId** (a mount's `outer_id`), NOT by
   path prefix. So `/cloudflare/...` reads/writes all resolve to the ONE driver registered under the
   `cloudflare` id — a nested `/cloudflare/d1` cannot get its own read/apply driver *by leading
   segment*. This is why a naive "register a second CfDriver mount" does not work across all three
   facets.

2. **A promising escape (needs a spike to confirm BEFORE committing to it).** `MountRemap::outer_id`
   (`crates/qfs/src/mount_adapter.rs:113`) is the **full** outer-mount path minus the leading `/`
   (not just the leading segment), and `DriverId` (`crates/types/src/schema.rs:19`) is an
   **unvalidated `String`**. So a mount at `/cloudflare/d1` yields DriverId `"cloudflare/d1"` —
   *distinct* from `"cloudflare"`. If a slash-bearing DriverId flows cleanly through
   plan-lowering → effect target → read/apply `.get(id)`, then the D1 surface can be a **separate
   nested mount** (id `cloudflare/d1`, plan-registry longest-prefix picks it over `/cloudflare`) and
   NO composite facet is needed. **UNVERIFIED / dead-end risk:** if any stage (plan lowering,
   capability qualification, `CALL` proc routing, the `mount_adapter` proc/effect rewrites) assumes a
   single-segment id, this collapses back to needing a **composite `cloudflare` facet** that
   internally dispatches `/cloudflare/d1/…` → CfDriver and everything else → RestDriver across read,
   apply, AND a merged plan/pushdown profile — a substantially larger build. Verify this first (a
   ~30-min spike: register a dummy nested mount, assert a read routes to id `a/b`).

3. **Wildcard D1 resolution gap.** `CfRegistry::d1` (`crates/driver-cf/src/registry.rs:280`) is an
   exact-match `HashMap<String, D1Database>.get(db)`. The declared path `/cloudflare/d1/{database}`
   is a **wildcard**; with no introspection the addressed segment must itself be the D1 api id
   (`D1Database::api_database_id`, `registry.rs:65`, already falls back to the path name when uuid is
   `None` — so `D1Database::new(backend, catalog)` addressed at `/cf/d1/<X>/<t>` uses `<X>` as the
   api id). The missing piece is a CfRegistry that resolves an **arbitrary** d1 key to a template
   `D1Database` carrying the declared `catalog()` — a small new driver-cf capability (e.g.
   `with_d1_template(catalog, backend)` + a `d1()` template fallback), unit-testable in
   `driver-cf/src/tests.rs`. This is forced by the no-introspection model, not a design choice.

**The wire backend is NOT a blocker.** The "HTTP-over-declared-wire CfBackend" the ticket names is
just `HttpApiBackend` (`crates/driver-cf/src/backend.rs:611` `HttpApiBackend::new(exchange,
account_id, token)`), whose D1 URL/req/resp shape already matches the declared endpoint
`/http/cloudflare/accounts/{account}/d1/database/{database}/query`. Build it from the declared
inputs — `account_id` = mount `at_locator` (same as compiled `/cf`), `token` = the resolved
`AUTH ACCOUNT 'cf'` bearer, exchange = `crate::transport::cf_exchange()` (`transport.rs:161`) —
instead of from compiled discovery. `D1Database::discovered(backend, uuid, catalog)`
(`registry.rs:48`) is the exact lift; `DeclaredSqlResource::catalog()` (Stage 1) is the `catalog`.

**Recommended next-session decomposition (build order):**
- **2.0 (spike, no commit):** confirm blocker-2 (slash-bearing DriverId flows through plan→read/apply).
  This decides nested-mount vs composite — the whole shape of Stage 2. Escalate the shape choice
  (nested vs composite, and whether the declared D1 stays nested under `/cloudflare/d1` or moves to
  its own top-level segment — an owner-facing UX call) as a design decision if the spike is ambiguous.
- **2a:** the wildcard-D1 CfRegistry capability + the declared→`CfDriver` composition helper (backend
  from declared inputs + `CfRegistry` from `resource.catalog()` → `CfDriver`), unit-tested hermetically
  over `MockCfBackend`/`MockExchange` (assert ZERO `list_d1_databases`/introspection at build; a read
  issues `d1_query`, not `sqlite_master`).
- **2b:** wire it into the three declared-mount facets (`describe.rs:173`, `commit.rs:355-396`,
  `shell.rs:418-448`) per the 2.0 shape decision — the cross-cutting core.

Then Stages 3-6 as previously mapped. Binary stays `0.0.79`, plugins `0.13.0`; no acceptance ticked.
