---
type: Mission
title: The declared slack twin retires the compiled driver
slug: the-declared-slack-twin-retires-the-compiled-driver
status: active
created_at: 2026-07-24T01:10:45+09:00
author: a@qmu.jp
assignee: a@qmu.jp
strategy: integrations-are-declared-not-compiled
drive_authorized: true
predicted_hours:
actual_hours: 1.05
tickets: []
stories: []
concerns: []
gate_type:
gate_target:
gate_assert:
---

# The declared slack twin retires the compiled driver

## Goal

The DSL mission (`the-declared-driver-dsl-covers-the-compiled-drivers-concisely`, achieved
2026-07-24, shipped in v0.0.85) ruled every semantic gap the conversions need (blueprint §13.1
G1-G8, all landed), calibrated the conciseness bar (§13.2: a tier-1/2 service = one screen,
chatwork.qfs = 32 statement-lines), and wrote the conversion playbook (§13.3): four twin missions
in ascending quirk difficulty, **slack first**. This mission IS playbook entry #1. Its entry
conditions are met: G1 (read-over-POST, needed for the DM `conversations.open` shape) shipped in
v0.0.85; G2 (declared PUSHDOWN `oldest`/`latest`/`limit`) and G5 (typed CALL signatures) are
ruled.

The strategy stake: "declared, not compiled" stays a slogan until a compiled service driver
actually falls. Slack is the chosen first fall because its quirks are the mildest, and the
recently-shipped compiled-driver fixes (v0.0.89: channel-id resolution on every write, user-token
DMs via conversations.open) define exactly the behavior the twin must reproduce. The playbook
promises a fresh session needs to re-derive nothing: §13.1 has the rulings, §13.2 the bar,
§13.3 the row-equivalence bar and the shared retirement steps.

Also in scope, per §13.3's honest-tiering table: the **`/cf` queue-pull twin** — the one exception
whose reason is "not yet done", not "cannot be done" (G1 removed its wall). Retiring it keeps the
tiering table honest while the same declared-twin muscle is warm.

## Scope

**Done when** every acceptance item below is ticked: a committed `slack.qfs` declaration reads
row-equivalent to `driver-slack` on the shared fixtures, its post/CALL maps are effect-equivalent,
the compiled `driver-slack` crate is deleted per the shared retirement steps (docs and skills
regenerated, plugin minor-bumped), and the `/cf` compiled queue-pull is likewise twinned and
retired.

**Out of scope — do not do these in passing:**

- **The github/drive/mail twins** — playbook entries #2-#4, their own missions. In particular the
  Drive id-lookup work ruled over from the predicate-honesty mission belongs to the drive twin.
- **G7 blob-namespace ergonomics and G8 non-REST arms** — parked by §13.1; the twin exposes ops as
  views/maps, never a shell archetype.
- **Slack Verb::Rm advertising** (tracked concern `slack-workspace-namespace-still-advertises-verb`)
  — resolve it BY the conversion only if the declared twin naturally states the honest verb set;
  do not widen into new file-delete features.
- **Live Slack verification** — equivalence is proven on hermetic shared fixtures; live rounds are
  the developer's attended business per the standing live-rounds discipline.

## Experience

1. **One screen declares Slack.** A committed `slack.qfs` (target: under the ~40 statement-line
   bar) declares the read surface — channels, messages (with `oldest`/`latest`/`limit` pushdown
   per G2), threads, reactions, files, users, and the DM read via the G1 `|> POST` stage over
   `conversations.open` — and the post map, connected to the same stored token the compiled
   driver uses.
2. **The twin is row-equivalent before anything is deleted.** On the shared message / thread /
   reaction / file / user fixtures, the declared reads return the same rows as `driver-slack`;
   the post map and the five CALL maps (react / pin / unpin / update / delete, typed per G5) are
   effect-equivalent. The ratchet: compiled stays until the gate is green, then compiled goes.
3. **The retirement is complete, not partial.** `driver-slack` crate + its
   `compiled_describe_registry` entry deleted; `gen-docs` drops `/slack`'s compiled entry from
   `docs/drivers.md`; `gen-skills` re-renders any cookbook recipe that taught the compiled path;
   the qfs plugin takes a MINOR bump across all four version fields (taught-surface break); the
   binary takes its patch bump.
4. **The `/cf` queue-pull follows the same arc** — declared twin over the G1 POST shape, hermetic
   equivalence on the wire fixture, compiled queue-pull deleted, honest-tiering table updated to
   record the exception as closed.

## Acceptance

- [x] slack.qfs declares the read surface + post map within the conciseness bar and reads row-equivalent to driver-slack on the shared hermetic fixtures, DM read included (#20260724014000-declare-the-slack-twin-and-prove-read-equivalence.md)
- [ ] The five typed CALL maps are effect-equivalent to the compiled CALLs on fixtures (#20260724014100-slack-call-maps-effect-equivalent.md)
- [ ] driver-slack is deleted per the shared retirement steps with docs/skills regenerated and the plugin minor-bumped in all four fields (#20260724014200-retire-the-compiled-slack-driver.md)
- [x] The /cf compiled queue-pull is twinned, proven equivalent on the wire fixture, and deleted; the honest-tiering table records the exception closed (#20260724014300-cf-queue-pull-twin-and-retirement.md)

## Changelog

- 2026-07-24 - mission created from the bare /mission planning session (playbook §13.3 entry #1; strategy gap after the DSL mission closed) - mission.md
- 2026-07-24 - strategy linked - integrations-are-declared-not-compiled
- 2026-07-24 - ruling recorded: /cf queue-pull retirement included as the recorded mechanical follow-up; github/drive/mail twins stay separate missions - mission.md
- 2026-07-24 - ticket added - 20260724014000-declare-the-slack-twin-and-prove-read-equivalence.md
- 2026-07-24 - ticket added - 20260724014100-slack-call-maps-effect-equivalent.md
- 2026-07-24 - ticket added - 20260724014200-retire-the-compiled-slack-driver.md
- 2026-07-24 - ticket added - 20260724014300-cf-queue-pull-twin-and-retirement.md
- 2026-07-24 - drive_authorized stamped after the creation interrogation (playbook-grounded set; per-ticket Policies and Quality Gate pre-answered) - mission.md
- 2026-07-25 — ticket archived — 20260724014300-cf-queue-pull-twin-and-retirement.md
- 2026-07-25 — ticket archived — 20260724014000-declare-the-slack-twin-and-prove-read-equivalence.md
- 2026-07-25 — run recorded (+1.05h) — 20260725-101714
