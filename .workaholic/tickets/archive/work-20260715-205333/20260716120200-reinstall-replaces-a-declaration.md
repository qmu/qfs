---
created_at: 2026-07-16T12:02:00+09:00
author: a@qmu.jp
type: bugfix
layer: [Domain]
effort:
commit_hash:
category: Changed
depends_on:
mission: declared-drivers-are-the-normal-way-to-add-a-service
---

# Re-installing a declaration replaces it: upsert-on-key installs, newest-wins reads

## Overview

The mission's Goal says **"re-reading the declaration is what heals state"**. Today a re-install
appends rows and every lookup except `type` resolves the **oldest** one, so the stale declaration
keeps winning while the re-install reports success.

This is not latent. Measured on the operator's own registry (2026-07-16, shipped binary + system
DB): 30 rows, **14 of them duplicates** — `chatwork` ×2, `/chatwork/rooms/{room}/messages` ×4
(= two installs of a view + an INSERT map), `/cloudflare/zones/{zone}/dns_records` ×2, and more.
The duplicated `/chatwork/rooms/{room}/messages` view and map bodies **differ** between installs
(id 5 vs 21, id 6 vs 23 — compared byte-for-byte in the DB), so someone edited `chatwork.qfs`,
re-installed it, and **the edit is not in effect**. Because `/type/chatwork/*` already resolves
newest-wins, `/chatwork` currently runs a **new type contract over an old view body** — a
combination neither install ever declared.

Mechanism, all in `packages/qfs/crates/qfs/src/declared_driver.rs`:

- `load_from_conn` (`:122-125`): `… FROM sys_drivers ORDER BY id` — ascending, oldest first.
- `assemble` (`:148-163`): every `driver` row seeds a `DeclaredDriver`, so a re-installed driver
  yields two same-name entries.
- `find_mut` (`:194-196`): `.find()` returns the first = oldest; every view/map attaches to the
  stale entry. Duplicate view/map rows are all pushed in ascending order and
  `declared_eval::view_specs` (`declared_eval.rs:10-38`) preserves that order.
