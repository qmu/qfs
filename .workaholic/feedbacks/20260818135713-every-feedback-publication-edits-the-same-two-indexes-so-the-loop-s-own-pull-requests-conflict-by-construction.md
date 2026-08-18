---
type: Feedback
title: Every feedback publication edits the same two indexes, so the loop's own pull requests conflict by construction
kind: concern
source: development
subject: observer_ai:[Housekeep] routine
created_at: 2026-08-18T13:57:13+00:00
author: a@qmu.jp
supersedes: 
---

# Every feedback publication edits the same two indexes, so the loop's own pull requests conflict by construction

Every feedback-filing publication touches the same two shared files — `.workaholic/feedbacks/index.md` and `.workaholic/index.md` — so any two open publication pull requests conflict with each other by construction, and each merge makes the remaining ones conflict.

Measured at the 2026-08-18 13:51 UTC housekeep tick, over an untruncated read (`pulls-state.sh --limit 20`, `total_open: 15`, `read: 15`, `truncated: false`):

- 15 open pull requests, 11 reading `mergeable: false` / `mergeable_state: dirty` (#29 #46 #50 #57 #59 #64 #65 #77 #78 #79 #80) and 4 reading `unknown` (#71 #72 #75 #76).
- Six of the conflicted set (#75 #76 #77 #78 #79 #80) are the loop's own feedback publications, filed by housekeep ticks between 06:52 and 12:56 UTC today.
- Their file lists are near-identical: one new `.workaholic/feedbacks/<stem>.md` (no conflict possible) plus `.workaholic/feedbacks/index.md` and `.workaholic/index.md` (conflict guaranteed against any sibling).

Why it matters: the conflict is not caused by the content of any finding, so no author can avoid it and no reviewer can pre-empt it. At roughly one publication per hour and no publication merged since #74, the queue grows monotonically and every hour adds one more hand-resolution for a human. The `stuck-prs` reminders the loop posts hourly are therefore reporting a backlog the loop itself manufactures, which is why the earlier records about that step (20260818075621 unknown-as-clean, 20260818105743 read cap, 20260818125607 unstable digest) each describe a symptom and none names this cause.

Pointer only, per the sweep's quoting rule: pull request numbers and the two file paths above; no diff content, no message bodies.

Candidate directions, none of them decided here — this is a record, not a ruling:

- Make the OKF indexes generated at merge time rather than committed per publication, so a publication carries only its own new file.
- Give the index an append-only shape a textual merge can union (one line per record, ordered by stem), the way `persist-log.sh` already unions the tick log by `(tick, step)` instead of replaying a patch.
- Have the publication seam re-open its tree at a freshly fetched base and re-render the index just before pushing, which narrows the window without closing it.
- Merge publications promptly (or auto-merge them), so at most one is ever open.
