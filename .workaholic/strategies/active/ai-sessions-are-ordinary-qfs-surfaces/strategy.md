---
type: Strategy
title: AI sessions are ordinary qfs surfaces
slug: ai-sessions-are-ordinary-qfs-surfaces
status: active
created_at: 2026-07-22T08:52:02+09:00
author: a@qmu.jp
---

# AI sessions are ordinary qfs surfaces

## Direction

A machine's running AI agents are not a special case reachable only by sitting at a terminal —
they are ordinary qfs paths (owner directive, 2026-07-16: "anything you can do at a Claude Code
session, you can do with a qfs query"). Observing sessions, reading their state, steering them,
and launching them ride the same describe/preview/commit query discipline as every other
surface, under `/hosts/<host>/claude/...`, so fleets of sessions become queryable, auditable
infrastructure rather than terminal folklore. Safety is part of the direction, not a caveat:
process-touching operations are non-destructive by construction (durable inboxes over process
signals) and any live exercise runs only in isolation, because the surface being built is the
same surface the builders are running on.

## Changelog

<!-- Append-only, dated timeline. One line per event ("- YYYY-MM-DD — event — filename");
     never rewrite past lines. Retirement (rare) is a recorded transition, not a deletion. -->
