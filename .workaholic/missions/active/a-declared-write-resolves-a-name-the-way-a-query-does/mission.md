---
type: Mission
title: A declared write resolves a name the way a query does
slug: a-declared-write-resolves-a-name-the-way-a-query-does
status: active
created_at: 2026-07-27T13:55:01+09:00
author: a@qmu.jp
assignee: a@qmu.jp
strategy: integrations-are-declared-not-compiled
predicted_hours:
actual_hours:
tickets:
  - 20260725103000-declared-expand-must-splice-by-field-name.md
  - 20260726090000-map-body-expressions-can-reference-path-params.md
  - 20260726190000-declared-reverse-lookup-for-write-path-name-resolution.md
  - 20260724014100-slack-call-maps-effect-equivalent.md
  - 20260724014200-retire-the-compiled-slack-driver.md
  - 20260727214856-declared-rest-drivers-cannot-post-form-encoded-bodies.md
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

**The two axes are independent and can be driven in either order.** The form codec never had an open
design ruling blocking it, and the name-resolution axis is now ruled below, so both are drivable.
Neither may hold the other.

**The ruling is made (developer, 2026-08-01), and `drive_authorized` is stamped.** The spike this
Scope required ran first; its findings and the full option analysis are in
`design-brief-reverse-lookup.md` beside this file. Answers to the three questions it was to settle:

1. **Parsing the body as `program_seq` works** — `create_map_stmt` uses `inner_statement`
   (`crates/parser/src/grammar.rs:2445`), and `program_seq` (`:3576`) is the sole `let_binding` call
   site, so the change is one word. `body_to_json` (`:2048`) is a plain serde serialization, so a
   `Statement::Let` round-trips unchanged.
2. **Per-row evaluation exists, but the lookup cannot use it.** `eval_map_body`
   (`crates/exec/src/declared.rs:518`) lowers one `VALUES` expression to a per-row scalar and runs it
   through `eval_value` — a **pure evaluator with no wire access**, as the function's own doc states
   ("Purity holds: … the caller's confined applier performs the I/O at COMMIT").
3. **The declared write path does NOT walk a `Let` node** — `declared.rs:534` requires
   `Statement::Effect` on the first line and rejects anything else.

**The finding that corrects this mission's own framing:** the sentence above calling the gap "a
parser wiring decision, not a limit of the language" is true of the parser and **false of the
runtime**. The map body is pure by contract and a name lookup is I/O. A third run that flipped the
parser and stopped would have hit the same wall one layer deeper, for the third time.

So the ruling settles two axes, not one:

- **Spelling — `let` in the map body**, as recommended above, with the source confined to the
  driver's own declared surface the way G4 confines fan-out targets. Chosen because it adds no
  vocabulary and reuses the already-declared `/slack/{ws}/channels` view, whose cursor paging and
  `OF slack/channel` contract then apply without restatement. The `LOOKUP` stage and the
  `FOLLOW … INTO` selector are both declined.
- **Where it runs — COMMIT, in the confined applier** (`crates/qfs/src/apply_facets.rs:69`, already
  `async`, already the confinement boundary), immediately before the effect leg. This is the moment
  the compiled oracle resolves, so equivalence is provable on the shared fixtures rather than merely
  asserted. `eval_map_body` stays pure: resolved values are bound as extra columns before the per-row
  evaluator runs.
- **PREVIEW additionally refuses a malformed reference** — one that is neither a legal name nor a
  legal id shape — with no I/O.

**Resolving at PREVIEW is deliberately NOT taken, and is deferred to its own mission.**
`crates/exec/src/lib.rs:331` records that "PREVIEW structurally cannot reach the executor"; making it
resolve would make preview perform a network read for every name-addressed write — a product-wide
re-ruling of what PREVIEW means, which must land on the compiled side simultaneously or the twin is
not a twin.

