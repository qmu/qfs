---
created_at: 2026-07-25T12:44:00+09:00
author: a@qmu.jp
type: enhancement
layer: [Domain]
effort: 2h
commit_hash: 365d521
category: Added
depends_on:
mission: the-declared-slack-twin-retires-the-compiled-driver
---

# Declared per-row fan-out: `|> FOLLOW <field> INTO /http/<drv>/<template>` (blueprint §13.1 G4)

## Overview

Blueprint §13.1 **G4** — a declared body may fan a delivered field out into a SECOND wire request per
row — is **ruled but unimplemented**. Today `FOLLOW <field>` exists only in its single-row download
form (`qfs_exec::declared::eval_view_body`'s follow stage takes exactly ONE delivered row and fetches
its URL as raw bytes). There is no form that takes a delivered *value* and substitutes it into a
declared endpoint template, per row.

That gap is the exact wall the declared Slack twin hit
(ticket `20260724014100-slack-call-maps-effect-equivalent.md`, Quality Gate item 2 — still open):
the compiled `driver-slack` resolves a `#name` channel to its `Cxxxx` id (and a `Uxxxx` to its
`Dxxxx` IM channel) INSIDE its live client, on the way to every ID-requiring call. A declaration
cannot express that lookup, so a declared CALL can only accept an already-resolved id — neither
"a name-addressed channel resolves before the effect fires" nor "an unresolvable name is a structured
preview-time error" is reproducible. Playbook §13.3's remaining twins (github / drive / mail) need
the same primitive: the Drive id-lookup work ruled over from the predicate-honesty mission is the
same shape.

This ticket is the shared prerequisite, minted so the wall is queued work rather than a note inside a
blocked ticket. It does NOT add an acceptance item to the mission; the agreed plan is unchanged.

## Policies

- workaholic:design / 「推測するな、宣言して拒否せよ」 — an unresolvable reference must be refused at
  PREVIEW, as a structured usage error, never guessed and never a garbage id at commit.
- Blueprint §13 confinement — a fan-out target is a `/http/<self>/…` template like every other
  declared wire address; the anti-exfiltration boundary must hold per row, not just for the first
  fetch.
- workaholic:implementation / honest-surfaces — the runaway-fetch guard the cursor pagination already
  has (`MAX 50`) applies here too: a per-row fan-out over N rows is N requests and must be bounded
  and visible, never a silent storm.

## Quality Gate

1. A declared view body carrying `|> FOLLOW <field> INTO /http/<drv>/<template>` issues ONE wire
   request per delivered row, substituting that row's `<field>` into `<template>`, and splices the
   response into the row — proven on a hermetic multi-row fixture with the recorded requests
   asserted.
2. The fan-out target is confined to the driver's own `/http/<name>` namespace, re-checked per row;
   a foreign host is a structured refusal, and a bounded ceiling caps the request count.
3. A value the fan-out cannot resolve is a structured, secret-free error at PREVIEW time — not a
   silent `Null`, not a wire error at commit.
4. The declared Slack twin's channel-name→id resolution is expressed with it, and ticket
   20260724014100's Quality Gate item 2 becomes provable: a name-addressed channel resolves before
   the effect fires, and an unresolvable name is a preview-time refusal — both compared against the
   COMPILED driver on the shared fixtures.
5. Workspace gates green: `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`,
   `cargo fmt --all --check`.

## Considerations

- `crates/exec/src/declared.rs` owns the existing single-row `FOLLOW` (`follow_url`) and is where the
  per-row form belongs; the fetch stays an injected closure so exec keeps no transport.
- The grammar side is a new `INTO <path>` tail on the existing `FOLLOW` stage — check whether the
  single-row form should become the `INTO`-less special case or be retired outright (the project
  takes hard breaks; no compatibility shim).
- Sequencing: this is the blocker for 20260724014100 QG2 and therefore for
  `20260724014200-retire-the-compiled-slack-driver.md`, which cannot delete the compiled oracle while
  an equivalence proof still needs it.