- The asymmetry is one file wide: `types_from_conn` (`:526-533`, `:583-586`) uses
  `ORDER BY id DESC`, with a comment arguing exactly this ticket's case — applied only to `type`
  (PR #34).

## The ruling (owner, 2026-07-16)

**O3: installs replace, reads resolve newest-wins.** Ruled after a design brief; the alternatives
and the corrected premises are recorded here because two earlier framings of this item were wrong.

1. **The store is not append-only, and never was ruled to be.** An earlier draft called append-only
   "deliberate design" citing `type_catalog.rs:23` and the blueprint's WORM language. Verified
   false: the WORM tail is `sys_ddl_events` (a different table), and `sys_drivers` deletion is
   **already implemented and audited** — `remove_driver` (`sys.rs:645`) runs
   `DELETE FROM sys_drivers` inside one transaction with the t76 audit row and a `ddl_event`
   ("administration observes itself", `sys.rs:656-659`). It is reachable via the provisioning
   reconcile (`qfs apply`); only the ad-hoc pipeline face lacks a REMOVE verb
   (`/sys/drivers |> REMOVE` → `UnsupportedVerb, supported: ["SELECT","INSERT"]`).
2. **The concern's replace-on-install preference is implementable after all.** An earlier session
   statement that it was "not implementable" generalised the pipeline capability to the store.
   Wrong: the delete seam exists; only the install seam declines to use it.
3. **Appending is the outlier, not the norm.** `plan.rs:69-78`: `settings` and `paths` already
   apply **upsert-on-key** semantics; `drivers` alone is "install/uninstall only" with edits
   refused ("change a driver by removing + re-adding").

The decision:

- **Install replaces.** `insert_driver` (`sys.rs:478`) deletes rows matching the incoming
  **`(kind, name, verb)`** in the same transaction before inserting, and both legs land in
  `sys_ddl_events` (the INSERT already appends its event in-tx, `sys.rs:539-560`; the delete
  pattern is `remove_system_row`). The dedup key is verified against the live registry: grouping
  by it collapses every duplicate to exactly its install count with no false merges — a `view`
  and a `map` sharing a `name` stay distinct, as do two `map`s differing only in `verb`.
- **Reads resolve newest-wins**, like `type`: flip `load_from_conn` to `ORDER BY id DESC` and make
  `assemble` keep only the newest row per `(kind, name, verb)`. This is what heals the operator's
  existing 14 duplicate rows **immediately**, without waiting for each file to be re-installed —
  and keeps any historical DB correct.
- **The superseding delete is a supersede, not a destroy.** It does not require
  `--commit-irreversible`: it is the same replace-by-key that `settings`/`paths` upserts already
  perform ungated. State this in code comments; `REMOVE` in general remains inherently
  irreversible.
- **No ad-hoc REMOVE verb is added** to `/sys/drivers`. Uninstall stays the provisioning path's
  job. (Known, out of scope: `remove_driver` deletes by `name` only, so uninstalling a driver
  strands its view/map/type rows; stranded views drop fail-open at read. A bundle-aware uninstall
  is its own future item.)

## Policies

- `workaholic:implementation` / `coding-standards` and `directory-structure` — universal
  code-touching policies; the change stays inside the existing `qfs` crate seams (`sys.rs` store
  seam, `declared_driver.rs` model layer), no new modules.
- `workaholic:implementation` / `anti-corruption-structure` — the resolution rule (newest-wins per
  `(kind, name, verb)`) is model-layer logic and belongs in `declared_driver.rs::assemble`, not
  smeared across callers; the store seam owns the replace transaction.
- `workaholic:implementation` / `type-driven-design` — the identity key is a premise to gather
  onto the code (one named key, used by both the install delete and the read dedup), never a
  comment; a re-install that silently half-applies is the named anti-state (silent failure).
- `workaholic:implementation` / `test` — the tokenization table of this ticket is the
  keying/ordering behavior; unit tests own it (the `type` newest-wins test at
  `declared_driver.rs:2220` is the pattern).
- `workaholic:implementation` / `persistence` — loud failure over silent propagation: the malformed
  states this ticket removes (old body winning under new type) currently propagate silently.

## Key Files

- `packages/qfs/crates/qfs/src/sys.rs:478-560` — `insert_driver`: the install seam that gains the
  same-key delete, inside the existing transaction with audit + `ddl_event`.
- `packages/qfs/crates/qfs/src/sys.rs:645-700` — `remove_driver` / `remove_system_row`: the
  already-audited delete pattern to reuse.
- `packages/qfs/crates/qfs/src/declared_driver.rs:122-125, 148-163, 194-196` — ordering, assembly,
  and first-match resolution to flip to newest-wins keyed on `(kind, name, verb)`.
- `packages/qfs/crates/qfs/src/declared_driver.rs:526-533, 583-586, 2220` — the `type` precedent
  and its test.
- `packages/qfs/crates/qfs/src/declared_eval.rs:10-38` — `view_specs` inherits `assemble`'s order;
  no change if `assemble` dedups, but its assumption should be stated.
- `packages/qfs/crates/qfs/src/type_catalog.rs:23` — the "append-shaped" comment: rewrite it, since
  after this ticket the store converges and the comment describes the retired behavior.
- `packages/qfs/crates/provision/src/plan.rs:69-78` — the upsert-on-key precedent and the driver
  install/uninstall stance; unchanged by this ticket, but its "edits refused" note may deserve a
  follow-up once installs replace (an in-place driver edit becomes expressible).

## Quality Gate

Owner-selected framing: every claim proven in **both directions** where a defect is being fixed.

1. **A re-installed driver's new locator takes effect.** `CREATE DRIVER d AT 'first' …` then
   `CREATE DRIVER d AT 'second' …`: the assembled driver reports `second`. Write the test first
   against the current code and watch it fail (it reports `first` today).
2. **Storage converges.** After the second install, exactly **one** row per `(kind, name, verb)`
   remains in `sys_drivers`, and `sys_ddl_events` records both legs (the DELETE and the INSERT).
3. **Keying is exact.** A `view` and a `map` on the same path do not collapse into each other; two
   `map`s on one path with different verbs both survive; two installs of one view keep only the
   newer body.
4. **Pre-existing duplicates resolve newest without re-install** — seed a DB with duplicate rows
   the way the operator's registry has them (ascending ids, differing bodies) and assert the newest
   body wins through `assemble`/`view_specs`.
5. **`type` stays green**: the newest-wins test at `declared_driver.rs:2220` is untouched.
6. **Hermetic, and isolated correctly.** All tests run under a temp `XDG_CONFIG_HOME` (or an
   in-memory conn) — **never `QFS_HOME`, which qfs does not read.** This gate exists because the
   discovery for this very ticket polluted the operator's real system DB by trusting `QFS_HOME`;
   the rows had to be deleted via sqlite3 with owner approval. Verify isolation with a read before
   any `--commit`.
7. Baseline (`CLAUDE.md:22-24`): `cargo test --workspace`, `cargo clippy --workspace --all-targets
   -- -D warnings` (not `--all-features`), `cargo fmt --all --check`, patch bump in
   `packages/qfs/crates/qfs/Cargo.toml`.

## Considerations

- **After this ships, `/chatwork` on the operator's box changes behavior** — the newer view body
  starts winning (that is the point). Live verification belongs to the owner-attended live backlog,
  not this ticket's gate.
- The concern (`duplicate-declaration-rows-still-resolve-oldest`) carries a stale reproduction
  (`qfs run -f <driver>.qfs`; `-f` does not exist — `qfs run` takes one statement) and a `low`
  severity that the measurement above contradicts (a live-connected service is silently running a
  superseded body). Both are inputs for whoever judges the concern at `/report`.
- Do not dedupe only at the install seam and skip the read fix, or the operator's existing rows
  stay wrong until every declaration file is re-run; do not fix only the reads, or rows accumulate
  forever and the `/sys/drivers` listing keeps showing corpses. The ruling is both, deliberately.
- `insert_driver`'s §5.4 type validation runs before the transaction; the same-key delete must not
  reorder around it (validate → tx begin → delete → insert → audit → event → commit).