**A consequence to carry into the work:** ticket `20260724014100` stated the contract to reproduce as
"unresolvable names fail at PREVIEW time". The compiled driver resolves at commit
(`driver-slack/src/path.rs:45`, `:66`) and PREVIEW prints the **unresolved** `#general`
(`path.rs:54`), so that bar was unprovable — the equivalence tests could not both pass and prove
equivalence. Its wording is corrected to "before the effect leg fires" as part of this ruling.

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
- [x] A declared write can produce a form-urlencoded body, the shipped Chatwork INSERT commits against the live API, and the cookbook teaches the working statement (#20260727214856-declared-rest-drivers-cannot-post-form-encoded-bodies.md) — **fully met 2026-08-01: hermetic on branch `work-20260801-044839`, then confirmed live in an attended round (message `2135330710482190336` delivered and read back byte-intact)**

## Changelog

- 2026-07-27 — mission created by carrying `the-declared-slack-twin-retires-the-compiled-driver`; two unmet criteria inherited — mission.md
- 2026-07-27 — reframed from the predecessor's inherited body: the wall is a reverse lookup, not per-row fan-out, and `let` already expresses it — the gap is that a map body parses as `inner_statement`, which excludes `let_binding` — mission.md
- 2026-07-27 — strategy linked — integrations-are-declared-not-compiled
- 2026-07-27 — `drive_authorized` deliberately left unset: the spelling ruling above is a genuine design fork and the three unverified spike questions must be answered before a ticket set can be written — mission.md
- 2026-07-28 — ticket added - routed from the root queue by developer ruling 2026-07-28; the mission's Goal widened from name resolution to the general property that a declared write can express what the API requires — 20260727214856-declared-rest-drivers-cannot-post-form-encoded-bodies.md
- 2026-08-01 — **spike run, ruling made, mission approved and `drive_authorized` stamped.** The three
  questions this Scope held open were answered by reading the source at 4b485df: parsing the map body
  as `program_seq` is a one-word change and a stored `Let` round-trips through the plain-serde
  `body_to_json`; per-row evaluation already exists but runs on a **pure** evaluator with no wire
  access; and the write path rejects a `Let` node on its first line. The load-bearing finding is that
  this mission's own framing was incomplete — the map body is pure by contract and a name lookup is
  I/O, so the gap is a runtime one, not only a parser one, and a third run that flipped the parser
  alone would have failed the same way one layer deeper. Ruled: `let` in the map body as the
  spelling, COMMIT-time resolution in the confined applier as the site, a preview-time shape check on
  top, and resolving at PREVIEW deferred to its own mission because it re-rules what PREVIEW means
  product-wide. Ticket 20260724014100's unprovable "fail at PREVIEW" bar corrected to "before the
  effect leg fires". Six tickets bound to the mission — design-brief-reverse-lookup.md
- 2026-08-01 — ticket archived — 20260727214856-declared-rest-drivers-cannot-post-form-encoded-bodies.md
- 2026-08-01 — **the form-codec axis landed hermetically, and its acceptance criterion is deliberately
  left UNCHECKED.** `ENCODE form` exists, the applier picks it, the shipped `chatwork.qfs` message map
  and the cookbook statement both use it, and an end-to-end test drives the exact cookbook `INSERT`
  through the full commit stack asserting the `application/x-www-form-urlencoded` POST body. What is
  NOT done is the one thing the criterion also names: the live commit appearing in the room. This
  Scope reserved that as the attended item, so the box stays open rather than claiming a live
  confirmation that did not happen. The ticket's related-observation half split in two: the `204 No
  Content` read decoding as zero rows landed here, and the `force=1` question — whether the shipped
  messages view should read latest rather than unread-only — is a declaration-semantics ruling and was
  filed as its own ticket. Remaining on the mission: the two Slack-twin criteria and this live round —
  20260801061500-chatwork-messages-view-returns-unread-only.md
- 2026-08-01 — story reported — work-20260801-044839.md
- 2026-08-01 — **the attended live round ran, and the form-codec criterion is now fully met.** With the
  developer present, the updated declaration was installed locally and
  `insert into /chatwork/rooms/25496268/messages values (body) ('qfs v0.0.93 ENCODE form 動作確認 ✅ a&b=c')`
  committed against the live API — `committed: true`, no 400 — into the operator's own My Chat, the one
  room no one else can see. Reading the room back returned the body **byte-intact**: Japanese, the
  emoji, `&`, `=` and the spaces all survived the percent-encode/decode round trip (message
  `2135330710482190336`). The `204` fix was confirmed in the same round by an A/B against the released
  binary: a second read of the same view answers 0 rows on `0.0.93` and `invalid_path … http_decode` on
  the shipped `0.0.80`. The blueprint §13 counter-example this mission carried is closed on the form
  axis: a form-parameter REST API is now writable through a declaration, proven on the wire and not
  only on fixtures. Remaining: the two Slack-twin criteria — mission.md
- 2026-08-03 — concern deferred (stuck) — 20260803210019-the-shipped-chatwork-messages-view-still.md
- 2026-08-03 — concern deferred (stuck) — 20260803210019-an-archived-ticket-auto-ticks-a.md
