---
created_at: 2026-09-28T02:11:22+09:00
status: done
author: a@qmu.jp
assignees: [a@qmu.jp]
depends_on: [20260928021122-cluster-members-join-a-host-over-an-authenticated-websocket-and-report-their-resources.md]
mission: a-qfs-cluster-lets-a-member-borrow-the-host-s-connections-and-shows-its-pools
merge_policy:
verification_handoff: 
---

# The host serves a column GUI over its server pool, account pool and sessions

## Overview

The host shows the cluster in a column-oriented GUI built with plgg / plggmatic: a **server
pool** column (members with CPU, memory, disk and status), an **account pool** column
(Claude Code / Codex accounts registered on the host — metadata only in this slice) and a
**session allocation** column (member → directory → session, with status and last message).
Selecting a row in one column fills the next, in the plggmatic walk style.

## Policies

- `workaholic:implementation` / `policies/directory-structure.md` — conventional project layout
- `workaholic:implementation` / `policies/coding-standards.md` — style and structure conventions
- `workaholic:design` — the GUI is read-only and served on loopback by default

## Key Files

- `packages/qfs/crates/qfs/src/cluster.rs` — `/cluster/accounts` path and `qfs cluster account add`
- `packages/qfs/crates/cluster/` — a small HTTP endpoint on the host listener: `GET /` (the SPA) and `GET /api/cluster/{members,accounts,sessions}`
- `packages/qfs-viewer/packages/` — the plgg / plggmatic family the SPA is built with
- `packages/qfs/crates/cluster/ui/` (new) — the bundled SPA

## Implementation Steps

1. `/cluster/accounts` on the host (provider, label, email, plan, status, assigned_member) with `qfs cluster account add|list`; no credential material stored in this slice.
2. JSON read API on the host listener for the three collections.
3. A plggmatic column SPA: three columns, walk-by-selection, auto-refresh every 5 s. Build it with the plgg toolchain and embed the built bundle into the binary.
4. `qfs cluster host` prints the GUI URL at startup.

## Quality Gate

**Acceptance criteria** — the checkable conditions that must hold:

- Opening the printed URL shows the three columns filled from a live host with one joined member
- Adding an account with `qfs cluster account add` makes it appear in the account pool column

**Verification method** — the commands/tests/probes that prove them:

- A test over the JSON API; a browser check by hand (or Playwright) against a local host + member

**Gate** — what must pass before approval:

- The workspace gates in `CLAUDE.md`; the SPA build is reproducible from the repository

## Considerations

- The account pool is data only here; switching accounts on a usage limit is later work.
- If the plggmatic column engine cannot be built from inside this repository, record which part is missing rather than hand-rolling a second UI framework.

## Final Report

Development completed as planned.

### Discovered Insights

- **Insight**: The console is built on plgg + plgg-view only, not plggmatic (developer correction mid-ticket); plggmatic served as the design reference for the column walk and the two-scheme role palette.
  **Context**: `packages/qfs-viewer/packages/cluster-console` has its own pure walk core (`domain/usecase/walk.ts`: area column → list → member detail → directories → sessions → session detail) and a plgg-view view in `entrypoints/main.ts`. The package's runtime deps are `plgg` and `plgg-view` only.
- **Insight**: plgg-bundle's `"app"` target resolves bare specifiers through workspace discovery (sibling packages, then `node_modules`), not through the configured self-alias, so a `#cluster-console/...` import fails to bundle; the console imports its own modules by relative `.ts` path instead.
  **Context**: A bare `cluster-console/...` self-import bundles but is flagged by `gate-vendor-boundary.sh` as third-party. Relative imports satisfy both. Note the app target would also inline the SIBLING `packages/plggmatic` source (siblings win over `node_modules`) if anything imported `plggmatic`.
- **Insight**: The embedded console is a committed build artifact (`packages/qfs/crates/cluster/ui/index.html`, ~260 KB, deterministic), so `cargo build` needs no node; `./scripts/build-cluster-console.sh` regenerates it and `--check` (run by `check-all.sh` via `test-cluster-console.sh`) fails when it is stale.
  **Context**: `include_bytes!` in `host.rs` serves it at `GET /` and `/index.html`, loopback-only like the JSON API.
- **Insight**: On this machine's Node 23.x, `check-all.sh` fails in steps unrelated to this ticket: plgg-bundle 0.0.6's and qfs-viewer's own `relocate.mjs` call `rmSync` on a directory symlink left by a previous run (`ERR_FS_EISDIR`), and the npx smoke compares `--version` output that Node 23 prefixes with a type-stripping `ExperimentalWarning`.
  **Context**: Every gate this ticket touches passed individually (dependency and vendor-boundary gates, qfs-viewer 386 tests, plggmatic 311 tests, cluster-console 5 tests at 100% line coverage, embed `--check`). The full script needs Node 24, which the scripts already assume.
