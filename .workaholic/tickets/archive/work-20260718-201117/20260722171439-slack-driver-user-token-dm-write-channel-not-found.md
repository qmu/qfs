---
created_at: 2026-07-22T17:14:39+09:00
author: a@qmu.jp
type: bugfix
layer: [Domain]
effort:
commit_hash:
category: Changed
depends_on: 20260721190756-slack-driver-channel-id-resolution.md
mission: claude-code-sessions-are-queryable-and-steerable-as-qfs-paths
---

# slack driver: user-token DM write fails `channel_not_found` while read succeeds

## Overview

With a Slack **user token** mounted, posting a DM addressed by the recipient's **user id**
fails at commit even though reading the same DM path succeeds:

```
/slack-me/<ws>/<RECIPIENT_USER_ID>/messages |> select ts, user, text |> limit 3   # works
insert into /slack-me/<ws>/<RECIPIENT_USER_ID>/messages values ('hi')   # --commit -> channel_not_found
```

`chat.postMessage` does **not** accept a bare user id as `channel` for a user token — a user
token must post to the **opened IM channel id** (`Dxxxx`). The read path already resolves
`user-id → IM channel` (`conversations.history` needs a channel id and the read returns rows),
but the write path passes the user id straight through. That read/write asymmetry is the bug.
The error is `channel_not_found`, **not** `missing_scope`, and persists after a token `rotate`,
so this is addressing, not a scope gap. This is the write-side companion to
[[20260721190756-slack-driver-channel-id-resolution]] (ID-requiring calls on name-addressed
channels): both are the write path receiving an unresolved segment.

## Suggested fix

On a DM write addressed by a user id (segment `Uxxxx` / handle) with a user token, mirror the
read path: call `conversations.open(users=<user_id>)` to obtain the `Dxxxx` and post to that.
Accept an already-opened `Dxxxx` channel segment on the write target unchanged.

## Policies

- One address, one meaning: a path segment that resolves for reads must resolve identically for
  the write verb the node advertises — a DM path that lists rows must accept an append to the
  same path.
- Fail at PREVIEW, not at commit: if a DM target genuinely cannot be resolved (e.g. an IM cannot
  be opened), reject in the plan phase with a usage error, never let PREVIEW pass and the edge
  `chat.postMessage` fail with `channel_not_found`.
- Live writes reach a real recipient: the fix is proven hermetically. No third-party DM is sent
  during verification (see Quality Gate) — the overnight run must not message anyone.

## Quality Gate

- A driver-level test, against a **recorded fixture** (no live Slack, no third-party DM send),
  covering: a user-token DM write addressed by user id resolves `conversations.open(users=…) →
  Dxxxx` and posts to the `Dxxxx`, so the append succeeds where the bare user id returned
  `channel_not_found`.
- An already-`Dxxxx`-addressed write target keeps working unchanged (no double-open).
- PREVIEW rejects an unresolvable DM target with a usage error rather than passing through to a
  commit-time `channel_not_found`.
- Bot-token DM writes (which auto-open the IM) remain unchanged.
