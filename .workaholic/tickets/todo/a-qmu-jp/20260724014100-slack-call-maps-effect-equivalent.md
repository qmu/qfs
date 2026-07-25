---
created_at: 2026-07-24T01:41:00+09:00
author: a@qmu.jp
type: enhancement
layer: [Domain]
effort:
commit_hash:
category: Added
depends_on: [20260724014000-declare-the-slack-twin-and-prove-read-equivalence.md]
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

**What is NOT done — this ticket's Quality Gate items 1-3 are still open:**

1. **Wire-level effect equivalence (QG1).** The five maps are declared and their bodies are the right
   shape, but no test yet drives BOTH the declared map and the compiled `SlackEffect` to a recorded
   wire request and compares method / endpoint / resolved channel id / payload. Signature parity is
   the contract half; the effect half is unproven.
2. **Name→id resolution parity (QG2) — the real wall.** The compiled driver resolves `#name` → `Cxxxx`
   (and `Uxxxx` → `Dxxxx`) INSIDE its live client, on the way to every ID-requiring call. A
   declaration cannot express that today: it is a per-row fan-out into a second wire request, which
   is blueprint §13.1 **G4** (`|> FOLLOW <field> INTO /http/<drv>/<template>`) — **ruled but not
   implemented**. Until G4 ships, a declared CALL can only accept an already-resolved id, so neither
   "a name-addressed channel resolves before the effect fires" nor "an unresolvable name is a
   structured preview-time error" can be reproduced. This is the blocking dependency, and it is
   internal, not external.
3. **Typecheck rejection of a malformed argument (QG3).** Declared CALL maps are declared and
   advertised, but `CALL` DISPATCH for a declared mount is not wired (`declared_driver::map_verb`
   still returns `None` for a `CALL …` label, so no wire verb is aggregated for it), so there is no
   path on which a wrong-shaped argument is checked.

**Recommended next slice:** implement G4 per-row fan-out first (it is the shared prerequisite for
this ticket's QG2 *and* for the drive twin, playbook entry #3), then wire declared-CALL dispatch, then
the five wire-level equivalence tests.

**The tracked concern `slack-workspace-namespace-still-advertises-verb`** is NOT resolved by this
partial: the declared twin's honest verb set cannot be final while CALL dispatch is unwired.
