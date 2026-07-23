---
type: Mission
title: Declared drivers are the normal way to add a service
slug: declared-drivers-are-the-normal-way-to-add-a-service
status: achieved
created_at: 2026-07-15T20:55:00+09:00
author: a@qmu.jp
assignee: a@qmu.jp
drive_authorized: false
tickets:
  - 20260718203325-create-account-secret-ref-bind-time-resolution.md
  - 20260718203326-cf-surface-from-committed-declaration.md
  - 20260718203327-sql-git-path-binding-only-and-type-roundtrips.md
  - 20260718203328-declared-secrets-adapter-carries-oauth-app.md
stories: []
concerns: []
gate_type:
gate_target: /guide/connect
gate_assert: North star, not a machine check — every service connects through a committed declaration with a referenced secret, with no qfs account add prerequisite for cloud and no QFS_* env var as a working path. Verified per ticket, not by reading the page.
---

# Declared drivers are the normal way to add a service

## Goal

Adding a service to qfs should be a **reviewable qfs declaration you commit to a repo**, never a
compiled-Rust driver and never a name-shaped environment variable. A connection is a statement you
can read, review, and check in; secrets are *referenced*, never inlined; and re-reading the
declaration is what heals state.

**The "never a compiled-Rust driver" rule is a ratchet, not a partition** (blueprint §13, "the
self-hosting ratchet, honestly tiered": *"Compiled drivers remain until their script twin passes
the conformance suite; then they may be deleted — that ratchet, not rewrites, is the migration
path"*). Two compiled counter-examples exist today, named here so the rule stops reading as
absolute while unnamed exceptions ride it:

- **`/cf`** — REST-shaped, so the ratchet *can* reach it; doing so is this mission's own item 2.
- **`/claude`** — the AI-sessions façade (mission
  `claude-code-sessions-are-queryable-and-steerable-as-qfs-paths`, acceptance item 7, recorded
  2026-07-17). It **mechanically cannot be declared today**: the declared shape is REST-shaped
  (`base_url`/`auth`/`pagination`/`verb`/`body`) and `/claude` has no base URL, no auth, and no
  wire — it reads a local on-disk store. It is not in violation; the ratchet has not reached it.
  Converting it stays out of this mission's scope; if the declared shape ever grows a non-REST
  arm, the question reopens there — it is not pre-ruled here.

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

## Experience

An operator adds any service by committing a declaration and referencing a secret — never by
compiling Rust and never by exporting a `QFS_*` env var:

- **A cloud account is declared, not `account add`-ed.** `CREATE ACCOUNT cf 'mycf' SECRET
  'env:CF_TOKEN'` then `CONNECT /cf TO cf …` binds a live Cloudflare mount with no `qfs account
  add` step; the secret resolves from the reference **at use**, so rotating the env var (or vault
  entry) heals the binding on the next read. An inline secret literal is a parse error.
- **`/cf`'s D1 / KV / queues surface reads from the committed declaration**, not from compiled
  introspection at mount time (a declared sql-resource arm keeps the D1 relational surface); the
  compiled `/cf` driver retires once its declared twin passes the conformance suite (§13 ratchet).
- **`sql`/`git` mounts come only from `path_binding` rows** written by `CONNECT`; the legacy
  `connections.qfs` loader, the `QFS_SQL_*`/`QFS_GIT_*` env fallback, and the `CREATE CONNECTION`
  statement are all gone (hard break). NUMERIC / TIMESTAMP / UUID / JSON columns round-trip through
  a declared `/sql/<conn>`.
- **An OAuth declared driver refreshes its bearer through the mount's app**; declarations still
  carry only selectors, never tokens.

Every change lands hermetic-first; each live credentialed round hands over to the owner-attended
live backlog.

## Acceptance

- [x] **Cloud account declarations ship.** (#20260718203325-create-account-secret-ref-bind-time-resolution.md) A cloud mount comes from a committed declaration with a
      referenced secret (no `qfs account add` prerequisite); this includes deciding and implementing
      the `CREATE ACCOUNT … SECRET '<ref>'` edge together with the bind-time account-reference
      resolution it needs. `docs/roadmap.md` flips 🧭 → ✅ (concern
      `create-account-ships-the-core-two`, rescoped to the SECRET edge on 2026-07-15)
- [x] **`/cf`'s D1 / KV / queues surface comes from a committed declaration** (#20260718203326-cf-surface-from-committed-declaration.md), not from compiled
      introspection — the declaration shape carries what a per-resource cloud driver needs, and the
      compiled `/cf` driver stops being the way that service is reached (concern
      `cf-live-203090-unimplemented-cf-and`, rescoped 2026-07-16; its live round hands over to the
      live backlog). REST resource maps are **already declared** via the view/map → `resources` lift
      (`declared_driver.rs:233`) and are not part of this item.
      **Ticked 2026-07-19 with a documented, honest exception (Stage 5+6, commits `cfbe732` + the
      Stage-6 commit).** The DECLARABLE Cloudflare surface now comes from the committed
      `cloudflare.qfs` declaration: D1's relational surface from a `CREATE SQL … TABLES(…)` arm (no
      `introspect_d1` at mount), KV get/put/list and queue push as declared REST views/maps. Compiled
      `/cf` no longer discovers or serves any of them. **Two surfaces remain compiled and are NOT
      declared — queue PULL (Cloudflare pull is a POST-to-read; plain declared REST has no
      read-over-POST primitive) and Artifacts (a git-repo surface, not a REST resource).** These are
      structurally non-declarable today, so per this mission's own §13 ratchet framing (compiled
      remains until a script twin is possible) they legitimately stay compiled — the same honest
      tiering that leaves `/claude` compiled, not a violation. The compiled `/cf` is now a MINIMAL
      queue-pull + artifacts fallback. Recorded in the code, the cookbook, and the changelog below.
