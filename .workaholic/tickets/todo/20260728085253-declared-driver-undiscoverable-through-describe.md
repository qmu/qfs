---
created_at: 2026-07-28T08:52:53+09:00
author: a@qmu.jp
assignees: [a@qmu.jp]
type: enhancement
layer: [Domain]
effort:
commit_hash:
category: Changed
depends_on:
mission:
claim: work-20260817-023958
---

# A declared driver is undiscoverable through `describe`, so the documented agent loop cannot be run against one

## Overview

`SKILL.md` opens the agent loop with `describe` — "learn the node's archetype, columns,
supported verbs … **Always read this first**" — and tells the agent to build its statement from
the columns it returns. Against a declared (§13, query-based) driver that first step returns
nothing usable, so the loop cannot be entered as written.

Observed 2026-07-27 on the shipped `chatwork.qfs` declaration, mounted and connected:

```
$ qfs describe /chatwork --json
{"path":"/chatwork","archetype":"relational_table",
 "columns":[{"name":"value","ty":"Json","nullable":true, …}],
 "verbs":{"select":false,"insert":false, …},
 "procedures":[],"aliases":[],"child_address":{"kind":"none"}}

$ qfs describe /chatwork/rooms --json
{"path":"/chatwork/rooms", …
 "columns":[{"name":"value","ty":"Json","nullable":true, …}], …}

$ qfs run "/chatwork/rooms |> limit 1" --json
{"schema":[{"name":"room_id","type":"text"},{"name":"name","type":"text"},
           {"name":"type","type":"text"},{"name":"role","type":"text"}], "rows":[…]}
```

Two separate failures in that:

- **The mount root advertises no children.** `/chatwork` reports `child_address: none` and every
  verb `false`, so nothing points at `/chatwork/rooms`, `…/messages`, `…/files`, or
  `…/files/{file}/blob`. There is no path from "the driver is mounted" to "here is its surface"
  short of reading the `.qfs` source or grepping `qfs dump` — which is what I ended up doing.
- **A declared node reports one `value: Json` column** even though the declaration carries a
  `CREATE TYPE … OF` contract naming the real columns, and `run` on the same path returns
  exactly those typed columns. `describe` and `run` disagree about the same node; the compiled
  drivers do not behave this way, so a declared driver is strictly worse to work with despite
  the contract being present in config.

The DSL already knows both answers. `CREATE VIEW /a/{x}/b OF t` states the parent/child shape and
the row type; `describe` just isn't reading them.

## Scope

1. `describe` on a declared node resolves its `OF` type and reports those columns (name + type +
   nullability), matching what `run` returns for the same path. If a view has no `OF`, say so
   explicitly rather than reporting a synthetic `value: Json`.
2. `describe` on a mount root (and on any intermediate segment) enumerates the declared paths
   beneath it, including the `{param}` segments, so the surface is walkable from the mount down.
   A parameterised child should be legible as such — the caller needs to know a room id goes
   there.
3. Carry through the fields the upstream endpoint already delivers, instead of narrowing to four
   columns. For the shipped Chatwork declaration specifically, the room and message types drop
   what a caller most needs:
   - rooms: unread/mention counts, message count, last-update time — without them "which
     conversations have something new" means listing every room and eyeballing names, which is
     what this took;
   - messages: the sender's account id and name, and update time — without them you cannot tell
     who wrote a row in any room with more than two people.
   (Taken from the endpoints' documented response bodies; I could not re-read them live because
   the declared views project the extras away — which is the point of this item.)
4. Reply support, once item 3 exposes the sender. A reply on that service is a body prefixed
   with `[rp aid=<account_id> to=<room>-<message_id>]` — the ids appear inside the bodies qfs
   already returns, so the data is there but not reachable as columns. Either document the
   recipe in `docs/cookbook/chatwork.md` or give the map a `reply_to` field that composes the
   prefix. Today every message qfs sends is an unthreaded post, which is a visible downgrade
   from how a human uses the service.
5. Incremental reads. `pushdown.where_` is `false` on these nodes, so checking a conversation
   pulls the last 100 messages whole and there is no "since I last looked". A `send_time >`
   filter (pushed down where the API supports it, applied locally otherwise) makes a routine
   poll cheap.

## Related: a declared view cannot be removed

`CREATE VIEW` has no inverse. `DROP VIEW /chatwork/rooms_raw` fails to parse
(`RESERVED_AS_IDENTIFIER`), and the row it wrote cannot be deleted either —
`remove /sys/drivers where name == '…'` returns
`UnsupportedVerb { path: "/sys/drivers", verb: "REMOVE", supported: ["SELECT","INSERT"] }`.
So an experimental or mistaken declaration is permanent in local config. Two scratch views I
created while working around the items above are still there with no supported way to clean
them up. Worth an inverse verb, or at least a documented one.

## Quality Gate

- `qfs describe` on a declared node returns the same column set `run` returns for that node,
  pinned by a test over the shipped `chatwork.qfs` (and one other declaration) so the two paths
  cannot drift again.
- `qfs describe` on a declared mount root lists its declared children; walking from the root to a
  leaf using only `describe` output is possible and is exercised by a test.
- The Chatwork declaration's room and message types carry the added fields, and the cookbook's
  worked examples use them (e.g. selecting conversations by unread count, attributing a message
  to its sender).
- `cargo test --workspace` green.

## Notes

Filed from another repository via `/request`; sibling to the form-codec ticket filed
2026-07-27, which came out of the same session. Both are the same shape of gap — a declared
driver is presented as a first-class way to add a service, but reading its surface and writing
through it both fall short of what the compiled drivers do.
