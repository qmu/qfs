---
created_at: 2026-07-25T12:44:00+09:00
author: a@qmu.jp
type: enhancement
layer: [Domain]
effort:
commit_hash:
category: Added
depends_on:
mission: the-declared-slack-twin-retires-the-compiled-driver
---

# Declared per-row fan-out: `|> FOLLOW <field> INTO /http/<drv>/<template>` (blueprint §13.1 G4)

## Overview

Blueprint §13.1 **G4** — a declared body may fan a delivered field out into a SECOND wire request per
row — is **ruled but unimplemented**. Today `FOLLOW <field>` exists only in its single-row download
form (`qfs_exec::declared::eval_view_body`'s follow stage takes exactly ONE delivered row and fetches
its URL as raw bytes). There is no form that takes a delivered *value* and substitutes it into a
declared endpoint template, per row.

That gap is the exact wall the declared Slack twin hit
(ticket `20260724014100-slack-call-maps-effect-equivalent.md`, Quality Gate item 2 — still open):
the compiled `driver-slack` resolves a `#name` channel to its `Cxxxx` id (and a `Uxxxx` to its
`Dxxxx` IM channel) INSIDE its live client, on the way to every ID-requiring call. A declaration
cannot express that lookup, so a declared CALL can only accept an already-resolved id — neither
"a name-addressed channel resolves before the effect fires" nor "an unresolvable name is a structured
preview-time error" is reproducible. Playbook §13.3's remaining twins (github / drive / mail) need
the same primitive: the Drive id-lookup work ruled over from the predicate-honesty mission is the
same shape.

This ticket is the shared prerequisite, minted so the wall is queued work rather than a note inside a
blocked ticket. It does NOT add an acceptance item to the mission; the agreed plan is unchanged.

## Policies

- workaholic:design / 「推測するな、宣言して拒否せよ」 — an unresolvable reference must be refused at
  PREVIEW, as a structured usage error, never guessed and never a garbage id at commit.
- Blueprint §13 confinement — a fan-out target is a `/http/<self>/…` template like every other
  declared wire address; the anti-exfiltration boundary must hold per row, not just for the first
  fetch.
- workaholic:implementation / honest-surfaces — the runaway-fetch guard the cursor pagination already
  has (`MAX 50`) applies here too: a per-row fan-out over N rows is N requests and must be bounded
  and visible, never a silent storm.

## Quality Gate

1. A declared view body carrying `|> FOLLOW <field> INTO /http/<drv>/<template>` issues ONE wire
   request per delivered row, substituting that row's `<field>` into `<template>`, and splices the
   response into the row — proven on a hermetic multi-row fixture with the recorded requests
   asserted.
2. The fan-out target is confined to the driver's own `/http/<name>` namespace, re-checked per row;
   a foreign host is a structured refusal, and a bounded ceiling caps the request count.
3. A value the fan-out cannot resolve is a structured, secret-free error at PREVIEW time — not a
   silent `Null`, not a wire error at commit.
4. The declared Slack twin's channel-name→id resolution is expressed with it, and ticket
   20260724014100's Quality Gate item 2 becomes provable: a name-addressed channel resolves before
   the effect fires, and an unresolvable name is a preview-time refusal — both compared against the
   COMPILED driver on the shared fixtures.
5. Workspace gates green: `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`,
   `cargo fmt --all --check`.

## Considerations

- `crates/exec/src/declared.rs` owns the existing single-row `FOLLOW` (`follow_url`) and is where the
  per-row form belongs; the fetch stays an injected closure so exec keeps no transport.
- The grammar side is a new `INTO <path>` tail on the existing `FOLLOW` stage — check whether the
  single-row form should become the `INTO`-less special case or be retired outright (the project
  takes hard breaks; no compatibility shim).
- Sequencing: this is the blocker for 20260724014100 QG2 and therefore for
  `20260724014200-retire-the-compiled-slack-driver.md`, which cannot delete the compiled oracle while
  an equivalence proof still needs it.
