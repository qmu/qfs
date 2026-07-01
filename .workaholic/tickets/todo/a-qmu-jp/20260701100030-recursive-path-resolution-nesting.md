---
created_at: 2026-07-01T10:00:30+09:00
author: a@qmu.jp
type: enhancement
layer: [Domain]
effort:
commit_hash:
category: Added
depends_on: [20260701100010-design-defined-path-model-grammar.md, 20260701100020-defined-path-declaration-grammar-config.md]
---

# Resolution + recursive nesting for user-defined paths

> **DECISION UPDATE (2026-07-01, owner) — see `20260701100010` Design Resolution (authoritative).**
> The verb is **`CONNECT`** (a side-effecting statement), NOT `CREATE CONNECTION`/`CREATE ALIAS`.
> There is **NO `connections.qfs` file** — a connection is **server state**; running `CONNECT` mutates
> the project **DB** (the single source of truth), like a connection/DDL statement against MySQL/Postgres.
> qfs is **experimental: NO backward compatibility / deprecation** — old fixed mounts are simply removed,
> not shimmed. Ignore any `connections.qfs` / `CREATE CONNECTION` / deprecate-not-break wording below.


Part of EPIC `20260701100000`. Makes a declared, persisted defined-path (`100020`) actually
**resolve** through the engine — including the recursive `/<folder1>/<folder2>/<resource>` grouping.

## What's already done vs the gap (discovery, 2026-07-01)

- `MountRegistry::resolve_path` (`crates/core/src/registry.rs:421`) **already** longest-prefix-routes
  multi-segment mounts by boundary match — so a recursive user mount routes with NO router change.
- Every driver-facing `Path` is rebuilt as `/<driver.id()>/<sub>` (`resolve.rs:622`,
  `eval.rs:{538,690,837}`, `plan.rs:129`), so with a canonical `id()` (keystone decision) the
  per-driver parsers keep working untouched.
- **The gap:** `resolve_driver_namespace` (`crates/core/src/resolve.rs:599`) builds a
  **single-segment** `/<namespace>` for CALL/alias receiver routing — it breaks for a multi-segment
  user mount. The recursive-nesting semantics from the keystone (multi-segment mount vs folder-tree
  grouping) must be implemented in resolution + precedence.

## Plan

1. **Multi-segment receiver routing.** Fix `resolve_driver_namespace` (and any single-segment
   `/{namespace}` assumption in `resolve.rs`/`eval.rs`/`plan.rs`) to handle a multi-segment
   user-defined mount, so `CALL`/pipeline-alias resolution works on a defined path.
2. **Recursive grouping semantics.** Implement the keystone's decision for
   `/<folder1>/<folder2>/<resource>` — whether a folder is part of one driver's multi-segment mount
   or a grouping node over several defined paths — and the resolution + precedence rules vs
   `resolve_name` ranking (`Reserved > Lexical > Mount > Connection > Unbound`, `registry.rs:195`),
   slotting user defined-paths in without letting one shadow a realm.
3. **Audit the id()-reconstruction sites.** Confirm all four `/<driver.id()>/<sub>` rebuild sites
   behave under multi-segment user mounts; add coverage for a driver registered under a
   non-canonical multi-segment mount (today only the `/ga` single-segment alias exercises this).

## Key files

- `crates/core/src/resolve.rs` (`resolve_driver_namespace:599`, reconstruction `:622`,
  `render_mount_path`), `crates/core/src/registry.rs` (`resolve_path`, `resolve_name`, `peel_scope`),
  `crates/core/src/{eval.rs,plan.rs}` (the other reconstruction/id-keyed sites).

## Considerations

- The `register()` realm-shadow guard only checks the FIRST segment — fine, since realms are
  first-segment; keep it enforcing on user mounts.
- Add tests for a driver under a multi-segment user mount end-to-end (route → reconstruct → parse →
  scan), the safety net discovery flagged as missing.

## Policies

- `design/modeless-design` + `implementation/accessibility-first` (the recursive namespace stays
  reachable by users AND AI agents, no dead paths), `design/access-control` (no escalation via
  unanticipated recursive constructions), `implementation/domain-layer-separation` (resolution stays
  domain-layer), `implementation/type-driven-design`.
