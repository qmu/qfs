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
