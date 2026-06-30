---
created_at: 2026-07-01T10:00:00+09:00
author: a@qmu.jp
type: enhancement
layer: [Domain, UX, Infrastructure]
effort:
commit_hash:
category: Added
depends_on: []
---

# EPIC: User-defined "defined paths" replace pre-designated driver mounts (+ recursive nesting)

## The goal (owner, 2026-07-01)

Today every driver hardcodes a fixed first-path mount (`/ga`, `/github`, `/mail`, `/slack`, `/sql`,
`/s3`, …). The owner wants to invert this:

1. **Only a MINIMUM set of SYSTEM-defined first-paths remain** (the realms + the few driver-backed
   system mounts). No more pre-designated per-driver mounts for third-party services.
2. **Every third-party / connecting driver is bound to a USER-chosen path** — and that binding is
   declared **at the same time the user configures the credential**. One declaration ties
   `{ path → driver + credential }`. The concept formerly called an **"alias" is renamed
   "defined path"** (one-concept-one-word; `alias` already means a pipeline-verb alias like `SEND`).
3. **Recursive / nested path grouping.** A user can define hierarchical paths —
   `/<folder1>/<resource1>` or `/<folder1>/<folder2>/<resource>` — so the path namespace is a
   user-shaped tree, not a flat driver list.

This supersedes the queued `CREATE ALIAS` shorthand ticket (`20260630204000`, moved to `abandoned/`
as subsumed) and extends the shipped **in-language connection declaration** epic
(`20260630004100`, `CREATE CONNECTION`). It is a change to qfs's **versioned surface** (grammar +
registries), so it must be **additive / deprecate-not-break**.

## Why this is feasible without a parser rewrite (discovery, 2026-07-01)

The decoupling seam ALREADY exists, which is what makes this an epic of wiring + grammar rather than
a ground-up rewrite:

- Every driver-facing `Path` is **reconstructed as `/<driver.id()>/<sub>`** before it reaches the
  driver (`crates/core/src/resolve.rs:622`, `eval.rs:{538,690,837}`, `plan.rs:129`). So a driver's
  per-path parser only ever sees its **canonical** prefix — never the user-facing mount. If
  `Driver::id()` stays canonical, the per-driver `path.rs` parsers keep working **untouched** under a
  user-defined mount. (`Driver::id()` defaults to `mount().strip_prefix('/')`, `driver/lib.rs:587` —
  the load-bearing coupling; see ticket 203110's insight.)
- `MountRegistry::resolve_path` (`registry.rs:421`) **already routes multi-segment mounts** by
  longest-prefix boundary match — so `/<folder>/<sub>/<resource>` user mounts route with **no router
  change**. The single known single-segment assumption is `resolve_driver_namespace` (CALL routing,
  `resolve.rs:599`).
- `MountRegistry::register_alias` (`registry.rs:381`, shipped by `203110`/commit `754a348`) is the
  literal precedent: a routing-only second key into the SAME driver `Arc` with the SAME canonical id.
  A user "defined path" is this, generalized and declared in-language.
- `RESERVED_REALMS` (`registry.rs:33`) is the existing closed, system-defined first-path set — the
  precedent for "minimal system mounts; everything else user-bound". Realms are NOT mounts; only
  `/sys` is driver-backed.

## Sub-tickets

1. `20260701100010` — **Design keystone**: the `defined path` model, terminology, grammar shape
   (contextual idents), the `id()`-stays-canonical decision, the minimal system-mount set, recursive
   nesting semantics, and the deprecate-not-break framing. **Gates the rest.**
2. `20260701100020` — **Declaration grammar + config**: add the `defined path` clause to the parser
   AST + grammar (no new frozen keyword) and the `connections.qfs` / `DeclaredConnection` model;
   persist the `{path → driver + credential}` binding at `connection add`. (Subsumes `204000`.)
3. `20260701100030` — **Resolution + recursive nesting**: route user-defined multi-segment mounts
   through the resolver / eval / plan reconstruction sites; fix the single-segment CALL-routing
   assumption; the recursive folder-grouping semantics.
4. `20260701100040` — **Registration redesign + minimal system set + mount deprecation**: replace the
   three static registration sites (shell / describe / commit) with a minimum-system-set + a
   per-declared-connection binding loop, keeping `id()` canonical; deprecate (not delete) the old
   per-driver mounts.

## Considerations

- **Freeze-safety is the spine of this epic.** The new declaration clause MUST be a contextual ident
  (like `CONNECTION`/`DRIVER`/`SECRET`/`AT`), never a new keyword, or it is a MAJOR grammar break.
  Removing per-driver mounts must be a DEPRECATION (old mounts keep routing for one release with a
  warning + migration), not a deletion.
- **Fail-closed** must hold: an unconfigured / uncredentialed user path leaves the driver
  UNREGISTERED with a clear "no source" error, never a faked success (the `commit.rs` pattern).
- **Anti-drift docs**: the mount model is rendered into `docs/drivers.md` by `xtask gen-docs`;
  every slice regenerates and keeps `gen-docs --check` green.

## Policies

- `design/rest-api-design` — versioned-surface + deprecate-not-break for the mount/grammar change.
- `design/access-control`, `design/data-sovereignty`, `design/defense-in-depth` — a binding ties a
  credential to a path; single authoritative authz layer, least-privilege, no escalation via
  unanticipated (recursive) path constructions.
- `design/modeless-design`, `implementation/accessibility-first` — the user-defined recursive
  namespace must stay reachable without modes, by users AND AI agents.
- `implementation/type-driven-design` — recursive paths + bindings as value objects / sum types, not
  bare strings; the freeze-safety check is mechanical.
- `implementation/domain-layer-separation`, `implementation/persistence` — resolution + binding logic
  in the domain layer; the binding registry is schema-first persisted state in qfs's own DB.
- `planning/terminology` — `alias` → `defined path` rename updated everywhere in one change.
- `implementation/directory-structure`, `implementation/coding-standards` — always apply.
