---
created_at: 2026-07-07T03:49:22+09:00
author: a@qmu.jp
type: housekeeping
layer: [Domain, UX, Infrastructure]
effort: 2h
commit_hash: e8c0d82
category: Changed
depends_on:
---

# Reorganize qfs documentation as a current design snapshot

## Overview

The qfs documentation has accreted supplemental notes while the system design changed quickly:
defined paths, mount-bound accounts, labeled OAuth apps, `/sys` administration, event-sourced DDL
history, dump/restore, declared drivers, jobs, and server/automation surfaces now form one design.
Reorganize the docs so they describe today's qfs design as a coherent snapshot, not as a pile of
incremental appendices.

The output should read like the current source of truth for operators and agents: what qfs is, which
state lives where, how a query becomes preview/commit work, how credentials and OAuth app labels are
selected, how `/sys` is administered and backed up, and which surfaces are intentionally preview-only
or live-gated.

## Policies

- `workaholic:implementation` / `policies/directory-structure.md` — keep generated docs, hand-written
  docs, skills, and source comments in their existing ownership boundaries; do not create a parallel
  documentation tree.
- `workaholic:implementation` / `policies/coding-standards.md` — documentation must describe the
  actual typed/domain surfaces rather than historical shorthand or stringly behavior.
- `workaholic:design` — the docs are part of the product interface for operators and AI agents; they
  must make the system's capabilities, safety boundaries, and failure modes discoverable without
  tribal memory.
- `workaholic:operation` — the snapshot must cover backup/restore, migration/event history, live
  credential requirements, and deployment/runtime checks as operational facts.

## Key Files

- `packages/qfs/docs/` — generated and checked documentation that must remain in sync with source
  contracts.
- `packages/qfs/crates/qfs/src/docs.rs` — doc generation tests and rendering entry points.
- `packages/qfs/crates/qfs/src/google.rs`, `account.rs`, `connection.rs`, `path_binding.rs`,
  `commit.rs` — current account/app/mount behavior that the docs must explain accurately.
- `packages/qfs/crates/qfs/src/dump.rs`, `restore.rs`, `sys.rs` — current `/sys`, event-log,
  dump/restore design that should be documented as one state-management model.
- `packages/qfs/crates/parser/src/grammar.rs` — query-language forms the docs should present as
  current syntax, especially `CONNECT`, `CREATE ACCOUNT`, jobs, drivers, and DDL.
- `packages/qfs/xtask` — `gen-docs` and `gen-skills` checks that prevent generated docs from drifting.

## Related History

- `.workaholic/tickets/archive/work-20260707-025845/20260706175249-multi-oauth-app-per-provider.md`
  — labeled OAuth apps changed the app/account/mount model and must be reflected in the design
  snapshot.
- `.workaholic/tickets/archive/work-20260707-025845/20260707022409-ddl-event-log-schema.md`,
  `20260707022410-record-ddl-events-on-config-writes.md`, `20260707022411-dump-current-qfs-state.md`,
  and `20260707022412-restore-and-replay-qfs-state.md` — qfs state history and backup/restore now
  need first-class documentation.
- v0.0.26 dependency posture docs in `docs/blueprint.md` remain useful background, but this ticket is
  about reorganizing the user/operator design docs rather than appending another supplement.

## Implementation Steps

1. Inventory the current docs and generated outputs. Identify duplicated historical sections,
   outdated single-app/default-account language, and supplemental notes that should become primary
   design sections.
2. Propose a coherent documentation outline for today's qfs design. Recommended top-level flow:
   mental model, query/preview/commit loop, source and mount registry, credentials/accounts/OAuth
   apps, `/sys` administration, DDL/event history, dump/restore, automation/server surfaces, and
   operational gates.
3. Rewrite or reorganize the relevant docs so each current design concept has one canonical home.
   Preserve generated-doc banners and use the existing generator where applicable.
4. Update examples to use current syntax: labeled `qfs app add google <label>`, `qfs account add
   google --app <label>`, `qfs connect ... --account ... [--app ...]`, `CREATE ACCOUNT ... APP`, and
   current dump/restore commands.
5. Remove or demote stale supplement-style explanations that contradict the current design.
6. Regenerate/check docs and skills, and update any tests that intentionally pin documentation text.

## Quality Gate

Acceptance criteria:

- The documentation presents today's qfs design as a coherent snapshot, with no primary section still
  describing the retired single Google app/default-slot model or active-account selection.
- The snapshot covers current state management: Project DB/System DB roles, `/sys` current-state
  views, DDL/config event history, and secret-free dump/restore.
- Examples use current CLI and query-language syntax for accounts, app labels, mounts, dump/restore,
  jobs, and declared drivers.
- Generated docs and embedded skill/docs artifacts remain in sync with source contracts.

Verification method:

- `cargo run -p xtask -- gen-docs --check`
- `cargo run -p xtask -- gen-skills --check`
- `cargo test -p qfs docs::tests`
- `cargo test -p qfs-cmd`
- Manual review of the reorganized table of contents against the current source surfaces listed in
  Key Files.

Gate:

- All commands above pass, and manual review confirms the docs read as a current design snapshot
  rather than incremental release notes.

## Considerations

- Do not hide live-only limitations. The docs should explicitly call out live-gated Cloudflare and
  Postgres acceptance paths, and preview-only behavior where a driver has no commit facet.
- Avoid duplicating generated reference material by hand. Prefer reorganizing narrative docs around
  generated references and keeping examples minimal but current.
- Treat AI-agent readers as first-class users: every operational path should name the DESCRIBE →
  PREVIEW → COMMIT loop, safety gates, and backup/recovery commands precisely.

## Final Report

Development completed as planned.

### Discovered Insights

- **Insight**: The generated qfs skill artifacts are downstream of the cookbook pages, so changing the
  Gmail/Drive/FAQ setup text requires `cargo run -p xtask -- gen-skills` in addition to the docs
  generator.
  **Context**: Keeping the plugin skills synchronized prevents agents from learning stale account and
  OAuth app syntax after the website docs have been corrected.

### Verification

- `cargo fmt --all`
- `cargo run -p xtask -- gen-docs --check`
- `cargo run -p xtask -- gen-skills --check`
- `cargo test -p qfs docs::tests`
- `cargo test -p qfs-cmd`
