# Design brief — how a declared write resolves a name against a collection

Written 2026-07-31, after the throwaway spike the mission's Scope required before the spelling
ruling is applied. Every claim below is cited to the source it was read from at `4b485df`.

## Why this brief exists

The mission records three questions as **unverified, to be settled by a spike before the ruling**:

1. whether parsing the map body as `program_seq` actually works end to end,
2. whether a body-level `let` evaluates **per row** (each row may name a different channel, so a
   once-only evaluation is useless),
3. whether the declared write path walks a stored `Let` node.

All three are now answered. One of them changes the shape of the decision, so the three spelling
candidates the mission carries are restated here against what the runtime actually does.

## What the spike found

### 1. The parser claim is exactly right — and it is one word

`create_map_stmt` parses its body with `inner_statement`
(`packages/qfs/crates/parser/src/grammar.rs:2445`). `let_binding` (`:3513`) has exactly one call
site, `program_seq` (`:3576`), which is `alt((let_binding, inner_statement))` — so `inner_statement`
structurally cannot reach a `LET`. Changing `:2445` to `program_seq` makes `LET` legal inside a map
body with no other parser change.

Serialization survives it untouched: `body_to_json` (`:2048`) is a plain
`serde_json::to_string(stmt)`, so a `Statement::Let` node round-trips into and out of the stored
driver row with no special-casing.

**Answer: yes at the parse and storage layers.**

### 2. The declared write path does NOT walk a `Let` node

`eval_map_body` (`packages/qfs/crates/exec/src/declared.rs:518`) rehydrates the stored body and
immediately requires an effect:

```rust
let Statement::Effect(effect) = stmt else {
    return Err(invalid("declared map body is not a write effect"));
};
```

A stored `Statement::Let` fails on that line. **Answer: no.**

### 3. Per-row evaluation exists — but only for pure scalars, and that is the wall

`eval_map_body` lowers **one** `VALUES` expression to a per-row scalar via `lower_scalar`
(`declared.rs:580`), then maps it over the incoming rows, binding each row as a single `row` struct
column and running it through `eval_value` (`declared.rs:586-602`).

So per-row evaluation is real and already there. But `eval_value` is a **pure scalar evaluator with
no I/O**, and the function's own contract says so (`declared.rs:506`):

> Purity holds: this constructs the wire effect; the caller's confined applier performs the I/O at
> COMMIT.

A `let` bound to `/slack/{ws}/channels |> WHERE name == row.channel |> SELECT id` is a **relation
requiring a fetch**. It cannot ride the per-row scalar evaluator, whatever the parser accepts.

**Answer: per-row, yes; but the lookup cannot use it.**

### The finding that matters

The mission says the gap is *"a parser wiring decision, not a limit of the language."* That is true
of the parser and false of the runtime. **The real gap is that the map body is pure by contract and
a name lookup is I/O.** A run that flipped `:2445` and stopped would have hit the same wall one layer
deeper — for the third time in this mission's history.

The decision is therefore not only *how the lookup is spelled* but **where it runs**.

### A fourth finding: the equivalence ticket's PREVIEW bar is not what the compiled driver does

Ticket `20260724014100` states the contract to reproduce as: *"unresolvable names fail at PREVIEW
time as usage errors, never as garbage ids at commit."* The compiled driver does not do the first
half.

- `driver-slack/src/path.rs:45` — a symbolic `#name` is *"needing a `conversations.list` lookup **at
  commit**"*.
- `path.rs:66` — *"the `@name`→id resolution is I/O performed by the applier **at commit**"*.
- `resolve_channel_segment` (`client.rs:356`) is reached only from the applier's send path.
- `exec/src/lib.rs:331` — *"**PREVIEW structurally cannot reach the executor**"*; preview renders
  `plan_preview(plan)` and nothing else.
- What PREVIEW actually prints is `ChannelRef::symbolic()` (`path.rs:54`) — the **unresolved**
  `#general`.

So "resolved destination at PREVIEW" (mission Experience) and "unresolvable name refused at PREVIEW"
(ticket QG2) are **new requirements, not parity**. Holding the declared twin to them means clearing a
bar the compiled original never cleared — and doing it in the twin only would make the two
*non*-equivalent, which is the one thing the equivalence ticket exists to prevent.

