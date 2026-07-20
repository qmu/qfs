---
type: Mission
title: The declared driver DSL covers the compiled drivers concisely
slug: the-declared-driver-dsl-covers-the-compiled-drivers-concisely
status: active
created_at: 2026-07-20T15:12:33+09:00
author: a@qmu.jp
assignee: a@qmu.jp
drive_authorized:
tickets: []
stories: []
concerns: []
gate_type:
gate_target: blueprint §13 + a conversion playbook
gate_assert: North star, not a machine check — for every surface the compiled slack/github/gdrive/gmail drivers expose, the declared DSL either expresses it concisely today, or a recorded runtime-semantic ruling says exactly what changes to make it expressible, or an honest named park says why it stays compiled; and no per-driver twin conversion starts before those rulings land. Verified per ticket, not by reading a page.
---

# The declared driver DSL covers the compiled drivers concisely

## Goal

**Before writing many drivers, design how far one driver declaration can go.** The owner's
direction (2026-07-20): far more services must be supportable than compiled Rust can keep up
with, so compiled drivers convert into **declared drivers written in the qfs query language** —
but first, prepare how *flexibly, concisely, and shortly* a driver (extension) can be written
as a qfs-query declaration. **The existing compiled drivers are the coverage target**: if the
declared DSL can cover them, it can cover other products and services. Where the DSL/runtime is
not powerful enough, **redesign the runtime semantics** — qfs is experimental, there is no
backward-compatibility constraint, and hard breaks are correct.

This mission is the **prerequisite gate** for the per-driver twin conversions. It exists so
that four conversion missions do not each discover the same expressiveness wall and each invent
a local workaround: the wall gets one design answer here, then the conversions are mechanical.

**What is true today** (blueprint §13, verified against the source and the shipped examples):

- The declaration surface is implemented: `CREATE DRIVER` (base URL, `AUTH NONE | BEARER |
  HEADER | ACCOUNT | OAUTH2`, `PAGINATE CURSOR|LINK`), `CREATE TYPE` (the `OF` contract,
  enforced by tier-2 shaping), `CREATE VIEW` with typed `{param}` template segments, `CREATE
  MAP` (verb → wire effect, per-map `IRREVERSIBLE`), stored as `/sys/drivers` rows,
  reconciled by §16, structurally host-confined (load-time AND plan-time; pinned redirects).
- **Tier 2 is the load-bearing rule**: a declared view IS its stored pipeline, evaluated
  through the real planner/engine with the confined wire as the only source — envelope quirks
  are handled by ordinary pipe ops (`EXPAND`, `WHERE`, `EXTEND`), not descriptors.
- Generic primitives shipped beyond plain REST: `|> FOLLOW <field>` (second GET off a
  delivered URL, credential-free — file download), `|> ENCODE multipart` (form upload),
  `CREATE SQL … TABLES(…)` (a declared SQL-dialect arm — the Cloudflare D1 conversion).
- **Real declared twins exist**: `chatwork.qfs` (full tier-1 + file transfer, zero Rust),
  `cloudflare.qfs` (D1 SQL arm + KV/queue-push REST; compiled `/cf` already demoted to a
  minimal queue-pull + Artifacts fallback), `github_account.qfs` (read-only `/ghdecl` slice
  proving `AUTH ACCOUNT`). The Slack twin exercise (2026-07-05) is what forced tier 2.
- **The compiled service drivers that remain** are the coverage bar: **slack, github, gdrive,
  gmail** (all REST-shaped, all convertible in principle), plus the honest structural
  exceptions the ratchet has not reached and this mission must *name rather than hide*:
  `/git` (local repo, not a wire), `/claude` (local on-disk store, no base URL/auth), `/cf`'s
  queue pull (read-over-POST) and Artifacts (a git surface), `/local`/`/s3`-class blob
  primitives, and `/sql` (already declaration-connected; its engine is a primitive).

**Known expressiveness gaps going in** (the inventory ticket verifies, extends, and prices
each — none is pre-solved here):

- **Read-over-POST** — a read whose wire shape is a POST (Cloudflare queue pull; GraphQL;
  most search endpoints with body-carried queries). Plain declared REST has no primitive; this
  is the sharpest known wall and blocks full `/cf` retirement too.
