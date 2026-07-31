---
type: Mission
title: A declared write resolves a name the way a query does
slug: a-declared-write-resolves-a-name-the-way-a-query-does
status: draft
created_at: 2026-07-27T13:55:01+09:00
author: a@qmu.jp
assignee: a@qmu.jp
strategy: integrations-are-declared-not-compiled
predicted_hours:
actual_hours:
tickets: []
stories: []
concerns: []
gate_type:
gate_target:
gate_assert:
carried_from: the-declared-slack-twin-retires-the-compiled-driver
---

# A declared write resolves a name the way a query does

## Goal

**A declaration should not need a parallel mechanism for something the query language already
expresses.** The predecessor mission (`the-declared-slack-twin-retires-the-compiled-driver`, carried
2026-07-27) proved the declared Slack driver reads row-equivalent to the compiled one and retired the
`/cf` queue-pull, then stopped on a single wall: a declared **write** cannot turn `#general` into
`C0123ABCD`.

The compiled driver does it in three steps — GET `conversations.list`, scan the returned array
locally for `name == general`, use that row's `id`. Slack's Web API offers no name-addressed channel
endpoint, so this is a **reverse lookup against a collection**, not a substitution into an address.

Two overnight runs each found the remainder one layer deeper than the plan. The second implemented
blueprint §13.1 **G4** (`|> FOLLOW <field> INTO /http/<drv>/<template>`) on the belief that per-row
fan-out was the missing piece. It was not: G4 substitutes a value **into** an address, which is the
opposite direction. G4 is shipped and useful; it simply does not answer this.

**The finding that reframes this mission (2026-07-27):** the language already has the piece. `let`
binds a pipeline (`binding = "let", name, "=", (pipeline | lambda | literal)`) and a let-bound name is
a legal source, so the reverse lookup is expressible today with **zero new grammar**:

```qfs
let cid = /slack/{ws}/channels |> WHERE name == row.channel |> SELECT id
```

It does not work inside a declaration for one narrow reason: `create_map_stmt` parses its body with
`inner_statement`, and `let_binding` has exactly one call site — `program_seq` — reached only at top
level and as a `let`'s own body. **A parser wiring decision, not a limit of the language.**

Verified by reading (`packages/qfs/crates/parser/src/grammar.rs`): the map body parses at
`create_map_stmt`; `program_seq` is the sole `let_binding` call site; `body_to_json` is a plain serde
serialization of `Statement`, so a `Let` node round-trips without special-casing; and `Statement::Let`
is already handled in `core/src/eval.rs`, `core/src/resolve.rs` and the exec layer.

