---
type: Strategy
title: The viewer renders qfs as a walk over trails
slug: the-viewer-renders-qfs-as-a-walk-over-trails
status: active
created_at: 2026-07-22T12:20:44+09:00
author: a@qmu.jp
---

# The viewer renders qfs as a walk over trails

## Direction

qfs-viewer presents qfs as a **walk over trails**, not as a bespoke UI over a query language.
A *trail* is one written path within the path concept — the canonical containment backbone plus
selection/relation/reverse segments, "where you have walked, recorded"; a *walk* is the act of
extending a trail one column at a time, always linear (exactly one trail, never a graph). The
viewer's whole interaction model derives from this: because a qfs path is simultaneously a query
(intension — describe yields schema/keys/relations) and a set (extension — read yields rows), the
column strip renders one object at two aspects rather than two artifacts, and every column is one
step the current trail admits. Non-linearity (a define-time DAG) is confined to a column's
interior, never spread across the strip. The viewer is deliberately a faithful representation of
the subset it covers — 100% parity with everything the query language can express is given up,
and fidelity is the content's responsibility, not the container's. The same column UI carries
the AI-letter concept: a letter is an envelope (context + interactivity) confined inward like a
declared driver's host-confinement, with a single typed egress (a reply is a typed INSERT into
the sender's inbox) and effects only at a walk's terminal column.

The long-lived direction this sets: as qfs grows new paths, relations, and effects, the viewer
extends by teaching the walk new admitted steps — not by growing a parallel configuration
surface. The observable consequence is that a person navigating qfs-viewer and an agent issuing
qfs queries are doing the same thing at two aspects of one object; "who drives is not the design
axis." Completion conditions are deliberately absent — this outlives every recording and
implementation mission that executes it.

## Changelog

<!-- Append-only, dated timeline. One line per event ("- YYYY-MM-DD — event — filename");
     never rewrite past lines. Retirement (rare) is a recorded transition, not a deletion. -->