- **Declared pushdown** — declaring which predicates/params push to the wire as query/body
  parameters (Slack `oldest/latest`, Gmail `q=`, GitHub list filters) so a declared twin is
  not pathologically chatty; residual honesty must survive the declaration.
- **Body/MIME assembly** — Gmail send/draft (RFC 2822 + base64url multipart). Multipart form
  encode shipped; MIME assembly did not. Codec, prelude function, or named park — to be ruled.
- **Batch and subrequest shapes** — Gmail batch endpoints; per-row fan-out (following a
  delivered id into a detail GET — today `FOLLOW` is bytes-oriented, single-purpose).
- **Push/watch channels, GraphQL, websockets** — blueprint-named parks; they stay parks
  unless the inventory finds one load-bearing for a coverage-bar driver.
- **Terseness devices** — what makes declarations *short*, not just possible: driver-level
  defaults with per-view override (pagination has this; auth headers/error envelopes may
  want it), shared pipeline fragments (§5.9 pipeline-valued lambdas as the sanctioned
  genericity axis), declared prelude aliases (`SEND`), and `OF`-type inference or shorthand
  where a full `CREATE TYPE` is ceremony without contract value.
- **Non-REST arm** — whether the declared shape grows an arm for non-wire sources (ruled in
  mission `declared-drivers-are-the-normal-way-to-add-a-service` as "reopens if the shape
  grows one"; this mission decides whether it does, or records the honest park).

**The rulings this mission carries going in:**

1. **Coverage bar = the compiled drivers, measured surface by surface.** The inventory
   enumerates every node/verb/`CALL` procedure the compiled slack/github/gdrive/gmail drivers
   expose (from the compiled describe registry that renders `docs/drivers.md` —
   connection-free by design) and disposes each into: *expressible today* (with the
   declaration text), *needs a ruled semantic* (with the ruling), or *named park* (with why
   waiting is honest).
2. **Redesign is sanctioned, hedging is not.** A gap closes by changing grammar/evaluator
   semantics where that is the clean answer — redefinition, not migration, per the
   experimental-software policy. No deprecation shims, no compatibility framing.
3. **Conciseness is a measured property, not a vibe.** The design fixes a stated bar (the
   chatwork proof: a full tier-1 service with file transfer is ~30 statement lines) and every
   new semantic states its declaration cost — a device that makes declarations longer than
   the compiled driver's docs is the wrong device.
4. **Twin conversions are downstream, named, and not created yet**: `slack` → `github` →
   `drive` → `mail`, in that order (ascending service-quirk difficulty; Gmail last because
   the MIME/batch rulings must exist first). Each becomes its own mission only after this
   mission's rulings land in blueprint §13. The §13 twin-and-retire ratchet governs each:
   compiled stays until the twin is row-equivalent on shared fixtures, then is deleted.
5. **Shared semantics live once.** Per-row codec application and multi-relation codecs are
   owned by the sibling mission `a-file-collection-is-a-declared-set-over-any-blob-source`;
   this mission consumes those rulings (a declared driver decoding a collected response set
   rides the same rule) and must not fork them.

## Scope

**Done when** every acceptance item below is ticked: the gap inventory is complete against the
compiled surfaces, every gap has a recorded disposition (expressible / ruled semantic / named
park) in blueprint §13, at least one new ruled semantic is proven hermetically end-to-end
through a declared driver, the conciseness bar is stated, and the conversion playbook names
the four downstream missions with their entry conditions.

**Out of scope** (deliberately):

- **The per-driver twin conversions themselves** (slack/github/drive/mail) — downstream
  missions, created after the rulings land, not before.
- **Live credentialed verification** — owner-attended, per the standing live-round backlog
  convention; everything here lands hermetic-first.
- **The collection/codec semantics** owned by the sibling mission (ruling 5).
- **New services for their own sake** — a service enters only as proof a ruling works
  (the chatwork/cloudflare pattern).
- **`/git`, `/claude`, blob primitives, `/sql` engines** — structural exceptions stay
  compiled under the honest tiering unless the non-REST-arm ruling says otherwise; converting
  them is not this mission even then.

## Experience

- An agent (or human) writing an integration for a new REST service reads one skill and writes
  one `.qfs` file: driver + types + views + maps, typically one screen, never Rust. Where the
  service has a quirk (POST-read, odd envelope, pushable search params), the DSL has a
  declared spelling for it — the author never falls back to "this needs a compiled driver"
  for anything tier-1/tier-2 shaped.
- A reviewer reads the declaration and sees the whole integration: hosts confined, secrets
  referenced never inlined, irreversibility declared per map, pushdown declared and honest.
- The design deliverables are readable artifacts: blueprint §13 carries the rulings; a
  conversion playbook (in the blueprint or beside it) tells the next four missions exactly
  what to do, in what order, and what "done" means (row-equivalence on shared fixtures, then
  compiled deletion, docs/skills regeneration, plugin version bump).
- The expressiveness wall, wherever it genuinely remains, is a *named* park with a reason —
  never a silent workaround inside one driver's declaration.

## Acceptance

- [ ] **The coverage inventory exists and is verified.** Every node, verb, and `CALL`
      procedure the compiled slack/github/gdrive/gmail drivers expose is enumerated from the
      compiled describe registry (the `gen-docs` source, not prose) and disposed as
      *expressible today* / *needs a ruled semantic* / *named park* — each disposition with
      evidence (a working declaration snippet, or the concrete missing primitive).
- [ ] **Every "needs a ruled semantic" gap has a recorded ruling in blueprint §13** —
      grammar/evaluator redesigns stated as redefinitions (hard breaks sanctioned, no
      migration framing), each with its declaration-cost note; read-over-POST and declared
      pushdown are ruled explicitly (they gate the slack/github conversions and full `/cf`
      retirement); the non-REST-arm question is ruled or parked with a reason.
- [ ] **At least one ruled semantic ships and is proven hermetically end-to-end**: a declared
      driver using the new semantic installs, describes, plans, and reads/writes against
      hermetic wire fixtures through the real tier-2 evaluator — the proof pattern
      `cloudflare.qfs`'s SQL arm set — so the rulings are demonstrated, not speculative.
- [ ] **The conciseness bar is stated and measured.** The blueprint records the target (a
      tier-1/tier-2 REST service ≈ one screen of statements, chatwork as the calibration
      point) and the inventory's *expressible today* dispositions are measured against it;
      terseness devices adopted (defaults, shared fragments, prelude aliases, type shorthand)
      each show a before/after on a real declaration.
- [ ] **The conversion playbook exists and names the downstream missions**: `slack` →
      `github` → `drive` → `mail`, each with its entry condition (which rulings it needs
      landed), its fixture/row-equivalence bar, and its retirement steps (compiled deletion,
      gen-docs/gen-skills regeneration, plugin version bump per CLAUDE.md). The playbook
      states plainly that none of the four starts before this mission's rulings land.
- [ ] **Honest tiering is restated, not eroded**: `/git`, `/claude`, queue-pull/Artifacts (as
      far as still compiled), blob primitives, and `/sql` engines are recorded as named
      structural exceptions with reasons, so "declared is the normal way" keeps its honest
      boundary and no silent exception rides the conversions.

## Changelog

- 2026-07-20 — Mission placed by the design session (owner strategic direction, 2026-07-20:
  prepare declared-driver DSL expressiveness — flexibly, concisely, shortly — with the
  existing drivers as the coverage target, before the per-driver conversions; runtime-semantic
  redesign sanctioned, no backward compatibility). The parallel/preparatory track — qfs-viewer
  stays priority #1 and is untouched. Grounded in blueprint §13 (tiered model, tier-2 rule,
  ratchet), the shipped declared twins (`chatwork.qfs`, `cloudflare.qfs`,
  `github_account.qfs`), and the declared-drivers mission's Item-2 record (queue-pull/
  Artifacts as the live example of a structural gap). Downstream twin-conversion missions
  named (slack → github → drive → mail) but deliberately NOT created — this mission is their
  gate. Rulings 1-5 in ## Goal are the design session's judgment, recorded for the driving
  session to execute or overturn with cause. No tickets cut yet; `drive_authorized`
  deliberately left empty (no per-ticket interrogation has happened).