This mission is worth doing beyond Slack: the same reverse lookup is what the Drive twin (playbook
entry #3) needs for its path→id parent walk, and what every future declared write that addresses
something by a human-readable name will need.

**Widened 2026-07-28 (developer ruling): the mission's property is that a declared write can express
what the target API actually requires — name resolution is its headline case, not its whole extent.**
A second instance of the same shape arrived from another repository via `/request`: the shipped
`INSERT INTO /chatwork/rooms/{room}/messages` — declared in `chatwork.qfs` and taught by
`docs/cookbook/chatwork.md` — **always fails with 400** against the live API. The declared map sends
the row as a JSON body; that endpoint, like most plain-REST APIs of its generation, accepts only
`application/x-www-form-urlencoded`, and `ENCODE` has no form codec (json, jsonl, yaml, toml, csv,
md, multipart — no form).

So a form-parameter REST API is today **readable but not writable** through a declaration. That is a
direct counter-example to the blueprint §13 claim that such an API is expressible as declarative
config, and it is the same failure as the Slack one at a different layer: the declared write cannot
say something the wire demands. The mission's slug still names the headline case; the Goal above is
the general property.

## Scope

**Done when** a declared write can resolve a name against a collection using the query language, a
declared write can produce a form-urlencoded body so a plain-REST API is writable and the shipped
Chatwork INSERT actually commits, the five Slack CALL maps are effect-equivalent to the compiled ones
on the shared fixtures, and the compiled `driver-slack` crate is deleted per the shared retirement
steps.

**The two axes are independent and can be driven in either order.** The form codec has no open design
ruling blocking it — its shape is settled in its own ticket — so it is drivable now, while the name
resolution waits on the spelling ruling below. Do not let the blocked axis hold the unblocked one.

**The one open ruling, and it is what blocks `drive_authorized`.** How the reverse lookup is spelled
is not yet decided. Three candidates were written out with real code on 2026-07-27:

- **Open the map body to `let`** — parse the body as `program_seq`, and confine the `let` source to
  the driver's own declared surface the way G4 confines fan-out targets. Zero new keywords; reuses
  the already-declared `/slack/{ws}/channels` view, so that view's cursor paging and
  `OF slack/channel` contract apply without restatement; composes with the already-ruled "map body
  reaches its path `{param}`" ticket. Its cost is a new confinement rule and a decision about what a
  relation-valued `let` means where a scalar is expected. **Recommended.**
- **A new `LOOKUP` stage** — one stage, one meaning. But it needs the same map-body structural change
  *plus* new syntax, it addresses the raw wire endpoint so it must re-express
  `DECODE json |> EXPAND channels`, and it re-opens the paging question the declared view already
  answers.
- **A selector on `FOLLOW … INTO`** — one name covers both directions, but the same clause then means
  substitution or search depending on whether the target carries a `{param}`, which a reader cannot
  tell without inspecting the URL.

**Unverified, and to be settled by a throwaway spike before the ruling is applied:** whether parsing
the body as `program_seq` actually works end to end, whether the body-level `let` evaluates **per
row** (each row may name a different channel, so once-only evaluation is useless), and whether the
declared write path walks a stored `Let` node.

**Out of scope:**

- **The github/drive/mail twins** — playbook entries #2-#4, their own missions, even though this
  mission's mechanism is what unblocks the drive twin.
- **G7 blob-namespace ergonomics and G8 non-REST arms** — parked by §13.1.
- **Live Slack verification.** Equivalence is proven on hermetic shared fixtures; this environment has
  a live-connected Slack workspace and no step here may post, upload, or probe it.
- **Widening the declared write surface** beyond what these two axes require. Name resolution and body
  encoding are both "the declared write cannot say what the wire demands"; a third capability needs
  its own evidence, not a ride on this mission.

**Attended step, and it cannot be driven unattended.** The form-codec ticket's first Quality Gate item
requires the Chatwork INSERT to commit **against the live API and appear in the room** — a write other
people can see. Everything else in that ticket (the encoder unit tests, the `|> ENCODE form` parse
test, the workspace gate) is hermetic and drivable; the live confirmation is the developer's attended
call. Land the hermetic part, then hold that one item for an attended round rather than recording the
ticket blocked.

## Experience

- An author writes a declared write that addresses a channel by name and resolves that name with the
  same `|> WHERE … |> SELECT …` they would type at the shell — not a declaration-only construct they
  have to learn separately.
- The resolution reuses the driver's own already-declared read view, so paging and the type contract
  written once apply to it without restatement.
- A name that matches nothing, or matches more than one row, is a **structured refusal before the
  effect fires** — never a guess, never a garbage id on the wire, never a silent no-op. This is the
  predicate-honesty law applied to writes.
- `PREVIEW` on a name-addressed write shows the **resolved** destination, so the preview states what
  will actually happen instead of deferring the question to commit.
- A declaration still cannot read outside its own driver's surface: whatever binds a lookup source is
  confined and re-checked the way a G4 fan-out target is, so the host-confinement guarantee at the
  head of every declaration file survives.
- A declared write against a form-parameter REST API commits. Concretely, the `INSERT` printed in
  `docs/cookbook/chatwork.md` succeeds and the message appears in the room — so the cookbook stops
  teaching a statement that always answers 400.
- What `ENCODE form` does with a nested or bytes field is stated, not implicit: an unencodable field
  is refused with a structured error rather than silently flattened or dropped.

## Acceptance

- [ ] The five typed CALL maps are effect-equivalent to the compiled CALLs on fixtures (#20260724014100-slack-call-maps-effect-equivalent.md)
- [ ] driver-slack is deleted per the shared retirement steps with docs/skills regenerated and the plugin minor-bumped in all four fields (#20260724014200-retire-the-compiled-slack-driver.md)
- [ ] A declared write can produce a form-urlencoded body, the shipped Chatwork INSERT commits against the live API, and the cookbook teaches the working statement (#20260727214856-declared-rest-drivers-cannot-post-form-encoded-bodies.md)

## Changelog

- 2026-07-27 — mission created by carrying `the-declared-slack-twin-retires-the-compiled-driver`; two unmet criteria inherited — mission.md
- 2026-07-27 — reframed from the predecessor's inherited body: the wall is a reverse lookup, not per-row fan-out, and `let` already expresses it — the gap is that a map body parses as `inner_statement`, which excludes `let_binding` — mission.md
- 2026-07-27 — strategy linked — integrations-are-declared-not-compiled
- 2026-07-27 — `drive_authorized` deliberately left unset: the spelling ruling above is a genuine design fork and the three unverified spike questions must be answered before a ticket set can be written — mission.md
- 2026-07-28 — ticket added - routed from the root queue by developer ruling 2026-07-28; the mission's Goal widened from name resolution to the general property that a declared write can express what the API requires — 20260727214856-declared-rest-drivers-cannot-post-form-encoded-bodies.md
