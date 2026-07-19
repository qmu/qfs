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

## Drive status — 2026-07-19 (wave-4 /monitor leaf: DEFERRED to an owner design ruling; nothing built)

This leaf resumed at Stage 2, re-ran the blocker analysis against source, and completed the
**mechanical half of the 2.0 spike by code-read** (no runtime test committed):

- `DriverId` is an unvalidated `String` (`types/src/schema.rs:19`) and `MountRemap::outer_id`
  (`mount_adapter.rs:113`) keeps the full outer path minus the leading `/`, so a `/cloudflare/d1`
  mount yields the slash-bearing id `"cloudflare/d1"`, distinct from `"cloudflare"` — the nested-mount
  escape is structurally available.
- The plan/describe registry routes by **longest-prefix path** (`core/src/registry.rs:594
  resolve_service_path`, boundary-matched), so `/cloudflare/d1/…` would prefer a nested mount.
- BUT the **read** funnel (`exec/src/read.rs:57` `ReadRegistry`) and apply funnel are keyed by
  `DriverId` via `.get(id)`, and every read driver registered today uses a **single-segment** id
  (`shell.rs`: `local`, `sys`, `transform`, `type`, …). Whether the `DriverId`-keyed read/apply
  funnels and plan-lowering tolerate a slash-bearing id is the ONE unverified seam — it needs a
  runtime spike (register a dummy nested mount at id `a/b`, assert a read resolves `.get("a/b")`),
  which decides nested (A, small) vs composite (B, large).

**Outcome: DEFERRED, not built.** Consistent with the three prior leaves, Stage 2 wired to serve a
real mount cannot be made gate-green in one leaf without first ruling the mount shape (Q1: nested vs
composite) and the D1 address placement (Q2: nested under `/cloudflare/d1` vs its own top-level
segment — a pure owner UX call). The older brief (`20260716214300`) reserves this as Fable-tier
judgment: "Do not start implementation before the brief is ruled." An unattended `/monitor` leaf
cannot make that call, so it is escalated as a discrete decision artifact:
**`20260719004500-cf-declared-d1-mount-shape-owner-ruling.md`** (deferred; blocks this ticket).

No speculative scaffolding was added: blocker-3's wildcard-`CfRegistry` capability and the
declared→`CfDriver` composition helper are shape-independent and safe, but their build stage and
exact home follow the Q1 ruling, and landing a fourth thin pre-ruling commit is precisely what the
brief forbids. The tree is clean at `ed3e075`, binary `0.0.79`, plugins `0.13.0`, no acceptance
ticked. When the owner rules Q1+Q2 in this ticket, a normal drive builds Stages 2–6 per the build
order above and the deferred escalation ticket retires.

Then Stages 3-6 as previously mapped. Binary stays `0.0.79`, plugins `0.13.0`; no acceptance ticked.

## Drive status — 2026-07-19 (wave-5 /monitor leaf: SPIKE RUN + GREEN; owner RULING recorded — nested shape A, nested placement)

The owner ruled (2026-07-19) and authorized running the 2.0 routing spike autonomously (it is
hermetic, not a process/live exercise) and — if green — implementing the **nested** mount-id shape
with the D1 address nested under `/cloudflare/d1`. **The spike ran and is GREEN.**

**Spike (committed as a permanent regression test):** `crates/exec/tests/oneshot.rs`, module
`nested_mount_id_routing_spike` (4 tests, all pass). It registers two OVERLAPPING mounts — the top
`/a` (id `"a"`) and the NESTED `/a/b` (slash-bearing id `"a/b"`, from the default
`Driver::id()` = mount minus the leading `/`) — and proves, end-to-end through the REAL funnels:

1. `read_funnel_routes_the_slash_bearing_nested_mount_id` — a SELECT of `/a/b/x` resolves to the
   NESTED driver via the real read funnel (`plan_query` tags the scan source `"a/b"` by longest-prefix
   over the overlapping `/a`; `ReadRegistry.get("a/b")` — `exec::exec::id_of` — resolves it). The
   returned row is tagged by the driver that served it (`"/a/b"`), so it is attributable. Control: a
   SELECT of `/a/x` still routes to the top id `"a"`.
2. `apply_funnel_targets_the_slash_bearing_nested_mount_id` — `build_plan` of
   `INSERT INTO /a/b/tbl …` lowers the effect target to `DriverId("a/b")` at path `/a/b/tbl` — the key
   the runtime apply funnel (`interpreter::drivers.get(id)`) resolves. No lowering / capability stage
   choked on the slash.
