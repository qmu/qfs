---
created_at: 2026-07-26T09:00:00+09:00
author: a@qmu.jp
type: enhancement
layer: [Domain]
effort: 1h
commit_hash:
category: Changed
depends_on:
mission: a-declared-write-resolves-a-name-the-way-a-query-does
---

# A map body expression can reference the path's own parameters

## Overview

A declared driver maps a qfs path onto an external API call. The map's **target** — the wire URL it
sends to — can already interpolate the `{param}` segments bound by the qfs path. The map's **body**
— the expression that builds what gets sent — cannot: it is closed over the incoming row, so it can
only reference that row's columns.

The result is a declaration that says the same thing twice by two different routes. The shipped
Slack post map is addressed as
`/slack/{ws}/channels/{channel}/messages`, but its body cannot read `{channel}`, so it takes the
channel from a column on the inserted row instead. The caller names the destination in the path and
then has to supply it again in the payload.

**Ruled by the developer on 2026-07-26: extend `{param}` binding into map body expressions.** The
alternative — formalising the row-closed rule and documenting it — was considered and rejected: the
duplication is not a simplification, and a declaration that ignores half of its own address is
harder to read, not easier.

## Measured

`slack_driver.qfs`'s post map is the concrete instance. During the overnight run of 2026-07-25 the
drive leaf recorded the constraint rather than working around it silently:

> the declared post map takes `channel` from the incoming row rather than the path's `{channel}`
> segment, because a MAP body's `VALUES` expression is row-closed — path `{param}` bindings reach
> the map's wire TARGET but not its body expression.

## Scope

**In scope.** Make the `{param}` bindings a declared map already resolves for its target also
resolvable inside the map's body expression, for every universal verb's map form (not a CALL-only
special case). Update `slack_driver.qfs`'s post map to take `channel` from the path, which is what
its own address already says.

**Out of scope.**

- G4 per-row fan-out (`20260725124400`). That is a *second wire request* per row; this is a *binding
  already in hand* that the body cannot see. They are independent, and this one does not unblock
  QG2 of `20260724014100`.
- Any change to how the target itself interpolates. That works and is not being redesigned.

## Considerations

- **Name collision is the design question to settle first.** If a `{param}` and an incoming row
  column share a name, one must win and the rule must be stated, not discovered. Decide it before
  writing the resolver, and pin it with a test that would fail if the precedence flipped.
- The mission's law is that a declaration is honest about what it does. A body that silently
  resolved a name to a row column when the author meant the path parameter would be exactly the
  class of quiet wrong answer the sibling predicate-honesty mission spent a whole branch removing —
  so an ambiguous reference is better refused than guessed.
- This changes a taught surface if any cookbook article shows a map body. Check
  `docs/cookbook/*.md`, and if one does, edit the article and regenerate — never hand-edit a
  `SKILL.md` — and bump all four plugin `version` fields.
- No shipped declaration other than Slack's post map is known to hit this; `chatwork.qfs` and
  `cloudflare.qfs` should be checked rather than assumed.

## Policies

- workaholic:design / 「推測するな、宣言して拒否せよ」 — when a name in a map body could resolve to
  either a path parameter or a row column, the ambiguity is refused at PREVIEW as a structured usage
  error. Guessing one and silently sending the other is the failure this ticket exists to prevent,
  not a convenience.
- workaholic:implementation / honest-surfaces — a declaration must mean what its own address says. A
  map addressed at `{channel}` whose body reads a different `channel` is a surface that reads true
  and behaves otherwise.
- Blueprint §13.2 conciseness bar — the point of this change is that a declaration stops repeating
  itself; the Slack post map must not grow to buy the path binding.

## Quality Gate

1. A map body expression can reference a `{param}` bound by its own path, proven on a hermetic
   fixture that asserts the resulting wire request.
2. The name-collision rule between a `{param}` and a row column is decided, documented at the seam,
   and pinned by a test that fails if the precedence changes.
3. `slack_driver.qfs`'s post map takes `channel` from the path, and the existing read- and
   effect-equivalence tests still pass unchanged.
4. Workspace gates green: `cargo test --workspace`, `cargo clippy --workspace --all-targets --
   -D warnings`, `cargo fmt --all --check`, `cargo run -p xtask -- gen-docs --check`,
   `cargo run -p xtask -- gen-skills --check`.

## Final Report

A map body now binds a second struct, `path`, carrying the `{param}` segments its own address
already resolved for the wire target. The shipped Slack post map reads `path.channel` and no longer
takes the destination from a column on the inserted row.

**The collision question was designed out rather than adjudicated.** The ticket asked for a
precedence rule between a `{param}` and a row column of the same name. A precedence rule is the
wrong shape of answer here: whichever way it falls, one of the two references silently resolves to
something the author may not have meant, which is the quiet wrong answer the mission's own law
forbids. Separate namespaces (`path.channel` vs `row.channel`) mean the two are simply different
expressions and nothing is ever guessed. The test that would have pinned a precedence instead pins
that the two stay separate — it fails if they are ever merged behind one bare name, whichever
direction the merge favours.

One collision does remain possible and is refused: a §13.1 G9 `LET` binding named `row` or `path`
would shadow one of the two names the body itself binds, so it is rejected at evaluation.

No taught surface moved — `slack_driver.qfs` is not shown in any cookbook article, and the two
articles that do print a `CREATE MAP` body (chatwork, cloudflare) use `VALUES (row)` with no path
reference. `gen-skills --check` and `gen-docs --check` both report in sync, so no plugin version
bump is owed by this ticket.

### Discovered Insights

- **Insight**: `eval_map_body` already received the path bindings as its `params` argument — they
  were rendered into the wire target on the line above and then dropped. The body could not see them
  not because they were unavailable but because nothing bound them into the per-row schema.
  **Context**: The "row-closed" constraint recorded by the overnight run of 2026-07-25 reads like a
  design boundary and was actually a missing two-line binding. Worth remembering when a recorded
  constraint is phrased as a property of the language rather than of a specific function.

- **Insight**: The path struct is row-invariant, so it is built once outside the per-row loop. Every
  other binding in that function varies per row, which is why the loop is shaped the way it is.
  **Context**: A future binding that is also statement-scoped (a connection attribute, a mount
  coordinate) belongs in the same place rather than inside the loop.
