---
created_at: 2026-07-26T19:00:00+09:00
author: a@qmu.jp
type: enhancement
layer: [Domain]
effort:
commit_hash:
depends_on: [20260725124400-declared-follow-into-per-row-fan-out-g4.md]
mission: the-declared-slack-twin-retires-the-compiled-driver
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

## The design question this ticket must settle FIRST

Two rulings are needed, and neither is derivable from the blueprint as written. The G4 drive stopped
rather than guessing, because adding a write-path fan-out that served the DM case and not the
`#name` case would have grown the taught grammar while leaving the equivalence bar exactly as unmet.

1. **Shape.** A new stage (`LOOKUP <field> IN /http/<drv>/<collection> BY <key>`) that selects one
   row out of a collection response by a declared key — or `FOLLOW … INTO` grows a selector so one
   stage covers both directions? Two stages risk teaching two names for one idea; one stage risks a
   clause that means different things in different arms. §13.2's conciseness bar applies: whichever
   is chosen, the Slack twin must not grow to buy it.
2. **Time.** May a MAP body carry a resolution stage at all, and if so does it run at **PREVIEW** (a
   wire request during a preview, which today performs no effect I/O at all) or at **COMMIT**,
   immediately before the effect leg? This matters because
   `20260724014100-slack-call-maps-effect-equivalent.md`'s QG2 asks for a *preview-time* structured
   error, while the COMPILED driver it compares against resolves inside `RestSlackClient::apply` —
   at commit. One of the two must move: either the declared twin is held to a stricter bar than its
   oracle (defensible, and arguably the better surface), or QG2's wording is corrected to
   "before the effect leg fires". **Settle this explicitly; do not let the equivalence test pick.**

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

1. The chosen shape and the chosen time are both recorded in blueprint §13.1 before the
   implementation lands, so the ruling outlives this ticket.
2. A declared write body resolves a name against a collection endpoint to exactly one row, proven on
   a hermetic fixture with BOTH recorded wire requests asserted in order (the lookup, then the
   effect leg carrying the resolved id).
3. An unresolvable name is a structured, secret-free refusal at the ruled time, and the effect leg is
   never issued — asserted by the absence of the second recorded request.
4. A name that matches more than one row in the collection is a refusal, not a pick.
5. `20260724014100-slack-call-maps-effect-equivalent.md`'s QG2 becomes provable against the compiled
   oracle on the shared fixtures, for BOTH the `#name` → `Cxxxx` and `Uxxxx` → `Dxxxx` arms.
6. Workspace gates green: `cargo test --workspace`,
   `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all --check`,
   `cargo run -p xtask -- gen-docs --check`, `cargo run -p xtask -- gen-skills --check`.

## Considerations

- `crates/exec/src/declared.rs` owns both `eval_view_body` (which now has the working per-row
  fan-out to copy the confinement + bound discipline from) and `eval_map_body` (which is pure today
  — a resolution stage means injecting a wire closure into it, exactly as the read side does).
- `crates/qfs/src/apply_facets.rs` is the seam where a map body meets the wire; if the ruling is
  COMMIT-time, the resolution belongs there, and if it is PREVIEW-time it needs a new hook because
  effect preview performs no I/O today.
- The compiled `resolve_channel_id` pages up to `MAX_PAGES` before declaring a miss. A declared
  lookup that reads only the first page would report "not found" for a workspace whose channel sits
  on page two — a quiet wrong answer of exactly the class the sibling predicate-honesty mission
  removed. Decide whether the declared lookup pages, or refuses when a next cursor exists.
