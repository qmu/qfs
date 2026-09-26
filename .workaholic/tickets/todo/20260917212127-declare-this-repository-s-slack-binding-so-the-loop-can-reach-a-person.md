---
created_at: 2026-09-17T21:21:27+09:00
author: a@qmu.jp
assignees: [a@qmu.jp]
depends_on:
feedback: [20260917212108-fb-the-loop-s-slack-destination-does-not-exist-so-nothing-reaches-a-person.md]
merge_policy:
verification_handoff: Which Slack workspace and channel this loop posts to - creating or nominating a channel is visible to other people and is the operator's call
---

# Declare this repository's Slack binding so the loop can reach a person

## Overview

This repository declares no Slack binding, so `transport/scripts/observe-channel.sh` falls back
to the environment and resolves the destination to workspace `qmu` / channel `qfs` — the
repository's own name. No such channel exists on any reachable surface, so every outbound post
and every inbound read fails with `channel_unreadable`, permanently and at any backoff. There is
currently **no path from this loop to a person**: seven landed asks hold their finish line, three
thread reconciles are unreadable, and eleven `[Moderate]` questions are `ask: true` and
unsendable, re-presented every tick against an unconsumed ledger.

The fix is one fenced block in this repository's `CLAUDE.md`. What it cannot supply is the one
value the block needs — **which channel** — because creating a channel or nominating an existing
one is an act other people see. That fork is recorded under `## Correction, measured 2026-09-18

**`#dev-qfs` exists. It is a PRIVATE channel, and qfs can read it.** Everything below this line
that says otherwise was measured through a view that could not see it.

Slack's `conversations.list` defaults to `types=public_channel`, and the shipped declaration had
exactly one discovery view built on that default. Every probe recorded in this ticket and in its
feedback record — three mounts, zero matches, "a settled zero rather than an unknown" — was a
question asked of the public half of the namespace only. The sibling `private-channels` view
(`types=private_channel`) was added on 2026-09-18 and the answer changed on the first read:

| mount | account | `private-channels` | `dev-qfs` |
| --- | --- | --- | --- |
| `/slack` | team | 4 rows | **present** |
| `/slack-cc-for-qmu` | cc-for-qmu | 4 rows | **present** |
| `/slack-me` | me | `missing_scope` (no `groups:read`) | unknown, not absent |
| `/slack-codex-for-osbr` | codex-for-osbr | 1 row | absent |

`dev-qfs` is `C0BM2ASB63G`, and `/slack-cc-for-qmu/<ws>/C0BM2ASB63G/messages` returns its history
— including this loop's own past posts. So the destination was never gone; the instrument was
blind, and a `missing_scope` on one account was being reported as absence on all of them.

**What this does to the fork below.** It dissolves it. The operator does not have to create or
nominate a channel: the channel this loop already used is reachable today from two of the five
connected mounts. What remains is mechanical — declare `workspace` / `channel: dev-qfs` /
`channel_id: C0BM2ASB63G` / `mount: /slack-cc-for-qmu` in the binding block. Read the fork below as
history, not as a live question.

Note for whoever lands this: address the channel by **id**, not by name — a declared read view
substitutes the path segment into `conversations.history?channel=` verbatim and Slack's method takes
an id, which ticket `20260918044700` covers.

## Open Decisions` and this ticket
declares `verification_handoff`, so the unit opens a pull request that stays open for the
operator rather than being re-claimed and re-failed every tick.

## Policies

- `workaholic:implementation` / `policies/directory-structure.md` — conventional project layout
- `workaholic:implementation` / `policies/coding-standards.md` — style and structure conventions
- `workaholic:operation` — a running system must stay reachable; an outbound path that fails
  permanently is an outage, not a transient error

## Key Files

- `CLAUDE.md` (repository root) — the declaration's home. `read-declared-binding.sh` reads a
  fenced ```` ```workaholic-slack-binding ```` block from `CLAUDE.md` / `AGENTS.md` at the root,
  then the same two under each `--scope` directory (deeper wins), then
  `WORKAHOLIC_SLACK_BINDING_FILE`. Today the root file carries no such block.
- `<plugin>/skills/transport/scripts/read-declared-binding.sh` — the one reader. Required keys
  are `workspace` and `channel`; the full known set is `workspace`, `channel`, `channel_id`,
  `mount`, `account`, `sender_id`, `operations`, `fallback`. `complete: true` additionally wants
  the mount/account and `sender_id`, and a partial binding is usable rather than refused.
- `<plugin>/skills/transport/scripts/observe-channel.sh` — the consumer that falls back to the
  environment when nothing is declared, which is how `qmu` / `qfs` was reached.

## Implementation Steps

1. **Take the operator's answer to the `## Open Decisions` item** — the workspace and channel
   name. Nothing below can run without it, which is why this ticket carries
   `verification_handoff` rather than a Quality Gate item.
2. **Confirm the named destination is reachable from this loop** before declaring it: read the
   channel list on the mount that is supposed to carry it and match the name to exactly one id.
   A name that resolves to zero or to more than one is the same failure this ticket exists to
   end, so do not declare it.
