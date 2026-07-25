---
created_at: 2026-07-23T02:00:55+09:00
author: a@qmu.jp
type: bugfix
layer: [Domain, Infrastructure]
effort: 2h
commit_hash:
category: Changed
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

## Final Report — priority 1 landed at the engine seam (2026-07-25)

**What was actually broken.** The gdrive driver already computed the right answer and threw it away.
`query::build_query` splits a `WHERE` into the part Drive's `q` expresses *exactly* (`name =`,
`mimeType =`, `trashed =`, a parent scope) and a **truthful residual** — everything else, `id == …`
above all. `read::list_children` built that residual and then dropped the binding on the floor; the
read facet did not apply it either. So the listing came back unfiltered, and — this is the part that
made it a wrong answer rather than a slow one — the planner leaves **no local `Filter`** in the plan
when a predicate is pushed, so nothing downstream re-checked it. `describe`'s `where_: true` was not
the lie; the missing enforcement was.

**The fix is one seam, not one driver.** `qfs-exec`'s read executor now re-applies
`scan.pushed.filter` over whatever a facet returns (`exec.rs` step 2b). The pushed predicate is a
**narrowing hint**, never a delegation of correctness: a facet that ignores it merely over-returns,
which the seam already permitted and the engine now corrects. That closes the class for every
compiled, declared, and future driver at once, rather than adding a call each new facet can forget.
It also makes `read.rs`'s own long-standing doc true — it always claimed the executor re-applied the
residual; it never did.

**The opt-out, and why it has to exist.** `ReadDriver::honors_pushed_filter()` defaults to `false`
(the guarantee). Two kinds of facet cannot be re-checked from outside, and both now say so and
enforce the predicate themselves:

| facet | why the executor cannot re-check it | what it does instead |
| --- | --- | --- |
| `/sql`, `/cf` D1 | narrows the batch to the pushed **projection** — `where secret == 1 \|> select name` leaves no `secret` column to compare | applies the SQL compiler's truthful residual **before** narrowing |
| `/ga` | the GA4 report returns exactly the requested dimensions/metrics, and adding the filtered-on dimension would change the report's **aggregation granularity**, not just its columns | applies GA's residual |
| `/mail` | a Gmail `WHERE` may name **search pseudo-columns** the message schema does not carry (`label`, `is_unread`), and a `date` bound is compared against the driver's **coerced** epoch-ms literal, not the date string the caller wrote | now applies `query::unpushed_residual` (it was computed and discarded, exactly like gdrive) |
| `/drive` | same pseudo-column problem (`text`/`full_text` → `fullText contains`, `parent` → `'<id>' in parents`) | now applies `query::unpushed_residual` — **this is the reported defect's fix** |
| `/s3`, `/r2` | (already applied its `plan_ls` residual; now declares it) | unchanged |

`/github` and `/slack` keep their eager facet-side filter (every predicate column there is a real,
described column) **and** get the executor's backstop — the round-3 `/users` defect is now closed
twice over, once by a call and once by a seam that cannot be forgotten.

**A second live instance of the same defect, found and fixed in passing.** `/cf`'s KV-namespace,
KV-key and queue-tail branches took the pushed `WHERE` and never applied it — `kv_list_keys` and
`queue_tail` accept only a cap — and then narrowed to the pushed projection. Same silent drop,
different backend. All three now apply the predicate before narrowing.

### Quality Gate

1. **The defect is gone, both directions** — `read_facets::drive_where_on_an_absent_id_returns_no_rows_not_the_unfiltered_listing`
   drives `DriveReadDriver` over `MockDriveClient` seeded with a two-file listing:
   `where id == 'no-such-file-id'` → **0 rows** (it returned the complete listing before);
   `where id == 'f2'` → **exactly `["f2"]`**; no predicate → **`["f1","f2"]`** untouched.
   The generic seam is pinned end-to-end by
   `oneshot::pushed_filter_enforcement::an_ignored_pushed_predicate_is_re_applied_by_the_executor`
   over a source that declares `where_: true` and ignores the predicate. **Demonstrated in both
   directions by running it against the un-enforced code**: `left: 3, right: 0` — the whole listing —
   then green with the seam in place.
2. **Local fallback is general** — enumerated in the table above. For `/drive` specifically:
   `name ==` (exact) pushes with no residual and its rows are kept (pinned by
   `drive_pushes_the_exact_name_term_and_keeps_the_rows_it_returns`, which also asserts the term
   reached Drive's `q`); `mime_type ==`, `trashed ==` and the parent scope likewise; `id ==`,
   `LIKE`, `~`, `OR`, `NOT`, `IN`, `BETWEEN`, the `modifiedTime` bound and every unmapped column are
   residual and now filtered locally. Nothing is refused — every shape has a local answer, so the
   "structured refusal" alternative in the ticket was not needed.
3. **`describe` is truthful** — `where_: true` is unchanged and now honest by construction: the flag
   declares a *native narrowing ability* (Drive really does push `name =`/`mimeType =`), and it can
   no longer advertise a pushdown the engine silently works around, because the engine enforces the
   predicate regardless. This is now written into `PushdownProfile`'s own doc so the next driver
   author reads the contract, not the inference.
4. **Id-lookup** — out of scope by the mission ruling (deferred to the drive twin); not attempted.
5. **Workspace gates** — `cargo test --workspace` green (raw `RAW_EXIT=0`, no pipe); `fmt`, `clippy`,
   `gen-docs --check`, `gen-skills --check` run on the branch gate. Patch bumped 0.0.89 → 0.0.90.

**Not fixed, and deliberately so:** `read_rows` still returns only the rows, with the residual
recomputed by the facet through `query::unpushed_residual` (a pure function of the predicate alone —
the parent/label scope only ever contributes an *exact* term). Threading a residual back out of
`read_rows` would have changed a signature every caller and test shares for no behavioural gain.
