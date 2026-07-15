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
2. **The declaration shape is too thin for the remaining drivers.** `/cf` and `/rest` are reachable,
   cred-free planning/describe **placeholder** mounts; their per-resource config (which D1/KV/queues;
   which REST resource maps) needs a richer declaration than `(driver, locator, secret)` carries.
3. **`sql`/`git` never moved onto the `path_binding` registry** — they still ride the older
   declared-connection seam (a documented CONNECT-epic follow-up), and NUMERIC/TIMESTAMP/UUID/JSON
   column round-trips are not covered.
4. **Re-installing a declaration does not heal it.** Repeated `qfs run -f <driver>.qfs` *appends*
   `sys_drivers` rows. Only the `type` lookup went newest-wins; duplicate `driver` and `view`/`map`
   rows still resolve **oldest-first**, so a re-install silently keeps the stale row.
5. **The declaration file's own parser has a defect**: the `--` comment stripper truncates paths
   containing `--`.
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
- [ ] **A richer per-resource connection declaration is designed and shipped**, and `/cf` (D1 / KV /
      queues) and `/rest` (resource maps) stop being placeholder mounts on the back of it
      (concern `cf-live-203090-unimplemented-cf-and`; its live round hands over to the live backlog)
- [ ] **`sql`/`git` move onto the `path_binding` registry**, and declared-path column-type coverage
      broadens to NUMERIC / TIMESTAMP / UUID / JSON round-trips
      (concern `postgres-mysql-declarations-for-the-declared`)
- [ ] **A re-install heals every declaration row kind** — `driver`, `view`, and `map` lookups get the
      same replace-on-install (preferred) or newest-wins semantic the `type` lookup already has, so
      re-running a declaration file is idempotent rather than append-only
      (concern `duplicate-declaration-rows-still-resolve-oldest`)
- [ ] **The connections config parser stops truncating paths containing `--`**
      (concern `the-config-comment-stripper-truncates-paths`)
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