## Progress (partial — the ruled stage shipped; QG4 stays open)

An overnight `/monitor` drive (run 20260726-184527) implemented the stage exactly as blueprint §13.1
G4 spells it. **Committed and green at `365d521`; QG1, QG2, QG3 and QG5 are met.** What landed:

- **Grammar.** `FOLLOW <field> [INTO <path>]`. `into` is a contextual identifier like `follow`
  itself, so the frozen keyword set stays 39. The tail COMMITS (`cut_err`): `FOLLOW id INTO` with no
  path is a hard parse error, never an `alt` fallthrough that would silently degrade the fan-out
  into the bytes form. `FollowRef` gains `into: Option<Vec<PathSegment>>`, serde-defaulted and
  skipped when absent so an already-installed declaration rehydrates byte-identically.
- **The redefinition the blueprint asked for, not a second stage.** The shipped bytes download is
  now literally the no-template arm of the same `PipeOp::Follow`; no shipped declaration changed
  behavior, and `follow_stage_parses_as_the_follow_pipe_op` asserts `into == None` so an old body
  can never enter the fan-out arm by accident.
- **Per-row fan-out** (`qfs_exec::declared::fan_out_rows`). For each delivered row the template's
  `{name}` placeholders are substituted and ONE detail request is issued through the driver's own
  confined wire closure — which is why `eval_view_body`'s `fetch` parameter moved from `FnOnce` to
  `Fn`. The detail requests ride the driver's auth and host pin; the credential-free `follow`
  closure stays exclusive to the self-authorizing bytes URL.
- **Confined per row, not per template** — `confined_wire_resource` is re-checked for every request,
  so a row VALUE can never steer the fan-out off `/http/<self>`. Proven by a test in which the
  foreign-host template is refused with zero requests recorded.
- **Bounded and visible** — `FOLLOW_FANOUT_MAX = 50`, deliberately the same number the declared
  cursor pagination already spends (`PAGINATE CURSOR (… MAX 50)`): one chattiness budget, one
  number. Over the cap is a REFUSAL naming the cap, not a truncation — hydrating the first 50 of 500
  rows would be precisely the quiet wrong answer the guard exists to prevent.
- **Refuse, never guess** — a row whose followed field is absent, `Null`, or empty is refused at the
  read (which is preview time), never spliced as `Null` and never sent as a garbage id; a detail
  request answering with no row (or several) is the same refusal.

**Two precedence rules had to be DECIDED, and both are pinned by a test that fails if they flip**
(they are documented on `fan_out_rows` at the seam):

1. **Substitution scope.** `{name}` resolves against the DELIVERED ROW's columns first, and the
   view's bound `{param}` segments only as the outer fallback. Row-first is the honest default for a
   stage whose whole meaning is "per row".
2. **Splice precedence.** The detail's columns are spliced onto the row and, on a name collision,
   the DETAIL replaces the row's value — the row carried a stub, the detail is the hydrated truth,
   which is the entire point of a list→detail fan-out.

### QG4 is NOT met, and the reason is structural — record it before re-driving

QG4 asks that the Slack twin's channel-name→id resolution be *expressed with* this stage. It cannot
be, and the gap is not effort. Two independent walls, both demonstrated rather than reasoned:

**Wall 1 — the shape. G4 substitutes a value INTO an address; Slack's name lookup is a
reverse lookup against a collection.** The compiled oracle's own resolver
(`crates/driver-slack/src/client.rs::resolve_channel_id`) GETs `conversations.list` and then scans
the returned array locally for `name == wanted`, paging up to `MAX_PAGES`. The Slack Web API has no
name-addressed channel endpoint — `conversations.info` takes an id only — so there is no template a
`#name` can be substituted into. Verified against the oracle:

```
$ cargo test -p qfs-driver-slack resolves_channel_name -- --nocapture
test tests::rest_client_resolves_channel_name_for_update_procedure ... ok
test tests::rest_client_resolves_channel_name_before_deleting_a_message ... ok
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 60 filtered out
```

