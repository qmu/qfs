---
type: Feedback
title: What describe says is not what the system does, in four directions at the agent loop entry point
kind: concern
source: development
created_at: 2026-07-14T01:07:13+09:00
author: a@qmu.jp
supersedes:
severity: moderate
concern_id: what-describe-says-is-not-what
owner: 
mission: 
tickets: []
origin_pr: 39
origin_pr_url: https://github.com/qmu/qfs/pull/39
origin_branch: work-20260713-233938
origin_commit: 3dae249
last_seen: 2026-07-28T21:48:57+09:00
---

# What describe says is not what the system does, in four directions at the agent loop entry point

## Description

`describe` is the **entry point of the documented agent loop** — `SKILL.md` opens with "learn the
node's archetype, columns, supported verbs … **Always read this first**" and tells the agent to build
its statement from what comes back. Four independent findings say that first step lies, and they lie
in different directions, so no single workaround exists:

1. **It wrongly admits.** driver-local's describe ignores its path argument entirely and returns
   `Archetype::BlobNamespace` for every path, so `cd` into a blob *file* is accepted
   (was `cd-into-a-blob-file-is`, PR #41).
2. **It wrongly refuses.** `node_for_path` requires a `/sys/` prefix and returns `None` for the bare
   root, so `describe /sys` raises `UnsupportedVerb` and `cd /sys` fails before any gate; `/slack`'s
   root is rejected by `SlackPath::parse` the same way (was `sys-and-slack-do-not-describe`, PR #41).
3. **It advertises what cannot be run.** `SlackNode::Files` lists `Verb::Rm` with no grammar behind
   it — the taught detach form resolves to a different node and a different verb
   (was `slack-workspace-namespace-still-advertises-verb`, PR #39).
4. **Against a declared driver it returns nothing usable.** `describe /chatwork` reports
   `child_address: none`, every verb `false`, and a single placeholder `value: Json` column — while
   the same path queried returns a real four-column schema. The mount root points at none of its
   children, so there is no path from "the driver is mounted" to "here is its surface". Filed
   2026-07-27 as `20260728085253-declared-driver-undiscoverable-through-describe.md`, which is **not
   yet routed to a mission**.

**Why these belong together, and why the compound is worth more than the parts.** Individually each
reads as a cosmetic introspection gap that no one will get to. Together they mean the loop the
product documents cannot be entered as written, and item 4 makes it strategic: declared drivers are
the direction the project is committed to, and they are the case where `describe` is emptiest.

PR #27 supplied the evidence that this class is **inherited rather than caught**: the compiled Slack
driver had advertised `slack.update` and `slack.delete` since v0.0.89 while the parser could not
express a call to either, and it took a lexer fix to notice. An equivalence proof measured against a
described surface reproduces an untrue advertisement faithfully. The successor mission
`a-declared-write-resolves-a-name-the-way-a-query-does` is preparing to measure a declared twin
against exactly that surface, so the defect is load-bearing now rather than cosmetic.

## How to Fix

Treat the describe seam as one surface with one law — **what `describe` reports is what the system
will do** — rather than fixing four sites separately:

- Give driver-local a describe-time archetype gate so a blob *file* is refused at `cd`.
- Make a driver's bare root a describable catalog node that names its children, for `/sys`, `/slack`
  and every declared mount (item 4 is the same fix, applied where it matters most).
- Derive the advertised verb set from the grammar that actually exists, so a verb with no statement
  behind it cannot be listed.
- Route `20260728085253` into whichever mission takes this on; it is currently unstamped and
  therefore drivable by nobody.