## The decision

Two axes, and the second is the expensive one.

### Axis A — where the lookup runs

**A1. Commit-time, inside the confined applier (parity).** `RestApplyDriver::apply_one`
(`qfs/src/apply_facets.rs:69`) is already `async`, already holds `cx`, already confined. Resolve the
`Let` values there — one fetch of the driver's own declared read view per statement, cached, then a
local per-row match. This is precisely what the compiled driver does (`conversations.list`, then scan
the array). `eval_map_body` stays pure: the resolved values are bound as extra columns before the
pure per-row evaluator runs, so the purity contract at `declared.rs:506` survives verbatim.

- Unresolvable name → terminal effect error at commit. No garbage id ever reaches the wire.
- PREVIEW shows the unresolved symbolic name — **identical to the compiled driver today**.
- Cost: one new async pre-pass in the applier. No change to preview semantics, no change to the
  compiled side, equivalence provable on the existing hermetic fixtures.

**A2. Plan-time, so PREVIEW shows the resolved destination.** Delivers the mission's Experience
bullets literally. But planning performs no wire I/O for writes today, so this makes **PREVIEW
perform a network read** — preview stops being free and offline, for every name-addressed write, and
the change is not local to this mission: to stay equivalent the compiled driver needs it too. This is
a product-wide re-ruling of what PREVIEW means, not a step in a driver twin.

**A3. A1 plus a preview-time *shape* check.** Preview refuses a **malformed** reference (not a legal
channel-name or id shape) with no I/O; a well-formed but non-existent name still fails at commit.
Cheap, honest, and it is the strongest preview guarantee obtainable without A2's semantic change.

### Axis B — how it is spelled

Unchanged from the mission's three candidates, and the spike does not disturb the ranking:

- **B1. Open the map body to `LET`** — the one-word parser change above, with the `let` source
  confined to the driver's own declared surface the way a G4 fan-out target is. Zero new keywords;
  reuses the already-declared `/slack/{ws}/channels` view so its cursor paging and `OF slack/channel`
  contract apply without restatement; composes with the already-ruled "map body reaches its path
  `{param}`" ticket.
- **B2. A new `LOOKUP` stage** — needs the same runtime work *plus* new syntax, addresses the raw wire
  endpoint so it must re-express `DECODE json |> EXPAND channels`, and re-opens the paging question
  the declared view already answers.
- **B3. A selector on `FOLLOW … INTO`** — one clause means substitution or search depending on whether
  the target carries a `{param}`, which a reader cannot tell without inspecting the URL.

## Recommendation

**A3 + B1**, and **re-scope the equivalence ticket's PREVIEW wording to what v0.0.89 actually does.**

- **B1** because the spike confirmed its parser cost is one word and its storage cost is zero, and
  because it is the only candidate that adds no vocabulary to the language.
- **A3** because it gives the mission's real guarantee — *never a garbage id on the wire, never a
  silent no-op* — at the cost of one async pre-pass, while keeping the pure evaluator and the preview
  contract untouched.
- **The re-scope** because ticket `20260724014100` currently asks the declared twin to refuse at
  PREVIEW something the compiled driver refuses at commit. Left as written, the equivalence tests
  cannot both pass and prove equivalence.

If the PREVIEW upgrade is wanted, it is **A2 and its own mission**: it changes what PREVIEW means for
every driver, and it must land on both sides of the twin at once or the twin is not a twin.

## What each ruling unblocks

| Ruling | Immediately drivable |
| --- | --- |
| A3 + B1 + re-scope | `20260726190000` (reverse lookup), then `20260724014100` (five CALL maps), then `20260724014200` (delete the compiled crate) |
| A2 instead | none of the above until PREVIEW semantics are re-ruled product-wide |
| Spelling only, axis A deferred | nothing — the parser change alone does not make a lookup run |

Independent of all of it: `20260727214856` (the `ENCODE form` codec, the Chatwork 400) has no design
dependency and is drivable now except for its one live-confirmation item.
