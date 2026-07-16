---
created_at: 2026-07-17T01:04:00+09:00
author: a@qmu.jp
type: refactoring
layer: [Domain, UX]
effort:
commit_hash:
category: Changed
depends_on: [20260717010200-claude-mount-registration-and-e2e-guard.md]
mission: claude-code-sessions-are-queryable-and-steerable-as-qfs-paths
---

# Path canon: /hosts/<host>/claude/... is canonical, top-level /claude retires

## Overview

Mission acceptance item 1 (owner ruling, 2026-07-16): `/hosts/<host>/claude/...` is the
canonical path and top-level `/claude` retires — honouring t64's own title
(`20260626102200-t64-claude-driver.md`), which the shipped code contradicted
(`driver-claude/src/schema.rs:30` mounts bare `/claude`). qfs is experimental; a hard break is
correct.

Machinery gap, verified this session: `peel_scope` (`core/src/registry.rs:234`) resolves the
`/hosts/<principal>/…` realm, but **the planning path never calls it** — `plan_pipeline`
(`core/src/plan.rs:96-108`) feeds the raw path to `mounts.resolve_path`, so
`/hosts/local/claude/sessions` today lowers to a synthetic `hosts` source and dies as
`unknown_source`. The canonical shape needs realm peeling wired into path→mount resolution (at
least for the local host), host-name resolution for "this box" (`hosts.rs` seeds `local`), and
a POLICY re-check seam for the remote hop (t63 tunnel — documented, explicitly out of mission
scope).

Ship, in order:

1. blueprint records the ruling (canonical form, retirement, and why);
2. realm peeling wired so `/hosts/local/claude/sessions` (and the host's real name, if the
   registry names one) resolves to the claude mount — the general `/hosts/<h>/<svc>` peel, not
   a claude special-case;
3. bare `/claude/...` retires: structured error pointing at the canonical form (hard break;
   grammar/docs/query-cookbook.md examples updated; plugin version bump per CLAUDE.md if a
   taught surface changes — query-cookbook.md:374,3765-3798 already teaches the /hosts form);
4. the dangling citation `lib.rs:1` ("roadmap §3.3 / M7, t64" — `docs/roadmap.md` has no
   numbered sections, no M7, no t64, zero `/claude` mentions) is corrected against the roadmap
   that exists, and `/claude` gains its honest roadmap marker.

## Policies

- `workaholic:implementation` / reachability — one canonical address per surface; a retired
  alias fails with a pointer, never a silent second path.
- `workaholic:development` / change history — the hard break is recorded in the blueprint with
  the owner ruling and date, not just in code.

## Quality Gate

1. `/hosts/local/claude/sessions |> LIMIT 1` returns rows over a fixture store (e2e, spawning
   the real binary).
2. Bare `/claude/sessions` returns the structured retirement error naming the canonical path.
3. peel_scope-driven resolution has unit coverage for `/hosts/<h>/claude/...` including the
   cross-realm rejection cases that already exist.
4. gen-docs/gen-skills `--check` green after doc regeneration; grep proves no doc teaches bare
   `/claude` any more.

## Considerations

- Residue from the first slice (2026-07-17): the read path was mounted at top-level `/claude`
  because realm peeling was missing from planning — the minimal honest step. This ticket owns
  the move; the slice's e2e guards must be updated to the canonical form here.
- Decide what non-`local` host names do before the tunnel exists: fail closed with
  "remote hosts are not yet executable" (the `require_known_host` precedent in `hosts.rs`).
- Realm peeling in planning likely affects `/members`/`/projects` paths too — keep the change
  general but gate non-hosts realms to their existing behaviour (no accidental widening).
