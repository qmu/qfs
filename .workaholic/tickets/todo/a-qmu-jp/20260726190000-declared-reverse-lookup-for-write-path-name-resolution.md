---
created_at: 2026-07-26T19:00:00+09:00
author: a@qmu.jp
type: enhancement
layer: [Domain]
effort:
commit_hash:
depends_on: [20260725124400-declared-follow-into-per-row-fan-out-g4.md]
mission: a-declared-write-resolves-a-name-the-way-a-query-does
---

# A declared write can resolve a name against a collection endpoint

## Overview

Blueprint §13.1 **G4** shipped on 2026-07-26 (`365d521`): a declared view body can fan a delivered
field out into a second wire request per row, `|> FOLLOW <field> INTO /http/<drv>/<template>`. That
closes the list→detail hydration the mail and drive twins need.

It does **not** close the Slack twin's channel-name→id resolution, and the reason is structural
rather than effort. This ticket carries the residue, minted from the G4 drive rather than left as a
note inside a blocked ticket. It adds **no** acceptance item to the mission — the agreed plan is
unchanged; this names the work the plan already implied.

## Measured — the two walls, demonstrated

**1. G4 substitutes a value INTO an address. Slack's name lookup is a reverse lookup against a
collection.** The compiled oracle (`crates/driver-slack/src/client.rs::resolve_channel_id`) GETs
`conversations.list` and scans the returned array locally for `name == wanted`. The Slack Web API has
no name-addressed channel endpoint (`conversations.info` takes an id), so there is no template a
`#name` can be substituted into. The oracle's own tests pin the two-request shape:

```
$ cargo test -p qfs-driver-slack resolves_channel_name -- --nocapture
test tests::rest_client_resolves_channel_name_for_update_procedure ... ok
test tests::rest_client_resolves_channel_name_before_deleting_a_message ... ok
```

The `Uxxxx` → `Dxxxx` DM open is the exception: `conversations.open?users={channel}` IS a pure G4
substitution, so the DM half of the resolution is already expressible and the `#name` half is not.

**2. The resolution the Slack CALLs need is on the WRITE path, and a map body has no stage slot.**
`eval_map_body`'s pipeline arm accepts exactly one `ENCODE` stage over `VALUES`; the grammar refuses
the stage earlier still:

```
$ qfs run "CREATE MAP CALL slack.delete ( channel text, ts text ) /slack/{ws}/{channel}/messages \
    AS INSERT INTO /http/slack/chat.delete |> FOLLOW channel INTO /http/slack/conversations.list \
    VALUES ({channel: row.channel, ts: row.ts})"
{"error":{"code":"parse_error", … "detail":"UNKNOWN_KEYWORD"}}
```

## Ruling (developer, 2026-08-01) — both questions settled

