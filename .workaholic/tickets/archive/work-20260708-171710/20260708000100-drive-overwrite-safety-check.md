---
created_at: 2026-07-08T00:01:00+09:00
author: a@qmu.jp
type: enhancement
layer: [Infrastructure, Domain]
effort:
commit_hash: 852c2f9
category: Changed
depends_on:
mission:
---

# Guard against silent Drive overwrite on an inferred copy

## Overview

`UPSERT INTO /drive/my/<name> …` converges to a **content replace** when a file already exists at
that path (this is the correct retry-safe blob semantics). But when an agent workflow copies a file
whose identity it *inferred* (e.g. "the latest Slack file"), that convergence silently replaces an
existing same-named Drive file with different bytes. A prior session did exactly this: it overwrote a
Drive file with an older Slack file because it inferred "latest" wrongly (see
`20260707175424-resume-qfs-slack-drive-safety.md`). The write reported success; the data was lost.

The reversible-UPSERT byte-integrity gap (a byteless upload) is now fixed
(`20260707181404`, shipped as fail-closed). This ticket addresses the *other* half: a byte-carrying
UPSERT that silently REPLACES an existing Drive object the operator did not mean to replace.

## Overview of the intended behavior

When a copy targets a `/drive` path that already holds a **different** file, the workflow should not
silently replace it. Options to weigh:

- surface the existing target's metadata (name, id, size, modified) in the PREVIEW so an agent/operator
  sees a replace is about to happen, and
- require an explicit replace intent (a flag column, or a distinct verb) for a path-addressed UPSERT
  that resolves to an existing file, so the default is create-or-refuse rather than create-or-replace.

This is a design decision (blueprint §7/§8 safety) — write the design brief before coding.

## Policies

- `workaholic:safety` — a live cloud write that replaces existing data requires conservative,
  explicit intent; the default must not destroy an operator's file on an inferred match.
- `workaholic:design` — the PREVIEW must make a replace legible before COMMIT.
- `workaholic:implementation` / `policies/directory-structure.md` — keep the guard in the Drive
  driver decode/preview seam and the host planning layer that owns UPSERT convergence.

## Key Files

- `packages/qfs/crates/driver-gdrive/src/effect.rs` — `decode_upsert` convergence to `Update`.
- `packages/qfs/crates/driver-gdrive/src/applier.rs` — the apply leg / preview surface.
- host planning + preview rendering — where the existing-target metadata would surface.

## Implementation Steps

1. Write the design brief: create-or-refuse vs create-or-replace default; how replace intent is
   expressed; what the PREVIEW shows for an existing target.
2. Implement the chosen guard in the Drive decode/preview seam.
3. Cover with hermetic tests: an UPSERT onto an existing file without replace intent is refused (or
   clearly flagged in preview); with intent it replaces; a create onto an empty path is unaffected.
4. Document the behavior in the Drive cookbook once settled.

## Quality Gate

- A path-addressed UPSERT that resolves to an existing different file cannot silently replace it
  without explicit intent, proven by a mock/hermetic test.
- A fresh create (no existing target) and an explicit replace both still work.
- `cargo test -p qfs-driver-gdrive`.

---

## Design brief (for owner decision — written 2026-07-08, drive work-20260708-171710)

This ticket asked for a design brief before coding. Below is the analysis, options, and a
recommendation. **No code was written; the owner decides the axes marked "OWNER" before implementation.**

### Current behavior (grounded in `crates/driver-gdrive/src/effect.rs`)

- `INSERT INTO /drive/...` → `DriveEffect::Upload` — a fresh file under the resolved parent. Drive
  permits duplicate names, so an INSERT onto an already-taken name creates a **second** file; it
  never refuses and never replaces.
- `UPSERT INTO /drive/...` → `DriveEffect::Update` (content replace) when a `file_id` is given OR the
  path resolves to an existing file (`WriteResolver::existing`); otherwise `Upload` (create).
- The replace-vs-create branch is decided by the **live** resolver at COMMIT. The pure `NoResolve`
  used at PREVIEW returns `existing() == None`, so an UPSERT that will replace at COMMIT is decoded
  as **create** at PREVIEW. **PREVIEW therefore does not reveal an impending replace** — that is the
  "silent" in silent replace.
- The byteless-upload half is already fail-closed (ticket 20260707181404, commit `8130417`).

### The danger

An agent copies an **inferred** file identity (e.g. "the latest Slack file") and UPSERTs to
`/drive/my/<name>`. If that path already holds a **different** file, COMMIT replaces it. qfs reports
success; the operator was never told a replace happened. (Drive retains prior revisions, but nothing
surfaced the overwrite.)

### Constraints and tensions