Those tests assert the two-request shape `conversations.list` → `chat.delete`/`chat.update` with a
LOCAL name scan between them. A fan-out into `/http/slack/conversations.list` would splice one row
carrying a `channels` LIST column into the row, and no declared expression can select the element
whose `name` equals the row's own value. The exception is the `Uxxxx` → `Dxxxx` DM open, which IS a
pure template substitution (`conversations.open?users={channel}`) — so the DM half of the resolution
is expressible and the `#name` half is not.

**Wall 2 — the seam. The resolution the Slack CALLs need is on the WRITE path, and a map body has
no stage slot.** G4 as ruled (and as implemented) is a view-body stage. A declared CALL's channel
arrives as a CALL argument evaluated by `eval_map_body`, whose pipeline arm accepts exactly one
`ENCODE` stage over `VALUES` and refuses anything else. The grammar refuses it earlier still:

```
$ qfs run "CREATE MAP CALL slack.delete ( channel text, ts text ) /slack/{ws}/{channel}/messages \
    AS INSERT INTO /http/slack/chat.delete |> FOLLOW channel INTO /http/slack/conversations.list \
    VALUES ({channel: row.channel, ts: row.ts})"
{"error":{"code":"parse_error","kind":"parse","message":"closed-core keywords are lowercase
(recognized case-insensitively); this keyword is not valid here (blueprint §3, decision S)",
"detail":"UNKNOWN_KEYWORD"}}
```

**Why the drive stopped here rather than inventing a surface.** Closing QG4 needs a *second* ruled
primitive — a declared reverse lookup that selects one row out of a collection endpoint by a key,
usable in a map body, at a defined time. Adding half of that unattended (a write-path `FOLLOW …
INTO` that serves the DM case and not the `#name` case) would have grown the taught grammar while
leaving the equivalence bar exactly as unmet, and the developer may well rule a single stage that
subsumes both rather than two. That question is minted as
`20260726190000-declared-reverse-lookup-for-write-path-name-resolution.md`.

**Open questions for the developer, precisely:**

1. Should the reverse lookup be a new stage (`LOOKUP <field> IN /http/<drv>/<collection> BY <key>`),
   or should `FOLLOW … INTO` grow a selector so one stage covers both directions?
2. May a MAP body carry a resolution stage at all, and if so does it run at PREVIEW (a wire request
   during a pure preview, which today performs no I/O for effects) or at COMMIT immediately before
   the effect leg — which is where the COMPILED driver actually resolves, per
   `RestSlackClient::apply` above? Ticket 20260724014100's QG2 says "preview-time error"; the oracle
   it compares against resolves at apply. That mismatch needs settling before the equivalence test
   can be written to a true bar.

## Archived 2026-08-12 — this shipped, the ticket outlived its mission

The G4 stage is **implemented and covered**: `|> FOLLOW <field> INTO /http/<drv>/<template>` parses
(`crates/parser/src/tests.rs`, `follow_into_parses_the_per_row_fan_out_target`) and evaluates
(`crates/exec/src/declared.rs`, `fan_out_rows`) with the ticket's four rules mechanical — the
`FOLLOW_FANOUT_MAX` = 50 ceiling as a refusal rather than a truncation, per-row confinement to
`/http/<driver>/…`, a refusal (never a silent `Null`) for an unresolvable field, and one shape for
every row. It landed in `365d521` "Add per-row FOLLOW INTO fan-out" and reached `main` through
PR #27, so the ticket is filed under that branch's archive.

It sat in `todo` afterwards only because its mission (`the-declared-slack-twin-retires-the-compiled-driver`)
closed `carried` and the stamp kept it out of every survey — the same defect that stranded five
sibling tickets, corrected in the same pass. **Quality-gate item 4 was met by a different route:**
the Slack channel-name→id resolution is expressed with the §13.1 **G9** `LET` reverse lookup rather
than with this fan-out stage, and its equivalence against the compiled driver is proven in the twin's
tests.