- [x] **`sql`/`git` move onto the `path_binding` registry** (#20260718203327-sql-git-path-binding-only-and-type-roundtrips.md), and declared-path column-type coverage
      broadens to NUMERIC / TIMESTAMP / UUID / JSON round-trips
      (concern `postgres-mysql-declarations-for-the-declared`)
- [x] **A re-install heals every declaration row kind** — `driver`, `view`, and `map` lookups get the
      same replace-on-install (preferred) or newest-wins semantic the `type` lookup already has, so
      re-running a declaration file is idempotent rather than append-only
      (concern `duplicate-declaration-rows-still-resolve-oldest`; shipped as **both**, `3bc2710` —
      installs replace on `(kind, name, verb)` in the audited transaction, and reads resolve newest
      per key so append-era registries heal without a re-install)
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
- [x] **Config writes are ledger-transactional because the declarative tables live beside the
      ledger** — `path_binding` and `connection_consent` re-home into the System DB, the Project DB
      becomes the vault proper, and CONNECT/DISCONNECT/account writes land audit + DDL event in one
      transaction like every other config write
      (`20260716143641-rehome-declarative-tables-into-the-system-db.md`). (Ruled 2026-07-16,
      superseding this item's original text: the concern's two bridging options — a second
      Project-DB chain, or a cross-store envelope — were both declined. A second chain forks the
      config history across two WAL files with no total order; an envelope builds backfill
      machinery the re-homing retires. WAL rules out a shared cross-file transaction, and both
      mis-homed tables declare "never a secret" in their own schema headers — the boundary was
      drawn in the wrong place, so it moves. Concern
      `project-db-configuration-events-are-not`'s How-to-Fix is superseded accordingly.)
- [x] **The declared-secrets adapter carries the OAuth app** (#20260718203328-declared-secrets-adapter-carries-oauth-app.md), closing the declared-model follow-up
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
- 2026-07-16 — Gate demoted from `documentation` to none (owner directive: thin at the start,
  revised as the tickets run). `/guide/connect` is hand-written prose, so a docs gate over it checks
  that **someone wrote the right words**, not that a cloud mount actually binds from a committed
  declaration — and `gate.sh` resolves no port for this mission anyway, so nothing could have been
  driven. `gate_target`/`gate_assert` stay as the north star. This is not a loss: item 5 shipped
  today verified by its **ticket's** gate — the `--`-bearing TMPDIR reproduction, the three
  binary reproductions, and both loaders agreeing — and the mission-level docs assertion played no
  part in it. A ticket's gate is written after reading the source; a mission's is written from a
  summary, which is the same reason three of the seven items above were wrong.
- 2026-07-16 — ticket archived — 20260716005029-unify-the-qfs-statement-splitter.md
- 2026-07-16 — ticket archived — 20260716120200-reinstall-replaces-a-declaration.md
- 2026-07-16 — **Item 6 ruled: re-draw the boundary instead of bridging it** (design brief, owner
  choice C over a second Project-DB chain and a cross-store envelope). The investigation corrected
  the record twice more: `qfs dump` does NOT miss bindings (`dump.rs:82` emits them — the earlier
  claim was wrong; the real gaps are the ledger, dump's missing accounts section, and restore's
  eventless binding replay), and WAL mode rules out a shared cross-file transaction outright, so
  "cannot share one transaction" is a hard fact, not a shortcut. Both mis-homed tables carry
  "never a secret" in their own schema headers; the ruling's boundary principle — one file holds
  secret material, the other holds everything declarative plus the ledger — goes to the blueprint
  when the ticket ships. Sequencing note recorded: items 1 and 3 both write `path_binding`, so
  neither starts before the re-homing lands. Ticket:
  `20260716143641-rehome-declarative-tables-into-the-system-db.md`.
- 2026-07-16 — concern resolved (unstuck) — duplicate-declaration-rows-still-resolve-oldest.md
- 2026-07-16 — concern resolved (unstuck) — the-config-comment-stripper-truncates-paths.md
- 2026-07-16 — story reported — work-20260715-205333.md
- 2026-07-16 — concern deferred (stuck) — create-account-s-secret-reference-form.md
- 2026-07-16 — concern deferred (stuck) — hard-break-bare-paths-can-no.md
- 2026-07-16 — concern deferred (stuck) — append-era-duplicate-rows-persist-on.md
- 2026-07-16 — concern deferred (stuck) — live-chatwork-behavior-change-awaits-owner.md
- 2026-07-16 — ticket archived — 20260716143641-rehome-declarative-tables-into-the-system-db.md
- 2026-07-16 — ticket archived — 20260716144816-RESUME-report-and-ship-work-20260715-205333.md
- 2026-07-16 — concern resolved (unstuck) — project-db-configuration-events-are-not.md
- 2026-07-16 — story reported — work-20260716-152000.md
- 2026-07-16 — concern deferred (stuck) — the-dead-project-db-config-tables.md
- 2026-07-16 — concern deferred (stuck) — shared-connection-and-broker-connection-homing.md
- 2026-07-16 — concern deferred (stuck) — the-operator-s-live-box-runs.md
- 2026-07-16 — acceptance ticked manually (tick-acceptance.sh misses a ticket filename on a continuation line) — 20260716143641-rehome-declarative-tables-into-the-system-db.md
- 2026-07-17 — `/claude` named as the second compiled counter-example beside `/cf`, with blueprint
  §13's ratchet framing as what governs both (Goal section). Requested by the claude-code-sessions
  mission's acceptance item 7 (ticket `20260717010700-claude-compiled-standing-recorded.md`) — the
  only integration between the two missions the evidence supports. No code change.
- 2026-07-18 — **Replan: the four unchecked items ticketed and driving authorized** (`/monitor`
  interrogation, AskUserQuestion). Three owner design rulings, baked into the tickets as settled:
  1. **Item 1 — account SECRET is resolved at use.** The `CREATE ACCOUNT … SECRET '<ref>'`
     reference is stored on the System-DB `connection_consent` row (a new append-only migration;
     #17 is frozen) and resolved lazily at request-build via `networked_credential`, exactly as a
     mount's `CONNECT … SECRET` already behaves — re-reading the declaration heals state, rotation
     is an env change. Sealing-on-apply and a separate accounts table were both declined.
  2. **Item 2 — declared sql-resource arm.** `/cf`'s D1 relational surface moves by growing the
     declaration shape so a resource declares a sqlite-dialect SQL endpoint over a REST verb,
     lifting onto the existing driver-sql/driver-cf planner; KV/queues ship as plain declared REST
     first inside the same ticket. This is the only option under which compiled `/cf` stops being
     the way the service is reached without regressing the relational surface.
  3. **Item 3 — `CREATE CONNECTION` retired.** With the `connections.qfs` loader deleted,
     `CONNECT /sql/<name> TO …` is the one declaration statement; `CREATE CONNECTION` becomes a
     parse error pointing at it, and `--import-env` re-emits `CONNECT` statements. Keeping two
     spellings of one registry row was the fork this mission exists to close (pre-release,
     no-backward-compat).
  Four tickets emitted (`todo/a-qmu-jp/20260718203325`–`203328`) for the four unchecked items,
  each stamped `mission:` with pre-answered `## Policies`/`## Quality Gate`, ordered by
  `depends_on` (item 2 depends on item 1's declared-account token path; items 1/3/4 independent —
  the item-6 re-homing already landed, so the path_binding gate is open). `## Experience` written;
  each acceptance item now links its ticket by `(#…)`. `drive_authorized: true` stamped — every
  judgement call about these exact tickets is answered, and each ticket lands hermetic-first with
  its live round handed to the owner-attended backlog.
- 2026-07-18 — ticket added — 20260718203325-create-account-secret-ref-bind-time-resolution.md
- 2026-07-18 — ticket added — 20260718203326-cf-surface-from-committed-declaration.md
- 2026-07-18 — ticket added — 20260718203327-sql-git-path-binding-only-and-type-roundtrips.md
- 2026-07-18 — ticket added — 20260718203328-declared-secrets-adapter-carries-oauth-app.md
- 2026-07-18 — mission replanned — declared-drivers-cloud-and-cf-and-sqlgit-and-oauth
- 2026-07-18 — ticket archived — 20260718203325-create-account-secret-ref-bind-time-resolution.md
- 2026-07-18 — ticket archived — 20260718203328-declared-secrets-adapter-carries-oauth-app.md
- 2026-07-18 — ticket archived — 20260718203327-sql-git-path-binding-only-and-type-roundtrips.md
- 2026-07-19 — ticket archived — 20260719004500-cf-declared-d1-mount-shape-owner-ruling.md
- 2026-07-19 — **Item 2 done, with a documented honest exception (Stage 5 + Stage 6, `/monitor`
  wave-8 leaf).** Stage 5 (`cfbe732`) demoted compiled `/cf` to a minimal queue-pull + artifacts
  fallback: the `driver_from_backend_with_artifact_sealer` D1/KV discovery and `introspect_d1`/
  `introspect_d1_columns` are deleted, and `cred_free_cf_registry` now represents only queue +
  artifacts. `CfDriver`/`CfReadDriver`/`cf_apply_driver` stay — the declared twin reuses them. The
  Stage-4 equivalence twin fired the §13 ratchet (green at `f1bd5f3`) and retired with the compiled
  D1 path it was gating; the declared D1 path keeps its standalone no-network + no-introspection
  tests. Stage 6 grew `cloudflare.qfs` with a `CREATE SQL /cloudflare/d1/{database}` D1 arm, updated
  the `qfs-cloudflare` cookbook to teach the new split, regenerated the skill, and bumped the four
  plugin `version` fields 0.13.0 → **0.14.0** (a taught-surface break: the skill previously taught
  "use `/cf` for D1/KV/Queues") and the binary patch 0.0.79 → **0.0.80**. The full workspace test
  also surfaced (and this leaf fixed) a **latent pre-existing** `dep_direction` failure — the
  test-only `qfs-http-core` dev-dependency added in Stage 4 was never in the `qfs-cmd` allowlist
  because prior leaves ran only per-crate; extended the allowlist (dev-only, never in the shipped
  binary). **The honest exception (recorded here, in the ticket, in the code, and in the cookbook):**
  queue PULL and Artifacts remain compiled — pull is a POST-to-read with no declared REST primitive,
  Artifacts is a git-repo surface, not REST. Both are structurally non-declarable today, so per this
  mission's §13 ratchet framing they legitimately stay compiled (the same tiering that leaves
  `/claude` compiled), not a violation. Item 2 ticked. Gates: `cargo test --workspace` exit 0, 2582
  passed 0 failed; clippy/fmt clean; gen-docs/gen-skills `--check` in sync; check-migrations clean.
- 2026-07-19 — ticket archived — 20260718203326-cf-surface-from-committed-declaration.md
- 2026-07-22 — mission achieved — mission.md
- 2026-07-22 — drive authorization revoked on archive - an achieved mission must not keep authorizing its leftover queue tickets for unattended runs (audit finding, 2026-07-22) — mission.md
