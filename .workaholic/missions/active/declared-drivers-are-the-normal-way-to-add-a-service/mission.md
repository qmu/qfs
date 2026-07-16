---
type: Mission
title: Declared drivers are the normal way to add a service
slug: declared-drivers-are-the-normal-way-to-add-a-service
status: active
created_at: 2026-07-15T20:55:00+09:00
author: a@qmu.jp
assignee: a@qmu.jp
tickets: []
stories: []
concerns: []
gate_type: documentation
gate_target: /guide/connect
gate_assert: Every service on the page connects through a committed declaration with a referenced secret — no qfs account add prerequisite for cloud, and no QFS_* env var presented as a working path.
---

# Declared drivers are the normal way to add a service

## Goal

Adding a service to qfs should be a **reviewable qfs declaration you commit to a repo**, never a
compiled-Rust driver and never a name-shaped environment variable. A connection is a statement you
can read, review, and check in; secrets are *referenced*, never inlined; and re-reading the
declaration is what heals state.

This mission is **framed as a standing property of the product, not an episode of work** — see the
2026-07-15 reframing. It exists because the property is *half-true today*, and because the previous
mission that carried it (`qfs-capability-tryout-…`, goal #2 "less platform, more language: push
drivers out of compiled Rust into qfs-query declarations") was archived `achieved` while this part
of its goal was still unfinished. Seven open concerns were left with no live home; they are adopted
here.

**What is true today.** `CREATE CONNECTION <name> DRIVER <driver> [AT '<locator>'] [SECRET '<ref>']`
parses; declared `sqlite|postgres|mysql` mount `/sql/<name>` and declared `git` mounts `/git/<name>`
with no env var; SQL passwords resolve lazily from `env:`/`vault:` references; the `QFS_SQL_*` /
`QFS_GIT_*` env vars are a warned, deprecated fallback with a `--import-env` migration.

**What is not true yet**, each item traceable to a concern or to `docs/roadmap.md`:

1. **Cloud accounts are outside the declaration surface.** They still need `qfs account add` +
   `qfs connect`; the roadmap carries cloud account declarations as 🧭 proposed. The `SECRET '<ref>'`
   clause on `CREATE ACCOUNT` is deliberately unimplemented because there is no bind-time
   `env:`/`vault:` resolution for accounts (unlike a mount's `CONNECT … SECRET`) — a parse-only
   clause would be a surface that cannot resolve, which "docs true / no fake success" forbids.
2. **`/cf`'s per-resource config lives in compiled Rust, not in a declaration.** (Corrected
   2026-07-16 against the source; the original text called `/cf` and `/rest` "cred-free
   placeholder" mounts and was stale on both counts.) REST is **done**: a declared driver's
   `resources` are derived from its view/map nodes and lifted onto the wire engine by
   `declared_driver.rs:233 rest_config()`, so `/chatwork` and `/cloudflare` already come from
   committed declarations. `/cf` is the opposite: it is a **live compiled driver** — registered at
   `cloud_mounts.rs:53`, resolving its token from the vault at `cf.rs:86` and requiring
   `qfs account add cf <conn>` — whose D1/KV/queues surface comes from compiled introspection
   (`cf.rs:309 introspect_d1`). It is not a placeholder, and that is exactly the problem: it is the
   mission's counter-example, a working service that is reachable *because* it was compiled in.
3. **`sql`/`git` never moved onto the `path_binding` registry** — they still ride the older
   declared-connection seam (a documented CONNECT-epic follow-up), and NUMERIC/TIMESTAMP/UUID/JSON
   column round-trips are not covered.
4. **Re-installing a declaration does not heal it.** Repeated `qfs run -f <driver>.qfs` *appends*
   `sys_drivers` rows. Only the `type` lookup went newest-wins; duplicate `driver` and `view`/`map`
   rows still resolve **oldest-first**, so a re-install silently keeps the stale row.
5. **Three `.qfs` readers disagree on the same text, and none matches the lexer.** The server
   config/job loader (`server/src/runtime.rs:533 strip_line_comment`) cuts at the first `--` on a
   line with no quote or token awareness, and splits on any `;` (`:503`) the same way; it is shared
   with the provisioning desired-state loader (`provision/src/load.rs:108`), so a `--`-bearing path
   breaks the source-of-truth document too. `core/src/ddl/connections.rs:65 split_statements` is
   quote-aware but escape-blind, token-blind, `#`-blind and line-number-less. Only
   `lang/src/lex.rs` is token-accurate. `statements()`'s own doc comment (`runtime.rs:491-493`)
   asserts "the reconcile loop never forks a second, drifting statement chunker" while
   `split_statements` is exactly that fork — and the correct one on the quote axis, the broken one
   on `#` and line numbers. (Corrected 2026-07-16: the original text blamed "the declaration file's
   own parser" and a later draft over-credited `connections.rs` as simply "correct". Both were
   wrong; the defect is the fork itself.)
6. **Config writes are not uniformly events.** System DB-backed writes append DDL events
   transactionally; Project DB-backed `path_binding` / account-consent state cannot share that
   transaction boundary, so those configuration events never reach the DDL event log.

## Scope

**Done when** every acceptance item below is ticked: a cloud account is reachable from a committed
declaration with a referenced secret, the declaration shape carries what `/cf` and `/rest` need,
`sql`/`git` ride `path_binding`, a re-install heals every declaration row kind, the config parser
stops truncating, and Project DB config writes are events like every other config write.

**Out of scope:**

- New service integrations for their own sake — a driver enters only as proof the declared model
  covers its shape.
- The live credentialed rounds these changes eventually need. Live verification is owner-attended
  and tracked as its own mission-free backlog (2026-07-15 reframing); this mission lands
  hermetic-first and hands each round over.
- `CREATE AGENT` / principal semantics — a separate mission.

## Acceptance

- [ ] **Cloud account declarations ship.** A cloud mount comes from a committed declaration with a
      referenced secret (no `qfs account add` prerequisite); this includes deciding and implementing
      the `CREATE ACCOUNT … SECRET '<ref>'` edge together with the bind-time account-reference
      resolution it needs. `docs/roadmap.md` flips 🧭 → ✅ (concern
      `create-account-ships-the-core-two`, rescoped to the SECRET edge on 2026-07-15)
- [ ] **`/cf`'s D1 / KV / queues surface comes from a committed declaration**, not from compiled
      introspection — the declaration shape carries what a per-resource cloud driver needs, and the
      compiled `/cf` driver stops being the way that service is reached (concern
      `cf-live-203090-unimplemented-cf-and`, rescoped 2026-07-16; its live round hands over to the
      live backlog). REST resource maps are **already declared** via the view/map → `resources` lift
      (`declared_driver.rs:233`) and are not part of this item.
- [ ] **`sql`/`git` move onto the `path_binding` registry**, and declared-path column-type coverage
      broadens to NUMERIC / TIMESTAMP / UUID / JSON round-trips
      (concern `postgres-mysql-declarations-for-the-declared`)
- [ ] **A re-install heals every declaration row kind** — `driver`, `view`, and `map` lookups get the
      same replace-on-install (preferred) or newest-wins semantic the `type` lookup already has, so
      re-running a declaration file is idempotent rather than append-only
      (concern `duplicate-declaration-rows-still-resolve-oldest`)
- [x] **The `.qfs` config document gets one correct statement splitter** — `--` and `#` start a
      comment only at a token boundary, `'…'` (with `\'` escapes) is opaque, and a `/`-led path
      token consumes to a real delimiter, so neither `--` nor `;` inside a path or locator
      truncates or splits. Today three readers disagree on the same text: the server config/job
      loader (`server/src/runtime.rs:533`, shared with the provisioning loader at
      `provision/src/load.rs:108`) truncates at the first `--` and splits at any `;`;
      `core/src/ddl/connections.rs:65` is quote-aware but escape-blind, token-blind (an **unquoted**
      `/local/a--b.txt` is still cut, though `lang/src/lex.rs:659 is_path_delimiter` excludes `-`,
      making it a legal path), `#`-blind, and line-number-less. `lang/src/lex.rs:156-173,254-296`
      is the only token-accurate authority and is the semantics to mirror. (Corrected 2026-07-16:
      an earlier draft of this item called `connections.rs:65` "the correct implementation" and
      told a driver to reuse it — reusing it verbatim would break every `#`-only fixture and every
      line-located error.)
      (concern `the-config-comment-stripper-truncates-paths`; retargeted 2026-07-16 — **this item**
      named the wrong parser. The concern itself says "the `.qfs` config statement splitter", which
      is right; it also records the repro this item lacked: two `qfs` job tests fail whenever
      `$TMPDIR` contains `--`, green under a clean TMPDIR and in CI.)
- [ ] **Project DB configuration writes append DDL events** with the same secret-redaction and
      hash-chain discipline as System DB writes — via a Project DB event writer or an explicit
      cross-store event envelope (concern `project-db-configuration-events-are-not`)
- [ ] **The declared-secrets adapter carries the OAuth app**, closing the declared-model follow-up
      left by the capability-tryout mission (concern `declared-model-and-scheduling-follow-ups`; its
      Chatwork live-encoding and Slack-threading remainders hand over to the live backlog)

## Changelog

- 2026-07-15 — Mission created by the missions/tickets reframing (owner-approved). Framed as a
  standing product property rather than an activity. Adopted the seven open concerns that the
  archived `qfs-capability-tryout-…` mission's unfinished goal #2 had orphaned, plus the roadmap's
  🧭 cloud-account-declaration gap. No implementation yet; acceptance derived from the concerns'
  recorded findings, not re-litigated.
- 2026-07-16 — Acceptance re-litigated against the source before cutting the first ticket, which is
  what the 2026-07-15 entry above had deliberately skipped. Two items were wrong and would have sent
  a driver at the wrong file:
  - **Item 5 named the wrong parser.** `connections.qfs`'s own splitter is already quote-aware with
    a regression test (`core/src/ddl/connections.rs:65`); the truncation is in the server config/job
    loader (`server/src/runtime.rs:533`), which also feeds the provisioning loader
    (`provision/src/load.rs:108`). Read as written, the item was satisfiable by a correct file. The
    concern was not at fault here — it says "the `.qfs` config statement splitter", which points the
    right way, and it carries a repro the item had dropped (`$TMPDIR` containing `--` fails two job
    tests). Only this mission's paraphrase went wrong.
  - **Item 2 was stale in both directions.** `/cf` is not a cred-free placeholder — it is a live
    compiled driver requiring `qfs account add` (`cf.rs:86`, `cloud_mounts.rs:53`), i.e. the
    mission's counter-example rather than an unfinished mount. REST's per-resource declaration is
    already shipped via the view/map → `resources` lift (`declared_driver.rs:233`). Rescoped to
    `/cf` alone.
  Items 1, 3, 4, 6 and 7 were each re-checked in the source and stand as written (the asymmetry in
  item 4 is visible at `declared_driver.rs:125` vs `:532`/`:586`). Gate fields and assignee added
  the same day; `gate.sh` reports the gate valid, with empty ports because this mission has no
  worktree.
- 2026-07-16 — Item 5 corrected a **second** time, during its own `/ticket` discovery. The first
  correction over-swung: having found that `connections.rs:65` was not the defect, it recorded that
  splitter as "the correct implementation" and pointed the fix at reusing it. Verification against
  the source showed it is correct only on the quote axis — it is escape-blind, token-blind (an
  **unquoted** `/local/a--b.txt` is truncated, though `lex.rs:659 is_path_delimiter` excludes `-`,
  and bare paths are how every fixture is written), `#`-blind (every server/provision fixture uses
  `#` exclusively), and line-number-less (`LoadError::Parse{line}` depends on the attribution).
  Reusing it verbatim would have broken every boot fixture. The item is re-framed around the real
  defect — three `.qfs` readers disagreeing, none matching `lang/src/lex.rs` — and now also carries
  the `;`-in-quote and trailing-`#` defects of the same class, both reproduced through the shipped
  binary during discovery. Recorded because the same item has now been mis-stated twice in two days
  by paraphrase, in both directions.
- 2026-07-16 — **Item 5 done** (`0afaf2b`, ticket `20260716005029-unify-the-qfs-statement-splitter.md`).
  One splitter in `core/src/ddl/document.rs` runs the lexer and splits on the `Token::Semicolon`s it
  emits; boot, the provisioning loader and `parse_connections` all call it, and both hand-rolled
  scanners are deleted. The implementation overturned the ticket's own plan twice, so the record is
  worth keeping: (1) the ticket ruled a lexer-based splitter out of scope because `qfs-server` and
  `qfs-provision` lack a `qfs-lang` dep — irrelevant, since the splitter lives in `qfs-core`, which
  **already** has that dep; (2) a hand-rolled scanner cannot mirror `lex.rs` "exactly" at all,
  because `slash_starts_path` consults the preceding token stream and a **private** keyword table —
  imitating it means writing a third lexer, the very fork the ticket exists to close.
  The root cause turned out to be one line: `is_path_delimiter` (`lex.rs:659`) omitted `;`, so a
  path swallowed the `;` glued to its right. That is why the splitter could not use the lexer — the
  terminator it needed to find never became a token — **and it was a shipped language bug**:
  `transaction { … |> insert into /a/b; … }`, exactly as `docs/language.md:113` documents it, raised
  a parse error, while the same text with one space before the `;` parsed. Adding `;` to the
  delimiter set (owner-approved hard break; a bare path can no longer carry a literal `;`, joining
  the `#` and `,` already there) fixed the language bug, the config splitter, and the `;`-in-quote
  and trailing-`#` defects at once. Verified in both directions: with the fix stashed and `$TMPDIR`
  carrying `--`, exactly the two job tests the concern named fail; with it restored the `qfs` crate
  passes 368 under the same TMPDIR.
- 2026-07-16 — ticket archived — 20260716005029-unify-the-qfs-statement-splitter.md
