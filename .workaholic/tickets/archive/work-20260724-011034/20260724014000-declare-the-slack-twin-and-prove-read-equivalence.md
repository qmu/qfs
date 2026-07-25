---
created_at: 2026-07-24T01:40:00+09:00
author: a@qmu.jp
type: enhancement
layer: [Domain]
effort: 4h
commit_hash:
category: Changed
depends_on:
mission: the-declared-slack-twin-retires-the-compiled-driver
---

# Declare the slack twin and prove read equivalence

## Overview

Playbook §13.3 entry #1, first half. Author the committed `slack.qfs` declaration covering the
compiled driver's **read surface** and the **post map**, and prove it row-equivalent to
`driver-slack` on the shared hermetic fixtures.

Read blueprint §13.1 (rulings), §13.2 (conciseness bar + terseness devices, chatwork.qfs as the
calibration artifact), and §13.3 (the slack row: entry conditions, equivalence bar) plus the §13
coverage inventory (`.workaholic/missions/archive/the-declared-driver-dsl-covers-the-compiled-drivers-concisely/inventory-compiled-driver-surfaces.md`
on main) before writing a line — the point of the playbook is that nothing here is re-derived.

Surface to declare (per the inventory's slack rows):

- channels listing; channel messages with **G2 declared PUSHDOWN** for `oldest`/`latest`/`limit`;
  thread replies; reactions; files listing; users — each row-equivalent to the compiled reads.
- the **DM read** via the G1 `|> POST` stage over `conversations.open` (the read-over-POST shape
  shipped in v0.0.85) — matching the compiled user-token DM behavior fixed in v0.0.89.
- the **post map** (message post), effect-equivalent.
- connected to the same stored token/secret binding the compiled driver's CONNECT uses.

## Policies

- workaholic:implementation / honest-surfaces — the declared DESCRIBE must advertise exactly what
  the declaration answers (verbs, pushdown flags); no compiled-era capability may survive as a
  phantom advertisement.
- Blueprint §13 twin-and-retire ratchet — the compiled driver is NOT touched by this ticket;
  equivalence first, deletion later (ticket 20260724014200).
- workaholic:development / hermetic gates — equivalence is proven on wire fixtures, no network,
  no credentials.

## Quality Gate

1. `slack.qfs` is committed and installs via the declared-driver path; its statement-line count
   is measured and recorded in the ticket's final report against the §13.2 bar (~40 lines;
   overrunning the bar requires naming which terseness device was insufficient, per the bar's
   own instruction — not silently accepting bloat).
2. A hermetic row-equivalence test drives BOTH the declared twin and `driver-slack` over the
   shared message/thread/reaction/file/user fixtures and asserts identical rows — including the
   DM read via the POST stage and at least one pushdown case each for `oldest`/`latest`/`limit`
   where the fixture proves the parameter reached the wire request.
3. The post map is effect-equivalent on fixtures (same wire request shape as the compiled post).
4. Workspace gates green: `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`,
   `cargo fmt --all --check`.

## Considerations

- The tier-2 evaluator and its wire-fixture harness from the read-over-POST ship
  (`read_over_post_pulls_rows_through_the_real_evaluator`) are the intended test substrate.
- chatwork.qfs is the calibration artifact for idiom — prefer its declaration style over
  inventing new shapes.

## Final Report

**1. The declaration — `crates/skill/assets/examples/slack_driver.qfs` (exported as
`qfs_skill::SLACK_DRIVER`).** 16 statements, **31 statement-lines** — under the §13.2 bar (~40) and
just one line over the `chatwork.qfs` calibration point (32), for a service with more nodes. No
terseness device was found insufficient. It declares: the driver (bearer auth, Slack's NESTED
`response_metadata.next_cursor` pagination, and the G2 pushdown default), five `OF` types matching
the compiled DTO columns exactly, nine read views (channels, channel messages, thread replies,
reactions, the DM open, the DM message log, workspace files, channel files, users), and the post map.

It could not reuse the `slack.qfs` filename: that asset is the single-statement PREVIEW golden in
`qfs_skill::EXAMPLES`, so the declaration ships beside it as `slack_driver.qfs` (the
`chatwork.qfs`/`cloudflare.qfs` install-program shape).

**2. G2 — declared pushdown, implemented, not just consumed.** The clause did not exist; this ticket
built it end to end:

- **Grammar** — `PUSHDOWN ( <col> <op> => '<param>' [EXACT|PREFILTER], …, LIMIT => '<param>' )` on a
  `CREATE VIEW` (after the body) and on `CREATE DRIVER` as a default, the same
  default-with-per-view-override shape `PAGINATE` has. `PUSHDOWN`/`EXACT`/`PREFILTER` are contextual
  UPPERCASE idents — zero new frozen keywords. Desugars to a JSON descriptor.
