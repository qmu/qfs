---
type: Concern
mission: 
tickets: [20260711121532-switch-predicate-model-routing.md, 20260711121530-pdf-extraction-to-drive-pipeline.md, 20260711121536-command-execution-risk-assurance.md, 20260711121533-dependency-reduction-execution.md]
origin_pr: 33
origin_pr_url: https://github.com/qmu/qfs/pull/33
origin_branch: work-20260711-121525
origin_commit: f1a3d21
created_at: 2026-07-12T01:52:23+09:00
last_seen: 2026-07-15T16:35:34+09:00
first_seen: 2026-07-12T01:52:23+09:00
concern_id: scope-cuts-and-monitored-items
severity: low
status: active
resolved_by_pr: 
resolved_by_commit: 
---

# Scope cuts and monitored items

## Description

Recorded, deliberate cuts and watches: the switch first slice (blueprint §18) requires `else` on every switch, defers all-pure switch, keeps arm vocabulary row-local, excludes UPDATE/REMOVE arm terminals, and lists fired arms only in the committed summary; the PDF inline byte caps are set conservatively and long-PDF chunking is out of scope; the exec-inventory `cfg(test)` stripper is heuristic (ri… (`scope-cuts-and-monitored-items.md`, origin `f1a3d21`)

## How to Fix

Each switch cut lifts as its prerequisite lands (notably refinement-carrying schemas for exhaustiveness without `else`); tune the PDF caps after the live round; tighten the stripper with a real syntax pass only if it misbehaves; drop `async-trait` when native async-in-trait covers its sites; residue cleanup is the owner's call (`remove transform triage` drops the definition); script the localhost-…

