---
created_at: 2026-07-24T01:40:00+09:00
author: a@qmu.jp
type: enhancement
layer: [Domain]
effort:
commit_hash:
category: Added
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
