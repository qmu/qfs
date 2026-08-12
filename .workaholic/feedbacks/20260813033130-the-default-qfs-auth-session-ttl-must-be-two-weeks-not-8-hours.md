---
type: Feedback
title: The default qfs auth session TTL must be two weeks, not 8 hours
kind: instruction
source: discussion
created_at: 2026-08-13T03:31:30+09:00
author: a@qmu.jp
supersedes: 
---

# The default qfs auth session TTL must be two weeks, not 8 hours

The default session TTL for `qfs auth` must be TWO WEEKS, not 8 hours.

Given as an instruction in conversation (2026-08-13), and stated by the developer as a repeat: "いやいや既定の8hがおかしい、2weekとFBしたはずです". Registered now because the earlier telling never became a record — searched on 2026-08-13 across all 92 feedback records, every ticket in todo and archive, every mission, story and doc in this repository, and the workaholic repository's own stream: nothing asks for a two-week TTL. The 8h default has stood unchanged since the mechanism shipped (ticket 20260704170000), so the ask was lost in a session rather than declined.

Two facts that make this more than a constant edit:

1. Two weeks is currently UNREACHABLE even with the documented override. `resolved_ttl_secs` clamps to MAX_TTL_SECS = 7 days, so `QFS_SESSION_TTL=14d qfs auth` silently yields 7d — the clamp does not say it clamped.
2. The session key folds in the boot id, so any TTL is really "until the next reboot". On this server that is usually a long time, but the developer should decide whether a two-week session is meant to survive a reboot; the reboot binding is a security property, not an accident.
