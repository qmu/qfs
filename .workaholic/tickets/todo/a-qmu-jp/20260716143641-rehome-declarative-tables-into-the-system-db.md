---
created_at: 2026-07-16T14:36:41+09:00
author: a@qmu.jp
type: enhancement
layer: [Domain, DB]
effort:
commit_hash:
category:
depends_on:
mission: declared-drivers-are-the-normal-way-to-add-a-service
---

# Re-home the declarative tables into the System DB, so config writes are ledger-transactional

## Overview

Blueprint §16 (`blueprint.md:1443`) promises that **"every applied effect lands in the hash-chained
`sys_ddl_events` WORM tail"**. It is false for the two config writes that live in the Project DB:
`CONNECT`/`DISCONNECT` (`path_binding`) and account declare/remove (`connection_consent`) emit a
best-effort `AuditEvent` (verb + path metadata, no payload) **after** their write commits, and
never a `DdlEvent` — `sys.rs:456-460` and `:634-637` say so outright ("a cross-DB write cannot
share one transaction"). A crash between the write and the audit leaves a binding with no trace at
all. The config ledger cannot answer "when was this mount connected, when was this account
declared" — for any registry, ever.

Verified facts that frame the ruling:

- **A shared transaction is technically closed off.** The stores are two SQLite files, both WAL
  (`store/src/lib.rs:100-103`), and SQLite's ATTACH gives **no cross-file atomicity under WAL**.
  "Just wrap both in one transaction" is not on the table.
- **The mis-homed tables say so themselves.** `connection_consent`'s schema header reads
  "SELECTORS + METADATA ONLY — never a secret" (`schema/project_consent.sql`); `path_binding`
  carries references (`secret_ref` is an `env:`/`vault:` *reference*), never material. Two
  declarative config tables live in the credential file for historical reasons (the M4 consent
  ledger and the CONNECT epic), not by principle.
- **The record corrected**: an earlier session statement claimed `qfs dump` misses CONNECTed paths.
  Wrong — `dump.rs:82` emits `path_bindings` from the Project DB. The real gaps: dump has **no
  accounts/consent section at all** (sections: sys_drivers, settings, policies, billing,
  path_bindings); `--include-events` history contains no connection/account operations; and
  restore's own header ("a committed restore … records new local audit/DDL events") is silently
  false for bindings, because the replay goes through the eventless `insert_binding`.
- **What the two stores actually hold.** System DB: identity, sessions, policies, drivers,
  settings, transforms, billing, oauth/oidc, audit, `sys_ddl_events` — the administration plane.
  Project DB: the vault proper (`secret_store`, `vault_key_slots`, rotation, E2E) **plus** the two
  mis-homed declarative tables, plus the team-ownership registries (`shared_connections`,
  `broker_connections`). `project.db` is machine-global; "project" refers to the team-ownership
  model, not per-directory scoping.

## The ruling (owner, 2026-07-16)

**Re-draw the boundary instead of bridging it.** `path_binding` and `connection_consent` (with the
mount-coordinate and `app` columns the later migrations ALTERed onto them) move into the System
DB. The Project DB becomes the **vault proper**. Config writes then land in the same
single-DB transaction as their audit row and `DdlEvent` — exactly the `insert_driver` pattern,
including the supersede/remove legs — and the problem class dissolves rather than being
compensated.

The boundary principle this buys, worth stating in the blueprint: **one file holds secret
material; the other holds everything declarative, plus the ledger that observes it.**

Alternatives considered and declined (recorded because the concern's own How-to-Fix names them):
a second hash chain in the Project DB forks the config history permanently — two chains across two
WAL files have no total order; a cross-store envelope with a backfill sweep is §16-faithful but
builds reconciliation machinery this ruling retires. Both bridge a boundary that is simply drawn
in the wrong place.

**Sequencing is the point.** Mission items 1 (cloud account declarations) and 3 (`sql`/`git` onto
`path_binding`) both add write traffic to these tables. Landing this first means every future
config write is ledger-transactional by construction, instead of enlarging the hole first and
draining it after.

## Scope

1. **New System-DB migration** creating `path_binding` and `connection_consent` in their *current*
   shape (fold the ALTER history — mount coordinate, `app` — into the fresh CREATE; the per-DB
   migration chains stay append-only and untouched on the Project side).
2. **One-shot boot copy** (app-level, outside the per-DB migration framework, because the two
   files' migrations cannot order against each other): when the System-DB tables are empty and the
   Project-DB tables have rows, copy once; idempotent on every later boot. The old Project-DB
   tables go **dead but not dropped** in this release — dropping them is a later Project-DB
   migration once a release containing the copy has shipped. That is data-safety sequencing, not a
   compatibility period: the drop must not be able to run before the copy has.
3. **Rewire the writers** — `insert_binding` / `remove_binding` (`sys.rs:440+`),
   `record_account` / `remove_account` (`sys.rs:570+`), and their underlying
   `path_binding::db_upsert_binding` / `account::declare_account` / `account::remove_account` — to
   the System-DB connection, inside the standard transaction with `append_audit_tx` +
   `append_ddl_event_tx`. The best-effort `audit_paths_write` / `audit_accounts_write` helpers
   (`sys.rs:707-751`) are retired.
4. **Repoint every reader.** `path_binding`/`connection_consent` are read across at least:
   `path_binding.rs`, `cloud_mounts.rs`, `commit.rs` (the `cloud_bind_allowed` consent gate and
   `networked_credential`), `account.rs`, `connection.rs`, `describe.rs`, `google.rs`, `git.rs`,
   `provision.rs`, `dump.rs`, `restore.rs`, `secret_store.rs`, `declared_driver.rs`, `lib.rs`. A
   missed reader keeps reading the dead table and sees stale or empty state — this inventory is
   the ticket's main risk and the Quality Gate pins it.
5. **dump/restore follow the move**: `dump_path_bindings` reads the System DB, and dump gains the
   missing **accounts/consent section** (selectors + metadata only, never a token — the same
   secret-free discipline as every other section). Restore's binding replay now lands ledger
   events natively through the rewired seam.
6. **Docs become true**: blueprint §16's WORM sentence, and the store-boundary description,
   updated; `gen-docs --check` decides whether any rendered surface moves.

**Out of scope:** `shared_connections` / `broker_connections` homing (team-ownership registries,
M9 territory — same class, own decision later, recorded here); compacting the dead Project-DB
tables (the later drop migration); the Project-DB `secret_store` and key-slot machinery (the vault
does not move — it is what remains).

## Policies

- `workaholic:implementation` / `anti-corruption-structure` — the whole ticket is a boundary
  correction: the vault (dependency/secret plane) and the declarative model separate into the
  files that own them, and the ledger sits beside what it observes.
- `workaholic:implementation` / `directory-structure` — placement readable from structure: a
  config table in the credential file is the storage-level version of the violation this policy
  names.
- `workaholic:implementation` / `persistence` — migrations stay append-only under the checksum
  guard; the copy-then-later-drop sequencing exists so a schema change can never destroy data it
  has not copied; loud failure over silent propagation for the copy step.
- `workaholic:implementation` / `type-driven-design` — the in-transaction event append makes "this
  config write is ledgered" a structural property, not a best-effort promise in a comment.
- `workaholic:implementation` / `coding-standards` + `test` — universal code-touching policies;
  the reader-repoint inventory is pinned by tests, not by review diligence.

## Key Files

- `packages/qfs/crates/qfs/src/sys.rs:440-462, 463-490, 570-600, 707-751` — the two writers, their
  remove twins, and the best-effort audit helpers this ticket retires.
- `packages/qfs/crates/qfs/src/sys.rs:478-560, 645-700` — `insert_driver` / `remove_system_row`:
  the transactional audit+event pattern the rewired writers adopt verbatim.
- `packages/qfs/crates/qfs/src/path_binding.rs` — `db_upsert_binding` and the binding readers; the
  central seam to repoint.
- `packages/qfs/crates/qfs/src/account.rs` — `declare_account` / `remove_account` / consent reads.
- `packages/qfs/crates/qfs/src/commit.rs` — `cloud_bind_allowed` / `networked_credential`: the
  bind gate that must keep failing closed against the *new* home.
- `packages/qfs/crates/store/src/schema/` + `packages/qfs/crates/store/src/lib.rs` — the new
  System-DB migration; `SYSTEM_MIGRATIONS` list; the checksum guard context.
- `packages/qfs/crates/store/src/schema/project_consent.sql`, `project_path_bindings.sql`,
  `project_mount_coordinate.sql`, `project_google_app_labels.sql` — the shapes being folded into
  the fresh CREATE (frozen files; read, never edited).
- `packages/qfs/crates/qfs/src/dump.rs:77-95, 176+` / `restore.rs` — the moved section, the new
  accounts section, and the replay leg that starts landing events.
- `docs/blueprint.md:1443` and the store-boundary prose — the sentences this ticket makes true.

## Quality Gate

1. **The ledger holds the missing history, proven in both directions.** Before the fix: a test
   asserting that `CONNECT` (insert_binding) and account declare produce a `DdlEvent` **fails** on
   current code. After: `CONNECT`, `DISCONNECT`, account declare and account remove each land
   audit row + `DdlEvent` in **one** System-DB transaction, payloads secret-free (references and
   labels only — assert the dump-style redaction discipline).
2. **The copy is once and only once.** Seed a Project DB with binding + consent rows in the old
   shape; first boot copies them into the System DB; a second boot copies nothing; a fresh install
   (no Project rows) copies nothing. The dead tables are not read afterward: a row planted in the
   dead Project table after the copy is invisible to the bind gate and to `cloud_mounts_from`.
3. **The bind gate still fails closed across the move.** No consent row in the System DB → cloud
   bind refused, exactly as today; consent written post-move gates through the new home.
4. **dump/restore round-trip.** `qfs dump` emits path_bindings **and** the new accounts/consent
   section from the System DB, secret-free; `qfs restore --commit` of a binding record produces a
   local `DdlEvent` (the asymmetry restore's header promises but does not deliver today — write
   this test against current code first and watch it fail).
5. **No reader left behind**: the moved tables' readers all take the System-DB connection — pinned
   by the behavioral tests above plus a source-level assertion that no non-test code opens the
   Project DB for `path_binding`/`connection_consent`.
6. **Hermetic and correctly isolated**: every test under a temp `XDG_CONFIG_HOME` (never
   `QFS_HOME`, which qfs does not read — the prior ticket's pollution incident is the reason this
   line exists).
7. Baseline (`CLAUDE.md:22-27`): workspace tests, clippy (not `--all-features`), fmt,
   `gen-docs --check`, `gen-skills --check`, `check-migrations` (needs release tags; baseline is
   `v0.0.71`+ in this repo), and the patch bump on this ticket's own shipped PR.

## Considerations

- **Ship the current branch first.** `work-20260715-205333` already carries the splitter
  unification and replace-on-install; this ticket is a second large change and wants its own
  branch and PR after `/report` ships the present one.
- The concern (`project-db-configuration-events-are-not`, severity moderate) prescribes the two
  bridging options in its How-to-Fix; this ruling supersedes that prescription — whoever judges
  the concern at `/report` should resolve it against this ticket, not against its own text.
- The copy step must handle the operator's real registry (bindings and consent rows exist on this
  box, including live-connected accounts). The copy is read-only toward the Project DB; nothing
  deletes vault rows. The dead-table drop is deliberately NOT in this ticket.
- `restore` treats dumped historical events as external provenance (never imported into the local
  chain) — unchanged; only the *new* events restore records become complete.
- After this lands, mission items 1 and 3 inherit a ledger-transactional `path_binding` for free;
  neither should start before this ships.
