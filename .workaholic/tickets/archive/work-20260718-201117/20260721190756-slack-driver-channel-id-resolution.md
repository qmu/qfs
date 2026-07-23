---
created_at: 2026-07-21T19:07:56+09:00
author: a@qmu.jp
type: bugfix
layer: [Domain]
effort:
commit_hash:
category: Changed
depends_on:
mission: claude-code-sessions-are-queryable-and-steerable-as-qfs-paths
---

# slack driver: ID-requiring calls fail on name-addressed channels

## Overview

The slack driver accepts a channel **name** as the path segment (`/slack/<ws>/<channel>/messages`)
and resolves it for reads (`conversations.history`) and appends (`chat.postMessage`) — but the
ID-requiring Slack API calls receive the segment (or the CALL's `channel` argument) **unresolved**.
Observed with `chat.delete`; by signature the same applies to `react` / `pin` / `unpin` / `update`,
which all take a `channel` param the Slack API only accepts as a `C…` ID.

Reproduction (any workspace/channel the token can post to):

1. `insert into /slack/<ws>/<channel>/messages values ('x')` — commit ok (name accepted).
2. `/slack/<ws>/<channel>/messages |> select ts |> limit 1` — read ok (name resolved).
3. `remove /slack/<ws>/<channel>/messages where ts == '<ts>'` — PREVIEW is fine (REMOVE is a
   native verb, `child_address` keys on `ts`), but commit fails:
   `Terminal { reason: "Slack API chat.delete returned ok=false: channel_not_found" }`.
4. `/slack/<ws>/<channel>/messages |> call slack.delete('<channel>', '<ts>')` — same failure.

So the append log's own `REMOVE` verb (and every channel-taking procedure) is unusable on exactly
the address form that reads and inserts accept — a message the same token just posted cannot be
removed through qfs at all.

## Expected

Resolve name→ID once per channel (the resolution the read path already performs) and pass the ID to
**every** Slack API call — REMOVE lowering and all `slack.*` procedures alike — so the three
address answers (describe / enumerate / read) and the write verbs agree on what a channel segment
means. A name that resolves for `SELECT` must resolve for `REMOVE`.

## Policies

- One address, one meaning: a path segment that resolves for one verb must resolve for all verbs
  the node advertises — DESCRIBE lists `REMOVE` as native, so its commit path has to honor the
  same addressing.
- Fail at PREVIEW, not at commit: if a segment genuinely cannot be resolved for a verb, the
  resolver should reject in the plan phase with a usage error, never let PREVIEW pass and the
  edge call fail.

## Quality Gate

- A driver-level test covering: post by name → read `ts` by name → `remove … where ts == …` by
  name succeeds against the API (or its recorded fixture).
- The `channel` parameter of `react`/`pin`/`unpin`/`update`/`delete` goes through the same
  name→ID resolution, covered by at least one procedure test.
- PREVIEW/commit behavior unchanged for ID-addressed channels (`C…` segments keep working).
