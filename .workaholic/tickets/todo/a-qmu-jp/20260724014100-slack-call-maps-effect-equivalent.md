---
created_at: 2026-07-24T01:41:00+09:00
author: a@qmu.jp
type: enhancement
layer: [Domain]
effort:
commit_hash:
category: Added
depends_on: [20260724014000-declare-the-slack-twin-and-prove-read-equivalence.md, 20260725124400-declared-follow-into-per-row-fan-out-g4.md]
mission: the-declared-slack-twin-retires-the-compiled-driver
---

# Slack CALL maps effect-equivalent

## Overview

Playbook §13.3 entry #1, second half of the equivalence bar: the five compiled Slack CALLs —
**react, pin, unpin, update, delete** — become **typed CALL maps** in `slack.qfs` per the G5
ruling (typed CALL signatures, blueprint §13.1), each effect-equivalent to its compiled
counterpart on hermetic wire fixtures.

The v0.0.89 compiled-driver behavior is the contract to reproduce: every ID-requiring call routes
through channel-name→id resolution (one address, one meaning), and unresolvable names fail at
PREVIEW time as usage errors, never as garbage ids at commit.

## Policies

- workaholic:design / 「推測するな、宣言して拒否せよ」 — an unresolvable channel/user reference is
  refused at preview, exactly as the compiled driver does since v0.0.89.
- Blueprint §13.1 G5 — CALL signatures are typed; a wrong-shaped argument is a parse/typecheck
  error, not a wire error.
- workaholic:development / hermetic gates — no live Slack tokens; fixtures only.

## Quality Gate

1. Each of the five CALL maps produces the same wire request as the compiled CALL on the shared
   fixtures (method, endpoint, resolved channel id, payload) — five effect-equivalence tests.
2. The name→id resolution behavior matches: a fixture case proves a name-addressed channel
   resolves before the effect fires, and an unresolvable name is a structured preview-time error.
3. Typed signatures reject a malformed argument at typecheck (one negative case per distinct
   signature shape).
4. Workspace gates green: `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`,
   `cargo fmt --all --check`.

## Considerations

- If the honest verb set the declaration states naturally resolves the tracked concern
  `slack-workspace-namespace-still-advertises-verb` (Verb::Rm advertised without grammar), record
  that in the final report so the ship-time concern judge can close it — but do not add
  file-delete capability to force it.

## Progress (partial — NOT complete; ticket stays open)

An overnight `/monitor` drive (run 20260725-101714) landed the **G5 half** of this ticket and stopped
at the wall the remaining half sits behind. What is committed and green:

- **G5 grammar** — `CREATE MAP CALL <drv>.<action> ( <param> <type>, … ) /<node> AS <effect>
  [IRREVERSIBLE]`. The typed parameter list is optional; without it the CALL is untyped (the
  pre-G5 shorthand, preserved). The signature is rendered into the stored `verb` label
  (`CALL slack.react(channel text, ts text, emoji text)`), so it lives in the column the registry
  already keys declarations by — no schema change.
- **Two lexer facts the grammar forced**, both structural rather than guessed:
  (a) a `/` after `)` is ordinarily DIVISION, so `… ( channel text ) /slack/{ws}/…` did not lex as a
  path; the lexer now treats it as a path start only when that paren is matched back to a
  `CALL <ident> . <action>` head, leaving `(a + b) / c` arithmetic untouched. (b) a procedure NAME
  may collide with a frozen keyword (`slack.update`, `slack.delete`) — the action sits in a name
  position, so a keyword-shaped word is read as its canonical text rather than forcing a rename that
  would break signature parity with the compiled registry.
- **Typed signatures surfaced** — a declared driver lifts its `CALL` maps to typed `ProcSig`s carried
  on the describe mount and the live driver, so `DESCRIBE` reports the declared signature cred-free.
- **The five maps are declared** in `slack_driver.qfs` (react / pin / unpin / update / delete), with
  `pin` and `delete` marked `IRREVERSIBLE`.
- **Signature parity is asserted, not asserted-in-prose**:
  `shipped_slack_call_maps_carry_typed_g5_signatures` compares the declared `ProcSig` list against
  the COMPILED `SlackDriver::procedures()` — name, parameter names, parameter types, irreversibility
  — and they are equal. `declared_call_signature_parses_typed_and_untyped` covers both grammar arms.

A second `/monitor` wave of the same run (20260725-101714) then closed **QG1 and QG3**. What that
wave landed:

- **Declared CALL DISPATCH is wired** (`crates/qfs/src/apply_facets.rs`). A `CALL` effect on a
  declared mount now selects the map whose declared **action** matches the called procedure — not
  merely its mount path, which matters because the six shipped Slack maps (the post `INSERT` plus the
  five CALLs) ALL mount at `/slack/{ws}/{channel}/messages`; a path-only match would have fired
  whichever map was declared first. The stored verb label rides onto `MapSpec` for that selection,
  and `MapWrite` now carries the wire verb the map BODY declares, which the facet stamps onto the
  wire effect (`Call` is not a kind the generic REST driver services). A CALL matching no declared
  CALL map still reaches the stock applier and is refused terminally — an unmapped CALL fails, it
  never POSTs something else.
