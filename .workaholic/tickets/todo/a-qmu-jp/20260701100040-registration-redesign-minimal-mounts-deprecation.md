---
created_at: 2026-07-01T10:00:40+09:00
author: a@qmu.jp
type: enhancement
layer: [Domain, Infrastructure, UX]
effort:
commit_hash:
category: Changed
depends_on: [20260701100010-design-defined-path-model-grammar.md, 20260701100020-defined-path-declaration-grammar-config.md, 20260701100030-recursive-path-resolution-nesting.md]
---

# Registration redesign: minimal system mounts + per-connection binding loop + deprecate old mounts

Part of EPIC `20260701100000`. The final slice: stop hardcoding per-driver mounts and instead
register each driver under its user-declared defined path, keeping only a minimal system set — while
**deprecating, not deleting**, the old per-driver mounts.

## The three static sites to redesign (discovery, 2026-07-01)

Every driver mount is wired statically at three places; all switch from "bulk register under each
driver's `const MOUNT`" to "register the minimum system set + loop over declared connections,
registering each driver instance under its user-defined (possibly multi-segment) path, with `id()`
canonical":

1. `crates/qfs/src/shell.rs` (`run_engine_and_reads:177`, `register_google_planning_mounts:153`) —
   planning + read facets.
2. `crates/qfs/src/describe.rs` (`describe_registry:53`) — the cred-free DESCRIBE/catalog registry;
   must register declared bindings or DESCRIBE won't surface user-defined paths
   (`all_registered_mounts_describe_cred_free:192` re-founds on declared connections).
3. `crates/qfs/src/commit.rs` (`register_google:450`, `register_objstore:363`) — the apply registry,
   behind live credentials; the credential a connection carries drives the user-mount registration.

## Plan

1. **Minimal system set.** Register only the keystone-decided system mounts (realms are not mounts;
   likely `/sys` + `/local`, possibly `/git`). Generalize the existing "declared connection → built
   mount" pattern (today only `/sql`, `/git` via `crate::sql::conn_registry` reading
   `declared_for('sqlite')`) to ALL drivers, with the connection AS the mount (not the 2nd segment).
2. **Per-connection binding loop.** For each declared defined-path binding (`100020`), construct the
   driver instance and `register`/`register_alias` it under the user path, keeping `id()` canonical
   so the reconstruction sites + parsers (`100030`) keep working. Preserve **fail-closed**: an
   unconfigured/uncredentialed binding leaves the driver UNREGISTERED with a clear error, never a
   faked success.
3. **Deprecate old per-driver mounts.** Keep the legacy mounts (`/github`, `/mail`, `/drive`,
   `/slack`, `/s3`, …) routing for ONE release as built-in deprecated defined-paths, each emitting a
   deprecation warning + pointing at the `connection`/`path` migration (mirror the
   `/ga`→`/google-analytics` shim, ticket `203110`, `register_alias`). Then schedule removal.
4. **Docs + version.** Regenerate `docs/drivers.md` (the catalogue now reflects declared paths);
   `gen-docs --check` green; bump the patch version (the mount model is the versioned surface).

## Key files

- `crates/qfs/src/{shell.rs,describe.rs,commit.rs}`, `crates/qfs/src/{sql.rs,connections_config.rs}`
  (the declared-connection→registry seam), `crates/core/src/registry.rs` (`register`/`register_alias`),
  `crates/qfs/src/catalog.rs` (`representative_path` / `driver_catalog` for docs).

## Considerations

- This is the highest-blast-radius slice: it changes how EVERY driver mounts. Land it behind the
  deprecation shim so existing `/github` etc. queries keep working through the transition.
- `describe_registry` registering under multiple keys would double-count a driver in `driver_catalog`
  (the `203110` GA insight) — register the canonical/user path for docs, the deprecated legacy mount
  as routing-only.
- Re-found the `all_registered_mounts_describe_cred_free` test on declared bindings.

## Policies

- `design/rest-api-design` (deprecate-not-break is the core discipline of this ticket),
  `implementation/domain-layer-separation` (the minimal-set + binding loop is domain logic, not CLI
  glue), `implementation/persistence` (registration is founded on the persisted binding store),
  `design/access-control`/`design/data-sovereignty` (a binding registers a credentialed path),
  `implementation/accessibility-first` (DESCRIBE surfaces user paths for AI agents).