Settled from `design-brief-reverse-lookup.md` beside the mission, which answered the three spike
questions the mission held open. The spike's load-bearing finding, and the reason the earlier framing
was incomplete: **the map body is pure by contract and a name lookup is I/O.** `eval_map_body`
(`crates/exec/src/declared.rs:518`) lowers one `VALUES` expression to a per-row scalar and runs it
through `eval_value`, a pure evaluator with no wire access — the function's own doc states the
contract ("Purity holds: this constructs the wire effect; the caller's confined applier performs the
I/O at COMMIT"). So opening the parser alone does not make a lookup run; the gap is a runtime one.

**1. Shape — `LET` in the map body. No new stage, no `FOLLOW` selector.**

The reverse lookup is already expressible in the language: `let` binds a pipeline and a let-bound
name is a legal source. `create_map_stmt` parses its body with `inner_statement`
(`crates/parser/src/grammar.rs:2445`); `let_binding` (`:3513`) has exactly one call site,
`program_seq` (`:3576`) = `alt((let_binding, inner_statement))`. Changing `:2445` to `program_seq`
makes `LET` legal in a map body with no other parser change, and `body_to_json` (`:2048`) is a plain
serde serialization, so a `Statement::Let` round-trips into and out of the stored driver row
untouched. Verified by reading, 2026-08-01.

```qfs
let cid = /slack/{ws}/channels |> WHERE name == row.channel |> SELECT id
```

Chosen over the `LOOKUP` stage and the `FOLLOW … INTO` selector because it adds **no vocabulary**:
the author writes the same `|> WHERE … |> SELECT …` they would type at the shell. It also reuses the
driver's **own already-declared read view**, which settles the paging question in Considerations
below without restating it — the view's cursor paging and `OF slack/channel` contract apply as
written.

**2. Time — COMMIT, immediately before the effect leg, plus a preview-time SHAPE check.**

The lookup runs in the confined applier (`crates/qfs/src/apply_facets.rs:69`), which is already
`async`, already holds `cx`, and is already the confinement boundary. This is the same moment the
compiled oracle resolves, so equivalence is provable on the shared hermetic fixtures rather than
merely asserted.

PREVIEW additionally refuses a **malformed** reference — one that is neither a legal channel-name nor
a legal id shape — with no I/O. A well-formed but non-existent name is refused at commit, before the
effect leg fires. This is the strongest preview guarantee available without changing what PREVIEW
means product-wide.

**Not chosen: resolving at PREVIEW.** `exec/src/lib.rs:331` records that "PREVIEW structurally cannot
reach the executor"; preview renders `plan_preview(plan)` and nothing else. Making it resolve would
make preview perform a network read for every name-addressed write — a product-wide re-ruling of what
PREVIEW means, and one that must land on the compiled side simultaneously or the twin is not a twin.
That is its own mission, not a step in this one.

**3. Consequence — QG2 of the equivalence ticket is corrected, not the implementation.**

`20260724014100-slack-call-maps-effect-equivalent.md` states the contract to reproduce as
"unresolvable names fail at PREVIEW time". The compiled driver does not do that: `driver-slack/src/
path.rs:45` says a symbolic `#name` needs "a `conversations.list` lookup **at commit**", `path.rs:66`
says the `@name`→id resolution "is I/O performed by the applier **at commit**", and what PREVIEW
prints is `ChannelRef::symbolic()` (`path.rs:54`) — the **unresolved** `#general`. Held as written,
the equivalence tests could not both pass and prove equivalence. That ticket's QG2 is reworded to
"before the effect leg fires" as part of this ruling.

## Policies

- workaholic:design / 「推測するな、宣言して拒否せよ」 — an unresolvable name is refused as a
  structured, secret-free usage error, never guessed and never sent to the wire as a garbage id.
- Blueprint §13 confinement — a lookup target is a `/http/<self>/…` address like every other declared
  wire address, re-checked per row, exactly as the G4 fan-out already does.
- workaholic:implementation / honest-surfaces — the lookup is bounded and visible (the G4 stage's
  `FOLLOW_FANOUT_MAX = 50`, the cursor pagination's budget), never a silent request storm; a
  collection that pages must either page honestly or refuse, not silently miss a match on page two.
- experimental-no-backward-compat — if `FOLLOW … INTO` is the stage that grows, change it outright;
  no second spelling kept alive for compatibility.

## Quality Gate

1. Blueprint §13.1 records the ruling above — `LET` in a map body as the reverse-lookup spelling,
   commit-time resolution in the confined applier, and the preview-time shape check — before the
   implementation lands, so it outlives this ticket.
2. A map body parses a `LET` binding: `create_map_stmt` uses `program_seq`, and a parse test pins a
   stored body carrying a `Statement::Let` round-tripping through `body_to_json` unchanged.
3. The `LET` source is CONFINED to the driver's own declared surface, re-checked the way a G4 fan-out
   target is; a body naming a foreign source is a structured refusal at declaration time, and a
   negative test pins it. The host-confinement guarantee at the head of every declaration file must
   survive this ticket.
4. A declared write body resolves a name against the driver's declared collection view to exactly one
   row, proven on a hermetic fixture with BOTH recorded wire requests asserted in order (the lookup,
   then the effect leg carrying the resolved id).
5. The binding evaluates **per row** — a fixture with two incoming rows naming two different channels
   produces two correctly-resolved effect legs. The collection is fetched once per statement and
   matched locally per row (the compiled oracle's own shape); the test asserts one lookup request,
   not two.
6. An unresolvable name is a structured, secret-free refusal **before the effect leg fires**, and the
   effect leg is never issued — asserted by the absence of the second recorded request. A malformed
   reference (neither a legal name nor a legal id shape) is refused at PREVIEW with **no** recorded
   request at all.
7. A name that matches more than one row in the collection is a refusal, not a pick.
8. `eval_map_body` stays pure: the resolved values are bound as additional columns before the per-row
   scalar evaluator runs, and the purity contract in its doc comment (`declared.rs:506`) is still
   true when the ticket lands. If it cannot stay true, say so in the story rather than deleting the
   comment.
9. `20260724014100-slack-call-maps-effect-equivalent.md`'s QG2 becomes provable against the compiled
   oracle on the shared fixtures, for BOTH the `#name` → `Cxxxx` and `Uxxxx` → `Dxxxx` arms.
10. Workspace gates green: `cargo test --workspace`,
    `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all --check`,
    `cargo run -p xtask -- gen-docs --check`, `cargo run -p xtask -- gen-skills --check`.

## Considerations

- `crates/exec/src/declared.rs` owns both `eval_view_body` (which now has the working per-row
  fan-out to copy the confinement + bound discipline from) and `eval_map_body` (which is pure today
  — a resolution stage means injecting a wire closure into it, exactly as the read side does).
- `crates/qfs/src/apply_facets.rs:69` is the seam where a map body meets the wire, and the ruling puts
  the resolution there. It is already `async` and already holds `cx`, so no new hook is needed — the
  work is a pre-pass that evaluates the body's `LET` bindings through the confined read path and binds
  the results before `eval_map_body` runs.
- The compiled `resolve_channel_id` pages up to `MAX_PAGES` before declaring a miss. The ruling avoids
  re-deciding this: the `LET` source is the driver's **own declared read view**, whose cursor paging is
  already declared and already tested, so the declared lookup pages exactly as that view does. Prove
  it rather than assume it — a fixture whose match sits on page two must resolve, not report "not
  found". A first-page-only lookup would be a quiet wrong answer of exactly the class the sibling
  predicate-honesty mission removed.
- The `Uxxxx` → `Dxxxx` DM arm is a pure G4 substitution (`conversations.open?users={channel}`) and is
  already expressible. Do not route it through the new `LET` path for symmetry's sake; QG9 asks that
  both arms be provable, not that both use one mechanism.
