---
created_at: 2026-07-28T08:52:53+09:00
status: done
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

## Final Report

Development completed as planned for Scope 1–5. The "Related: a declared view cannot be removed"
observation is **outside this ticket's Scope and Quality Gate** and is a grammar + catalog change of
its own, so it was minted as ticket `20260817030500-a-declared-view-cannot-be-removed.md` and left
in the queue rather than folded in here.

What shipped:

1. **Declared nodes report their `OF` columns.** A new `DeclaredSurface` (`declared_surface.rs`)
   indexes each declared driver's view templates and resolves every `OF <type>` against the
   declared-type registry; `DeclaredDescribeDriver` answers `describe` from that index and delegates
   everything else (capabilities, procedures, pushdown, irreversibility, applier) to the inner stock
   `RestDriver` untouched. A view with no `OF` reports the shape its body statically decides
   (`qfs_exec::declared::body_delivered_columns`: a bytes `FOLLOW` delivers `content`, a terminal
   `SELECT` delivers its named projections) and otherwise an EMPTY column list plus a null
   `row_contract` — the honest "nothing declared here", never the old synthetic `value: Json`.
2. **Mount roots and intermediate segments enumerate their children.** `NodeDesc` gained
   `children: Vec<ChildNode>` (driver-stated segments) and `row_contract`; `DescribeReport` composes
   each child into a full `ChildLink` address under the node's own path. A `{param}` segment is
   legible as one (`param: "room"`), and `node` separates an addressable view from a pure interior.
   The driver states only the segment, so a remapped mount can never leak its inner `/rest/<name>`
   namespace outward.
3. **The shipped Chatwork declaration carries the fields the endpoints deliver.** `chatwork/room`
   keeps `sticky`, `icon_path`, `last_update_time`, and all six counts; `chatwork/message` keeps
   `update_time` plus the sender, lifted out of Chatwork's nested `account` object by an `EXTEND`
   stage in both message views. The cookbook's worked examples use them (selecting conversations by
   unread count, attributing a message to its sender).
4. **Reply support** is the documented recipe the ticket allows as the alternative to a `reply_to`
   map field — now writable because item 3 exposes `account_id` and `message_id` as columns.
5. **Incremental reads** are documented as what they honestly are: Chatwork's messages endpoint
   takes no since-parameter, so `send_time >` is applied as a truthful LOCAL residual over the one
   GET rather than a pushdown the API cannot perform. Pinned by a test asserting the request count
   and that the facet does not claim `honors_pushed_filter`.

Verification: `cargo test --workspace` green; `cargo fmt --all --check`;
`cargo clippy --workspace --all-targets -- -D warnings`; `gen-docs --check` and `gen-skills --check`
in sync. Gate 1 is pinned by `describe_of_a_declared_node_reports_the_columns_a_read_delivers_*`
over BOTH shipped declarations (`chatwork.qfs` and `slack_driver.qfs`), comparing the describe fold
against `declared_eval::view_specs`' `of_columns` — the exact list `shape_to_type` projects a read
to, so the two answers cannot drift apart again. Gate 2 is pinned by
`a_declared_surface_is_walkable_from_the_mount_root_using_only_describe`, which walks
`/chatwork` → `/chatwork/rooms` → `/chatwork/rooms/{room}` → `…/messages` following only child links
through a real `MountRegistry` resolution.

### Discovered Insights

- **Insight**: The declared read path shapes rows with `shape_to_type`, which projects to the `OF`
  type's column NAMES and takes each column's TYPE from the delivered batch. So describe and run can
  agree exactly on the column set while still differing on types (describe reports the declared
  `timestamp`; a read of a Chatwork epoch field reports `Int`).
  **Context**: Any future "describe matches run" ratchet must compare names, not typed columns —
  and closing the type gap means teaching the shaper to coerce, which is a separate decision.
- **Insight**: A shipped `.qfs` asset can be lifted into the live model *through the real desugar*
  by parsing each statement and reading the `/sys/drivers` row off the serialized
  `INSERT INTO /sys/drivers` effect (`declared_driver::shipped_declared`). The binary deliberately
  has no `qfs-parser` edge, so serde over `qfs_exec::parse`'s output is the only way in — and it is
  a faithful one, because that JSON *is* what an install writes.
  **Context**: The existing shipped-asset tests hand-rebuilt their fixtures (`shipped_slack_views()`
  and friends), which can silently drift from the asset. New tests over a shipped declaration should
  use `shipped_declared` instead.
- **Insight**: `RestDriver::caps_for` resolves capabilities by LEADING resource segment only, so
  `/chatwork/rooms` advertises the `INSERT` that is actually declared at
  `/chatwork/rooms/{room}/messages`. That is a known, separately-claimed defect
  (`20260817001110-a-declared-mounts-verb-gate-is-leading-segment-coarse`).
  **Context**: This ticket deliberately left `capabilities` alone and changed only the describe
  answer, so the two changes do not collide in the same file.
- **Insight**: `crates/qfs`'s `provision::tests::offline_run_engine_does_not_mount_server` is flaky
  under test parallelism — it intermittently panics in `store::forbid_shared_home_fallback_in_tests`
  because the `HomeGuard` env isolation is process-global. Observed once in three runs on unmodified
  code paths.
  **Context**: Not caused by this change; worth its own ticket if it recurs in CI.
