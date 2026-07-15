---
type: Concern
mission: qfs-capability-tryout-file-handling-transformation-and-platform-hardening
tickets: [20260712005000-drive-multi-row-insert-silent-loss.md, 20260712005100-chatwork-declared-live-read-empty-columns.md]
origin_pr: 33
origin_pr_url: https://github.com/qmu/qfs/pull/33
origin_branch: work-20260711-121525
origin_commit: f1a3d21
created_at: 2026-07-12T01:52:23+09:00
last_seen: 2026-07-12T01:52:23+09:00
first_seen: 2026-07-12T01:52:23+09:00
concern_id: two-live-round-driver-defects-ticketed
severity: moderate
status: resolved
resolved_by_pr: 34
resolved_by_commit: 8596226
---

# Two live-round driver defects, ticketed for the next drive

## Description

The owner-attended switch live round surfaced two driver-side defects that were ticketed rather
than fixed on the branch. (1) **Drive folder multi-row INSERT silent loss**: a 2-row
`INSERT INTO /drive/my/<folder>` reported `affected 2` but wrote exactly one file — an
honest-count violation on the write side; additionally, a missing destination folder fails at
COMMIT though the gdrive cookbook promises a structured error at PREVIEW. (2) **Chatwork declared
live read returns zero-column rows**: `/chatwork/rooms` against the real API returns the right row
count but every row is empty — column values are lost between the HTTP JSON decode and the typed
view, exactly where the hermetic mock's schema-carrying batch stops.

## How to Fix

Both are precisely ticketed in the todo queue (`20260712005000`, `20260712005100`): make the Drive
applier upload every row with per-row failure surfacing and `affected` equal to real writes, plus a
plan-time parent-folder check (or cookbook correction); fix the declared-driver decode→typed-view
column mapping and lock it with a real-shaped JSON fixture.

## Resolution (2026-07-12, PR #34)

Both defects fixed and hermetically locked on branch `work-20260712-015928`: the Drive applier
uploads every row with honest `affected` and exact-progress `partial_apply` failures (`def5f40`);
the zero-column declared read was traced to stale pre-§5.4 type rows and now refuses pre-network
with a heal instruction, with newest-wins type lookup making re-install actually heal
(`8596226`, additionally verified with the real binary against this host's stale System DB). The
owner-attended live re-runs stay tracked by the "Remaining owner-attended live rounds" concern.
