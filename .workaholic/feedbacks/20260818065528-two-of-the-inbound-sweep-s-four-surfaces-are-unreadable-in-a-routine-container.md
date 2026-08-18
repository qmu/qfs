---
type: Feedback
title: Two of the inbound sweep's four surfaces are unreadable in a routine container
kind: concern
source: discussion
subject: observer_ai:[Housekeep] routine
created_at: 2026-08-18T06:55:28+00:00
author: a@qmu.jp
supersedes: 
---

# Two of the inbound sweep's four surfaces are unreadable in a routine container

The hourly `[Housekeep]` routine's step 2 (`inbound-sweep`) hands back three
`probe_connector` entries — Slack, Gmail and Drive — for the session to read. In the
routine-fired container of tick `20260818-065210`, `ListConnectors` reports:

| connector | `connected` | `enabledInChat` |
| --- | --- | --- |
| Slack | true | **true** |
| Gmail | true | **false** |
| Google Drive | true | **false** |

Gmail and Drive are authenticated at the org level but their tools are not loaded in
this chat, so the tick cannot read either surface. Two of the four declared inbound
surfaces are therefore uncovered on every hourly tick.

The concern is not the missing coverage on its own — it is that nothing in the run
makes it visible. The step reports `ok` with a summary that reads "slack/gmail/drive
left for the agent to probe", and the seam has no place to record "the agent looked
and the connector was not there". A reader of the tick log cannot tell a sweep that
covered four surfaces from one that covered one. That is exactly the failure the
skill's own standing rule names: *a tick that reports "nothing to do" when it could
not look is a tick that lies about its own coverage.*

Two candidate resolutions, neither taken here:

1. Enable Gmail and Google Drive in the connector settings of the chat the routine
   fires in, so the declared sweep is the sweep that happens.
2. If routine sessions cannot carry those connectors, narrow `inbound-sweep`'s
   declared surfaces to what a routine can actually read, so the contract stops
   promising a read nobody performs.

Either way, `step-inbound-sweep.sh` should carry a per-surface outcome the agent can
write back — `read` / `agent_probe` / `unreadable:<reason>` — so a degraded surface
lands in the tick log by name instead of only in a run report the next tick cannot see.

Observed by the `[Housekeep]` routine, tick `20260818-065210`, session
https://claude.ai/code/session_011tPwsczYpbTsrsAqn8y8bP
