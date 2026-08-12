---
created_at: 2026-08-04T17:30:00+09:00
author: a@qmu.jp
assignees: [a@qmu.jp]
type: enhancement
layer: [Domain]
effort:
commit_hash:
category: Added
depends_on:
mission:
merge_policy:
---

# A declared lookup cannot be shared across maps, so the same binding is written once per map

## Overview

Minted while driving `20260724014100` (Slack CALL maps effect-equivalent), under the mission
`a-declared-write-resolves-a-name-the-way-a-query-does` — which was closed `achieved` on 2026-08-05.
The `mission:` stamp was cleared deliberately at that close so this ticket returns to the ordinary
backlog: `plan-units.sh` excludes any mission-stamped ticket from the backlog without checking
whether that mission is still active, so leaving the stamp on would have made this undrivable.
The provenance lives here in prose instead. Wiring the §13.1 G9
channel lookup into the five Slack CALL maps meant writing the **identical** binding five times:

```qfs
let cid = /slack/{ws}/channels |> WHERE name == row.channel OR id == row.channel |> SELECT id
```

All five maps mount at the same path (`/slack/{ws}/{channel}/messages`), resolve the same argument,
against the same view, by the same rule. The declaration language has no way to say that once.

## Measured — this is not a style complaint

It moved a shipped ratchet. `slack_driver.qfs` measured **40** statement-lines before, exactly at the
blueprint §13.2 one-screen bar; adding the five bindings measured **45**, and the bar assertion in
`shipped_slack_script_installs_statement_for_statement` had to move with it. Every one of those five
lines is the same line.

The §13.2 claim is that a declared twin is *more concise* than the compiled driver it replaces. That
claim now rests on a declaration whose growth is duplication rather than content, which is the
weakest possible form of it. A twin for a service with more ID-requiring calls than Slack's five
would degrade further and linearly.

## Scope

Rule and implement a way for one declaration to state a lookup once and have several maps use it.
Shapes worth weighing, deliberately not pre-chosen here:

1. **A named binding at declaration scope** — `CREATE LOOKUP slack/channel_id AS /slack/{ws}/channels
   |> WHERE …`, referenced by name from each map body. Most explicit; adds a DDL statement kind.
2. **Binding inheritance from the mount path** — maps sharing a mount path share its `LET` bindings,
   declared once against the path. No new statement kind; makes a map's bindings non-local to read.
3. **Leave it, and record the cost.** Five lines is five lines; the bar moves to 45 and the §13.2
   claim is restated in terms of what a declaration *contains* rather than how long it is.

**Out of scope.** Widening the accepted `LET` pipeline shape any further (the disjunction arm landed
with `20260724014100`); anything about where a lookup runs (COMMIT, ruled 2026-08-01).

## Considerations

- Shape 2 interacts with dispatch: the six shipped Slack maps share one mount path and are told apart
  by verb alone, so "the bindings of this path" is already a well-defined set. That is either the
  neatest fit or an accident worth not building on.
- Whichever shape wins, `map_body_lookups` currently reads the bindings off a single body's `LET`
  chain. A shared binding means the resolver needs the declaration's other rows, which is a wider
  input than it takes today.
- The related open concern `the-13-2-calibration-table-was` asks whether the ~40 bar is right at all
  now that a real conversion has tested it. That question and this one should be answered together —
  raising the bar is shape 3, and it should be chosen rather than defaulted into.

## Policies

- Blueprint §13.2 conciseness bar — the mission's own claim is that a declaration stops repeating
  itself. Five identical bindings is the declaration repeating itself.
- workaholic:implementation / objective-documentation — the bar assertion must state the measured
  reality and why it moved, not a number someone hopes for.

## Quality Gate

1. A ruling is recorded (in the blueprint, beside the G9 entry) choosing among the shapes above,
   with the rejected ones and the reason.
2. If a sharing mechanism is implemented: `slack_driver.qfs` states the channel lookup once, the five
   CALL maps still resolve identically (the existing equivalence and refusal tests pass unchanged),
   and the measured statement-line count is asserted at its new value.
3. If shape 3 is ruled instead: the §13.2 text is updated to say what the bar measures and why 45 is
   the figure, and this ticket closes against that edit.
4. Workspace gates green: `cargo test --workspace`, `cargo clippy --workspace --all-targets --
   -D warnings`, `cargo fmt --all --check`, `gen-docs --check`, `gen-skills --check`.
