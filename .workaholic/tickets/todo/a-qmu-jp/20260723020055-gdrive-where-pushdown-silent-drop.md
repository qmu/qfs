---
created_at: 2026-07-23T02:00:55+09:00
author: a@qmu.jp
type: bugfix
layer: [Domain, Infrastructure]
effort:
commit_hash:
category:
depends_on:
mission: a-where-predicate-is-honored-or-refused-never-dropped
---

# gdrive: `where` predicate silently dropped on folder listings; no id-based lookup for Drive share links

## Overview

Two related gaps in the `gdrive` driver, hit while trying to read a single Google Drive file
whose only known identifier was the **file id** from a standard share link
(`https://drive.google.com/file/d/<fileId>/view`):

1. **Correctness bug (primary):** a `where` predicate on a `/drive` folder listing was
   **silently ignored** — the query returned the complete unfiltered listing instead of the
   filtered rows, at exit 0. A predicate the driver cannot push down must be evaluated
   locally over the listed rows, or refused with a structured "unsupported predicate" error.
   It must never be dropped while the query still answers.
2. **Missing capability:** there is no id-based addressing anywhere in the `/drive` path
   model (`child_address` is `entry_name`), and no corpus-wide search — so a Drive file id
   cannot be resolved to a path from inside qfs at all. The share-link → file task cannot be
   completed without leaving qfs.

## Observed (CLI, driver `gdrive`, mount `/drive`)

`describe` advertises the predicate as supported:

```
$ qfs describe /drive/my
# columns include: id, name, mime_type, size, ...
# pushdown: where_: true
```

But filtering on `id` returns the **whole unfiltered listing**:

```
$ qfs run "/drive/my |> where id == '<fileId>' |> select id, name, mime_type, size"
# → 8 rows: the complete My Drive root listing, NONE with a matching id
# expected: 0 or 1 rows
```

The predicate had no effect on the result. This is worse than an error: the consumer
receives a plausible relation that reads as "these rows matched", and every downstream
step (scripts branching on row_count, agents in the describe→preview→commit loop) is
corrupted silently.

Control — once the human-readable path was known (resolved **outside** qfs via a separate
Drive API tool: id → parent chain → names), the single-file read worked perfectly:

```
$ qfs run "/drive/shared/<DriveName>/<Folder>/<file>.pdf |> select id, name, mime_type, size, md5"
# → 1 row, correct id and md5
```

So the driver's per-entry surface is fine; the defect is confined to predicate handling on
listings, and the capability gap is the absence of any id → path resolution.

## Requested fixes, in priority order

1. **Never silently drop a `where` predicate.** If the gdrive driver cannot push a
   predicate down to the Drive API, either (a) evaluate it locally over the listed rows in
   the engine — the pushdown planner already has a local filter path — or (b) fail the
   query with a structured "unsupported predicate" error naming the predicate and the
   driver. Also make `describe`'s `where_: true` honest: it must not advertise pushdown the
   driver then ignores.
2. **Support id-based lookup.** Drive share links carry only the file id, so this is the
   natural entry point for "read this file someone shared with me". Either:
   - push `where id == '<fileId>'` down to a Drive `files.get` (exact-match fast path), or
   - add an id path coordinate, e.g. `/drive/by-id/<fileId>`, resolving to the same
     folder/file/blob surface as a name path.
   Either shape resolves the share-link task in one statement.
3. **(Nice to have) corpus-wide search** backed by Drive `files.list` with a `q=` search,
   so `name`/`id` predicates can match beyond a single folder listing rather than only
   within one directory's children.

## Policies

- implementation/honest-surfaces — `describe` reporting `where_: true` while the driver
  drops the predicate is a dishonest surface; the declaration and the behavior must agree.
- workaholic:design — 「推測するな、宣言して拒否せよ」: a predicate the driver cannot honor
  must be refused (or honored locally), never answered with an unfiltered relation that
  reads as a fact about the data.
- workaholic:safety — a wrong answer at exit 0 is consumed as a right one; an unfiltered
  listing returned for a filtered query leaks sibling rows the caller never asked to see.

## Quality Gate

1. **The defect is gone, both directions.** `/drive/<folder> |> where id == '<absent>'`
   returns 0 rows (or a structured "unsupported predicate" error at non-zero exit) — never
   the unfiltered listing. A test that fails on the current behavior and passes after.
2. **Local fallback is general.** Whichever predicate shapes gdrive does not push down
   (`==` on `id`, `name`, `mime_type`; `like`; `in`; …) are either evaluated locally or
   refused — enumerate what each does after the change.
3. **`describe` is truthful.** The pushdown flags reported for `/drive` listings match what
   the driver actually pushes down after the change.
4. If id-lookup lands: `where id == '<fileId>'` (or `/drive/by-id/<fileId>`) over a
   fixture account returns exactly the one matching row, and a nonexistent id returns 0
   rows / not-found — not a listing.
5. Workspace gates green: `cargo fmt --all --check`, `cargo clippy --workspace
   --all-targets -- -D warnings`, `cargo test --workspace`, `cargo run -p xtask --
   gen-docs --check` if a describable surface moved.

## Considerations

- This is the driver-level sibling of
  `20260717180100-where-on-an-unknown-column-returns-zero-rows-at-exit-0.md` (todo): that
  ticket is about an unknown column silently matching nothing; this one is about a **known,
  described column** whose predicate silently matches **everything**. Same family — the
  engine/driver seam lets a `where` mean nothing — opposite failure direction, and this
  direction additionally over-discloses rows. If one fix lands at the pushdown-planner
  seam (unpushed predicates always get a local `Filter`), it likely closes both.
- Priority 1 stands alone and should not wait for the id-lookup design; it is a small,
  general correctness fix. Priorities 2–3 are gdrive feature work and can land separately.
- For priority 2, the `files.get` pushdown and the `/drive/by-id/` coordinate are not
  exclusive; the coordinate has the advantage of also giving writes and blob reads an
  id-addressed path.
- **Mission ruling (2026-07-24, developer):** this mission takes **priority 1 only** (the
  engine-seam guarantee: unpushed predicates get a local Filter or a structured refusal, and
  describe stays truthful). Priority 2 (id-based lookup / `/drive/by-id`) and priority 3
  (corpus-wide search) are **deferred to the drive twin mission** (blueprint §13.3 #3), whose G4
  path→id machinery is where they land without double work.
