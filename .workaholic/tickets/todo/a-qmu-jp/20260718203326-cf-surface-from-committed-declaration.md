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