3. `full_commit_path_drives_the_nested_mount_without_choking_on_the_slash` — the whole one-shot COMMIT
   path (capability gate + effect dispatch) drives the nested-mount INSERT to a clean commit (exit 0),
   applying the write at `/a/b/tbl`.
4. `both_funnel_registries_resolve_a_slash_bearing_id_with_no_single_segment_collision` — the shared
   `HashMap<DriverId,_>` `.get(id)` primitive: `"a/b"` resolves and does NOT collide with `"a"`.

**Ruling recorded (owner, 2026-07-19):**
- **Q1 = (A) NESTED MOUNT.** The spike confirms a slash-bearing `DriverId` flows cleanly through
  plan-lowering → the read funnel → the apply funnel. The declared D1 surface becomes its OWN nested
  mount (id `cloudflare/d1`), which the plan/describe registry's longest-prefix router prefers over
  `/cloudflare`. **No composite facet is needed.**
- **Q2 = NESTED PLACEMENT under `/cloudflare/d1/{database}`** (co-located with the plain-REST
  Cloudflare mount; NOT a new top-level segment).

The deferred ruling ticket `20260719004500-cf-declared-d1-mount-shape-owner-ruling.md` is now
RESOLVED (its only content was the ruling request) and archived this leaf.

**What still remains (the build, now design-unblocked):** Stages 2a → 2b → 3 → 4 → 5 → 6 per the
build order above, wired to the **nested-mount** shape. Acceptance item 7/7 is NOT yet ticked — the
compiled `/cf` still serves D1/KV/queues until the declared nested twin passes the conformance suite
(the §13 ratchet forbids deleting first). Binary stays `0.0.79`, plugins `0.13.0`.

**Stage 2a-i LANDED (gate-green, additive):** the wildcard-D1 `CfRegistry` capability
(`crates/driver-cf/src/registry.rs`) — `CfRegistry::with_d1_template(handle)` plus a template
fallback in `d1()`/`has_d1()`/`is_empty()`. A single template handle (backend + declared catalog,
`uuid=None`) answers ANY `{database}` key not explicitly registered, with the addressed segment used
AS the Cloudflare D1 api id (`api_database_id` already falls back to the path name when uuid is
`None`) — the no-introspection resolution the declared `/cloudflare/d1/{database}` mount needs. An
explicit `with_d1` (discovered) registration still wins over the template. Unit-tested hermetically
over `MockCfBackend` (`wildcard_d1_template_resolves_any_database_key_without_introspection`,
`crates/driver-cf/src/tests.rs`): resolves any key, api-id falls back to the queried name, declared
catalog served with ZERO backend I/O, discovered-over-template precedence. `cargo test -p
qfs-driver-cf` 22 pass; `clippy -p qfs-driver-cf --all-targets -D warnings` clean; `fmt` clean.
Purely internal (no CLI surface) — no plugin version bump, no gen-docs/gen-skills change.

**Stage 2a-ii + 2b remain (the cross-cutting core):** the declared→`CfDriver` composition helper
(backend from declared inputs — `HttpApiBackend::new(cf_exchange(), at_locator, AUTH-ACCOUNT-bearer)`
— + `CfRegistry::new().with_d1_template(D1Database::new(backend, resource.catalog()))` → `CfDriver`)
and wiring it into the three declared-mount facets (`describe.rs:173`, `commit.rs:355-396`,
`shell.rs:418-448`) as a NESTED `/cloudflare/d1` mount (id `cloudflare/d1`). Then Stages 3–6.

## Drive status — 2026-07-19 (wave-6 /monitor leaf: STAGE 2a-ii + 2b LANDED, gate-green, additive)

**Commit `ff2085d`** (still in todo; acceptance NOT ticked — the compiled `/cf` still serves D1
until the conformance twin passes, per the §13 ratchet). The cross-cutting core the four prior
leaves deferred is now built and green. The tree is clean at `ff2085d`, binary `0.0.79`, plugins
`0.13.0`.

