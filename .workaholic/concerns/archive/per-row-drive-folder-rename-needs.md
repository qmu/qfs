---
type: Concern
mission: qfs-capability-tryout-file-handling-transformation-and-platform-hardening
tickets: [20260713195008-effect-selector-channel-folder-rename.md]
origin_pr: 37
origin_pr_url: https://github.com/qmu/qfs/pull/37
origin_branch: work-20260713-150833
origin_commit: 14b9a41
created_at: 2026-07-13T16:45:00+09:00
last_seen: 2026-07-14T02:00:00+09:00
first_seen: 2026-07-13T16:45:00+09:00
concern_id: per-row-drive-folder-rename-needs
severity: low
status: resolved
resolved_by_pr: 7b72cab
resolved_by_commit: 
---

# Per-row Drive folder rename needs a predicate/selector channel

## Description

The v0.0.60 Drive folder-UPDATE fix REFUSES a name-path folder rename (safe) rather than renaming the matching child, because a same-column `SET name WHERE name` is unrepresentable in the flat effect RowBatch — `setwhere_row_batch` de-dups the WHERE key when it shares the SET column, so the driver cannot tell the selector from the new value. Renaming the matching child (the richer, non-refusing behaviour) needs the effect representation to carry the WHERE selector distinct from the SET payload.

## How to Fix

Give the effect node a predicate/selector channel separate from the SET row payload (so a same-column filter survives to the driver), then decode_move can resolve the child-by-name and rename it instead of refusing.

> **Deferred (2026-07-13, owner decision):** this is a design-layer change to the shared
> `EffectNode` representation, not a contained driver fix. Captured as ticket
> `20260713195008-effect-selector-channel-folder-rename.md` (routed through a design brief first).
> Status stays `active` until that ticket lands. Its siblings
> `37-new-driver-fs-content-omission.md` and `37-new-drive-select-content-schema-divergence.md` were
> resolved this session (branch `work-20260713-185925`).

> **Design gate cleared (2026-07-14, branch `work-20260714-013531`):** the Fable-grade design brief
> was written and **Codex-reviewed** (all three judgments upheld, ground-truth confirmed), and lives
> on ticket `20260713195008` with the decision recorded in blueprint §7. The design is
> `selector: Option<RowBatch>` on `EffectNode` with uniform `WHERE`→selector / `SET`→args lowering;
> the blast radius is a checklist (gdrive + SQL + **CF D1 mirror** + gmail/slack/transform/sys/cf-KV +
> sql-catalog). Concern stays `active` until the **implementation** lands, but it is now
> implement-ready — no design work remains.
