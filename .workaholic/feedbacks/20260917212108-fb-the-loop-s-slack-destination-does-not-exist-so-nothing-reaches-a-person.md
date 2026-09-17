---
type: Feedback
title: [FB] The loop's Slack destination does not exist, so nothing reaches a person
kind: instruction
source: development
subject: person:tamurayoshiya
created_at: 2026-09-17T21:21:08+09:00
author: a@qmu.jp
supersedes: 
---

# [FB] The loop's Slack destination does not exist, so nothing reaches a person

kind: instruction / source: development / subject: person:tamurayoshiya

# The loop's Slack destination does not exist, so nothing reaches a person

This repository **declares no Slack binding**
(`transport/scripts/read-declared-binding.sh --root .` →
`{"declared": false, "reason": "no_declaration", "missing": ["workspace", "channel"]}`,
re-run and confirmed 2026-09-17). With no declaration, `observe-channel.sh` falls back to the
environment and resolves the destination to workspace `qmu` / channel `qfs` — the repository's
own name.

**That destination does not exist.** The reporting tick read `channels` and `private-channels`
on all three connected Slack mounts and matched zero rows for `qfs`, on the `team` account and
on the owner's own `me` account alike:

| Mount | Account | channels | private-channels |
| --- | --- | --- | --- |
| `/slack` | team | 0 matches | 0 matches |
| `/slack-me` | me | 0 matches | 0 matches |
| `/slack-test0813` | test0813 | 0 matches | 0 matches |

`/slack/qmu` carries nine channels and none is `qfs`: `gcp-monitoring`, `general`, `gh_en9jin`,
`internal-data-bridge-notification`, `jenkins`, `random`, `rmc-crawler-notifications`,
`takenori_kasamatsu-personal-claude`, `テスト用---クローラー通知`. The reads themselves
succeeded, so this is a settled zero rather than an unknown. `describe-native-qfs.sh` cannot
resolve the channel name to a single id and returns `channel_unreadable` on every mount.

## Superseded in part, 2026-09-18

The destination **exists**: `#dev-qfs` is a private channel (`C0BM2ASB63G`), readable today through
`/slack` and `/slack-cc-for-qmu`. Every zero recorded below was measured through a channel listing
that only ever asked for public channels (Slack's `conversations.list` default), so it could not
have found it. The `private-channels` sibling view added on 2026-09-18 finds it on the first read.
The impact recorded below was real; its stated cause was not.

## What this run measured on top of the report

The obvious repair — point the binding at the channel this repository actually used — was
tested and **is not available**. This repository's own records show the loop reading and posting
to `#dev-qfs` through 2026-08-19 (housekeeping ticks quote `in:#dev-qfs` searches; two feedback
records carry `subject: observer_ai:dev-qfs channel assistant`). Probed 2026-09-17 from this
session's own Slack connector:

- `slack_list_user_channels(name_prefix: "dev-qfs")` → not a member of any such channel.
- `slack_search_channels("dev-qfs", public + private, include_archived: true)` → no results.
- The connector's full membership is seven channels — `general`, `social`,
  `internal_with_yosan`, `coop-csnet-playground`, `coop-planner`, `dev-portal`, `pj-miko-lab` —
  which is a different set again from the `/slack/qmu` nine, so at least two workspaces are in
  play and neither carries `qfs` or `dev-qfs`.

So the historical destination is gone from every surface this loop can reach, and the fork below
is genuine rather than an unchecked assumption.

## Impact, as reported

Retrying never succeeds, at any backoff. Measured consequences:

- The `[Work]` coordinator's Slack observation is `channel_unreadable` every tick. Unread is not
  quiet, so the cursor does not advance and no quiet streak accumulates.
- Seven landed asks still carry their finish line as `held:channel_unreadable`.
- `[Moderate]`'s `unanswered-asks` is `channel_unreadable`; three `thread-reconcile` subjects
  (#121, #116, #120) are all `thread_unreadable`.
- Eleven `[Moderate]` questions are `ask: true` and unsendable (`direction-none:`,
  `handoff-unit:…`, `stranded-unit:…` ×7, `mission-leftovers:…`,
  `inbound-channel-unreadable:qfs`), so the ledger never consumes them and they are re-presented
  every tick.

**There is currently no path from this loop to a person.**

## The decision this needs, and why the loop did not take it

Recovery is provisioning, not code. Two options, both visible to other people, so neither was
taken unilaterally:

1. Create a channel for this loop in the `qmu` workspace and declare the binding here.
2. Nominate an existing channel as the destination and declare it the same way. (No company-wide
   channel such as `general` was commandeered without being asked.)

Once the destination is named, adding the declaration is immediate and is this loop's own work.

## On duplication

`[Moderate]` already produced the same finding as the escalation question
`inbound-channel-unreadable:qfs`, but it cannot be sent, so its ledger is unconsumed. This record
is the permanent form; if a later `[Moderate]` tick re-presents that key, absorb it into this.

Source: https://github.com/qmu/qfs/issues/124