1. **UPSERT idempotency.** Re-running the *same* UPSERT must converge (replace) — the retry-safe
   property this ticket itself calls correct. A blanket "refuse when the target exists" default
   would break it.
2. **Drive names are not unique.** `/drive/my/report.pdf` may resolve to **0, 1, or many** files.
   "Does the target exist?" is ambiguous; any guard must define multi-match behavior.
3. **PREVIEW purity.** qfs previews touch nothing. Revealing the existing target needs a live
   metadata GET at plan/preview time — a read (no mutation) but still a network call. Whether that
   fits the "preview touches nothing" contract is an owner call.
4. **Only two universal write verbs.** INSERT and UPSERT are the whole surface; there is no room for
   a third "create-only" verb without redefining one of them.

### Options

**Axis 1 — default behavior on an existing target:**

- **A1 create-or-replace (status quo):** silent replace. This is the danger; rejected.
- **A2 create-or-refuse:** refuse on an existing target unless explicit replace intent. Fail-closed,
  but breaks UPSERT idempotency (tension 1).
- **A3 create-or-replace, made legible:** keep replace semantics but have PREVIEW surface the
  existing target's metadata and label the plan **REPLACE** (vs CREATE). Preserves idempotency;
  removes "silent." Needs a preview-time GET (tension 3).

**Axis 2 — how replace intent is expressed:**

- **B1 verb distinction:** redefine `INSERT` to `/drive` as **create-only** (refuse with
  `target_exists` when the name already resolves to a file) and keep `UPSERT` as create-or-replace.
  Reuses the existing grammar; gives agents a safe "put a new file, fail if taken" verb and an
  explicit replace verb. Changes INSERT's current create-a-duplicate behavior (arguably itself an
  improvement — silently creating a same-named duplicate is surprising).
- **B2 a replace modifier on UPSERT** (an `on_conflict replace` clause or a `replace=true` flag
  column): explicit per statement; adds grammar/surface.
- **B3 content-hash guard** (replace only when the new md5 differs): a differing hash is still a
  legitimate replace, so this does not distinguish intended from unintended; noted and rejected.

**Axis 3 — multi-match name policy (OWNER):** when a name resolves to many files, `existing()` must
choose: **refuse** (ambiguous — address by id), replace-newest, or replace-all. Refuse is the
conservative default and doubles as overwrite protection.

### Recommendation

Adopt **A3 (legible preview) as the baseline, plus B1 (INSERT = create-only) as the intent
mechanism**:

1. Run `existing()` at PLAN/preview time so the PREVIEW effect shows **CREATE vs REPLACE** and, on a
   replace, the existing target's `id`/`name`/`size`/`modified`. Document it as a preview-time
   metadata read — a pure GET, no mutation — the one sanctioned preview network read, mirroring how a
   planned source read is shown. If the owner rejects any preview network call, fall back to a static
   "MAY replace an existing file" label and surface the target metadata at the typed-COMMIT
   confirmation instead.
2. Redefine `INSERT` to `/drive` as **create-only**: refuse with a structured `target_exists` error
   when the name already resolves to a file, so an agent that means "place a new file" fails closed
   on a collision instead of silently duplicating or replacing. `UPSERT` stays the explicit
   create-or-replace (retry-safe) verb.
3. On a multi-match name, refuse with a structured `ambiguous_target` error (the match count + "address
   by file id").
4. `cp` shell alias: keep `cp` → UPSERT (Unix `cp` overwrites), but the cookbook should steer
   inferred-identity copies to `INSERT` (create-only) so a wrong-target collision fails closed.

This satisfies tension 1 (UPSERT still converges), gives a fail-closed create path (INSERT), makes
replaces legible (A3), and resolves name ambiguity conservatively (refuse).

### Open questions for the owner

1. Is a preview-time metadata GET acceptable within qfs's "preview touches nothing" contract? (If
   not, use the confirmation-step fallback in recommendation 1.)
2. Redefine `INSERT` as create-only (a hard break from create-duplicate)? Acceptable under the
   experimental / no-backward-compat policy, but confirm the intent.
3. Multi-match name policy: refuse (recommended) vs replace-newest?
4. Should `cp` map to `INSERT` (safe, create-only) instead of `UPSERT` (overwrites)? Trade-off:
   Unix-`cp` familiarity vs safe-by-default.

### Test obligations (when implemented)

- UPSERT onto an existing different file: PREVIEW labels REPLACE and shows the target metadata;
  COMMIT replaces.
- INSERT onto an existing name: refused (`target_exists`), fail-closed, no duplicate created.
- INSERT/UPSERT onto a free path: creates.
- Multi-match name: refused (`ambiguous_target`) with the count.
- All hermetic (`MockDriveClient`); the existing byteless fail-closed stays green.