- **A parse gap the dispatch exposed** (`crates/parser/src/grammar.rs`): `|> CALL <drv>.<action>`
  read the action with `ident`, so `CALL slack.update(…)` did not parse — `update` is a frozen
  keyword. The registry could DECLARE a procedure no `CALL` could spell (this hit the COMPILED
  driver too, which has advertised `slack.update` since v0.0.89). The action sits in a NAME position,
  so it now reads a keyword-shaped word as its canonical text — the same ruling `CREATE MAP CALL`
  already makes.
- **QG1 is proven** by `shipped_slack_call_maps_are_wire_equivalent_to_the_compiled_calls`: for each
  of react / pin / unpin / update / delete, the SHIPPED asset's map (bodies read out of
  `slack_driver.qfs` itself, so the proof cannot drift from what an install writes) is driven through
  the full commit stack to a recorded wire request, the compiled `SlackEffect` twin is driven through
  the real `RestSlackClient` over a recording transport, and the two requests are asserted equal on
  METHOD, ENDPOINT and PAYLOAD — with the channel id asserted to reach the wire unchanged. The
  channel is addressed by an already-resolved `Cxxxx` id, so the proof does not depend on the
  unimplemented resolution below.
- **QG3 is proven** by `declared_slack_call_signatures_reject_a_malformed_argument`: with dispatch
  wired, a `|> CALL` against the declared mount resolves against the declared G5 signatures, so a
  wrong-shaped argument is refused at TYPECHECK before a plan exists. One negative case per distinct
  declared shape — `react(channel, ts, emoji)` with a surplus argument (`arity_mismatch`),
  `pin(channel, ts)` with react's `emoji` (`unknown_arg`, and the refusal names the DECLARED
  parameters), `update(channel, ts, text)` with `emoji` for `text` (`unknown_arg`).

**What is NOT done — Quality Gate item 2 is still open:**

1. ~~**Wire-level effect equivalence (QG1).**~~ Closed by the second wave (above).
2. **Name→id resolution parity (QG2) — the real wall, still standing.** The compiled driver resolves `#name` → `Cxxxx`
   (and `Uxxxx` → `Dxxxx`) INSIDE its live client, on the way to every ID-requiring call. A
   declaration cannot express that today: it is a per-row fan-out into a second wire request, which
   is blueprint §13.1 **G4** (`|> FOLLOW <field> INTO /http/<drv>/<template>`) — **ruled but not
   implemented**. Until G4 ships, a declared CALL can only accept an already-resolved id, so neither
   "a name-addressed channel resolves before the effect fires" nor "an unresolvable name is a
   structured preview-time error" can be reproduced. This is the blocking dependency, and it is
   internal, not external.
3. ~~**Typecheck rejection of a malformed argument (QG3).**~~ Closed by the second wave (above).

**Recommended next slice:** implement G4 per-row fan-out — now queued as its own ticket,
`20260725124400-declared-follow-into-per-row-fan-out-g4.md`. It is the shared prerequisite for this
ticket's QG2 *and* for the drive twin (playbook entry #3), and it is the only thing left between this
ticket and closure.

**This ticket therefore stays OPEN on QG2 alone**, and
`20260724014200-retire-the-compiled-slack-driver.md` stays BLOCKED on it: deleting `driver-slack`
now would remove the compiled oracle the outstanding resolution proof compares against.

**The tracked concern `slack-workspace-namespace-still-advertises-verb`** is still NOT resolved: CALL
dispatch is wired now, but the twin's honest verb set is settled only when the retirement ticket
lands, and that is blocked on QG2.

**One thing observed but deliberately NOT changed** (no shipped declaration hits it today, so it is a
note, not a ticket): a UNIVERSAL-verb map still issues its wire leg under the OPERATOR's verb rather
than the verb its body declares, so a hypothetical `CREATE MAP REMOVE … AS INSERT INTO /http/…` would
send a DELETE carrying a body. Only the CALL arm consults `MapWrite::wire_kind`. Every shipped map
(chatwork, cloudflare, slack) declares a body whose verb already matches its map verb.

## Replan (developer, 2026-07-26)

**G4 is now an explicit prerequisite of this ticket, not a discovery inside it.**
`depends_on` gains `20260725124400-declared-follow-into-per-row-fan-out-g4.md`.

Why the plan changed: this mission's entry conditions checked that blueprint §13.1 G1 and G2 had
shipped and never asked about G4. Quality Gate item 2 — a name-addressed channel resolves before
the effect fires, and an unresolvable name is a structured preview-time error — cannot be written
in a declaration at all until per-row fan-out exists, because resolving `#name` to `Cxxxx` is a
second wire request per row. The overnight run of 2026-07-25 closed QG1 and QG3 against
already-resolved ids and stopped honestly at QG2 rather than weakening it to something passable.

G4 is also the prerequisite for playbook entry #3 (the drive twin's path→id parent walk), so it is
sequenced as its own ticket ahead of both rather than buried as a step inside either.

**Nothing in the mission's `## Acceptance` changed** — the agreed plan stands; only the ordering
under it does. This ticket stays open, and `20260724014200` stays blocked behind it: deleting
driver-slack would remove the compiled oracle the outstanding effect-equivalence proof compares
against.

Remaining here after G4 lands: QG2 only. QG1 and QG3 are closed at `73fa5de`.