- **Storage** — System DB migration **#19** (`system_drivers_pushdown.sql`) appends a nullable
  `pushdown TEXT` column to `sys_drivers`; #14 stays frozen. A pre-G2 row reads back `NULL`, which is
  exactly the honest-but-chatty everything-residual default. The column holds parameter NAMES only —
  the credential-free-script contract is untouched.
- **Lowering** (`qfs_exec::declared`) — `parse_pushdown` + `lower_declared_pushdown` turn a pushed
  predicate/limit into wire query parameters plus a **truthful residual**: EXACT drops the conjunct,
  PREFILTER pushes AND keeps it, an unmatched conjunct (and any OR/NOT/LIKE shape) stays wholly
  residual. This is `driver-slack/pushdown.rs`'s own discipline, read off the declaration instead of
  compiled into Rust. A descriptor missing its `exact` flag is read as the WEAKER claim, so an
  unreadable declaration can never silently drop a predicate.
- **Advertising + facet** — a declared driver whose views carry a map sets
  `RestApiConfig::declared_where_pushdown`, so `RestDriver`'s profile claims `WHERE` and the planner
  hands the predicate to the read facet; the facet lowers it, appends the parameters to the wire
  source, and re-applies the residual locally (the `SqlReadDriver` discipline) before the limit cap.

**3. Row equivalence, on shared hermetic fixtures.** Five new tests drive the declared view and the
compiled `driver-slack` read over the SAME fixture JSON and compare delivered column names + row
values: `slack_twin_message_read_is_row_equivalent`,
`slack_twin_replies_reactions_files_and_users_are_row_equivalent` (three nodes),
`slack_twin_dm_read_rides_the_g1_post_stage` (the G1 `conversations.open` POST read plus the DM
message log), and `slack_twin_declared_pushdown_reaches_the_wire_with_a_truthful_residual`. The
pushdown test asserts each of `oldest`/`latest`/`limit` **provably reached the wire request URL**,
and asserts EXACT-drops-residual vs PREFILTER-keeps-residual directly.
`shipped_slack_script_installs_statement_for_statement` parses every shipped statement, measures the
conciseness bar, and pins host confinement + credential-freedom;
`shipped_slack_script_declares_the_g2_pushdown` proves the descriptor the equivalence tests lower
through is the shipped declaration's own, parsed from the asset bytes.

**4. Honest differences, recorded not papered over.**

- **`created` scale**: the compiled `FileDto` multiplies Slack's seconds by 1000; the declaration
  delivers the field verbatim. The test asserts the *relation* (`Int(1700)` vs
  `Timestamp(1_700_000)`) rather than pretending they match.
- **Two evaluator gaps forced fully-populated fixtures**, and both are now a **newly minted ticket**,
  `20260725103000-declared-expand-must-splice-by-field-name.md`: (a) tier-2 `EXPAND` splices a
  struct's values POSITIONALLY, so a ragged JSON array — exactly what Slack returns, since
  `thread_ts`/`subtype` are optional — shifts every later column (observed: the envelope's own `ok`
  value slid into the `ts` column); (b) the compiled DTOs fold `""` to `Null` while the declaration
  delivers `Text("")`. (a) is a silent wrong-rows defect for any declared driver over an
  optional-field API, so it is filed as a bugfix, not a note.
- **DM addressing**: the compiled driver resolves `U…` → `D…` *inside its live client* on the way to
  `conversations.history` — one path read, two wire calls. The declaration exposes that as two
  addressable nodes (`/slack/{ws}/dms/{user}` opens the IM and returns its id;
  `/slack/{ws}/dms/{channel}/messages` reads it), because chaining them in one view is G4
  (`FOLLOW … INTO`), which is ruled but not implemented. Both nodes are proven row-equivalent;
  the single-address convenience is the recorded difference.
- **The post map's channel** rides the incoming row (`{channel: row.channel, text: row.text}`) rather
  than the path's `{channel}` segment: a map body's `VALUES` expression is row-closed, so path
  `{param}` bindings do not reach it (they do reach the map's wire TARGET). Recorded here; the wire
  body shape is otherwise the compiled post's, minus the compiled-only `client_msg_id` idempotency
  key the pre-existing test already documents.

**Gate:** `cargo test --workspace` green; `cargo clippy --workspace --all-targets -- -D warnings`
green; `cargo fmt --all --check` green; `gen-docs --check` and `gen-skills --check` in sync;
`check-migrations` green (migration #19 is a new append, no shipped body edited).

**Not touched (the ratchet):** `driver-slack` is untouched — equivalence first, deletion in ticket
20260724014200.