3. **Add the fenced block to the repository root `CLAUDE.md`**, carrying at minimum the two
   required keys and, when known, the keys that make it `complete`:

   ```
   workspace: <the operator's answer>
   channel: <the operator's answer>
   mount: /slack/<workspace>
   sender_id: <the bot or user id that posts>
   operations: read_channel_delta, read_thread, post_root, post_reply
   fallback: connector
   ```

   Only the eight known keys above are accepted; an unknown key is reported as such.
4. **Prove the declaration is read**: `read-declared-binding.sh --root .` answers
   `declared: true` with no `missing`, no `conflicts`, no `invalid` and no `unknown_keys`.
5. **Prove one message actually lands**, through the transport seam rather than by hand, and
   then **drain what is held**: the seven `held:channel_unreadable` finish lines and the eleven
   unsent `[Moderate]` questions are the backlog this repair exists to release, and a repair
   that declares a binding without them reaching anyone has not been verified.

## Quality Gate

**Acceptance criteria** — the checkable conditions that must hold:

- The repository root `CLAUDE.md` carries one `workaholic-slack-binding` fenced block with
  `workspace` and `channel` set.
- `read-declared-binding.sh --root .` reports `declared: true`, `missing: []`, `conflicts: []`,
  `invalid: []`, `unknown_keys: []`.
- The declared channel resolves to exactly one channel id on the declared mount.
- One message posted through the transport seam is visible in the declared channel.

**Verification method** — the commands/tests/probes that prove them:

- `bash <plugin>/skills/transport/scripts/read-declared-binding.sh --root .` — inspect the JSON.
- Resolve the channel name to an id on the declared mount and assert exactly one match.
- Post one message through the transport seam and read it back from the channel.

**Gate** — what must pass before approval:

- The four probes above, on the destination the operator named.
- The change touches `CLAUDE.md` only, so no build or test gate is implicated; run the
  repository's gates unchanged if the branch carries anything else.

## Open Decisions

**Which Slack workspace and channel does this loop post to?**

*Sources consulted, and what they said.*

- `transport/scripts/read-declared-binding.sh --root .` (re-run 2026-09-17) →
  `{"declared": false, "reason": "no_declaration", "missing": ["workspace", "channel"]}`. The
  repository has never declared one, so the destination has only ever been the environment
  fallback.
- The reporting tick read `channels` and `private-channels` on all three connected mounts
  (`/slack` team, `/slack-me` me, `/slack-test0813` test0813) and matched **zero** rows for
  `qfs`. `/slack/qmu` carries nine channels, none of them `qfs`. The reads succeeded, so this is
  a settled zero rather than an unknown.
- *This proposal's own probe of the obvious repair.* This repository's records show the loop
  reading and posting to `#dev-qfs` through 2026-08-19 (housekeeping ticks quote `in:#dev-qfs`
  searches; two feedback records carry `subject: observer_ai:dev-qfs channel assistant`), which
  looked like an already-decided destination. It is not available:
  `slack_list_user_channels(name_prefix: "dev-qfs")` → not a member of any; `slack_search_channels`
  over public and private, archived included → no results; and the connector's full membership is
  a seven-channel set (`general`, `social`, `internal_with_yosan`, `coop-csnet-playground`,
  `coop-planner`, `dev-portal`, `pj-miko-lab`) disjoint from the `/slack/qmu` nine. At least two
  workspaces are in play and neither carries `qfs` or `dev-qfs`.
- The strategy set is empty (`strategy/scripts/list.sh` → `count: 0`) and no
  `subject: person:` feedback record names a destination for this loop, so the operator has not
  already ruled this. That check was run before writing this item, per the rule that a fork is
  only operator-only once the operator's own records have been read.

*The fork's sides.*

1. **Create a channel for this loop** in the `qmu` workspace (or whichever workspace the operator
   wants it in) and declare it here. Clean, but it adds a channel to somebody's workspace.
2. **Nominate an existing channel** as the destination and declare that. Nothing new is created,
   but it puts routine loop traffic into a channel that already has an audience and a purpose. No
   company-wide channel such as `general` was commandeered without being asked.

Whose ruling settles it: the operator's. Both sides are visible to other people, which is why
this loop did not pick one. Once the name is given, step 3 is immediate and is this loop's own
work.

## Considerations

- **Why a handoff rather than a Quality Gate item.** Nothing in the unattended loop can clear an
  operator-only fork, so a gate item would be re-claimed and re-failed every tick forever; the
  handoff route opens a pull request that stays open, quotes the reason in a non-droppable
  `## Handoff`, and leaves a standing claim the survey does not re-offer.
- **Why this is not record-only.** Record-only would be the ordinary outcome for an ask blocked
  on a decision — but the surface that would carry the notice is exactly the one that is down.
  A standing open pull request on GitHub is the only path to the operator that currently works.
- **The escalation ledger.** `[Moderate]` already produced this finding as
  `inbound-channel-unreadable:qfs` and cannot send it, so its ledger is unconsumed and the same
  key will be re-presented. Absorb it into this unit rather than filing it again.
- **Scope.** This ticket declares a binding. It does not change the fallback behaviour in
  `observe-channel.sh` — that a repository with no declaration silently resolves a destination
  derived from its own name is arguably worth its own ask, but it lives in the `workaholic`
  plugin repository and is out of reach from here.