**What landed (all three facets wired ATOMICALLY, additive — the compiled `/cf` untouched):**
- **Stage 2a-ii — composition helpers.** `cf.rs`: `declared_d1_driver(backend, catalog)` = a
  `CfDriver` over `CfRegistry::new().with_d1_template(D1Database::new(backend, catalog))` (serves the
  declared catalog for ANY `{database}` with ZERO `list_*`/`introspect_d1`); `declared_d1_backend(
  account_id, token)` = the live `HttpApiBackend` over `cf_exchange()`. The `backend` is INJECTED
  (not built inside), so the pure describe mount uses `MockCfBackend` and read/apply use the live
  wire — the seam the Stage-4 twin will drive. `declared_driver.rs`: `declared_sql_mounts()` pairs
  each connected declared driver with the `CREATE SQL` resource whose path leading-segment is its
  name (`+ prefix` = segments before the first `{…}`, e.g. `/cloudflare/d1`); `declared_d1_remap()`
  = `MountRemap::new_prefixed(prefix, "/cf/d1", "cf")` (outer id the slash-bearing `cloudflare/d1`);
  `declared_auth_bearer()` resolves the raw `Secret` through the same `(provider,"default")` /
  `(driver,"default")` `SecretRef` coordinate the `RestDriver` uses; `DeclaredMount` grew
  `at_locator`.
- **Stage 2b — the three facets.** describe (`describe.rs` `register_defined_paths_where`): a NESTED
  plan/describe mount over a cred-free `MockCfBackend` + the declared catalog (pure, network-free).
  read (`shell.rs`): a live `CfReadDriver` over `declared_d1_backend` + declared catalog, wrapped in
  `MountReadDriver` at id `cloudflare/d1`. apply (`commit.rs`): a `LazyApplyDriver` → `cf_apply_driver`
  → `MountApplyDriver` at id `cloudflare/d1` (bind deferred to first apply). All fail-closed per mount.
- **Tests (7 new, hermetic):** `declared_d1_driver_serves_the_declared_catalog_without_introspection`
  (cf.rs — zero I/O); `sql_resource_mount_prefix_*`, `declared_d1_remap_*`, `declared_sql_mounts_*`
  (×2), `declared_auth_bearer_resolves_the_account_provider_bearer` (declared_driver.rs);
  `declared_d1_nested_mount_describes_the_declared_catalog_network_free` (describe.rs — end-to-end
  through the real describe registry, proving the nested-mount routing + remap + declared catalog,
  and that an UNDECLARED table is not describable — no hidden introspection fallback).

**Gates (all exit 0):** `cargo test -p qfs` 402 pass, `-p qfs-driver-cf` 22, `-p qfs-test
cookbook_skills` 1; `clippy --workspace --all-targets -D warnings` clean; `fmt --all --check` clean;
`gen-docs`/`gen-skills --check` in sync; `check-migrations` clean. (The full `cargo test --workspace`
could not complete on this shared host — `/` hit 100% disk from concurrent worktree builds, `os
error 28` at link, NOT a test failure; reclaimed only own `target/incremental` and verified the
affected crates per-crate.)

**What remains (Stages 3–6), unchanged in shape but now atop a live nested D1 twin:**
3. **KV/queues as plain declared REST** (served get/put/push/pull beyond the listings already in
   `cloudflare.qfs`), staged with the compiled `/cf` still alive.
4. **The conformance twin.** The read/apply facets call `declared_d1_backend` → `cf_exchange()` (real
   reqwest) internally, so an end-to-end funnel twin needs a TEST SEAM to inject a
   `MockExchange`/`MockCfBackend` through the facet (e.g. thread an optional exchange override, or
   factor the per-mount driver build behind an injectable backend provider). Then the twin asserts
   the declared surface matches the compiled `/cf` over `MockCfBackend`/`MockHttpClient` + a
   describe-purity (no-network) assertion — BEFORE any deletion (the §13 ratchet).
5. **Delete/demote compiled `/cf`** (`cf.rs:153-242` `driver_from_backend_with_artifact_sealer` +
   `introspect_d1`; `cloud_mounts.rs:54` the `cf` canonical-id arm; `describe.rs` the `"cf"`
   `cred_free_driver` arm; `shell.rs`/`commit.rs` the compiled `cf` read/apply branches) only once
   the twin is green. NOTE `CfReadDriver`/`cf_apply_driver`/`CfDriver` STAY — the declared twin reuses
   them; only the compiled *discovery/registration* path is deleted. High regression surface (the
   `cf.rs` tests, `cloud_mounts` tests, any `/cf` integration test) — validate with a full workspace
   test once disk allows.
6. `cloudflare.qfs` asset extension (a `CREATE SQL` D1 arm) + cookbook + `gen-skills`; plugin version
   bump if `qfs-cloudflare`'s taught surface changes; binary patch bump; then archive + tick mission
   line 142.

Stages 4 + 5 are the all-or-nothing remainder and were NOT attempted this leaf: the twin needs the
injection seam above, deletion is a hard break with a wide regression surface, and the shared host's
disk pressure blocked a full-workspace validation — landing them half-wired is exactly what the
safety floor forbids. Binary stays `0.0.79`, plugins `0.13.0`; no acceptance ticked.
