---
created_at: 2026-07-28T08:52:53+09:00
author: a@qmu.jp
assignees: [a@qmu.jp]
type: enhancement
layer: [Domain]
effort: 4h
commit_hash:
category: Changed
depends_on:
mission:
claim: work-20260729-145625
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

## Final Report

Scope items 1–4 are implemented and gated. Item 5 (incremental reads) is answered as far as the
service truthfully allows — see the insight below.

`DESCRIBE` now answers a declared node from the declaration itself rather than falling back to the
generic REST answer. Three things carry it:

- **`SchemaContract`** (new, on `NodeDesc`/`DescribeReport`) states *where* a node's columns come
  from — `Declared { of_type }`, `Compiled`, or `Undeclared`. A declared view with an `OF` reports
  that type's columns; one without reports **zero columns and `Undeclared`**, so "nobody stated a
  row type" is a declared answer instead of a synthetic `value: Json` a caller reads as the schema.
- **`ChildNode`** (new) + `Driver::children` (default-empty) makes a mount walkable: `/chatwork`
  names `rooms`, `/chatwork/rooms` names `{room}` *as a parameter*, and so on to a leaf. `MountDriver`
  maps child paths back **out** through the remap so the caller gets addressable paths.
- **`RestDriver::with_declared_nodes`** is the lift — the same shape `with_procs` already had for
  `CREATE MAP CALL`. `declared_node_descs` builds it from the driver's own view/map rows, resolving
  each `OF` against the same declared-type registry `declared_eval::view_specs` reads.

Verified live against the developer's real installed `chatwork` declaration (not only fixtures):
`describe /chatwork/rooms` returned `room_id/name/type/role` with
`schema_contract: {"kind":"declared","of_type":"/type/chatwork/room"}`, `child_address` keyed on
`room_id`, and `children: [{segment:"{room}", parameter:"room"}]` — the ticket's exact repro,
answered.

### Discovered Insights

- **Insight**: A declared node's capabilities were keyed by the *resource segment*
  (`resources()` groups every declaration under the first segment after the driver name), so
  `/chatwork/rooms` advertised the `INSERT` that actually belongs to
  `/chatwork/rooms/{room}/messages`. Capabilities are now per declared NODE when a mount declares
  any; a mount that declares none keeps the segment gate unchanged.
  **Context**: `describe`'s `native_verbs` is derived from capabilities, so this over-claim reached
  the agent-facing hint — the exact "never advertise a capability by over-claim" failure the
  describe contract's own docs forbid. It was invisible before only because the root and the
  leaves shared one answer.

- **Insight**: Chatwork nests the sender under an `account` object, so `account_id`/`account_name`
  are not top-level fields an `OF` contract can name. The view lifts them with `Expr::Path`
  struct navigation inside the body (`|> SELECT … account.account_id AS account_id`) — the `OF`
  projection runs *after* the body, so the body is where a nested field becomes a column.
  **Context**: This is the general recipe for any declared driver over an API with nested response
  objects; without it the `OF` contract can only ever name what the endpoint puts at the top level.

- **Insight**: Item 5 (incremental reads) cannot be pushed down truthfully — Chatwork's
  `GET /rooms/{room}/messages` accepts only `force`, no since-parameter. Declaring a `PUSHDOWN`
  map for `send_time >` would have produced a *silently wrong* wire call. What the change does
  deliver is `send_time` as a real column, so `|> where send_time > …` works as a local residual,
  and the cookbook says plainly that it filters locally and does not save the round trip.
  **Context**: The §13.1 G2 `PUSHDOWN (…)` clause is only honest when the endpoint has the
  parameter; "honest-but-chatty" is the correct answer when it does not.

- **Insight**: The declared-type registry keys types by their **`/type/`-prefixed** path
  (`/type/chatwork/message`), which is what both `CREATE VIEW … OF` stores and `types_from_conn`
  reads. A lookup written against the bare `chatwork/message` spelling silently resolves nothing
  and degrades to `Undeclared` rather than failing.
  **Context**: `declared_from_script` in the tests rebuilds the model through the real desugar for
  exactly this reason — a hand-built fixture would have hidden the prefix.
